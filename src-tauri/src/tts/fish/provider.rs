use crate::error::{AppError, AppResult};
use crate::tts::fish::client::FishClient;
use crate::tts::provider::{TtsProvider, VoiceSummary};

pub struct FishProvider {
    client: FishClient,
    voice_id: Option<String>,
}

impl FishProvider {
    pub fn new(client: FishClient, voice_id: Option<String>) -> Self {
        Self { client, voice_id }
    }

    /// Fish has no sensible built-in default voice, so an unset one is an
    /// error the user can act on rather than a guess.
    fn voice(&self, requested: &str) -> AppResult<String> {
        if !requested.trim().is_empty() {
            return Ok(requested.to_string());
        }
        self.voice_id.clone().ok_or_else(|| {
            AppError::Voice("Choose a Fish Audio voice in Settings before using it.".into())
        })
    }
}

#[async_trait::async_trait]
impl TtsProvider for FishProvider {
    fn id(&self) -> &'static str {
        "fish"
    }

    async fn synthesize(&self, text: &str, voice: &str, _language: &str) -> AppResult<Vec<u8>> {
        // Fish infers language from the text across 83 languages, so the
        // language parameter is unused rather than mapped.
        self.client.synthesize(text, &self.voice(voice)?, 1.0).await
    }

    async fn ensure_ready(&self) -> AppResult<()> {
        self.voice("")?;
        self.client.credit().await.map(|_| ())
    }

    async fn list_voices(&self) -> AppResult<Vec<VoiceSummary>> {
        self.client.list_voices().await
    }
}
