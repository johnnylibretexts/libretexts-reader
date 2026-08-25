//! Pinned translation coverage for every language Supertonic can pronounce.
//!
//! Supertonic exposes 31 spoken languages plus the special `na` pronunciation
//! fallback. Translation is intentionally an English hub: every one of the 30
//! non-English spoken languages has an English -> language direction and a
//! language -> English direction. A single M2M100 runtime serves those pairs,
//! so readers download and verify roughly 496 MB once instead of keeping 60
//! mostly duplicated model directories.

use crate::tts::supertonic::voice::SUPERTONIC_LANGUAGES;

pub(crate) struct ModelFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

pub(crate) struct TranslationModel {
    pub model_id: String,
    pub cache_key: String,
    pub repo: String,
    pub revision: String,
    pub files: Vec<ModelFile>,
    pub source_token: String,
    pub target_token: String,
    pub verified: bool,
}

struct PinnedFile {
    path: &'static str,
    size_bytes: u64,
    sha256: &'static str,
}

const REPO: &str = "gn64/M2M100_418M_CTranslate2";
const REVISION: &str = "18e406c615ef2991fa74d53734bf66b0a6b10cb4";
const CACHE_KEY: &str = "m2m100-418m-int8-18e406c";

const FILES: &[PinnedFile] = &[
    PinnedFile {
        path: "model.bin",
        size_bytes: 490_667_752,
        sha256: "a1826980fc5c037e69c7ac94fcb56c03001a66f380eb71863cc0a3879e71421b",
    },
    PinnedFile {
        path: "config.json",
        size_bytes: 223,
        sha256: "8f6496adfc930cbfecbe8281112197705c488fab47d34b4829b06d7f478909af",
    },
    PinnedFile {
        path: "shared_vocabulary.json",
        size_bytes: 2_796_509,
        sha256: "7eb5d0ff184c6095c7c10f9911c0aea492250abd12854f9c3d787c64b1c6397e",
    },
    PinnedFile {
        path: "sentencepiece.bpe.model",
        size_bytes: 2_423_393,
        sha256: "d8f7c76ed2a5e0822be39f0a4f95a55eb19c78f4593ce609e2edbc2aea4d380a",
    },
];

/// Supertonic's actual spoken-language list, excluding English and the `na`
/// language-agnostic pronunciation fallback.
pub(crate) const TRANSLATION_LANGUAGES: &[&str] = &[
    "ko", "ja", "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hi", "hr", "hu", "id",
    "it", "lt", "lv", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "tr", "uk", "vi",
];

pub(crate) fn resolve_pair(source: &str, target: &str) -> Option<TranslationModel> {
    if source == target {
        return None;
    }
    let covered = (source == "en" && TRANSLATION_LANGUAGES.contains(&target))
        || (target == "en" && TRANSLATION_LANGUAGES.contains(&source));
    if !covered {
        return None;
    }

    Some(TranslationModel {
        // Include the direction because the language controls are part of the
        // inference configuration and therefore part of cache validity.
        model_id: format!("{REPO}@{REVISION}:{source}-{target}"),
        cache_key: CACHE_KEY.to_string(),
        repo: REPO.to_string(),
        revision: REVISION.to_string(),
        files: FILES
            .iter()
            .map(|file| ModelFile {
                path: file.path.to_string(),
                size_bytes: file.size_bytes,
                sha256: file.sha256.to_string(),
            })
            .collect(),
        source_token: format!("__{source}__"),
        target_token: format!("__{target}__"),
        verified: true,
    })
}

pub(crate) fn available_targets(source: &str) -> Vec<String> {
    if source == "en" {
        return TRANSLATION_LANGUAGES
            .iter()
            .filter(|target| SUPERTONIC_LANGUAGES.contains(target))
            .map(|target| (*target).to_string())
            .collect();
    }
    if TRANSLATION_LANGUAGES.contains(&source) {
        return vec!["en".to_string()];
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_supertonic_spoken_language_has_both_english_directions() {
        assert_eq!(TRANSLATION_LANGUAGES.len(), 30);
        for language in TRANSLATION_LANGUAGES {
            let forward = resolve_pair("en", language).expect("forward pair");
            let reverse = resolve_pair(language, "en").expect("reverse pair");
            assert_eq!(forward.cache_key, reverse.cache_key);
            assert_eq!(forward.source_token, "__en__");
            assert_eq!(forward.target_token, format!("__{language}__"));
            assert_eq!(reverse.source_token, format!("__{language}__"));
            assert_eq!(reverse.target_token, "__en__");
        }
    }

    #[test]
    fn language_agnostic_pronunciation_is_not_a_translation_target() {
        assert!(SUPERTONIC_LANGUAGES.contains(&"na"));
        assert!(!TRANSLATION_LANGUAGES.contains(&"na"));
        assert!(resolve_pair("en", "na").is_none());
        assert!(!available_targets("en").contains(&"na".to_string()));
    }

    #[test]
    fn manifest_is_complete_immutable_and_sha256_pinned() {
        let model = resolve_pair("en", "es").expect("pinned model");
        assert!(model.verified);
        assert_eq!(model.revision.len(), 40);
        assert_ne!(model.revision, "main");
        assert_eq!(model.files.len(), 4);
        for required in [
            "model.bin",
            "config.json",
            "shared_vocabulary.json",
            "sentencepiece.bpe.model",
        ] {
            let file = model
                .files
                .iter()
                .find(|file| file.path == required)
                .unwrap_or_else(|| panic!("missing {required}"));
            assert!(file.size_bytes > 0);
            assert_eq!(file.sha256.len(), 64);
            assert!(file.sha256.bytes().any(|byte| byte != b'0'));
        }
    }

    #[test]
    fn refuses_cross_language_unknown_and_no_op_pairs() {
        assert!(resolve_pair("es", "fr").is_none());
        assert!(resolve_pair("en", "klingon").is_none());
        assert!(resolve_pair("es", "es").is_none());
    }

    #[test]
    fn every_offered_target_is_one_supertonic_can_speak() {
        for target in available_targets("en") {
            assert!(SUPERTONIC_LANGUAGES.contains(&target.as_str()));
        }
    }
}
