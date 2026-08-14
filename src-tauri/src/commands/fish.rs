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

/// Store the key only if validation succeeded.
///
/// Takes the validation result rather than performing it, so the ordering --
/// an invalid key must never reach the keychain -- is testable without a
/// network round trip or a real keychain.
fn store_if_valid(
    store: &dyn SecretStore,
    key: &str,
    validation: AppResult<f64>,
) -> AppResult<FishKeyStatus> {
    match validation {
        Ok(credit) => {
            store.set(key)?;
            Ok(FishKeyStatus {
                present: true,
                valid: Some(true),
                credit: Some(credit),
            })
        }
        Err(error) => Err(error),
    }
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

    let validation = FishClient::new(key.clone())?.credit().await;
    store_if_valid(&KeyringSecretStore::new(FISH_KEY_ACCOUNT), &key, validation)
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

    #[test]
    fn store_if_valid_rejects_invalid_key_without_writing() {
        let store = MemorySecretStore::default();
        let validation_error = Err(crate::error::AppError::Auth("rejected".into()));

        let result = store_if_valid(&store, "sk-bad-key", validation_error);

        assert!(result.is_err(), "store_if_valid must propagate the error");
        assert!(
            store.get().expect("store read").is_none(),
            "invalid key must not be stored"
        );
    }

    #[test]
    fn store_if_valid_accepts_valid_key_and_returns_credit() {
        let store = MemorySecretStore::default();
        let validation = Ok(42.5);

        let status = store_if_valid(&store, "sk-good-key", validation).expect("store_if_valid");

        assert!(status.present);
        assert_eq!(status.valid, Some(true));
        assert_eq!(status.credit, Some(42.5));
        assert_eq!(
            store.get().expect("store read"),
            Some("sk-good-key".to_string()),
            "valid key must be stored"
        );
    }

    #[test]
    fn store_if_valid_preserves_existing_key_on_validation_failure() {
        let store = MemorySecretStore::default();
        store.set("sk-existing-key").expect("seed existing key");

        let validation_error = Err(crate::error::AppError::Auth("rejected".into()));
        let result = store_if_valid(&store, "sk-new-bad-key", validation_error);

        assert!(result.is_err(), "store_if_valid must propagate the error");
        assert_eq!(
            store.get().expect("store read"),
            Some("sk-existing-key".to_string()),
            "existing key must not be overwritten on validation failure"
        );
    }
}
