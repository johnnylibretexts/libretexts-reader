//! One-time reclamation of files left behind by the removed Kokoro engine.
//!
//! Best-effort by design. An existing install holds ~417 MB of dead ONNX
//! models and a directory of voice embeddings; getting that space back is
//! worth doing and is never worth failing a launch over, so every error here
//! is reported and swallowed.

use std::path::Path;

use crate::paths;
use crate::tts::supertonic::cache::{
    LEGACY_SUPERTONIC_CACHE_DIR, TTS_CACHE_DIR, TTS_CACHE_VERSION,
};

const KOKORO_MODEL_FILES: &[&str] = &["kokoro-fp32.onnx", "kokoro-q8.onnx"];

pub fn reclaim_kokoro_artifacts() {
    match paths::app_data_dir() {
        Ok(dir) => reclaim_in(&dir),
        Err(error) => eprintln!("kokoro cleanup: could not resolve app data dir: {error}"),
    }
}

pub fn reclaim_stale_tts_cache() {
    match paths::cache_dir() {
        Ok(dir) => reclaim_stale_tts_cache_in(&dir),
        Err(error) => eprintln!("tts cache cleanup: could not resolve cache dir: {error}"),
    }
}

/// Remove rendered chapter audio no current cache key can ever reach.
///
/// Two kinds: whole cache versions that have been superseded, and the
/// pre-multi-provider `supertonic-tts` directory. Both are unreachable rather
/// than merely stale — the version is part of the path, so nothing hashes into
/// them — and a book's worth of chapter MP3s is hundreds of megabytes.
///
/// Best-effort like the rest of this module: reclaiming space is never worth
/// failing a launch over.
pub fn reclaim_stale_tts_cache_in(cache_root: &Path) {
    remove_tree(&cache_root.join(LEGACY_SUPERTONIC_CACHE_DIR));

    let versions_dir = cache_root.join(TTS_CACHE_DIR);
    let entries = match std::fs::read_dir(&versions_dir) {
        Ok(entries) => entries,
        // A cache that was never written is the normal case on a fresh install.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            eprintln!(
                "tts cache cleanup: could not read {}: {error}",
                versions_dir.display()
            );
            return;
        }
    };

    for entry in entries.flatten() {
        // Anything that is not the live version cannot be reached by any key
        // this build can produce.
        if entry.file_name() != TTS_CACHE_VERSION {
            remove_tree(&entry.path());
        }
    }
}

fn remove_tree(path: &Path) {
    match std::fs::remove_dir_all(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => eprintln!(
            "tts cache cleanup: could not remove {}: {error}",
            path.display()
        ),
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

#[cfg(test)]
mod tts_cache_tests {
    use super::reclaim_stale_tts_cache_in;
    use crate::tts::supertonic::cache::{
        LEGACY_SUPERTONIC_CACHE_DIR, TTS_CACHE_DIR, TTS_CACHE_VERSION,
    };
    use std::fs;
    use uuid::Uuid;

    /// A throwaway cache root passed explicitly, for the same reason
    /// `reclaim_in` takes one: `set_var` is process-global and Rust runs
    /// tests as threads in one process.
    fn scratch_cache() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("libretexts-reader-cache-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("create cache root");
        dir
    }

    #[test]
    fn superseded_cache_versions_are_reclaimed_and_the_current_one_is_kept() {
        // A chapter export is tens of megabytes and a book is hundreds. Left
        // in place these are unreachable -- no current cache key can hash to a
        // name inside a superseded version -- so they are pure dead weight.
        let cache = scratch_cache();
        let current = cache.join(TTS_CACHE_DIR).join(TTS_CACHE_VERSION);
        let superseded = cache.join(TTS_CACHE_DIR).join("tts-cache-v1");
        fs::create_dir_all(&current).expect("current");
        fs::create_dir_all(&superseded).expect("superseded");
        fs::write(current.join("live.mp3"), b"live").expect("write live");
        fs::write(superseded.join("dead.mp3"), b"dead").expect("write dead");

        reclaim_stale_tts_cache_in(&cache);

        assert!(
            current.join("live.mp3").exists(),
            "the current version is the live cache — removing it would re-bill every Fish export"
        );
        assert!(!superseded.exists());

        fs::remove_dir_all(&cache).ok();
    }

    #[test]
    fn the_pre_multi_provider_cache_directory_is_reclaimed() {
        let cache = scratch_cache();
        let legacy = cache.join(LEGACY_SUPERTONIC_CACHE_DIR);
        fs::create_dir_all(&legacy).expect("legacy");
        fs::write(legacy.join("orphan.mp3"), b"orphan").expect("write orphan");

        reclaim_stale_tts_cache_in(&cache);

        assert!(!legacy.exists());

        fs::remove_dir_all(&cache).ok();
    }

    #[test]
    fn a_cache_that_was_never_written_is_not_an_error() {
        let cache = scratch_cache();

        // Must not panic and must not create anything: launch never depends
        // on this running.
        reclaim_stale_tts_cache_in(&cache);

        assert!(!cache.join(TTS_CACHE_DIR).exists());

        fs::remove_dir_all(&cache).ok();
    }
}
