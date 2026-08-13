use std::fs::File;
use std::io::Read;
use std::path::Path;

use chrono::Utc;
use epub::doc::EpubDoc;
use scraper::{ElementRef, Html, Selector};
use serde_json::json;
use uuid::Uuid;
use zip::ZipArchive;

use crate::content::document::{DocumentBuilder, SectionBuilder};
use crate::content::html_section::{self, SectionSource};
use crate::content::split_paragraphs;
use crate::db::models::SourceType;
use crate::error::{AppError, AppResult};
use crate::paths;

pub fn import_from_path(path: &Path) -> AppResult<DocumentBuilder> {
    detect_drm(path)?;

    let mut doc = EpubDoc::new(path)?;
    let title = doc
        .get_title()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|name| name.to_str())
                .map_or_else(|| "EPUB".to_string(), ToOwned::to_owned)
        });
    let cover_image_path = save_cover(&mut doc)?;
    let mut sections = Vec::new();

    for index in 0..doc.get_num_chapters() {
        if !doc.set_current_chapter(index) {
            continue;
        }

        let Some((html, mime)) = doc.get_current_str() else {
            continue;
        };
        if !mime.contains("html") && !mime.contains("xml") {
            continue;
        }

        if let Some(section) = section_from_html(&html, index) {
            sections.push(section);
        }
    }

    if sections.is_empty() {
        return Err(AppError::InvalidInput(
            "EPUB did not contain readable text".into(),
        ));
    }

    Ok(DocumentBuilder {
        title,
        source_type: SourceType::Epub,
        source_metadata: json!({
            "file_path": path.to_string_lossy(),
            "imported_at": Utc::now().to_rfc3339()
        }),
        cover_image_path,
        license: None,
        attribution: None,
        sections,
    })
}

fn detect_drm(path: &Path) -> AppResult<()> {
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)
        .map_err(|error| AppError::InvalidInput(format!("invalid EPUB archive: {error}")))?;

    let Ok(mut encryption) = archive.by_name("META-INF/encryption.xml") else {
        return Ok(());
    };

    let mut content = String::new();
    encryption.read_to_string(&mut content)?;
    if content.contains("http://www.adobe.com/adept") || content.contains("<EncryptedKey") {
        return Err(AppError::DrmProtected);
    }

    Ok(())
}

fn save_cover<R: Read + std::io::Seek>(doc: &mut EpubDoc<R>) -> AppResult<Option<String>> {
    let Some((bytes, mime)) = doc.get_cover() else {
        return Ok(None);
    };

    let extension = match mime.as_str() {
        "image/jpeg" | "image/jpg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "bin",
    };
    let path = paths::covers_dir()?.join(format!("{}.{}", Uuid::new_v4(), extension));
    std::fs::write(&path, bytes)?;
    Ok(Some(path.to_string_lossy().to_string()))
}

/// EPUB carries no source-specific chrome the way a MindTouch or OpenStax page
/// does, so it keeps everything the reader finds. Adopting the shared reader is
/// what gives EPUB imports MathML tokens, which its own parser never produced.
struct EpubSource;

impl SectionSource for EpubSource {
    fn should_skip_paragraph(&self, _element: &ElementRef<'_>) -> bool {
        false
    }
}

fn section_from_html(html: &str, index: usize) -> Option<SectionBuilder> {
    let document = Html::parse_document(html);
    let heading_selector = Selector::parse("h1, h2, h3").expect("valid heading selector");
    let title = document
        .select(&heading_selector)
        .find_map(|element| normalized_text(element.text()))
        .unwrap_or_else(|| format!("Section {}", index + 1));

    // Images are deliberately dropped here: an EPUB's are zip entries, not
    // URLs, so the shared downloader has nothing to fetch.
    let paragraphs: Vec<String> = html_section::paragraphs_from_html(html, &EpubSource)
        .into_iter()
        .filter(|text| *text != title)
        .flat_map(|text| split_paragraphs(&text))
        .collect();

    if paragraphs.is_empty() {
        None
    } else {
        Some(SectionBuilder::text(title, paragraphs))
    }
}

fn normalized_text<'a>(text: impl Iterator<Item = &'a str>) -> Option<String> {
    let value = text.collect::<Vec<_>>().join(" ");
    let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_chapter_into_paragraphs() {
        let section = section_from_html(
            "<h1>Chapter One</h1><p>First paragraph.</p><p>Second paragraph.</p>",
            0,
        )
        .expect("a section");

        assert_eq!(section.title, "Chapter One");
        assert_eq!(section.paragraphs.len(), 2);
    }

    #[test]
    fn now_preserves_mathml_the_way_the_other_importers_do() {
        // EPUB's own parser dropped math into bare glyphs. Adopting the shared
        // reader is what gives it tokens the reader can render with KaTeX.
        let section = section_from_html(
            "<h1>Maths</h1><p>Given <math><mi>x</mi></math> we continue.</p>",
            0,
        )
        .expect("a section");

        assert!(
            section.paragraphs.iter().any(|p| p.contains("[[mathml:")),
            "{:?}",
            section.paragraphs
        );
    }

    #[test]
    fn a_section_with_no_readable_text_is_dropped() {
        assert!(section_from_html("<div></div>", 0).is_none());
    }
}
