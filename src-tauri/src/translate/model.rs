//! Downloading translation model files and reporting what is already present.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::broadcast;

use crate::error::{AppError, AppResult};
use crate::net::download::{download_verified, file_matches_sha256, Download};
use crate::translate::catalog::{ModelFile, TranslationModel};

pub(crate) const MODEL_DOWNLOAD_CANCELLED: &str = "Translation model download cancelled.";
const TRANSLATION_USER_AGENT: &str = concat!(
    "libretexts-reader/",
    env!("CARGO_PKG_VERSION"),
    " translation-model-downloader"
);
const READ_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelStatus {
    pub downloaded: bool,
    pub missing_files: Vec<String>,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct TranslationDownloadCancel(Arc<AtomicBool>);

impl TranslationDownloadCancel {
    fn request(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    fn clear(&self) {
        self.0.store(false, Ordering::SeqCst);
    }

    pub(crate) fn check(&self) -> AppResult<()> {
        if self.0.load(Ordering::SeqCst) {
            return Err(AppError::Model(MODEL_DOWNLOAD_CANCELLED.into()));
        }
        Ok(())
    }
}

type DownloadOutcome = Result<String, String>;

/// Coordinates the process-wide download for translation models.
///
/// Settings and playback can reach this work at the same time. Only the first
/// caller starts it; later callers subscribe to that same result and Cancel
/// stops the shared operation.
#[derive(Debug, Default, Clone)]
pub(crate) struct TranslationDownload {
    cancel: TranslationDownloadCancel,
    in_flight: Arc<Mutex<Option<broadcast::Sender<DownloadOutcome>>>>,
}

impl TranslationDownload {
    pub(crate) fn request_cancel(&self) {
        self.cancel.request();
    }

    pub(crate) async fn run<F, Fut>(&self, download: F) -> AppResult<String>
    where
        F: FnOnce(TranslationDownloadCancel) -> Fut,
        Fut: std::future::Future<Output = AppResult<String>>,
    {
        let joined = {
            let mut in_flight = self.lock_slot();
            match in_flight.as_ref() {
                Some(running) => Some(running.subscribe()),
                None => {
                    let (sender, _) = broadcast::channel(1);
                    *in_flight = Some(sender);
                    self.cancel.clear();
                    None
                }
            }
        };

        if let Some(mut waiting) = joined {
            return match waiting.recv().await {
                Ok(outcome) => outcome.map_err(AppError::Model),
                Err(_) => Err(AppError::Model(
                    "the translation model download stopped without reporting a result".into(),
                )),
            };
        }

        let _slot = SlotHeld(self.in_flight.clone());
        let outcome = download(self.cancel.clone()).await;

        if let Some(sender) = self.lock_slot().take() {
            let _ = sender.send(match &outcome {
                Ok(directory) => Ok(directory.clone()),
                Err(error) => Err(error.message()),
            });
        }

        outcome
    }

    fn lock_slot(&self) -> std::sync::MutexGuard<'_, Option<broadcast::Sender<DownloadOutcome>>> {
        self.in_flight
            .lock()
            .unwrap_or_else(|held| held.into_inner())
    }
}

struct SlotHeld(Arc<Mutex<Option<broadcast::Sender<DownloadOutcome>>>>);

impl Drop for SlotHeld {
    fn drop(&mut self) {
        self.0
            .lock()
            .unwrap_or_else(|held| held.into_inner())
            .take();
    }
}

pub(crate) fn model_status(model: &TranslationModel, root: &Path) -> ModelStatus {
    let total_bytes = model.files.iter().map(|file| file.size_bytes).sum();
    let mut downloaded_bytes = 0;
    let mut missing_files = Vec::new();

    for file in &model.files {
        let Ok(path) = model_file_path(root, file) else {
            missing_files.push(file.path.clone());
            continue;
        };

        match path.metadata() {
            Ok(metadata) => {
                downloaded_bytes += metadata.len();
                let complete = (file.size_bytes == 0 || metadata.len() == file.size_bytes)
                    && file_matches_sha256(&path, &file.sha256).unwrap_or(false);
                if !complete {
                    missing_files.push(file.path.clone());
                }
            }
            Err(_) => missing_files.push(file.path.clone()),
        }
    }

    ModelStatus {
        downloaded: !model.files.is_empty() && missing_files.is_empty(),
        missing_files,
        downloaded_bytes,
        total_bytes,
    }
}

/// Download and verify every file in `model` into the explicitly supplied root.
pub(crate) async fn fetch(
    model: &TranslationModel,
    root: &Path,
    cancel: TranslationDownloadCancel,
) -> AppResult<String> {
    if model.files.is_empty() {
        return Err(AppError::Model(format!(
            "translation model {} has no downloadable file manifest",
            model.model_id
        )));
    }

    tokio::fs::create_dir_all(root).await?;
    let client = reqwest::Client::builder()
        .user_agent(TRANSLATION_USER_AGENT)
        .build()?;

    for file in &model.files {
        cancel.check()?;
        let target_path = model_file_path(root, file)?;
        if file_complete(&target_path, file)? {
            continue;
        }

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let temp_path = temp_download_path(&target_path)?;
        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            model.repo, file.path
        );
        download_verified(
            &client,
            Download {
                url: &url,
                temp_path: &temp_path,
                expected_sha256: &file.sha256,
                expected_size: file.size_bytes,
                read_timeout: READ_TIMEOUT,
                error: AppError::Model,
            },
            |_, _| cancel.check(),
        )
        .await?;

        if target_path.exists() {
            tokio::fs::remove_file(&target_path).await?;
        }
        tokio::fs::rename(temp_path, target_path).await?;
    }

    Ok(root.to_string_lossy().into_owned())
}

fn model_file_path(root: &Path, file: &ModelFile) -> AppResult<PathBuf> {
    let relative = Path::new(&file.path);
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir
                    | Component::RootDir
                    | Component::Prefix(_)
                    | Component::CurDir
            )
        })
    {
        return Err(AppError::Model(format!(
            "invalid translation model path: {}",
            file.path
        )));
    }
    Ok(root.join(relative))
}

fn temp_download_path(target: &Path) -> AppResult<PathBuf> {
    let name = target
        .file_name()
        .ok_or_else(|| AppError::Model("invalid translation model file path".into()))?
        .to_string_lossy();
    Ok(target.with_file_name(format!("{name}.download")))
}

fn file_complete(path: &Path, file: &ModelFile) -> AppResult<bool> {
    match path.metadata() {
        Ok(metadata) if file.size_bytes == 0 || metadata.len() == file.size_bytes => {
            file_matches_sha256(path, &file.sha256)
        }
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tokio::sync::oneshot;

    use super::*;

    fn test_model(files: Vec<ModelFile>) -> TranslationModel {
        TranslationModel {
            model_id: "test/model@pinned".into(),
            repo: "test/model".into(),
            files,
            verified: true,
        }
    }

    #[test]
    fn reports_missing_files_without_touching_the_real_app_data_dir() {
        let root = tempfile::tempdir().unwrap();
        let model = crate::translate::catalog::resolve_pair("en", "es").unwrap();

        let status = model_status(&model, root.path());
        assert!(!status.downloaded);
        assert!(status.missing_files.contains(&"model.bin".to_string()));
    }

    #[test]
    fn reports_only_verified_files_as_downloaded() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("model.bin"), b"abc").unwrap();
        let model = test_model(vec![ModelFile {
            path: "model.bin".into(),
            size_bytes: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
        }]);

        let status = model_status(&model, root.path());
        assert!(status.downloaded);
        assert_eq!(status.downloaded_bytes, 3);
        assert_eq!(status.total_bytes, 3);

        std::fs::write(root.path().join("model.bin"), b"bad").unwrap();
        let status = model_status(&model, root.path());
        assert!(!status.downloaded);
        assert_eq!(status.missing_files, ["model.bin"]);
    }

    #[test]
    fn refuses_model_paths_outside_the_explicit_root() {
        let root = tempfile::tempdir().unwrap();
        for unsafe_path in ["../outside.bin", "/outside.bin", "", "."] {
            let model = test_model(vec![ModelFile {
                path: unsafe_path.into(),
                size_bytes: 3,
                sha256: String::new(),
            }]);

            let status = model_status(&model, root.path());
            assert!(!status.downloaded);
            assert_eq!(status.missing_files, [unsafe_path]);
            assert!(model_file_path(root.path(), &model.files[0]).is_err());
        }
    }

    #[test]
    fn cancellation_is_shared_and_recognisable() {
        let cancel = TranslationDownloadCancel::default();
        let reader = cancel.clone();
        reader.request();
        let error = cancel.check().expect_err("the download should stop");
        assert!(error.to_string().contains(MODEL_DOWNLOAD_CANCELLED));
        cancel.clear();
        assert!(cancel.check().is_ok());
    }

    #[tokio::test]
    async fn a_second_request_joins_the_download_already_running() {
        let download = TranslationDownload::default();
        let starts = Arc::new(AtomicUsize::new(0));
        let (release, held) = oneshot::channel::<()>();

        let mut first = Box::pin(download.run({
            let starts = starts.clone();
            |_| async move {
                starts.fetch_add(1, Ordering::SeqCst);
                held.await.ok();
                Ok("model directory".to_string())
            }
        }));
        assert!(futures::poll!(&mut first).is_pending());

        let mut second = Box::pin(download.run({
            let starts = starts.clone();
            |_| async move {
                starts.fetch_add(1, Ordering::SeqCst);
                Ok("competing directory".to_string())
            }
        }));
        assert!(futures::poll!(&mut second).is_pending());

        release.send(()).ok();
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap(), "model directory");
        assert_eq!(second.unwrap(), "model directory");
        assert_eq!(starts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancellation_reaches_every_caller_waiting_on_the_download() {
        let download = TranslationDownload::default();
        let (release, held) = oneshot::channel::<()>();
        let mut first = Box::pin(download.run(|cancel| async move {
            held.await.ok();
            cancel.check()?;
            Ok(String::new())
        }));
        assert!(futures::poll!(&mut first).is_pending());
        let mut second = Box::pin(
            download.run(|_| async { panic!("a joined caller must not start another download") }),
        );
        assert!(futures::poll!(&mut second).is_pending());

        download.request_cancel();
        release.send(()).ok();
        let (first, second) = tokio::join!(first, second);
        for result in [first, second] {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains(MODEL_DOWNLOAD_CANCELLED));
        }
    }
}
