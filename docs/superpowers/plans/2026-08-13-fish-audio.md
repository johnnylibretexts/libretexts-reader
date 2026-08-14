# Fish Audio Provider Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Fish Audio as an optional bring-your-own-API-key TTS provider for both live playback and chapter MP3 export, with Supertonic remaining the default and the only engine that works offline.

**Architecture:** Fish synthesis runs in Rust behind a narrow `TtsProvider` trait, so the API key never enters the webview. The webview remains the single place a provider is chosen and passes that choice explicitly on every request; Rust dispatches on the parameter and never re-reads `tts_provider`. Caching, estimates, progress and the confirmation gate stay outside the trait in shared export code.

**Tech Stack:** Rust (Tauri 2, `reqwest`, `keyring`, `async-trait`), React 19 + Zustand + TypeScript, vitest.

**Spec:** `docs/superpowers/specs/2026-08-13-fish-audio-design.md`

## Global Constraints

- **Node 22.x** for every JS command (`source "$HOME/.nvm/nvm.sh" && nvm use 22.20.0`). Node 24 hangs on Vite/Rollup.
- **Commit messages must NOT contain a `Claude-Session:` trailer.** A global `commit-msg` hook rejects any message containing an AI conversation link. Never bypass with `--no-verify`.
- **Never call `paths::` helpers from a test.** They `create_dir_all` on resolve. Pass directories in explicitly, as `cache_path_in` and `cleanup::reclaim_in` do. `scripts/ci/check-app-data-isolation.sh` fails the build otherwise.
- **Never touch the real OS keychain from a test.** Same class of bug: use the `SecretStore` trait's in-memory implementation (Task 1). A test that writes to the developer's login keychain is indistinguishable from real usage.
- **No test that runs by default may call `api.fish.audio`.** It needs a real key and bills whoever runs the suite. Live tests must be `#[ignore]`d.
- **Every new `AppError` variant must be mirrored in `src/types/domain.ts`** or `scripts/ci/check-error-kinds.sh` fails CI.
- **The Fish model string is one named constant**, used by both the request header and the cache key. Never a literal at a call site.
- **Do not add `api.fish.audio` to the CSP.** Rust's HTTP client is not subject to the webview CSP; adding it widens the webview's reach for no benefit.
- **Full gate before every commit:** `npm run build`, `npm test`, `cargo test -p libretexts-reader`, `cargo clippy -p libretexts-reader --all-targets -- -D warnings`, `cargo fmt --check`, `scripts/ci/check-identifier.sh`, `scripts/ci/check-error-kinds.sh`, `scripts/ci/check-app-data-isolation.sh`, `git diff --check`.

## File Structure

**Create (Rust)**
- `src-tauri/src/secrets.rs` — `SecretStore` trait, keyring and in-memory implementations. One responsibility: storing one named secret.
- `src-tauri/src/tts/provider.rs` — the `TtsProvider` trait and `VoiceSummary`.
- `src-tauri/src/tts/fish/mod.rs` — module root, public constants.
- `src-tauri/src/tts/fish/client.rs` — HTTP shaping and status→error mapping. No knowledge of Tauri or settings.
- `src-tauri/src/tts/fish/provider.rs` — `FishProvider`, implements `TtsProvider`.
- `src-tauri/src/tts/supertonic/provider.rs` — `SupertonicProvider`, implements `TtsProvider` over the existing engine.
- `src-tauri/src/commands/fish.rs` — key set/clear/status and voice listing commands.

**Rename (Rust)**
- `src-tauri/src/commands/supertonic_tts.rs` → `src-tauri/src/commands/chapter_tts.rs`. It becomes the shared, provider-agnostic export path; keeping a provider's name on it would mislead.

**Create (frontend)**
- `src/lib/speech/fishEngine.ts` — a thin `SpeechEngine` that only invokes Rust.
- `src/components/Settings/FishAudioSettings.tsx` — key entry, validation state, voice picker.

**Modify**
- `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/src/error.rs`, `src-tauri/src/db/settings.rs`, `src-tauri/src/tts/mod.rs`, `src-tauri/src/tts/supertonic/cache.rs`, `src-tauri/src/commands/tts.rs`, `src-tauri/src/commands/mod.rs`
- `src/types/domain.ts`, `src/lib/tauri.ts`, `src/lib/speech/index.ts`, `src/lib/speech/types.ts`, `src/stores/settings.ts`, `src/components/Settings/SettingsPanel.tsx`, `src/components/Reader/SupertonicChapterExport.tsx`
- `CLAUDE.md`, `README.md`, `HANDOFF.md`

**No new SQL migration.** Settings defaults are seeded in code by `seed_default_settings` with `INSERT OR IGNORE`; adding `fish_voice_id` to `default_settings()` is sufficient. Do not create `0008` for this.

---

### Task 1: Secret storage with a testable seam

**Files:**
- Create: `src-tauri/src/secrets.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `SecretStore` trait with `set(&self, secret: &str) -> AppResult<()>`, `get(&self) -> AppResult<Option<String>>`, `clear(&self) -> AppResult<()>`; `KeyringSecretStore::new(account: &str)`; `MemorySecretStore::default()`; `const FISH_KEY_ACCOUNT: &str = "fish-audio-api-key"`; `const KEYCHAIN_SERVICE: &str = "dev.johnnylibretexts.reader"`.

- [ ] **Step 1: Add the dependency**

```bash
cd src-tauri && cargo add keyring@3 --features apple-native,windows-native,sync-secret-service
cargo add async-trait@0.1
```

Confirm the emitted `Cargo.toml` lists those features; `keyring` v3 requires an explicit platform backend feature or every call fails at runtime with "no credential store".

- [ ] **Step 2: Write the failing test**

Create `src-tauri/src/secrets.rs` containing only the test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // MemorySecretStore, never KeyringSecretStore: a test that writes to the
    // developer's login keychain is indistinguishable from real usage, the
    // same failure mode as the app-data leak in issue #2.
    #[test]
    fn stores_reads_and_clears_a_secret() {
        let store = MemorySecretStore::default();
        assert_eq!(store.get().expect("read empty"), None);

        store.set("sk-test-123").expect("set");
        assert_eq!(store.get().expect("read"), Some("sk-test-123".to_string()));

        store.clear().expect("clear");
        assert_eq!(store.get().expect("read cleared"), None);
    }

    #[test]
    fn clearing_an_absent_secret_is_not_an_error() {
        let store = MemorySecretStore::default();
        store.clear().expect("clearing nothing must succeed");
    }
}
```

- [ ] **Step 3: Run it to make sure it fails**

Run: `cargo test -p libretexts-reader secrets::`
Expected: FAIL to compile — `MemorySecretStore` not found.

- [ ] **Step 4: Write the implementation**

Above the test module in `src-tauri/src/secrets.rs`:

```rust
//! Storage for the one secret this app holds: a Fish Audio API key.
//!
//! A trait rather than direct `keyring` calls so tests never touch the real
//! login keychain -- the same reason `cache_path_in` takes a root instead of
//! calling `paths::cache_dir()`.

use std::sync::Mutex;

use crate::error::{AppError, AppResult};

pub const KEYCHAIN_SERVICE: &str = "dev.johnnylibretexts.reader";
pub const FISH_KEY_ACCOUNT: &str = "fish-audio-api-key";

pub trait SecretStore: Send + Sync {
    fn set(&self, secret: &str) -> AppResult<()>;
    fn get(&self) -> AppResult<Option<String>>;
    fn clear(&self) -> AppResult<()>;
}

pub struct KeyringSecretStore {
    account: String,
}

impl KeyringSecretStore {
    pub fn new(account: &str) -> Self {
        Self { account: account.to_string() }
    }

    fn entry(&self) -> AppResult<keyring::Entry> {
        keyring::Entry::new(KEYCHAIN_SERVICE, &self.account)
            .map_err(|error| AppError::Auth(format!("cannot open the system keychain: {error}")))
    }
}

impl SecretStore for KeyringSecretStore {
    fn set(&self, secret: &str) -> AppResult<()> {
        self.entry()?
            .set_password(secret)
            .map_err(|error| AppError::Auth(format!("cannot store the key: {error}")))
    }

    fn get(&self) -> AppResult<Option<String>> {
        match self.entry()?.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(AppError::Auth(format!("cannot read the key: {error}"))),
        }
    }

    fn clear(&self) -> AppResult<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(AppError::Auth(format!("cannot clear the key: {error}"))),
        }
    }
}

#[derive(Default)]
pub struct MemorySecretStore {
    secret: Mutex<Option<String>>,
}

impl SecretStore for MemorySecretStore {
    fn set(&self, secret: &str) -> AppResult<()> {
        *self.secret.lock().expect("secret lock") = Some(secret.to_string());
        Ok(())
    }

    fn get(&self) -> AppResult<Option<String>> {
        Ok(self.secret.lock().expect("secret lock").clone())
    }

    fn clear(&self) -> AppResult<()> {
        *self.secret.lock().expect("secret lock") = None;
        Ok(())
    }
}
```

Add `mod secrets;` to `src-tauri/src/lib.rs` beside the other module declarations.

- [ ] **Step 5: Add the `Auth` error variant**

In `src-tauri/src/error.rs`, add to the `AppError` enum after `Voice`:

```rust
    #[error("authentication error: {0}")]
    Auth(String),
```

Add its arm to `AppError::kind` returning `"auth"`, and to the `retryable` logic returning `false`. Then add `"auth"` to the `AppErrorKind` union in `src/types/domain.ts`.

- [ ] **Step 6: Run the tests and the error-kind gate**

Run: `cargo test -p libretexts-reader secrets::` — Expected: 2 passed.
Run: `scripts/ci/check-error-kinds.sh` — Expected: `error kinds OK: 18 kinds match`.

- [ ] **Step 7: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock src-tauri/src/secrets.rs src-tauri/src/lib.rs src-tauri/src/error.rs src/types/domain.ts
git commit -m "feat: add keychain-backed secret storage behind a testable trait"
```

---

### Task 2: Fish HTTP client — request shaping and error mapping

**Files:**
- Create: `src-tauri/src/tts/fish/mod.rs`, `src-tauri/src/tts/fish/client.rs`, `src-tauri/src/tts/provider.rs`
- Modify: `src-tauri/src/tts/mod.rs`, `src-tauri/src/error.rs`, `src/types/domain.ts`

**Interfaces:**
- Consumes: `AppError` from Task 1.
- Produces: `struct VoiceSummary { id: String, name: String, ready: bool }`; `const FISH_MODEL: &str = "s2.1-pro"`; `const FISH_BASE_URL: &str = "https://api.fish.audio"`; `fn map_status(status: u16, body: &str) -> AppError`; `fn tts_request_body(text: &str, voice_id: &str, speed: f32) -> serde_json::Value`; `struct FishClient` with `new(api_key: String)`, `async fn synthesize(&self, text: &str, voice_id: &str, speed: f32) -> AppResult<Vec<u8>>`, `async fn credit(&self) -> AppResult<f64>`, `async fn list_voices(&self) -> AppResult<Vec<VoiceSummary>>`.

**Note on ordering:** `VoiceSummary` is created here, not in Task 3, because `FishClient::list_voices` returns it and this task lands first. Task 3 adds the `TtsProvider` trait to the same file.

- [ ] **Step 1: Add the remaining error variants**

In `src-tauri/src/error.rs`:

```rust
    #[error("payment required: {0}")]
    PaymentRequired(String),

    #[error("rate limited: {0}")]
    RateLimited(String),
```

`kind` returns `"payment_required"` and `"rate_limited"`. `retryable` is `false` for `PaymentRequired` and **`true`** for `RateLimited`. Mirror both in `AppErrorKind` in `src/types/domain.ts`.

- [ ] **Step 2: Write the failing tests**

Create `src-tauri/src/tts/fish/client.rs` with only:

```rust
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
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p libretexts-reader fish::client`
Expected: FAIL to compile — `map_status` and `tts_request_body` not found.

- [ ] **Step 4: Write the implementation**

Create `src-tauri/src/tts/fish/mod.rs`:

```rust
//! Fish Audio: an optional cloud TTS provider the user supplies a key for.
//!
//! Runs in Rust rather than the webview so the API key never crosses into it,
//! and because chapter export already lives here.

pub mod client;
pub mod provider;

/// The model Fish should use, as one constant.
///
/// Fish selects the model from an HTTP header and falls back to `s2.1-pro`
/// when it is missing or unrecognised. This value is also hashed into the
/// audio cache key, so a literal at a call site could silently pin cached
/// audio to a model we never meant to use.
pub const FISH_MODEL: &str = "s2.1-pro";

pub const FISH_BASE_URL: &str = "https://api.fish.audio";

/// Fish publishes no latency SLA, so every call needs a ceiling.
pub const FISH_TIMEOUT_SECONDS: u64 = 20;
```

Create `src-tauri/src/tts/provider.rs` holding only the shared type for now:

```rust
//! The seam between "some engine speaks" and "which engine speaks".
//! Task 3 adds the `TtsProvider` trait here.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceSummary {
    pub id: String,
    pub name: String,
    pub ready: bool,
}
```

Add `pub mod fish;` and `pub mod provider;` to `src-tauri/src/tts/mod.rs`.

In `client.rs`, above the tests:

```rust
use std::time::Duration;

use serde_json::{json, Value};

use crate::error::{AppError, AppResult};
use crate::tts::fish::{FISH_BASE_URL, FISH_MODEL, FISH_TIMEOUT_SECONDS};
use crate::tts::provider::VoiceSummary;

pub fn map_status(status: u16, body: &str) -> AppError {
    let detail = body.trim();
    let detail = if detail.is_empty() { "no detail" } else { detail };

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

    /// Point the client at a local fixture server. Test-only.
    #[cfg(test)]
    pub fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.to_string();
        self
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
                    name: item["title"].as_str().unwrap_or("Untitled voice").to_string(),
                    ready: item["state"].as_str() == Some("trained"),
                })
            })
            .collect())
    }
}
```

- [ ] **Step 5: Run the tests**

Run: `cargo test -p libretexts-reader fish::client` — Expected: 5 passed.
Run: `scripts/ci/check-error-kinds.sh` — Expected: `20 kinds match`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/tts/fish src-tauri/src/tts/mod.rs src-tauri/src/error.rs src/types/domain.ts
git commit -m "feat: add a Fish Audio HTTP client with explicit error mapping"
```

---

### Task 3: The `TtsProvider` trait and both implementations

**Files:**
- Create: `src-tauri/src/tts/provider.rs`, `src-tauri/src/tts/supertonic/provider.rs`, `src-tauri/src/tts/fish/provider.rs`
- Modify: `src-tauri/src/tts/mod.rs`, `src-tauri/src/tts/supertonic/mod.rs`

**Interfaces:**
- Consumes: `FishClient` (Task 2), `SecretStore` (Task 1).
- Produces: `trait TtsProvider`; `struct VoiceSummary { id: String, name: String, ready: bool }`; `SupertonicProvider::new(voice_style: String, language: String)`; `FishProvider::new(client: FishClient, voice_id: Option<String>)`.

- [ ] **Step 1: Write the failing test**

Create `src-tauri/src/tts/provider.rs` with only:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct StubProvider;

    #[async_trait::async_trait]
    impl TtsProvider for StubProvider {
        fn id(&self) -> &'static str {
            "stub"
        }
        async fn synthesize(&self, text: &str, _voice: &str, _language: &str) -> AppResult<Vec<u8>> {
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
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p libretexts-reader tts::provider`
Expected: FAIL to compile — `TtsProvider` not found.

- [ ] **Step 3: Write the trait**

Add to the existing `provider.rs` (created in Task 2, which already defines `VoiceSummary`), above the tests:

```rust
//! Deliberately narrow. Caching, progress events, estimates and the export
//! confirmation live outside it, in shared command code: Supertonic reports
//! chunk-level progress and Fish returns a whole utterance, and a trait that
//! tried to model both notions of progress would leak one into the other.
//!
//! Mirrors `SpeechEngine` in `src/lib/speech/types.ts`. Two layers, one idea.

use crate::error::AppResult;

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
```

- [ ] **Step 4: Run the test**

Run: `cargo test -p libretexts-reader tts::provider` — Expected: 1 passed.

- [ ] **Step 5: Widen three visibilities**

`SupertonicProvider` needs three items that are currently private. Change exactly these, and nothing else:

- `src-tauri/src/commands/supertonic_tts.rs:334` — `async fn synthesize_supertonic_mp3` → `pub(crate) async fn synthesize_supertonic_mp3`
- `src-tauri/src/tts/supertonic/voice.rs:16` — `const SUPERTONIC_VOICE_STYLES` → `pub(crate) const SUPERTONIC_VOICE_STYLES`
- `src-tauri/src/tts/supertonic/model.rs:51` is already `pub(crate)`. Note it is **synchronous** — do not `.await` it.

- [ ] **Step 6: Write `SupertonicProvider`**

Create `src-tauri/src/tts/supertonic/provider.rs`. The module is still named `supertonic_tts` at this point; Task 5 renames it and updates this import.

```rust
use crate::commands::supertonic_tts::synthesize_supertonic_mp3;
use crate::error::AppResult;
use crate::tts::provider::{TtsProvider, VoiceSummary};
use crate::tts::supertonic::model::supertonic_model_status;
use crate::tts::supertonic::voice::SUPERTONIC_VOICE_STYLES;
use crate::tts::supertonic::SUPERTONIC_DEFAULT_SPEED;

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
```

Add `pub mod provider;` to `src-tauri/src/tts/supertonic/mod.rs`.

- [ ] **Step 7: Write `FishProvider`**

Create `src-tauri/src/tts/fish/provider.rs`:

```rust
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
```

- [ ] **Step 8: Run the full Rust suite**

Run: `cargo test -p libretexts-reader && cargo clippy -p libretexts-reader --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src
git commit -m "feat: add a TtsProvider trait with Supertonic and Fish implementations"
```

---

### Task 4: Cache keys must distinguish providers

**Files:**
- Modify: `src-tauri/src/tts/supertonic/cache.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `cache_path_in(cache_root: &Path, provider: &str, model: &str, material: &ChapterMaterial, voice_style: &str, language: &str) -> PathBuf` — note the two new leading parameters.

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `cache.rs`:

```rust
    #[test]
    fn cache_path_distinguishes_providers_and_models() {
        // Without this, a chapter exported with Supertonic would be served
        // from cache for a Fish request -- identical text, voice and language,
        // identical key -- and the user would silently get the wrong voice.
        let root = Path::new("/nonexistent/cache");
        let supertonic = cache_path_in(root, "supertonic", "v1", &material("Hello."), "M1", "en");
        let fish = cache_path_in(root, "fish", "s2.1-pro", &material("Hello."), "M1", "en");
        let other_model = cache_path_in(root, "fish", "s2-pro", &material("Hello."), "M1", "en");

        assert_ne!(supertonic, fish, "provider must change the path");
        assert_ne!(fish, other_model, "model must change the path");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p libretexts-reader cache_path`
Expected: FAIL to compile — `cache_path_in` takes 4 arguments, 6 supplied.

- [ ] **Step 3: Change the signature and hash inputs**

In `cache_path_in`, add `provider: &str` and `model: &str` as the second and third parameters, and hash them immediately after the version constants:

```rust
    hasher.update(provider.as_bytes());
    hasher.update(model.as_bytes());
```

Bump the cache version so existing entries cannot collide with the new scheme:

```rust
pub(crate) const SUPERTONIC_CACHE_VERSION: &str = "tts-cache-v2";
```

Update `cache_path_for_chapter` to take and forward the same two parameters, and update the existing `cache_path_is_content_addressed` test's closure to pass `"supertonic", "v1"`.

- [ ] **Step 4: Run the tests**

Run: `cargo test -p libretexts-reader cache` — Expected: all pass, including the pre-existing content-addressing test.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/tts/supertonic/cache.rs
git commit -m "fix: key the audio cache on provider and model"
```

---

### Task 5: Rename the export command module and dispatch by parameter

**Files:**
- Rename: `src-tauri/src/commands/supertonic_tts.rs` → `src-tauri/src/commands/chapter_tts.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/commands/tts.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/tts/supertonic/mod.rs`

**Interfaces:**
- Consumes: `TtsProvider` (Task 3), `cache_path_in` (Task 4).
- Produces: `fn provider_for(name: &str, settings: &ProviderSettings) -> AppResult<Box<dyn TtsProvider>>`; `SynthesizeSpeechRequest` gains `pub provider: String`; `ChapterRequest` (renamed from `SupertonicChapterRequest`) gains `pub provider: String`.

**The rule this task exists to preserve:** `commands/tts.rs` deliberately stopped reading `tts_provider` from settings. Its doc comment records why — the webview already decides, and deciding a second time from a different source with no ordering guarantee is the bug that was removed. So dispatch happens on a **request field**, never on a settings read. Do not reintroduce a settings lookup here.

- [ ] **Step 1: Rename the file and its references**

```bash
git mv src-tauri/src/commands/supertonic_tts.rs src-tauri/src/commands/chapter_tts.rs
```

In `src-tauri/src/commands/mod.rs`, change `pub mod supertonic_tts;` to `pub mod chapter_tts;`. Update the five `commands::supertonic_tts::` entries in `src-tauri/src/lib.rs`'s `generate_handler!` to `commands::chapter_tts::`, and the import in `commands/tts.rs`.

Rename `SupertonicChapterRequest` → `ChapterRequest`, `SupertonicChapterEstimate` → `ChapterEstimate`, `SupertonicChapterExport` → `ChapterExport` in `src-tauri/src/tts/supertonic/mod.rs` and every reference. Keep the Tauri command names (`estimate_supertonic_chapter`, `export_supertonic_chapter_mp3`) unchanged in this task — renaming those is a frontend-visible change handled in Task 7.

- [ ] **Step 2: Run to confirm the rename compiles**

Run: `cargo test -p libretexts-reader`
Expected: all pass. This step changes no behaviour.

- [ ] **Step 3: Commit the rename on its own**

```bash
git add -A src-tauri/src
git commit -m "refactor: rename the chapter export module off the provider name"
```

- [ ] **Step 4: Write the failing dispatch test**

Add to `chapter_tts.rs`:

```rust
#[cfg(test)]
mod dispatch_tests {
    use super::*;

    #[test]
    fn dispatch_reads_the_request_not_the_settings_table() {
        let settings = ProviderSettings {
            supertonic_voice_style: "M1".into(),
            supertonic_language: "en".into(),
            fish_voice_id: None,
            fish_api_key: Some("sk-test".into()),
        };

        assert_eq!(provider_for("supertonic", &settings).unwrap().id(), "supertonic");
        assert_eq!(provider_for("fish", &settings).unwrap().id(), "fish");
    }

    #[test]
    fn an_unknown_provider_is_rejected_rather_than_defaulted() {
        // Falling back to a default would let a frontend bug silently switch
        // engines, which is the class of bug the single-decision-point rule
        // in commands/tts.rs exists to prevent.
        let settings = ProviderSettings {
            supertonic_voice_style: "M1".into(),
            supertonic_language: "en".into(),
            fish_voice_id: None,
            fish_api_key: None,
        };
        assert!(provider_for("kokoro", &settings).is_err());
    }

    #[test]
    fn fish_without_a_key_is_an_auth_error_not_a_panic() {
        let settings = ProviderSettings {
            supertonic_voice_style: "M1".into(),
            supertonic_language: "en".into(),
            fish_voice_id: Some("voice-1".into()),
            fish_api_key: None,
        };
        assert_eq!(provider_for("fish", &settings).unwrap_err().kind(), "auth");
    }
}
```

- [ ] **Step 5: Run to verify it fails**

Run: `cargo test -p libretexts-reader dispatch_tests`
Expected: FAIL to compile — `ProviderSettings` and `provider_for` not found.

- [ ] **Step 6: Implement the dispatcher**

In `chapter_tts.rs`:

```rust
/// Everything a provider needs, read once by the caller.
///
/// A struct rather than a `DbPool` so `provider_for` is pure and testable and
/// cannot reach the database or the keychain itself.
pub struct ProviderSettings {
    pub supertonic_voice_style: String,
    pub supertonic_language: String,
    pub fish_voice_id: Option<String>,
    pub fish_api_key: Option<String>,
}

/// Build the provider the *caller* named.
///
/// `name` comes from the request, never from the `tts_provider` setting. See
/// the doc comment on `commands::tts::synthesize_speech`: the webview is the
/// single place an engine is chosen, and re-deriving that here from a second
/// source with no ordering guarantee is the bug that was removed.
pub fn provider_for(name: &str, settings: &ProviderSettings) -> AppResult<Box<dyn TtsProvider>> {
    match name {
        "supertonic" => Ok(Box::new(SupertonicProvider)),
        "fish" => {
            let key = settings.fish_api_key.clone().ok_or_else(|| {
                AppError::Auth("Add a Fish Audio API key in Settings first.".into())
            })?;
            Ok(Box::new(FishProvider::new(
                FishClient::new(key)?,
                settings.fish_voice_id.clone(),
            )))
        }
        other => Err(AppError::InvalidInput(format!("unknown TTS provider: {other}"))),
    }
}
```

Add `pub provider: String` to `SynthesizeSpeechRequest` in `commands/tts.rs` and to `ChapterRequest`, and route both commands through `provider_for`. Read `fish_api_key` via `KeyringSecretStore::new(FISH_KEY_ACCOUNT)` in the command, not in `provider_for`.

- [ ] **Step 7: Run the tests**

Run: `cargo test -p libretexts-reader && cargo clippy -p libretexts-reader --all-targets -- -D warnings`
Expected: all pass, no warnings.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src
git commit -m "feat: dispatch TTS on the requested provider, never on stored settings"
```

---

### Task 6: Key management and voice listing commands

**Files:**
- Create: `src-tauri/src/commands/fish.rs`
- Modify: `src-tauri/src/commands/mod.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/db/settings.rs`

**Interfaces:**
- Consumes: `SecretStore`, `FishClient`.
- Produces: Tauri commands `set_fish_api_key(key: String) -> FishKeyStatus`, `clear_fish_api_key() -> ()`, `get_fish_key_status() -> FishKeyStatus`, `list_fish_voices() -> Vec<VoiceSummary>`; `struct FishKeyStatus { present: bool, valid: Option<bool>, credit: Option<f64> }`.

- [ ] **Step 1: Add the `fish_voice_id` default setting**

In `src-tauri/src/db/settings.rs`, add to `default_settings()`:

```rust
        ("fish_voice_id", json!(null)),
```

No SQL migration: `seed_default_settings` uses `INSERT OR IGNORE`, so existing databases pick this up on next open.

- [ ] **Step 2: Write the failing test**

In `src-tauri/src/commands/fish.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecretStore;

    #[test]
    fn status_reports_absence_without_touching_the_network() {
        let store = MemorySecretStore::default();
        assert_eq!(key_status_from(&store).unwrap().present, false);
    }

    #[test]
    fn status_reports_presence_without_revealing_the_key() {
        let store = MemorySecretStore::default();
        store.set("sk-secret").expect("set");

        let status = key_status_from(&store).unwrap();
        assert!(status.present);

        // The whole point: serialising the status must not leak the key.
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(!json.contains("sk-secret"), "the key must never reach the frontend");
    }
}
```

- [ ] **Step 3: Run to verify it fails**

Run: `cargo test -p libretexts-reader commands::fish`
Expected: FAIL to compile — `key_status_from` not found.

- [ ] **Step 4: Implement**

```rust
//! Fish Audio key management.
//!
//! There is deliberately no command that returns the key. The frontend can
//! learn that a key is present and whether it validated; a getter would put
//! the secret into the webview and into any devtools session, which is the
//! one thing the keychain choice exists to prevent.

use serde::Serialize;

use crate::error::AppResult;
use crate::secrets::{KeyringSecretStore, SecretStore, FISH_KEY_ACCOUNT};
use crate::tts::fish::client::FishClient;
use crate::tts::provider::VoiceSummary;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FishKeyStatus {
    pub present: bool,
    pub valid: Option<bool>,
    pub credit: Option<f64>,
}

/// Presence only -- no network call, so the Settings panel can render
/// immediately on open without waiting on Fish.
pub fn key_status_from(store: &dyn SecretStore) -> AppResult<FishKeyStatus> {
    Ok(FishKeyStatus {
        present: store.get()?.is_some(),
        valid: None,
        credit: None,
    })
}

#[tauri::command]
pub async fn get_fish_key_status() -> AppResult<FishKeyStatus> {
    key_status_from(&KeyringSecretStore::new(FISH_KEY_ACCOUNT))
}

/// Validate before storing, so an invalid key is never persisted.
///
/// Validation is a wallet read, not a test synthesis: both prove the key
/// works, but only one of them bills the user for every key entry.
#[tauri::command]
pub async fn set_fish_api_key(key: String) -> AppResult<FishKeyStatus> {
    let key = key.trim().to_string();
    if key.is_empty() {
        return Err(crate::error::AppError::InvalidInput("The API key is empty.".into()));
    }

    let credit = FishClient::new(key.clone())?.credit().await?;
    KeyringSecretStore::new(FISH_KEY_ACCOUNT).set(&key)?;

    Ok(FishKeyStatus { present: true, valid: Some(true), credit: Some(credit) })
}

#[tauri::command]
pub async fn clear_fish_api_key() -> AppResult<()> {
    KeyringSecretStore::new(FISH_KEY_ACCOUNT).clear()
}

#[tauri::command]
pub async fn list_fish_voices() -> AppResult<Vec<VoiceSummary>> {
    let key = KeyringSecretStore::new(FISH_KEY_ACCOUNT)
        .get()?
        .ok_or_else(|| crate::error::AppError::Auth("Add a Fish Audio API key first.".into()))?;
    FishClient::new(key)?.list_voices().await
}
```

Add `pub mod fish;` to `commands/mod.rs` and register all four commands in `generate_handler!`.

- [ ] **Step 5: Run the tests and the whole gate**

Run: `cargo test -p libretexts-reader && cargo clippy -p libretexts-reader --all-targets -- -D warnings && scripts/ci/check-app-data-isolation.sh`
Expected: all pass; isolation reports nothing created under `$HOME`.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src
git commit -m "feat: add Fish Audio key management commands with no getter"
```

---

### Task 7: Frontend engine, types and invoke wrappers

**Files:**
- Create: `src/lib/speech/fishEngine.ts`
- Modify: `src/types/domain.ts`, `src/lib/tauri.ts`, `src/lib/speech/types.ts`, `src/lib/speech/index.ts`, `src/stores/settings.ts`
- Test: `src/lib/speech/index.test.ts` (create)

**Interfaces:**
- Consumes: the four commands from Task 6, plus `synthesize_speech` now requiring `provider`.
- Produces: `createFishEngine(options: { voiceId: string | null }): SpeechEngine`; `SpeechEngineId` becomes `"supertonic" | "fish"`.

- [ ] **Step 1: Write the failing test**

Create `src/lib/speech/index.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { createSpeechEngine } from "./index";

describe("createSpeechEngine", () => {
  it("returns the Supertonic engine when the provider says supertonic", () => {
    const engine = createSpeechEngine({
      ttsProvider: "supertonic",
      supertonicLanguage: "en",
      fishVoiceId: null,
    });
    expect(engine.id).toBe("supertonic");
  });

  it("returns the Fish engine when the provider says fish", () => {
    const engine = createSpeechEngine({
      ttsProvider: "fish",
      supertonicLanguage: "en",
      fishVoiceId: "voice-abc",
    });
    expect(engine.id).toBe("fish");
  });
});
```

- [ ] **Step 2: Run to verify it fails**

Run: `npm test -- src/lib/speech/index.test.ts`
Expected: FAIL — `"fish"` is not assignable to `SpeechEngineId`.

- [ ] **Step 3: Widen the types**

In `src/lib/speech/types.ts`: `export type SpeechEngineId = "supertonic" | "fish";`

In `src/types/domain.ts`: widen `TtsProvider` to `"supertonic" | "fish"`.

In `src/lib/tauri.ts`, add `provider: TtsProvider` to `SynthesizeSpeechRequest` and `SupertonicChapterRequest`, and add wrappers:

```ts
  getFishKeyStatus: () => invokeDesktop<FishKeyStatus>("get_fish_key_status"),
  setFishApiKey: (key: string) => invokeDesktop<FishKeyStatus>("set_fish_api_key", { key }),
  clearFishApiKey: () => invokeDesktop<void>("clear_fish_api_key"),
  listFishVoices: () => invokeDesktop<SpeechVoice[]>("list_fish_voices"),
```

with `export interface FishKeyStatus { present: boolean; valid: boolean | null; credit: number | null; }`.

- [ ] **Step 4: Write the Fish engine**

Create `src/lib/speech/fishEngine.ts`:

```ts
import { api } from "../tauri";
import { throwIfAborted, type SpeechEngine } from "./types";
import { speechAudioToBlob } from "./supertonicEngine";

/**
 * Fish Audio runs entirely in Rust, so this adapter only invokes. The API key
 * is never handed to the webview and so never appears here.
 */
export function createFishEngine(options: { voiceId: string | null }): SpeechEngine {
  return {
    id: "fish",
    // Fish has no built-in default voice; an unset one is an error the user
    // can act on, raised by Rust rather than guessed at here.
    defaultVoice: options.voiceId ?? "",

    async synthesize(request, signal) {
      throwIfAborted(signal);
      const speech = await api.synthesizeSpeech({
        provider: "fish",
        text: request.text,
        speed: request.speed,
        voiceId: request.voice || options.voiceId || "",
        language: null,
      });
      throwIfAborted(signal);
      return speechAudioToBlob(speech);
    },

    async ensureReady() {
      const status = await api.getFishKeyStatus();
      if (!status.present) {
        throw new Error("Add a Fish Audio API key in Settings to use this voice.");
      }
    },

    async listVoices() {
      return api.listFishVoices();
    },
  };
}
```

Add the `case "fish":` arm to `createSpeechEngine` in `src/lib/speech/index.ts` and add `fishVoiceId: string | null` to `SpeechEngineSettings`. Add `provider: "supertonic"` to the existing Supertonic `synthesizeSpeech` call.

- [ ] **Step 5: Run the tests**

Run: `npm test && npm run build` — Expected: all pass, typecheck clean.

- [ ] **Step 6: Commit**

```bash
git add src/lib src/types src/stores
git commit -m "feat: add the Fish speech engine behind createSpeechEngine"
```

---

### Task 8: Settings UI for the key and voice

**Files:**
- Create: `src/components/Settings/FishAudioSettings.tsx`
- Modify: `src/components/Settings/SettingsPanel.tsx`, `src/stores/settings.ts`

**Interfaces:**
- Consumes: `api.getFishKeyStatus`, `api.setFishApiKey`, `api.clearFishApiKey`, `api.listFishVoices`.
- Produces: `<FishAudioSettings />`, self-contained.

- [ ] **Step 1: Build the component**

`FishAudioSettings.tsx` renders three states driven by `getFishKeyStatus` on mount:

- **absent** — a `type="password"` input, a Save button, and a link to `https://fish.audio/app/api-keys`.
- **present** — "A key is saved" with Replace and Remove buttons. Never renders the key: no command returns it.
- **saving/invalid** — inline error from the rejected `setFishApiKey` call; on `auth` the message must say the key was rejected, not that the network failed.

Below it, a voice picker populated from `listFishVoices()` plus a free-text field accepting a public voice id pasted from fish.audio, since the good narration voices are public models the user does not own. Persist to `fish_voice_id` via `api.setSetting`.

- [ ] **Step 2: Add a provider picker**

`SettingsPanel.tsx` currently has no provider selector — the one-option pickers were removed with Kokoro. Add a two-option control bound to `useSettingsStore.setTtsProvider` (which exists and has had zero callers by design). Selecting Fish with no key saved must show the reason inline rather than silently failing at playback.

- [ ] **Step 3: Fix the test button**

`SettingsPanel.tsx`'s TTS test button hardcodes `ttsProvider: "supertonic"`. With two providers it would silently test the wrong engine. Change it to pass the currently selected provider.

- [ ] **Step 4: Verify**

Run: `npm run build && npm test` — Expected: typecheck clean, all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/components/Settings src/stores/settings.ts
git commit -m "feat: add Fish Audio key and voice settings"
```

---

### Task 9: The export confirmation gate

**Files:**
- Modify: `src-tauri/src/commands/chapter_tts.rs`, `src-tauri/src/tts/supertonic/mod.rs`, `src/lib/tauri.ts`, `src/components/Reader/SupertonicChapterExport.tsx`

**Interfaces:**
- Consumes: `ChapterEstimate` (Task 5).
- Produces: `ChapterEstimate` gains `pub billable_characters: u32` and `pub provider: String`.

- [ ] **Step 1: Write the failing test**

In `chapter_tts.rs`:

```rust
#[cfg(test)]
mod billable_tests {
    use super::billable_characters;

    #[test]
    fn a_cached_chapter_bills_nothing() {
        // The gate must not ask the user to approve spending on audio that
        // already exists on disk.
        assert_eq!(billable_characters("Some text here.", "fish", true), 0);
    }

    #[test]
    fn an_uncached_fish_chapter_bills_its_characters() {
        assert_eq!(billable_characters("Some text here.", "fish", false), 15);
    }

    #[test]
    fn supertonic_never_bills() {
        assert_eq!(billable_characters("Some text here.", "supertonic", false), 0);
    }

    #[test]
    fn counts_characters_not_bytes() {
        // Fish bills text, and a multi-byte character is one character. Using
        // len() here would overstate an accented or CJK chapter by 2-3x.
        assert_eq!(billable_characters("héllo", "fish", false), 5);
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p libretexts-reader billable`
Expected: FAIL to compile — `billable_characters` not found.

- [ ] **Step 3: Implement**

In `chapter_tts.rs`:

```rust
/// How many characters this export will actually be billed for.
///
/// Zero for a cached chapter and zero for any local provider, so the gate
/// never asks the user to approve spending that will not happen.
pub fn billable_characters(text: &str, provider: &str, cached: bool) -> u32 {
    if cached || provider != "fish" {
        return 0;
    }
    text.chars().count() as u32
}
```

Add `billable_characters: u32` and `provider: String` to `ChapterEstimate`, populated from this function, and mirror both fields on `SupertonicChapterEstimate` in `src/lib/tauri.ts` as `billableCharacters: number` and `provider: TtsProvider`.

- [ ] **Step 4: Add the frontend gate**

In `SupertonicChapterExport.tsx`, when `estimate.billableCharacters > 0`, require an explicit confirmation naming the provider and the character count before calling the export command. Show the credit balance from `getFishKeyStatus`.

**Do not display a computed price.** A per-character rate hardcoded in the app goes stale silently and then misstates what the user is about to spend. Characters and the live balance are both facts that stay true.

- [ ] **Step 5: Verify and commit**

Run: `cargo test -p libretexts-reader && npm run build && npm test`

```bash
git add src-tauri/src src/components/Reader src/lib/tauri.ts
git commit -m "feat: gate paid chapter export behind an explicit confirmation"
```

---

### Task 10: Failure handling in the player

**Files:**
- Modify: `src/stores/player.ts`, `src/components/MiniPlayer.tsx`

- [ ] **Step 1: Stop and surface, never fall back silently**

On a synthesis error while the Fish engine is active, playback pauses and surfaces a message naming the cause, with a one-click "Switch to Supertonic" action that sets `ttsProvider` and resumes.

Do **not** fall back automatically. A silent fallback means the user cannot tell a paid engine from a free one by ear, and a permanently broken key would go unnoticed indefinitely.

- [ ] **Step 2: Map the error kinds to messages**

`auth` → "Fish Audio rejected your API key." · `payment_required` → "Your Fish Audio account is out of credit." · `rate_limited` → "Fish Audio is rate limiting requests." · `voice` → "Choose a Fish Audio voice in Settings." · anything else → the error's own message.

- [ ] **Step 3: Write a store test**

Assert that a rejecting synthesis leaves playback paused, records the message, and does **not** change `ttsProvider` — the switch is the user's action, not an automatic one. Follow the pattern in `src/stores/imports.test.ts`: seed a non-null value before asserting it changes, so the test cannot pass against a no-op.

- [ ] **Step 4: Verify and commit**

Run: `npm test && npm run build`

```bash
git add src/stores/player.ts src/components/MiniPlayer.tsx
git commit -m "feat: stop and offer a switch when Fish Audio fails"
```

---

### Task 11: Documentation

**Files:**
- Modify: `CLAUDE.md`, `README.md`, `HANDOFF.md`

- [ ] **Step 1: Reword the offline claim**

`CLAUDE.md` and `README.md` both say the app is on-device / offline by design. Change to: local by default, with one optional cloud provider the user configures. State that the app works fully offline on Supertonic.

- [ ] **Step 2: Delete the obsolete CSP note**

`HANDOFF.md` says `connect-src` must gain `https://api.fish.audio`. Remove it — Fish runs in Rust, which is not subject to the webview CSP. Leaving it would invite someone to widen the CSP for nothing.

- [ ] **Step 3: Record data retention**

Note in `README.md` that Fish may retain requests to improve model quality. A user reading licensed material aloud should learn that from our docs, not only from Fish's blog.

- [ ] **Step 4: Add the engine to `CLAUDE.md`'s architecture section**

Document `TtsProvider`, that the key lives in the keychain and is never exposed to the webview, and the rule that provider dispatch reads the request rather than settings.

- [ ] **Step 5: Commit**

```bash
git add CLAUDE.md README.md HANDOFF.md
git commit -m "docs: describe the Fish Audio provider and drop the obsolete CSP note"
```

---

## Manual verification

Needs a real API key and will cost a small amount of money.

- [ ] Paste an invalid key → rejected at entry, nothing stored. Confirm with Keychain Access that no entry was created.
- [ ] Paste a valid key → validates, credit balance shown, entry appears under service `dev.johnnylibretexts.reader`.
- [ ] Play a section on Fish → the chosen voice is what is heard.
- [ ] Export one chapter → the gate names the right character count; re-export reports cached and bills nothing.
- [ ] Switch to Supertonic and export the same chapter → different audio, not the cached Fish file. This is the Task 4 regression, verified end to end.
- [ ] Disconnect the network mid-playback → playback stops, names the cause, offers Supertonic; accepting it resumes.
- [ ] Remove the key → playback on Fish fails with a message pointing at Settings, and Supertonic still works with no network.
