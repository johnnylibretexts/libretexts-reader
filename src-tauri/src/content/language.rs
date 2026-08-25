//! What language a book is written in.
//!
//! Declared metadata only, deliberately. Statistical detection over textbook
//! prose is a coin-flip on short samples and confidently wrong on bilingual
//! glossaries, and being wrong here silently translates a Spanish chapter
//! into Spanish. The `sample` parameter is reserved so a detector can be
//! added later without changing every call site.

use scraper::Html;

pub(crate) const DEFAULT_SOURCE_LANGUAGE: &str = "en";

pub(crate) fn detect_source_language(declared: Option<&str>, _sample: &str) -> String {
    declared
        .map(str::trim)
        .filter(|value| !value.is_empty())
        // BCP-47 down to the primary subtag: `es-MX` and `es` are one source
        // language as far as a translation pair is concerned.
        .and_then(|value| value.split(['-', '_']).next())
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .unwrap_or_else(|| DEFAULT_SOURCE_LANGUAGE.to_string())
}

/// Read the BCP-47 declaration from a complete HTML document, if it has one.
///
/// LibreTexts pages carry their language on the root element rather than in
/// the catalogue response. Keep extraction here so every HTML-backed importer
/// applies the same `lang` / XHTML `xml:lang` rules.
pub(crate) fn declared_html_language(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    let root = document.root_element();
    root.value()
        .attr("lang")
        .or_else(|| root.value().attr("xml:lang"))
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_declared_language_wins_over_the_text() {
        // EPUB `dc:language`, an OpenStax/LibreTexts metadata field, or an
        // html lang attribute. The publisher knows better than a heuristic.
        assert_eq!(
            detect_source_language(Some("es"), "This text is English."),
            "es"
        );
        assert_eq!(
            detect_source_language(Some("es-MX"), ""),
            "es",
            "region subtags are dropped"
        );
        assert_eq!(
            detect_source_language(Some("  ES  "), ""),
            "es",
            "trimmed and lowercased"
        );
    }

    #[test]
    fn an_undeclared_language_falls_back_to_english_not_to_null() {
        // Null would leave translation permanently unavailable with nothing on
        // screen explaining why. English is wrong sometimes and correctable in
        // the Reader; null is unusable and invisible.
        assert_eq!(
            detect_source_language(None, "Some prose with no declaration."),
            "en"
        );
        assert_eq!(detect_source_language(None, ""), "en");
    }

    #[test]
    fn a_declared_language_we_cannot_translate_from_is_still_recorded() {
        // Recording it is what lets the Reader say "no models for Welsh"
        // instead of silently pretending the book is English.
        assert_eq!(detect_source_language(Some("cy"), ""), "cy");
    }

    #[test]
    fn reads_a_language_declared_on_the_html_root() {
        assert_eq!(
            declared_html_language(r#"<html lang="fr-CA"><body>Texte</body></html>"#).as_deref(),
            Some("fr-CA")
        );
        assert_eq!(
            declared_html_language("<html><body>Text</body></html>"),
            None
        );
    }
}
