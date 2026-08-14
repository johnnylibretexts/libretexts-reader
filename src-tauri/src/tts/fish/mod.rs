//! Fish Audio: an optional cloud TTS provider the user supplies a key for.
//!
//! Runs in Rust rather than the webview so the API key never crosses into it,
//! and because chapter export already lives here.

pub mod client;
pub mod provider;

/// The model Fish should use, as one constant.
///
/// Fish selects the model from an HTTP header and falls back to `s2.1-pro`
/// when it is missing or unrecognised. This value is also hashed into the
/// audio cache key, so a literal at a call site could silently pin cached
/// audio to a model we never meant to use.
pub const FISH_MODEL: &str = "s2.1-pro";

pub const FISH_BASE_URL: &str = "https://api.fish.audio";

/// Fish publishes no latency SLA, so every call needs a ceiling.
pub const FISH_TIMEOUT_SECONDS: u64 = 20;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fish_model_constant_is_the_paid_model() {
        // Fish selects the model from an HTTP header and silently falls back
        // to a different model when that header is unrecognised. This constant
        // is also hashed into the audio cache key, so a typo would pin cached
        // audio to a model that was never intended -- and every later run
        // would hit that cache and never reveal the mistake.
        assert_eq!(FISH_MODEL, "s2.1-pro");
    }
}
