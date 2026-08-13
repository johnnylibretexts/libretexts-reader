//! One-time reclamation of files left behind by the removed Kokoro engine.
//!
//! Best-effort by design. An existing install holds ~417 MB of dead ONNX
//! models and a directory of voice embeddings; getting that space back is
//! worth doing and is never worth failing a launch over, so every error here
//! is reported and swallowed.

use std::path::Path;

use crate::paths;

const KOKORO_MODEL_FILES: &[&str] = &["kokoro-fp32.onnx", "kokoro-q8.onnx"];

pub fn reclaim_kokoro_artifacts() {
    match paths::app_data_dir() {
        Ok(dir) => reclaim_in(&dir),
        Err(error) => eprintln!("kokoro cleanup: could not resolve app data dir: {error}"),
    }
}

/// Split out from `reclaim_kokoro_artifacts` so tests can pass a throwaway
/// directory instead of mutating the app-data environment variable.
fn reclaim_in(app_data_dir: &Path) {
    // Only the two named files. models/ also holds the Supertonic model.
    let models_dir = app_data_dir.join("models");
    for file_name in KOKORO_MODEL_FILES {
        let path = models_dir.join(file_name);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => eprintln!(
                "kokoro cleanup: could not remove {}: {error}",
                path.display()
            ),
        }
    }

    let voices_dir = app_data_dir.join("voices");
    match std::fs::remove_dir_all(&voices_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!(
            "kokoro cleanup: could not remove {}: {error}",
            voices_dir.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::reclaim_in;
    use std::fs;
    use uuid::Uuid;

    /// A throwaway directory passed explicitly, never via
    /// LIBRETEXTS_READER_APP_DATA_DIR. Rust tests share one process and there
    /// is no serial_test dev-dependency, so mutating that env var here would
    /// let one test redirect another's app data mid-run.
    fn scratch_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("libretexts-reader-sweep-{}", Uuid::new_v4()));
        fs::create_dir_all(dir.join("models")).expect("create models dir");
        fs::create_dir_all(dir.join("voices")).expect("create voices dir");
        dir
    }

    #[test]
    fn reclaims_the_kokoro_models_and_the_voices_directory() {
        let dir = scratch_dir();
        fs::write(dir.join("models/kokoro-fp32.onnx"), b"x").expect("write fp32");
        fs::write(dir.join("models/kokoro-q8.onnx"), b"x").expect("write q8");
        fs::write(dir.join("voices/af_heart.bin"), b"x").expect("write voice");

        reclaim_in(&dir);

        assert!(!dir.join("models/kokoro-fp32.onnx").exists());
        assert!(!dir.join("models/kokoro-q8.onnx").exists());
        assert!(!dir.join("voices").exists());

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leaves_the_supertonic_model_directory_untouched() {
        let dir = scratch_dir();
        fs::create_dir_all(dir.join("models/supertonic-v1")).expect("create model dir");
        fs::write(dir.join("models/supertonic-v1/model.onnx"), b"x").expect("write model");

        reclaim_in(&dir);

        assert!(
            dir.join("models/supertonic-v1/model.onnx").exists(),
            "models/ is shared — deleting the directory would destroy the surviving engine"
        );

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_clean_install_is_not_an_error() {
        let dir = scratch_dir();
        fs::remove_dir_all(dir.join("voices")).expect("remove voices dir");

        // Nothing to reclaim. This must not panic and must not recreate
        // anything: the whole point is that launch never depends on it.
        reclaim_in(&dir);

        assert!(!dir.join("voices").exists());

        fs::remove_dir_all(&dir).ok();
    }
}
