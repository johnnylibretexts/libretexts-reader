//! Getting the Supertonic model onto disk and knowing whether it is there.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

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

/// What a finished download tells the callers that joined it.
///
/// The message rather than the `AppError`, because `AppError` cannot be
/// `Clone` -- it wraps `rusqlite::Error` and `reqwest::Error`, neither of
/// which is. A joined caller therefore sees `AppError::Model` whatever the
/// leader actually hit. That is exact for the case that matters, since
/// cancellation is already an `AppError::Model` the webview matches by
/// message, and honest for the rest: from a joiner's side this *is* a failure
/// of the model download it was waiting on.
type DownloadOutcome = Result<String, String>;

/// The one model download this process will run, and the flag that stops it.
///
/// Two commands can ask for the model -- the player on first Play and the
/// Settings Download button -- and nothing used to keep them apart. Both
/// cleared the shared cancel flag on entry, so a Cancel the reader pressed
/// before the second one arrived was silently voided, and both wrote and
/// renamed the same `<file>.download` temp path. A second caller now joins the
/// download already in flight instead of starting a competing one.
#[derive(Debug, Default, Clone)]
pub struct SupertonicDownload {
    cancel: SupertonicDownloadCancel,
    in_flight: Arc<Mutex<Option<broadcast::Sender<DownloadOutcome>>>>,
}

impl SupertonicDownload {
    /// Ask the download in flight to stop, whoever started it.
    pub(crate) fn request_cancel(&self) {
        self.cancel.request();
    }

    /// Fetch the model, or join the fetch already running.
    ///
    /// `download` is only called when this caller is the one that starts it;
    /// everyone else waits for its result. It receives the cancel handle
    /// rather than reading one from managed state, so the download it runs and
    /// the Cancel the reader presses cannot refer to different flags.
    pub(crate) async fn run<F, Fut>(&self, download: F) -> AppResult<String>
    where
        F: FnOnce(SupertonicDownloadCancel) -> Fut,
        Fut: std::future::Future<Output = AppResult<String>>,
    {
        let joined = {
            let mut in_flight = self.lock_slot();
            match in_flight.as_ref() {
                // Subscribing here, under the same lock the leader publishes
                // under, is what makes the handoff safe: a receiver created
                // while the sender is still in the slot cannot miss the result.
                Some(running) => Some(running.subscribe()),
                None => {
                    let (sender, _) = broadcast::channel(1);
                    *in_flight = Some(sender);
                    // Only where a download actually begins. Clearing on every
                    // entry is what let a second caller void a Cancel the
                    // reader had already pressed on the first.
                    self.cancel.clear();
                    None
                }
            }
        };

        if let Some(mut waiting) = joined {
            return match waiting.recv().await {
                Ok(outcome) => outcome.map_err(AppError::Model),
                // The leader went away without publishing -- dropped mid-flight
                // or panicked. Saying so beats waiting for a result that is
                // never coming.
                Err(_) => Err(AppError::Model(
                    "the Supertonic model download stopped without reporting a result".into(),
                )),
            };
        }

        // Releases the slot even if the future below is dropped or panics. A
        // leader that never reaches the publish knows nothing about the callers
        // parked behind it; without this they wait forever.
        let _slot = SlotHeld(self.in_flight.clone());

        let outcome = download(self.cancel.clone()).await;

        if let Some(sender) = self.lock_slot().take() {
            // Fails only when nobody joined, which is the common case.
            let _ = sender.send(match &outcome {
                Ok(directory) => Ok(directory.clone()),
                Err(error) => Err(error.message()),
            });
        }

        outcome
    }

    /// A poisoned slot is recoverable: the value behind it is an `Option` that
    /// a panicking leader leaves in a valid state either way.
    fn lock_slot(&self) -> std::sync::MutexGuard<'_, Option<broadcast::Sender<DownloadOutcome>>> {
        self.in_flight
            .lock()
            .unwrap_or_else(|held| held.into_inner())
    }
}

/// Clears the in-flight slot when the leader stops, however it stops.
///
/// Dropping the sender is what wakes anyone waiting: they see the channel
/// close and report a download that ended without a result, rather than
/// parking on one that will never publish.
struct SlotHeld(Arc<Mutex<Option<broadcast::Sender<DownloadOutcome>>>>);

impl Drop for SlotHeld {
    fn drop(&mut self) {
        let mut slot = self.0.lock().unwrap_or_else(|held| held.into_inner());
        slot.take();
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

#[cfg(test)]
mod single_flight_tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use tokio::sync::oneshot;

    use super::{SupertonicDownload, MODEL_DOWNLOAD_CANCELLED};

    /// Drive a future to the point where it first waits.
    ///
    /// Everything `run` does before it awaits happens in that single poll --
    /// claiming the slot, or joining one and subscribing to its result -- which
    /// is exactly where the old code cleared the cancel flag. Polling by hand
    /// rather than spawning keeps the ordering of two callers exact instead of
    /// leaving it to the scheduler.
    macro_rules! poll_until_it_waits {
        ($future:expr, $what:literal) => {
            assert!(
                futures::poll!(&mut $future).is_pending(),
                concat!($what, " should still be waiting at this point")
            );
        };
    }

    #[tokio::test]
    async fn a_second_request_joins_the_download_already_running() {
        let download = SupertonicDownload::default();
        let starts = Arc::new(AtomicUsize::new(0));
        let (release, held) = oneshot::channel::<()>();

        let mut player = Box::pin(download.run({
            let starts = starts.clone();
            |_| async move {
                starts.fetch_add(1, Ordering::SeqCst);
                held.await.ok();
                Ok("the model directory".to_string())
            }
        }));
        poll_until_it_waits!(player, "the player's download");

        let mut settings = Box::pin(download.run({
            let starts = starts.clone();
            |_| async move {
                starts.fetch_add(1, Ordering::SeqCst);
                Ok("a competing download".to_string())
            }
        }));
        poll_until_it_waits!(settings, "the Settings download");

        release.send(()).ok();
        let (player, settings) = tokio::join!(player, settings);

        assert_eq!(
            player.expect("the player's download should finish"),
            "the model directory"
        );
        assert_eq!(
            settings.expect("the joined download should finish"),
            "the model directory",
            "the second caller should be handed the running download's result"
        );
        assert_eq!(
            starts.load(Ordering::SeqCst),
            1,
            "only one download should ever have started"
        );
    }

    #[tokio::test]
    async fn a_second_request_does_not_void_a_cancel_already_pending() {
        let download = SupertonicDownload::default();
        let seen_by_the_download = Arc::new(Mutex::new(None));

        let outcome = download
            .run(|cancel| {
                let download = download.clone();
                let seen = seen_by_the_download.clone();
                async move {
                    // The reader presses Cancel on the download now running.
                    download.request_cancel();

                    // Settings then asks for the model too, while that Cancel
                    // is still pending.
                    let mut settings = Box::pin(download.run(|_| async {
                        panic!("a second download must not start while one is running")
                    }));
                    poll_until_it_waits!(settings, "the Settings download");

                    *seen.lock().expect("the observation") = Some(cancel.check().is_err());
                    Ok("the model directory".to_string())
                }
            })
            .await;

        assert!(
            outcome.is_ok(),
            "the leader itself should not have failed here"
        );
        assert_eq!(
            *seen_by_the_download.lock().expect("the observation"),
            Some(true),
            "a second request must not clear a Cancel the reader already pressed"
        );
    }

    #[tokio::test]
    async fn every_caller_waiting_on_a_cancelled_download_is_told_it_was_cancelled() {
        let download = SupertonicDownload::default();
        let (release, held) = oneshot::channel::<()>();

        let mut player = Box::pin(download.run(|cancel| async move {
            held.await.ok();
            cancel.check()?;
            Ok("the model directory".to_string())
        }));
        poll_until_it_waits!(player, "the player's download");

        let mut settings =
            Box::pin(download.run(|_| async {
                panic!("a second download must not start while one is running")
            }));
        poll_until_it_waits!(settings, "the Settings download");

        download.request_cancel();
        release.send(()).ok();
        let (player, settings) = tokio::join!(player, settings);

        for (who, result) in [("the player", player), ("Settings", settings)] {
            let error = result.expect_err(&format!("{who} should see the cancellation"));
            assert!(
                error.to_string().contains(MODEL_DOWNLOAD_CANCELLED),
                "{who} got {error}, which the webview cannot recognise as a Cancel"
            );
        }
    }

    #[tokio::test]
    async fn a_download_that_has_finished_does_not_capture_the_next_one() {
        let download = SupertonicDownload::default();

        download
            .run(|_| async { Ok("the model directory".to_string()) })
            .await
            .expect("the first download should finish");

        // Bounded on purpose. A slot the finished download never released
        // does not make this fail, it makes it *hang* -- a later caller
        // subscribes to a sender that will never send again, since a broadcast
        // receiver created after a send never sees it. This repo has already
        // lost a CI run to the six-hour job cap (#44); a regression here should
        // cost five seconds.
        let second = tokio::time::timeout(
            Duration::from_secs(5),
            download.run(|_| async { Ok("a later download".to_string()) }),
        )
        .await
        .expect("a finished download must release the slot, not capture every later caller");

        assert_eq!(
            second.expect("a later download should be allowed to run"),
            "a later download",
            "the slot must be released, or every later caller waits on a download that is over"
        );
    }

    #[tokio::test]
    async fn an_abandoned_download_does_not_park_later_callers_forever() {
        let download = SupertonicDownload::default();
        let (started, has_started) = oneshot::channel::<()>();

        let abandoned = tokio::spawn({
            let download = download.clone();
            async move {
                download
                    .run(|_| async move {
                        started.send(()).ok();
                        std::future::pending::<()>().await;
                        Ok(String::new())
                    })
                    .await
            }
        });
        has_started.await.expect("the download should have started");

        // A window closing, or a panic, drops the command's future mid-flight.
        abandoned.abort();
        let _ = abandoned.await;

        let later = tokio::time::timeout(
            Duration::from_secs(5),
            download.run(|_| async { Ok("a later download".to_string()) }),
        )
        .await
        .expect("a later caller must not wait on a download that will never report");

        assert_eq!(
            later.expect("the later download should finish"),
            "a later download"
        );
    }
}
