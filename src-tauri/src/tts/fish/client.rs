use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::tts::fish::{synthesis_timeout, FISH_BASE_URL, FISH_MODEL, FISH_TIMEOUT_SECONDS};
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
        // A key that exists but may not do this: no permission for the model,
        // or an account state that blocks it. Auth rather than the generic arm
        // so the player names it and Settings treats it as a key problem.
        403 => AppError::Auth(format!("Fish Audio refused this API key: {detail}")),
        404 => AppError::Voice(format!("Fish Audio has no such voice: {detail}")),
        422 => AppError::InvalidInput(format!("Fish Audio rejected the request: {detail}")),
        429 => AppError::RateLimited(format!("Fish Audio is rate limiting: {detail}")),
        other => AppError::Tts(format!("Fish Audio returned {other}: {detail}")),
    }
}

/// Read the wallet balance, refusing to guess.
///
/// Zero is a balance a reader can genuinely have, so defaulting to it when the
/// payload cannot be read is indistinguishable from an empty wallet. That
/// number is shown in the export gate at the moment the reader is asked to
/// approve a charge, and it is what `set_fish_api_key` treats as proof the key
/// works -- so a renamed or restyled field would silently store an unvalidated
/// key and tell a funded reader they have nothing.
pub fn parse_credit(payload: &Value) -> AppResult<f64> {
    payload["credit"].as_f64().ok_or_else(|| {
        AppError::Tts(format!(
            "Fish Audio returned a wallet balance we could not read: {payload}"
        ))
    })
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

/// One connection pool for the whole process.
///
/// `reqwest::Client` is a handle around a pool and is documented as
/// create-once-clone-many. Building one per call gave every playback sentence
/// its own pool and a fresh TLS handshake to api.fish.audio -- with the
/// player's ten-sentence lookahead, ten handshakes for what should be one
/// reused connection, adding latency to exactly the window the buffering UI
/// exists to hide.
static HTTP_CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();

pub struct FishClient {
    api_key: String,
    base_url: String,
    http: reqwest::Client,
}

impl std::fmt::Debug for FishClient {
    /// Redacts the key: this type ends up inside `Box<dyn TtsProvider>` for
    /// `unwrap_err`'s `Debug` bound, and a derived impl would put the API key
    /// in any panic message or log that prints the value.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FishClient")
            .field("api_key", &"<redacted>")
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl FishClient {
    pub fn new(api_key: String) -> AppResult<Self> {
        // Cloning a Client shares its pool; it does not copy it. The timeout
        // set here is the control-plane ceiling, which synthesis overrides per
        // request (see `tts_request`).
        let http = match HTTP_CLIENT.get() {
            Some(client) => client.clone(),
            None => {
                let client = reqwest::Client::builder()
                    .timeout(Duration::from_secs(FISH_TIMEOUT_SECONDS))
                    .build()?;
                // A racing thread may win; either way one client is shared.
                HTTP_CLIENT.get_or_init(|| client).clone()
            }
        };

        Ok(Self {
            api_key,
            base_url: FISH_BASE_URL.to_string(),
            http,
        })
    }

    /// The synthesis request, built but not sent.
    ///
    /// Separated from `synthesize` so a test can inspect the headers the
    /// request actually carries -- Fish picks the model from the `model`
    /// header and silently falls back to a different one when it is missing
    /// or unrecognised, and that model is hashed into the audio cache key, so
    /// a wrong header would pin cached audio to a model nobody chose and
    /// every later run would hit that cache and never reveal it. Asserting
    /// `FISH_MODEL == "s2.1-pro"` proves nothing about the request; building
    /// it and reading the header back does.
    fn tts_request(&self, text: &str, voice_id: &str, speed: f32) -> reqwest::RequestBuilder {
        self.http
            .post(format!("{}/v1/tts", self.base_url))
            .bearer_auth(&self.api_key)
            .header("model", FISH_MODEL)
            .timeout(synthesis_timeout(text.len()))
            .json(&tts_request_body(text, voice_id, speed))
    }

    pub async fn synthesize(&self, text: &str, voice_id: &str, speed: f32) -> AppResult<Vec<u8>> {
        let response = self.tts_request(text, voice_id, speed).send().await?;

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
        parse_credit(&payload)
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
        // 403 is Fish refusing a key that exists -- no permission for the
        // requested model, or an account state that blocks it. Falling through
        // to the generic arm showed the reader a raw backend string and stopped
        // the Settings panel presenting it as the key problem it is.
        assert_eq!(map_status(403, "forbidden").kind(), "auth");
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
    fn the_synthesis_request_carries_the_paid_model_in_its_header() {
        // Built, never sent: no network, no key required beyond a placeholder.
        // This replaces an assertion that FISH_MODEL equalled its own literal,
        // which could only fail if someone edited both lines at once.
        let client = FishClient::new("sk-not-a-real-key".into()).expect("client");
        let request = client
            .tts_request("Hello.", "voice-abc", 1.0)
            .build()
            .expect("the request must build");

        assert_eq!(
            request
                .headers()
                .get("model")
                .expect("the model header selects the engine and must be present"),
            FISH_MODEL
        );
        assert_eq!(request.url().path(), "/v1/tts");
    }

    #[test]
    fn fish_clients_share_one_pooled_http_client() {
        // Playback builds a client per sentence through provider_for. Sharing
        // the pool is the difference between one TLS handshake per session and
        // one per sentence.
        let _first = FishClient::new("sk-not-a-real-key".into()).expect("first client");
        let _second = FishClient::new("sk-also-not-real".into()).expect("second client");

        assert!(
            HTTP_CLIENT.get().is_some(),
            "FishClient must build through the shared pool, not its own builder"
        );
    }

    #[test]
    fn a_balance_is_read_from_the_wallet_payload() {
        assert_eq!(
            parse_credit(&json!({ "credit": 42.5 })).expect("credit"),
            42.5
        );
    }

    #[test]
    fn a_genuinely_empty_wallet_reads_as_zero() {
        // Zero must stay a readable balance, or the check below would reject
        // the one account state it most needs to report accurately.
        assert_eq!(parse_credit(&json!({ "credit": 0 })).expect("credit"), 0.0);
    }

    #[test]
    fn an_unreadable_wallet_payload_fails_rather_than_reporting_zero() {
        // Zero is a balance a reader can really have, so returning it for a
        // payload we could not read is indistinguishable from an empty wallet.
        // This value is shown in the export gate at the moment the reader is
        // asked to approve a charge, and it is also what `set_fish_api_key`
        // treats as proof the key works.
        for payload in [
            json!({ "balance": 42.5 }),   // renamed field
            json!({ "credit": "42.50" }), // sent as a string
            json!({ "credit": null }),
            json!({}),
        ] {
            let error = parse_credit(&payload).expect_err("must not invent a balance");
            assert_eq!(error.kind(), "tts", "payload was {payload}");
        }
    }

    #[test]
    fn a_chapter_sized_request_carries_its_own_ceiling() {
        // The client-level timeout is reqwest's *total* request budget, body
        // download included. Applied to a whole-chapter synthesis it aborts the
        // request after Fish has already synthesized and billed for it, so the
        // reader pays and gets nothing -- on every retry. Synthesis must
        // override it per request.
        let client = FishClient::new("sk-not-a-real-key".into()).expect("client");
        let chapter = "word ".repeat(8_000);
        let request = client
            .tts_request(&chapter, "voice-abc", 1.0)
            .build()
            .expect("the request must build");

        let timeout = request
            .timeout()
            .copied()
            .expect("synthesis must set its own ceiling, not inherit the control-plane one");
        assert!(
            timeout > Duration::from_secs(FISH_TIMEOUT_SECONDS),
            "a chapter cannot finish inside the control-plane ceiling, got {timeout:?}"
        );
    }

    #[test]
    fn speed_is_clamped_to_the_range_fish_accepts() {
        // Fish rejects prosody.speed outside 0.5..=2 with a 422. The player
        // allows a wider range, so clamp rather than send a request that fails.
        assert_eq!(tts_request_body("x", "v", 0.1)["prosody"]["speed"], 0.5);
        assert_eq!(tts_request_body("x", "v", 9.0)["prosody"]["speed"], 2.0);
    }
}
