//! Where a rendered chapter goes, and what it will cost.
//!
//! The cache path is content-addressed: same text, voice, language, model
//! version and step count means the same file, so re-exporting a chapter that
//! has not changed costs a copy rather than a synthesis.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::AppResult;
use crate::paths;
use crate::tts::provider::{export_extension, provider_display_name};
use crate::tts::supertonic::chunk::{chunk_text_for_language, count_words};
use crate::tts::supertonic::{
    ChapterEstimate, ChapterMaterial, ChapterRequest, SupertonicConfig, SUPERTONIC_TOTAL_STEPS,
};

pub(crate) const AUDIOBOOK_WORDS_PER_MINUTE: f64 = 165.0;
/// Bump when a change makes existing cached audio wrong to reuse.
///
/// A directory component rather than hash input alone: hashed into the
/// filename it was invisible, so audio from a superseded version sat
/// unreachable and unreclaimable next to live audio. As a directory,
/// `cleanup::reclaim_stale_tts_cache_in` can find and remove it, and the next
/// bump cleans up after itself.
// v3: Supertonic exports moved from MP3 to AAC/M4A (ADR-0004). The key does
// not hash the container, so a v2 entry and its v3 replacement would differ
// only by extension -- the old file would never be read again and never be
// collected either. A version bump puts them in a directory cleanup can see.
pub(crate) const TTS_CACHE_VERSION: &str = "tts-cache-v3";

/// Holds every provider's rendered chapters, not just Supertonic's -- the
/// cache key hashes the provider and model, so Fish and Supertonic audio for
/// the same chapter are different files under here.
pub(crate) const TTS_CACHE_DIR: &str = "tts-audio";

/// The directory this cache used before it held more than one provider.
///
/// Everything in it is unreachable: the version that produced those files is
/// superseded, so no current key can hash to any name inside it.
pub(crate) const LEGACY_SUPERTONIC_CACHE_DIR: &str = "supertonic-tts";

pub(crate) fn estimate_for_text(
    material: &ChapterMaterial,
    language: &str,
    output_path: &Path,
    cached: bool,
    provider: &str,
) -> ChapterEstimate {
    let chunks = chunk_text_for_language(&material.text, language);
    let word_count = count_words(&material.text) as u32;
    let estimated_seconds = ((word_count as f64 / AUDIOBOOK_WORDS_PER_MINUTE) * 60.0)
        .ceil()
        .max(1.0) as u32;

    ChapterEstimate {
        word_count,
        estimated_seconds,
        chunk_count: chunks.len() as u32,
        cached,
        output_path: path_to_string(output_path),
        // The real value is filled in by `resolve_chapter_job`, which owns
        // `billable_characters` (see `commands::chapter_tts`) -- this
        // function has no notion of billing, only of the text and provider
        // name.
        billable_characters: 0,
        provider: provider.to_string(),
    }
}

pub(crate) fn output_path_for_chapter(
    config: &SupertonicConfig,
    material: &ChapterMaterial,
    voice_style: &str,
    language: &str,
    target_lang: Option<&str>,
    request: &ChapterRequest,
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
        "{:03} - {} - {} - {} - {}.{}",
        material.section.ordinal + 1,
        sanitize_file_component(&material.section.title, 72),
        // Not the literal "Supertonic" it used to be: a Fish export wrote a
        // file whose name said Supertonic had produced it.
        sanitize_file_component(provider_display_name(&request.provider), 16),
        voice_file_component(voice_style),
        sanitize_file_component(target_lang.unwrap_or(language), 8),
        export_extension(&request.provider)
    );
    directory.join(filename)
}

/// The voice part of an export filename, kept short without becoming ambiguous.
///
/// Supertonic voice styles are two characters, so 16 was plenty. A Fish
/// `reference_id` is an opaque 32+ character model id, and truncating two of
/// them to a shared 16-character prefix would give two different voices the
/// same output path — one export silently overwriting the other. (The cache
/// path is unaffected: it hashes the full voice id.) When truncation actually
/// discards anything, a short digest of the full id goes back on the end.
fn voice_file_component(voice_style: &str) -> String {
    let short = sanitize_file_component(voice_style, 16);
    if short == sanitize_file_component(voice_style, usize::MAX) {
        return short;
    }

    let digest = hex::encode(Sha256::digest(voice_style.as_bytes()));
    format!("{short} {}", &digest[..8])
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
    provider: &str,
    model: &str,
    material: &ChapterMaterial,
    voice_style: &str,
    language: &str,
    target_lang: Option<&str>,
) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(TTS_CACHE_VERSION.as_bytes());
    hasher.update(provider.as_bytes());
    hasher.update(model.as_bytes());
    hasher.update(SUPERTONIC_TOTAL_STEPS.to_le_bytes());
    hasher.update(voice_style.as_bytes());
    hasher.update(language.as_bytes());
    hasher.update(material.document.id.as_bytes());
    hasher.update(material.section.id.as_bytes());
    hasher.update(material.text.as_bytes());
    // Appended after every pre-translation field. Updating with an empty
    // slice changes no hash bytes, so `None` preserves every existing cache
    // key while each translated target gets its own audio.
    hasher.update(target_lang.unwrap_or_default().as_bytes());
    let hash = hex::encode(hasher.finalize());

    cache_root
        .join(TTS_CACHE_DIR)
        .join(TTS_CACHE_VERSION)
        .join(format!("{hash}.{}", export_extension(provider)))
}

/// The cache path under the real app-data cache directory.
///
/// Resolving that directory creates it, which is correct here — every caller is
/// about to read or write the file.
pub(crate) fn cache_path_for_chapter(
    provider: &str,
    model: &str,
    material: &ChapterMaterial,
    voice_style: &str,
    language: &str,
    target_lang: Option<&str>,
) -> AppResult<PathBuf> {
    Ok(cache_path_in(
        &paths::cache_dir()?,
        provider,
        model,
        material,
        voice_style,
        language,
        target_lang,
    ))
}

pub(crate) async fn copy_cached_export(cache_path: &Path, output_path: &Path) -> AppResult<()> {
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
                source_language: "en".into(),
                imported_at: Utc::now(),
                last_opened_at: None,
                progress: 0.0,
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
    fn cached_audio_is_filed_under_its_cache_version() {
        // The version was only hashed into the filename, so audio from a
        // superseded version was indistinguishable from live audio and could
        // never be reclaimed -- potentially a book's worth of dead MP3s after
        // a single bump. As a directory component it is identifiable, which is
        // what lets `cleanup` sweep it and makes the next bump self-cleaning.
        let path = cache_path_in(
            Path::new("/cache"),
            "fish",
            "s2.1-pro",
            &material("Vapor pressure rises with temperature."),
            "M1",
            "en",
            None,
        );

        assert!(
            path.components()
                .any(|component| component.as_os_str() == TTS_CACHE_VERSION),
            "expected {TTS_CACHE_VERSION} as a path component, got {}",
            path.display()
        );
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
    fn cache_path_distinguishes_providers_and_models() {
        // Without this, a chapter exported with Supertonic would be served
        // from cache for a Fish request -- identical text, voice and language,
        // identical key -- and the user would silently get the wrong voice.
        let root = Path::new("/nonexistent/cache");
        let supertonic = cache_path_in(
            root,
            "supertonic",
            "v1",
            &material("Hello."),
            "M1",
            "en",
            None,
        );
        let fish = cache_path_in(
            root,
            "fish",
            "s2.1-pro",
            &material("Hello."),
            "M1",
            "en",
            None,
        );
        let other_model = cache_path_in(
            root,
            "fish",
            "s2-pro",
            &material("Hello."),
            "M1",
            "en",
            None,
        );

        assert_ne!(supertonic, fish, "provider must change the path");
        assert_ne!(fish, other_model, "model must change the path");

        // Same model, different provider: this is the assertion that fails if
        // `provider` is ever dropped from the hash. The provider-vs-model
        // comparison above varies both at once and so cannot catch it.
        assert_ne!(
            cache_path_in(
                root,
                "supertonic",
                "v1",
                &material("Hello."),
                "M1",
                "en",
                None,
            ),
            cache_path_in(root, "fish", "v1", &material("Hello."), "M1", "en", None,),
            "provider alone must change the path"
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
            cache_path_in(
                root,
                "supertonic",
                "v1",
                &material(text),
                voice,
                language,
                None,
            )
        };

        let same_a = path("Hello.", "M1", "en");
        let same_b = path("Hello.", "M1", "en");
        assert_eq!(same_a, same_b, "identical input must reuse the cached file");
        assert_eq!(
            same_a,
            root.join(TTS_CACHE_DIR)
                .join(TTS_CACHE_VERSION)
                .join("9f951a07823dd8fdabd5590629a8cf5a3d50575b196bedbafb7d2c20bad8fa24.m4a"),
            "an untranslated key must remain byte-identical across the target-language change"
        );

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
    fn a_translated_export_does_not_reuse_the_untranslated_audio() {
        // Same section, same voice, same speech-engine language, different
        // translation target. One cache key for both would hand the reader
        // yesterday's English export and report success.
        let root = Path::new("/tmp/does-not-need-to-exist");
        let chapter = material("The cell divides.");
        let english = cache_path_in(root, "supertonic", "v1", &chapter, "M1", "en", None);
        let spanish = cache_path_in(root, "supertonic", "v1", &chapter, "M1", "en", Some("es"));
        let french = cache_path_in(root, "supertonic", "v1", &chapter, "M1", "en", Some("fr"));

        assert_ne!(english, spanish);
        assert_ne!(spanish, french);
        assert_eq!(
            english,
            cache_path_in(root, "supertonic", "v1", &chapter, "M1", "en", None,)
        );
    }

    fn config() -> SupertonicConfig {
        SupertonicConfig {
            voice_style: "M1".into(),
            language: "en".into(),
            export_directory: "/nonexistent/exports".into(),
        }
    }

    fn request(provider: &str) -> ChapterRequest {
        ChapterRequest {
            document_id: "doc-1".into(),
            section_id: "sec-1".into(),
            provider: provider.to_string(),
            voice_style: None,
            language: None,
            output_path: None,
            force: None,
        }
    }

    #[test]
    fn the_output_filename_names_the_provider_that_produced_the_audio() {
        // The filename used to hardcode "Supertonic", so a Fish export landed
        // on disk claiming an engine that had nothing to do with it.
        let fish = output_path_for_chapter(
            &config(),
            &material("Hello."),
            "d8ee9d1a-6f3e-4b8a",
            "en",
            None,
            &request("fish"),
        );
        assert!(
            path_to_string(&fish).contains("Fish Audio"),
            "a Fish export must say Fish Audio: {}",
            path_to_string(&fish)
        );
        assert!(!path_to_string(&fish).contains("Supertonic"));

        let supertonic = output_path_for_chapter(
            &config(),
            &material("Hello."),
            "M1",
            "en",
            None,
            &request("supertonic"),
        );
        assert!(path_to_string(&supertonic).contains("Supertonic"));
    }

    #[test]
    fn the_output_filename_names_the_translation_target_when_present() {
        let chapter = material("Hello.");
        let request = request("supertonic");
        let original = output_path_for_chapter(&config(), &chapter, "M1", "en", None, &request);
        let spanish =
            output_path_for_chapter(&config(), &chapter, "M1", "en", Some("es"), &request);

        assert!(path_to_string(&original).contains(" - en.m4a"));
        assert!(path_to_string(&spanish).contains(" - es.m4a"));
        assert_ne!(original, spanish);
    }

    #[test]
    fn two_voice_ids_sharing_a_prefix_do_not_collide_on_the_output_path() {
        // Fish reference_ids are opaque 32+ character model ids. Truncated to
        // 16 characters, these two are identical -- one export would silently
        // overwrite the other's file.
        let first = "d8ee9d1a6f3e4b8a9c1d000000000001";
        let second = "d8ee9d1a6f3e4b8a9c1d000000000002";
        assert_eq!(
            &first[..16],
            &second[..16],
            "the ids must actually share a 16-character prefix, or this test proves nothing"
        );

        assert_ne!(
            output_path_for_chapter(
                &config(),
                &material("Hello."),
                first,
                "en",
                None,
                &request("fish")
            ),
            output_path_for_chapter(
                &config(),
                &material("Hello."),
                second,
                "en",
                None,
                &request("fish")
            ),
        );
    }

    #[test]
    fn a_short_voice_style_is_left_exactly_as_it_was() {
        // Supertonic's two-character styles never truncate, so nothing is
        // appended to them and existing export filenames are unchanged.
        assert_eq!(voice_file_component("M1"), "M1");
    }

    #[test]
    fn estimate_scales_with_the_amount_of_text() {
        let short = estimate_for_text(
            &material("One two three."),
            "en",
            Path::new("/tmp/a.mp3"),
            false,
            "supertonic",
        );
        let long = estimate_for_text(
            &material(&"word ".repeat(500)),
            "en",
            Path::new("/tmp/a.mp3"),
            false,
            "supertonic",
        );

        assert!(long.word_count > short.word_count);
        assert!(long.estimated_seconds > short.estimated_seconds);
        assert!(!short.cached);
    }
}
