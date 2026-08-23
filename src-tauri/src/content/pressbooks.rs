//! Pressbooks as a content Source.
//!
//! Pressbooks calls its Catalogs "networks", and that word appears here and in
//! the browser because it is the publisher's own. The domain term is Catalog —
//! see `CONTEXT.md`. There is deliberately no `Network` domain type.
//!
//! Two properties of the API decide the shape of everything below. `per_page`
//! is hard-capped at 10, and the `search` parameter is accepted and ignored.
//! Together they mean a Catalog can only be searched after being enumerated
//! locally, which is why `pressbooks_book` exists and why listing a Catalog is
//! a crawl-once-then-read rather than a query.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use scraper::ElementRef;
use serde::Deserialize;
use serde_json::json;

use crate::content::document::{DocumentBuilder, SectionBuilder};
use crate::content::html_section::{section_content_from_html, SectionSource};
use crate::content::images::{download_cover, download_images};
use crate::content::remote;
use crate::db::connection::DbPool;
use crate::db::models::{PressbooksBook, PressbooksCatalog, PressbooksCatalogListing, SourceType};
use crate::error::{AppError, AppResult};

/// The Catalog the browser opens on.
///
/// Ninety books, so a first visit costs nine requests. The bundled list is
/// ordered by size and its largest Catalog holds three thousand, which is three
/// hundred requests with no progress indicator behind them -- so which Catalog
/// this is matters, and it is named here rather than inferred from the list's
/// order. `catalogs` marks it on the payload for the browser.
pub const DEFAULT_NETWORK_HOST: &str = "milnepublishing.geneseo.edu";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Pressbooks networks sit behind a WAF that rejects a bare tool name with 403
/// and an HTML error page — `libretexts-reader-pressbooks-importer` never
/// reaches the API. The `Mozilla/5.0 (compatible; …)` form is the convention
/// the User-Agent header was designed for: it names this client honestly
/// without impersonating a specific browser, and it is accepted.
const USER_AGENT: &str = concat!(
    "Mozilla/5.0 (compatible; LibreTexts Reader/",
    env!("CARGO_PKG_VERSION"),
    ")"
);

/// The API's own ceiling, not a choice. Asking for more is a 400.
const PER_PAGE: usize = 10;

/// Catalog pages fetched at once. The cap is a deliberate server-load choice by
/// Pressbooks, so this stays modest rather than as high as it could go.
const CATALOG_CONCURRENCY: usize = 8;

/// Chapter pages fetched at once. Lower than the Catalog crawl: a book is a
/// handful of requests, and there is nothing to gain by leaning harder.
const CONTENT_CONCURRENCY: usize = 4;

#[derive(Debug, Clone)]
pub struct PressbooksClient {
    fetcher: remote::Fetcher,
    db: DbPool,
    network_base_url: String,
    host: String,
}

/// One readable entry in a book's table of contents, already flattened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TocEntry {
    pub id: u64,
    pub title: String,
    pub kind: EntryKind,
    pub link: String,
}

/// Which collection endpoint an entry's content comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    FrontMatter,
    Chapter,
    BackMatter,
}

impl EntryKind {
    fn path(self) -> &'static str {
        match self {
            Self::FrontMatter => "front-matter",
            Self::Chapter => "chapters",
            Self::BackMatter => "back-matter",
        }
    }
}

// --- wire types -----------------------------------------------------------
//
// Shapes below were read off the live API rather than inferred. Every field is
// `#[serde(default)]` where the API may omit it, because a Catalog is thousands
// of independently edited books and one missing subtitle must not fail a crawl.

#[derive(Debug, Deserialize)]
struct BundledCatalogs {
    networks: Vec<PressbooksCatalog>,
}

#[derive(Debug, Deserialize)]
struct ApiCatalogBook {
    #[serde(default)]
    link: String,
    #[serde(default)]
    metadata: ApiBookMetadata,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiBookMetadata {
    #[serde(default)]
    name: String,
    /// The subtitle. Named `alternateName`, not `alternativeHeadline`.
    #[serde(default)]
    alternate_name: Option<String>,
    #[serde(default)]
    image: Option<String>,
    #[serde(default)]
    thumbnail_url: Option<String>,
    #[serde(default)]
    word_count: u32,
    #[serde(default)]
    author: Vec<ApiPerson>,
    #[serde(default)]
    license: Option<ApiLicense>,
    #[serde(default)]
    network: Option<ApiNetwork>,
}

#[derive(Debug, Deserialize)]
struct ApiNetwork {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Deserialize)]
struct ApiPerson {
    #[serde(default)]
    name: String,
}

#[derive(Debug, Default, Deserialize)]
struct ApiLicense {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiToc {
    #[serde(default, rename = "front-matter")]
    front_matter: Vec<ApiTocEntry>,
    #[serde(default)]
    parts: Vec<ApiTocPart>,
    #[serde(default, rename = "back-matter")]
    back_matter: Vec<ApiTocEntry>,
}

#[derive(Debug, Deserialize)]
struct ApiTocPart {
    #[serde(default)]
    chapters: Vec<ApiTocEntry>,
}

#[derive(Debug, Deserialize)]
struct ApiTocEntry {
    id: u64,
    #[serde(default)]
    title: String,
    /// The TOC's own answer to "is there anything here?". Used directly: a
    /// heuristic over the HTML would disagree with the publisher about their
    /// own book.
    #[serde(default)]
    has_post_content: bool,
    /// `None` when the Catalog never sent the field, distinct from a measured
    /// `Some(0)` -- an absent count must not be treated as a publisher's
    /// answer of "empty" when `has_post_content` already answered that.
    #[serde(default)]
    word_count: Option<u32>,
    #[serde(default)]
    link: String,
}

#[derive(Debug, Deserialize)]
struct ApiContent {
    id: u64,
    #[serde(default)]
    content: ApiRendered,
}

#[derive(Debug, Default, Deserialize)]
struct ApiRendered {
    #[serde(default)]
    rendered: String,
}

// --- client ---------------------------------------------------------------

impl PressbooksClient {
    pub fn new(db: DbPool) -> Self {
        Self::with_network_base_url(db, format!("https://{DEFAULT_NETWORK_HOST}"))
    }

    /// The base URL is injected rather than read from the environment: tests
    /// point it at a mock server, and `set_var` is process-global while Rust
    /// runs tests as threads in one process.
    pub fn with_network_base_url(db: DbPool, network_base_url: impl Into<String>) -> Self {
        let network_base_url = network_base_url.into().trim_end_matches('/').to_string();
        let host = network_base_url
            .split_once("://")
            .map(|(_, host)| host)
            .unwrap_or(&network_base_url)
            .to_string();

        Self {
            fetcher: remote::Fetcher::with_user_agent(REQUEST_TIMEOUT, USER_AGENT),
            db,
            network_base_url,
            host,
        }
    }

    /// The User-Agent is a default on the client rather than a header here, so
    /// that Figure downloads -- which build their own requests -- carry it too.
    fn request(&self, url: impl Into<String>) -> remote::Request {
        remote::Request::get(url)
    }

    fn source_error(error: remote::FetchError) -> AppError {
        match error {
            remote::FetchError::Status { url, status } => {
                AppError::Pressbooks(format!("request to {url} failed with HTTP {status}"))
            }
            remote::FetchError::Transport(error) => AppError::Http(error),
        }
    }

    async fn fetch_json<T: serde::de::DeserializeOwned>(
        &self,
        request: &remote::Request,
    ) -> AppResult<T> {
        let response = self
            .fetcher
            .send(request)
            .await
            .map_err(Self::source_error)?;
        Ok(response.json::<T>().await?)
    }

    /// The books in this Catalog, crawling first if the local copy is stale.
    ///
    /// A Catalog already crawled stays readable with no network. The freshness
    /// check is one request, and failing it means "cannot tell whether this
    /// changed", not "there are no books" -- so a cached Catalog is served
    /// rather than an error. Only an empty cache makes the failure fatal,
    /// because then there is genuinely nothing to show.
    pub async fn list_catalog(
        &self,
        mut on_progress: impl FnMut(u32, u32),
    ) -> AppResult<PressbooksCatalogListing> {
        // Held for the whole call, so a second request for the same Catalog
        // waits rather than crawling alongside the first. By the time it runs,
        // the first has finished and its freshness check answers "unchanged".
        let lock = crawl_lock(&self.host);
        let _crawling = lock.lock().await;
        let cached = self.cached_listing()?;

        let live_total = match self.live_book_count().await {
            Ok(live_total) => live_total,
            // Cannot tell whether the Catalog changed, which is not the same as
            // there being no books. One already whole is still worth showing;
            // one that is not, and that this attempt could not extend, is a
            // failure the reader has to see -- otherwise Continue loading is a
            // button that silently does nothing.
            Err(error) => {
                return if cached.is_complete && !cached.books.is_empty() {
                    Ok(cached)
                } else {
                    Err(error)
                };
            }
        };

        // Two reasons to crawl: the Catalog changed size, or the last crawl did
        // not finish. The second is what makes reopening a Catalog resume it.
        if live_total == cached.total_books && cached.is_complete {
            return Ok(cached);
        }

        let outcome = self.crawl_catalog(live_total, &mut on_progress).await?;
        let listing = self.cached_listing()?;

        match outcome.failure {
            None => Ok(listing),
            // Pages landed and were committed, so the listing on disk is better
            // than the one this started with, even though the crawl stopped.
            Some(_) if outcome.pages_written > 0 => Ok(listing),
            Some(error) => {
                if listing.is_complete && !listing.books.is_empty() {
                    Ok(listing)
                } else {
                    Err(error)
                }
            }
        }
    }

    /// The Catalog's current size, in one request.
    ///
    /// `X-WP-Total` comes back on any listing, so asking for a single book
    /// answers "has this Catalog changed?" without enumerating it.
    async fn live_book_count(&self) -> AppResult<u32> {
        let request = self
            .request(self.books_url())
            .query(&[("per_page", "1".to_string()), ("page", "1".to_string())]);
        let response = self
            .fetcher
            .send(&request)
            .await
            .map_err(Self::source_error)?;

        // Treated as a failure rather than as zero. A zero would crawl exactly
        // one page, store ten books as if that were the whole Catalog, and
        // record a total of 0 -- which then never matches the cache and
        // re-crawls those same ten rows on every visit, with no error anywhere.
        header_count(&response, "x-wp-total").ok_or_else(|| {
            AppError::Pressbooks(format!(
                "Pressbooks network {} did not report how many books it holds",
                self.host
            ))
        })
    }

    /// Fetch the Catalog's pages, writing each one as it arrives.
    ///
    /// Committed per page rather than at the end, because the interruption
    /// worth surviving is not only a failed request: a crawl of the largest
    /// bundled Catalog runs for minutes, and a reader who closes the
    /// application during it would otherwise lose every page. Each commit
    /// moves `synced_pages`, so the position on disk is always a page a later
    /// run can carry on from.
    ///
    /// The concurrency cap is a politeness limit on a third party, not a
    /// throughput dial -- the ten-books-per-page limit it works around is the
    /// publisher's own tightening of their platform default.
    async fn crawl_catalog(
        &self,
        total_books: u32,
        on_progress: &mut impl FnMut(u32, u32),
    ) -> AppResult<CrawlOutcome> {
        let total_pages = total_books.div_ceil(PER_PAGE as u32).max(1);
        let start = self.crawl_position(total_books, total_pages)?;
        let pages = (start.first_page..=total_pages).collect::<Vec<_>>();
        let done_before = start.first_page - 1;
        let mut pages_written = 0_u32;

        on_progress(done_before, total_pages);
        let failure = remote::fetch_in_order(
            &pages,
            CATALOG_CONCURRENCY,
            |index, books: Vec<ApiCatalogBook>| {
                let page = start.first_page + index as u32;
                self.store_page(page, books, &start, total_books, total_pages)?;
                pages_written = index as u32 + 1;
                on_progress(done_before + pages_written, total_pages);
                Ok(())
            },
            |page: u32| async move {
                let request = self.request(self.books_url()).query(&[
                    ("per_page", PER_PAGE.to_string()),
                    ("page", page.to_string()),
                ]);
                self.fetch_json::<Vec<ApiCatalogBook>>(&request).await
            },
        )
        .await;

        Ok(CrawlOutcome {
            pages_written,
            failure,
        })
    }

    /// Commit one page of the Catalog, and the position it establishes.
    fn store_page(
        &self,
        page: u32,
        books: Vec<ApiCatalogBook>,
        start: &CrawlStart,
        total_books: u32,
        total_pages: u32,
    ) -> AppResult<()> {
        let mut network_name = None;
        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        // Derived from the page rather than counted, so a resumed crawl numbers
        // its books exactly as an uninterrupted one would have.
        let mut position = (page as i64 - 1) * PER_PAGE as i64;

        for book in books {
            if book.link.trim().is_empty() {
                continue;
            }
            let metadata = book.metadata;
            if network_name.is_none() {
                network_name = metadata
                    .network
                    .as_ref()
                    .map(|network| network.name.trim().to_string())
                    .filter(|name| !name.is_empty());
            }
            let authors = metadata
                .author
                .iter()
                .map(|person| person.name.trim())
                .filter(|name| !name.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            let license = metadata.license.unwrap_or_default();

            tx.execute(
                "INSERT OR REPLACE INTO pressbooks_book (
                    host, book_url, title, subtitle, cover_url, thumbnail_url,
                    authors, license_name, license_url, word_count, menu_position,
                    seen_at
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    self.host,
                    book.link.trim(),
                    metadata.name.trim(),
                    clean(metadata.alternate_name),
                    clean(metadata.image),
                    clean(metadata.thumbnail_url),
                    authors,
                    license.name.trim(),
                    clean(license.url),
                    metadata.word_count,
                    position,
                    start.stamp,
                ],
            )?;
            position += 1;
        }

        // The name is kept rather than rewritten when a page does not carry
        // one: only some pages do, and a later page overwriting the Catalog's
        // name with its host would undo what the first page established.
        tx.execute(
            "INSERT INTO pressbooks_network
                 (host, name, total_books, synced_pages, total_pages, synced_at, crawl_stamp)
             VALUES (?1, COALESCE(?2, ?1), ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(host) DO UPDATE SET
                 name = COALESCE(?2, name),
                 total_books = excluded.total_books,
                 synced_pages = excluded.synced_pages,
                 total_pages = excluded.total_pages,
                 synced_at = excluded.synced_at,
                 crawl_stamp = excluded.crawl_stamp",
            params![
                self.host,
                network_name,
                total_books,
                page,
                total_pages,
                Utc::now().to_rfc3339(),
                start.stamp,
            ],
        )?;

        // Only a crawl that reached the last page knows what the Catalog no
        // longer holds. Deleting earlier would take books this crawl has not
        // reached yet, which on its first page is the whole listing.
        if page >= total_pages {
            tx.execute(
                "DELETE FROM pressbooks_book
                 WHERE host = ?1 AND (seen_at IS NULL OR seen_at <> ?2)",
                params![self.host, start.stamp],
            )?;
        }

        tx.commit()?;

        Ok(())
    }

    /// Where a crawl of this Catalog should start, and under whose stamp.
    ///
    /// Resumes only into a Catalog of the same size. One that has grown or
    /// shrunk renumbers its pages, so continuing into it would interleave two
    /// different listings under one set of positions.
    fn crawl_position(&self, total_books: u32, total_pages: u32) -> AppResult<CrawlStart> {
        let conn = self.db.get()?;
        let stored = conn
            .query_row(
                "SELECT total_books, synced_pages, total_pages, crawl_stamp
                 FROM pressbooks_network WHERE host = ?1",
                params![self.host],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, u32>(2)?,
                        row.get::<_, Option<String>>(3)?,
                    ))
                },
            )
            .optional()?;

        let fresh = CrawlStart {
            first_page: 1,
            stamp: Utc::now().to_rfc3339(),
        };

        let Some((stored_books, synced_pages, stored_pages, Some(stamp))) = stored else {
            return Ok(fresh);
        };

        if stored_books == total_books && stored_pages == total_pages && synced_pages < total_pages
        {
            Ok(CrawlStart {
                first_page: synced_pages + 1,
                stamp,
            })
        } else {
            Ok(fresh)
        }
    }

    /// The Catalog as it stands on disk, with how far its crawl got.
    fn cached_listing(&self) -> AppResult<PressbooksCatalogListing> {
        let books = self.cached_books()?;
        let conn = self.db.get()?;
        let progress = conn
            .query_row(
                "SELECT total_books, synced_pages, total_pages
                 FROM pressbooks_network WHERE host = ?1",
                params![self.host],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, u32>(2)?,
                    ))
                },
            )
            .optional()?
            .unwrap_or((0, 0, 0));

        let (total_books, synced_pages, total_pages) = progress;

        Ok(PressbooksCatalogListing {
            books,
            total_books,
            // A Catalog crawled before crawls could stop part-way recorded
            // equal page counts, so it reads as complete -- which it is.
            is_complete: total_pages > 0 && synced_pages >= total_pages,
        })
    }

    /// The cached Catalog, narrowed to the books whose title, subtitle or
    /// author contain `query`.
    ///
    /// Cache-only, and deliberately so. Pressbooks ignores its own `search`
    /// parameter and returns the whole Catalog whatever is asked of it, so the
    /// local enumeration is the only searchable copy -- and reading it costs no
    /// request, which is what lets results follow the reader's typing.
    pub fn search_catalog(&self, query: &str) -> AppResult<Vec<PressbooksBook>> {
        self.cached_books_matching(query.trim())
    }

    fn cached_books(&self) -> AppResult<Vec<PressbooksBook>> {
        self.cached_books_matching("")
    }

    fn cached_books_matching(&self, query: &str) -> AppResult<Vec<PressbooksBook>> {
        let conn = self.db.get()?;
        // A plain LIKE, per the design: at a few thousand rows this is
        // sub-millisecond, so no full-text search extension is introduced and
        // no build configuration changes. LIKE also ignores case for ASCII on
        // its own, which is the behaviour a reader typing a title expects.
        let mut statement = conn.prepare(
            "SELECT book_url, title, subtitle, cover_url, thumbnail_url,
                    authors, license_name, license_url, word_count
             FROM pressbooks_book
             WHERE host = ?1
               AND (?2 = ''
                    OR title LIKE ?3 ESCAPE '\\'
                    OR subtitle LIKE ?3 ESCAPE '\\'
                    OR authors LIKE ?3 ESCAPE '\\')
             ORDER BY menu_position",
        )?;
        let books = statement
            .query_map(params![self.host, query, like_pattern(query)], |row| {
                Ok(PressbooksBook {
                    book_url: row.get(0)?,
                    title: row.get(1)?,
                    subtitle: row.get(2)?,
                    cover_url: row.get(3)?,
                    thumbnail_url: row.get(4)?,
                    authors: row.get(5)?,
                    license_name: row.get(6)?,
                    license_url: row.get(7)?,
                    word_count: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(books)
    }

    fn books_url(&self) -> String {
        format!("{}/wp-json/pressbooks/v2/books", self.network_base_url)
    }

    /// A book's own API root. Each Pressbooks book is its own WordPress site.
    fn book_api_url(book_url: &str, path: &str) -> String {
        format!(
            "{}/wp-json/pressbooks/v2/{path}",
            book_url.trim_end_matches('/')
        )
    }

    async fn fetch_metadata(&self, book_url: &str) -> AppResult<ApiBookMetadata> {
        let request = self.request(Self::book_api_url(book_url, "metadata"));
        self.fetch_json(&request).await
    }

    /// The book's readable entries, in the order the table of contents gives.
    pub async fn fetch_toc(&self, book_url: &str) -> AppResult<Vec<TocEntry>> {
        let request = self.request(Self::book_api_url(book_url, "toc"));
        let toc = self.fetch_json::<ApiToc>(&request).await?;
        Ok(flatten_toc(toc))
    }

    /// Every entry's rendered HTML, keyed by entry id.
    ///
    /// Fetched by collection rather than one entry at a time: a 40-chapter book
    /// is about seven requests this way and forty-one the other.
    async fn fetch_content(&self, book_url: &str, entries: &[TocEntry]) -> AppResult<Vec<Page>> {
        let mut pages = Vec::new();
        for kind in [
            EntryKind::FrontMatter,
            EntryKind::Chapter,
            EntryKind::BackMatter,
        ] {
            if !entries.iter().any(|entry| entry.kind == kind) {
                continue;
            }
            pages.extend(self.fetch_collection(book_url, kind).await?);
        }

        Ok(pages)
    }

    /// One whole collection endpoint, however many pages it runs to.
    ///
    /// The page count comes from the collection's own `X-WP-TotalPages`, never
    /// from how many entries the table of contents kept. A collection returns
    /// every post of its kind including the empty ones the table of contents
    /// drops, so sizing the crawl by the readable count fetches the first N
    /// posts and silently loses any readable entry sitting past them -- a
    /// truncated book that looks complete.
    async fn fetch_collection(&self, book_url: &str, kind: EntryKind) -> AppResult<Vec<Page>> {
        let first = self.fetch_collection_page(book_url, kind, 1).await?;
        let total_pages = first.total_pages.max(1);
        let mut pages = first.pages;

        if total_pages > 1 {
            let rest = (2..=total_pages).collect::<Vec<_>>();
            let fetched = remote::fetch_all(
                &rest,
                CONTENT_CONCURRENCY,
                |_| Ok(()),
                |page: u32| async move {
                    Ok(self
                        .fetch_collection_page(book_url, kind, page)
                        .await?
                        .pages)
                },
            )
            .await?;
            pages.extend(fetched.into_iter().flatten());
        }

        Ok(pages)
    }

    async fn fetch_collection_page(
        &self,
        book_url: &str,
        kind: EntryKind,
        page: u32,
    ) -> AppResult<CollectionPage> {
        let request = self
            .request(Self::book_api_url(book_url, kind.path()))
            .query(&[
                ("per_page", PER_PAGE.to_string()),
                ("page", page.to_string()),
            ]);
        let response = self
            .fetcher
            .send(&request)
            .await
            .map_err(Self::source_error)?;
        let total_pages = header_count(&response, "x-wp-totalpages").unwrap_or(1);
        let body = response.json::<Vec<ApiContent>>().await?;

        Ok(CollectionPage {
            total_pages,
            pages: body
                .into_iter()
                .map(|entry| Page {
                    id: entry.id,
                    html: entry.content.rendered,
                })
                .collect(),
        })
    }
}

/// What a crawl achieved: how many pages it committed, and what stopped it.
///
/// Separate from a plain `Result` because "failed after writing nothing" and
/// "failed after writing two hundred pages" are different answers to give a
/// reader -- the first is a failure to report, the second is progress.
struct CrawlOutcome {
    pages_written: u32,
    failure: Option<AppError>,
}

/// Where a crawl begins: its first page, and the stamp its rows carry.
///
/// The Catalog's name is not here. Only some pages carry one, so a resumed
/// crawl may see none at all -- the upsert keeps whatever is already recorded
/// rather than having the crawl carry it forward.
struct CrawlStart {
    first_page: u32,
    stamp: String,
}

struct CollectionPage {
    total_pages: u32,
    pages: Vec<Page>,
}

fn header_count(response: &reqwest::Response, name: &str) -> Option<u32> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u32>().ok())
}

struct Page {
    id: u64,
    html: String,
}

/// Front matter, then parts' chapters, then back matter — the order the table
/// of contents presents them in.
///
/// Parts are flattened away: they are containers with no content of their own,
/// and `DocumentBuilder.sections` is a flat `Vec` with nowhere to put one.
/// Entries the table of contents marks as empty are dropped here, using its
/// flags directly rather than guessing from the HTML.
fn flatten_toc(toc: ApiToc) -> Vec<TocEntry> {
    let mut entries = Vec::new();

    for entry in toc.front_matter {
        push_readable(&mut entries, entry, EntryKind::FrontMatter);
    }
    for part in toc.parts {
        for entry in part.chapters {
            push_readable(&mut entries, entry, EntryKind::Chapter);
        }
    }
    for entry in toc.back_matter {
        push_readable(&mut entries, entry, EntryKind::BackMatter);
    }

    entries
}

fn push_readable(entries: &mut Vec<TocEntry>, entry: ApiTocEntry, kind: EntryKind) {
    // `has_post_content` stands alone: a Catalog that never sends `word_count`
    // must not fail closed. A measured zero still excludes the entry.
    if !entry.has_post_content || entry.word_count == Some(0) {
        return;
    }

    entries.push(TocEntry {
        id: entry.id,
        title: entry.title.trim().to_string(),
        kind,
        link: entry.link,
    });
}

/// Refuse a book URL that does not belong to a Catalog the application offers.
///
/// Every request an Import makes is built from this URL, and it arrives from
/// the webview, so it is checked at the boundary rather than trusted -- the
/// same guard `list_books` applies to the host it is given. It lives here
/// rather than inside `import_book` so that `import_book` stays reachable from
/// a mock server in tests.
pub fn verify_offered_book_url(book_url: &str) -> AppResult<()> {
    let url = reqwest::Url::parse(book_url)
        .map_err(|_| AppError::Pressbooks(format!("{book_url} is not a usable book URL")))?;

    // The host check is the reason this guard exists, but the rest of the URL
    // is no more trusted than the host is: `book_api_url` builds every
    // request from it verbatim, scheme included.
    if url.scheme() != "https" {
        return Err(AppError::Pressbooks(format!("{book_url} must use https")));
    }
    if url.port().is_some() {
        return Err(AppError::Pressbooks(format!(
            "{book_url} must not name a port"
        )));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(AppError::Pressbooks(format!(
            "{book_url} must not carry userinfo"
        )));
    }

    let host = url
        .host_str()
        .ok_or_else(|| AppError::Pressbooks(format!("{book_url} is not a usable book URL")))?;
    offered_host(host).map(|_| ())
}

/// One crawl at a time per Catalog.
///
/// Ordinary navigation unmounts and remounts the browser, and every mount asks
/// for its Catalog again. Without this, a reader who switches tabs during a
/// three-hundred-request crawl has two of them running against the same third
/// party -- and two crawls committing positions in whatever order they finish
/// leaves the recorded one wrong, so the next open re-fetches pages that are
/// already on disk.
///
/// Keyed by host rather than global: crawling two different Catalogs at once is
/// two readers' worth of load on two different servers, which is fine.
fn crawl_lock(host: &str) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();

    let locks = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut locks = locks
        .lock()
        .expect("crawl lock registry should not be poisoned");
    Arc::clone(
        locks
            .entry(host.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

/// A contains-match pattern for LIKE.
///
/// `%` and `_` are wildcards to LIKE, so a reader typing one would otherwise
/// widen their own search instead of narrowing it -- `%` alone would match the
/// whole Catalog. The backslash escapes itself first, or escaping the wildcards
/// would introduce escapes the reader could then break.
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Pressbooks renders equations to QuickLaTeX images. Kept as figures they
/// would be pictures of mathematics a reader cannot hear, so they are dropped —
/// but QuickLaTeX writes the original LaTeX into the image's `alt`, and that is
/// recovered as notation the reader can see and the speech path can say.
struct PressbooksSource;

impl SectionSource for PressbooksSource {
    fn should_skip_paragraph(&self, _element: &ElementRef<'_>) -> bool {
        false
    }

    fn should_skip_image(&self, element: &ElementRef<'_>) -> bool {
        is_quicklatex_image(element)
    }

    fn math_from_image(&self, element: &ElementRef<'_>) -> Option<String> {
        is_quicklatex_image(element)
            .then(|| quicklatex_latex(element))
            .flatten()
    }
}

/// QuickLaTeX marks every image it renders with a `ql-img-…` class, and serves
/// it from a path naming `quicklatex.com`. The class is the reliable half:
/// Pressbooks caches the images under its own host, where the class survives
/// and the hostname does not — but the filename it caches them under still
/// carries the name, so both are worth asking.
fn is_quicklatex_image(element: &ElementRef<'_>) -> bool {
    let attr = |name| element.value().attr(name).unwrap_or_default();

    attr("class")
        .split_whitespace()
        .any(|class| class.starts_with("ql-img-"))
        || attr("src").contains("quicklatex.com")
}

/// The LaTeX QuickLaTeX kept in the `alt`, already entity-decoded by the HTML
/// parser. `None` when the image carries no notation to recover — an equation
/// with nothing readable behind it is better dropped than turned into a token
/// that renders as an empty box or as QuickLaTeX's own name.
fn quicklatex_latex(element: &ElementRef<'_>) -> Option<String> {
    element
        .value()
        .attr("alt")
        .map(str::trim)
        .filter(|alt| !alt.is_empty() && !alt.eq_ignore_ascii_case(QUICKLATEX_CREDIT))
        .map(str::to_string)
}

/// What QuickLaTeX puts in the `alt` when it has no source to put there.
const QUICKLATEX_CREDIT: &str = "Rendered by QuickLaTeX.com";

/// The Catalogs the application offers.
///
/// Bundled rather than discovered: Pressbooks has no directory API, and the one
/// host that comes closest answers an unrecognised client with a block page.
/// The resource also records the Catalogs deliberately left out and why, so an
/// unreachable one is not re-added by someone who rediscovers the problem.
pub fn catalogs() -> AppResult<Vec<PressbooksCatalog>> {
    let bundled: BundledCatalogs = serde_json::from_str(include_str!(
        "../../resources/catalog/pressbooks-networks.json"
    ))?;
    Ok(bundled
        .networks
        .into_iter()
        .map(|mut catalog| {
            catalog.is_default = catalog.host == DEFAULT_NETWORK_HOST;
            catalog
        })
        .collect())
}

/// Reject a host the application does not offer.
///
/// The host arrives from the webview and becomes part of every URL the crawl
/// requests, so it is checked against the bundled list rather than trusted. It
/// also keeps a typo from silently creating an empty Catalog under a host that
/// does not exist.
fn offered_host(host: &str) -> AppResult<String> {
    catalogs()?
        .into_iter()
        .find(|catalog| catalog.host == host)
        .map(|catalog| catalog.host)
        .ok_or_else(|| {
            AppError::Pressbooks(format!(
                "{host} is not a Pressbooks catalog this app offers"
            ))
        })
}

/// List a Catalog, crawling or resuming it if the local copy is not current.
///
/// `on_progress` reports pages fetched against pages needed. The largest
/// bundled Catalog is three hundred requests, so a first open without it reads
/// as a frozen application.
pub async fn list_books(
    db: DbPool,
    host: &str,
    on_progress: impl FnMut(u32, u32),
) -> AppResult<PressbooksCatalogListing> {
    let host = offered_host(host)?;
    PressbooksClient::with_network_base_url(db, format!("https://{host}"))
        .list_catalog(on_progress)
        .await
}

/// Search a Catalog the reader has already opened.
///
/// Separate from [`list_books`] because that one checks the Catalog against the
/// network before answering. Searching must not: it runs on every keystroke,
/// and the enumerated cache is exactly what makes that affordable.
pub fn search_books(db: DbPool, host: &str, query: &str) -> AppResult<Vec<PressbooksBook>> {
    let host = offered_host(host)?;
    PressbooksClient::with_network_base_url(db, format!("https://{host}")).search_catalog(query)
}

/// Import a book, keeping its cover in `covers_dir`.
///
/// The directory is a parameter because `paths::covers_dir` creates what it
/// resolves, so a test importing a book with a cover would otherwise write into
/// the real application data directory and be indistinguishable from use.
pub async fn import_book<F>(
    db: DbPool,
    book_url: &str,
    covers_dir: &Path,
    mut on_progress: F,
) -> AppResult<DocumentBuilder>
where
    F: FnMut(u32, u32) -> AppResult<()> + Send,
{
    let client = PressbooksClient::new(db);
    let metadata = client.fetch_metadata(book_url).await?;
    let entries = client.fetch_toc(book_url).await?;

    if entries.is_empty() {
        return Err(AppError::Pressbooks(format!(
            "Pressbooks book {book_url} did not include readable content"
        )));
    }

    let total = entries.len() as u32;
    on_progress(0, total)?;
    let pages = client.fetch_content(book_url, &entries).await?;

    let mut sections = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        on_progress(index as u32 + 1, total)?;

        let Some(page) = pages.iter().find(|page| page.id == entry.id) else {
            // The table of contents said this entry has content. Skipping it
            // quietly would hand the reader a book missing a chapter and
            // looking complete.
            return Err(AppError::Pressbooks(format!(
                "Pressbooks book {book_url} listed \"{}\" in its table of contents but returned no content for it",
                entry.title
            )));
        };
        let (paragraphs, image_candidates) =
            section_content_from_html(&page.html, &entry.link, &PressbooksSource);
        let images = download_images(client.fetcher.http(), image_candidates).await?;
        if paragraphs.is_empty() && images.is_empty() {
            continue;
        }

        sections.push(SectionBuilder {
            title: entry.title.clone(),
            paragraphs,
            images,
        });
    }

    if sections.is_empty() {
        return Err(AppError::Pressbooks(format!(
            "Pressbooks book {book_url} did not include readable content"
        )));
    }

    // Fetched last, so a book that failed to assemble leaves no cover behind,
    // and a book that names no cover makes no request for one. A cover that
    // will not download is `None`, never an Import failure: the book is
    // readable without it.
    let cover_image_path = match clean(metadata.image) {
        Some(url) => download_cover(client.fetcher.http(), covers_dir, book_url, &url).await,
        None => None,
    };

    let license = metadata.license.unwrap_or_default();
    let authors = metadata
        .author
        .iter()
        .map(|person| person.name.trim())
        .filter(|name| !name.is_empty())
        .collect::<Vec<_>>()
        .join(", ");

    Ok(DocumentBuilder {
        title: metadata.name.trim().to_string(),
        source_type: SourceType::Pressbooks,
        source_metadata: json!({
            "book_url": book_url,
            "imported_at": Utc::now().to_rfc3339(),
        }),
        cover_image_path,
        license: clean(Some(license.name)),
        attribution: clean(Some(authors)),
        sections,
    })
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use std::time::Duration;

    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use regex::Regex;

    use super::{EntryKind, PressbooksClient, USER_AGENT};
    use crate::db::connection::temporary_pool;
    use crate::db::models::SourceType;

    #[test]
    fn user_agent_reports_the_version_this_build_actually_is() {
        // It went out to third-party Pressbooks servers reading
        // "LibreTexts Reader/0.1.0" as a literal, so the first version bump
        // made it misreport the client -- silently, since nothing here reads
        // it back. Deriving it is what keeps that impossible.
        assert!(
            USER_AGENT.contains(env!("CARGO_PKG_VERSION")),
            "User-Agent {USER_AGENT:?} does not name the crate version {:?}",
            env!("CARGO_PKG_VERSION"),
        );
    }

    /// A table of contents with one of everything the flattening rules care
    /// about: readable front matter, an empty title page, a part wrapping two
    /// chapters, one of which has no words, and readable back matter.
    const TOC_JSON: &str = r#"{
        "front-matter": [
            {"id": 10, "title": "Title Page", "has_post_content": false, "word_count": 0,
             "link": "https://books.test/logic/front-matter/title-page/"},
            {"id": 11, "title": "About This Book", "has_post_content": true, "word_count": 120,
             "link": "https://books.test/logic/front-matter/about/"}
        ],
        "parts": [
            {"title": "Part I", "has_post_content": false, "word_count": 0, "chapters": [
                {"id": 21, "title": "1. A Precise Language", "has_post_content": true,
                 "word_count": 400, "link": "https://books.test/logic/chapter/precise/"},
                {"id": 22, "title": "Placeholder", "has_post_content": true, "word_count": 0,
                 "link": "https://books.test/logic/chapter/placeholder/"}
            ]}
        ],
        "back-matter": [
            {"id": 31, "title": "Glossary", "has_post_content": true, "word_count": 90,
             "link": "https://books.test/logic/back-matter/glossary/"}
        ]
    }"#;

    async fn server_with_toc() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/toc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(TOC_JSON))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn the_table_of_contents_keeps_its_own_order_across_matter_and_parts() {
        let (_dir, pool) = temporary_pool();
        let server = server_with_toc().await;
        let client = PressbooksClient::with_network_base_url(pool, server.uri());

        let entries = client
            .fetch_toc(&format!("{}/book", server.uri()))
            .await
            .expect("the table of contents should be readable");

        assert_eq!(
            entries
                .iter()
                .map(|entry| (entry.title.as_str(), entry.kind))
                .collect::<Vec<_>>(),
            vec![
                ("About This Book", EntryKind::FrontMatter),
                ("1. A Precise Language", EntryKind::Chapter),
                ("Glossary", EntryKind::BackMatter),
            ],
            "front matter, then parts' chapters, then back matter -- with parts flattened away"
        );
    }

    /// A book's metadata, with `cover` as its `image` -- the field Pressbooks
    /// puts the cover URL in. Taken as a parameter rather than fixed, so the
    /// cover points at the mock server instead of at a host nothing serves.
    fn metadata_json(cover: Option<&str>) -> String {
        let image = match cover {
            Some(url) => format!(r#", "image": "{url}""#),
            None => String::new(),
        };
        format!(
            r#"{{
        "name": "A Concise Introduction to Logic",
        "author": [{{"@type": "Person", "name": "Craig DeLancey"}},
                   {{"@type": "Person", "name": "  "}}],
        "license": {{"@type": "CreativeWork", "name": "CC BY-NC-SA (Attribution NonCommercial ShareAlike)",
                    "url": "https://creativecommons.org/licenses/by-nc-sa/4.0/"}}{image}
    }}"#
        )
    }

    /// A throwaway directory for covers. Never `paths::covers_dir` -- that one
    /// creates what it resolves, in the real application data directory.
    fn covers_dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("temporary covers directory")
    }

    /// Stands in for a cover image. `download_image` stores what it is given
    /// and does not decode it, so the bytes only have to be recognisable.
    const COVER_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n-- a concise cover --";

    fn content_body(id: u64, html: &str) -> String {
        format!(
            r#"[{{"id": {id}, "content": {{"rendered": {}}}}}]"#,
            serde_json::json!(html)
        )
    }

    /// A whole book, served the way the API serves one: metadata, a table of
    /// contents, then one collection endpoint per kind of entry.
    async fn server_with_book() -> MockServer {
        server_with_book_cover(Some(200)).await
    }

    /// The same book, differing only in what it says about its cover: `None`
    /// names no cover at all, `Some(status)` names one the server answers with
    /// `status`.
    async fn server_with_book_cover(cover: Option<u16>) -> MockServer {
        let server = server_with_toc().await;
        let cover_url = cover.map(|_| format!("{}/cover.png", server.uri()));
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/metadata"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(metadata_json(cover_url.as_deref())),
            )
            .mount(&server)
            .await;
        if let Some(status) = cover {
            let response = if status == 200 {
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(COVER_BYTES)
            } else {
                ResponseTemplate::new(status)
            };
            Mock::given(method("GET"))
                .and(path("/cover.png"))
                .respond_with(response)
                .mount(&server)
                .await;
        }
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/front-matter"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(content_body(11, "<p>This book introduces logic.</p>")),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/chapters"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(content_body(21, "<p>We begin with sentences.</p>")),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/back-matter"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(content_body(31, "<p>Argument: a set of premises.</p>")),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn an_imported_book_becomes_a_document_in_table_of_contents_order() {
        let (_dir, pool) = temporary_pool();
        let covers = covers_dir();
        let server = server_with_book().await;

        let document = super::import_book(
            pool,
            &format!("{}/book", server.uri()),
            covers.path(),
            |_, _| Ok(()),
        )
        .await
        .expect("a readable book should import");

        assert_eq!(document.title, "A Concise Introduction to Logic");
        assert_eq!(document.source_type, SourceType::Pressbooks);
        assert_eq!(
            document
                .sections
                .iter()
                .map(|section| section.title.as_str())
                .collect::<Vec<_>>(),
            vec!["About This Book", "1. A Precise Language", "Glossary"],
            "front matter and back matter are Sections too, in the book's own order"
        );
        assert_eq!(
            document.sections[1].paragraphs,
            vec!["We begin with sentences."]
        );
    }

    #[tokio::test]
    async fn a_book_cover_is_downloaded_and_kept_with_the_document() {
        let (_dir, pool) = temporary_pool();
        let covers = covers_dir();
        let server = server_with_book().await;

        let document = super::import_book(
            pool,
            &format!("{}/book", server.uri()),
            covers.path(),
            |_, _| Ok(()),
        )
        .await
        .expect("the book should import");

        let cover = document
            .cover_image_path
            .expect("the Document should carry its cover");
        assert!(
            std::path::Path::new(&cover).starts_with(covers.path()),
            "the cover should be stored where it was told to, got {cover}"
        );
        assert_eq!(
            std::fs::read(&cover).expect("the cover should be on disk"),
            COVER_BYTES
        );
    }

    #[tokio::test]
    async fn a_cover_that_will_not_download_does_not_fail_the_import() {
        // A book is readable without its cover. Failing the Import over one
        // would cost the reader the whole book to save them a thumbnail.
        let (_dir, pool) = temporary_pool();
        let covers = covers_dir();
        let server = server_with_book_cover(Some(404)).await;

        let document = super::import_book(
            pool,
            &format!("{}/book", server.uri()),
            covers.path(),
            |_, _| Ok(()),
        )
        .await
        .expect("a book whose cover 404s should still import");

        assert!(document.cover_image_path.is_none());
        assert!(
            !document.sections.is_empty(),
            "the book should still arrive"
        );
        assert_eq!(
            std::fs::read_dir(covers.path())
                .expect("the covers directory should be readable")
                .count(),
            0,
            "a failed download should leave nothing behind"
        );
    }

    #[tokio::test]
    async fn a_book_that_names_no_cover_imports_without_one() {
        let (_dir, pool) = temporary_pool();
        let covers = covers_dir();
        let server = server_with_book_cover(None).await;

        let document = super::import_book(
            pool,
            &format!("{}/book", server.uri()),
            covers.path(),
            |_, _| Ok(()),
        )
        .await
        .expect("a book with no cover should still import");

        assert!(document.cover_image_path.is_none());
        assert!(
            !document.sections.is_empty(),
            "the book should still arrive"
        );
    }

    #[tokio::test]
    async fn the_licence_attribution_and_book_url_travel_with_the_document() {
        let (_dir, pool) = temporary_pool();
        let covers = covers_dir();
        let server = server_with_book().await;
        let book_url = format!("{}/book", server.uri());

        let document = super::import_book(pool, &book_url, covers.path(), |_, _| Ok(()))
            .await
            .expect("a readable book should import");

        assert_eq!(
            document.license.as_deref(),
            Some("CC BY-NC-SA (Attribution NonCommercial ShareAlike)")
        );
        // The blank author in the metadata is dropped rather than joined in as
        // an empty name.
        assert_eq!(document.attribution.as_deref(), Some("Craig DeLancey"));
        assert_eq!(document.source_metadata["book_url"], book_url);
    }

    #[tokio::test]
    async fn a_book_with_nothing_readable_fails_rather_than_importing_empty() {
        let (_dir, pool) = temporary_pool();
        let covers = covers_dir();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_string(metadata_json(None)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/toc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"front-matter": [{"id": 1, "title": "Title Page",
                    "has_post_content": false, "word_count": 0, "link": ""}],
                    "parts": [], "back-matter": []}"#,
            ))
            .mount(&server)
            .await;

        let Err(error) = super::import_book(
            pool,
            &format!("{}/book", server.uri()),
            covers.path(),
            |_, _| Ok(()),
        )
        .await
        else {
            panic!("a book with no readable entries should fail");
        };

        // The Library is left untouched because nothing is persisted: an
        // importer that returned an empty Document would be persisted as one.
        assert_eq!(error.kind(), "pressbooks");
    }

    fn catalog_page(link: &str, name: &str) -> String {
        format!(
            r#"[{{"id": 1, "link": "{link}", "metadata": {{
                "name": "{name}", "alternateName": "A subtitle", "wordCount": 25637,
                "author": [{{"@type": "Person", "name": "Orna Farrell"}}],
                "license": {{"name": "CC BY (Attribution)", "url": "https://creativecommons.org/licenses/by/4.0/"}},
                "thumbnailUrl": "https://books.test/thumb.png"
            }}}}]"#
        )
    }

    #[tokio::test]
    async fn a_catalog_is_crawled_once_and_read_from_the_cache_after_that() {
        let (_dir, pool) = temporary_pool();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-wp-total", "1")
                    .set_body_string(catalog_page("https://books.test/openteach/", "Openteach")),
            )
            .mount(&server)
            .await;

        let client = PressbooksClient::with_network_base_url(pool, server.uri());
        let first = client
            .list_catalog(|_, _| {})
            .await
            .expect("the Catalog should list");
        let after_crawl = server
            .received_requests()
            .await
            .expect("mock server should record requests")
            .len();
        let second = client
            .list_catalog(|_, _| {})
            .await
            .expect("the Catalog should list again");
        let after_second = server
            .received_requests()
            .await
            .expect("mock server should record requests")
            .len();

        assert_eq!(first.books.len(), 1);
        assert_eq!(first.books[0].title, "Openteach");
        assert_eq!(first.books[0].book_url, "https://books.test/openteach/");
        assert_eq!(first.books[0].subtitle.as_deref(), Some("A subtitle"));
        assert_eq!(first.books[0].authors, "Orna Farrell");
        assert_eq!(first.books[0].license_name, "CC BY (Attribution)");
        assert_eq!(first.books[0].word_count, 25637);

        // The second visit costs one request -- the freshness check -- not a
        // second crawl. Re-enumerating on every visit is what the local cache
        // exists to avoid.
        assert_eq!(
            second.books.len(),
            1,
            "the cached Catalog should still list its books"
        );
        assert_eq!(
            after_second - after_crawl,
            1,
            "a fresh Catalog should cost only the count check"
        );
    }

    /// Three books that differ in title, subtitle and author, so a match can
    /// be attributed to exactly one field rather than to any of them.
    const SEARCHABLE_CATALOG: &str = r#"[
        {"id": 1, "link": "https://books.test/openteach/", "metadata": {
            "name": "Openteach", "alternateName": "A Guide for Teaching Online",
            "author": [{"@type": "Person", "name": "Orna Farrell"}]}},
        {"id": 2, "link": "https://books.test/geology/", "metadata": {
            "name": "Physical Geology",
            "author": [{"@type": "Person", "name": "Steven Earle"}]}},
        {"id": 3, "link": "https://books.test/logic/", "metadata": {
            "name": "A Concise Introduction to Logic",
            "author": [{"@type": "Person", "name": "Craig DeLancey"}]}}
    ]"#;

    /// A Catalog of [`SEARCHABLE_CATALOG`], already crawled into the cache.
    async fn crawled_searchable_catalog() -> (tempfile::TempDir, MockServer, PressbooksClient) {
        let (dir, pool) = temporary_pool();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-wp-total", "3")
                    .set_body_string(SEARCHABLE_CATALOG),
            )
            .mount(&server)
            .await;

        let client = PressbooksClient::with_network_base_url(pool, server.uri());
        client
            .list_catalog(|_, _| {})
            .await
            .expect("the Catalog should list");
        (dir, server, client)
    }

    #[tokio::test]
    async fn a_search_matches_title_subtitle_and_author_without_asking_the_network() {
        let (_dir, server, client) = crawled_searchable_catalog().await;
        let after_crawl = server
            .received_requests()
            .await
            .expect("mock server should record requests")
            .len();

        // Each term is lower case and each field is not, so a match is also a
        // match on case having been ignored.
        let by_title = client.search_catalog("logic").expect("title search");
        let by_subtitle = client.search_catalog("teaching").expect("subtitle search");
        let by_author = client.search_catalog("earle").expect("author search");

        assert_eq!(
            by_title
                .iter()
                .map(|book| book.book_url.as_str())
                .collect::<Vec<_>>(),
            ["https://books.test/logic/"]
        );
        assert_eq!(
            by_subtitle
                .iter()
                .map(|book| book.book_url.as_str())
                .collect::<Vec<_>>(),
            ["https://books.test/openteach/"]
        );
        assert_eq!(
            by_author
                .iter()
                .map(|book| book.book_url.as_str())
                .collect::<Vec<_>>(),
            ["https://books.test/geology/"]
        );

        // Every keystroke would otherwise cost the freshness check, which is
        // the whole reason the Catalog was enumerated locally in the first
        // place.
        assert_eq!(
            server
                .received_requests()
                .await
                .expect("mock server should record requests")
                .len(),
            after_crawl,
            "searching the local cache should cost no request"
        );
    }

    #[tokio::test]
    async fn an_empty_search_lists_the_whole_catalog_in_its_own_order() {
        // What clearing the search box has to restore.
        let (_dir, _server, client) = crawled_searchable_catalog().await;

        let all = client.search_catalog("").expect("empty search");
        let blank = client.search_catalog("   ").expect("whitespace search");

        assert_eq!(
            all.iter()
                .map(|book| book.title.as_str())
                .collect::<Vec<_>>(),
            // The Catalog's own order, which is not alphabetical.
            [
                "Openteach",
                "Physical Geology",
                "A Concise Introduction to Logic"
            ]
        );
        assert_eq!(blank.len(), 3, "whitespace is not a search term");
    }

    #[tokio::test]
    async fn a_term_no_book_matches_finds_nothing_rather_than_everything() {
        let (_dir, _server, client) = crawled_searchable_catalog().await;

        let found = client
            .search_catalog("thermodynamics")
            .expect("unmatched search");

        assert!(found.is_empty());
    }

    #[tokio::test]
    async fn a_wildcard_in_the_term_matches_that_character_rather_than_every_book() {
        // `%` and `_` mean something to LIKE and nothing to a reader, who is
        // typing characters they expect to find in a title.
        let (_dir, _server, client) = crawled_searchable_catalog().await;

        let percent = client.search_catalog("%").expect("percent search");
        let underscore = client
            .search_catalog("Physical_Geology")
            .expect("underscore search");

        assert!(
            percent.is_empty(),
            "no book has a percent sign in its title, subtitle or author"
        );
        assert!(
            underscore.is_empty(),
            "an underscore should not stand in for the space in a title"
        );
    }

    /// The LaTeX inside every `[[latex:…]]` token in a piece of text.
    fn decoded_latex(text: &str) -> Vec<String> {
        Regex::new(r"\[\[latex:([A-Za-z0-9+/=]+)\]\]")
            .expect("valid token regex")
            .captures_iter(text)
            .map(|captures| {
                String::from_utf8(
                    BASE64_STANDARD
                        .decode(captures[1].as_bytes())
                        .expect("the token payload should be base64"),
                )
                .expect("the token payload should be UTF-8")
            })
            .collect()
    }

    #[test]
    fn quicklatex_equations_become_mathematics_while_real_figures_keep_their_captions() {
        // Pressbooks renders equations to QuickLaTeX PNGs. Imported as figures
        // they would be pictures of mathematics a reader cannot hear; the LaTeX
        // the alt carries is what makes them readable and speakable instead.
        let html = r#"
            <p>The density ratio follows.</p>
            <p><img src="https://quicklatex.com/cache3/9210efab_l3.png"
                    class="ql-img-displayed-equation" alt="\[ \Theta=\Theta_g \]" /></p>
            <figure>
                <img src="https://books.test/uploads/soil-profile.jpg" alt="A soil profile" />
                <figcaption>Figure 2.1 A soil profile.</figcaption>
            </figure>
            <p>Sampling follows the same rule.</p>
        "#;

        let (paragraphs, images) = super::section_content_from_html(
            html,
            "https://books.test/logic/chapter/precise/",
            &super::PressbooksSource,
        );

        assert_eq!(paragraphs.len(), 3, "{paragraphs:?}");
        assert_eq!(paragraphs[0], "The density ratio follows.");
        assert_eq!(
            decoded_latex(&paragraphs[1]),
            vec![r"\[ \Theta=\Theta_g \]"]
        );
        assert_eq!(paragraphs[2], "Sampling follows the same rule.");

        assert_eq!(images.len(), 1, "the equation image should be dropped");
        assert_eq!(images[0].url, "https://books.test/uploads/soil-profile.jpg");
        assert_eq!(
            images[0].caption.as_deref(),
            Some("Figure 2.1 A soil profile.")
        );
        assert_eq!(
            images[0].anchor_paragraph_ordinal,
            Some(1),
            "a Figure is anchored to the Paragraph it followed"
        );
    }

    #[test]
    fn an_entity_encoded_alt_decodes_to_the_publishers_latex() {
        // This is how the alt actually arrives on the wire: every backslash and
        // bracket written as a numeric character reference.
        let html = concat!(
            r#"<p>Given <img src="https://books.test/wp-content/ql-cache/quicklatex.com-a_l3.png""#,
            r#" class="ql-img-inline-formula" "#,
            r#"alt="&#92;&#040;&#92;&#84;&#104;&#101;&#116;&#97;&#61;&#50;&#92;&#041;" /> exactly.</p>"#
        );

        let paragraphs =
            super::section_content_from_html(html, "https://books.test/", &super::PressbooksSource)
                .0;

        assert_eq!(decoded_latex(&paragraphs[0]), vec![r"\(\Theta=2\)"]);
        assert!(paragraphs[0].starts_with("Given"), "{paragraphs:?}");
        assert!(paragraphs[0].ends_with("exactly."), "{paragraphs:?}");
    }

    #[test]
    fn a_locally_cached_equation_is_recognised_by_its_class() {
        // Pressbooks serves QuickLaTeX images from its own host. Recognising
        // them only by a `quicklatex.com` src would miss the cached copies.
        let html = r#"<p><img src="https://books.test/wp-content/uploads/eq.png"
                              class="ql-img-displayed-formula quicklatex-auto-format"
                              alt="\[ E=mc^2 \]" /></p>"#;

        let (paragraphs, images) =
            super::section_content_from_html(html, "https://books.test/", &super::PressbooksSource);

        assert_eq!(decoded_latex(&paragraphs[0]), vec![r"\[ E=mc^2 \]"]);
        assert!(images.is_empty(), "{images:?}");
    }

    #[test]
    fn an_equation_image_carrying_no_latex_yields_no_token() {
        // QuickLaTeX writes its own name into the alt when it has no source to
        // put there. Encoding that would hand the reader a token that renders
        // as an advertisement.
        let html = r#"
            <p>Before. <img src="https://quicklatex.com/cache3/empty_l3.png"
                            class="ql-img-inline-formula" alt="" /></p>
            <p>After. <img src="https://quicklatex.com/cache3/boiler_l3.png"
                           class="ql-img-inline-formula" alt="Rendered by QuickLaTeX.com" /></p>
        "#;

        let (paragraphs, images) =
            super::section_content_from_html(html, "https://books.test/", &super::PressbooksSource);

        assert_eq!(paragraphs, vec!["Before.", "After."]);
        assert!(
            images.is_empty(),
            "the images are still dropped: {images:?}"
        );
    }

    #[tokio::test]
    async fn entries_the_table_of_contents_marks_as_empty_produce_no_section() {
        let (_dir, pool) = temporary_pool();
        let server = server_with_toc().await;
        let client = PressbooksClient::with_network_base_url(pool, server.uri());

        let entries = client
            .fetch_toc(&format!("{}/book", server.uri()))
            .await
            .expect("the table of contents should be readable");
        let ids = entries.iter().map(|entry| entry.id).collect::<Vec<_>>();

        // 10 has has_post_content: false, 22 has word_count: 0. Both flags come
        // from the publisher; neither is inferred from the HTML.
        assert!(
            !ids.contains(&10),
            "a title page should not become a Section"
        );
        assert!(
            !ids.contains(&22),
            "an entry with zero words should not become a Section"
        );
        assert_eq!(ids, vec![11, 21, 31]);
    }

    /// A Catalog whose table of contents never sends `word_count` at all --
    /// distinct from sending `word_count: 0`. `has_post_content` alone must
    /// decide, except where the publisher did send a measured zero.
    const TOC_JSON_NO_WORD_COUNTS: &str = r#"{
        "front-matter": [
            {"id": 10, "title": "Title Page", "has_post_content": false,
             "link": "https://books.test/logic/front-matter/title-page/"},
            {"id": 11, "title": "About This Book", "has_post_content": true,
             "link": "https://books.test/logic/front-matter/about/"}
        ],
        "parts": [
            {"title": "Part I", "has_post_content": false, "chapters": [
                {"id": 21, "title": "1. A Precise Language", "has_post_content": true,
                 "link": "https://books.test/logic/chapter/precise/"},
                {"id": 22, "title": "Measured Zero", "has_post_content": true, "word_count": 0,
                 "link": "https://books.test/logic/chapter/measured-zero/"}
            ]}
        ],
        "back-matter": [
            {"id": 31, "title": "Glossary", "has_post_content": true,
             "link": "https://books.test/logic/back-matter/glossary/"}
        ]
    }"#;

    async fn server_with_toc_missing_word_counts() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/toc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(TOC_JSON_NO_WORD_COUNTS))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_table_of_contents_that_omits_word_count_still_keeps_readable_entries() {
        let (_dir, pool) = temporary_pool();
        let server = server_with_toc_missing_word_counts().await;
        let client = PressbooksClient::with_network_base_url(pool, server.uri());

        let entries = client
            .fetch_toc(&format!("{}/book", server.uri()))
            .await
            .expect("the table of contents should be readable");
        let ids = entries.iter().map(|entry| entry.id).collect::<Vec<_>>();

        assert!(
            !ids.contains(&10),
            "has_post_content: false must still drop the entry with no word_count field"
        );
        assert!(
            !ids.contains(&22),
            "a measured zero must still drop the entry when the field is present"
        );
        assert_eq!(
            ids,
            vec![11, 21, 31],
            "has_post_content: true must keep an entry whose word_count was never sent"
        );
    }

    /// A chapter collection larger than one page, where the entries the table
    /// of contents keeps are spread across both.
    ///
    /// Sizing the crawl by the readable count -- four here -- fetches a single
    /// page and loses the two readable chapters sitting at collection positions
    /// 11 and 12. The collection's own `X-WP-TotalPages` is the only honest
    /// source for how far to page.
    async fn server_with_a_paged_collection() -> MockServer {
        let server = MockServer::start().await;
        let readable = [1_u64, 2, 11, 12];
        let toc_entries = (1..=12)
            .map(|id| {
                let words = if readable.contains(&id) { 100 } else { 0 };
                format!(
                    r#"{{"id": {id}, "title": "Chapter {id}", "has_post_content": true,
                        "word_count": {words}, "link": "https://books.test/c/{id}/"}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_string(metadata_json(None)))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/toc"))
            .respond_with(ResponseTemplate::new(200).set_body_string(format!(
                r#"{{"front-matter": [], "parts": [{{"chapters": [{toc_entries}]}}], "back-matter": []}}"#
            )))
            .mount(&server)
            .await;

        for (page, ids) in [(1, 1..=10_u64), (2, 11..=12_u64)] {
            let body = ids
                .map(|id| {
                    format!(
                        r#"{{"id": {id}, "content": {{"rendered": "<p>Chapter {id} says something.</p>"}}}}"#
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            Mock::given(method("GET"))
                .and(path("/book/wp-json/pressbooks/v2/chapters"))
                .and(query_param("page", page.to_string()))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("x-wp-totalpages", "2")
                        .set_body_string(format!("[{body}]")),
                )
                .mount(&server)
                .await;
        }

        server
    }

    #[tokio::test]
    async fn a_collection_longer_than_one_page_is_fetched_whole() {
        let (_dir, pool) = temporary_pool();
        let covers = covers_dir();
        let server = server_with_a_paged_collection().await;

        let document = super::import_book(
            pool,
            &format!("{}/book", server.uri()),
            covers.path(),
            |_, _| Ok(()),
        )
        .await
        .expect("a book whose chapters span two pages should import");

        assert_eq!(
            document
                .sections
                .iter()
                .map(|section| section.title.as_str())
                .collect::<Vec<_>>(),
            vec!["Chapter 1", "Chapter 2", "Chapter 11", "Chapter 12"],
            "chapters past the first page must not be lost"
        );
    }

    #[tokio::test]
    async fn a_catalog_that_will_not_say_how_big_it_is_fails_rather_than_listing_ten_books() {
        let (_dir, pool) = temporary_pool();
        let server = MockServer::start().await;
        // No x-wp-total. Reading that as zero would store one page as if it
        // were the whole Catalog and re-crawl it on every visit.
        Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(catalog_page("https://books.test/openteach/", "Openteach")),
            )
            .mount(&server)
            .await;

        let client = PressbooksClient::with_network_base_url(pool, server.uri());
        let Err(error) = client.list_catalog(|_, _| {}).await else {
            panic!("a Catalog with no count should fail rather than look ten books long");
        };

        assert_eq!(error.kind(), "pressbooks");
    }

    #[tokio::test]
    async fn a_catalog_already_crawled_still_lists_when_the_network_is_gone() {
        let (_dir, pool) = temporary_pool();
        let server = MockServer::start().await;
        let mock = Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-wp-total", "1")
                    .set_body_string(catalog_page("https://books.test/openteach/", "Openteach")),
            )
            .up_to_n_times(2)
            .mount_as_scoped(&server)
            .await;

        let client = PressbooksClient::with_network_base_url(pool, server.uri());
        client
            .list_catalog(|_, _| {})
            .await
            .expect("the Catalog should list");
        drop(mock);

        // Every later request now 404s -- the freshness check cannot run.
        let books = client
            .list_catalog(|_, _| {})
            .await
            .expect("a Catalog already on disk should still list with no network");

        assert_eq!(books.books.len(), 1);
        assert_eq!(books.books[0].title, "Openteach");
    }

    #[test]
    fn the_bundled_list_offers_catalogs_and_records_the_excluded_ones() {
        let catalogs = super::catalogs().expect("the bundled Catalog list should parse");

        assert!(
            catalogs.len() >= 10,
            "the picker should offer more than a token few, got {}",
            catalogs.len()
        );
        assert!(
            catalogs
                .iter()
                .any(|c| c.host == super::DEFAULT_NETWORK_HOST),
            "the Catalog the browser opens on must be one it offers"
        );
        assert!(
            catalogs
                .iter()
                .all(|c| !c.host.trim().is_empty() && !c.name.trim().is_empty()),
            "every Catalog needs a host to reach and a name to show"
        );

        let mut hosts = catalogs.iter().map(|c| c.host.as_str()).collect::<Vec<_>>();
        hosts.sort_unstable();
        let unique = hosts.len();
        hosts.dedup();
        assert_eq!(hosts.len(), unique, "a Catalog is listed twice");
    }

    #[test]
    fn exactly_one_offered_catalog_is_the_one_the_browser_opens_on() {
        let offered = super::catalogs().expect("the bundled list should parse");

        let default = offered
            .iter()
            .filter(|catalog| catalog.is_default)
            .collect::<Vec<_>>();

        assert_eq!(
            default.len(),
            1,
            "the browser opens on exactly one Catalog, not {}",
            default.len()
        );
    }

    #[test]
    fn the_catalog_the_browser_opens_on_is_a_small_one() {
        // The point of naming a default at all. Opening on the largest bundled
        // Catalog costs three hundred requests before the reader has asked for
        // anything, and there is no progress indicator yet to carry that.
        let offered = super::catalogs().expect("the bundled list should parse");
        let largest = offered
            .iter()
            .map(|catalog| catalog.book_count)
            .max()
            .expect("the bundled list should offer a Catalog");

        let default = offered
            .iter()
            .find(|catalog| catalog.is_default)
            .expect("one Catalog should be the one the browser opens on");

        assert!(
            default.book_count <= 100,
            "{} holds {} books, so a first visit crawls {} pages",
            default.host,
            default.book_count,
            default.book_count.div_ceil(super::PER_PAGE as u32)
        );
        assert!(
            default.book_count < largest,
            "the default should not be the largest Catalog on offer"
        );
    }

    #[test]
    fn every_excluded_catalog_records_why_and_is_not_also_offered() {
        // The point of recording exclusions is that nobody re-adds a Catalog
        // nobody can reach after rediscovering the problem by hand. An entry
        // with no reason, or one that also appears in the offered list, fails
        // at exactly that.
        #[derive(serde::Deserialize)]
        struct Bundled {
            networks: Vec<super::PressbooksCatalog>,
            excluded: Vec<Excluded>,
        }
        #[derive(serde::Deserialize)]
        struct Excluded {
            host: String,
            reason: String,
        }

        let bundled: Bundled = serde_json::from_str(include_str!(
            "../../resources/catalog/pressbooks-networks.json"
        ))
        .expect("the bundled Catalog list should parse");

        assert!(!bundled.excluded.is_empty());
        for excluded in &bundled.excluded {
            assert!(
                excluded.reason.trim().len() > 20,
                "{} is excluded without saying why",
                excluded.host
            );
            assert!(
                !bundled.networks.iter().any(|c| c.host == excluded.host),
                "{} is both offered and excluded",
                excluded.host
            );
        }
    }

    /// One page of a Catalog: `count` books, numbered from `first`.
    fn catalog_books(first: u32, count: u32) -> String {
        let books = (first..first + count)
            .map(|n| {
                format!(
                    r#"{{"id": {n}, "link": "https://books.test/book-{n}/", "metadata": {{
                        "name": "Book {n}",
                        "author": [{{"@type": "Person", "name": "A Writer"}}]
                    }}}}"#
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("[{books}]")
    }

    /// A Catalog of 25 books over three pages, whose third page fails.
    ///
    /// Three pages is more than one and fewer than the concurrency limit, so
    /// every page is in flight at once -- which is exactly the case where a
    /// crawl that stops has to decide what to keep.
    async fn server_with_a_failing_third_page() -> (MockServer, wiremock::MockGuard) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .and(query_param("per_page", "1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-wp-total", "25")
                    .set_body_string(catalog_books(1, 1)),
            )
            .mount(&server)
            .await;
        // Scoped, and mounted ahead of the working page below it: dropping the
        // guard is what lets the same Catalog be resumed rather than mocked
        // twice.
        let failing = Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .and(query_param("page", "3"))
            .respond_with(ResponseTemplate::new(500))
            .mount_as_scoped(&server)
            .await;
        for page in [1_u32, 2, 3] {
            Mock::given(method("GET"))
                .and(path("/wp-json/pressbooks/v2/books"))
                .and(query_param("page", page.to_string()))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("x-wp-total", "25")
                        .set_body_string(catalog_books(
                            (page - 1) * 10 + 1,
                            if page == 3 { 5 } else { 10 },
                        )),
                )
                .mount(&server)
                .await;
        }
        (server, failing)
    }

    #[tokio::test]
    async fn an_interrupted_crawl_lists_what_it_fetched_and_says_it_is_incomplete() {
        // Twenty books that arrived are worth more to a reader than an error
        // about the five that did not -- as long as the Catalog does not then
        // claim to hold twenty.
        let (_dir, pool) = temporary_pool();
        let (server, _failing) = server_with_a_failing_third_page().await;
        let client = PressbooksClient::with_network_base_url(pool, server.uri());

        let listing = client
            .list_catalog(|_, _| {})
            .await
            .expect("a Catalog that stopped part way should still list");

        assert_eq!(
            listing.books.len(),
            20,
            "the pages that arrived should list"
        );
        assert_eq!(
            listing.total_books, 25,
            "the Catalog's real size, not the part that arrived"
        );
        assert!(
            !listing.is_complete,
            "a Catalog missing a page is not complete"
        );
    }

    #[tokio::test]
    async fn a_crawl_reports_progress_that_advances_to_the_end() {
        // Three hundred requests with no sign of movement is indistinguishable
        // from a frozen application.
        let (_dir, pool) = temporary_pool();
        let (server, failing) = server_with_a_failing_third_page().await;
        drop(failing);
        let client = PressbooksClient::with_network_base_url(pool, server.uri());

        let mut reported = Vec::new();
        client
            .list_catalog(|current, total| reported.push((current, total)))
            .await
            .expect("the Catalog should list");

        assert_eq!(
            reported.first(),
            Some(&(0, 3)),
            "progress should start from nothing done, got {reported:?}"
        );
        assert_eq!(
            reported.last(),
            Some(&(3, 3)),
            "progress should finish at the last page, got {reported:?}"
        );
        assert!(
            reported.windows(2).all(|pair| pair[0].0 <= pair[1].0),
            "progress should never go backwards, got {reported:?}"
        );
    }

    #[tokio::test]
    async fn a_resumed_crawl_reports_progress_from_where_it_stopped() {
        // A resume that reported from zero would show a reader work being
        // redone that is not being redone.
        let (_dir, pool) = temporary_pool();
        let (server, failing) = server_with_a_failing_third_page().await;
        let client = PressbooksClient::with_network_base_url(pool, server.uri());
        client
            .list_catalog(|_, _| {})
            .await
            .expect("the first attempt should list what it fetched");
        drop(failing);

        let mut reported = Vec::new();
        client
            .list_catalog(|current, total| reported.push((current, total)))
            .await
            .expect("the Catalog should finish");

        assert_eq!(
            reported.first(),
            Some(&(2, 3)),
            "a resume should start from the pages already fetched, got {reported:?}"
        );
        assert_eq!(reported.last(), Some(&(3, 3)));
    }

    /// A Catalog of 25 books over three pages, whose last page is slow.
    ///
    /// The delay is what opens a window to look at the database while the crawl
    /// is still running.
    async fn server_with_a_slow_last_page(delay: Duration) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .and(query_param("per_page", "1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-wp-total", "25")
                    .set_body_string(catalog_books(1, 1)),
            )
            .mount(&server)
            .await;
        for page in [1_u32, 2, 3] {
            let mut response = ResponseTemplate::new(200)
                .insert_header("x-wp-total", "25")
                .set_body_string(catalog_books(
                    (page - 1) * 10 + 1,
                    if page == 3 { 5 } else { 10 },
                ));
            if page == 3 {
                response = response.set_delay(delay);
            }
            Mock::given(method("GET"))
                .and(path("/wp-json/pressbooks/v2/books"))
                .and(query_param("page", page.to_string()))
                .respond_with(response)
                .mount(&server)
                .await;
        }
        server
    }

    #[tokio::test]
    async fn pages_are_committed_as_they_arrive_rather_than_at_the_end() {
        // The interruption worth surviving is not only a failed request. A
        // crawl of the largest bundled Catalog runs for minutes, and a reader
        // who closes the application during it would otherwise lose every page
        // -- "resumable" would mean only "survives an HTTP error".
        let (_dir, pool) = temporary_pool();
        let server = server_with_a_slow_last_page(Duration::from_secs(30)).await;
        let crawling = PressbooksClient::with_network_base_url(pool.clone(), server.uri());
        // A second client on the same pool, standing in for the next launch:
        // it can only see what has actually been committed.
        let onlooker = PressbooksClient::with_network_base_url(pool, server.uri());

        let crawl = tokio::spawn(async move { crawling.list_catalog(|_, _| {}).await });

        let mut landed = 0;
        for _ in 0..100 {
            landed = onlooker
                .cached_listing()
                .expect("the cache should read")
                .books
                .len();
            if landed >= 20 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        assert_eq!(
            landed, 20,
            "the pages that arrived should be on disk before the crawl ends"
        );
        assert!(
            !crawl.is_finished(),
            "this has to be observed while the crawl is still running, or it \
             proves nothing about when the write happened"
        );
        crawl.abort();
    }

    #[tokio::test]
    async fn two_openings_of_one_catalog_do_not_crawl_it_twice() {
        // Ordinary navigation unmounts and remounts the browser, so this is
        // one reader switching tabs -- not a contrived race. Two crawls would
        // double the load on a third party this file caps concurrency for.
        let (_dir, pool) = temporary_pool();
        let server = server_with_a_slow_last_page(Duration::from_millis(200)).await;
        let first = PressbooksClient::with_network_base_url(pool.clone(), server.uri());
        let second = PressbooksClient::with_network_base_url(pool, server.uri());

        let (one, two) = tokio::join!(
            first.list_catalog(|_, _| {}),
            second.list_catalog(|_, _| {})
        );

        assert_eq!(one.expect("the first should list").books.len(), 25);
        assert_eq!(two.expect("the second should list").books.len(), 25);
        let crawled = server
            .received_requests()
            .await
            .expect("mock server should record requests")
            .iter()
            .filter(|request| {
                request
                    .url
                    .query()
                    .is_some_and(|query| query.contains(&format!("per_page={}", super::PER_PAGE)))
            })
            .count();
        assert_eq!(
            crawled, 3,
            "the Catalog should be enumerated once, not once per opening"
        );
    }

    #[tokio::test]
    async fn a_partial_catalog_that_cannot_be_extended_reports_the_failure() {
        // Otherwise Continue loading is a button that silently does nothing:
        // the reader clicks, the same partial listing comes back, and there is
        // no way to tell that from a button that is not wired up.
        let (_dir, pool) = temporary_pool();
        let (server, failing) = server_with_a_failing_third_page().await;
        let client = PressbooksClient::with_network_base_url(pool, server.uri());
        client
            .list_catalog(|_, _| {})
            .await
            .expect("the first attempt should list what it fetched");

        // Released before the reset: a scoped mock outliving the set it was
        // mounted into panics when it is dropped.
        drop(failing);
        // The Catalog stops answering at all, which is what a reader clicking
        // Continue on a train sees.
        server.reset().await;
        let error = client
            .list_catalog(|_, _| {})
            .await
            .expect_err("a Catalog that is not whole and cannot be extended should say so");

        assert_eq!(error.kind(), "pressbooks");
    }

    #[tokio::test]
    async fn a_resumed_crawl_asks_only_for_the_pages_it_is_missing() {
        // The point of recording a position. Starting again would cost the
        // largest bundled Catalog three hundred requests it has already made.
        let (_dir, pool) = temporary_pool();
        let (server, failing) = server_with_a_failing_third_page().await;
        let client = PressbooksClient::with_network_base_url(pool, server.uri());
        client
            .list_catalog(|_, _| {})
            .await
            .expect("the first attempt should list what it fetched");
        let before_resume = server
            .received_requests()
            .await
            .expect("mock server should record requests")
            .len();
        drop(failing);

        let listing = client
            .list_catalog(|_, _| {})
            .await
            .expect("the Catalog should finish");

        assert_eq!(listing.books.len(), 25, "the whole Catalog should be there");
        assert!(listing.is_complete);
        let resumed = server
            .received_requests()
            .await
            .expect("mock server should record requests")
            .split_off(before_resume);
        let asked_for = resumed
            .iter()
            .filter_map(|request| request.url.query().map(str::to_string))
            .collect::<Vec<_>>();
        assert_eq!(
            asked_for.len(),
            2,
            "a resume should cost the count check and the one missing page, got {asked_for:?}"
        );
        // The freshness check is itself a page request -- `per_page=1&page=1`
        // -- so the crawled pages are the ones asking for a full page.
        let crawled = asked_for
            .iter()
            .filter(|query| query.contains(&format!("per_page={}", super::PER_PAGE)))
            .collect::<Vec<_>>();
        assert_eq!(
            crawled,
            [&format!("per_page={}&page=3", super::PER_PAGE)],
            "only the page that was missing should be fetched"
        );
    }

    #[tokio::test]
    async fn a_crawl_that_fails_part_way_leaves_the_cached_catalog_listing() {
        let (_dir, pool) = temporary_pool();
        let server = MockServer::start().await;

        // First visit: one book, crawled and cached.
        let first = Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-wp-total", "1")
                    .set_body_string(catalog_page("https://books.test/openteach/", "Openteach")),
            )
            .mount_as_scoped(&server)
            .await;
        let client = PressbooksClient::with_network_base_url(pool, server.uri());
        client
            .list_catalog(|_, _| {})
            .await
            .expect("the Catalog should list");
        drop(first);

        // The Catalog has grown, so a crawl starts -- and then falls over. The
        // count check succeeding is what makes this different from having no
        // network at all.
        Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .and(query_param("per_page", "1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-wp-total", "2")
                    .set_body_string(catalog_page("https://books.test/openteach/", "Openteach")),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/wp-json/pressbooks/v2/books"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let books = client
            .list_catalog(|_, _| {})
            .await
            .expect("a crawl that failed should leave the cached Catalog listing");

        // The crawl commits per page and this one got no page at all, so the
        // rows from the first visit are untouched on disk.
        assert_eq!(books.books.len(), 1);
        assert_eq!(books.books[0].title, "Openteach");
    }

    #[tokio::test]
    async fn each_catalog_caches_independently() {
        let (_dir, pool) = temporary_pool();
        let first = MockServer::start().await;
        let second = MockServer::start().await;

        for (server, title) in [(&first, "Openteach"), (&second, "Logic")] {
            Mock::given(method("GET"))
                .and(path("/wp-json/pressbooks/v2/books"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .insert_header("x-wp-total", "1")
                        .set_body_string(catalog_page(
                            &format!("https://books.test/{title}/"),
                            title,
                        )),
                )
                .mount(server)
                .await;
        }

        let one = PressbooksClient::with_network_base_url(pool.clone(), first.uri());
        let two = PressbooksClient::with_network_base_url(pool, second.uri());

        one.list_catalog(|_, _| {})
            .await
            .expect("the first should list");
        two.list_catalog(|_, _| {})
            .await
            .expect("the second should list");

        // Crawling the second must not have emptied the first: a reader
        // switching back and forth would otherwise pay a fresh crawl each way.
        let back = one
            .list_catalog(|_, _| {})
            .await
            .expect("the first should still list after the second was crawled");
        let requests = first
            .received_requests()
            .await
            .expect("mock server should record requests")
            .len();

        assert_eq!(back.books.len(), 1);
        assert_eq!(back.books[0].title, "Openteach");
        assert_eq!(
            requests, 3,
            "returning to a Catalog should cost only its freshness check"
        );
    }

    #[test]
    fn every_offered_catalog_can_show_its_thumbnails() {
        // The bundled list and the `img-src` CSP are twins with no link between
        // them. Adding a Catalog without widening the policy leaves its cards
        // rendering placeholders, with the refusal visible only in a devtools
        // console nobody opens -- CLAUDE.md flags this class of failure.
        let config = include_str!("../../tauri.conf.json");
        let img_src = config
            .split("img-src ")
            .nth(1)
            .and_then(|rest| rest.split(';').next())
            .expect("tauri.conf.json should carry an img-src policy");

        for catalog in super::catalogs().expect("the bundled Catalog list should parse") {
            let allowed = img_src.contains(&format!("https://{}", catalog.host))
                || catalog
                    .host
                    .split_once('.')
                    .is_some_and(|(_, domain)| img_src.contains(&format!("https://*.{domain}")));

            assert!(
                allowed,
                "{} is offered in the picker but its thumbnails are blocked by img-src",
                catalog.host
            );
        }
    }

    #[tokio::test]
    async fn a_host_the_app_does_not_offer_is_refused() {
        let (_dir, pool) = temporary_pool();

        // The host arrives from the webview and becomes part of every URL the
        // crawl requests, so it is checked rather than trusted.
        let Err(error) = super::list_books(pool, "evil.example.com", |_, _| {}).await else {
            panic!("an unoffered host should be refused");
        };

        assert_eq!(error.kind(), "pressbooks");
        assert!(error.message().contains("evil.example.com"));
    }

    #[test]
    fn a_book_url_outside_the_offered_catalogs_is_refused() {
        // The Import path builds every request from this URL, so the same
        // guard the listing path applies has to cover it.
        for url in [
            "http://internal-host/secret",
            "https://evil.example.com/book/",
            "not-a-url",
        ] {
            let Err(error) = super::verify_offered_book_url(url) else {
                panic!("{url} should be refused");
            };
            assert_eq!(error.kind(), "pressbooks");
        }

        super::verify_offered_book_url(
            "https://milnepublishing.geneseo.edu/concise-introduction-to-logic/",
        )
        .expect("a book in an offered Catalog should be allowed");
    }

    #[test]
    fn an_offered_host_with_a_disallowed_scheme_port_or_userinfo_is_refused() {
        // The host check alone is not enough: every field of the URL becomes
        // part of every request the Import makes, and none of them is any
        // more trusted than the host is.
        for url in [
            "http://milnepublishing.geneseo.edu/concise-introduction-to-logic/",
            "https://milnepublishing.geneseo.edu:8443/concise-introduction-to-logic/",
            "https://user:pass@milnepublishing.geneseo.edu/concise-introduction-to-logic/",
        ] {
            let Err(error) = super::verify_offered_book_url(url) else {
                panic!("{url} should be refused");
            };
            assert_eq!(error.kind(), "pressbooks");
        }

        super::verify_offered_book_url(
            "https://milnepublishing.geneseo.edu/concise-introduction-to-logic/",
        )
        .expect("an https book URL on the default port with no userinfo should still be allowed");
    }

    /// The one test that proves the recorded wire format is still the real one.
    ///
    /// Excluded from the default run: it reaches Milne over the network. Run it
    /// with `cargo test -p libretexts-reader live_imports_a_small_pressbooks_book
    /// -- --ignored --nocapture`.
    ///
    /// It downloads real Figures into the real app-data images directory,
    /// because that is what an Import does and this test exists to exercise the
    /// real path. The files it created are deleted at the end by path rather
    /// than by pointing the whole app-data directory elsewhere -- `set_var` is
    /// process-global, and the LibreTexts live test already uses it.
    #[tokio::test]
    #[ignore]
    async fn live_imports_a_small_pressbooks_book() {
        let (dir, pool) = temporary_pool();
        let covers = covers_dir();
        let client = PressbooksClient::new(pool.clone());

        let listing = client
            .list_catalog(|_, _| {})
            .await
            .expect("the Milne Catalog should list");
        let books = listing.books;
        assert!(
            books.len() > 50,
            "Milne should return a whole Catalog, got {}",
            books.len()
        );
        assert!(
            books.iter().all(|book| !book.title.trim().is_empty()),
            "every catalogued book should carry a title"
        );

        let book_url = "https://milnepublishing.geneseo.edu/concise-introduction-to-logic/";
        let document =
            super::import_book(pool.clone(), book_url, covers.path(), |current, total| {
                eprintln!("Pressbooks smoke import progress: {current}/{total}");
                Ok(())
            })
            .await
            .expect("a small public Pressbooks book should import");

        assert_eq!(document.title, "A Concise Introduction to Logic");
        assert!(!document.sections.is_empty());
        assert!(
            document.license.is_some(),
            "the licence should travel with the Document"
        );
        assert!(
            document.attribution.is_some(),
            "the attribution should travel with the Document"
        );

        // This book is full of QuickLaTeX equations, which is the whole reason
        // it is the one imported live: they must arrive as notation, and none
        // of them may arrive as a picture.
        let paragraphs = document
            .sections
            .iter()
            .flat_map(|section| &section.paragraphs)
            .filter(|paragraph| paragraph.contains("[[latex:"))
            .count();
        assert!(
            paragraphs > 10,
            "the book's equations should reach the reader as notation, got {paragraphs}"
        );
        assert!(
            document
                .sections
                .iter()
                .flat_map(|section| &section.images)
                .all(|image| !image.source_url.contains("quicklatex")),
            "no equation should be imported as a Figure"
        );

        for image in document.sections.iter().flat_map(|section| &section.images) {
            let _ = std::fs::remove_file(&image.local_path);
        }

        // Persisted and listed back, not just built. Stopping at the
        // `DocumentBuilder` is how this test previously missed that
        // `documents.source_type` carries a CHECK constraint naming every
        // Source: the Import assembled fine and could not be saved at all.
        let mut conn = pool.get().expect("a connection should be available");
        let document_id = document
            .persist(&mut conn)
            .expect("the Document should persist");
        let listed = crate::db::library::list_documents(&conn)
            .expect("the Library should list after a Pressbooks Import");
        assert!(listed.iter().any(|d| d.id == document_id));

        crate::db::library::delete_document(&conn, &document_id)
            .expect("the temporary Document should delete");
        drop(conn);
        drop(dir);
    }
}
