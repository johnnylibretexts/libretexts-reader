use std::collections::HashSet;
use std::io::{Read, Write};
use std::sync::OnceLock;
use std::time::Duration;

use chrono::Utc;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use regex::Regex;
use reqwest::StatusCode;
use reqwest::Url;
use rusqlite::{params, OptionalExtension};
use scraper::{ElementRef, Html, Selector};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::content::document::{DocumentBuilder, SectionBuilder};
use crate::content::html_section::{self, normalize_text, SectionSource};
use crate::content::images::{download_images, source_images_from_html, SourceImage};
use crate::db::connection::DbPool;
use crate::db::models::{LibreTextsBook, LibreTextsLibrary, SourceType};
use crate::error::{AppError, AppResult};

const DEFAULT_COMMONS_BASE_URL: &str = "https://commons.libretexts.org";
const MAX_RETRIES: usize = 3;

static BOOK_ID_RE: OnceLock<Regex> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct LibreTextsClient {
    http: reqwest::Client,
    db: DbPool,
    commons_base_url: String,
}

#[derive(Debug, Clone)]
pub struct LibreTextsToc {
    pub book_id: String,
    pub title: String,
    pub library: String,
    pub cover_page_id: String,
    pub chapter_count: u32,
    pub pages: Vec<LibreTextsTocEntry>,
}

#[derive(Debug, Clone)]
pub struct LibreTextsTocEntry {
    pub page_id: String,
    pub title: String,
    pub url: Option<String>,
    pub chapter_number: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibreTextsPageContent {
    pub page_id: String,
    pub title: String,
    pub html: String,
    pub revision: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<ApiBook>,
}

#[derive(Debug, Deserialize)]
struct LibrariesResponse {
    #[serde(default)]
    libraries: Vec<ApiLibrary>,
}

#[derive(Debug, Deserialize)]
struct BookDetailResponse {
    book: ApiBook,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiBook {
    #[serde(rename = "bookID")]
    book_id: String,
    title: String,
    #[serde(default)]
    author: Option<String>,
    #[serde(default)]
    affiliation: Option<String>,
    library: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    license: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    thumbnail: Option<String>,
    #[serde(default)]
    links: Option<ApiBookLinks>,
    #[serde(default)]
    last_updated: Option<String>,
    #[serde(default)]
    location: Option<String>,
    #[serde(default)]
    program: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiBookLinks {
    #[serde(default)]
    online: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiLibrary {
    subdomain: String,
    title: String,
}

#[derive(Debug, Deserialize)]
struct TreeResponse {
    page: TreePage,
}

#[derive(Debug, Clone, Deserialize)]
struct TreePage {
    #[serde(rename = "@id")]
    id: String,
    title: String,
    #[serde(default)]
    subpages: TreeSubpages,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum TreeSubpages {
    Pages { page: OneOrManyTreePage },
    Empty(#[allow(dead_code)] String),
}

impl Default for TreeSubpages {
    fn default() -> Self {
        Self::Empty(String::new())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum OneOrManyTreePage {
    One(Box<TreePage>),
    Many(Vec<TreePage>),
}

#[derive(Debug, Deserialize)]
struct ContentResponse {
    #[serde(rename = "@revision")]
    revision: Option<String>,
    #[serde(rename = "@title")]
    title: Option<String>,
    body: ContentBody,
}

#[derive(Debug, Clone)]
struct PublicTocSeed {
    page_id: Option<String>,
    title: String,
    url: String,
    chapter_number: u32,
}

#[derive(Debug, Clone)]
struct PublicPage {
    page_id: Option<String>,
    title: Option<String>,
    content_html: String,
    children: Vec<PublicTocSeed>,
    revision: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ContentBody {
    Html(String),
    Parts(Vec<ContentBodyPart>),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ContentBodyPart {
    Html(String),
    Other(#[allow(dead_code)] serde_json::Value),
}

impl LibreTextsClient {
    pub fn new(db: DbPool) -> Self {
        let commons_base_url = std::env::var("JOHNNY_READER_LIBRETEXTS_COMMONS_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_COMMONS_BASE_URL.to_string());
        Self::with_commons_base_url(db, commons_base_url)
    }

    pub fn with_commons_base_url(db: DbPool, commons_base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .expect("valid LibreTexts HTTP client"),
            db,
            commons_base_url: commons_base_url.into().trim_end_matches('/').to_string(),
        }
    }

    pub async fn search_books(
        &self,
        query: Option<&str>,
        library: Option<&str>,
    ) -> AppResult<Vec<LibreTextsBook>> {
        let url = format!("{}/api/v1/search/books-v2", self.commons_base_url);
        let mut params = vec![
            ("limit", "100".to_string()),
            ("page", "1".to_string()),
            ("sort", "title".to_string()),
        ];

        if let Some(query) = query.map(str::trim).filter(|query| !query.is_empty()) {
            params.push(("searchQuery", query.to_string()));
        }
        if let Some(library) = library.map(str::trim).filter(|library| !library.is_empty()) {
            params.push(("library", library.to_string()));
        }

        let response = self.fetch_json::<SearchResponse>(&url, &params).await?;
        Ok(response.results.into_iter().map(Into::into).collect())
    }

    pub async fn list_libraries(&self) -> AppResult<Vec<LibreTextsLibrary>> {
        let url = format!("{}/api/v1/commons/libraries", self.commons_base_url);
        let response = self.fetch_json::<LibrariesResponse>(&url, &[]).await?;
        let mut libraries = response
            .libraries
            .into_iter()
            .map(|library| LibreTextsLibrary {
                subdomain: library.subdomain,
                title: library.title,
            })
            .collect::<Vec<_>>();
        libraries.sort_by(|left, right| left.title.cmp(&right.title));
        Ok(libraries)
    }

    pub async fn fetch_book_detail(&self, book_id: &str) -> AppResult<LibreTextsBook> {
        validate_book_id(book_id)?;
        let url = format!("{}/api/v1/commons/book/{book_id}", self.commons_base_url);
        let response = self.fetch_json::<BookDetailResponse>(&url, &[]).await?;
        Ok(response.book.into())
    }

    pub async fn fetch_toc<F>(
        &self,
        book: &LibreTextsBook,
        on_progress: &mut F,
    ) -> AppResult<LibreTextsToc>
    where
        F: FnMut(u32, u32) + Send,
    {
        let (library, cover_page_id) = parse_book_id(&book.book_id)?;
        let url = format!(
            "{}/@api/deki/pages/{}/tree",
            library_base_url(&library),
            cover_page_id
        );
        let response = match self
            .fetch_json::<TreeResponse>(&url, &[("dream.out.format", "json".to_string())])
            .await
        {
            Ok(response) => response,
            Err(error)
                if should_fallback_to_public_html(&error)
                    && book
                        .online_url
                        .as_deref()
                        .is_some_and(|url| !url.trim().is_empty()) =>
            {
                return self
                    .fetch_toc_from_public_html(book, &library, &cover_page_id, on_progress)
                    .await;
            }
            Err(error) => return Err(error),
        };
        let mut pages = Vec::new();
        let chapter_count = collect_book_pages(&response.page, &mut pages);

        if pages.is_empty() {
            return Err(AppError::LibreTexts(format!(
                "LibreTexts book {} did not include readable pages",
                book.book_id
            )));
        }

        Ok(LibreTextsToc {
            book_id: book.book_id.clone(),
            title: if book.title.trim().is_empty() {
                response.page.title
            } else {
                book.title.clone()
            },
            library,
            cover_page_id,
            chapter_count,
            pages,
        })
    }

    async fn fetch_toc_from_public_html<F>(
        &self,
        book: &LibreTextsBook,
        library: &str,
        cover_page_id: &str,
        on_progress: &mut F,
    ) -> AppResult<LibreTextsToc>
    where
        F: FnMut(u32, u32) + Send,
    {
        let root_url = book
            .online_url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| {
                AppError::LibreTexts(format!(
                    "LibreTexts book {} did not include a public URL",
                    book.book_id
                ))
            })?;
        let root_scope = normalized_public_url(root_url);
        let mut stack = vec![PublicTocSeed {
            page_id: Some(cover_page_id.to_string()),
            title: book.title.clone(),
            url: root_url.to_string(),
            chapter_number: 0,
        }];
        let mut visited = HashSet::new();
        let mut pages = Vec::new();
        let mut chapter_count = 1;
        let mut reported_chapter = 0;

        while let Some(seed) = stack.pop() {
            let normalized_url = normalized_public_url(&seed.url);
            let is_root = normalized_url == root_scope;
            if !visited.insert(normalized_url) {
                continue;
            }

            if seed.chapter_number > 0 && seed.chapter_number != reported_chapter {
                reported_chapter = seed.chapter_number;
                on_progress(reported_chapter, chapter_count);
            }

            let html = self.fetch_html(&seed.url).await?;
            let page = parse_public_page(&html, &seed.url, &root_scope)?;
            let page_id = page
                .page_id
                .clone()
                .or_else(|| seed.page_id.clone())
                .unwrap_or_else(|| normalized_public_url(&seed.url));
            let chapter_number = seed.chapter_number.max(1);
            let title = page
                .title
                .clone()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or_else(|| seed.title.clone());
            let should_store_page = page.children.is_empty()
                || (!is_root && public_page_has_importable_content(&page, &seed.url));

            if should_store_page {
                if chapter_number != reported_chapter {
                    reported_chapter = chapter_number;
                    on_progress(reported_chapter, chapter_count);
                }

                let page_content = LibreTextsPageContent {
                    page_id: page_id.clone(),
                    title: title.clone(),
                    html: page.content_html.clone(),
                    revision: page.revision.clone(),
                };
                let cache_key = format!("{}:{page_id}", book.book_id);
                self.store_page(&book.book_id, &page_content, &cache_key)?;

                pages.push(LibreTextsTocEntry {
                    page_id,
                    title,
                    url: Some(seed.url.clone()),
                    chapter_number,
                });
            }

            if !page.children.is_empty() {
                if is_root {
                    chapter_count = page.children.len().max(1) as u32;
                }

                let children = page
                    .children
                    .into_iter()
                    .enumerate()
                    .map(|(index, mut child)| {
                        child.chapter_number = if is_root {
                            index as u32 + 1
                        } else {
                            seed.chapter_number
                        };
                        child
                    })
                    .collect::<Vec<_>>();

                for child in children.into_iter().rev() {
                    stack.push(child);
                }
            }
        }

        if pages.is_empty() {
            return Err(AppError::LibreTexts(format!(
                "LibreTexts book {} did not include readable public pages",
                book.book_id
            )));
        }

        Ok(LibreTextsToc {
            book_id: book.book_id.clone(),
            title: book.title.clone(),
            library: library.to_string(),
            cover_page_id: cover_page_id.to_string(),
            chapter_count,
            pages,
        })
    }

    pub async fn fetch_page(
        &self,
        book_id: &str,
        library: &str,
        entry: &LibreTextsTocEntry,
    ) -> AppResult<LibreTextsPageContent> {
        let cache_key = format!("{book_id}:{}", entry.page_id);

        if let Some(page) = self.cached_page(&cache_key)? {
            return Ok(page);
        }

        if let Some(url) = &entry.url {
            let html = self.fetch_html(url).await?;
            let public_page = parse_public_page(&html, url, &normalized_public_url(url))?;
            let page = LibreTextsPageContent {
                page_id: entry.page_id.clone(),
                title: public_page
                    .title
                    .filter(|title| !title.trim().is_empty())
                    .unwrap_or_else(|| entry.title.clone()),
                html: public_page.content_html,
                revision: public_page.revision,
            };

            self.store_page(book_id, &page, &cache_key)?;
            return Ok(page);
        }

        let url = format!(
            "{}/@api/deki/pages/{}/contents",
            library_base_url(library),
            entry.page_id
        );
        let response = self
            .fetch_json::<ContentResponse>(&url, &[("dream.out.format", "json".to_string())])
            .await?;
        let page = LibreTextsPageContent {
            page_id: entry.page_id.clone(),
            title: response.title.unwrap_or_else(|| entry.title.clone()),
            html: response.body.into_html(),
            revision: response.revision,
        };

        self.store_page(book_id, &page, &cache_key)?;
        Ok(page)
    }

    pub async fn fetch_book_pages<F>(
        &self,
        toc: &LibreTextsToc,
        mut on_progress: F,
    ) -> AppResult<Vec<LibreTextsPageContent>>
    where
        F: FnMut(u32, u32) + Send,
    {
        let total = toc.chapter_count.max(1);
        let mut current_chapter = 0;
        let mut pages = Vec::with_capacity(toc.pages.len());

        for entry in &toc.pages {
            if entry.chapter_number != current_chapter {
                current_chapter = entry.chapter_number;
                on_progress(current_chapter, total);
            }
            pages.push(self.fetch_page(&toc.book_id, &toc.library, entry).await?);
        }

        Ok(pages)
    }

    async fn fetch_json<T: DeserializeOwned>(
        &self,
        url: &str,
        params: &[(&str, String)],
    ) -> AppResult<T> {
        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            let request = self
                .http
                .get(url)
                .query(params)
                .header("user-agent", "johnny-reader-libretexts-importer");

            match request.send().await {
                Ok(response) if response.status().is_success() => {
                    return Ok(response.json::<T>().await?);
                }
                Ok(response) => {
                    let status = response.status();
                    if !should_retry_status(status) || attempt + 1 == MAX_RETRIES {
                        return Err(AppError::LibreTexts(format!(
                            "request to {url} failed with HTTP {status}"
                        )));
                    }
                    last_error = Some(format!("HTTP {status}"));
                }
                Err(error) => {
                    if attempt + 1 == MAX_RETRIES {
                        return Err(AppError::Http(error));
                    }
                    last_error = Some(error.to_string());
                }
            }

            tokio::time::sleep(Duration::from_millis(500 * 2_u64.pow(attempt as u32))).await;
        }

        Err(AppError::LibreTexts(format!(
            "request to {url} failed: {}",
            last_error.unwrap_or_else(|| "unknown error".to_string())
        )))
    }

    async fn fetch_html(&self, url: &str) -> AppResult<String> {
        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            let request = self
                .http
                .get(url)
                .header("user-agent", "johnny-reader-libretexts-importer")
                .header("accept", "text/html,application/xhtml+xml");

            match request.send().await {
                Ok(response) if response.status().is_success() => {
                    return Ok(response.text().await?);
                }
                Ok(response) => {
                    let status = response.status();
                    if !should_retry_status(status) || attempt + 1 == MAX_RETRIES {
                        return Err(AppError::LibreTexts(format!(
                            "request to {url} failed with HTTP {status}"
                        )));
                    }
                    last_error = Some(format!("HTTP {status}"));
                }
                Err(error) => {
                    if attempt + 1 == MAX_RETRIES {
                        return Err(AppError::Http(error));
                    }
                    last_error = Some(error.to_string());
                }
            }

            tokio::time::sleep(Duration::from_millis(500 * 2_u64.pow(attempt as u32))).await;
        }

        Err(AppError::LibreTexts(format!(
            "request to {url} failed: {}",
            last_error.unwrap_or_else(|| "unknown error".to_string())
        )))
    }

    fn cached_page(&self, cache_key: &str) -> AppResult<Option<LibreTextsPageContent>> {
        let conn = self.db.get()?;
        let content_gzip = conn
            .query_row(
                "SELECT content_gzip FROM libretexts_cache WHERE cache_key = ?1",
                params![cache_key],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;

        let Some(content_gzip) = content_gzip else {
            return Ok(None);
        };

        let mut decoder = GzDecoder::new(content_gzip.as_slice());
        let mut json = String::new();
        decoder.read_to_string(&mut json)?;
        Ok(Some(serde_json::from_str(&json)?))
    }

    fn store_page(
        &self,
        book_id: &str,
        page: &LibreTextsPageContent,
        cache_key: &str,
    ) -> AppResult<()> {
        let json = serde_json::to_vec(page)?;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&json)?;
        let content_gzip = encoder.finish()?;
        let fetched_at = Utc::now().to_rfc3339();
        let conn = self.db.get()?;

        conn.execute(
            "INSERT OR REPLACE INTO libretexts_cache (
                cache_key, book_id, page_id, content_gzip, content_revision, fetched_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                cache_key,
                book_id,
                page.page_id,
                content_gzip,
                page.revision,
                fetched_at
            ],
        )?;

        Ok(())
    }
}

pub async fn list_catalog(
    db: DbPool,
    query: Option<String>,
    library: Option<String>,
) -> AppResult<Vec<LibreTextsBook>> {
    LibreTextsClient::new(db)
        .search_books(query.as_deref(), library.as_deref())
        .await
}

pub async fn list_libraries(db: DbPool) -> AppResult<Vec<LibreTextsLibrary>> {
    LibreTextsClient::new(db).list_libraries().await
}

pub async fn import_book<F>(
    db: DbPool,
    book_id: &str,
    mut on_progress: F,
) -> AppResult<DocumentBuilder>
where
    F: FnMut(u32, u32) + Send,
{
    let client = LibreTextsClient::new(db);
    let book = client.fetch_book_detail(book_id).await?;
    let toc = client.fetch_toc(&book, &mut on_progress).await?;
    let pages = client.fetch_book_pages(&toc, on_progress).await?;
    let sections = sections_from_pages(&client, &toc, &pages).await?;

    if sections.is_empty() {
        return Err(AppError::LibreTexts(
            "LibreTexts book did not contain readable text".into(),
        ));
    }

    Ok(DocumentBuilder {
        title: toc.title,
        source_type: SourceType::Libretexts,
        source_metadata: json!({
            "book_id": book.book_id,
            "library": toc.library,
            "cover_page_id": toc.cover_page_id,
            "source_url": book.online_url,
            "imported_at": Utc::now().to_rfc3339()
        }),
        cover_image_path: None,
        license: Some(license_label(&book.license).to_string()),
        attribution: book.online_url,
        sections,
    })
}

impl From<ApiBook> for LibreTextsBook {
    fn from(book: ApiBook) -> Self {
        Self {
            book_id: book.book_id,
            title: book.title,
            author: clean_optional(book.author),
            affiliation: clean_optional(book.affiliation),
            library: book.library,
            subject: clean_optional(book.subject),
            license: clean_optional(book.license),
            summary: clean_optional(book.summary),
            thumbnail: book.thumbnail.filter(|value| !value.trim().is_empty()),
            online_url: book
                .links
                .and_then(|links| links.online)
                .filter(|value| !value.trim().is_empty()),
            last_updated: book.last_updated.filter(|value| !value.trim().is_empty()),
            location: clean_optional(book.location),
            program: clean_optional(book.program),
        }
    }
}

impl ContentBody {
    fn into_html(self) -> String {
        match self {
            Self::Html(html) => html,
            Self::Parts(parts) => parts
                .into_iter()
                .filter_map(|part| match part {
                    ContentBodyPart::Html(html) => Some(html),
                    ContentBodyPart::Other(_) => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }
}

fn collect_book_pages(page: &TreePage, pages: &mut Vec<LibreTextsTocEntry>) -> u32 {
    let children = tree_children(page);

    if children.is_empty() {
        collect_page_tree(page, 1, pages);
        return 1;
    }

    for (index, child) in children.iter().enumerate() {
        collect_page_tree(child, index as u32 + 1, pages);
    }

    children.len() as u32
}

fn collect_page_tree(page: &TreePage, chapter_number: u32, pages: &mut Vec<LibreTextsTocEntry>) {
    pages.push(LibreTextsTocEntry {
        page_id: page.id.clone(),
        title: page.title.clone(),
        url: None,
        chapter_number,
    });

    for child in tree_children(page) {
        collect_page_tree(child, chapter_number, pages);
    }
}

fn tree_children(page: &TreePage) -> &[TreePage] {
    match &page.subpages {
        TreeSubpages::Pages { page } => page.as_slice(),
        TreeSubpages::Empty(_) => &[],
    }
}

impl OneOrManyTreePage {
    fn as_slice(&self) -> &[TreePage] {
        match self {
            Self::One(page) => std::slice::from_ref(page.as_ref()),
            Self::Many(pages) => pages.as_slice(),
        }
    }
}

fn parse_book_id(book_id: &str) -> AppResult<(String, String)> {
    validate_book_id(book_id)?;
    let Some((library, page_id)) = book_id.split_once('-') else {
        return Err(AppError::LibreTexts(format!(
            "invalid LibreTexts book ID: {book_id}"
        )));
    };
    Ok((library.to_string(), page_id.to_string()))
}

fn validate_book_id(book_id: &str) -> AppResult<()> {
    if book_id_re().is_match(book_id) {
        Ok(())
    } else {
        Err(AppError::LibreTexts(format!(
            "invalid LibreTexts book ID: {book_id}"
        )))
    }
}

fn book_id_re() -> &'static Regex {
    BOOK_ID_RE.get_or_init(|| Regex::new(r"^[a-z1-2]{3,9}-[0-9]{2,10}$").expect("valid regex"))
}

fn library_base_url(library: &str) -> String {
    std::env::var("JOHNNY_READER_LIBRETEXTS_LIBRARY_BASE_URL")
        .unwrap_or_else(|_| format!("https://{library}.libretexts.org"))
        .trim_end_matches('/')
        .to_string()
}

fn page_base_url(library: &str, entry: &LibreTextsTocEntry) -> String {
    entry.url.clone().unwrap_or_else(|| {
        format!(
            "{}/@api/deki/pages/{}/contents",
            library_base_url(library),
            entry.page_id
        )
    })
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

fn should_fallback_to_public_html(error: &AppError) -> bool {
    match error {
        AppError::LibreTexts(message) => {
            message.contains("HTTP 401") || message.contains("HTTP 403")
        }
        _ => false,
    }
}

fn parse_public_page(html: &str, current_url: &str, root_scope: &str) -> AppResult<PublicPage> {
    let document = Html::parse_document(html);
    let page_id = text_by_selector(&document, "#pageIDHolder");
    let title = text_by_selector(&document, "#title")
        .or_else(|| text_by_selector(&document, "#titleHolder"));
    let revision = text_by_selector(&document, "#modifiedHolder");
    let content_html = content_container_html(&document).unwrap_or_else(|| html.to_string());
    let children = public_child_links(&document, current_url, root_scope)?;

    Ok(PublicPage {
        page_id,
        title,
        content_html,
        children,
        revision,
    })
}

fn text_by_selector(document: &Html, selector: &str) -> Option<String> {
    let selector = Selector::parse(selector).ok()?;
    document
        .select(&selector)
        .next()
        .map(|element| normalize_text(&element.text().collect::<Vec<_>>().join(" ")))
        .filter(|text| !text.is_empty())
}

fn content_container_html(document: &Html) -> Option<String> {
    let selector = Selector::parse("section.mt-content-container").ok()?;
    document
        .select(&selector)
        .next()
        .map(|element| element.html())
}

fn public_child_links(
    document: &Html,
    current_url: &str,
    root_scope: &str,
) -> AppResult<Vec<PublicTocSeed>> {
    let selector = Selector::parse("section.mt-content-container li[data-page-id] a[href]")
        .expect("valid LibreTexts public child selector");
    let mut seen = HashSet::new();
    let mut children = Vec::new();

    for link in document.select(&selector) {
        let Some(href) = link.value().attr("href") else {
            continue;
        };
        if href.starts_with('#') || href.starts_with("mailto:") || href.starts_with("javascript:") {
            continue;
        }

        let child_url = resolve_public_url(current_url, href)?;
        let normalized_url = normalized_public_url(&child_url);
        if !url_is_within_scope(&normalized_url, root_scope) || !seen.insert(normalized_url) {
            continue;
        }

        let page_id = link
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find(|node| node.value().name() == "li" && node.value().attr("data-page-id").is_some())
            .and_then(|node| node.value().attr("data-page-id").map(str::to_string))
            .filter(|value| !value.trim().is_empty());
        let title = link
            .value()
            .attr("title")
            .map(normalize_text)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| normalize_text(&link.text().collect::<Vec<_>>().join(" ")));

        if title.is_empty() {
            continue;
        }

        children.push(PublicTocSeed {
            page_id,
            title,
            url: child_url,
            chapter_number: 1,
        });
    }

    Ok(children)
}

fn resolve_public_url(current_url: &str, href: &str) -> AppResult<String> {
    let base = Url::parse(current_url).map_err(|error| {
        AppError::LibreTexts(format!(
            "invalid LibreTexts page URL {current_url}: {error}"
        ))
    })?;
    Ok(base
        .join(href)
        .map_err(|error| {
            AppError::LibreTexts(format!("invalid LibreTexts child URL {href}: {error}"))
        })?
        .to_string())
}

fn normalized_public_url(url: &str) -> String {
    Url::parse(url)
        .map(|mut parsed| {
            parsed.set_fragment(None);
            parsed.set_query(None);
            parsed.to_string().trim_end_matches('/').to_string()
        })
        .unwrap_or_else(|_| url.trim_end_matches('/').to_string())
}

fn url_is_within_scope(url: &str, root_scope: &str) -> bool {
    url == root_scope || url.starts_with(&format!("{root_scope}/"))
}

fn public_page_has_importable_content(page: &PublicPage, page_url: &str) -> bool {
    !paragraphs_from_html(&page.content_html).is_empty()
        || !source_images_from_html(&page.content_html, page_url).is_empty()
}

async fn sections_from_pages(
    client: &LibreTextsClient,
    toc: &LibreTextsToc,
    pages: &[LibreTextsPageContent],
) -> AppResult<Vec<SectionBuilder>> {
    let mut sections = Vec::new();

    for page in pages {
        let entry = toc.pages.iter().find(|entry| entry.page_id == page.page_id);
        let title = entry
            .map(|entry| entry.title.clone())
            .filter(|title| !title.trim().is_empty())
            .unwrap_or_else(|| page.title.clone());
        let (paragraphs, image_candidates) = match entry {
            Some(entry) => {
                section_content_from_html(&page.html, &page_base_url(&toc.library, entry))
            }
            None => (paragraphs_from_html(&page.html), Vec::new()),
        };
        let images = download_images(&client.http, image_candidates).await?;
        if paragraphs.is_empty() && images.is_empty() {
            continue;
        }

        sections.push(SectionBuilder {
            title,
            paragraphs,
            images,
        });
    }

    Ok(sections)
}

/// MindTouch wraps content in a lot of chrome — navigation, category
/// containers, feedback widgets — and repeats page listings inside the body.
/// Unlike OpenStax, the same rule applies to images: a listing thumbnail is not
/// a figure.
struct LibreTextsSource;

impl SectionSource for LibreTextsSource {
    fn should_skip_paragraph(&self, element: &ElementRef<'_>) -> bool {
        should_skip_element(element)
    }

    fn should_skip_image(&self, element: &ElementRef<'_>) -> bool {
        should_skip_element(element)
    }

    fn is_readable(&self, text: &str) -> bool {
        is_readable_paragraph(text)
    }
}

pub fn paragraphs_from_html(html: &str) -> Vec<String> {
    html_section::paragraphs_from_html(html, &LibreTextsSource)
}

fn section_content_from_html(html: &str, base_url: &str) -> (Vec<String>, Vec<SourceImage>) {
    html_section::section_content_from_html(html, base_url, &LibreTextsSource)
}

fn should_skip_element(element: &ElementRef) -> bool {
    if element_html_contains_navigation_listing(element) {
        return true;
    }

    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|node| {
            let name = node.value().name();
            matches!(
                name,
                "script" | "style" | "nav" | "noscript" | "header" | "footer" | "aside" | "form"
            ) || node.value().classes().any(|class| {
                matches!(
                    class,
                    "mt-script-comment"
                        | "Headertext"
                        | "autoattribution"
                        | "mt-category-container"
                        | "noinclude"
                        | "noindex"
                        | "noprint"
                        | "printfooter"
                        | "mt-content-footer"
                        | "mt-learningpathway"
                        | "mt-feedback"
                        | "mt-social"
                        | "lt-social"
                        | "mt-guide-content"
                        | "mt-guide-listings"
                        | "mt-list-topics"
                        | "mt-listing-detailed"
                        | "mt-subpage-listings-container"
                        | "mt-topic-hierarchy-listings"
                        | "mt-sortable-listings-container"
                        | "toc"
                )
            })
        })
}

fn element_html_contains_navigation_listing(element: &ElementRef<'_>) -> bool {
    let html = element.html();
    html.contains("mt-topic-hierarchy-listings")
        || html.contains("mt-sortable-listings-container")
        || html.contains("mt-subpage-listings-container")
}

fn is_readable_paragraph(text: &str) -> bool {
    !text.is_empty() && text != "No headers" && text != "Table of Contents"
}

fn clean_optional(value: Option<String>) -> String {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

fn license_label(value: &str) -> &str {
    match value.trim().to_lowercase().as_str() {
        "ccby" => "CC BY",
        "ccbync" => "CC BY-NC",
        "ccbyncnd" => "CC BY-NC-ND",
        "ccbyncsa" => "CC BY-NC-SA",
        "ccbynd" => "CC BY-ND",
        "ccbysa" => "CC BY-SA",
        "publicdomain" => "Public Domain",
        "arr" => "All Rights Reserved",
        "" => "Unknown",
        _ => value,
    }
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use crate::db::{connection, library};

    #[test]
    fn paragraphs_ignore_libretexts_navigation_listings() {
        let html = r#"
            <section class="mt-content-container">
                <p>This chapter has its own introduction.</p>
                <p>
                    <div class="mt-category-container mt-subpage-listings-container noindex">
                        <ul class="mt-sortable-listings-container">
                            <li data-page-id="1"><a href="/chapter/1">1.1: Child Page</a></li>
                            <li data-page-id="2"><a href="/chapter/2">1.2: Child Page</a></li>
                        </ul>
                    </div>
                </p>
            </section>
        "#;

        assert_eq!(
            super::paragraphs_from_html(html),
            vec!["This chapter has its own introduction."]
        );
    }

    #[test]
    fn section_images_keep_text_position() {
        let html = r#"
            <section class="mt-content-container">
                <div class="mt-category-container mt-subpage-listings-container noindex">
                    <img src="/book/navigation-thumb.jpg" alt="Navigation thumbnail" />
                </div>
                <p>Cells store information in DNA.</p>
                <figure>
                    <img src="/book/cell.jpg" alt="Cell diagram" />
                    <figcaption>Figure 1.1 Cell structure.</figcaption>
                </figure>
                <p>Proteins perform many cellular functions.</p>
            </section>
        "#;

        let (paragraphs, images) =
            super::section_content_from_html(html, "https://bio.libretexts.org/chapter/page");

        assert_eq!(
            paragraphs,
            vec![
                "Cells store information in DNA.",
                "Proteins perform many cellular functions."
            ]
        );
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].url, "https://bio.libretexts.org/book/cell.jpg");
        assert_eq!(images[0].anchor_paragraph_ordinal, Some(0));
        assert_eq!(
            images[0].caption.as_deref(),
            Some("Figure 1.1 Cell structure.")
        );
    }

    #[test]
    fn public_non_leaf_page_with_own_content_is_importable() {
        let page = super::PublicPage {
            page_id: Some("chapter".to_string()),
            title: Some("Chapter".to_string()),
            content_html: r#"
                <section class="mt-content-container">
                    <p>This chapter has its own figure.</p>
                    <figure><img src="/chapter/figure.jpg" alt="Figure" /></figure>
                    <div class="mt-guide-content">
                        <ul class="mt-topic-hierarchy-listings">
                            <li data-page-id="child"><a href="/chapter/child">Child</a></li>
                        </ul>
                    </div>
                </section>
            "#
            .to_string(),
            children: vec![super::PublicTocSeed {
                page_id: Some("child".to_string()),
                title: "Child".to_string(),
                url: "https://example.com/chapter/child".to_string(),
                chapter_number: 1,
            }],
            revision: None,
        };

        assert!(super::public_page_has_importable_content(
            &page,
            "https://example.com/chapter"
        ));
    }

    #[tokio::test]
    #[ignore]
    async fn live_imports_small_public_book_with_images() {
        let app_data_dir =
            std::env::temp_dir().join(format!("johnny-reader-libretexts-smoke-{}", Uuid::new_v4()));
        std::env::set_var("JOHNNY_READER_APP_DATA_DIR", &app_data_dir);
        let db_path = app_data_dir.join("library.sqlite");
        let pool = connection::init_pool(&db_path).expect("temporary database should initialize");

        let document = super::import_book(pool.clone(), "human-15711", |current, total| {
            eprintln!("LibreTexts smoke import progress: {current}/{total}");
        })
        .await
        .expect("small public LibreTexts book should import");

        assert!(
            !document.sections.is_empty(),
            "import should produce readable sections"
        );
        assert!(
            document
                .sections
                .iter()
                .any(|section| !section.images.is_empty()),
            "import should download at least one section image"
        );

        let mut conn = pool.get().expect("database connection should be available");
        let document_id = document
            .persist(&mut conn)
            .expect("imported document should persist");
        let sections =
            library::list_sections(&conn, &document_id).expect("sections should be listed");
        let image_count = sections
            .iter()
            .map(|section| {
                library::list_section_images(&conn, &section.id)
                    .expect("section images should be listed")
                    .len()
            })
            .sum::<usize>();

        assert!(image_count > 0, "persisted document should include images");
        library::delete_document(&conn, &document_id).expect("temporary document should delete");
        drop(conn);
        drop(pool);
        let _ = std::fs::remove_dir_all(app_data_dir);
    }
}
