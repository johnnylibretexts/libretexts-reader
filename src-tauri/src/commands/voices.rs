use std::path::Path;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use tauri::{Emitter, Runtime, State, Window};
use uuid::Uuid;

use crate::db::connection::DbPool;
use crate::db::models::Voice;
use crate::db::settings;
use crate::error::{AppError, AppResult};
use crate::net::download::{download_verified, Download};
use crate::paths;
use crate::voices::manifest;
use crate::voices::models;

const USER_AGENT: &str = "libretexts-reader/0.1 model-downloader";

/// Time allowed to establish a connection before the download is aborted.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Time allowed between received chunks before a stalled download is aborted.
/// An overall request timeout is intentionally avoided so that legitimately
/// large model files can finish downloading on slow connections.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelDownloadProgress {
    downloaded: u64,
    total: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VoiceDownloadProgress {
    voice_id: String,
    downloaded: u64,
    total: u64,
}

#[tauri::command]
pub async fn list_voices(state: State<'_, DbPool>) -> AppResult<Vec<Voice>> {
    let conn = state.get()?;
    manifest::list_seeded_voices(&conn)
}

#[tauri::command]
pub async fn download_voice<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    voice_id: String,
) -> AppResult<()> {
    let metadata = manifest::voice_metadata(&voice_id)?;
    if metadata.is_bundled_default {
        let conn = state.get()?;
        manifest::set_voice_downloaded(&conn, &voice_id, true)?;
        return Ok(());
    }

    let target_path = voice_path(&voice_id)?;
    if target_path.exists() && models::file_matches_sha256(&target_path, &metadata.sha256)? {
        let conn = state.get()?;
        manifest::set_voice_downloaded(&conn, &voice_id, true)?;
        return Ok(());
    }

    if target_path.exists() {
        std::fs::remove_file(&target_path)?;
    }

    let voice_manifest = manifest::load_voice_manifest()?;
    let primary_url = voice_manifest
        .bundle_url_template
        .replace("{id}", &voice_id);
    let mirror_url = voice_manifest
        .mirror_url_template
        .replace("{id}", &voice_id);
    // Unique per-invocation temp file so concurrent downloads of the same voice
    // cannot corrupt each other's in-progress file before the atomic rename.
    let temp_path = paths::temp_dir()?.join(format!("{voice_id}.{}.bin.download", Uuid::new_v4()));
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()?;
    emit_voice_progress(&window, &voice_id, 0, metadata.size_bytes)?;

    // Download and publish in one fallible unit so the temp file is always
    // cleaned up on any error path (mirror failure, finalize failure, rename).
    let download_and_publish = async {
        let primary_result = download_verified(
            &client,
            Download {
                url: &primary_url,
                temp_path: &temp_path,
                expected_sha256: &metadata.sha256,
                expected_size: metadata.size_bytes,
                read_timeout: READ_TIMEOUT,
                error: AppError::Voice,
            },
            |downloaded, total| emit_voice_progress(&window, &voice_id, downloaded, total),
        )
        .await;

        if let Err(primary_error) = primary_result {
            let _ = tokio::fs::remove_file(&temp_path).await;
            download_verified(
                &client,
                Download {
                    url: &mirror_url,
                    temp_path: &temp_path,
                    expected_sha256: &metadata.sha256,
                    expected_size: metadata.size_bytes,
                    read_timeout: READ_TIMEOUT,
                    error: AppError::Voice,
                },
                |downloaded, total| emit_voice_progress(&window, &voice_id, downloaded, total),
            )
            .await
            .map_err(|mirror_error| {
                AppError::Voice(format!(
                    "voice download failed: {primary_error}; mirror failed: {mirror_error}"
                ))
            })?;
        }

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // The destination was already cleared above if it existed and did not
        // match the expected hash, so rename directly: it replaces the target
        // atomically and avoids a window where the target is missing.
        tokio::fs::rename(&temp_path, &target_path).await?;
        Ok::<(), AppError>(())
    };

    if let Err(error) = download_and_publish.await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(error);
    }

    let conn = state.get()?;
    manifest::set_voice_downloaded(&conn, &voice_id, true)?;
    emit_voice_progress(&window, &voice_id, metadata.size_bytes, metadata.size_bytes)?;
    Ok(())
}

#[tauri::command]
pub async fn delete_voice(state: State<'_, DbPool>, voice_id: String) -> AppResult<()> {
    let metadata = manifest::voice_metadata(&voice_id)?;
    if metadata.is_bundled_default {
        return Err(AppError::InvalidInput(
            "bundled voices cannot be deleted".to_string(),
        ));
    }

    let target_path = voice_path(&voice_id)?;
    match tokio::fs::remove_file(&target_path).await {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let conn = state.get()?;
    manifest::set_voice_downloaded(&conn, &voice_id, false)?;
    Ok(())
}

#[tauri::command]
pub async fn ensure_model_downloaded<R: Runtime>(
    state: State<'_, DbPool>,
    window: Window<R>,
    precision: String,
) -> AppResult<String> {
    let metadata = models::metadata_for(&precision)?;
    let target_path = models::model_path(&precision)?;

    if target_path.exists() && models::file_matches_sha256(&target_path, &metadata.sha256)? {
        mark_model_downloaded(&state, &precision)?;
        return Ok(path_to_string(&target_path));
    }

    if target_path.exists() {
        std::fs::remove_file(&target_path)?;
    }

    let temp_path = paths::temp_dir()?.join(format!(
        "{}.{}.download",
        models::model_file_name(&precision)?,
        Uuid::new_v4()
    ));
    let client = reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .connect_timeout(CONNECT_TIMEOUT)
        .build()?;
    emit_model_progress(&window, 0, metadata.size_bytes)?;

    // Download and publish in one fallible unit so the temp file is always
    // cleaned up on any error path (mirror failure, finalize failure, rename).
    let download_and_publish = async {
        let primary_result = download_verified(
            &client,
            Download {
                url: &metadata.url,
                temp_path: &temp_path,
                expected_sha256: &metadata.sha256,
                expected_size: metadata.size_bytes,
                read_timeout: READ_TIMEOUT,
                error: AppError::Model,
            },
            |downloaded, total| emit_model_progress(&window, downloaded, total),
        )
        .await;

        if let Err(primary_error) = primary_result {
            let _ = tokio::fs::remove_file(&temp_path).await;
            download_verified(
                &client,
                Download {
                    url: &metadata.mirror,
                    temp_path: &temp_path,
                    expected_sha256: &metadata.sha256,
                    expected_size: metadata.size_bytes,
                    read_timeout: READ_TIMEOUT,
                    error: AppError::Model,
                },
                |downloaded, total| emit_model_progress(&window, downloaded, total),
            )
            .await
            .map_err(|mirror_error| {
                AppError::Model(format!(
                    "model download failed: {primary_error}; mirror failed: {mirror_error}"
                ))
            })?;
        }

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        // The destination was already cleared above if it existed and did not
        // match the expected hash, so rename directly: it replaces the target
        // atomically and avoids a window where the target is missing.
        tokio::fs::rename(&temp_path, &target_path).await?;
        Ok::<(), AppError>(())
    };

    if let Err(error) = download_and_publish.await {
        let _ = tokio::fs::remove_file(&temp_path).await;
        return Err(error);
    }

    mark_model_downloaded(&state, &precision)?;
    emit_model_progress(&window, metadata.size_bytes, metadata.size_bytes)?;

    Ok(path_to_string(&target_path))
}

#[tauri::command]
pub async fn get_model_path(precision: String) -> AppResult<String> {
    let metadata = models::metadata_for(&precision)?;
    let path = models::model_path(&precision)?;

    if !path.exists() {
        return Err(AppError::Model(format!(
            "{precision} model is not downloaded"
        )));
    }

    if !models::file_matches_sha256(&path, &metadata.sha256)? {
        return Err(AppError::Model(format!(
            "{precision} model failed SHA-256 verification"
        )));
    }

    Ok(path_to_string(&path))
}

fn mark_model_downloaded(state: &State<'_, DbPool>, precision: &str) -> AppResult<()> {
    let conn = state.get()?;
    settings::set_setting(&conn, "model_downloaded", &json!(true))?;
    settings::set_setting(&conn, "model_precision", &json!(precision))?;
    Ok(())
}

fn emit_model_progress<R: Runtime>(
    window: &Window<R>,
    downloaded: u64,
    total: u64,
) -> AppResult<()> {
    window.emit(
        "model-download-progress",
        ModelDownloadProgress { downloaded, total },
    )?;
    Ok(())
}

fn emit_voice_progress<R: Runtime>(
    window: &Window<R>,
    voice_id: &str,
    downloaded: u64,
    total: u64,
) -> AppResult<()> {
    window.emit(
        "voice-download-progress",
        VoiceDownloadProgress {
            voice_id: voice_id.to_string(),
            downloaded,
            total,
        },
    )?;
    Ok(())
}

fn voice_path(voice_id: &str) -> AppResult<std::path::PathBuf> {
    Ok(paths::voices_dir()?.join(format!("{voice_id}.bin")))
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}
