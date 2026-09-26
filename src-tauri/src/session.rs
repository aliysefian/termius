//! Lifecycle of the unlocked vault inside the running app: open/create, hold
//! the key in memory, run the file watcher, lock again. Tauri-agnostic so it
//! can be unit tested; `commands.rs` is the thin IPC layer on top.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use uuid::Uuid;

use crate::crypto::KdfParams;
use crate::sync::{self, RecordChange, SharedVault, VaultWatcher};
use crate::vault::{Vault, VaultError};

const WATCH_DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("vault is locked")]
    Locked,
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("could not watch vault folder: {0}")]
    Watch(#[from] notify::Error),
}

/// What the UI needs to decide which screen to show.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum VaultStatus {
    /// No vault folder chosen yet on this machine.
    NotConfigured,
    /// Folder chosen but it holds no manifest: offer "create vault".
    NeedsSetup {
        path: PathBuf,
    },
    /// Manifest present, key not in memory: ask for the master password.
    Locked {
        path: PathBuf,
    },
    Unlocked {
        path: PathBuf,
        vault_id: Uuid,
    },
}

#[derive(Default)]
pub struct Session {
    vault: SharedVault,
    watcher: Mutex<Option<VaultWatcher>>,
}

impl Session {
    pub fn new() -> Self {
        Self {
            vault: Arc::new(Mutex::new(None)),
            watcher: Mutex::new(None),
        }
    }

    pub fn status(&self, configured_path: Option<&Path>) -> VaultStatus {
        if let Some(v) = self
            .vault
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            return VaultStatus::Unlocked {
                path: v.root().to_path_buf(),
                vault_id: v.manifest().vault_id,
            };
        }
        match configured_path {
            None => VaultStatus::NotConfigured,
            Some(p) if Vault::exists(p) => VaultStatus::Locked {
                path: p.to_path_buf(),
            },
            Some(p) => VaultStatus::NeedsSetup {
                path: p.to_path_buf(),
            },
        }
    }

    pub fn is_unlocked(&self) -> bool {
        self.vault
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some()
    }

    /// Open an existing vault and start watching it.
    pub fn unlock<F>(
        &self,
        root: PathBuf,
        password: &[u8],
        on_change: F,
    ) -> Result<(), SessionError>
    where
        F: Fn(RecordChange) + Send + 'static,
    {
        let vault = Vault::open(&root, password)?;
        self.install(vault, on_change)
    }

    /// Create a new vault and start watching it.
    pub fn create<F>(
        &self,
        root: PathBuf,
        password: &[u8],
        kdf: KdfParams,
        on_change: F,
    ) -> Result<(), SessionError>
    where
        F: Fn(RecordChange) + Send + 'static,
    {
        let vault = Vault::create(&root, password, kdf)?;
        self.install(vault, on_change)
    }

    fn install<F>(&self, vault: Vault, on_change: F) -> Result<(), SessionError>
    where
        F: Fn(RecordChange) + Send + 'static,
    {
        let root = vault.root().to_path_buf();
        self.lock();
        *self.vault.lock().unwrap_or_else(|p| p.into_inner()) = Some(vault);
        let watcher = sync::watch(self.vault.clone(), root, WATCH_DEBOUNCE, on_change)?;
        *self.watcher.lock().unwrap_or_else(|p| p.into_inner()) = Some(watcher);
        Ok(())
    }

    /// Drop the key and stop the watcher. Safe to call when already locked.
    pub fn lock(&self) {
        // Stop the watcher first so it cannot observe a half-locked state.
        self.watcher
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        self.vault.lock().unwrap_or_else(|p| p.into_inner()).take();
    }

    /// Run `f` against the unlocked vault.
    pub fn with_vault<R>(
        &self,
        f: impl FnOnce(&Vault) -> Result<R, VaultError>,
    ) -> Result<R, SessionError> {
        let guard = self.vault.lock().unwrap_or_else(|p| p.into_inner());
        let vault = guard.as_ref().ok_or(SessionError::Locked)?;
        Ok(f(vault)?)
    }

    /// Run `f` with mutable access, e.g. for password changes.
    pub fn with_vault_mut<R>(
        &self,
        f: impl FnOnce(&mut Vault) -> Result<R, VaultError>,
    ) -> Result<R, SessionError> {
        let mut guard = self.vault.lock().unwrap_or_else(|p| p.into_inner());
        let vault = guard.as_mut().ok_or(SessionError::Locked)?;
        Ok(f(vault)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::Collection;

    #[test]
    fn status_transitions() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("vault");
        let s = Session::new();

        assert_eq!(s.status(None), VaultStatus::NotConfigured);
        assert_eq!(
            s.status(Some(&root)),
            VaultStatus::NeedsSetup { path: root.clone() }
        );

        s.create(root.clone(), b"pw", KdfParams::insecure_for_tests(), |_| {})
            .unwrap();
        assert!(matches!(
            s.status(Some(&root)),
            VaultStatus::Unlocked { .. }
        ));
        assert!(s.is_unlocked());

        s.lock();
        assert_eq!(
            s.status(Some(&root)),
            VaultStatus::Locked { path: root.clone() }
        );
        assert!(matches!(
            s.with_vault(|_| Ok(())).unwrap_err(),
            SessionError::Locked
        ));

        assert!(matches!(
            s.unlock(root.clone(), b"nope", |_| {}).unwrap_err(),
            SessionError::Vault(VaultError::WrongPassword)
        ));
        s.unlock(root.clone(), b"pw", |_| {}).unwrap();
        s.with_vault(|v| {
            v.insert(Collection::Snippets, &"ls".to_string())
                .map(|_| ())
        })
        .unwrap();
        let n = s
            .with_vault(|v| {
                v.list::<String>(Collection::Snippets)
                    .map(|l| l.records.len())
            })
            .unwrap();
        assert_eq!(n, 1);
    }
}
