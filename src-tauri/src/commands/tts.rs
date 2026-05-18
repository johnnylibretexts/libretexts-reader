use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::commands::supertonic_tts;
use crate::db::connection::DbPool;
use crate::db::settings;
use crate::error::{AppError, AppResult};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesizeSpeechRequest {
    pub text: String,
    pub speed: f32,
    pub voice_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechAudio {
    pub audio: Vec<u8>,
    pub mime_type: String,
}

#[tauri::command]
pub async fn synthesize_speech(
    state: State<'_, DbPool>,
    request: SynthesizeSpeechRequest,
) -> AppResult<SpeechAudio> {
    let text = request.text.trim();
    if text.is_empty() {
        return Err(AppError::InvalidInput("text is required".into()));
    }

    let values = {
        let conn = state.get()?;
        settings::get_all_settings(&conn)?
    };
    let provider = setting_string(&values, "tts_provider", "kokoro");

    if provider == "supertonic" {
        return supertonic_tts::synthesize_supertonic_text(
            &values,
            text,
            request.voice_id.as_deref(),
            request.speed,
        )
        .await;
    }

    Err(AppError::InvalidInput(
        "Supertonic is not the selected TTS provider.".into(),
    ))
}

fn setting_string(
    values: &std::collections::HashMap<String, Value>,
    key: &str,
    fallback: &str,
) -> String {
    optional_setting_string(values, key).unwrap_or_else(|| fallback.to_string())
}

fn optional_setting_string(
    values: &std::collections::HashMap<String, Value>,
    key: &str,
) -> Option<String> {
    values
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}
