//! Where a rendered chapter goes, and what it will cost.
//!
//! The cache path is content-addressed: same text, voice, language, model
//! version and step count means the same file, so re-exporting a chapter that
//! has not changed costs a copy rather than a synthesis.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::AppResult;
use crate::paths;
use crate::tts::supertonic::chunk::{chunk_text_for_language, count_words};
use crate::tts::supertonic::model::SUPERTONIC_MODEL_VERSION;
use crate::tts::supertonic::{
    ChapterMaterial, SupertonicChapterEstimate, SupertonicChapterRequest, SupertonicConfig,
    SUPERTONIC_TOTAL_STEPS,
};

pub(crate) const AUDIOBOOK_WORDS_PER_MINUTE: f64 = 165.0;
pub(crate) const SUPERTONIC_CACHE_VERSION: &str = "supertonic-tts-cache-v1";

pub(crate) fn estimate_for_text(
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

pub(crate) fn output_path_for_chapter(
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

/// Derive the content-addressed cache path under an explicitly supplied root.
///
/// Pure: it hashes and joins, and touches the filesystem not at all. The root
/// is a parameter rather than resolved here because `paths::cache_dir()` calls
/// `create_dir_all`, so merely asking it for the path materialises the real
/// app-data tree — which is how `cargo test` used to create
/// `~/Library/Application Support/dev.johnnylibretexts.reader/cache` on
/// machines where the app had never been launched.
///
/// Passed explicitly rather than redirected through
/// `LIBRETEXTS_READER_APP_DATA_DIR`, for the same reason `cleanup::reclaim_in`
/// takes a directory: `set_var` is process-global and Rust runs tests as
/// threads in one process, so an override here could race another test.
pub(crate) fn cache_path_in(
    cache_root: &Path,
    material: &ChapterMaterial,
    voice_style: &str,
    language: &str,
) -> PathBuf {
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

    cache_root
        .join("supertonic-tts")
        .join(format!("{hash}.mp3"))
}

/// The cache path under the real app-data cache directory.
///
/// Resolving that directory creates it, which is correct here — every caller is
/// about to read or write the file.
pub(crate) fn cache_path_for_chapter(
    material: &ChapterMaterial,
    voice_style: &str,
    language: &str,
) -> AppResult<PathBuf> {
    Ok(cache_path_in(
        &paths::cache_dir()?,
        material,
        voice_style,
        language,
    ))
}

pub(crate) async fn copy_cached_mp3(cache_path: &Path, output_path: &Path) -> AppResult<()> {
    if let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    if cache_path == output_path {
        return Ok(());
    }

    tokio::fs::copy(cache_path, output_path).await?;
    Ok(())
}

pub(crate) fn sanitize_file_component(value: &str, max_chars: usize) -> String {
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

pub(crate) fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().to_string()
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;
    use crate::db::models::{Document, Section, SourceType};

    fn material(text: &str) -> ChapterMaterial {
        ChapterMaterial {
            document: Document {
                id: "doc-1".into(),
                title: "Chemistry: The Central Science".into(),
                source_type: SourceType::Openstax,
                source_metadata: serde_json::Value::Null,
                cover_image_path: None,
                license: None,
                attribution: None,
                word_count: 10,
                imported_at: Utc::now(),
                last_opened_at: None,
            },
            section: Section {
                id: "sec-1".into(),
                document_id: "doc-1".into(),
                ordinal: 0,
                title: "11.5: Vapor Pressure".into(),
                word_count: 10,
            },
            text: text.into(),
        }
    }

    #[test]
    fn strips_path_separators_from_titles() {
        // Section titles like "11.5: Vapor Pressure" reach the filesystem, and
        // LibreTexts titles routinely contain slashes.
        let sanitized = sanitize_file_component("11.5: Vapor/Pressure", 60);

        assert!(!sanitized.contains('/'));
        assert!(!sanitized.contains(':'));
        assert!(sanitized.contains("Vapor"));
    }

    #[test]
    fn truncates_a_long_title() {
        assert!(
            sanitize_file_component(&"word ".repeat(100), 40)
                .chars()
                .count()
                <= 40
        );
    }

    #[test]
    fn cache_path_is_content_addressed() {
        // A fictional root. This test asserts only how inputs map to paths, so
        // it must not reach paths::cache_dir() -- that call creates the real
        // app-data tree as a side effect, on a machine that may never have run
        // the app.
        let root = Path::new("/nonexistent/cache");
        let path = |text: &str, voice: &str, language: &str| {
            cache_path_in(root, &material(text), voice, language)
        };

        let same_a = path("Hello.", "M1", "en");
        let same_b = path("Hello.", "M1", "en");
        assert_eq!(same_a, same_b, "identical input must reuse the cached file");

        assert_ne!(
            same_a,
            path("Hello.", "F2", "en"),
            "voice must change the path"
        );
        assert_ne!(
            same_a,
            path("Hello.", "M1", "fr"),
            "language must change the path"
        );
        assert_ne!(
            same_a,
            path("Goodbye.", "M1", "en"),
            "text must change the path"
        );

        assert!(
            !Path::new("/nonexistent").exists(),
            "deriving a cache path must not create directories"
        );
    }

    #[test]
    fn estimate_scales_with_the_amount_of_text() {
        let short = estimate_for_text(
            &material("One two three."),
            "en",
            Path::new("/tmp/a.mp3"),
            false,
        );
        let long = estimate_for_text(
            &material(&"word ".repeat(500)),
            "en",
            Path::new("/tmp/a.mp3"),
            false,
        );

        assert!(long.word_count > short.word_count);
        assert!(long.estimated_seconds > short.estimated_seconds);
        assert!(!short.cached);
    }
}
