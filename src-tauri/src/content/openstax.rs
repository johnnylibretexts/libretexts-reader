use std::collections::HashMap;
use std::io::{Read, Write};
use std::time::Duration;

use chrono::Utc;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use reqwest::StatusCode;
use rusqlite::{params, OptionalExtension};
use scraper::{ElementRef, Html};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::content::document::{DocumentBuilder, SectionBuilder};
use crate::content::html_section::{self, normalize_text, SectionSource};
use crate::content::images::{download_images, SourceImage};
use crate::db::connection::DbPool;
use crate::db::models::{OpenStaxBook, SourceType};
use crate::error::{AppError, AppResult};

const DEFAULT_OPENSTAX_BASE_URL: &str = "https://openstax.org";
const MAX_RETRIES: usize = 3;

#[derive(Debug, Clone)]
pub struct OpenStaxClient {
    http: reqwest::Client,
    db: DbPool,
    base_url: String,
    release: tokio::sync::OnceCell<ReleaseManifest>,
}

#[derive(Debug, Clone)]
pub struct BookToc {
    pub book_uuid: String,
    pub release: String,
    pub title: String,
    pub license: String,
    pub language: String,
    pub pages: Vec<TocEntry>,
}

#[derive(Debug, Clone)]
pub struct TocEntry {
    pub page_uuid: String,
    pub title: String,
    pub depth: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageContent {
    pub page_uuid: String,
    pub title: String,
    pub html: String,
}

#[derive(Debug, Deserialize)]
struct Catalog {
    books: Vec<OpenStaxBook>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseManifest {
    archive_url: String,
    books: HashMap<String, ReleaseBook>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseBook {
    default_version: String,
}

#[derive(Debug, Deserialize)]
struct ArchiveBook {
    title: String,
    tree: TocNode,
    license: Option<ArchiveLicense>,
    language: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArchiveLicense {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TocNode {
    id: String,
    title: String,
    #[serde(default)]
    toc_type: Option<String>,
    #[serde(default)]
    contents: Vec<TocNode>,
}

#[derive(Debug, Deserialize)]
struct ArchivePage {
    id: String,
    title: String,
    content: String,
}

impl OpenStaxClient {
    pub fn new(db: DbPool) -> Self {
        let base_url = std::env::var("LIBRETEXTS_READER_OPENSTAX_BASE_URL")
            .unwrap_or_else(|_| DEFAULT_OPENSTAX_BASE_URL.to_string());
        Self::with_base_url(db, base_url)
    }

    pub fn with_base_url(db: DbPool, base_url: impl Into<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("valid OpenStax HTTP client"),
            db,
            base_url: base_url.into().trim_end_matches('/').to_string(),
            release: tokio::sync::OnceCell::new(),
        }
    }

    pub async fn fetch_toc(&self, book_uuid: &str) -> AppResult<BookToc> {
        let release = self.release().await?;
        let version = self.default_version(book_uuid, release)?;
        let release_key = release_key(release, version);
        let url = self.archive_url(&format!("/contents/{book_uuid}@{version}.json"), release);
        let book = self.fetch_json::<ArchiveBook>(&url).await?;
        let mut pages = Vec::new();

        collect_toc_entries(&book.tree, 0, &mut pages);
        if pages.is_empty() {
            return Err(AppError::OpenStax(format!(
                "OpenStax book {book_uuid} did not include readable pages"
            )));
        }

        Ok(BookToc {
            book_uuid: book_uuid.to_string(),
            release: release_key,
            title: book.title,
            license: book
                .license
                .and_then(|license| license.name)
                .unwrap_or_else(|| "Unknown".to_string()),
            language: book.language.unwrap_or_else(|| "en".to_string()),
            pages,
        })
    }

    pub async fn fetch_page(&self, book_uuid: &str, page_uuid: &str) -> AppResult<PageContent> {
        let release = self.release().await?;
        let version = self.default_version(book_uuid, release)?;
        let release_key = release_key(release, version);
        let cache_key = format!("{book_uuid}:{page_uuid}");

        if let Some(page) = self.cached_page(&cache_key, &release_key)? {
            return Ok(page);
        }

        let url = self.archive_url(
            &format!("/contents/{book_uuid}@{version}:{page_uuid}.json"),
            release,
        );
        let page = self.fetch_json::<ArchivePage>(&url).await?;
        let content = PageContent {
            page_uuid: strip_version(&page.id),
            title: page.title,
            html: page.content,
        };

        self.store_page(book_uuid, &content, &cache_key, &release_key)?;
        Ok(content)
    }

    pub async fn fetch_book<F>(
        &self,
        book_uuid: &str,
        on_progress: F,
    ) -> AppResult<Vec<PageContent>>
    where
        F: Fn(u32, u32) + Send,
    {
        let toc = self.fetch_toc(book_uuid).await?;
        let total = toc.pages.len() as u32;
        let mut pages = Vec::with_capacity(toc.pages.len());

        for (index, entry) in toc.pages.iter().enumerate() {
            pages.push(self.fetch_page(book_uuid, &entry.page_uuid).await?);
            on_progress(index as u32 + 1, total);
        }

        Ok(pages)
    }

    async fn release(&self) -> AppResult<&ReleaseManifest> {
        self.release
            .get_or_try_init(|| async {
                let url = format!("{}/rex/release.json", self.base_url);
                self.fetch_json::<ReleaseManifest>(&url).await
            })
            .await
    }

    fn default_version<'a>(
        &self,
        book_uuid: &str,
        release: &'a ReleaseManifest,
    ) -> AppResult<&'a str> {
        release
            .books
            .get(book_uuid)
            .map(|book| book.default_version.as_str())
            .ok_or_else(|| {
                AppError::OpenStax(format!(
                    "OpenStax book {book_uuid} is not available in the current release"
                ))
            })
    }

    fn archive_url(&self, path: &str, release: &ReleaseManifest) -> String {
        format!("{}{}{}", self.base_url, release.archive_url, path)
    }

    async fn page_url(&self, book_uuid: &str, page_uuid: &str) -> AppResult<String> {
        let release = self.release().await?;
        let version = self.default_version(book_uuid, release)?;
        Ok(self.archive_url(
            &format!("/contents/{book_uuid}@{version}:{page_uuid}.json"),
            release,
        ))
    }

    async fn fetch_json<T: DeserializeOwned>(&self, url: &str) -> AppResult<T> {
        let mut last_error = None;

        for attempt in 0..MAX_RETRIES {
            match self.http.get(url).send().await {
                Ok(response) if response.status().is_success() => {
                    return Ok(response.json::<T>().await?);
                }
                Ok(response) => {
                    let status = response.status();
                    if !should_retry_status(status) || attempt + 1 == MAX_RETRIES {
                        return Err(AppError::OpenStax(format!(
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

        Err(AppError::OpenStax(format!(
            "request to {url} failed: {}",
            last_error.unwrap_or_else(|| "unknown error".to_string())
        )))
    }

    fn cached_page(&self, cache_key: &str, release_key: &str) -> AppResult<Option<PageContent>> {
        let conn = self.db.get()?;
        let row = conn
            .query_row(
                "SELECT content_gzip, archive_release FROM openstax_cache WHERE cache_key = ?1",
                params![cache_key],
                |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?;

        let Some((content_gzip, archive_release)) = row else {
            return Ok(None);
        };

        if archive_release != release_key {
            return Ok(None);
        }

        let mut decoder = GzDecoder::new(content_gzip.as_slice());
        let mut json = String::new();
        decoder.read_to_string(&mut json)?;
        Ok(Some(serde_json::from_str(&json)?))
    }

    fn store_page(
        &self,
        book_uuid: &str,
        page: &PageContent,
        cache_key: &str,
        release_key: &str,
    ) -> AppResult<()> {
        let json = serde_json::to_vec(page)?;
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&json)?;
        let content_gzip = encoder.finish()?;
        let fetched_at = Utc::now().to_rfc3339();
        let conn = self.db.get()?;

        conn.execute(
            "INSERT OR REPLACE INTO openstax_cache (
                cache_key, book_uuid, page_uuid, content_gzip, archive_release, fetched_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                cache_key,
                book_uuid,
                page.page_uuid,
                content_gzip,
                release_key,
                fetched_at
            ],
        )?;

        Ok(())
    }
}

pub fn catalog() -> AppResult<Vec<OpenStaxBook>> {
    let catalog: Catalog =
        serde_json::from_str(include_str!("../../resources/catalog/openstax.json"))?;
    Ok(catalog.books)
}

pub async fn import_book<F>(
    db: DbPool,
    book_uuid: &str,
    on_progress: F,
) -> AppResult<DocumentBuilder>
where
    F: Fn(u32, u32) + Send,
{
    let client = OpenStaxClient::new(db);
    let catalog_book = catalog()?
        .into_iter()
        .find(|book| book.uuid == book_uuid)
        .ok_or_else(|| AppError::OpenStax(format!("unknown OpenStax book: {book_uuid}")))?;
    let toc = client.fetch_toc(book_uuid).await?;
    let pages = client.fetch_book(book_uuid, on_progress).await?;
    let sections = sections_from_pages(&client, book_uuid, &toc, &pages).await?;

    if sections.is_empty() {
        return Err(AppError::OpenStax(
            "OpenStax book did not contain readable text".into(),
        ));
    }

    Ok(DocumentBuilder {
        title: catalog_book.title,
        source_type: SourceType::Openstax,
        source_metadata: json!({
            "book_uuid": toc.book_uuid,
            "slug": catalog_book.slug,
            "release": toc.release,
            "language": toc.language,
            "imported_at": Utc::now().to_rfc3339()
        }),
        cover_image_path: None,
        license: Some(catalog_book.license),
        attribution: Some(format!("https://openstax.org/books/{}", catalog_book.slug)),
        sections,
    })
}

async fn sections_from_pages(
    client: &OpenStaxClient,
    book_uuid: &str,
    toc: &BookToc,
    pages: &[PageContent],
) -> AppResult<Vec<SectionBuilder>> {
    let mut sections = Vec::new();

    for page in pages {
        let base_url = client.page_url(book_uuid, &page.page_uuid).await?;
        let (paragraphs, image_candidates) = section_content_from_html(&page.html, &base_url);
        let images = download_images(&client.http, image_candidates).await?;
        if paragraphs.is_empty() && images.is_empty() {
            continue;
        }

        sections.push(SectionBuilder {
            title: section_title_for_page(toc, page),
            paragraphs,
            images,
        });
    }

    Ok(sections)
}

fn section_title_for_page(toc: &BookToc, page: &PageContent) -> String {
    toc.pages
        .iter()
        .find(|entry| entry.page_uuid == page.page_uuid)
        .map(|entry| entry.title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| page.title.clone())
}

fn collect_toc_entries(node: &TocNode, depth: u8, pages: &mut Vec<TocEntry>) {
    if node.contents.is_empty() && node.toc_type.as_deref() == Some("book-content") {
        pages.push(TocEntry {
            page_uuid: strip_version(&node.id),
            title: html_fragment_to_text(&node.title),
            depth,
        });
    }

    for child in &node.contents {
        collect_toc_entries(child, depth.saturating_add(1), pages);
    }
}

fn strip_version(id: &str) -> String {
    id.split('@').next().unwrap_or(id).to_string()
}

fn release_key(release: &ReleaseManifest, version: &str) -> String {
    format!("{}@{}", release.archive_url, version)
}

fn should_retry_status(status: StatusCode) -> bool {
    status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// OpenStax marks structure with `data-type` attributes. Figure captions,
/// tables, worked examples and exercises are dropped from the reading flow —
/// but a figure's *image* is kept, which is why only the paragraph rule applies.
struct OpenStaxSource;

impl SectionSource for OpenStaxSource {
    fn should_skip_paragraph(&self, element: &ElementRef<'_>) -> bool {
        should_skip_element(element)
    }
}

pub fn paragraphs_from_html(html: &str) -> Vec<String> {
    html_section::paragraphs_from_html(html, &OpenStaxSource)
}

fn section_content_from_html(html: &str, base_url: &str) -> (Vec<String>, Vec<SourceImage>) {
    html_section::section_content_from_html(html, base_url, &OpenStaxSource)
}

fn should_skip_element(element: &ElementRef<'_>) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|node| {
            let name = node.value().name();
            name == "figure"
                || name == "table"
                || (name == "aside" && node.value().attr("data-type") == Some("example"))
                || (name == "div" && node.value().attr("data-type") == Some("exercise"))
        })
}

fn html_fragment_to_text(html: &str) -> String {
    let fragment = Html::parse_fragment(html);
    normalize_text(&fragment.root_element().text().collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests {
    use super::{paragraphs_from_html, section_content_from_html};

    #[test]
    fn paragraphs_preserve_mathml_tokens_for_reader_rendering() {
        let html = r#"
            <p>
                Given
                <math display="inline"><mrow><mi>x</mi><mo>=</mo><mn>2</mn></mrow></math>
                as a value.
            </p>
        "#;

        let paragraphs = paragraphs_from_html(html);

        assert_eq!(paragraphs.len(), 1);
        assert!(paragraphs[0].starts_with("Given [[mathml:"));
        assert!(paragraphs[0].ends_with("]] as a value."));
        assert!(!paragraphs[0].contains("equation"));
    }

    #[test]
    fn section_images_keep_text_position() {
        let html = r#"
            <p>Cells store information in DNA.</p>
            <div class="os-figure">
                <figure>
                    <span data-type="media" data-alt="Detailed cell diagram">
                        <img src="../resources/cell-image"
                             data-media-type="image/png"
                             alt="Cell diagram" />
                    </span>
                </figure>
                <div class="os-caption-container">
                    <span class="os-title-label">Figure </span>
                    <span class="os-number">1.2</span>
                    <span class="os-caption">A labeled cell diagram.</span>
                </div>
            </div>
            <p>Proteins perform many cellular functions.</p>
        "#;

        let (paragraphs, images) = section_content_from_html(
            html,
            "https://openstax.org/apps/archive/20260407.195030/contents/book@version:page.json",
        );

        assert_eq!(
            paragraphs,
            vec![
                "Cells store information in DNA.",
                "Proteins perform many cellular functions."
            ]
        );
        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0].url,
            "https://openstax.org/apps/archive/20260407.195030/resources/cell-image"
        );
        assert_eq!(images[0].anchor_paragraph_ordinal, Some(0));
    }
}
