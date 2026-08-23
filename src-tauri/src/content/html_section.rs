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
//!
//! A source can also recover notation an image carries rather than shows —
//! Pressbooks renders equations to pictures and keeps the LaTeX in the `alt` —
//! by answering `math_from_image`. That becomes a `[[latex:…]]` token standing
//! where the picture stood.

use std::sync::OnceLock;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};

use crate::content::images::{source_image_from_element, SourceImage};

static MATH_RE: OnceLock<Regex> = OnceLock::new();
static IMG_RE: OnceLock<Regex> = OnceLock::new();
static WHITESPACE_RE: OnceLock<Regex> = OnceLock::new();
static BLOCK_SELECTOR: OnceLock<Selector> = OnceLock::new();
static IMG_SELECTOR: OnceLock<Selector> = OnceLock::new();

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

    /// The notation an image carries rather than shows, as LaTeX.
    ///
    /// A source that renders equations to pictures answers here with the
    /// original notation, and it takes the picture's place in the paragraph.
    /// Such an image should also be skipped as a figure, or the reader gets
    /// the equation twice — once readable and once as a picture of itself.
    fn math_from_image(&self, _element: &ElementRef<'_>) -> Option<String> {
        None
    }

    /// Whether an extracted paragraph is worth keeping.
    fn is_readable(&self, text: &str) -> bool {
        !text.is_empty()
    }
}

/// Stands in for a table the paragraph-flow importer cannot represent.
///
/// Import is a reading flow, not a layout clone: `block_selector` matches
/// headings, paragraphs, list items and images, so a table built from bare
/// `<td>` cells was never selected and simply was not there. In an OpenStax or
/// LibreTexts STEM chapter that can be a large fraction of the page, and the
/// reader had no way to know. Silence reads as "the app is broken" rather than
/// "this app does not do tables yet".
///
/// Plain prose rather than a `[[...]]` token, and deliberately so. The math
/// tokens are base64 payloads that KaTeX typesets and the speech path degrades
/// to "equation"; there is no payload here to render, only an absence to
/// announce. Prose means it needs no decoder, cannot reach a reader or an
/// engine undecoded, and is already a whole sentence -- so it speaks correctly
/// and splits correctly with no special case anywhere downstream.
pub const OMITTED_TABLE: &str = "A table is omitted here.";

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
        } else if element.value().name() == "table" {
            // A nested table is part of the outer one the reader is already
            // being told about; marking it again would announce the same
            // absence twice.
            if !has_table_ancestor(&element) {
                paragraphs.push(OMITTED_TABLE.to_string());
            }
        } else if has_table_ancestor(&element) {
            // Cell text, after the marker already stood in for the whole
            // table. Emitting it too would read the row out as loose
            // sentences stripped of the column headings that gave them
            // meaning -- worse than the marker alone, and directly after it.
            continue;
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

    let text = text_with_math_replacements(element, source);
    source.is_readable(&text).then_some(text)
}

/// The ordinal of the paragraph an image follows, or None when it precedes them
/// all — in which case the reader renders it before the first paragraph.
fn anchor_paragraph_ordinal(paragraph_count: usize) -> Option<u32> {
    paragraph_count.checked_sub(1).map(|index| index as u32)
}

/// Swap math markup for a token *before* stripping tags, so the notation
/// survives text extraction instead of collapsing into its own glyphs — or, for
/// an image that carries its own source, instead of vanishing entirely.
fn text_with_math_replacements(element: &ElementRef<'_>, source: &dyn SectionSource) -> String {
    let html = element.html();
    let replaced = math_re().replace_all(&html, |captures: &regex::Captures<'_>| {
        format!(" {} ", mathml_token(&captures[0]))
    });
    // Asked of the real elements first, which costs nothing to parse. Sources
    // that recover no math — every source but Pressbooks — stop here, rather
    // than re-parsing each of a figure-heavy book's images to be told `None`.
    let replaced = if recovers_math_from_an_image(element, source) {
        img_re().replace_all(&replaced, |captures: &regex::Captures<'_>| {
            math_from_image_tag(&captures[0], source).map_or_else(
                || captures[0].to_string(),
                |latex| format!(" {} ", latex_token(&latex)),
            )
        })
    } else {
        replaced
    };

    let fragment = Html::parse_fragment(&replaced);
    normalize_text(&fragment.root_element().text().collect::<Vec<_>>().join(" "))
}

fn recovers_math_from_an_image(element: &ElementRef<'_>, source: &dyn SectionSource) -> bool {
    element
        .select(img_selector())
        .any(|image| source.math_from_image(&image).is_some())
}

/// Re-parse the one tag, so the source is handed an element with its attribute
/// values already entity-decoded rather than a slice of raw markup.
///
/// Matching the tag's text back to the element it came from would save the
/// parse, but a match the walk does not yield — inside a comment, say — would
/// shift every later image onto the wrong equation, silently.
fn math_from_image_tag(tag: &str, source: &dyn SectionSource) -> Option<String> {
    let fragment = Html::parse_fragment(tag);
    let image = fragment.select(img_selector()).next()?;
    source.math_from_image(&image)
}

fn mathml_token(markup: &str) -> String {
    format!("[[mathml:{}]]", BASE64_STANDARD.encode(markup.as_bytes()))
}

/// Base64 so the notation cannot be mistaken for prose on its way through the
/// sentence splitter, the speech normalizer and the reader.
fn latex_token(latex: &str) -> String {
    format!("[[latex:{}]]", BASE64_STANDARD.encode(latex.as_bytes()))
}

/// Collapse runs of whitespace, including the non-breaking spaces textbook HTML
/// is full of — `split_whitespace` leaves those in place.
pub fn normalize_text(text: &str) -> String {
    whitespace_re()
        .replace_all(&text.replace('\u{a0}', " "), " ")
        .trim()
        .to_string()
}

fn has_table_ancestor(element: &ElementRef<'_>) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|node| node.value().name() == "table")
}

fn block_selector() -> &'static Selector {
    BLOCK_SELECTOR.get_or_init(|| {
        Selector::parse("h1, h2, h3, h4, h5, h6, p, li, table, img[src], img[data-src]")
            .expect("valid block selector")
    })
}

fn math_re() -> &'static Regex {
    MATH_RE.get_or_init(|| {
        Regex::new(r"(?is)<(?:m:)?math\b.*?</(?:m:)?math>").expect("valid MathML regex")
    })
}

/// Quote-aware: `>` is legal and unescaped inside an attribute value, and
/// LaTeX is full of it, so `[^>]*` would cut a tag in half.
fn img_re() -> &'static Regex {
    IMG_RE.get_or_init(|| {
        Regex::new(r#"(?is)<img\b(?:"[^"]*"|'[^']*'|[^>"'])*>"#).expect("valid image tag regex")
    })
}

fn img_selector() -> &'static Selector {
    IMG_SELECTOR.get_or_init(|| Selector::parse("img").expect("valid image selector"))
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

    struct MathInAlt;
    impl SectionSource for MathInAlt {
        fn should_skip_paragraph(&self, _element: &ElementRef<'_>) -> bool {
            false
        }

        fn math_from_image(&self, element: &ElementRef<'_>) -> Option<String> {
            element
                .value()
                .attr("alt")
                .map(str::trim)
                .filter(|alt| !alt.is_empty())
                .map(str::to_string)
        }
    }

    #[test]
    fn a_table_leaves_a_marker_instead_of_vanishing() {
        // block_selector matches h1-h6, p, li and img -- a <table> is not in
        // it, so a table built from bare <td> cells was never selected and
        // simply was not there. In an OpenStax STEM chapter that can be a
        // large fraction of the page, with nothing telling the reader.
        let html = "<p>Before.</p>\
                    <table><tr><td>Mass</td><td>9.1e-31 kg</td></tr></table>\
                    <p>After.</p>";

        let paragraphs = paragraphs_from_html(html, &KeepEverything);

        assert_eq!(
            paragraphs,
            vec![
                "Before.".to_string(),
                OMITTED_TABLE.to_string(),
                "After.".to_string()
            ],
            "the marker must sit where the table was, in reading order"
        );
    }

    #[test]
    fn a_tables_own_paragraphs_do_not_escape_past_the_marker() {
        // A table whose cells hold <p> is the case that would otherwise emit a
        // marker AND the cell text, so the reader hears the row twice: once as
        // "a table is omitted here" and again as loose sentences with no
        // column headings to make sense of them.
        let html = "<p>Before.</p>\
                    <table><tr><td><p>Cell one.</p></td><td><p>Cell two.</p></td></tr></table>\
                    <p>After.</p>";

        let paragraphs = paragraphs_from_html(html, &KeepEverything);

        assert_eq!(
            paragraphs,
            vec![
                "Before.".to_string(),
                OMITTED_TABLE.to_string(),
                "After.".to_string()
            ]
        );
    }

    #[test]
    fn a_nested_table_marks_once_not_twice() {
        let html = "<table><tr><td><table><tr><td>Inner</td></tr></table></td></tr></table>";

        let paragraphs = paragraphs_from_html(html, &KeepEverything);

        assert_eq!(paragraphs, vec![OMITTED_TABLE.to_string()]);
    }

    #[test]
    fn an_image_inside_a_table_is_still_collected() {
        // The marker covers text the walker cannot represent. A figure is
        // representable and is downloaded and anchored as usual -- dropping it
        // would lose content the app can actually show.
        let html = "<p>Before.</p><table><tr><td><img src=\"/plot.png\"></td></tr></table>";

        let (paragraphs, images) =
            section_content_from_html(html, "https://example.test", &KeepEverything);

        assert_eq!(
            paragraphs,
            vec!["Before.".to_string(), OMITTED_TABLE.to_string()]
        );
        assert_eq!(images.len(), 1, "the plot inside the table is renderable");
    }

    fn decoded_latex_tokens(text: &str) -> Vec<String> {
        let token_re = Regex::new(r"\[\[latex:([A-Za-z0-9+/=]+)\]\]").expect("valid token regex");
        token_re
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
    fn a_source_can_recover_mathematics_an_image_carries() {
        // Pressbooks renders equations to pictures and keeps the LaTeX in the
        // alt. Without this the equation leaves no trace in the paragraph.
        let paragraphs = paragraphs_from_html(
            r#"<p>The ratio is <img src="eq.png" alt="\(\Theta=2\)" /> exactly.</p>"#,
            &MathInAlt,
        );

        assert_eq!(decoded_latex_tokens(&paragraphs[0]), vec![r"\(\Theta=2\)"]);
        assert!(paragraphs[0].starts_with("The ratio is"), "{paragraphs:?}");
        assert!(paragraphs[0].ends_with("exactly."), "{paragraphs:?}");
    }

    #[test]
    fn an_image_the_source_finds_no_mathematics_in_leaves_the_paragraph_alone() {
        let paragraphs = paragraphs_from_html(
            r#"<p>A photograph <img src="soil.jpg" alt="" /> follows.</p>"#,
            &MathInAlt,
        );

        assert_eq!(paragraphs, vec!["A photograph follows."]);
    }

    #[test]
    fn mathematics_containing_a_greater_than_sign_survives_the_tag_scan() {
        // `>` is legal, and unescaped, inside an attribute value. A naive
        // `<img[^>]*>` scan would cut the tag in half and lose the rest.
        let paragraphs = paragraphs_from_html(
            r#"<p>Given <img src="eq.png" alt="\(x > 5\)" /> it holds.</p>"#,
            &MathInAlt,
        );

        assert_eq!(decoded_latex_tokens(&paragraphs[0]), vec![r"\(x > 5\)"]);
    }

    #[test]
    fn a_source_that_recovers_no_mathematics_reads_exactly_as_before() {
        // The hook is opt-in: sources that do not implement it must see the
        // same text they saw before it existed.
        let html = r#"<p>Given <math><mi>x</mi></math> and <img src="a.png" alt="\(y\)" />.</p>"#;

        assert!(!paragraphs_from_html(html, &KeepEverything)[0].contains("[[latex:"));
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
