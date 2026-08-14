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
pub trait TtsProvider: Send + Sync {
    fn id(&self) -> &'static str;

    /// Encoded MP3 bytes. Both implementations return the same thing so the
    /// export path never branches on which one produced the audio.
    async fn synthesize(&self, text: &str, voice: &str, language: &str) -> AppResult<Vec<u8>>;

    /// Downloads the model for Supertonic; checks the key and voice for Fish.
    async fn ensure_ready(&self) -> AppResult<()>;

    async fn list_voices(&self) -> AppResult<Vec<VoiceSummary>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubProvider;

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
        ) -> AppResult<Vec<u8>> {
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
        let provider: Box<dyn TtsProvider> = Box::new(StubProvider);
        assert_eq!(provider.id(), "stub");
        assert_eq!(provider.synthesize("hi", "v", "en").await.unwrap(), b"hi");
    }
}
