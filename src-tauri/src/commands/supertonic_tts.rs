use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{Runtime, State, Window};
use uuid::Uuid;

use crate::commands::tts::SpeechAudio;
use crate::content::normalize::normalize_for_tts;
use crate::db::connection::DbPool;
use crate::db::library;
use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::net::download::{download_verified, Download};
use crate::tts::supertonic::audio::{encode_f32_to_mp3, encode_f32_to_wav, SUPERTONIC_SAMPLE_RATE};
use crate::tts::supertonic::cache::{
    cache_path_for_chapter, copy_cached_mp3, estimate_for_text, output_path_for_chapter,
    path_to_string,
};
use crate::tts::supertonic::engine;
use crate::tts::supertonic::model::{
    emit_supertonic_model_progress, existing_supertonic_model_bytes, file_complete,
    supertonic_model_dir, supertonic_model_file_path, supertonic_model_manifest,
    supertonic_model_status, temp_download_path, SupertonicModelStatus, SUPERTONIC_READ_TIMEOUT,
    SUPERTONIC_USER_AGENT,
};
use crate::tts::supertonic::voice::{
    normalize_language, playback_voice_style, resolve_language, resolve_voice_style,
    DEFAULT_LANGUAGE, DEFAULT_VOICE_STYLE,
};
use crate::tts::supertonic::{
    ChapterMaterial, SupertonicChapterEstimate, SupertonicChapterRequest, SupertonicConfig,
    SUPERTONIC_DEFAULT_SPEED,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupertonicPreviewRequest {
    pub text: String,
    pub voice_style: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupertonicChapterExport {
    pub output_path: String,
    pub cached: bool,
    pub byte_length: u64,
    pub estimate: SupertonicChapterEstimate,
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

        // Size and digest are checked as the bytes arrive; the old loop here
        // wrote the whole file first and then re-read it to hash it.
        let file_size = file.size_bytes;
        let file_label = file.path.clone();
        download_verified(
            &client,
            Download {
                url: &file.url,
                temp_path: &temp_path,
                expected_sha256: &file.sha256,
                expected_size: file_size,
                read_timeout: SUPERTONIC_READ_TIMEOUT,
                error: AppError::Model,
            },
            |file_downloaded, _total| {
                emit_supertonic_model_progress(
                    &window,
                    &file_label,
                    downloaded + file_downloaded,
                    total.max(downloaded + file_downloaded),
                )
            },
        )
        .await?;

        let file_downloaded = file_size;
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

    synthesize_supertonic_audio(text, voice_style, language, SUPERTONIC_DEFAULT_SPEED).await
}

/// Everything both chapter commands need to know before they diverge.
///
/// The two used to work this out separately from identical eight-line
/// prologues, and export then recomputed the estimate the frontend had just
/// asked for. Resolving it once means the estimate a reader is shown and the
/// one returned by the export are the same value, not two that happen to agree.
struct ChapterJob {
    material: ChapterMaterial,
    voice_style: String,
    language: String,
    output_path: PathBuf,
    cache_path: PathBuf,
    estimate: SupertonicChapterEstimate,
}

fn resolve_chapter_job(
    state: &State<'_, DbPool>,
    request: &SupertonicChapterRequest,
) -> AppResult<ChapterJob> {
    let config = supertonic_config_from_state(state)?;
    let material = chapter_material(state, &request.document_id, &request.section_id)?;
    let voice_style = resolve_voice_style(request.voice_style.as_deref(), &config.voice_style)?;
    let language = resolve_language(request.language.as_deref(), &config.language)?;
    let output_path = output_path_for_chapter(&config, &material, &voice_style, &language, request);
    let cache_path = cache_path_for_chapter(&material, &voice_style, &language)?;
    let estimate = estimate_for_text(&material, &language, &output_path, cache_path.exists());

    Ok(ChapterJob {
        material,
        voice_style,
        language,
        output_path,
        cache_path,
        estimate,
    })
}

#[tauri::command]
pub async fn estimate_supertonic_chapter(
    state: State<'_, DbPool>,
    request: SupertonicChapterRequest,
) -> AppResult<SupertonicChapterEstimate> {
    Ok(resolve_chapter_job(&state, &request)?.estimate)
}

#[tauri::command]
pub async fn export_supertonic_chapter_mp3(
    state: State<'_, DbPool>,
    request: SupertonicChapterRequest,
) -> AppResult<SupertonicChapterExport> {
    let job = resolve_chapter_job(&state, &request)?;
    let force = request.force.unwrap_or(false);

    if job.cache_path.exists() && !force {
        copy_cached_mp3(&job.cache_path, &job.output_path).await?;
        let bytes = tokio::fs::metadata(&job.output_path).await?.len();
        return Ok(SupertonicChapterExport {
            output_path: path_to_string(&job.output_path),
            cached: true,
            byte_length: bytes,
            estimate: job.estimate,
        });
    }

    if let Some(parent) = job.cache_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    let mp3 = synthesize_supertonic_mp3(
        job.material.text.clone(),
        job.voice_style.clone(),
        job.language.clone(),
        SUPERTONIC_DEFAULT_SPEED,
    )
    .await?;
    if mp3.is_empty() {
        return Err(AppError::Tts("Supertonic returned empty audio.".into()));
    }

    // Unique temp name so concurrent exports of the same chapter cannot write
    // to the same in-progress file before the atomic rename into place.
    let temp_path = job
        .cache_path
        .with_extension(format!("{}.mp3.download", Uuid::new_v4()));
    tokio::fs::write(&temp_path, &mp3).await?;
    tokio::fs::rename(&temp_path, &job.cache_path).await?;
    copy_cached_mp3(&job.cache_path, &job.output_path).await?;

    Ok(SupertonicChapterExport {
        output_path: path_to_string(&job.output_path),
        cached: false,
        byte_length: mp3.len() as u64,
        // The chapter was not cached when the job was resolved, which is why
        // it was synthesized; it is now.
        estimate: SupertonicChapterEstimate {
            cached: true,
            ..job.estimate
        },
    })
}

/// Playback synthesis, driven entirely by what the caller passes.
///
/// Deliberately lenient about voice and language rather than erroring the way
/// the preview and export commands do: the player carries one voice id across
/// engines, so a reader who switches engines mid-session can briefly hold an
/// id this engine does not know. Falling back beats cutting the audio off.
/// User-initiated commands still reject unknown values outright.
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

pub(crate) async fn synthesize_supertonic_mp3(
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
        engine::synthesize_samples_blocking(&text, &voice_style, &language, speed)
    })
    .await
    .map_err(|error| AppError::Tts(format!("Supertonic runtime task failed: {error}")))?
}

fn clamp_speed(speed: f32) -> f32 {
    speed.clamp(0.5, 2.0)
}
