//! Fetching, retrying and caching shared by every remote Source.
//!
//! OpenStax and LibreTexts each grew their own copy of the same retry loop, and
//! LibreTexts grew a second copy of its own so that HTML and JSON could carry
//! different headers. The three differed only in which error variant they
//! named, which header they sent, and how they decoded the body. This module
//! keeps the loop once and lets each Source keep the three things that are
//! genuinely its own.
//!
//! ADR-0002 records that the importers do not share an entry shape. That still
//! holds: what is shared here is machinery, not the shape of an Import.

use std::future::Future;
use std::time::Duration;

use futures::{StreamExt, TryStreamExt};
use reqwest::StatusCode;
use rusqlite::OptionalExtension;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::db::connection::DbPool;
use crate::error::{AppError, AppResult};

/// How many times a request is attempted before the Source is told it failed.
const MAX_ATTEMPTS: usize = 3;

/// The delay before the second attempt; each further wait doubles it.
const BASE_BACKOFF: Duration = Duration::from_millis(500);

/// A request that failed, before a Source has named it.
///
/// The shared loop cannot construct `AppError::OpenStax` or
/// `AppError::LibreTexts` without picking a Source, and the kind string that
/// reaches the webview comes from that choice. So it reports what happened and
/// leaves the naming to the caller — see `LibreTextsClient::source_error` and
/// its OpenStax counterpart.
#[derive(Debug)]
pub enum FetchError {
    /// The server answered and the client gave up on that status — either
    /// because it is not worth retrying, or because the attempts ran out.
    Status { url: String, status: StatusCode },
    /// No response was ever produced.
    Transport(reqwest::Error),
}

/// One request, described so the retry loop can rebuild it for every attempt.
///
/// A `reqwest::RequestBuilder` cannot be relied on to clone, so the loop is
/// given the parts rather than a half-built request. This is also where the
/// only difference between the old JSON and HTML paths now lives: a header.
#[derive(Debug, Clone)]
pub struct Request {
    url: String,
    query: Vec<(String, String)>,
    headers: Vec<(&'static str, String)>,
}

impl Request {
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            query: Vec::new(),
            headers: Vec::new(),
        }
    }

    pub fn query(mut self, params: &[(&str, String)]) -> Self {
        self.query.extend(
            params
                .iter()
                .map(|(name, value)| ((*name).to_string(), value.clone())),
        );
        self
    }

    pub fn header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

/// An HTTP client that retries with exponential backoff.
#[derive(Debug, Clone)]
pub struct Fetcher {
    http: reqwest::Client,
}

impl Fetcher {
    /// `timeout` applies to each attempt, not to the sequence.
    pub fn new(timeout: Duration) -> Self {
        Self::build(timeout, None)
    }

    /// A client that sends `user_agent` on every request it makes.
    ///
    /// Per-request headers reach only the requests a Source builds itself.
    /// Image downloads -- Figures and covers -- go through `Fetcher::http` and
    /// build their own, so a Source whose host rejects an unidentified client
    /// needs the header on the client rather than on the request: otherwise the
    /// API works and every Figure and cover silently 403s away.
    pub fn with_user_agent(timeout: Duration, user_agent: &str) -> Self {
        Self::build(timeout, Some(user_agent))
    }

    fn build(timeout: Duration, user_agent: Option<&str>) -> Self {
        let mut builder = reqwest::Client::builder().timeout(timeout);
        if let Some(user_agent) = user_agent {
            builder = builder.user_agent(user_agent);
        }

        Self {
            http: builder.build().expect("valid remote HTTP client"),
        }
    }

    /// The underlying client, for work that streams rather than retries.
    ///
    /// Image downloads are the callers -- Figures, and a book's cover. Both
    /// tolerate a miss, and retrying each one three times would multiply the
    /// slowest part of an Import. Sharing the client rather than building a
    /// second one also keeps a Source to a single connection pool.
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Send the request, retrying a retryable status or a transport failure.
    ///
    /// Returns the first successful response; the caller decodes the body,
    /// which is the other thing the old JSON and HTML paths disagreed about.
    /// A decode failure is therefore the caller's error, and is not retried —
    /// which is what both copies did before.
    pub async fn send(&self, request: &Request) -> Result<reqwest::Response, FetchError> {
        let mut attempt = 0;

        // `loop` rather than `for`: every branch on the final attempt returns,
        // so there is no path out of the bottom. The `for` version needed an
        // unreachable trailing `Err` and an accumulator that fed it.
        loop {
            let is_final = attempt + 1 >= MAX_ATTEMPTS;
            let mut builder = self.http.get(&request.url).query(&request.query);
            for (name, value) in &request.headers {
                builder = builder.header(*name, value);
            }

            match builder.send().await {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) => {
                    let status = response.status();
                    if !should_retry_status(status) || is_final {
                        return Err(FetchError::Status {
                            url: request.url.clone(),
                            status,
                        });
                    }
                }
                Err(error) => {
                    if is_final {
                        return Err(FetchError::Transport(error));
                    }
                }
            }

            tokio::time::sleep(BASE_BACKOFF * 2_u32.pow(attempt as u32)).await;
            attempt += 1;
        }
    }
}

/// A status worth trying again: the server asked us to slow down, or it broke.
fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Pages already fetched from a Source, stored gzipped.
///
/// Keyed by Source as well as by page, so two Sources that number their pages
/// the same way cannot read each other's rows.
#[derive(Debug, Clone)]
pub struct PageCache {
    db: DbPool,
    source: &'static str,
}

impl PageCache {
    pub fn new(db: DbPool, source: &'static str) -> Self {
        Self { db, source }
    }

    pub fn read<T: DeserializeOwned>(&self, cache_key: &str) -> AppResult<Option<T>> {
        use std::io::Read;

        let conn = self.db.get()?;
        let content_gzip = conn
            .query_row(
                "SELECT content_gzip FROM source_page_cache
                 WHERE source = ?1 AND cache_key = ?2",
                rusqlite::params![self.source, cache_key],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;

        let Some(content_gzip) = content_gzip else {
            return Ok(None);
        };

        let mut decoder = flate2::read::GzDecoder::new(content_gzip.as_slice());
        let mut json = String::new();
        decoder.read_to_string(&mut json)?;
        Ok(Some(serde_json::from_str(&json)?))
    }

    pub fn write<T: Serialize>(
        &self,
        cache_key: &str,
        book_id: &str,
        page_id: &str,
        revision: Option<&str>,
        page: &T,
    ) -> AppResult<()> {
        use std::io::Write;

        let json = serde_json::to_vec(page)?;
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&json)?;
        let content_gzip = encoder.finish()?;
        let fetched_at = chrono::Utc::now().to_rfc3339();
        let conn = self.db.get()?;

        conn.execute(
            "INSERT OR REPLACE INTO source_page_cache (
                source, cache_key, book_id, page_id, content_gzip, content_revision, fetched_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                self.source,
                cache_key,
                book_id,
                page_id,
                content_gzip,
                revision,
                fetched_at
            ],
        )?;

        Ok(())
    }
}

/// Fetch every item, at most `concurrency` at a time, in item order.
///
/// `on_start` fires with each item's index as it is dispatched, which is where
/// a Source reports progress. At `concurrency` 1 nothing is dispatched until
/// the previous item has been collected, so the whole thing is the sequential
/// loop it replaced.
///
/// Results come back in item order however many run at once, and the first
/// error in that order wins and abandons the rest -- again matching what a
/// sequential `?` did.
/// Fetch in order, keeping everything that arrived before the first failure.
///
/// [`fetch_all`] is all-or-nothing, which is right when a partial answer is
/// useless -- half a book is not a book. It is wrong when the work is
/// resumable: a Catalog crawl that loses its three-hundredth page should not
/// throw away the two hundred and ninety-nine before it.
///
/// Results come back in request order even though requests overlap, so what is
/// returned is a contiguous prefix rather than whichever pages happened to
/// land. That is what makes the stopping point a position a later run can
/// resume from, rather than a set of holes to track.
///
/// `on_done` fires as each item lands, with how many have landed, for progress.
pub async fn fetch_prefix<I, T, F, Fut, P>(
    items: &[I],
    concurrency: usize,
    mut on_done: P,
    fetch_one: F,
) -> (Vec<T>, Option<AppError>)
where
    I: Clone,
    F: Fn(I) -> Fut,
    Fut: Future<Output = AppResult<T>>,
    P: FnMut(usize),
{
    // Same lifetime constraint as `fetch_all`: no closure here may take a
    // reference in argument position, or the failure surfaces unexplained at
    // the `generate_handler!` boundary.
    let requests = (0..items.len()).map(|index| items[index].clone());
    let mut stream = futures::stream::iter(requests)
        .map(fetch_one)
        .buffered(concurrency.max(1));

    let mut fetched = Vec::new();
    while let Some(result) = stream.next().await {
        match result {
            Ok(item) => {
                fetched.push(item);
                on_done(fetched.len());
            }
            // Everything after this is dropped even if it already succeeded:
            // keeping it would leave a hole, and a prefix with a hole in it is
            // not a position anything can resume from.
            Err(error) => return (fetched, Some(error)),
        }
    }

    (fetched, None)
}

pub async fn fetch_all<I, T, F, Fut, P>(
    items: &[I],
    concurrency: usize,
    mut on_start: P,
    fetch_one: F,
) -> AppResult<Vec<T>>
where
    I: Clone,
    F: Fn(I) -> Fut,
    Fut: Future<Output = AppResult<T>>,
    P: FnMut(usize),
{
    // No closure here takes a reference in argument position -- not
    // `on_start`, not `fetch_one`, and not the mapping below, which is why it
    // counts indices rather than the more obvious `iter().enumerate()`. A
    // closure whose argument is `&'a I` inside an `async fn` cannot be proved
    // general enough over lifetimes, and the failure surfaces at the
    // `#[tauri::command]` boundary -- pointing at `generate_handler!`, with no
    // mention of this file. A Source that wants the item during `on_start`
    // holds the slice already and can index it. Cloning a table-of-contents
    // entry costs nothing next to the request it is about to make.
    //
    // `Iterator::map` is lazy, so `on_start` still fires as each item is
    // dispatched rather than all at once up front.
    let dispatched = (0..items.len()).map(|index| {
        on_start(index);
        items[index].clone()
    });

    futures::stream::iter(dispatched)
        .map(fetch_one)
        .buffered(concurrency.max(1))
        .try_collect()
        .await
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use crate::db::connection::temporary_pool;
    use crate::error::AppError;

    #[test]
    fn a_page_cached_by_one_source_is_invisible_to_another() {
        let (_dir, pool) = temporary_pool();
        let libretexts = super::PageCache::new(pool.clone(), "libretexts");
        let pressbooks = super::PageCache::new(pool, "pressbooks");

        // The same cache key on purpose: page ids are only unique within a
        // Source, so this is the collision the Source column exists to stop.
        libretexts
            .write("book:1", "book", "1", Some("rev-a"), &"LibreTexts page")
            .expect("the first Source should cache its page");
        pressbooks
            .write("book:1", "book", "1", Some("rev-b"), &"Pressbooks page")
            .expect("the second Source should cache its page");

        assert_eq!(
            libretexts
                .read::<String>("book:1")
                .expect("the first Source should read its own row"),
            Some("LibreTexts page".to_string())
        );
        assert_eq!(
            pressbooks
                .read::<String>("book:1")
                .expect("the second Source should read its own row"),
            Some("Pressbooks page".to_string())
        );
    }

    #[test]
    fn a_page_another_source_never_cached_reads_as_a_miss() {
        let (_dir, pool) = temporary_pool();
        super::PageCache::new(pool.clone(), "libretexts")
            .write("book:1", "book", "1", None, &"LibreTexts page")
            .expect("the first Source should cache its page");

        assert_eq!(
            super::PageCache::new(pool, "pressbooks")
                .read::<String>("book:1")
                .expect("a cache miss is not an error"),
            None
        );
    }

    /// Yield to the runtime `count` times, so an item can be made to finish
    /// later than one dispatched after it without touching the clock.
    async fn yield_times(count: usize) {
        for _ in 0..count {
            tokio::task::yield_now().await;
        }
    }

    #[tokio::test]
    async fn results_come_back_in_item_order_whatever_order_they_finish_in() {
        let items = vec![0_usize, 1, 2, 3];
        let finished = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&finished);

        let results = super::fetch_all(
            &items,
            4,
            |_| {},
            move |item: usize| {
                let recorder = Arc::clone(&recorder);
                async move {
                    // Earlier items take longer, so completion order is the
                    // reverse of item order.
                    yield_times(8 - item * 2).await;
                    recorder
                        .lock()
                        .expect("recorder is not poisoned")
                        .push(item);
                    Ok(item * 10)
                }
            },
        )
        .await
        .expect("every item should be fetched");

        assert_eq!(
            *finished.lock().expect("recorder is not poisoned"),
            vec![3, 2, 1, 0],
            "the test should be watching items finish out of order"
        );
        assert_eq!(results, vec![0, 10, 20, 30]);
    }

    #[tokio::test]
    async fn concurrency_one_never_has_two_items_in_flight() {
        let items = vec![0_usize, 1, 2, 3];
        let in_flight = Arc::new(AtomicUsize::new(0));
        let peak = Arc::new(AtomicUsize::new(0));

        let run = |concurrency: usize| {
            let in_flight = Arc::clone(&in_flight);
            let peak = Arc::clone(&peak);
            let items = items.clone();
            async move {
                in_flight.store(0, Ordering::SeqCst);
                peak.store(0, Ordering::SeqCst);
                super::fetch_all(
                    &items,
                    concurrency,
                    |_| {},
                    move |item: usize| {
                        let in_flight = Arc::clone(&in_flight);
                        let peak = Arc::clone(&peak);
                        async move {
                            let now = in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(now, Ordering::SeqCst);
                            yield_times(4).await;
                            in_flight.fetch_sub(1, Ordering::SeqCst);
                            Ok::<_, AppError>(item)
                        }
                    },
                )
                .await
                .expect("every item should be fetched")
            }
        };

        run(1).await;
        assert_eq!(
            peak.load(Ordering::SeqCst),
            1,
            "at concurrency 1 the fetches should be strictly sequential"
        );

        // The same probe under a higher limit, so a peak of 1 above means the
        // limit held rather than that the probe cannot see overlap at all.
        run(4).await;
        assert!(
            peak.load(Ordering::SeqCst) > 1,
            "the probe should observe overlap when the limit allows it"
        );
    }

    #[tokio::test]
    async fn progress_is_reported_as_each_item_is_dispatched_not_all_up_front() {
        let items = vec![10_usize, 20, 30];
        let log = Arc::new(Mutex::new(Vec::new()));
        let dispatch_log = Arc::clone(&log);
        let fetch_log = Arc::clone(&log);

        super::fetch_all(
            &items,
            1,
            |index| {
                dispatch_log
                    .lock()
                    .expect("log is not poisoned")
                    .push(format!("start {index}"))
            },
            move |item: usize| {
                let log = Arc::clone(&fetch_log);
                async move {
                    yield_times(2).await;
                    log.lock()
                        .expect("log is not poisoned")
                        .push(format!("done {item}"));
                    Ok::<_, AppError>(item)
                }
            },
        )
        .await
        .expect("every item should be fetched");

        // Interleaved, not batched. Asserting only the order of the starts
        // would pass just as happily if every item were announced before the
        // first request went out, which is the reader watching the progress
        // bar reach the end and then wait.
        assert_eq!(
            *log.lock().expect("log is not poisoned"),
            vec!["start 0", "done 10", "start 1", "done 20", "start 2", "done 30"]
        );
    }

    #[tokio::test]
    async fn the_first_failure_in_item_order_abandons_the_rest() {
        let items = vec![0_usize, 1, 2, 3];
        let attempted = Arc::new(Mutex::new(Vec::new()));
        let recorder = Arc::clone(&attempted);

        let error = super::fetch_all(
            &items,
            1,
            |_| {},
            move |item: usize| {
                let recorder = Arc::clone(&recorder);
                async move {
                    recorder
                        .lock()
                        .expect("recorder is not poisoned")
                        .push(item);
                    if item == 1 {
                        return Err(AppError::LibreTexts("page 1 is gone".to_string()));
                    }
                    Ok(item)
                }
            },
        )
        .await
        .expect_err("a failing item should fail the whole fetch");

        assert_eq!(
            *attempted.lock().expect("recorder is not poisoned"),
            vec![0, 1],
            "items after the failure should never be fetched"
        );
        assert_eq!(error.message(), "page 1 is gone");
    }
}
