//! Getting the Supertonic model onto disk and knowing whether it is there.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{Emitter, Runtime, Window};

use crate::error::{AppError, AppResult};
use crate::net::download::file_matches_sha256;
use crate::paths;

pub(crate) const SUPERTONIC_MODEL_ID: &str = "Supertone/supertonic-3";
pub(crate) const SUPERTONIC_MODEL_VERSION: &str = "supertonic-3";
pub(crate) const SUPERTONIC_USER_AGENT: &str = concat!(
    "libretexts-reader/",
    env!("CARGO_PKG_VERSION"),
    " supertonic-model-downloader"
);
/// What a cancelled download fails with. The webview matches on it to tell
/// "the reader pressed Cancel" apart from a real failure, so it must not drift
/// from `MODEL_DOWNLOAD_CANCELLED` in `src/lib/speech/supertonicEngine.ts`.
pub(crate) const MODEL_DOWNLOAD_CANCELLED: &str = "Supertonic model download cancelled.";

/// The flag the reader's Cancel flips, shared between the command doing the
/// downloading and the command asking it to stop.
///
/// Deliberately Tauri managed state rather than a `static`: a process-global
/// would be shared by every test in this crate, which Rust runs as threads in
/// one process, so one test's cancel could abort another's download.
#[derive(Debug, Default, Clone)]
pub struct SupertonicDownloadCancel(Arc<AtomicBool>);

impl SupertonicDownloadCancel {
    /// Ask the download in flight to stop. Takes effect on its next chunk.
    pub(crate) fn request(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Forget a cancel that has already done its work. Called when a download
    /// starts, never when one ends: the request can land after the loop has
    /// exited, and clearing it there would leave it set for the next Play.
    pub(crate) fn clear(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    /// Fail the download the reader asked to stop.
    ///
    /// Called from the progress closure, which `download_verified` invokes on
    /// every chunk and `?`-propagates -- so returning an error here drops the
    /// HTTP stream mid-file rather than waiting for the current 256MB file.
    pub(crate) fn check(&self) -> AppResult<()> {
        if self.0.load(Ordering::SeqCst) {
            return Err(AppError::Model(MODEL_DOWNLOAD_CANCELLED.into()));
        }
        Ok(())
    }
}

/// Abort a stalled model download if no chunk arrives within this window. An
/// overall request timeout is intentionally avoided so large files can finish.
pub(crate) const SUPERTONIC_READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

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
pub(crate) struct SupertonicModelDownloadProgress {
    file: String,
    downloaded: u64,
    total: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct SupertonicModelManifest {
    pub(crate) files: Vec<SupertonicModelFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SupertonicModelFile {
    pub(crate) path: String,
    pub(crate) url: String,
    pub(crate) size_bytes: u64,
    pub(crate) sha256: String,
}

pub(crate) fn supertonic_model_status() -> AppResult<SupertonicModelStatus> {
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

pub(crate) fn existing_supertonic_model_bytes(
    directory: &Path,
    files: &[SupertonicModelFile],
) -> u64 {
    files
        .iter()
        .filter_map(|file| supertonic_model_file_path(directory, file).ok())
        .filter_map(|path| path.metadata().ok())
        .map(|metadata| metadata.len())
        .sum()
}

pub(crate) fn supertonic_model_manifest() -> AppResult<SupertonicModelManifest> {
    if let Some(path) = std::env::var_os("LIBRETEXTS_READER_SUPERTONIC_MODEL_MANIFEST_PATH") {
        let raw = std::fs::read_to_string(path)?;
        return Ok(serde_json::from_str(&raw)?);
    }

    Ok(SupertonicModelManifest {
        files: default_supertonic_model_files(),
    })
}

pub(crate) fn default_supertonic_model_files() -> Vec<SupertonicModelFile> {
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

pub(crate) fn supertonic_model_dir() -> AppResult<PathBuf> {
    Ok(paths::models_dir()?.join(SUPERTONIC_MODEL_VERSION))
}

pub(crate) fn supertonic_model_file_path(
    directory: &Path,
    file: &SupertonicModelFile,
) -> AppResult<PathBuf> {
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

pub(crate) fn temp_download_path(target_path: &Path) -> AppResult<PathBuf> {
    let file_name = target_path
        .file_name()
        .ok_or_else(|| AppError::Model("invalid Supertonic model file path".into()))?
        .to_string_lossy();
    Ok(target_path.with_file_name(format!("{file_name}.download")))
}

pub(crate) fn file_complete(
    path: &Path,
    expected_size: u64,
    expected_sha256: &str,
) -> AppResult<bool> {
    match path.metadata() {
        Ok(metadata) if metadata.len() == expected_size => {
            file_matches_sha256(path, expected_sha256)
        }
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) fn emit_supertonic_model_progress<R: Runtime>(
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

#[cfg(test)]
mod user_agent_tests {
    use super::SUPERTONIC_USER_AGENT;

    #[test]
    fn user_agent_reports_the_version_this_build_actually_is() {
        // Sent to huggingface.co on every model download. It read
        // "libretexts-reader/0.1" as a literal, so it drifts the moment the
        // crate version moves and nothing in the app would notice.
        assert!(
            SUPERTONIC_USER_AGENT.contains(env!("CARGO_PKG_VERSION")),
            "User-Agent {SUPERTONIC_USER_AGENT:?} does not name the crate version {:?}",
            env!("CARGO_PKG_VERSION"),
        );
    }
}

#[cfg(test)]
mod cancel_tests {
    use super::{SupertonicDownloadCancel, MODEL_DOWNLOAD_CANCELLED};

    #[test]
    fn a_cancelled_download_stops_and_a_cleared_flag_lets_the_next_one_start() {
        let cancel = SupertonicDownloadCancel::default();
        assert!(
            cancel.check().is_ok(),
            "a download nobody cancelled must be allowed to continue"
        );

        cancel.request();
        let error = cancel
            .check()
            .expect_err("a cancelled download must not keep fetching chunks");
        assert!(
            error.to_string().contains(MODEL_DOWNLOAD_CANCELLED),
            "cancellation must be recognisable to the webview, got {error}"
        );

        // A cancel left set outlives the download it stopped: the next Play
        // would die before its first chunk, with nothing on screen saying why.
        cancel.clear();
        assert!(
            cancel.check().is_ok(),
            "clearing the flag must let a later download run"
        );
    }

    #[test]
    fn cancelling_through_one_handle_stops_the_download_holding_another() {
        // The two commands never share a handle: `cancel_supertonic_model_download`
        // is given one by Tauri while `ensure_supertonic_model_downloaded` is
        // already inside its loop holding its own. A handle that copied the flag
        // instead of sharing it would leave Cancel doing nothing at all.
        let held_by_the_download = SupertonicDownloadCancel::default();
        let held_by_the_reader = held_by_the_download.clone();

        held_by_the_reader.request();

        assert!(
            held_by_the_download.check().is_err(),
            "cancelling through one handle must stop the download holding another"
        );
    }
}
