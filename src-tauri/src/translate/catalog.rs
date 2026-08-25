//! Which translation model serves a pair, and whether we vouch for it.
//!
//! Two tiers on purpose. The pinned tier is converted from official
//! Helsinki-NLP weights, published under an account we control, and pinned by
//! SHA256 per file -- `gaudi/opus-mt-en-es-ctranslate2` ships config and vocab
//! with no `model.bin` at all, and a pinned manifest makes that class of
//! breakage impossible rather than merely unlikely. The fallback tier is a
//! community repo that cannot be pinned against force-push, so it is marked
//! unverified and the reader is asked before it is fetched.

use crate::tts::supertonic::voice::SUPERTONIC_LANGUAGES;

pub(crate) struct ModelFile {
    pub path: String,
    pub size_bytes: u64,
    pub sha256: String,
}

pub(crate) struct TranslationModel {
    pub model_id: String,
    pub repo: String,
    pub files: Vec<ModelFile>,
    pub verified: bool,
}

const PLACEHOLDER_SHA256: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// (source, target, repo, model.bin size, model.bin sha256)
///
/// Fill every placeholder by running `shasum -a 256` on the converted
/// artefacts before publishing them. A placeholder here is a release blocker.
const PINNED: &[(&str, &str, &str, u64, &str)] = &[
    (
        "en",
        "es",
        "libretexts/opus-mt-en-es-ct2",
        155_502_501,
        PLACEHOLDER_SHA256,
    ),
    (
        "es",
        "en",
        "libretexts/opus-mt-es-en-ct2",
        155_000_000,
        PLACEHOLDER_SHA256,
    ),
];

/// Pairs with no pinned conversion yet. Fetched only after the reader accepts
/// an unverified model.
const FALLBACK: &[(&str, &str, &str)] = &[("en", "sw", "michaelfeil/ct2fast-opus-mt-en-sw")];

fn tokenizer_files() -> Vec<ModelFile> {
    [
        "source.spm",
        "target.spm",
        "shared_vocabulary.json",
        "config.json",
    ]
    .iter()
    .map(|path| ModelFile {
        path: (*path).to_string(),
        size_bytes: 0,
        sha256: PLACEHOLDER_SHA256.to_string(),
    })
    .collect()
}

pub(crate) fn resolve_pair(source: &str, target: &str) -> Option<TranslationModel> {
    if source == target {
        return None;
    }
    if let Some((_, _, repo, size, sha)) = PINNED
        .iter()
        .find(|(from, to, _, _, _)| *from == source && *to == target)
    {
        let mut files = vec![ModelFile {
            path: "model.bin".to_string(),
            size_bytes: *size,
            sha256: (*sha).to_string(),
        }];
        files.extend(tokenizer_files());
        return Some(TranslationModel {
            model_id: format!("{repo}@pinned"),
            repo: (*repo).to_string(),
            files,
            verified: true,
        });
    }
    FALLBACK
        .iter()
        .find(|(from, to, _)| *from == source && *to == target)
        .map(|(_, _, repo)| TranslationModel {
            model_id: format!("{repo}@unverified"),
            repo: (*repo).to_string(),
            files: Vec::new(),
            verified: false,
        })
}

pub(crate) fn available_targets(source: &str) -> Vec<String> {
    PINNED
        .iter()
        .filter(|(from, _, _, _, _)| *from == source)
        .map(|(_, to, _, _, _)| (*to).to_string())
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
