//! The Supertonic speech engine.
//!
//! Split so the parts that need no model can be tested without one: `chunk`,
//! `voice`, `audio` and `text` are pure, and the ONNX runtime sits behind the
//! `engine` seam. The `commands` layer above holds only the Tauri entry points.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::db::models::{Document, Section};
use crate::db::settings::default_export_directory;
use crate::error::AppResult;
use crate::tts::supertonic::voice::{
    normalize_language, normalize_voice_style, DEFAULT_LANGUAGE, DEFAULT_VOICE_STYLE,
};

pub mod audio;
pub mod cache;
pub mod chunk;
pub mod engine;
pub mod model;
pub mod provider;
pub mod text;
pub mod voice;

pub(crate) const SUPERTONIC_TOTAL_STEPS: usize = 8;
pub(crate) const SUPERTONIC_DEFAULT_SPEED: f32 = 1.0;
pub(crate) const SUPERTONIC_SILENCE_SECONDS: f32 = 0.3;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterRequest {
    pub document_id: String,
    pub section_id: String,
    pub voice_style: Option<String>,
    pub language: Option<String>,
    pub output_path: Option<String>,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterEstimate {
    pub word_count: u32,
    pub estimated_seconds: u32,
    pub chunk_count: u32,
    pub cached: bool,
    pub output_path: String,
}

#[derive(Debug)]
pub(crate) struct SupertonicConfig {
    pub(crate) voice_style: String,
    pub(crate) language: String,
    pub(crate) export_directory: String,
}

#[derive(Debug)]
pub(crate) struct ChapterMaterial {
    pub(crate) document: Document,
    pub(crate) section: Section,
    pub(crate) text: String,
}

impl SupertonicConfig {
    pub(crate) fn from_settings(values: &HashMap<String, JsonValue>) -> AppResult<Self> {
        let voice_style = normalize_voice_style(
            optional_setting_string(values, "supertonic_voice_style").as_deref(),
            DEFAULT_VOICE_STYLE,
        )
        .to_string();
        let language = normalize_language(
            optional_setting_string(values, "supertonic_language").as_deref(),
            DEFAULT_LANGUAGE,
        )
        .to_string();

        Ok(Self {
            voice_style,
            language,
            export_directory: setting_string(
                values,
                "export_directory",
                &default_export_directory(),
            ),
        })
    }
}

pub(crate) fn setting_string(
    values: &HashMap<String, JsonValue>,
    key: &str,
    fallback: &str,
) -> String {
    optional_setting_string(values, key).unwrap_or_else(|| fallback.to_string())
}

pub(crate) fn optional_setting_string(
    values: &HashMap<String, JsonValue>,
    key: &str,
) -> Option<String> {
    values
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
