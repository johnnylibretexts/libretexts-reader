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
        Self {
            account: account.to_string(),
        }
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
