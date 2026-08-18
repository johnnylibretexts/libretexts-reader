use std::collections::HashSet;
use std::path::Path;

use futures::StreamExt;
use reqwest::header::CONTENT_TYPE;
use reqwest::{Client, Url};
use scraper::{ElementRef, Html, Selector};
use uuid::Uuid;

use crate::content::document::ImageBuilder;
use crate::content::html_section::normalize_text;
use crate::error::AppResult;
use crate::paths;

const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct SourceImage {
    pub url: String,
    pub alt_text: Option<String>,
    pub caption: Option<String>,
    pub content_type_hint: Option<String>,
    pub anchor_paragraph_ordinal: Option<u32>,
}

pub fn source_images_from_html(html: &str, base_url: &str) -> Vec<SourceImage> {
    let document = Html::parse_document(html);
    let image_selector = Selector::parse("img[src], img[data-src]").expect("valid image selector");
    let mut images = Vec::new();

    for image in document.select(&image_selector) {
        if let Some(image) = source_image_from_element(&image, base_url) {
            images.push(image);
        }
    }

    images
}

pub fn source_image_from_element(image: &ElementRef<'_>, base_url: &str) -> Option<SourceImage> {
    if should_skip_image(image) {
        return None;
    }

    let src = image
        .value()
        .attr("src")
        .or_else(|| image.value().attr("data-src"))
        .map(str::trim)
        .filter(|src| !src.is_empty())?;
    if should_skip_src(src) {
        return None;
    }

    let url = resolve_url(base_url, src)?;

    Some(SourceImage {
        url,
        alt_text: image_text_attribute(image, "alt")
            .or_else(|| image_text_attribute(image, "data-alt")),
        caption: caption_for_image(image),
        content_type_hint: image
            .value()
            .attr("data-media-type")
            .map(normalize_text)
            .filter(|value| !value.is_empty()),
        anchor_paragraph_ordinal: None,
    })
}

pub async fn download_images(
    http: &Client,
    candidates: Vec<SourceImage>,
) -> AppResult<Vec<ImageBuilder>> {
    // Resolve nothing when there is nothing to download. `paths::images_dir`
    // calls `create_dir_all`, so asking for it materialises the real app-data
    // tree -- which a Source importing a book with no figures should not do,
    // and which makes any test of such an Import indistinguishable from real
    // usage on disk.
    if candidates.is_empty() {
        return Ok(Vec::new());
    }

    let images_dir = paths::images_dir()?;
    let mut seen = HashSet::new();
    let mut images = Vec::new();

    for candidate in candidates {
        if !seen.insert(candidate.url.clone()) {
            continue;
        }
        if let Some(image) = download_image(http, &images_dir, candidate).await {
            images.push(image);
        }
    }

    Ok(images)
}

/// Download a book's cover into `covers_dir`, returning where it was stored.
///
/// The same download as a Figure's -- same size cap, same content-type and
/// extension rules, same client -- because a cover is one more image from the
/// same host, and a second request path would be a second place for those
/// rules to drift. What differs is only where it lands and that it carries no
/// caption or anchor.
///
/// `covers_dir` is passed in rather than resolved here: `paths::covers_dir`
/// creates every directory it resolves, so resolving it inside this function
/// would make a test of a cover indistinguishable from real usage on disk.
///
/// `None` means no cover, never a failed Import. A book is readable without
/// its cover, so nothing here is worth failing an Import over.
pub async fn download_cover(http: &Client, covers_dir: &Path, url: &str) -> Option<String> {
    download_image(
        http,
        covers_dir,
        SourceImage {
            url: url.to_string(),
            alt_text: None,
            caption: None,
            content_type_hint: None,
            anchor_paragraph_ordinal: None,
        },
    )
    .await
    .map(|cover| cover.local_path)
}

async fn download_image(
    http: &Client,
    images_dir: &Path,
    candidate: SourceImage,
) -> Option<ImageBuilder> {
    let response = http.get(&candidate.url).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_IMAGE_BYTES)
    {
        return None;
    }

    let response_content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(media_type)
        .filter(|value| is_image_media_type(value));
    let content_type = response_content_type.or_else(|| {
        candidate
            .content_type_hint
            .as_deref()
            .map(media_type)
            .filter(|value| is_image_media_type(value))
    });
    let extension = content_type
        .as_deref()
        .and_then(extension_for_media_type)
        .or_else(|| extension_from_url(&candidate.url));
    if content_type.is_none() && extension.is_none() {
        return None;
    }

    // Stream the body and abort as soon as the cap is exceeded, rather than
    // buffering the whole response before checking its size.
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.ok()?;
        if bytes.len() as u64 + chunk.len() as u64 > MAX_IMAGE_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    if bytes.is_empty() {
        return None;
    }

    let path = images_dir.join(format!(
        "{}.{}",
        Uuid::new_v4(),
        extension.unwrap_or_else(|| "bin".to_string())
    ));
    std::fs::write(&path, bytes).ok()?;

    Some(ImageBuilder {
        source_url: candidate.url,
        local_path: path.to_string_lossy().to_string(),
        alt_text: candidate.alt_text,
        caption: candidate.caption,
        content_type,
        anchor_paragraph_ordinal: candidate.anchor_paragraph_ordinal,
    })
}

fn should_skip_src(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    lower.starts_with("data:")
        || lower.starts_with("blob:")
        || lower.starts_with("javascript:")
        || lower.starts_with("mailto:")
}

fn should_skip_image(image: &ElementRef<'_>) -> bool {
    image
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|node| node.value().classes().any(is_navigation_class))
}

fn is_navigation_class(class: &str) -> bool {
    matches!(
        class,
        "noindex"
            | "mt-category-container"
            | "mt-guide-content"
            | "mt-guide-listings"
            | "mt-list-topics"
            | "mt-listing-detailed"
            | "mt-sortable-listing"
            | "mt-sortable-listing-image"
            | "mt-sortable-listings-container"
            | "mt-subpage-listings-container"
            | "mt-topic-hierarchy-listings"
    )
}

fn resolve_url(base_url: &str, src: &str) -> Option<String> {
    Url::parse(base_url)
        .ok()?
        .join(src)
        .ok()
        .map(|url| url.to_string())
}

fn image_text_attribute(image: &ElementRef<'_>, attribute: &str) -> Option<String> {
    image
        .value()
        .attr(attribute)
        .map(normalize_text)
        .filter(|value| !value.is_empty())
        .or_else(|| {
            image
                .ancestors()
                .filter_map(ElementRef::wrap)
                .find_map(|node| {
                    node.value()
                        .attr(attribute)
                        .map(normalize_text)
                        .filter(|value| !value.is_empty())
                })
        })
}

fn caption_for_image(image: &ElementRef<'_>) -> Option<String> {
    let caption_selector = Selector::parse(
        "figcaption, [data-type='caption'], .os-caption-container, .caption, .mt-figure-caption",
    )
    .expect("valid image caption selector");

    image
        .ancestors()
        .filter_map(ElementRef::wrap)
        .filter(is_caption_scope)
        .find_map(|node| {
            node.select(&caption_selector)
                .find_map(|caption| normalized_element_text(&caption))
        })
}

fn is_caption_scope(node: &ElementRef<'_>) -> bool {
    node.value().name() == "figure"
        || node.value().attr("data-type") == Some("figure")
        || node.value().classes().any(|class| {
            matches!(
                class,
                "os-figure" | "figure" | "mt-figure" | "mt-figure-wrapper"
            )
        })
}

fn normalized_element_text(element: &ElementRef<'_>) -> Option<String> {
    let text = normalize_text(&element.text().collect::<Vec<_>>().join(" "));
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn media_type(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase()
}

fn is_image_media_type(value: &str) -> bool {
    value.starts_with("image/")
}

fn extension_for_media_type(value: &str) -> Option<String> {
    match value {
        "image/jpeg" | "image/jpg" => Some("jpg".to_string()),
        "image/png" => Some("png".to_string()),
        "image/gif" => Some("gif".to_string()),
        "image/webp" => Some("webp".to_string()),
        "image/svg+xml" => Some("svg".to_string()),
        _ => None,
    }
}

fn extension_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let file_name = parsed.path_segments()?.next_back()?;
    let extension = file_name.rsplit('.').next()?.to_ascii_lowercase();

    match extension.as_str() {
        "jpeg" | "jpg" => Some("jpg".to_string()),
        "png" => Some("png".to_string()),
        "gif" => Some("gif".to_string()),
        "webp" => Some("webp".to_string()),
        "svg" => Some("svg".to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::source_images_from_html;

    #[test]
    fn extracts_openstax_style_images_with_captions() {
        let html = r#"
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
        "#;

        let images = source_images_from_html(
            html,
            "https://openstax.org/apps/archive/20260407.195030/contents/book@version:page.json",
        );

        assert_eq!(images.len(), 1);
        assert_eq!(
            images[0].url,
            "https://openstax.org/apps/archive/20260407.195030/resources/cell-image"
        );
        assert_eq!(images[0].alt_text.as_deref(), Some("Cell diagram"));
        assert_eq!(
            images[0].caption.as_deref(),
            Some("Figure 1.2 A labeled cell diagram.")
        );
        assert_eq!(images[0].content_type_hint.as_deref(), Some("image/png"));
    }

    #[test]
    fn skips_inline_data_images() {
        let images = source_images_from_html(
            r#"<img src="data:image/png;base64,abc" alt="inline" />"#,
            "https://example.com/book/page",
        );

        assert!(images.is_empty());
    }

    #[test]
    fn skips_libretexts_listing_thumbnails() {
        let html = r#"
            <div class="mt-category-container mt-subpage-listings-container noindex">
                <ul class="mt-sortable-listings-container">
                    <li class="mt-sortable-listing" data-page-id="1">
                        <a href="/book/chapter">
                            <span class="mt-sortable-listing-image">
                                <img src="https://files.mtstatic.com/site/book-thumb.jpg" />
                            </span>
                            Chapter
                        </a>
                    </li>
                </ul>
            </div>
            <figure>
                <img src="/book/real-figure.jpg" alt="Real figure" />
            </figure>
        "#;

        let images = source_images_from_html(html, "https://example.com/book/page");

        assert_eq!(images.len(), 1);
        assert_eq!(images[0].url, "https://example.com/book/real-figure.jpg");
    }
}
