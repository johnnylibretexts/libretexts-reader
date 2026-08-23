//! The seam between "some engine speaks" and "which engine speaks".
//!
//! Deliberately narrow. Caching, progress events, estimates and the export
//! confirmation live outside it, in shared command code: Supertonic reports
//! chunk-level progress and Fish returns a whole utterance, and a trait that
//! tried to model both notions of progress would leak one into the other.
//!
//! Mirrors `SpeechEngine` in `src/lib/speech/types.ts`. Two layers, one idea.

use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSummary {
    pub id: String,
    pub name: String,
    pub ready: bool,
}

/// The name a provider goes by in anything a person reads — today, the
/// exported MP3's filename.
///
/// Mirrors `SPEECH_ENGINE_LABELS` in `src/lib/speech/types.ts`; the same
/// reason that exists here too, so a filename can never claim one engine
/// produced audio that another did. An unrecognised name passes through
/// unchanged rather than erroring: this only decides a filename, and
/// `model_for_provider` has already rejected an unknown provider long before
/// an export reaches it.
pub fn provider_display_name(provider: &str) -> &str {
    match provider {
        "supertonic" => "Supertonic",
        "fish" => "Fish Audio",
        other => other,
    }
}

/// The container a provider's exported audio arrives in, without the dot.
///
/// A free function beside `provider_display_name` and for the same reason: the
/// path helpers know a provider only by its id string, having never built the
/// object. Falls back to MP3 for an unknown id, matching what every provider
/// produced before AAC arrived -- a wrong extension misnames a file, while
/// panicking here would fail an export outright.
pub fn export_extension(provider: &str) -> &'static str {
    match provider {
        "supertonic" => "m4a",
        "fish" => "mp3",
        _ => "mp3",
    }
}

#[async_trait::async_trait]
pub trait TtsProvider: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &'static str;

    /// Encoded audio bytes, in the container `export_extension` names for
    /// this provider's id.
    ///
    /// These used to be MP3 for both, so the export path never branched. They
    /// no longer are: Supertonic encodes locally, and macOS offers no MP3
    /// *encoder*, so dropping the LGPL LAME dependency meant moving Supertonic
    /// to AAC/M4A (ADR-0004). Fish still returns MP3 because that is what its
    /// API sends, and re-encoding lossy audio to change the extension would
    /// cost quality for nothing. Anything that names or tags the output must
    /// therefore ask `export_extension` rather than assume.
    ///
    /// `speed` is honoured where the engine supports it: both do today, each
    /// via its own native parameter (Supertonic's synthesis step count, Fish's
    /// `prosody.speed`).
    async fn synthesize(
        &self,
        text: &str,
        voice: &str,
        language: &str,
        speed: f32,
    ) -> AppResult<Vec<u8>>;
}

// This trait carried `ensure_ready` and `list_voices` too, and nothing ever
// called either through it: readiness is answered by the provider-specific
// `get_supertonic_model_status` / `get_fish_key_status`, and voices by
// `list_fish_voices`, which builds a `FishClient` directly. They were surface
// that had to be implemented and kept correct — Fish's `ensure_ready` made a
// live wallet call — while the one command that lists voices bypassed
// `provider_for` and so could not serve a second provider anyway.
//
// If a second provider ever needs voice listing, the fix is to route that
// command through `provider_for` and add the method back with a caller, not
// to reinstate an abstract method nothing dispatches on.

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Default)]
    struct StubProvider {
        // Interior mutability rather than `&mut self`: `synthesize` takes
        // `&self` (trait objects are shared, not owned exclusively), so
        // recording what it was called with needs a lock, not a field write.
        received_speed: std::sync::Mutex<Option<f32>>,
    }

    #[async_trait::async_trait]
    impl TtsProvider for StubProvider {
        fn id(&self) -> &'static str {
            "stub"
        }
        async fn synthesize(
            &self,
            text: &str,
            _voice: &str,
            _language: &str,
            speed: f32,
        ) -> AppResult<Vec<u8>> {
            *self.received_speed.lock().expect("speed lock") = Some(speed);
            Ok(text.as_bytes().to_vec())
        }
    }

    #[tokio::test]
    async fn a_provider_is_usable_behind_a_trait_object() {
        // The point of the trait: shared export code holds `dyn TtsProvider`
        // and never branches on which engine produced the bytes.
        let provider: Box<dyn TtsProvider> = Box::new(StubProvider::default());
        assert_eq!(provider.id(), "stub");
        assert_eq!(
            provider.synthesize("hi", "v", "en", 1.0).await.unwrap(),
            b"hi"
        );
    }

    #[tokio::test]
    async fn synthesize_forwards_the_requested_speed_unchanged() {
        // A call site that hardcoded a value instead of passing `speed`
        // through would still compile -- only a test that inspects what
        // actually arrived can catch that regression.
        let stub = StubProvider::default();

        stub.synthesize("hi", "v", "en", 1.75).await.unwrap();

        assert_eq!(*stub.received_speed.lock().expect("speed lock"), Some(1.75));
    }
}
