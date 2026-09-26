//! "Remember this vault on this device": the vault master key kept in the
//! operating system's credential store.
//!
//! | OS | Store |
//! |---|---|
//! | Windows | Credential Manager (DPAPI-protected) |
//! | macOS | Keychain |
//! | Linux | Secret Service (GNOME Keyring, KWallet) over D-Bus |
//!
//! What's stored is the VMK for one vault ID, never the master password.
//! Anyone who can read the user's keychain can open that vault on this
//! machine, which is the trade-off the user opts into. Every failure
//! (no Secret Service running, access denied, locked keychain) is reported
//! as "unavailable" and the app falls back to asking for the password; it
//! never falls back to storing the key anywhere else.

use uuid::Uuid;

use crate::crypto::MasterKey;
use crate::vault::{decode_key, encode_key};

const SERVICE: &str = "SSHVault";

#[derive(Debug, thiserror::Error)]
pub enum KeychainError {
    #[error("the system keychain is unavailable: {0}")]
    Unavailable(String),
}

pub trait KeyStore: Send + Sync {
    fn store(&self, vault_id: Uuid, key: &MasterKey) -> Result<(), KeychainError>;
    /// `Ok(None)` when nothing is stored for this vault.
    fn load(&self, vault_id: Uuid) -> Result<Option<MasterKey>, KeychainError>;
    fn forget(&self, vault_id: Uuid) -> Result<(), KeychainError>;
}

fn account(vault_id: Uuid) -> String {
    format!("vault-{vault_id}")
}

/// The real OS store. Calls block, so run them off the async runtime.
pub struct OsKeyStore;

impl KeyStore for OsKeyStore {
    fn store(&self, vault_id: Uuid, key: &MasterKey) -> Result<(), KeychainError> {
        let entry = keyring::Entry::new(SERVICE, &account(vault_id))
            .map_err(|e| KeychainError::Unavailable(e.to_string()))?;
        entry
            .set_password(&encode_key(key))
            .map_err(|e| KeychainError::Unavailable(e.to_string()))
    }

    fn load(&self, vault_id: Uuid) -> Result<Option<MasterKey>, KeychainError> {
        let entry = keyring::Entry::new(SERVICE, &account(vault_id))
            .map_err(|e| KeychainError::Unavailable(e.to_string()))?;
        match entry.get_password() {
            Ok(s) => Ok(decode_key(&zeroize::Zeroizing::new(s))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(KeychainError::Unavailable(e.to_string())),
        }
    }

    fn forget(&self, vault_id: Uuid) -> Result<(), KeychainError> {
        let entry = keyring::Entry::new(SERVICE, &account(vault_id))
            .map_err(|e| KeychainError::Unavailable(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(KeychainError::Unavailable(e.to_string())),
        }
    }
}

/// In-memory store for tests, optionally "unavailable".
#[cfg(test)]
#[derive(Default)]
pub struct MemoryKeyStore {
    pub unavailable: bool,
    map: std::sync::Mutex<std::collections::HashMap<Uuid, String>>,
}

#[cfg(test)]
impl KeyStore for MemoryKeyStore {
    fn store(&self, vault_id: Uuid, key: &MasterKey) -> Result<(), KeychainError> {
        if self.unavailable {
            return Err(KeychainError::Unavailable("no secret service".into()));
        }
        self.map
            .lock()
            .unwrap()
            .insert(vault_id, encode_key(key).to_string());
        Ok(())
    }
    fn load(&self, vault_id: Uuid) -> Result<Option<MasterKey>, KeychainError> {
        if self.unavailable {
            return Err(KeychainError::Unavailable("no secret service".into()));
        }
        Ok(self
            .map
            .lock()
            .unwrap()
            .get(&vault_id)
            .and_then(|s| decode_key(s)))
    }
    fn forget(&self, vault_id: Uuid) -> Result<(), KeychainError> {
        if self.unavailable {
            return Err(KeychainError::Unavailable("no secret service".into()));
        }
        self.map.lock().unwrap().remove(&vault_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::testutil::{new_vault, second_device};
    use crate::vault::{DeviceInfo, Unlock, Vault};

    #[test]
    fn remembered_key_unlocks_and_an_unavailable_keychain_falls_back() {
        let (_d, v) = new_vault();
        let store = MemoryKeyStore::default();
        store.store(v.vault_id(), v.master_key()).unwrap();
        let key = store.load(v.vault_id()).unwrap().expect("stored");
        let root = v.root().to_path_buf();
        let other = second_device(&v, "PC-B");
        drop((v, other));
        assert!(Vault::open(&root, Unlock::Key(key), DeviceInfo::new("PC-A")).is_ok());

        store.forget(uuid::Uuid::nil()).unwrap();
        let unavailable = MemoryKeyStore {
            unavailable: true,
            ..Default::default()
        };
        assert!(matches!(
            unavailable.load(uuid::Uuid::nil()),
            Err(KeychainError::Unavailable(_))
        ));
        assert!(unavailable
            .store(uuid::Uuid::nil(), &crate::crypto::MasterKey::generate())
            .is_err());
        // The vault itself is unaffected: the password still works.
        assert!(Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("PC-A")).is_ok());
    }
}
