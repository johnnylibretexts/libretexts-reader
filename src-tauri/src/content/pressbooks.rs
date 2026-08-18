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

use std::time::Duration;

use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use scraper::ElementRef;
use serde::Deserialize;
use serde_json::json;

use crate::content::document::{DocumentBuilder, SectionBuilder};
use crate::content::html_section::{section_content_from_html, SectionSource};
use crate::content::images::download_images;
use crate::content::remote;
use crate::db::connection::DbPool;
use crate::db::models::{PressbooksBook, PressbooksCatalog, SourceType};
use crate::error::{AppError, AppResult};

/// The Catalog the browser opens on: the smallest bundled one, so a first
/// visit is nine requests rather than three hundred.
pub const DEFAULT_NETWORK_HOST: &str = "milnepublishing.geneseo.edu";

const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Pressbooks networks sit behind a WAF that rejects a bare tool name with 403
/// and an HTML error page — `libretexts-reader-pressbooks-importer` never
/// reaches the API. The `Mozilla/5.0 (compatible; …)` form is the convention
/// the User-Agent header was designed for: it names this client honestly
/// without impersonating a specific browser, and it is accepted.
const USER_AGENT: &str = "Mozilla/5.0 (compatible; LibreTexts Reader/0.1.0)";

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
    #[serde(default)]
    word_count: u32,
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
    pub async fn list_catalog(&self) -> AppResult<Vec<PressbooksBook>> {
        let cached = self.cached_books()?;

        match self.live_book_count().await {
            Ok(live_total) => {
                if live_total != self.cached_book_count()? {
                    self.crawl_catalog(live_total).await?;
                    return self.cached_books();
                }
                Ok(cached)
            }
            Err(_) if !cached.is_empty() => Ok(cached),
            Err(error) => Err(error),
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

    async fn crawl_catalog(&self, total_books: u32) -> AppResult<()> {
        let mut network_name = None;
        let total_pages = total_books.div_ceil(PER_PAGE as u32).max(1);
        let pages = (1..=total_pages).collect::<Vec<_>>();

        let fetched = remote::fetch_all(
            &pages,
            CATALOG_CONCURRENCY,
            |_| {},
            |page: u32| async move {
                let request = self.request(self.books_url()).query(&[
                    ("per_page", PER_PAGE.to_string()),
                    ("page", page.to_string()),
                ]);
                self.fetch_json::<Vec<ApiCatalogBook>>(&request).await
            },
        )
        .await?;

        let mut conn = self.db.get()?;
        let tx = conn.transaction()?;
        // Replaced wholesale rather than merged: a book withdrawn from the
        // Catalog should leave the reader's view of it, and the crawl just
        // established what the Catalog currently holds.
        tx.execute(
            "DELETE FROM pressbooks_book WHERE host = ?1",
            params![self.host],
        )?;

        let mut position = 0_i64;
        for book in fetched.into_iter().flatten() {
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
                    authors, license_name, license_url, word_count, menu_position
                 )
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                ],
            )?;
            position += 1;
        }

        tx.execute(
            "INSERT OR REPLACE INTO pressbooks_network
                 (host, name, total_books, synced_pages, total_pages, synced_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                self.host,
                network_name.unwrap_or_else(|| self.host.clone()),
                total_books,
                total_pages,
                total_pages,
                Utc::now().to_rfc3339()
            ],
        )?;
        tx.commit()?;

        Ok(())
    }

    fn cached_book_count(&self) -> AppResult<u32> {
        let conn = self.db.get()?;
        let total = conn
            .query_row(
                "SELECT total_books FROM pressbooks_network WHERE host = ?1",
                params![self.host],
                |row| row.get::<_, u32>(0),
            )
            .optional()?;
        Ok(total.unwrap_or(0))
    }

    fn cached_books(&self) -> AppResult<Vec<PressbooksBook>> {
        let conn = self.db.get()?;
        let mut statement = conn.prepare(
            "SELECT book_url, title, subtitle, cover_url, thumbnail_url,
                    authors, license_name, license_url, word_count
             FROM pressbooks_book WHERE host = ?1
             ORDER BY menu_position",
        )?;
        let books = statement
            .query_map(params![self.host], |row| {
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
                |_| {},
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
    if !entry.has_post_content || entry.word_count == 0 {
        return;
    }

    entries.push(TocEntry {
        id: entry.id,
        title: entry.title.trim().to_string(),
        kind,
        link: entry.link,
    });
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Pressbooks renders equations to QuickLaTeX images. Until they are decoded
/// into speakable notation, importing them would produce mute figures a reader
/// cannot hear, so they are dropped rather than kept as pictures.
struct PressbooksSource;

impl SectionSource for PressbooksSource {
    fn should_skip_paragraph(&self, _element: &ElementRef<'_>) -> bool {
        false
    }

    fn should_skip_image(&self, element: &ElementRef<'_>) -> bool {
        element
            .value()
            .attr("src")
            .is_some_and(|src| src.contains("quicklatex.com"))
    }
}

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
    Ok(bundled.networks)
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

pub async fn list_books(db: DbPool, host: &str) -> AppResult<Vec<PressbooksBook>> {
    let host = offered_host(host)?;
    PressbooksClient::with_network_base_url(db, format!("https://{host}"))
        .list_catalog()
        .await
}

pub async fn import_book<F>(
    db: DbPool,
    book_url: &str,
    mut on_progress: F,
) -> AppResult<DocumentBuilder>
where
    F: FnMut(u32, u32) + Send,
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
    on_progress(0, total);
    let pages = client.fetch_content(book_url, &entries).await?;

    let mut sections = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        on_progress(index as u32 + 1, total);

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
        cover_image_path: None,
        license: clean(Some(license.name)),
        attribution: clean(Some(authors)),
        sections,
    })
}

#[cfg(test)]
mod tests {
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{EntryKind, PressbooksClient};
    use crate::db::connection::temporary_pool;
    use crate::db::models::SourceType;

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

    const METADATA_JSON: &str = r#"{
        "name": "A Concise Introduction to Logic",
        "author": [{"@type": "Person", "name": "Craig DeLancey"},
                   {"@type": "Person", "name": "  "}],
        "license": {"@type": "CreativeWork", "name": "CC BY-NC-SA (Attribution NonCommercial ShareAlike)",
                    "url": "https://creativecommons.org/licenses/by-nc-sa/4.0/"},
        "image": "https://books.test/cover.png"
    }"#;

    fn content_body(id: u64, html: &str) -> String {
        format!(
            r#"[{{"id": {id}, "content": {{"rendered": {}}}}}]"#,
            serde_json::json!(html)
        )
    }

    /// A whole book, served the way the API serves one: metadata, a table of
    /// contents, then one collection endpoint per kind of entry.
    async fn server_with_book() -> MockServer {
        let server = server_with_toc().await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_string(METADATA_JSON))
            .mount(&server)
            .await;
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
        let server = server_with_book().await;

        let document = super::import_book(pool, &format!("{}/book", server.uri()), |_, _| {})
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
    async fn the_licence_attribution_and_book_url_travel_with_the_document() {
        let (_dir, pool) = temporary_pool();
        let server = server_with_book().await;
        let book_url = format!("{}/book", server.uri());

        let document = super::import_book(pool, &book_url, |_, _| {})
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
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/book/wp-json/pressbooks/v2/metadata"))
            .respond_with(ResponseTemplate::new(200).set_body_string(METADATA_JSON))
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

        let Err(error) =
            super::import_book(pool, &format!("{}/book", server.uri()), |_, _| {}).await
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
            .list_catalog()
            .await
            .expect("the Catalog should list");
        let after_crawl = server
            .received_requests()
            .await
            .expect("mock server should record requests")
            .len();
        let second = client
            .list_catalog()
            .await
            .expect("the Catalog should list again");
        let after_second = server
            .received_requests()
            .await
            .expect("mock server should record requests")
            .len();

        assert_eq!(first.len(), 1);
        assert_eq!(first[0].title, "Openteach");
        assert_eq!(first[0].book_url, "https://books.test/openteach/");
        assert_eq!(first[0].subtitle.as_deref(), Some("A subtitle"));
        assert_eq!(first[0].authors, "Orna Farrell");
        assert_eq!(first[0].license_name, "CC BY (Attribution)");
        assert_eq!(first[0].word_count, 25637);

        // The second visit costs one request -- the freshness check -- not a
        // second crawl. Re-enumerating on every visit is what the local cache
        // exists to avoid.
        assert_eq!(
            second.len(),
            1,
            "the cached Catalog should still list its books"
        );
        assert_eq!(
            after_second - after_crawl,
            1,
            "a fresh Catalog should cost only the count check"
        );
    }

    #[test]
    fn quicklatex_equations_are_dropped_while_real_figures_keep_their_captions() {
        // Pressbooks renders equations to QuickLaTeX PNGs. Imported as figures
        // they would be pictures of mathematics a reader cannot hear.
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

        assert_eq!(
            paragraphs,
            vec![
                "The density ratio follows.",
                "Sampling follows the same rule."
            ]
        );
        assert_eq!(images.len(), 1, "the equation image should be dropped");
        assert_eq!(images[0].url, "https://books.test/uploads/soil-profile.jpg");
        assert_eq!(
            images[0].caption.as_deref(),
            Some("Figure 2.1 A soil profile.")
        );
        assert_eq!(
            images[0].anchor_paragraph_ordinal,
            Some(0),
            "a Figure is anchored to the Paragraph it followed"
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
            .respond_with(ResponseTemplate::new(200).set_body_string(METADATA_JSON))
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
        let server = server_with_a_paged_collection().await;

        let document = super::import_book(pool, &format!("{}/book", server.uri()), |_, _| {})
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
        let Err(error) = client.list_catalog().await else {
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
            .list_catalog()
            .await
            .expect("the Catalog should list");
        drop(mock);

        // Every later request now 404s -- the freshness check cannot run.
        let books = client
            .list_catalog()
            .await
            .expect("a Catalog already on disk should still list with no network");

        assert_eq!(books.len(), 1);
        assert_eq!(books[0].title, "Openteach");
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

        one.list_catalog().await.expect("the first should list");
        two.list_catalog().await.expect("the second should list");

        // Crawling the second must not have emptied the first: a reader
        // switching back and forth would otherwise pay a fresh crawl each way.
        let back = one
            .list_catalog()
            .await
            .expect("the first should still list after the second was crawled");
        let requests = first
            .received_requests()
            .await
            .expect("mock server should record requests")
            .len();

        assert_eq!(back.len(), 1);
        assert_eq!(back[0].title, "Openteach");
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
        let Err(error) = super::list_books(pool, "evil.example.com").await else {
            panic!("an unoffered host should be refused");
        };

        assert_eq!(error.kind(), "pressbooks");
        assert!(error.message().contains("evil.example.com"));
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
        let client = PressbooksClient::new(pool.clone());

        let books = client
            .list_catalog()
            .await
            .expect("the Milne Catalog should list");
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
        let document = super::import_book(pool, book_url, |current, total| {
            eprintln!("Pressbooks smoke import progress: {current}/{total}");
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

        for image in document.sections.iter().flat_map(|section| &section.images) {
            let _ = std::fs::remove_file(&image.local_path);
        }
        drop(dir);
    }
}
