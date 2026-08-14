use serde::{Deserialize, Serialize};
use tauri::State;

use crate::commands::chapter_tts;
use crate::db::connection::DbPool;
use crate::error::{AppError, AppResult};
use crate::tts::provider::TtsProvider;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesizeSpeechRequest {
    pub text: String,
    pub speed: f32,
    /// Which engine speaks this text. No `#[serde(default)]`: a request that
    /// omits it is a bug in the caller, not a reason to guess. See the
    /// doc comment below for why this can never fall back to a settings read.
    pub provider: String,
    pub voice_id: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechAudio {
    pub audio: Vec<u8>,
    pub mime_type: String,
}

/// Synthesize one piece of text for playback.
///
/// The caller states the voice, language, and now the engine it wants. This
/// command no longer reads `tts_provider` to decide which engine runs: which
/// one speaks is the SpeechEngine adapter's decision in the webview, and
/// making it a second time here — from a different source, with no ordering
/// guarantee between the two — is what this replaced. It also meant the
/// command failed by default, because the seeded provider was not the one
/// this command served. Keeping the decision in one place is what let a
/// second provider be added without this command relearning it from settings.
///
/// `request.provider` is that one decision, arriving as a plain field with no
/// default. `provider_for` (in `commands::chapter_tts`) rejects anything it
/// does not recognise rather than falling back, for the same reason: a silent
/// fallback here would be the exact bug this doc comment describes, just
/// moved one field over.
///
/// Supertonic keeps its own code path rather than going through
/// `TtsProvider::synthesize`: that trait always returns MP3, while playback
/// has always returned WAV, and routing it through the trait would add a
/// per-sentence MP3 decode for no benefit. `provider_for` is still called
/// first, so an unknown provider or a missing Fish key is rejected the same
/// way regardless of which branch a valid one takes.
#[tauri::command]
pub async fn synthesize_speech(
    state: State<'_, DbPool>,
    request: SynthesizeSpeechRequest,
) -> AppResult<SpeechAudio> {
    let text = request.text.trim();
    if text.is_empty() {
        return Err(AppError::InvalidInput("text is required".into()));
    }

    let settings = chapter_tts::provider_settings_from_state(&state)?;
    let provider = chapter_tts::provider_for(&request.provider, &settings)?;

    if provider.id() == "supertonic" {
        return chapter_tts::synthesize_supertonic_text(
            text,
            request.voice_id.as_deref(),
            request.language.as_deref(),
            request.speed,
        )
        .await;
    }

    let voice = request.voice_id.clone().unwrap_or_default();
    let language = request.language.clone().unwrap_or_default();
    synthesize_with(provider.as_ref(), text, &voice, &language, request.speed).await
}

/// The non-Supertonic half of `synthesize_speech`, pulled out so a test can
/// hand it a stub `TtsProvider` and inspect exactly what reached
/// `synthesize` -- the same reason `cache_path_in` takes a root instead of
/// calling `paths::cache_dir()`, or `provider_for` takes a `ProviderSettings`
/// instead of a `DbPool`. Without this seam, nothing in the test suite calls
/// the line that forwards `speed`, so a regression back to a hardcoded value
/// would compile and pass every test.
async fn synthesize_with(
    provider: &dyn TtsProvider,
    text: &str,
    voice: &str,
    language: &str,
    speed: f32,
) -> AppResult<SpeechAudio> {
    let audio = provider.synthesize(text, voice, language, speed).await?;

    Ok(SpeechAudio {
        audio,
        mime_type: "audio/mpeg".to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tts::provider::VoiceSummary;

    #[derive(Debug, Default)]
    struct StubProvider {
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
    async fn synthesize_with_forwards_the_requested_speed_unchanged() {
        // A distinctive, non-default value: 1.0 would still pass if the call
        // site regressed to hardcoding the default speed instead of
        // forwarding the caller's.
        let stub = StubProvider::default();

        synthesize_with(&stub, "hello", "voice-1", "en", 1.75)
            .await
            .expect("synthesize_with should succeed");

        assert_eq!(*stub.received_speed.lock().expect("speed lock"), Some(1.75));
    }

    #[tokio::test]
    async fn synthesize_with_returns_the_mp3_mime_type() {
        // TtsProvider::synthesize always returns encoded MP3 bytes -- the
        // response has to say so, since playback picks its decoder from
        // this field.
        let stub = StubProvider::default();

        let audio = synthesize_with(&stub, "hello", "voice-1", "en", 1.0)
            .await
            .expect("synthesize_with should succeed");

        assert_eq!(audio.mime_type, "audio/mpeg");
    }
}
