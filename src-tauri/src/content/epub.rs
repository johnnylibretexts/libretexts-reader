use std::fs::File;
use std::io::Read;
use std::path::Path;

use chrono::Utc;
use epub::doc::EpubDoc;
use scraper::{Html, Selector};
use serde_json::json;
use uuid::Uuid;
use zip::ZipArchive;

use crate::content::document::{DocumentBuilder, SectionBuilder};
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

fn section_from_html(html: &str, index: usize) -> Option<SectionBuilder> {
    let document = Html::parse_document(html);
    let block_selector =
        Selector::parse("h1, h2, h3, h4, h5, h6, p, li").expect("valid EPUB block selector");
    let heading_selector = Selector::parse("h1, h2, h3").expect("valid heading selector");

    let title = document
        .select(&heading_selector)
        .find_map(|element| normalized_text(element.text()))
        .unwrap_or_else(|| format!("Section {}", index + 1));

    let mut paragraphs = Vec::new();
    for element in document.select(&block_selector) {
        let Some(text) = normalized_text(element.text()) else {
            continue;
        };
        if text == title {
            continue;
        }
        paragraphs.extend(split_paragraphs(&text));
    }

    if paragraphs.is_empty() {
        None
    } else {
        Some(SectionBuilder { title, paragraphs })
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
