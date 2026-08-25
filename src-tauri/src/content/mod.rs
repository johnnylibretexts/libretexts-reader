pub mod article;
pub mod cancel;
pub mod document;
pub mod epub;
pub mod html_section;
pub mod images;
pub mod language;
pub mod libretexts;
pub mod normalize;
pub mod openstax;
pub mod pdf;
pub mod pressbooks;
pub mod remote;
pub mod tokenize;

use std::sync::OnceLock;

use chrono::Utc;
use regex::Regex;
use serde_json::json;

use crate::db::models::SourceType;
use crate::error::{AppError, AppResult};

use document::{DocumentBuilder, SectionBuilder};
use language::detect_source_language;

static PARAGRAPH_SPLIT_RE: OnceLock<Regex> = OnceLock::new();

pub fn import_pasted(title: &str, text: &str) -> AppResult<DocumentBuilder> {
    let title = title.trim();
    if title.is_empty() {
        return Err(AppError::InvalidInput("title is required".into()));
    }

    let paragraphs = split_paragraphs(text);
    if paragraphs.is_empty() {
        return Err(AppError::InvalidInput("text is required".into()));
    }

    Ok(DocumentBuilder {
        title: title.to_string(),
        source_type: SourceType::Pasted,
        source_metadata: json!({ "imported_at": Utc::now().to_rfc3339() }),
        source_language: detect_source_language(None, text),
        cover_image_path: None,
        license: None,
        attribution: None,
        sections: vec![SectionBuilder::text("Content", paragraphs)],
    })
}

pub(crate) fn split_paragraphs(text: &str) -> Vec<String> {
    paragraph_split_re()
        .split(text)
        .map(str::trim)
        .filter(|paragraph| !paragraph.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn paragraph_split_re() -> &'static Regex {
    PARAGRAPH_SPLIT_RE.get_or_init(|| Regex::new(r"\n[ \t\r]*\n+").expect("valid paragraph regex"))
}
