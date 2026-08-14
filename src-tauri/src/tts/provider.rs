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

#[async_trait::async_trait]
pub trait TtsProvider: Send + Sync + std::fmt::Debug {
    fn id(&self) -> &'static str;

    /// Encoded MP3 bytes. Both implementations return the same thing so the
    /// export path never branches on which one produced the audio. `speed`
    /// is honoured where the engine supports it: both do today, each via its
    /// own native parameter (Supertonic's synthesis step count, Fish's
    /// `prosody.speed`).
    async fn synthesize(
        &self,
        text: &str,
        voice: &str,
        language: &str,
        speed: f32,
    ) -> AppResult<Vec<u8>>;

    /// Reports whether the engine is usable right now. For Supertonic this
    /// is a status check, not a download — fetching the model is a separate
    /// command that reports progress. For Fish it checks the key and voice.
    async fn ensure_ready(&self) -> AppResult<()>;

    async fn list_voices(&self) -> AppResult<Vec<VoiceSummary>>;
}

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
        async fn ensure_ready(&self) -> AppResult<()> {
            Ok(())
        }
        async fn list_voices(&self) -> AppResult<Vec<VoiceSummary>> {
            Ok(vec![])
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
