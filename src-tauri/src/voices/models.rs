use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

pub use crate::net::download::file_matches_sha256;

use crate::error::{AppError, AppResult};
use crate::paths;

#[derive(Debug, Clone, Deserialize)]
pub struct ModelManifest {
    pub models: HashMap<String, ModelMetadata>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ModelMetadata {
    pub url: String,
    pub mirror: String,
    pub size_bytes: u64,
    pub sha256: String,
}

pub fn load_model_manifest() -> AppResult<ModelManifest> {
    if let Some(path) = std::env::var_os("LIBRETEXTS_READER_MODEL_MANIFEST_PATH") {
        let raw = std::fs::read_to_string(path)?;
        return Ok(serde_json::from_str(&raw)?);
    }

    let raw = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../models/manifest.json"
    ));
    Ok(serde_json::from_str(raw)?)
}

pub fn metadata_for(precision: &str) -> AppResult<ModelMetadata> {
    let manifest = load_model_manifest()?;
    manifest
        .models
        .get(precision)
        .cloned()
        .ok_or_else(|| AppError::InvalidInput(format!("unknown model precision: {precision}")))
}

pub fn model_path(precision: &str) -> AppResult<PathBuf> {
    Ok(paths::models_dir()?.join(model_file_name(precision)?))
}

pub fn model_file_name(precision: &str) -> AppResult<&'static str> {
    match precision {
        "fp32" => Ok("kokoro-fp32.onnx"),
        "q8" => Ok("kokoro-q8.onnx"),
        _ => Err(AppError::InvalidInput(format!(
            "unknown model precision: {precision}"
        ))),
    }
}
