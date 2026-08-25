//! Which translation model serves a pair, and whether we vouch for it.
//!
//! Two tiers on purpose. The pinned tier is converted from official
//! Helsinki-NLP weights and pinned by both repository revision and SHA256 per
//! file -- `gaudi/opus-mt-en-es-ctranslate2` ships config and vocab with no
//! `model.bin` at all, and a pinned manifest makes that class of breakage
//! impossible rather than merely unlikely. The fallback tier follows a
//! community repo's moving branch, so it is marked unverified and the reader
//! is asked before it is fetched.

use crate::tts::supertonic::voice::SUPERTONIC_LANGUAGES;

pub(crate) struct ModelFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

pub(crate) struct TranslationModel {
    pub model_id: String,
    pub repo: String,
    pub revision: String,
    pub files: Vec<ModelFile>,
    pub verified: bool,
}

struct PinnedFile {
    path: &'static str,
    size_bytes: u64,
    sha256: &'static str,
}

struct PinnedModel {
    source: &'static str,
    target: &'static str,
    repo: &'static str,
    revision: &'static str,
    files: &'static [PinnedFile],
}

const COMMON_CONFIG: PinnedFile = PinnedFile {
    path: "config.json",
    size_bytes: 159,
    sha256: "0c2f6fa2057c7264d052fb4a62ba3476eeae70487acddfa8e779a53a00cbf44c",
};
const COMMON_VOCABULARY: PinnedFile = PinnedFile {
    path: "shared_vocabulary.txt",
    size_bytes: 666_435,
    sha256: "77aee99211b7b8e569e0fb5b95dac01aba9f31bca2d1380b1fc6050797825ec6",
};
const ENGLISH_SPM: PinnedFile = PinnedFile {
    path: "source.spm",
    size_bytes: 801_636,
    sha256: "4dd547c24816a335e7b0b2e63376a8f1b3cbfc671eda5ab808dd44fdadaa8791",
};
const SPANISH_SPM: PinnedFile = PinnedFile {
    path: "target.spm",
    size_bytes: 825_924,
    sha256: "e236ee6d866b635c0142114f8647f39831f9d92534aa2aad75c942f6a78ad0e3",
};

const EN_ES_FILES: &[PinnedFile] = &[
    PinnedFile {
        path: "model.bin",
        size_bytes: 155_502_501,
        sha256: "36cd9bcb181fc6d5832deeaf770ce183ff4edbbc5e4fe0f86cec92da4379f3b7",
    },
    COMMON_CONFIG,
    COMMON_VOCABULARY,
    ENGLISH_SPM,
    SPANISH_SPM,
];

const ES_EN_FILES: &[PinnedFile] = &[
    PinnedFile {
        path: "model.bin",
        size_bytes: 155_502_501,
        sha256: "3a3b91dcb396ee7b682554e7d9f501909385c48b478a691bfe9bf9e3e32d3656",
    },
    COMMON_CONFIG,
    COMMON_VOCABULARY,
    PinnedFile {
        path: "source.spm",
        size_bytes: SPANISH_SPM.size_bytes,
        sha256: SPANISH_SPM.sha256,
    },
    PinnedFile {
        path: "target.spm",
        size_bytes: ENGLISH_SPM.size_bytes,
        sha256: ENGLISH_SPM.sha256,
    },
];

/// Published CTranslate2 conversions, locked to immutable Hugging Face commits
/// and to the SHA256 and byte length of every file the runtime opens.
const PINNED: &[PinnedModel] = &[
    PinnedModel {
        source: "en",
        target: "es",
        repo: "michaelfeil/ct2fast-opus-mt-en-es",
        revision: "76ec296588e2234f9b7dfad5254219a0f5ecb7af",
        files: EN_ES_FILES,
    },
    PinnedModel {
        source: "es",
        target: "en",
        repo: "michaelfeil/ct2fast-opus-mt-es-en",
        revision: "437f5ffc6c8544943c685ea405650e0d17cf6098",
        files: ES_EN_FILES,
    },
];

/// Pairs with no pinned conversion yet. Fetched only after the reader accepts
/// an unverified model.
const FALLBACK: &[(&str, &str, &str)] = &[("en", "sw", "michaelfeil/ct2fast-opus-mt-en-sw")];

pub(crate) fn resolve_pair(source: &str, target: &str) -> Option<TranslationModel> {
    if source == target {
        return None;
    }
    if let Some(pinned) = PINNED
        .iter()
        .find(|model| model.source == source && model.target == target)
    {
        return Some(TranslationModel {
            model_id: format!("{}@{}", pinned.repo, pinned.revision),
            repo: pinned.repo.to_string(),
            revision: pinned.revision.to_string(),
            files: pinned
                .files
                .iter()
                .map(|file| ModelFile {
                    path: file.path.to_string(),
                    size_bytes: file.size_bytes,
                    sha256: file.sha256.to_string(),
                })
                .collect(),
            verified: true,
        });
    }
    FALLBACK
        .iter()
        .find(|(from, to, _)| *from == source && *to == target)
        .map(|(_, _, repo)| TranslationModel {
            model_id: format!("{repo}@unverified"),
            repo: (*repo).to_string(),
            revision: "main".to_string(),
            files: Vec::new(),
            verified: false,
        })
}

pub(crate) fn available_targets(source: &str) -> Vec<String> {
    PINNED
        .iter()
        .filter(|model| model.source == source)
        .map(|model| model.target.to_string())
        .chain(
            FALLBACK
                .iter()
                .filter(|(from, _, _)| *from == source)
                .map(|(_, to, _)| (*to).to_string()),
        )
        .filter(|target| SUPERTONIC_LANGUAGES.contains(&target.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_the_pinned_conversion_over_the_community_one() {
        let model = resolve_pair("en", "es").expect("en-es is pinned");
        assert!(model.verified);
        assert!(!model.files.is_empty());
        assert!(
            model.files.iter().any(|file| file.path == "model.bin"),
            "a translation model without weights is the gaudi/opus-mt-en-es \
             failure this catalogue exists to make impossible"
        );
        assert!(model.files.iter().all(|file| file.sha256.len() == 64));
    }

    #[test]
    fn pinned_manifests_have_no_release_placeholders() {
        for pinned in PINNED {
            assert_ne!(pinned.revision, "main");
            assert_eq!(pinned.revision.len(), 40);
            assert!(!pinned.files.is_empty());
            for file in pinned.files {
                assert!(file.size_bytes > 0, "{} has no byte length", file.path);
                assert_eq!(file.sha256.len(), 64, "{} has a bad SHA256", file.path);
                assert!(
                    file.sha256.bytes().any(|byte| byte != b'0'),
                    "{} still has a placeholder SHA256",
                    file.path
                );
            }
        }
    }

    #[test]
    fn falls_back_to_an_unverified_repo_and_says_so() {
        let model = resolve_pair("en", "sw").expect("swahili is fallback-only");
        assert!(!model.verified);
    }

    #[test]
    fn refuses_a_pair_nothing_covers_and_refuses_a_no_op() {
        assert!(resolve_pair("en", "klingon").is_none());
        assert!(
            resolve_pair("es", "es").is_none(),
            "source == target is not a translation"
        );
    }

    #[test]
    fn every_offered_target_is_one_supertonic_can_speak() {
        // Translating into a language the speech engine cannot pronounce
        // produces nothing usable, so the two catalogues gate each other.
        for target in available_targets("en") {
            assert!(
                SUPERTONIC_LANGUAGES.contains(&target.as_str()),
                "{target} is not a Supertonic language"
            );
        }
    }
}
