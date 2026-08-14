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
        return Err(crate::error::AppError::InvalidInput(
            "The API key is empty.".into(),
        ));
    }

    let credit = FishClient::new(key.clone())?.credit().await?;
    KeyringSecretStore::new(FISH_KEY_ACCOUNT).set(&key)?;

    Ok(FishKeyStatus {
        present: true,
        valid: Some(true),
        credit: Some(credit),
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemorySecretStore;

    #[test]
    fn status_reports_absence_without_touching_the_network() {
        let store = MemorySecretStore::default();
        assert!(!key_status_from(&store).unwrap().present);
    }

    #[test]
    fn status_reports_presence_without_revealing_the_key() {
        let store = MemorySecretStore::default();
        store.set("sk-secret").expect("set");

        let status = key_status_from(&store).unwrap();
        assert!(status.present);

        // The whole point: serialising the status must not leak the key.
        let json = serde_json::to_string(&status).expect("serialize");
        assert!(
            !json.contains("sk-secret"),
            "the key must never reach the frontend"
        );
    }
}
