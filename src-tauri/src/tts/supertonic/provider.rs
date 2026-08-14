use crate::commands::chapter_tts::synthesize_supertonic_mp3;
use crate::error::AppResult;
use crate::tts::provider::{TtsProvider, VoiceSummary};
use crate::tts::supertonic::model::supertonic_model_status;
use crate::tts::supertonic::voice::SUPERTONIC_VOICE_STYLES;
use crate::tts::supertonic::SUPERTONIC_DEFAULT_SPEED;

#[derive(Debug)]
pub struct SupertonicProvider;

#[async_trait::async_trait]
impl TtsProvider for SupertonicProvider {
    fn id(&self) -> &'static str {
        "supertonic"
    }

    async fn synthesize(&self, text: &str, voice: &str, language: &str) -> AppResult<Vec<u8>> {
        synthesize_supertonic_mp3(
            text.to_string(),
            voice.to_string(),
            language.to_string(),
            SUPERTONIC_DEFAULT_SPEED,
        )
        .await
    }

    async fn ensure_ready(&self) -> AppResult<()> {
        // Synchronous, and deliberately not a download: fetching the model is
        // a separate command that reports progress. This only answers whether
        // the model is already usable.
        supertonic_model_status().map(|_| ())
    }

    async fn list_voices(&self) -> AppResult<Vec<VoiceSummary>> {
        Ok(SUPERTONIC_VOICE_STYLES
            .iter()
            .map(|style| VoiceSummary {
                id: (*style).to_string(),
                name: (*style).to_string(),
                ready: true,
            })
            .collect())
    }
}
