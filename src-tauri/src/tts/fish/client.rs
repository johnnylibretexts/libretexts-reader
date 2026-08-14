use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::tts::fish::{FISH_BASE_URL, FISH_MODEL, FISH_TIMEOUT_SECONDS};
use crate::tts::provider::VoiceSummary;

pub fn map_status(status: u16, body: &str) -> AppError {
    let detail = body.trim();
    let detail = if detail.is_empty() {
        "no detail"
    } else {
        detail
    };

    match status {
        401 => AppError::Auth(format!("Fish Audio rejected the API key: {detail}")),
        402 => AppError::PaymentRequired(format!("Fish Audio account is out of credit: {detail}")),
        404 => AppError::Voice(format!("Fish Audio has no such voice: {detail}")),
        422 => AppError::InvalidInput(format!("Fish Audio rejected the request: {detail}")),
        429 => AppError::RateLimited(format!("Fish Audio is rate limiting: {detail}")),
        other => AppError::Tts(format!("Fish Audio returned {other}: {detail}")),
    }
}

pub fn tts_request_body(text: &str, voice_id: &str, speed: f32) -> Value {
    json!({
        "text": text,
        "reference_id": voice_id,
        "format": "mp3",
        "mp3_bitrate": 128,
        "latency": "normal",
        "prosody": { "speed": speed.clamp(0.5, 2.0) },
    })
}

pub struct FishClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl FishClient {
    pub fn new(api_key: String) -> AppResult<Self> {
        Ok(Self {
            api_key,
            base_url: FISH_BASE_URL.to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(FISH_TIMEOUT_SECONDS))
                .build()?,
        })
    }

    pub async fn synthesize(&self, text: &str, voice_id: &str, speed: f32) -> AppResult<Vec<u8>> {
        let response = self
            .http
            .post(format!("{}/v1/tts", self.base_url))
            .bearer_auth(&self.api_key)
            .header("model", FISH_MODEL)
            .json(&tts_request_body(text, voice_id, speed))
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(map_status(status.as_u16(), &body));
        }

        Ok(response.bytes().await?.to_vec())
    }

    /// Validate the key and read the balance without synthesizing anything.
    ///
    /// A test synthesis would also prove the key works and would bill for it,
    /// on every key entry and re-validation. This costs nothing.
    pub async fn credit(&self) -> AppResult<f64> {
        let response = self
            .http
            .get(format!("{}/wallet/self/api-credit", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(map_status(status.as_u16(), &body));
        }

        let payload: Value = response.json().await?;
        Ok(payload["credit"].as_f64().unwrap_or(0.0))
    }

    pub async fn list_voices(&self) -> AppResult<Vec<VoiceSummary>> {
        let response = self
            .http
            .get(format!("{}/model", self.base_url))
            .query(&[("self", "true"), ("page_size", "100")])
            .bearer_auth(&self.api_key)
            .send()
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(map_status(status.as_u16(), &body));
        }

        let payload: Value = response.json().await?;
        let items = payload["items"].as_array().cloned().unwrap_or_default();
        Ok(items
            .iter()
            .filter_map(|item| {
                Some(VoiceSummary {
                    id: item["_id"].as_str()?.to_string(),
                    name: item["title"]
                        .as_str()
                        .unwrap_or("Untitled voice")
                        .to_string(),
                    ready: item["state"].as_str() == Some("trained"),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_http_status_to_the_right_error_kind() {
        // 401 and 402 are terminal: retrying bills nothing and fixes nothing.
        // 429 is the only retryable one.
        assert_eq!(map_status(401, "bad key").kind(), "auth");
        assert_eq!(map_status(402, "no credit").kind(), "payment_required");
        assert_eq!(map_status(404, "no model").kind(), "voice");
        assert_eq!(map_status(422, "bad field").kind(), "invalid_input");
        assert_eq!(map_status(429, "slow down").kind(), "rate_limited");
        assert_eq!(map_status(500, "boom").kind(), "tts");
    }

    #[test]
    fn only_rate_limiting_is_retryable() {
        assert!(map_status(429, "").retryable());
        assert!(!map_status(401, "").retryable());
        assert!(!map_status(402, "").retryable());
    }

    #[test]
    fn the_error_message_carries_the_response_body() {
        let error = map_status(422, "reference_id must be a string");
        assert!(
            error.to_string().contains("reference_id"),
            "the body is the only thing that says which field was wrong"
        );
    }

    #[test]
    fn request_body_pins_voice_speed_and_mp3_output() {
        let body = tts_request_body("Hello.", "voice-abc", 1.25);

        assert_eq!(body["text"], "Hello.");
        assert_eq!(body["reference_id"], "voice-abc");
        assert_eq!(body["format"], "mp3");
        assert_eq!(body["prosody"]["speed"], 1.25);
    }

    #[test]
    fn speed_is_clamped_to_the_range_fish_accepts() {
        // Fish rejects prosody.speed outside 0.5..=2 with a 422. The player
        // allows a wider range, so clamp rather than send a request that fails.
        assert_eq!(tts_request_body("x", "v", 0.1)["prosody"]["speed"], 0.5);
        assert_eq!(tts_request_body("x", "v", 9.0)["prosody"]["speed"], 2.0);
    }
}
