//! Which voice styles and languages Supertonic accepts, and how a requested
//! one is resolved.
//!
//! Two postures on purpose. `resolve_*` rejects an unknown value and is used by
//! user-initiated commands. `playback_voice_style` / `normalize_language` fall
//! back instead, because the player carries one voice id across engines and
//! cutting the audio off mid-chapter is worse than reading it in another voice.

use crate::error::{AppError, AppResult};

pub(crate) const SUPERTONIC_LANGUAGES: &[&str] = &[
    "en", "ko", "ja", "ar", "bg", "cs", "da", "de", "el", "es", "et", "fi", "fr", "hi", "hr", "hu",
    "id", "it", "lt", "lv", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "tr", "uk", "vi", "na",
];

const SUPERTONIC_VOICE_STYLES: &[&str] =
    &["M1", "M2", "M3", "M4", "M5", "F1", "F2", "F3", "F4", "F5"];

pub(crate) const DEFAULT_VOICE_STYLE: &str = "M1";
pub(crate) const DEFAULT_LANGUAGE: &str = "en";

pub(crate) fn is_valid_supertonic_language(language: &str) -> bool {
    SUPERTONIC_LANGUAGES.contains(&language)
}

pub(crate) fn resolve_language(value: Option<&str>, fallback: &str) -> AppResult<String> {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    if is_valid_supertonic_language(candidate) {
        Ok(candidate.to_string())
    } else {
        Err(AppError::InvalidInput(format!(
            "unknown Supertonic language: {candidate}"
        )))
    }
}

pub(crate) fn normalize_language<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    if is_valid_supertonic_language(candidate) {
        candidate
    } else {
        DEFAULT_LANGUAGE
    }
}

pub(crate) fn resolve_voice_style(value: Option<&str>, fallback: &str) -> AppResult<String> {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    SUPERTONIC_VOICE_STYLES
        .iter()
        .find(|voice_style| voice_style.eq_ignore_ascii_case(candidate))
        .map(|voice_style| (*voice_style).to_string())
        .ok_or_else(|| {
            AppError::InvalidInput(format!("unknown Supertonic voice style: {candidate}"))
        })
}

pub(crate) fn playback_voice_style(value: Option<&str>, fallback: &str) -> String {
    let candidate = value.map(str::trim).filter(|value| !value.is_empty());
    SUPERTONIC_VOICE_STYLES
        .iter()
        .find(|voice_style| {
            candidate.is_some_and(|candidate| voice_style.eq_ignore_ascii_case(candidate))
        })
        .copied()
        .unwrap_or(fallback)
        .to_string()
}

pub(crate) fn normalize_voice_style<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    let candidate = value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback);
    SUPERTONIC_VOICE_STYLES
        .iter()
        .find(|voice_style| voice_style.eq_ignore_ascii_case(candidate))
        .copied()
        .unwrap_or(DEFAULT_VOICE_STYLE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolving_rejects_a_voice_the_engine_does_not_have() {
        assert!(resolve_voice_style(Some("af_heart"), DEFAULT_VOICE_STYLE).is_err());
        assert_eq!(
            resolve_voice_style(Some("f2"), DEFAULT_VOICE_STYLE).unwrap(),
            "F2",
            "voice ids should match case-insensitively"
        );
    }

    #[test]
    fn playback_falls_back_instead_of_failing() {
        // The player carries one voice id across engines, so this can be handed
        // a Kokoro id. Reading the chapter in another voice beats silence.
        assert_eq!(
            playback_voice_style(Some("af_heart"), DEFAULT_VOICE_STYLE),
            "M1"
        );
        assert_eq!(playback_voice_style(Some("F3"), DEFAULT_VOICE_STYLE), "F3");
        assert_eq!(playback_voice_style(None, DEFAULT_VOICE_STYLE), "M1");
    }

    #[test]
    fn language_resolution_has_the_same_two_postures() {
        assert!(resolve_language(Some("klingon"), DEFAULT_LANGUAGE).is_err());
        assert_eq!(
            resolve_language(Some("fr"), DEFAULT_LANGUAGE).unwrap(),
            "fr"
        );

        assert_eq!(normalize_language(Some("klingon"), DEFAULT_LANGUAGE), "en");
        assert_eq!(normalize_language(Some("ja"), DEFAULT_LANGUAGE), "ja");
    }

    #[test]
    fn blank_input_takes_the_fallback_not_an_error() {
        assert_eq!(
            resolve_voice_style(Some("   "), DEFAULT_VOICE_STYLE).unwrap(),
            "M1"
        );
        assert_eq!(resolve_language(Some(""), DEFAULT_LANGUAGE).unwrap(), "en");
    }
}
