use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Cursor, Read};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use futures::StreamExt;
use hound::{SampleFormat, WavSpec, WavWriter};
use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};
use ndarray::{Array, Array3};
use once_cell::sync::Lazy;
use ort::{session::Session, value::Value as OrtValue};
use rand_distr::{Distribution, Normal};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use tauri::{Emitter, Runtime, State, Window};
use tokio::io::AsyncWriteExt;
use unicode_normalization::UnicodeNormalization;
use uuid::Uuid;

use crate::commands::tts::SpeechAudio;
use crate::content::normalize::normalize_for_tts;
use crate::db::connection::DbPool;
use crate::db::library;
use crate::db::models::{Document, Section};
use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::paths;

const SUPERTONIC_MODEL_ID: &str = "Supertone/supertonic-3";
const SUPERTONIC_MODEL_VERSION: &str = "supertonic-3";
const SUPERTONIC_USER_AGENT: &str = "johnny-reader/0.1 supertonic-model-downloader";
/// Abort a stalled model download if no chunk arrives within this window. An
/// overall request timeout is intentionally avoided so large files can finish.
const SUPERTONIC_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);
const SUPERTONIC_TOTAL_STEPS: usize = 8;
const SUPERTONIC_SAMPLE_RATE: u32 = 44_100;
const SUPERTONIC_DEFAULT_SPEED: f32 = 1.0;
const SUPERTONIC_SILENCE_SECONDS: f32 = 0.3;
const AUDIOBOOK_WORDS_PER_MINUTE: f64 = 165.0;
const SUPERTONIC_CACHE_VERSION: &str = "supertonic-tts-cache-v1";

pub const SUPERTONIC_LANGUAGES: &[&str] = &[
    "en", "ko", "ja", "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hi", "hr", "hu",
    "id", "it", "lt", "lv", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "tr", "uk", "vi", "na",
];

const SUPERTONIC_VOICE_STYLES: &[&str] =
    &["M1", "M2", "M3", "M4", "M5", "F1", "F2", "F3", "F4", "F5"];

const DEFAULT_VOICE_STYLE: &str = "M1";
const DEFAULT_LANGUAGE: &str = "en";

static SUPERTONIC_ENGINE: Lazy<Mutex<Option<CachedSupertonicEngine>>> =
    Lazy::new(|| Mutex::new(None));

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupertonicPreviewRequest {
    pub text: String,
    pub voice_style: Option<String>,
    pub language: Option<String>,
    pub document_title: Option<String>,
    pub section_title: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupertonicChapterRequest {
    pub document_id: String,
    pub section_id: String,
    pub voice_style: Option<String>,
    pub language: Option<String>,
    pub output_path: Option<String>,
    pub force: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupertonicChapterEstimate {
    pub word_count: u32,
    pub estimated_seconds: u32,
    pub chunk_count: u32,
    pub cached: bool,
    pub output_path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupertonicChapterExport {
    pub output_path: String,
    pub cached: bool,
    pub byte_length: u64,
    pub estimate: SupertonicChapterEstimate,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupertonicModelStatus {
    pub downloaded: bool,
    pub directory: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub missing_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SupertonicModelDownloadProgress {
    file: String,
    downloaded: u64,
    total: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct SupertonicModelManifest {
    files: Vec<SupertonicModelFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SupertonicModelFile {
    path: String,
    url: String,
    size_bytes: u64,
    sha256: String,
}

#[derive(Debug)]
struct SupertonicConfig {
    voice_style: String,
    language: String,
    export_directory: String,
}

#[derive(Debug)]
struct ChapterMaterial {
    document: Document,
    section: Section,
    text: String,
}

struct CachedSupertonicEngine {
    directory: PathBuf,
    engine: TextToSpeech,
}

#[tauri::command]
pub async fn get_supertonic_model_status() -> AppResult<SupertonicModelStatus> {
    supertonic_model_status()
}

#[tauri::command]
pub async fn ensure_supertonic_model_downloaded<R: Runtime>(
    window: Window<R>,
) -> AppResult<String> {
    let manifest = supertonic_model_manifest()?;
    let directory = supertonic_model_dir()?;
    tokio::fs::create_dir_all(&directory).await?;
    let total = manifest
        .files
        .iter()
        .map(|file| file.size_bytes)
        .sum::<u64>();
    let mut downloaded = existing_supertonic_model_bytes(&directory, &manifest.files);
    let client = reqwest::Client::builder()
        .user_agent(SUPERTONIC_USER_AGENT)
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()?;

    emit_supertonic_model_progress(&window, "Preparing", downloaded, total)?;
    for file in manifest.files {
        let target_path = supertonic_model_file_path(&directory, &file)?;
        if file_complete(&target_path, file.size_bytes, &file.sha256)? {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let temp_path = temp_download_path(&target_path)?;
        if temp_path.exists() {
            tokio::fs::remove_file(&temp_path).await?;
        }
        if target_path.exists() {
            tokio::fs::remove_file(&target_path).await?;
        }

        let response = client.get(&file.url).send().await?.error_for_status()?;
        let mut stream = response.bytes_stream();
        let mut output = tokio::fs::File::create(&temp_path).await?;
        let mut file_downloaded = 0_u64;

        loop {
            let next = tokio::time::timeout(SUPERTONIC_READ_TIMEOUT, stream.next())
                .await
                .map_err(|_| AppError::Model("Supertonic model download stalled".to_string()))?;
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk?;
            file_downloaded += chunk.len() as u64;
            // Abort during streaming as soon as the body exceeds the manifest
            // size, instead of writing past the expected size before checking.
            if file.size_bytes > 0 && file_downloaded > file.size_bytes {
                let _ = tokio::fs::remove_file(&temp_path).await;
                return Err(AppError::Model(format!(
                    "Supertonic model file size mismatch for {}: expected {} bytes",
                    file.path, file.size_bytes
                )));
            }
            output.write_all(&chunk).await?;
            emit_supertonic_model_progress(
                &window,
                &file.path,
                downloaded + file_downloaded,
                total.max(downloaded + file_downloaded),
            )?;
        }
        output.flush().await?;

        if file.size_bytes > 0 && file_downloaded != file.size_bytes {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AppError::Model(format!(
                "Supertonic model file size mismatch for {}: expected {} bytes, got {file_downloaded}",
                file.path, file.size_bytes
            )));
        }
        if !file_matches_sha256(&temp_path, &file.sha256)? {
            let _ = tokio::fs::remove_file(&temp_path).await;
            return Err(AppError::Model(format!(
                "Supertonic model file SHA-256 mismatch for {}",
                file.path
            )));
        }

        downloaded += file_downloaded;
        tokio::fs::rename(&temp_path, &target_path).await?;
    }

    let status = supertonic_model_status()?;
    if !status.downloaded {
        return Err(AppError::Model(
            "Supertonic model download is incomplete.".into(),
        ));
    }
    emit_supertonic_model_progress(
        &window,
        "Complete",
        status.downloaded_bytes,
        status.total_bytes,
    )?;
    Ok(status.directory)
}

#[tauri::command]
pub async fn preview_supertonic_tts(
    state: State<'_, DbPool>,
    request: SupertonicPreviewRequest,
) -> AppResult<SpeechAudio> {
    let text = normalize_for_tts(request.text.trim());
    if text.is_empty() {
        return Err(AppError::InvalidInput("preview text is required".into()));
    }

    let config = supertonic_config_from_state(&state)?;
    let voice_style = resolve_voice_style(request.voice_style.as_deref(), &config.voice_style)?;
    let language = resolve_language(request.language.as_deref(), &config.language)?;
    let _ = (&request.document_title, &request.section_title);

    synthesize_supertonic_audio(text, voice_style, language, SUPERTONIC_DEFAULT_SPEED).await
}

#[tauri::command]
pub async fn estimate_supertonic_chapter(
    state: State<'_, DbPool>,
    request: SupertonicChapterRequest,
) -> AppResult<SupertonicChapterEstimate> {
    let config = supertonic_config_from_state(&state)?;
    let material = chapter_material(&state, &request.document_id, &request.section_id)?;
    let voice_style = resolve_voice_style(request.voice_style.as_deref(), &config.voice_style)?;
    let language = resolve_language(request.language.as_deref(), &config.language)?;
    let output_path =
        output_path_for_chapter(&config, &material, &voice_style, &language, &request);
    let cache_path = cache_path_for_chapter(&material, &voice_style, &language)?;

    Ok(estimate_for_text(
        &material,
        &language,
        &output_path,
        cache_path.exists(),
    ))
}

#[tauri::command]
pub async fn export_supertonic_chapter_mp3(
    state: State<'_, DbPool>,
    request: SupertonicChapterRequest,
) -> AppResult<SupertonicChapterExport> {
    let config = supertonic_config_from_state(&state)?;
    let material = chapter_material(&state, &request.document_id, &request.section_id)?;
    let voice_style = resolve_voice_style(request.voice_style.as_deref(), &config.voice_style)?;
    let language = resolve_language(request.language.as_deref(), &config.language)?;
    let output_path =
        output_path_for_chapter(&config, &material, &voice_style, &language, &request);
    let cache_path = cache_path_for_chapter(&material, &voice_style, &language)?;
    let force = request.force.unwrap_or(false);

    if cache_path.exists() && !force {
        copy_cached_mp3(&cache_path, &output_path).await?;
        let bytes = tokio::fs::metadata(&output_path).await?.len();
        let estimate = estimate_for_text(&material, &language, &output_path, true);
        return Ok(SupertonicChapterExport {
            output_path: path_to_string(&output_path),
            cached: true,
            byte_length: bytes,
            estimate,
        });
    }

    if let Some(parent) = cache_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let text = material.text.clone();
    let mp3 = synthesize_supertonic_mp3(
        text,
        voice_style.clone(),
        language.clone(),
        SUPERTONIC_DEFAULT_SPEED,
    )
    .await?;
    if mp3.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty audio.".into()));
    }

    // Unique temp name so concurrent exports of the same chapter cannot write
    // to the same in-progress file before the atomic rename into place.
    let temp_path = cache_path.with_extension(format!("{}.mp3.download", Uuid::new_v4()));
    tokio::fs::write(&temp_path, &mp3).await?;
    tokio::fs::rename(&temp_path, &cache_path).await?;
    copy_cached_mp3(&cache_path, &output_path).await?;
    let estimate = estimate_for_text(&material, &language, &output_path, true);

    Ok(SupertonicChapterExport {
        output_path: path_to_string(&output_path),
        cached: false,
        byte_length: mp3.len() as u64,
        estimate,
    })
}

/// Playback synthesis, driven entirely by what the caller passes.
///
/// Deliberately lenient about voice and language rather than erroring the way
/// the preview and export commands do: the player carries one voice id across
/// engines, so a reader who switches from Kokoro to Supertonic mid-session can
/// briefly hold an id this engine does not know. Falling back beats cutting the
/// audio off. User-initiated commands still reject unknown values outright.
pub async fn synthesize_supertonic_text(
    text: &str,
    voice_id: Option<&str>,
    language: Option<&str>,
    speed: f32,
) -> AppResult<SpeechAudio> {
    let voice_style = playback_voice_style(voice_id, DEFAULT_VOICE_STYLE);
    let language = normalize_language(language, DEFAULT_LANGUAGE).to_string();

    synthesize_supertonic_audio(
        normalize_for_tts(text),
        voice_style,
        language,
        clamp_speed(speed),
    )
    .await
}

impl SupertonicConfig {
    fn from_settings(values: &HashMap<String, JsonValue>) -> AppResult<Self> {
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

fn supertonic_config_from_state(state: &State<'_, DbPool>) -> AppResult<SupertonicConfig> {
    let conn = state.get()?;
    SupertonicConfig::from_settings(&settings::get_all_settings(&conn)?)
}

fn chapter_material(
    state: &State<'_, DbPool>,
    document_id: &str,
    section_id: &str,
) -> AppResult<ChapterMaterial> {
    let conn = state.get()?;
    let document = library::get_document(&conn, document_id)?;
    let section = library::list_sections(&conn, document_id)?
        .into_iter()
        .find(|section| section.id == section_id)
        .ok_or_else(|| AppError::InvalidInput("section does not belong to document".into()))?;
    let paragraphs = library::list_paragraphs(&conn, section_id)?;
    let text = paragraphs
        .into_iter()
        .map(|paragraph| normalize_for_tts(paragraph.text.trim()))
        .filter(|paragraph| !paragraph.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n");

    if text.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "chapter has no readable text".into(),
        ));
    }

    Ok(ChapterMaterial {
        document,
        section,
        text,
    })
}

async fn synthesize_supertonic_audio(
    text: String,
    voice_style: String,
    language: String,
    speed: f32,
) -> AppResult<SpeechAudio> {
    let samples = synthesize_supertonic_samples(text, voice_style, language, speed).await?;
    let audio = encode_f32_to_wav(&samples, SUPERTONIC_SAMPLE_RATE)?;

    Ok(SpeechAudio {
        audio,
        mime_type: "audio/wav".to_string(),
    })
}

async fn synthesize_supertonic_mp3(
    text: String,
    voice_style: String,
    language: String,
    speed: f32,
) -> AppResult<Vec<u8>> {
    let samples = synthesize_supertonic_samples(text, voice_style, language, speed).await?;
    encode_f32_to_mp3(&samples, SUPERTONIC_SAMPLE_RATE)
}

async fn synthesize_supertonic_samples(
    text: String,
    voice_style: String,
    language: String,
    speed: f32,
) -> AppResult<Vec<f32>> {
    tokio::task::spawn_blocking(move || {
        synthesize_supertonic_samples_blocking(&text, &voice_style, &language, speed)
    })
    .await
    .map_err(|error| AppError::Tts(format!("Supertonic runtime task failed: {error}")))?
}

fn synthesize_supertonic_samples_blocking(
    text: &str,
    voice_style: &str,
    language: &str,
    speed: f32,
) -> AppResult<Vec<f32>> {
    if supertonic_fake_audio_enabled() {
        return Ok(fake_supertonic_samples(text));
    }

    ensure_supertonic_model_ready()?;
    let directory = supertonic_model_dir()?;
    let onnx_dir = directory.join("onnx");
    let style_path = directory
        .join("voice_styles")
        .join(format!("{voice_style}.json"));
    let style = load_voice_style(&[style_path])?;

    let mut guard = SUPERTONIC_ENGINE
        .lock()
        .map_err(|_| AppError::Tts("Supertonic runtime lock is poisoned.".into()))?;
    let should_reload = guard
        .as_ref()
        .is_none_or(|cached| cached.directory != directory);
    if should_reload {
        let engine = load_text_to_speech(&onnx_dir)?;
        *guard = Some(CachedSupertonicEngine {
            directory: directory.clone(),
            engine,
        });
    }

    let cached = guard
        .as_mut()
        .ok_or_else(|| AppError::Tts("Supertonic runtime did not initialize.".into()))?;
    let (samples, _duration) = cached.engine.call(
        text,
        language,
        &style,
        SUPERTONIC_TOTAL_STEPS,
        speed,
        SUPERTONIC_SILENCE_SECONDS,
    )?;
    if samples.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty audio.".into()));
    }
    Ok(samples)
}

fn ensure_supertonic_model_ready() -> AppResult<()> {
    let status = supertonic_model_status()?;
    if !status.downloaded {
        return Err(AppError::Model(
            "Supertonic model is not downloaded. Download it in Settings.".into(),
        ));
    }
    Ok(())
}

fn estimate_for_text(
    material: &ChapterMaterial,
    language: &str,
    output_path: &Path,
    cached: bool,
) -> SupertonicChapterEstimate {
    let chunks = chunk_text_for_language(&material.text, language);
    let word_count = count_words(&material.text) as u32;
    let estimated_seconds = ((word_count as f64 / AUDIOBOOK_WORDS_PER_MINUTE) * 60.0)
        .ceil()
        .max(1.0) as u32;

    SupertonicChapterEstimate {
        word_count,
        estimated_seconds,
        chunk_count: chunks.len() as u32,
        cached,
        output_path: path_to_string(output_path),
    }
}

fn output_path_for_chapter(
    config: &SupertonicConfig,
    material: &ChapterMaterial,
    voice_style: &str,
    language: &str,
    request: &SupertonicChapterRequest,
) -> PathBuf {
    if let Some(output_path) = request
        .output_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return PathBuf::from(output_path);
    }

    let directory = PathBuf::from(&config.export_directory)
        .join(sanitize_file_component(&material.document.title, 80));
    let filename = format!(
        "{:03} - {} - Supertonic - {} - {}.mp3",
        material.section.ordinal + 1,
        sanitize_file_component(&material.section.title, 72),
        sanitize_file_component(voice_style, 16),
        sanitize_file_component(language, 8)
    );
    directory.join(filename)
}

fn cache_path_for_chapter(
    material: &ChapterMaterial,
    voice_style: &str,
    language: &str,
) -> AppResult<PathBuf> {
    let mut hasher = Sha256::new();
    hasher.update(SUPERTONIC_CACHE_VERSION.as_bytes());
    hasher.update(SUPERTONIC_MODEL_VERSION.as_bytes());
    hasher.update(SUPERTONIC_TOTAL_STEPS.to_le_bytes());
    hasher.update(voice_style.as_bytes());
    hasher.update(language.as_bytes());
    hasher.update(material.document.id.as_bytes());
    hasher.update(material.section.id.as_bytes());
    hasher.update(material.text.as_bytes());
    let hash = hex::encode(hasher.finalize());
    Ok(paths::cache_dir()?
        .join("supertonic-tts")
        .join(format!("{hash}.mp3")))
}

async fn copy_cached_mp3(cache_path: &Path, output_path: &Path) -> AppResult<()> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if cache_path == output_path {
        return Ok(());
    }

    tokio::fs::copy(cache_path, output_path).await?;
    Ok(())
}

fn supertonic_model_status() -> AppResult<SupertonicModelStatus> {
    let manifest = supertonic_model_manifest()?;
    let directory = supertonic_model_dir()?;
    let total_bytes = manifest
        .files
        .iter()
        .map(|file| file.size_bytes)
        .sum::<u64>();
    let mut downloaded_bytes = 0_u64;
    let mut missing_files = Vec::new();

    for file in manifest.files {
        let target_path = supertonic_model_file_path(&directory, &file)?;
        match target_path.metadata() {
            Ok(metadata)
                if metadata.len() == file.size_bytes
                    && file_matches_sha256(&target_path, &file.sha256)? =>
            {
                downloaded_bytes += metadata.len();
            }
            Ok(metadata) => {
                downloaded_bytes += metadata.len();
                missing_files.push(file.path);
            }
            Err(_) => missing_files.push(file.path),
        }
    }

    Ok(SupertonicModelStatus {
        downloaded: missing_files.is_empty(),
        directory: directory.to_string_lossy().to_string(),
        downloaded_bytes,
        total_bytes,
        missing_files,
    })
}

fn existing_supertonic_model_bytes(directory: &Path, files: &[SupertonicModelFile]) -> u64 {
    files
        .iter()
        .filter_map(|file| supertonic_model_file_path(directory, file).ok())
        .filter_map(|path| path.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

fn supertonic_model_manifest() -> AppResult<SupertonicModelManifest> {
    if let Some(path) = std::env::var_os("JOHNNY_READER_SUPERTONIC_MODEL_MANIFEST_PATH") {
        let raw = std::fs::read_to_string(path)?;
        return Ok(serde_json::from_str(&raw)?);
    }

    Ok(SupertonicModelManifest {
        files: default_supertonic_model_files(),
    })
}

fn default_supertonic_model_files() -> Vec<SupertonicModelFile> {
    [
        (
            "onnx/duration_predictor.onnx",
            3_700_147,
            "c3eb91414d5ff8a7a239b7fe9e34e7e2bf8a8140d8375ffb14718b1c639325db",
        ),
        (
            "onnx/text_encoder.onnx",
            36_416_150,
            "c7befd5ea8c3119769e8a6c1486c4edc6a3bc8365c67621c881bbb774b9902ff",
        ),
        (
            "onnx/vector_estimator.onnx",
            256_534_781,
            "883ac868ea0275ef0e991524dc64f16b3c0376efd7c320af6b53f5b780d7c61c",
        ),
        (
            "onnx/vocoder.onnx",
            101_424_195,
            "085de76dd8e8d5836d6ca66826601f615939218f90e519f70ee8a36ed2a4c4ba",
        ),
        (
            "onnx/tts.json",
            8_253,
            "42078d3aef1cd43ab43021f3c54f47d2d75ceb4e75f627f118890128b06a0d09",
        ),
        (
            "onnx/unicode_indexer.json",
            277_676,
            "9bf7346e43883a81f8645c81224f786d43c5b57f3641f6e7671a7d6c493cb24f",
        ),
        (
            "voice_styles/M1.json",
            291_748,
            "e35604687f5d23694b8e91593a93eec0e4eca6c0b02bb8ed69139ab2ea6b0a5b",
        ),
        (
            "voice_styles/M2.json",
            292_055,
            "b76cbf62bac707c710cf0ae5aba5e31eea1a6339a9734bfae33ab98499534a50",
        ),
        (
            "voice_styles/M3.json",
            290_198,
            "ea1ac35ccb91b0d7ecad533a2fbd0eec10c91513d8951e3b25fbba99954e159b",
        ),
        (
            "voice_styles/M4.json",
            291_522,
            "ca8eefad4fcd989c9379032ff3e50738adc547eeb5e221b82593a6d7b3bac303",
        ),
        (
            "voice_styles/M5.json",
            291_469,
            "dd22b92740314321f8ae11c5e87f8dd60d060f15dd3a632b5adf77f471f77af2",
        ),
        (
            "voice_styles/F1.json",
            292_046,
            "bbdec6ee00231c2c742ad05483df5334cab3b52fda3ba38e6a07059c4563dbc2",
        ),
        (
            "voice_styles/F2.json",
            292_423,
            "7c722c6a72707b1a77f035d67f0d1351ba187738e06f7683e8c72b1df3477fc6",
        ),
        (
            "voice_styles/F3.json",
            290_794,
            "12f6ef2573baa2defa1128069cb59f203e3ab67c92af77b42df8a0e3a2f7c6ab",
        ),
        (
            "voice_styles/F4.json",
            291_808,
            "c2fa764c1225a76dfc3e2c73e8aa4f70d9ee48793860eb34c295fff01c2e032b",
        ),
        (
            "voice_styles/F5.json",
            291_479,
            "45966e73316415626cf41a7d1c6f3b4c70dbc1ba2bee5c1978ef0ce33244fc8d",
        ),
    ]
    .into_iter()
    .map(|(path, size_bytes, sha256)| SupertonicModelFile {
        path: path.to_string(),
        url: format!("https://huggingface.co/{SUPERTONIC_MODEL_ID}/resolve/main/{path}"),
        size_bytes,
        sha256: sha256.to_string(),
    })
    .collect()
}

fn supertonic_model_dir() -> AppResult<PathBuf> {
    Ok(paths::models_dir()?.join(SUPERTONIC_MODEL_VERSION))
}

fn supertonic_model_file_path(directory: &Path, file: &SupertonicModelFile) -> AppResult<PathBuf> {
    let relative_path = Path::new(&file.path);
    if relative_path.is_absolute()
        || relative_path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(AppError::Model(format!(
            "invalid Supertonic model manifest path: {}",
            file.path
        )));
    }
    Ok(directory.join(relative_path))
}

fn temp_download_path(target_path: &Path) -> AppResult<PathBuf> {
    let file_name = target_path
        .file_name()
        .ok_or_else(|| AppError::Model("invalid Supertonic model file path".into()))?
        .to_string_lossy();
    Ok(target_path.with_file_name(format!("{file_name}.download")))
}

fn file_complete(path: &Path, expected_size: u64, expected_sha256: &str) -> AppResult<bool> {
    match path.metadata() {
        Ok(metadata) if metadata.len() == expected_size => {
            file_matches_sha256(path, expected_sha256)
        }
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn file_matches_sha256(path: &Path, expected_sha256: &str) -> AppResult<bool> {
    if expected_sha256.trim().is_empty() {
        return Ok(true);
    }

    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()) == expected_sha256)
}

fn emit_supertonic_model_progress<R: Runtime>(
    window: &Window<R>,
    file: &str,
    downloaded: u64,
    total: u64,
) -> AppResult<()> {
    window.emit(
        "supertonic-model-download-progress",
        SupertonicModelDownloadProgress {
            file: file.to_string(),
            downloaded,
            total,
        },
    )?;
    Ok(())
}

pub fn chunk_text_for_language(text: &str, language: &str) -> Vec<String> {
    let max_len = if language == "ko" || language == "ja" {
        120
    } else {
        300
    };
    chunk_text(text, max_len)
}

fn chunk_text(text: &str, max_len: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let paragraph_re = Regex::new(r"\n\s*\n").expect("paragraph regex");
    for paragraph in paragraph_re.split(text) {
        let paragraph = paragraph.trim();
        if paragraph.is_empty() {
            continue;
        }
        if char_count(paragraph) <= max_len {
            chunks.push(paragraph.to_string());
            continue;
        }

        let mut current = String::new();
        for sentence in split_sentences(paragraph) {
            let sentence = sentence.trim();
            if sentence.is_empty() {
                continue;
            }
            if char_count(sentence) > max_len {
                push_current_chunk(&mut chunks, &mut current);
                for piece in split_long_piece(sentence, max_len) {
                    push_bounded_piece(&mut chunks, &mut current, &piece, max_len, " ");
                }
            } else {
                push_bounded_piece(&mut chunks, &mut current, sentence, max_len, " ");
            }
        }
        push_current_chunk(&mut chunks, &mut current);
    }

    chunks
}

fn split_sentences(text: &str) -> Vec<String> {
    let boundary = Regex::new(r"([.!?])\s+").expect("sentence regex");
    let matches = boundary.find_iter(text).collect::<Vec<_>>();
    if matches.is_empty() {
        return vec![text.to_string()];
    }

    let mut sentences = Vec::new();
    let mut last_end = 0;
    for match_ in matches {
        let end = match_.start() + 1;
        let before_punctuation = &text[last_end..match_.start()];
        let candidate = format!(
            "{}{}",
            before_punctuation.trim(),
            &text[match_.start()..end]
        );
        if is_abbreviation(&candidate) {
            continue;
        }
        sentences.push(text[last_end..match_.end()].to_string());
        last_end = match_.end();
    }

    if last_end < text.len() {
        sentences.push(text[last_end..].to_string());
    }
    if sentences.is_empty() {
        vec![text.to_string()]
    } else {
        sentences
    }
}

fn split_long_piece(text: &str, max_len: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut current = String::new();
    for part in text.split_inclusive(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if char_count(part) > max_len {
            push_current_chunk(&mut pieces, &mut current);
            for word_piece in split_by_words(part, max_len) {
                pieces.push(word_piece);
            }
        } else {
            push_bounded_piece(&mut pieces, &mut current, part, max_len, " ");
        }
    }
    push_current_chunk(&mut pieces, &mut current);
    pieces
}

fn split_by_words(text: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if char_count(word) > max_len {
            push_current_chunk(&mut chunks, &mut current);
            chunks.extend(split_by_chars(word, max_len));
        } else {
            push_bounded_piece(&mut chunks, &mut current, word, max_len, " ");
        }
    }
    push_current_chunk(&mut chunks, &mut current);
    chunks
}

fn split_by_chars(text: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if char_count(&current) >= max_len {
            chunks.push(current);
            current = String::new();
        }
        current.push(character);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn push_bounded_piece(
    chunks: &mut Vec<String>,
    current: &mut String,
    piece: &str,
    max_len: usize,
    separator: &str,
) {
    if piece.trim().is_empty() {
        return;
    }
    if current.is_empty() {
        current.push_str(piece.trim());
        return;
    }

    let next_len = char_count(current) + char_count(separator) + char_count(piece);
    if next_len > max_len {
        push_current_chunk(chunks, current);
    }
    if !current.is_empty() {
        current.push_str(separator);
    }
    current.push_str(piece.trim());
}

fn push_current_chunk(chunks: &mut Vec<String>, current: &mut String) {
    let chunk = current.trim();
    if !chunk.is_empty() {
        chunks.push(chunk.to_string());
    }
    current.clear();
}

fn is_abbreviation(value: &str) -> bool {
    const ABBREVIATIONS: &[&str] = &[
        "Dr.", "Mr.", "Mrs.", "Ms.", "Prof.", "Sr.", "Jr.", "St.", "Ave.", "Rd.", "Blvd.", "Dept.",
        "Inc.", "Ltd.", "Co.", "Corp.", "etc.", "vs.", "i.e.", "e.g.", "Ph.D.",
    ];
    ABBREVIATIONS
        .iter()
        .any(|abbreviation| value.ends_with(abbreviation))
}

fn char_count(value: &str) -> usize {
    value.chars().count()
}

fn count_words(text: &str) -> usize {
    let word_count = text
        .split_whitespace()
        .filter(|word| !word.trim().is_empty())
        .count();
    if word_count <= 1 && text.chars().any(is_cjk) {
        text.chars()
            .filter(|character| !character.is_whitespace())
            .count()
    } else {
        word_count
    }
}

fn is_cjk(character: char) -> bool {
    matches!(
        character as u32,
        0x3040..=0x30ff | 0x3400..=0x9fff | 0xac00..=0xd7af
    )
}

pub fn is_valid_supertonic_language(language: &str) -> bool {
    SUPERTONIC_LANGUAGES.contains(&language)
}

fn resolve_language(value: Option<&str>, fallback: &str) -> AppResult<String> {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    if is_valid_supertonic_language(candidate) {
        Ok(candidate.to_string())
    } else {
        Err(AppError::InvalidInput(format!(
            "unknown Supertonic language: {candidate}"
        )))
    }
}

fn normalize_language<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    if is_valid_supertonic_language(candidate) {
        candidate
    } else {
        DEFAULT_LANGUAGE
    }
}

fn resolve_voice_style(value: Option<&str>, fallback: &str) -> AppResult<String> {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    SUPERTONIC_VOICE_STYLES
        .iter()
        .find(|voice_style| voice_style.eq_ignore_ascii_case(candidate))
        .map(|voice_style| (*voice_style).to_string())
        .ok_or_else(|| {
            AppError::InvalidInput(format!("unknown Supertonic voice style: {candidate}"))
        })
}

fn playback_voice_style(value: Option<&str>, fallback: &str) -> String {
    let candidate = value.map(str::trim).filter(|value| !value.is_empty());
    SUPERTONIC_VOICE_STYLES
        .iter()
        .find(|voice_style| {
            candidate.is_some_and(|candidate| voice_style.eq_ignore_ascii_case(candidate))
        })
        .copied()
        .unwrap_or(fallback)
        .to_string()
}

fn normalize_voice_style<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    SUPERTONIC_VOICE_STYLES
        .iter()
        .find(|voice_style| voice_style.eq_ignore_ascii_case(candidate))
        .copied()
        .unwrap_or(DEFAULT_VOICE_STYLE)
}

fn setting_string(values: &HashMap<String, JsonValue>, key: &str, fallback: &str) -> String {
    optional_setting_string(values, key).unwrap_or_else(|| fallback.to_string())
}

fn optional_setting_string(values: &HashMap<String, JsonValue>, key: &str) -> Option<String> {
    values
        .get(key)
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn sanitize_file_component(value: &str, max_chars: usize) -> String {
    let mut sanitized = String::new();
    let mut previous_space = false;

    for character in value.chars() {
        let next = if character.is_alphanumeric() || matches!(character, '-' | '_' | '.') {
            previous_space = false;
            Some(character)
        } else if character.is_whitespace() {
            if previous_space {
                None
            } else {
                previous_space = true;
                Some(' ')
            }
        } else if previous_space {
            None
        } else {
            previous_space = true;
            Some(' ')
        };

        if let Some(character) = next {
            sanitized.push(character);
        }
        if sanitized.chars().count() >= max_chars {
            break;
        }
    }

    let sanitized = sanitized.trim().trim_matches('.').trim().to_string();
    if sanitized.is_empty() {
        "Untitled".to_string()
    } else {
        sanitized
    }
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

fn default_export_directory() -> String {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Documents")
        .join("Johnny Reader")
        .to_string_lossy()
        .to_string()
}

fn clamp_speed(speed: f32) -> f32 {
    speed.clamp(0.5, 2.0)
}

fn supertonic_fake_audio_enabled() -> bool {
    std::env::var_os("JOHNNY_READER_SUPERTONIC_FAKE_AUDIO").is_some()
}

fn fake_supertonic_samples(text: &str) -> Vec<f32> {
    let seconds = (0.15 + (text.chars().count() as f32 / 80.0)).clamp(0.2, 1.5);
    let sample_count = (SUPERTONIC_SAMPLE_RATE as f32 * seconds) as usize;
    (0..sample_count)
        .map(|index| {
            let phase =
                (index as f32 / SUPERTONIC_SAMPLE_RATE as f32) * 440.0 * std::f32::consts::TAU;
            phase.sin() * 0.12
        })
        .collect()
}

fn encode_f32_to_wav(samples: &[f32], sample_rate: u32) -> AppResult<Vec<u8>> {
    if samples.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty WAV audio.".into()));
    }

    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer =
            WavWriter::new(&mut cursor, spec).map_err(|error| AppError::Tts(error.to_string()))?;
        for &sample in samples {
            let clamped = sample.clamp(-1.0, 1.0);
            writer
                .write_sample((clamped * 32767.0) as i16)
                .map_err(|error| AppError::Tts(error.to_string()))?;
        }
        writer
            .finalize()
            .map_err(|error| AppError::Tts(error.to_string()))?;
    }
    Ok(cursor.into_inner())
}

fn encode_f32_to_mp3(samples: &[f32], sample_rate: u32) -> AppResult<Vec<u8>> {
    if samples.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty MP3 audio.".into()));
    }

    let pcm = samples
        .iter()
        .map(|sample| (sample.clamp(-1.0, 1.0) * 32767.0) as i16)
        .collect::<Vec<_>>();
    let mut encoder = Builder::new()
        .ok_or_else(|| AppError::Tts("could not initialize MP3 encoder".into()))?
        .with_num_channels(1)
        .map_err(|error| AppError::Tts(error.to_string()))?
        .with_sample_rate(sample_rate)
        .map_err(|error| AppError::Tts(error.to_string()))?
        .with_brate(Bitrate::Kbps128)
        .map_err(|error| AppError::Tts(error.to_string()))?
        .with_quality(Quality::Best)
        .map_err(|error| AppError::Tts(error.to_string()))?
        .build()
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let mut output = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(pcm.len()));
    encoder
        .encode_to_vec(MonoPcm(&pcm), &mut output)
        .map_err(|error| AppError::Tts(error.to_string()))?;
    output.reserve(7200);
    encoder
        .flush_to_vec::<FlushNoGap>(&mut output)
        .map_err(|error| AppError::Tts(error.to_string()))?;

    if output.is_empty() {
        return Err(AppError::Tts("MP3 encoder returned empty audio.".into()));
    }
    Ok(output)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    ae: AEConfig,
    ttl: TTLConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AEConfig {
    sample_rate: i32,
    base_chunk_size: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TTLConfig {
    chunk_compress_factor: i32,
    latent_dim: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct VoiceStyleData {
    style_ttl: StyleComponent,
    style_dp: StyleComponent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StyleComponent {
    data: Vec<Vec<Vec<f32>>>,
    dims: Vec<usize>,
    #[serde(rename = "type")]
    dtype: String,
}

struct UnicodeProcessor {
    indexer: Vec<i64>,
}

impl UnicodeProcessor {
    fn new(unicode_indexer_json_path: &Path) -> AppResult<Self> {
        let file = File::open(unicode_indexer_json_path)?;
        let reader = BufReader::new(file);
        let indexer: Vec<i64> = serde_json::from_reader(reader)?;
        Ok(Self { indexer })
    }

    fn call(
        &self,
        text_list: &[String],
        lang_list: &[String],
    ) -> AppResult<(Vec<Vec<i64>>, Array3<f32>)> {
        let processed_texts = text_list
            .iter()
            .zip(lang_list.iter())
            .map(|(text, language)| preprocess_text(text, language))
            .collect::<AppResult<Vec<_>>>()?;
        let text_ids_lengths = processed_texts
            .iter()
            .map(|text| text.chars().count())
            .collect::<Vec<_>>();
        let max_len = *text_ids_lengths.iter().max().unwrap_or(&0);

        let mut text_ids = Vec::new();
        for text in &processed_texts {
            let mut row = vec![0_i64; max_len];
            for (index, value) in text_to_unicode_values(text).iter().enumerate() {
                row[index] = if *value < self.indexer.len() {
                    self.indexer[*value]
                } else {
                    -1
                };
            }
            text_ids.push(row);
        }

        Ok((text_ids, get_text_mask(&text_ids_lengths)))
    }
}

struct Style {
    ttl: Array3<f32>,
    dp: Array3<f32>,
}

struct TextToSpeech {
    cfgs: Config,
    text_processor: UnicodeProcessor,
    dp_ort: Session,
    text_enc_ort: Session,
    vector_est_ort: Session,
    vocoder_ort: Session,
    sample_rate: i32,
}

impl TextToSpeech {
    fn new(
        cfgs: Config,
        text_processor: UnicodeProcessor,
        dp_ort: Session,
        text_enc_ort: Session,
        vector_est_ort: Session,
        vocoder_ort: Session,
    ) -> Self {
        let sample_rate = cfgs.ae.sample_rate;
        Self {
            cfgs,
            text_processor,
            dp_ort,
            text_enc_ort,
            vector_est_ort,
            vocoder_ort,
            sample_rate,
        }
    }

    fn infer(
        &mut self,
        text_list: &[String],
        lang_list: &[String],
        style: &Style,
        total_step: usize,
        speed: f32,
    ) -> AppResult<(Vec<f32>, Vec<f32>)> {
        let bsz = text_list.len();
        if bsz == 0 {
            return Err(AppError::InvalidInput("text is required".into()));
        }

        let (text_ids, text_mask) = self.text_processor.call(text_list, lang_list)?;
        let text_ids_array = {
            let text_ids_shape = (bsz, text_ids[0].len());
            let mut flat = Vec::new();
            for row in &text_ids {
                flat.extend_from_slice(row);
            }
            Array::from_shape_vec(text_ids_shape, flat)
                .map_err(|error| AppError::Tts(error.to_string()))?
        };

        let text_ids_value = OrtValue::from_array(text_ids_array)
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let text_mask_value = OrtValue::from_array(text_mask.clone())
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let style_dp_value = OrtValue::from_array(style.dp.clone())
            .map_err(|error| AppError::Tts(error.to_string()))?;

        let dp_outputs = self
            .dp_ort
            .run(ort::inputs! {
                "text_ids" => &text_ids_value,
                "style_dp" => &style_dp_value,
                "text_mask" => &text_mask_value
            })
            .map_err(|error| AppError::Tts(error.to_string()))?;

        let (_, duration_data) = dp_outputs["duration"]
            .try_extract_tensor::<f32>()
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let mut duration = duration_data.to_vec();
        for value in &mut duration {
            *value /= speed;
        }

        let style_ttl_value = OrtValue::from_array(style.ttl.clone())
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let text_enc_outputs = self
            .text_enc_ort
            .run(ort::inputs! {
                "text_ids" => &text_ids_value,
                "style_ttl" => &style_ttl_value,
                "text_mask" => &text_mask_value
            })
            .map_err(|error| AppError::Tts(error.to_string()))?;

        let (text_emb_shape, text_emb_data) = text_enc_outputs["text_emb"]
            .try_extract_tensor::<f32>()
            .map_err(|error| AppError::Tts(error.to_string()))?;
        let text_emb = Array3::from_shape_vec(
            (
                text_emb_shape[0] as usize,
                text_emb_shape[1] as usize,
                text_emb_shape[2] as usize,
            ),
            text_emb_data.to_vec(),
        )
        .map_err(|error| AppError::Tts(error.to_string()))?;

        let (mut xt, latent_mask) = sample_noisy_latent(
            &duration,
            self.sample_rate,
            self.cfgs.ae.base_chunk_size,
            self.cfgs.ttl.chunk_compress_factor,
            self.cfgs.ttl.latent_dim,
        );
        let total_step_array = Array::from_elem(bsz, total_step as f32);

        for step in 0..total_step {
            let current_step_array = Array::from_elem(bsz, step as f32);
            let xt_value = OrtValue::from_array(xt.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let text_emb_value = OrtValue::from_array(text_emb.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let latent_mask_value = OrtValue::from_array(latent_mask.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let text_mask_value = OrtValue::from_array(text_mask.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let current_step_value = OrtValue::from_array(current_step_array)
                .map_err(|error| AppError::Tts(error.to_string()))?;
            let total_step_value = OrtValue::from_array(total_step_array.clone())
                .map_err(|error| AppError::Tts(error.to_string()))?;

            let vector_est_outputs = self
                .vector_est_ort
                .run(ort::inputs! {
                    "noisy_latent" => &xt_value,
                    "text_emb" => &text_emb_value,
                    "style_ttl" => &style_ttl_value,
                    "latent_mask" => &latent_mask_value,
                    "text_mask" => &text_mask_value,
                    "current_step" => &current_step_value,
                    "total_step" => &total_step_value
                })
                .map_err(|error| AppError::Tts(error.to_string()))?;

            let (denoised_shape, denoised_data) = vector_est_outputs["denoised_latent"]
                .try_extract_tensor::<f32>()
                .map_err(|error| AppError::Tts(error.to_string()))?;
            xt = Array3::from_shape_vec(
                (
                    denoised_shape[0] as usize,
                    denoised_shape[1] as usize,
                    denoised_shape[2] as usize,
                ),
                denoised_data.to_vec(),
            )
            .map_err(|error| AppError::Tts(error.to_string()))?;
        }

        let final_latent_value =
            OrtValue::from_array(xt).map_err(|error| AppError::Tts(error.to_string()))?;
        let vocoder_outputs = self
            .vocoder_ort
            .run(ort::inputs! {
                "latent" => &final_latent_value
            })
            .map_err(|error| AppError::Tts(error.to_string()))?;

        let (_, wav_data) = vocoder_outputs["wav_tts"]
            .try_extract_tensor::<f32>()
            .map_err(|error| AppError::Tts(error.to_string()))?;
        Ok((wav_data.to_vec(), duration))
    }

    fn call(
        &mut self,
        text: &str,
        language: &str,
        style: &Style,
        total_step: usize,
        speed: f32,
        silence_duration: f32,
    ) -> AppResult<(Vec<f32>, f32)> {
        let chunks = chunk_text_for_language(text, language);
        if chunks.is_empty() {
            return Err(AppError::InvalidInput("text is required".into()));
        }

        let mut wav_cat = Vec::new();
        let mut duration_cat = 0.0_f32;
        for (index, chunk) in chunks.iter().enumerate() {
            let language_list = [language.to_string()];
            let (wav, duration) = self.infer(
                std::slice::from_ref(chunk),
                &language_list,
                style,
                total_step,
                speed,
            )?;
            let duration = duration[0];
            let wav_len = (self.sample_rate as f32 * duration) as usize;
            let wav_chunk = &wav[..wav_len.min(wav.len())];

            if index > 0 {
                let silence_len = (silence_duration * self.sample_rate as f32) as usize;
                wav_cat.extend(std::iter::repeat_n(0.0_f32, silence_len));
                duration_cat += silence_duration;
            }
            wav_cat.extend_from_slice(wav_chunk);
            duration_cat += duration;
        }

        Ok((wav_cat, duration_cat))
    }
}

fn preprocess_text(text: &str, language: &str) -> AppResult<String> {
    if !is_valid_supertonic_language(language) {
        return Err(AppError::InvalidInput(format!(
            "unknown Supertonic language: {language}"
        )));
    }

    let mut text = text.nfkd().collect::<String>();
    let emoji_pattern = Regex::new(r"[\x{1F600}-\x{1F64F}\x{1F300}-\x{1F5FF}\x{1F680}-\x{1F6FF}\x{1F700}-\x{1F77F}\x{1F780}-\x{1F7FF}\x{1F800}-\x{1F8FF}\x{1F900}-\x{1F9FF}\x{1FA00}-\x{1FA6F}\x{1FA70}-\x{1FAFF}\x{2600}-\x{26FF}\x{2700}-\x{27BF}\x{1F1E6}-\x{1F1FF}]+")
        .expect("emoji regex");
    text = emoji_pattern.replace_all(&text, "").to_string();

    for (from, to) in [
        ("–", "-"),
        ("‑", "-"),
        ("—", "-"),
        ("_", " "),
        ("\u{201C}", "\""),
        ("\u{201D}", "\""),
        ("\u{2018}", "'"),
        ("\u{2019}", "'"),
        ("´", "'"),
        ("`", "'"),
        ("[", " "),
        ("]", " "),
        ("|", " "),
        ("/", " "),
        ("#", " "),
        ("→", " "),
        ("←", " "),
        ("@", " at "),
        ("e.g.,", "for example, "),
        ("i.e.,", "that is, "),
    ] {
        text = text.replace(from, to);
    }
    for symbol in ["♥", "☆", "♡", "©", "\\"] {
        text = text.replace(symbol, "");
    }

    for (from, to) in [
        (" ,", ","),
        (" .", "."),
        (" !", "!"),
        (" ?", "?"),
        (" ;", ";"),
        (" :", ":"),
        (" '", "'"),
    ] {
        text = text.replace(from, to);
    }
    while text.contains("\"\"") {
        text = text.replace("\"\"", "\"");
    }
    while text.contains("''") {
        text = text.replace("''", "'");
    }
    while text.contains("``") {
        text = text.replace("``", "`");
    }

    text = Regex::new(r"\s+")
        .expect("space regex")
        .replace_all(&text, " ")
        .trim()
        .to_string();
    if !text.is_empty() {
        let ends_with_punctuation =
            Regex::new(r#"[.!?;:,'"\u{201C}\u{201D}\u{2018}\u{2019})\]}…。」』】〉》›»]$"#)
                .expect("punctuation regex");
        if !ends_with_punctuation.is_match(&text) {
            text.push('.');
        }
    }

    Ok(format!("<{language}>{text}</{language}>"))
}

fn text_to_unicode_values(text: &str) -> Vec<usize> {
    text.chars().map(|character| character as usize).collect()
}

fn length_to_mask(lengths: &[usize], max_len: usize) -> Array3<f32> {
    let mut mask = Array3::<f32>::zeros((lengths.len(), 1, max_len));
    for (batch, length) in lengths.iter().enumerate() {
        for index in 0..(*length).min(max_len) {
            mask[[batch, 0, index]] = 1.0;
        }
    }
    mask
}

fn get_text_mask(text_ids_lengths: &[usize]) -> Array3<f32> {
    let max_len = *text_ids_lengths.iter().max().unwrap_or(&0);
    length_to_mask(text_ids_lengths, max_len)
}

fn sample_noisy_latent(
    duration: &[f32],
    sample_rate: i32,
    base_chunk_size: i32,
    chunk_compress: i32,
    latent_dim: i32,
) -> (Array3<f32>, Array3<f32>) {
    let bsz = duration.len();
    let max_duration = duration
        .iter()
        .fold(0.0_f32, |left, &right| left.max(right));
    let wav_len_max = (max_duration * sample_rate as f32) as usize;
    let wav_lengths = duration
        .iter()
        .map(|duration| (duration * sample_rate as f32) as usize)
        .collect::<Vec<_>>();
    let chunk_size = (base_chunk_size * chunk_compress) as usize;
    let latent_len = wav_len_max.div_ceil(chunk_size);
    let latent_dim = (latent_dim * chunk_compress) as usize;

    let mut noisy_latent = Array3::<f32>::zeros((bsz, latent_dim, latent_len));
    let normal = Normal::new(0.0, 1.0).expect("normal distribution");
    let mut rng = rand::thread_rng();
    for batch in 0..bsz {
        for dimension in 0..latent_dim {
            for time in 0..latent_len {
                noisy_latent[[batch, dimension, time]] = normal.sample(&mut rng);
            }
        }
    }

    let latent_lengths = wav_lengths
        .iter()
        .map(|length| length.div_ceil(chunk_size))
        .collect::<Vec<_>>();
    let latent_mask = length_to_mask(&latent_lengths, latent_len);
    for batch in 0..bsz {
        for dimension in 0..latent_dim {
            for time in 0..latent_len {
                noisy_latent[[batch, dimension, time]] *= latent_mask[[batch, 0, time]];
            }
        }
    }

    (noisy_latent, latent_mask)
}

fn load_cfgs(onnx_dir: &Path) -> AppResult<Config> {
    let file = File::open(onnx_dir.join("tts.json"))?;
    let reader = BufReader::new(file);
    Ok(serde_json::from_reader(reader)?)
}

fn load_voice_style(voice_style_paths: &[PathBuf]) -> AppResult<Style> {
    if voice_style_paths.is_empty() {
        return Err(AppError::InvalidInput("voice style is required".into()));
    }

    let first_file = File::open(&voice_style_paths[0])?;
    let first_reader = BufReader::new(first_file);
    let first_data: VoiceStyleData = serde_json::from_reader(first_reader)?;
    let ttl_dims = &first_data.style_ttl.dims;
    let dp_dims = &first_data.style_dp.dims;
    let ttl_dim1 = ttl_dims[1];
    let ttl_dim2 = ttl_dims[2];
    let dp_dim1 = dp_dims[1];
    let dp_dim2 = dp_dims[2];
    let batch_size = voice_style_paths.len();
    let mut ttl_flat = vec![0.0_f32; batch_size * ttl_dim1 * ttl_dim2];
    let mut dp_flat = vec![0.0_f32; batch_size * dp_dim1 * dp_dim2];

    for (batch_index, path) in voice_style_paths.iter().enumerate() {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let data: VoiceStyleData = serde_json::from_reader(reader)?;

        let ttl_offset = batch_index * ttl_dim1 * ttl_dim2;
        let mut index = 0;
        for batch in &data.style_ttl.data {
            for row in batch {
                for &value in row {
                    ttl_flat[ttl_offset + index] = value;
                    index += 1;
                }
            }
        }

        let dp_offset = batch_index * dp_dim1 * dp_dim2;
        index = 0;
        for batch in &data.style_dp.data {
            for row in batch {
                for &value in row {
                    dp_flat[dp_offset + index] = value;
                    index += 1;
                }
            }
        }
    }

    Ok(Style {
        ttl: Array3::from_shape_vec((batch_size, ttl_dim1, ttl_dim2), ttl_flat)
            .map_err(|error| AppError::Tts(error.to_string()))?,
        dp: Array3::from_shape_vec((batch_size, dp_dim1, dp_dim2), dp_flat)
            .map_err(|error| AppError::Tts(error.to_string()))?,
    })
}

fn load_text_to_speech(onnx_dir: &Path) -> AppResult<TextToSpeech> {
    let cfgs = load_cfgs(onnx_dir)?;
    let dp_ort = Session::builder()
        .map_err(|error| AppError::Tts(error.to_string()))?
        .commit_from_file(onnx_dir.join("duration_predictor.onnx"))
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let text_enc_ort = Session::builder()
        .map_err(|error| AppError::Tts(error.to_string()))?
        .commit_from_file(onnx_dir.join("text_encoder.onnx"))
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let vector_est_ort = Session::builder()
        .map_err(|error| AppError::Tts(error.to_string()))?
        .commit_from_file(onnx_dir.join("vector_estimator.onnx"))
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let vocoder_ort = Session::builder()
        .map_err(|error| AppError::Tts(error.to_string()))?
        .commit_from_file(onnx_dir.join("vocoder.onnx"))
        .map_err(|error| AppError::Tts(error.to_string()))?;
    let text_processor = UnicodeProcessor::new(&onnx_dir.join("unicode_indexer.json"))?;

    Ok(TextToSpeech::new(
        cfgs,
        text_processor,
        dp_ort,
        text_enc_ort,
        vector_est_ort,
        vocoder_ort,
    ))
}
