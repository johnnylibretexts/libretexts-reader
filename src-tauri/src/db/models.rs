#![allow(dead_code)]

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub title: String,
    pub source_type: SourceType,
    pub source_metadata: serde_json::Value,
    pub cover_image_path: Option<String>,
    pub license: Option<String>,
    pub attribution: Option<String>,
    pub word_count: u32,
    pub imported_at: DateTime<Utc>,
    pub last_opened_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType {
    Openstax,
    Libretexts,
    Epub,
    Pdf,
    Pasted,
    Url,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    pub id: String,
    pub document_id: String,
    pub ordinal: u32,
    pub title: String,
    pub word_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Paragraph {
    pub id: String,
    pub section_id: String,
    pub ordinal: u32,
    pub text: String,
    pub sentence_offsets: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaybackState {
    pub document_id: String,
    pub section_id: String,
    pub paragraph_id: String,
    pub sentence_index: u32,
    pub sentence_offset_ms: u32,
    pub voice_id: String,
    pub speed: f32,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub id: String,
    pub display_name: String,
    pub language: String,
    pub gender: String,
    pub is_bundled: bool,
    pub is_downloaded: bool,
    pub size_bytes: u64,
    pub preview_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenStaxBook {
    pub uuid: String,
    pub slug: String,
    pub title: String,
    pub subject: String,
    pub edition: String,
    pub cover_url: Option<String>,
    pub license: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibreTextsBook {
    pub book_id: String,
    pub title: String,
    pub author: String,
    pub affiliation: String,
    pub library: String,
    pub subject: String,
    pub license: String,
    pub summary: String,
    pub thumbnail: Option<String>,
    pub online_url: Option<String>,
    pub last_updated: Option<String>,
    pub location: String,
    pub program: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibreTextsLibrary {
    pub subdomain: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportProgress {
    pub document_id: String,
    pub stage: ImportStage,
    pub current: u32,
    pub total: u32,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportStage {
    Fetching,
    Parsing,
    Tokenizing,
    Storing,
    Complete,
    Failed,
}
