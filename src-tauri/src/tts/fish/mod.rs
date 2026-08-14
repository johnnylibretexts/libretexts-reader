//! Fish Audio: an optional cloud TTS provider the user supplies a key for.
//!
//! Runs in Rust rather than the webview so the API key never crosses into it,
//! and because chapter export already lives here.

pub mod client;

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
