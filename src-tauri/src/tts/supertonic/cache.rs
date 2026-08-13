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

pub(crate) fn cache_path_for_chapter(
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

pub(crate) fn default_export_directory() -> String {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Documents")
        .join("Johnny Reader")
        .to_string_lossy()
        .to_string()
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
        let same_a = cache_path_for_chapter(&material("Hello."), "M1", "en").unwrap();
        let same_b = cache_path_for_chapter(&material("Hello."), "M1", "en").unwrap();
        assert_eq!(same_a, same_b, "identical input must reuse the cached file");

        let other_voice = cache_path_for_chapter(&material("Hello."), "F2", "en").unwrap();
        let other_language = cache_path_for_chapter(&material("Hello."), "M1", "fr").unwrap();
        let other_text = cache_path_for_chapter(&material("Goodbye."), "M1", "en").unwrap();

        assert_ne!(same_a, other_voice);
        assert_ne!(same_a, other_language);
        assert_ne!(same_a, other_text);
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
