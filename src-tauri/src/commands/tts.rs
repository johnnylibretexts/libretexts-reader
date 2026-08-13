use serde::{Deserialize, Serialize};

use crate::commands::supertonic_tts;
use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesizeSpeechRequest {
    pub text: String,
    pub speed: f32,
    pub voice_id: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechAudio {
    pub audio: Vec<u8>,
    pub mime_type: String,
}

/// Synthesize one piece of text with Supertonic.
///
/// The caller states the voice and language it wants. This command no longer
/// reads `tts_provider` to decide whether it should run: which engine speaks is
/// the SpeechEngine adapter's decision in the webview, and making it a second
/// time here — from a different source, with no ordering guarantee between the
/// two — is what this replaced. It also meant the command failed by default,
/// since settings seed `tts_provider` to `kokoro`.
#[tauri::command]
pub async fn synthesize_speech(request: SynthesizeSpeechRequest) -> AppResult<SpeechAudio> {
    let text = request.text.trim();
    if text.is_empty() {
        return Err(AppError::InvalidInput("text is required".into()));
    }

    supertonic_tts::synthesize_supertonic_text(
        text,
        request.voice_id.as_deref(),
        request.language.as_deref(),
        request.speed,
    )
    .await
}
