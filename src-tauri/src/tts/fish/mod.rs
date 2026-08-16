//! Fish Audio: an optional cloud TTS provider the user supplies a key for.
//!
//! Runs in Rust rather than the webview so the API key never crosses into it,
//! and because chapter export already lives here.

use std::time::Duration;

pub mod client;
pub mod provider;

/// The model Fish should use, as one constant.
///
/// Fish selects the model from an HTTP header and falls back to a different
/// model when it is missing or unrecognised. This value is also hashed into
/// the audio cache key, so a literal at a call site could silently pin cached
/// audio to a model we never meant to use — and every later run would hit
/// that cache and never reveal the mistake.
///
/// What guards that is `the_synthesis_request_carries_the_paid_model_in_its_header`
/// in `client.rs`, which builds the real request and reads the header back. A
/// test here asserting this constant equals its own literal could only fail
/// if someone edited both lines together, so it is not worth having.
pub const FISH_MODEL: &str = "s2.1-pro";

pub const FISH_BASE_URL: &str = "https://api.fish.audio";

/// Fish publishes no latency SLA, so every call needs a ceiling.
///
/// This one bounds the control-plane calls only — `credit` and `list_voices`,
/// which return a small JSON body and have no reason to take longer.
/// Synthesis does not use it; see [`synthesis_timeout`].
pub const FISH_TIMEOUT_SECONDS: u64 = 20;

/// The hard ceiling on one synthesis request, whatever its length.
///
/// A budget that grows without bound is not a ceiling, and the point of
/// timing out at all is that Fish publishes no latency SLA.
pub const FISH_MAX_SYNTHESIS_TIMEOUT_SECONDS: u64 = 900;

/// Characters of input we allow one second of request budget for.
///
/// Deliberately generous against Fish's real throughput: the cost of being
/// too tight is a paid synthesis thrown away, the cost of being too loose is
/// a slower failure.
const FISH_CHARS_PER_TIMEOUT_SECOND: usize = 50;

/// How long one synthesis request may take, scaled to the text it carries.
///
/// [`FISH_TIMEOUT_SECONDS`] is reqwest's *total* request budget, response body
/// included. Applied to a whole-chapter synthesis it aborted the request long
/// after Fish had synthesized and billed for it — the reader paid, got no
/// audio, and hit the identical failure on retry. Playback and export share
/// one code path, so the budget has to come from the request rather than the
/// client: a sentence keeps a short ceiling, a chapter gets minutes.
pub fn synthesis_timeout(text_len: usize) -> Duration {
    let allowance = (text_len / FISH_CHARS_PER_TIMEOUT_SECOND) as u64;
    let seconds = FISH_TIMEOUT_SECONDS
        .saturating_add(allowance)
        .min(FISH_MAX_SYNTHESIS_TIMEOUT_SECONDS);

    Duration::from_secs(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sentence_stays_near_the_control_plane_ceiling() {
        // Playback synthesizes one sentence at a time. A generous chapter-sized
        // ceiling here would leave a stuck player hanging for minutes.
        let timeout = synthesis_timeout(200);
        assert!(
            timeout <= Duration::from_secs(60),
            "a single sentence should not buy a multi-minute stall, got {timeout:?}"
        );
    }

    #[test]
    fn a_chapter_outlives_the_control_plane_ceiling_by_minutes() {
        // ~40k characters is a routine textbook chapter: roughly 20 minutes of
        // audio to synthesize and download in one request.
        let timeout = synthesis_timeout(40_000);
        assert!(
            timeout >= Duration::from_secs(300),
            "a chapter cannot synthesize and download in {timeout:?}"
        );
    }

    #[test]
    fn the_ceiling_is_capped_rather_than_unbounded() {
        // Without a cap a pathological input would mean no ceiling at all,
        // which is the failure the constant above exists to prevent.
        assert_eq!(
            synthesis_timeout(usize::MAX),
            Duration::from_secs(FISH_MAX_SYNTHESIS_TIMEOUT_SECONDS)
        );
    }
}
