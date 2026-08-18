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
    Pressbooks,
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
    /// Display text: mathematics is preserved as `[[mathml:…]]` tokens and
    /// LaTeX for the reader to render.
    pub text: String,
    /// Byte offsets into `text`, one pair per sentence.
    pub sentence_offsets: Vec<(usize, usize)>,
    /// Speech text, one entry per `sentence_offsets` pair and in the same
    /// order: the same sentence with notation written out in words. Derived on
    /// read rather than stored, so no import is ever stale.
    pub sentence_speech: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionImage {
    pub id: String,
    pub section_id: String,
    pub ordinal: u32,
    pub source_url: String,
    pub local_path: String,
    pub alt_text: Option<String>,
    pub caption: Option<String>,
    pub content_type: Option<String>,
    pub anchor_paragraph_ordinal: Option<u32>,
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

/// One Pressbooks Catalog the application offers.
///
/// Pressbooks calls these "networks" and the picker uses that word, because it
/// is the publisher's own. The type is not named after it: the domain term for
/// what this is remains Catalog -- see `CONTEXT.md`.
///
/// `book_count` is what was observed when the bundled list was probed. It
/// conveys scale in the picker; the live count comes from the Catalog itself at
/// browse time.
///
/// `is_default` marks the one Catalog the browser opens on. It is carried on
/// the payload rather than left to the browser to infer from position, because
/// opening a Catalog crawls it: reading the default off the list's order makes
/// reordering that list silently cost a reader three hundred requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressbooksCatalog {
    pub host: String,
    pub name: String,
    pub book_count: u32,
    /// Set by `content::pressbooks::catalogs`, not by the bundled resource --
    /// `DEFAULT_NETWORK_HOST` is the single place the default is named.
    #[serde(default)]
    pub is_default: bool,
}

/// One book in a Pressbooks Catalog, as the browser shows it.
///
/// `book_url` is the book's canonical URL. It is the identity everywhere: the
/// row key, the value `source_metadata` carries on an imported Document, and
/// what the browser matches on to tell an already-imported book from a new one.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PressbooksBook {
    pub book_url: String,
    pub title: String,
    pub subtitle: Option<String>,
    pub cover_url: Option<String>,
    pub thumbnail_url: Option<String>,
    pub authors: String,
    pub license_name: String,
    pub license_url: Option<String>,
    pub word_count: u32,
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
