//! Reading an HTML page into paragraphs and figures.
//!
//! OpenStax and LibreTexts had each written this out: the same selector, the
//! same anchoring, and one function byte-identical between them. What genuinely
//! differs between sources is which elements count as chrome — so that is the
//! only thing a source has to supply.
//!
//! Math is handled here for everyone. `<math>` markup is swapped for a
//! `[[mathml:…]]` token before the tags are stripped, so it survives text
//! extraction and can be rendered later. Sources that serve LaTeX rather than
//! MathML (LibreTexts does) simply have nothing for it to match, and their
//! notation passes through as text for the reader and the speech pipeline to
//! handle downstream.

use std::sync::OnceLock;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::content::images::{source_image_from_element, SourceImage};

static MATH_RE: OnceLock<Regex> = OnceLock::new();
static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();
static BLOCK_SELECTOR: OnceLock<Selector> = OnceLock::new();

/// What a particular source considers chrome rather than content.
pub trait SectionSource {
    /// Whether this element's text should be dropped.
    fn should_skip_paragraph(&self, element: &ElementRef<'_>) -> bool;

    /// Whether this image should be dropped.
    ///
    /// Separate from the paragraph rule on purpose: OpenStax skips the *text*
    /// inside a `<figure>` so captions do not become paragraphs, while the
    /// image inside that same figure is exactly what it wants to keep.
    fn should_skip_image(&self, _element: &ElementRef<'_>) -> bool {
        false
    }

    /// Whether an extracted paragraph is worth keeping.
    fn is_readable(&self, text: &str) -> bool {
        !text.is_empty()
    }
}

/// Walk a page in source order, returning its paragraphs and its figures.
///
/// Each image is anchored to the paragraph it followed, which is what lets the
/// reader place figures in the reading flow rather than collecting them at the
/// top of a section.
pub fn section_content_from_html(
    html: &str,
    base_url: &str,
    source: &dyn SectionSource,
) -> (Vec<String>, Vec<SourceImage>) {
    let document = Html::parse_document(html);
    let mut paragraphs = Vec::new();
    let mut images = Vec::new();

    for element in document.select(block_selector()) {
        if element.value().name() == "img" {
            if source.should_skip_image(&element) {
                continue;
            }
            if let Some(mut image) = source_image_from_element(&element, base_url) {
                image.anchor_paragraph_ordinal = anchor_paragraph_ordinal(paragraphs.len());
                images.push(image);
            }
        } else if let Some(paragraph) = paragraph_from_element(&element, source) {
            paragraphs.push(paragraph);
        }
    }

    (paragraphs, images)
}

/// Paragraphs only, for sources with no images to download.
pub fn paragraphs_from_html(html: &str, source: &dyn SectionSource) -> Vec<String> {
    section_content_from_html(html, "", source).0
}

fn paragraph_from_element(element: &ElementRef<'_>, source: &dyn SectionSource) -> Option<String> {
    if source.should_skip_paragraph(element) {
        return None;
    }

    let text = text_with_math_replacements(element);
    source.is_readable(&text).then_some(text)
}

/// The ordinal of the paragraph an image follows, or None when it precedes them
/// all — in which case the reader renders it before the first paragraph.
fn anchor_paragraph_ordinal(paragraph_count: usize) -> Option<u32> {
    paragraph_count.checked_sub(1).map(|index| index as u32)
}

/// Swap `<math>` markup for a token *before* stripping tags, so the notation
/// survives text extraction instead of collapsing into its own glyphs.
fn text_with_math_replacements(element: &ElementRef<'_>) -> String {
    let html = element.html();
    let replaced = math_re().replace_all(&html, |captures: &regex::Captures<'_>| {
        format!(" {} ", mathml_token(&captures[0]))
    });
    let fragment = Html::parse_fragment(&replaced);
    normalize_text(&fragment.root_element().text().collect::<Vec<_>>().join(" "))
}

fn mathml_token(markup: &str) -> String {
    format!("[[mathml:{}]]", BASE64_STANDARD.encode(markup.as_bytes()))
}

/// Collapse runs of whitespace, including the non-breaking spaces textbook HTML
/// is full of — `split_whitespace` leaves those in place.
pub fn normalize_text(text: &str) -> String {
    whitespace_re()
        .replace_all(&text.replace('\u{a0}', " "), " ")
        .trim()
        .to_string()
}

fn block_selector() -> &'static Selector {
    BLOCK_SELECTOR.get_or_init(|| {
        Selector::parse("h1, h2, h3, h4, h5, h6, p, li, img[src], img[data-src]")
            .expect("valid block selector")
    })
}

fn math_re() -> &'static Regex {
    MATH_RE.get_or_init(|| {
        Regex::new(r"(?is)<(?:m:)?math\b.*?</(?:m:)?math>").expect("valid MathML regex")
    })
}

fn whitespace_re() -> &'static Regex {
    WHITESPACE_RE.get_or_init(|| Regex::new(r"\s+").expect("valid whitespace regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct KeepEverything;
    impl SectionSource for KeepEverything {
        fn should_skip_paragraph(&self, _element: &ElementRef<'_>) -> bool {
            false
        }
    }

    struct SkipAsides;
    impl SectionSource for SkipAsides {
        fn should_skip_paragraph(&self, element: &ElementRef<'_>) -> bool {
            element
                .ancestors()
                .filter_map(ElementRef::wrap)
                .any(|node| node.value().name() == "aside")
        }
    }

    #[test]
    fn reads_blocks_in_source_order() {
        let paragraphs = paragraphs_from_html(
            "<h1>Title</h1><p>First.</p><ul><li>Second.</li></ul><p>Third.</p>",
            &KeepEverything,
        );

        assert_eq!(paragraphs, vec!["Title", "First.", "Second.", "Third."]);
    }

    #[test]
    fn preserves_math_as_a_token() {
        let paragraphs = paragraphs_from_html(
            "<p>Given <math><mi>x</mi></math> we continue.</p>",
            &KeepEverything,
        );

        assert!(paragraphs[0].contains("[[mathml:"), "{paragraphs:?}");
        assert!(paragraphs[0].starts_with("Given"), "{paragraphs:?}");
        assert!(paragraphs[0].ends_with("we continue."), "{paragraphs:?}");
    }

    #[test]
    fn collapses_non_breaking_spaces() {
        // Textbook HTML is full of &nbsp;. Splitting on ASCII whitespace alone
        // leaves them embedded, and they reach the reader and the speech path.
        let paragraphs = paragraphs_from_html("<p>One\u{a0}\u{a0}two   three</p>", &KeepEverything);

        assert_eq!(paragraphs, vec!["One two three"]);
    }

    #[test]
    fn applies_the_source_skip_rule() {
        let html = "<p>Body.</p><aside><p>Sidebar.</p></aside><p>More.</p>";

        assert_eq!(
            paragraphs_from_html(html, &SkipAsides),
            vec!["Body.", "More."]
        );
        assert_eq!(paragraphs_from_html(html, &KeepEverything).len(), 3);
    }

    #[test]
    fn anchors_each_image_to_the_paragraph_it_followed() {
        let (paragraphs, images) = section_content_from_html(
            r#"<p>First.</p><img src="a.png"><p>Second.</p><img src="b.png">"#,
            "https://example.org/book/",
            &KeepEverything,
        );

        assert_eq!(paragraphs.len(), 2);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].anchor_paragraph_ordinal, Some(0));
        assert_eq!(images[1].anchor_paragraph_ordinal, Some(1));
    }

    #[test]
    fn an_image_before_any_paragraph_has_no_anchor() {
        let (_, images) = section_content_from_html(
            r#"<img src="cover.png"><p>First.</p>"#,
            "https://example.org/book/",
            &KeepEverything,
        );

        assert_eq!(images[0].anchor_paragraph_ordinal, None);
    }

    #[test]
    fn a_paragraph_skip_rule_does_not_drop_the_figure_image() {
        // The reason the two skip rules are separate: OpenStax drops figure
        // captions as paragraphs but keeps the figure's image.
        struct SkipFigureText;
        impl SectionSource for SkipFigureText {
            fn should_skip_paragraph(&self, element: &ElementRef<'_>) -> bool {
                element
                    .ancestors()
                    .filter_map(ElementRef::wrap)
                    .any(|node| node.value().name() == "figure")
            }
        }

        let (paragraphs, images) = section_content_from_html(
            r#"<p>Body.</p><figure><img src="d.png"><p>Figure 1. A caption.</p></figure>"#,
            "https://example.org/book/",
            &SkipFigureText,
        );

        assert_eq!(paragraphs, vec!["Body."]);
        assert_eq!(images.len(), 1, "the figure's image must survive");
    }
}
