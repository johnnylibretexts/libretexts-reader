use crate::error::{AppError, AppResult};
use crate::tts::fish::client::FishClient;
use crate::tts::provider::TtsProvider;

#[derive(Debug)]
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

    async fn synthesize(
        &self,
        text: &str,
        voice: &str,
        _language: &str,
        speed: f32,
    ) -> AppResult<Vec<u8>> {
        // Fish infers language from the text across 83 languages, so the
        // language parameter is unused rather than mapped.
        self.client
            .synthesize(text, &self.voice(voice)?, speed)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(voice_id: Option<String>) -> FishProvider {
        // `FishClient::new` only builds a `reqwest::Client`; it never makes
        // a request, so a dummy key is fine here.
        FishProvider::new(FishClient::new("sk-test".to_string()).unwrap(), voice_id)
    }

    #[test]
    fn a_non_empty_requested_voice_wins_over_the_configured_one() {
        let provider = provider(Some("configured-voice".to_string()));
        assert_eq!(
            provider.voice("requested-voice").unwrap(),
            "requested-voice"
        );
    }

    #[test]
    fn an_empty_requested_voice_falls_back_to_the_configured_one() {
        let provider = provider(Some("configured-voice".to_string()));
        assert_eq!(provider.voice("").unwrap(), "configured-voice");
        assert_eq!(provider.voice("   ").unwrap(), "configured-voice");
    }

    #[test]
    fn an_empty_requested_voice_with_no_configured_voice_fails_loudly() {
        let provider = provider(None);
        let error = provider.voice("").unwrap_err();
        assert_eq!(error.kind(), "voice");
    }
}
