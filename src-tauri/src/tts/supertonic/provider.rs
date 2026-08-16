use crate::commands::chapter_tts::synthesize_supertonic_mp3;
use crate::error::AppResult;
use crate::tts::provider::TtsProvider;

#[derive(Debug)]
pub struct SupertonicProvider;

#[async_trait::async_trait]
impl TtsProvider for SupertonicProvider {
    fn id(&self) -> &'static str {
        "supertonic"
    }

    async fn synthesize(
        &self,
        text: &str,
        voice: &str,
        language: &str,
        speed: f32,
    ) -> AppResult<Vec<u8>> {
        synthesize_supertonic_mp3(
            text.to_string(),
            voice.to_string(),
            language.to_string(),
            speed,
        )
        .await
    }
}
