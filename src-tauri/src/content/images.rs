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
/// `url` is resolved against `book_url` first, the same way a Figure's `src`
/// is resolved against the page it sits on. WordPress emits root-relative and
/// protocol-relative image URLs, and a bare `Url::parse` of one fails -- which
/// would arrive as a book that simply has no cover.
///
/// `None` means no cover, never a failed Import. A book is readable without
/// its cover, so nothing here is worth failing an Import over.
pub async fn download_cover(
    http: &Client,
    covers_dir: &Path,
    book_url: &str,
    url: &str,
) -> Option<String> {
    download_image(
        http,
        covers_dir,
        SourceImage {
            url: resolve_url(book_url, url)?,
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

    // Decided on the body, and on what the server said about it -- never on the
    // URL alone. A URL extension names a file; it is not evidence of what came
    // back. Pressbooks networks sit behind a WAF that answers an unrecognised
    // client with an HTML error page, and a `200 text/html` block page for
    // `.../cover.png` would otherwise be written to disk as a PNG and hung on
    // the Library card.
    //
    // Either signal is enough on its own, because each covers the other's blind
    // spot. Servers that will not guess answer a genuine PNG as
    // `application/octet-stream`, so the bytes have to be able to speak for
    // themselves; and a format not sniffed here -- AVIF, say -- is still an
    // image when the server says so, so a sniff miss must not drop it.
    //
    // The cost of asking the body is that a response has to be read before it
    // can be refused, where a header rule could refuse it unread. There is no
    // safe way to shortcut that -- a non-image content type is exactly what a
    // real PNG arrives with from a server that will not guess -- so the size
    // cap above is what bounds it, and one decision on all the evidence is
    // worth more than two that can drift apart.
    let sniffed = sniffed_image_extension(&bytes);
    if sniffed.is_none() && content_type.is_none() {
        return None;
    }

    let extension = sniffed
        .map(str::to_string)
        .or_else(|| content_type.as_deref().and_then(extension_for_media_type))
        .or_else(|| extension_from_url(&candidate.url))
        .unwrap_or_else(|| "bin".to_string());

    let path = images_dir.join(format!("{}.{}", Uuid::new_v4(), extension));
    std::fs::write(&path, bytes).ok()?;

    Some(ImageBuilder {
        source_url: candidate.url,
        local_path: path.to_string_lossy().to_string(),
        alt_text: candidate.alt_text,
        caption: candidate.caption,
        // The sniffed type in preference to the claimed one, for the same
        // reason the extension is: it is the one derived from the actual image.
        content_type: sniffed.and_then(media_type_for_extension).or(content_type),
        anchor_paragraph_ordinal: candidate.anchor_paragraph_ordinal,
    })
}

/// The file extension the body's own leading bytes identify, if any.
///
/// Deliberately not exhaustive. It has to recognise the formats this module
/// already names extensions for, so a correctly served image is never worse off
/// for being sniffed -- beyond that, an unrecognised body is not a rejection,
/// only an absence of evidence.
fn sniffed_image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("jpg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("gif");
    }
    // RIFF containers carry their format in bytes 8..12; only WEBP is an image.
    if bytes.starts_with(b"RIFF") && bytes.len() >= 12 && &bytes[8..12] == b"WEBP" {
        return Some("webp");
    }
    if looks_like_svg(bytes) {
        return Some("svg");
    }

    None
}

/// Whether the body opens as an SVG document.
///
/// SVG is the one format here with no binary signature, so it is recognised by
/// its root element instead. An XML declaration may come first, and only then is
/// the opening tag worth searching for -- checking for `<svg` anywhere would
/// match an HTML page that happens to inline an icon.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let body = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let start = body
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(&[][..], |offset| &body[offset..]);
    let lower = start[..start.len().min(SVG_SNIFF_BYTES)].to_ascii_lowercase();

    if lower.starts_with(b"<svg") {
        return true;
    }

    // An XML declaration or an SVG doctype may stand ahead of the root element,
    // and only after one of those is the opening tag worth searching for.
    // Searching any document for `<svg` would match an HTML page that inlines an
    // icon; `<!doctype svg` is asked for by name so `<!doctype html` cannot.
    (lower.starts_with(b"<?xml") || lower.starts_with(b"<!doctype svg"))
        && lower.windows(4).any(|window| window == b"<svg")
}

/// How far into a document to look for an SVG root element. Enough for an XML
/// declaration, a doctype and a comment or two ahead of the opening tag.
const SVG_SNIFF_BYTES: usize = 1024;

/// The media type a sniffed extension stands for, inverse of
/// `extension_for_media_type` over the extensions this module produces.
fn media_type_for_extension(extension: &str) -> Option<String> {
    match extension {
        "jpg" => Some("image/jpeg".to_string()),
        "png" => Some("image/png".to_string()),
        "gif" => Some("image/gif".to_string()),
        "webp" => Some("image/webp".to_string()),
        "svg" => Some("image/svg+xml".to_string()),
        _ => None,
    }
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
    use std::path::Path;

    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    use super::{download_cover, source_images_from_html};

    const COVER_BYTES: &[u8] = b"-- a cover --";

    /// A PNG's eight-byte signature, then enough to be a plausible body.
    const PNG_BYTES: &[u8] = b"\x89PNG\r\n\x1a\n\x00\x00\x00\rIHDR-- pixels --";

    /// What a WAF answers an unrecognised client with. Both `images.rs` and
    /// `pressbooks.rs` document that Pressbooks networks sit behind one.
    const BLOCK_PAGE: &[u8] =
        b"<!DOCTYPE html><html><head><title>403 Forbidden</title></head><body>Request blocked.</body></html>";

    /// A server answering `at` with `body` and an optional content type.
    async fn server_answering(
        at: &'static str,
        body: &'static [u8],
        content_type: Option<&str>,
    ) -> MockServer {
        let server = MockServer::start().await;
        let mut response = ResponseTemplate::new(200).set_body_bytes(body);
        if let Some(content_type) = content_type {
            response = response.insert_header("content-type", content_type);
        }
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    async fn cover_from(server: &MockServer, covers: &Path, at: &str) -> Option<String> {
        download_cover(
            &reqwest::Client::new(),
            covers,
            &format!("{}/book/", server.uri()),
            at,
        )
        .await
    }

    #[tokio::test]
    async fn an_error_page_served_for_an_image_url_is_not_stored() {
        // The URL says `.png` and the status says 200, so the extension alone
        // would accept this and hang an HTML page on the Library card.
        let server =
            server_answering("/cover.png", BLOCK_PAGE, Some("text/html; charset=utf-8")).await;
        let covers = tempfile::tempdir().expect("temporary covers directory");

        assert_eq!(cover_from(&server, covers.path(), "/cover.png").await, None);
        assert_eq!(
            std::fs::read_dir(covers.path())
                .expect("the covers directory should be readable")
                .count(),
            0,
            "nothing should have been written to disk"
        );
    }

    #[tokio::test]
    async fn an_error_page_with_no_content_type_at_all_is_not_stored() {
        let server = server_answering("/cover.png", BLOCK_PAGE, None).await;
        let covers = tempfile::tempdir().expect("temporary covers directory");

        assert_eq!(cover_from(&server, covers.path(), "/cover.png").await, None);
    }

    #[tokio::test]
    async fn an_image_served_with_a_generic_content_type_is_still_stored() {
        // Some servers will not guess, and answer every file as octet-stream.
        // Refusing those would drop Figures that import correctly today.
        let server =
            server_answering("/cover.png", PNG_BYTES, Some("application/octet-stream")).await;
        let covers = tempfile::tempdir().expect("temporary covers directory");

        let stored = cover_from(&server, covers.path(), "/cover.png")
            .await
            .expect("a real image should be stored whatever the header says");

        assert!(stored.ends_with(".png"), "{stored}");
        assert_eq!(
            std::fs::read(&stored).expect("the cover should be on disk"),
            PNG_BYTES
        );
    }

    #[tokio::test]
    async fn an_image_served_with_no_content_type_is_still_stored() {
        let server = server_answering("/cover.png", PNG_BYTES, None).await;
        let covers = tempfile::tempdir().expect("temporary covers directory");

        let stored = cover_from(&server, covers.path(), "/cover.png")
            .await
            .expect("a real image should be stored with no header at all");

        assert!(stored.ends_with(".png"), "{stored}");
    }

    /// A server answering `/cover.png` with an image.
    async fn server_with_cover() -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cover.png"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(COVER_BYTES),
            )
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_cover_url_relative_to_the_book_is_resolved_before_it_is_fetched() {
        // WordPress emits root-relative and protocol-relative image URLs, and a
        // Source hands this whatever its metadata carries. Fetching that
        // verbatim fails to parse, and a swallowed parse failure looks exactly
        // like a book that has no cover.
        let server = server_with_cover().await;
        let covers = tempfile::tempdir().expect("temporary covers directory");
        let http = reqwest::Client::new();

        let stored = download_cover(
            &http,
            covers.path(),
            &format!("{}/book/", server.uri()),
            "/cover.png",
        )
        .await
        .expect("a relative cover should be resolved and fetched");

        assert_eq!(
            std::fs::read(&stored).expect("the cover should be on disk"),
            COVER_BYTES
        );
    }

    #[tokio::test]
    async fn an_absolute_cover_url_is_fetched_as_given() {
        let server = server_with_cover().await;
        let covers = tempfile::tempdir().expect("temporary covers directory");
        let http = reqwest::Client::new();

        let stored = download_cover(
            &http,
            covers.path(),
            &format!("{}/book/", server.uri()),
            &format!("{}/cover.png", server.uri()),
        )
        .await
        .expect("an absolute cover should be fetched");

        assert_eq!(
            std::fs::read(&stored).expect("the cover should be on disk"),
            COVER_BYTES
        );
    }

    #[tokio::test]
    async fn an_image_the_url_and_the_headers_both_say_nothing_about_is_stored() {
        // Nothing but the body identifies this: no extension on the URL, no
        // content type on the response. OpenStax serves figures from
        // extension-less resource paths, and today they are dropped.
        let server = server_answering("/resources/cell-image", PNG_BYTES, None).await;
        let covers = tempfile::tempdir().expect("temporary covers directory");

        let stored = cover_from(&server, covers.path(), "/resources/cell-image")
            .await
            .expect("the body alone should be enough to identify an image");

        assert!(stored.ends_with(".png"), "{stored}");
    }

    #[test]
    fn recognises_the_formats_it_names_extensions_for() {
        use super::sniffed_image_extension as sniff;

        assert_eq!(sniff(b"\x89PNG\r\n\x1a\nrest"), Some("png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00]), Some("jpg"));
        assert_eq!(sniff(b"GIF89a...."), Some("gif"));
        assert_eq!(sniff(b"GIF87a...."), Some("gif"));
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WEBPVP8 "), Some("webp"));
    }

    #[test]
    fn recognises_svg_through_a_bom_a_declaration_and_leading_space() {
        use super::sniffed_image_extension as sniff;

        assert_eq!(
            sniff(br#"<svg xmlns="http://www.w3.org/2000/svg"/>"#),
            Some("svg")
        );
        assert_eq!(
            sniff(b"<?xml version=\"1.0\"?>\n<!-- drawn by hand -->\n<svg/>"),
            Some("svg")
        );
        assert_eq!(sniff(b"\xEF\xBB\xBF\n  <SVG/>"), Some("svg"));
        // A doctype and no declaration at all, which is how older SVG files open.
        assert_eq!(
            sniff(b"<!DOCTYPE svg PUBLIC \"-//W3C//DTD SVG 1.1//EN\" \"svg11.dtd\">\n<svg/>"),
            Some("svg")
        );
    }

    #[test]
    fn does_not_mistake_a_page_for_an_image() {
        use super::sniffed_image_extension as sniff;

        assert_eq!(sniff(BLOCK_PAGE), None);
        // An HTML page may inline an icon. Searching the whole head for `<svg`
        // rather than requiring it to open the document would match this.
        assert_eq!(
            sniff(b"<!DOCTYPE html><html><body><svg viewBox=\"0 0 1 1\"/></body></html>"),
            None
        );
        assert_eq!(sniff(b"{\"error\":\"forbidden\"}"), None);
        // RIFF is a container, and most of what it carries is not an image.
        assert_eq!(sniff(b"RIFF\x00\x00\x00\x00WAVEfmt "), None);
        assert_eq!(sniff(b""), None);
    }

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
