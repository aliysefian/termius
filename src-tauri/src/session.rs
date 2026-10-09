//! Lifecycle of the unlocked vault inside the running app: create, unlock
//! (password, device key or recovery key, upgrading v1 on the way), hold the
//! key in memory, run the file watcher and the lock heartbeat, and lock
//! again. Tauri-agnostic so it can be unit tested; `commands.rs` is the thin
//! IPC layer on top.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use uuid::Uuid;

use crate::crypto::{KdfParams, RecoveryKey};
use crate::sync::{self, RecordChange, SharedVault, VaultWatcher};
use crate::vault::{backup, migrate, CreateOptions, DeviceInfo, Unlock, Vault, VaultError};

const WATCH_DEBOUNCE: Duration = Duration::from_millis(400);
const HEARTBEAT: Duration = Duration::from_secs(60);

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
    /// Folder chosen but it holds no vault: offer "create vault".
    NeedsSetup {
        path: PathBuf,
    },
    /// A vault is there; the key is not in memory.
    Locked {
        path: PathBuf,
        /// This device stored the key in the OS keychain.
        remembered: bool,
        /// Old format: will be upgraded on unlock (needs the password).
        needs_upgrade: bool,
    },
    Unlocked {
        path: PathBuf,
        vault_id: Uuid,
    },
}

/// What happened while unlocking, for the UI to mention.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UnlockReport {
    /// The vault was upgraded from the old format.
    pub migrated: bool,
    /// Sync conflicts resolved automatically.
    pub merged_conflicts: usize,
    /// Sync conflicts that need a decision.
    pub open_conflicts: usize,
    /// An automatic backup was taken.
    pub backed_up: bool,
    /// Records that went backwards or vanished since this device last saw them (see `vault::highwater`).
    pub rollbacks: usize,
}

pub struct Session {
    vault: SharedVault,
    watcher: Mutex<Option<VaultWatcher>>,
    heartbeat_stop: Mutex<Option<Arc<AtomicBool>>>,
    /// Where this device keeps what it has seen of each vault, for noticing a rolled-back folder.
    highwater_dir: Mutex<Option<PathBuf>>,
    /// How many backups to keep, from the vault's synced settings.
    pub backup_retention: std::sync::atomic::AtomicUsize,
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

impl Session {
    pub fn new() -> Self {
        Self {
            vault: Arc::new(Mutex::new(None)),
            watcher: Mutex::new(None),
            heartbeat_stop: Mutex::new(None),
            highwater_dir: Mutex::new(None),
            backup_retention: backup::DEFAULT_RETENTION.into(),
        }
    }

    /// Turn on rollback detection: the per-vault memory files live in `dir`, outside the synced folder.
    pub fn set_highwater_dir(&self, dir: PathBuf) {
        *self.highwater_dir.lock().unwrap_or_else(|p| p.into_inner()) = Some(dir);
    }

    pub fn status(&self, configured_path: Option<&Path>, remembered: bool) -> VaultStatus {
        if let Some(v) = self
            .vault
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
        {
            return VaultStatus::Unlocked {
                path: v.root().to_path_buf(),
                vault_id: v.vault_id(),
            };
        }
        match configured_path {
            None => VaultStatus::NotConfigured,
            Some(p) if Vault::exists(p) => VaultStatus::Locked {
                path: p.to_path_buf(),
                remembered,
                needs_upgrade: matches!(
                    crate::vault::format::read(p),
                    Ok(crate::vault::format::OnDisk::V1)
                ),
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

    /// Create a new vault and unlock it. Returns the recovery key if asked for.
    pub fn create<F>(
        &self,
        root: PathBuf,
        password: &[u8],
        opts: CreateOptions,
        device: DeviceInfo,
        on_change: F,
    ) -> Result<Option<RecoveryKey>, SessionError>
    where
        F: Fn(RecordChange) + Send + 'static,
    {
        let (vault, rk) = Vault::create(&root, password, opts, device)?;
        self.install(vault, on_change)?;
        Ok(rk)
    }

    /// Unlock an existing vault. A v1 vault is upgraded first, which needs
    /// the password (a device key or recovery key can't exist for v1).
    pub fn unlock<F>(
        &self,
        root: PathBuf,
        unlock: Unlock<'_>,
        device: DeviceInfo,
        kdf: KdfParams,
        on_change: F,
    ) -> Result<UnlockReport, SessionError>
    where
        F: Fn(RecordChange) + Send + 'static,
    {
        let mut migrated = false;
        let vault = match (
            Vault::open(&root, unlock_clone(&unlock), device.clone()),
            &unlock,
        ) {
            (Err(VaultError::NeedsMigration), Unlock::Password(pw)) => {
                migrated = true;
                migrate::migrate_v1(&root, pw, device, kdf)?
            }
            (r, _) => r?,
        };
        let mut report = self.install(vault, on_change)?;
        report.migrated = migrated;
        Ok(report)
    }

    /// "Forgot master password": unlock with the recovery key and set a new
    /// password in one step.
    pub fn reset_with_recovery<F>(
        &self,
        root: PathBuf,
        recovery: &str,
        new_password: &[u8],
        device: DeviceInfo,
        kdf: KdfParams,
        on_change: F,
    ) -> Result<UnlockReport, SessionError>
    where
        F: Fn(RecordChange) + Send + 'static,
    {
        let vault =
            Vault::reset_password_with_recovery(&root, recovery, new_password, kdf, device)?;
        self.install(vault, on_change)
    }

    fn install<F>(&self, vault: Vault, on_change: F) -> Result<UnlockReport, SessionError>
    where
        F: Fn(RecordChange) + Send + 'static,
    {
        let root = vault.root().to_path_buf();
        self.lock();

        // Housekeeping that must not stop an unlock if it fails (for example
        // a read-only sync folder): conflicts, device registry, lock, backup.
        let mut report = UnlockReport::default();
        // Before anything below writes: what does the folder look like compared with what this device saw last time?
        if let Some(dir) = self.highwater_dir.lock().unwrap_or_else(|p| p.into_inner()).clone() {
            vault.attach_highwater(dir.join(format!("{}.json", vault.vault_id())));
            report.rollbacks = vault.check_rollbacks().map(|a| a.len()).unwrap_or(0);
        }
        if let Ok(r) = vault.reconcile() {
            report.merged_conflicts = r.merged;
            report.open_conflicts = r.unresolved;
        }
        let _ = vault.register_device(std::env::consts::OS, env!("CARGO_PKG_VERSION"));
        let _ = vault.touch_lock();
        // Retention is a vault-wide setting, shared by every device.
        if let Some(settings) = vault
            .get::<crate::models::VaultSettings>(crate::vault::Collection::Settings, crate::models::SETTINGS_ID)
            .ok()
            .and_then(|r| r.data)
        {
            self.backup_retention
                .store(settings.backup_retention.clamp(1, 1000), Ordering::Relaxed);
        }
        let retention = self.backup_retention.load(Ordering::Relaxed);
        report.backed_up = matches!(
            vault.auto_backup(backup::DEFAULT_INTERVAL_MS, retention),
            Ok(Some(_))
        );

        *self.vault.lock().unwrap_or_else(|p| p.into_inner()) = Some(vault);
        let watcher = sync::watch(self.vault.clone(), root, WATCH_DEBOUNCE, on_change)?;
        *self.watcher.lock().unwrap_or_else(|p| p.into_inner()) = Some(watcher);
        self.start_heartbeat();
        Ok(report)
    }

    /// Refresh the advisory lock while unlocked, so other devices can show
    /// "also open on …". Stops by itself when the vault locks.
    fn start_heartbeat(&self) {
        let stop = Arc::new(AtomicBool::new(false));
        *self
            .heartbeat_stop
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(Arc::clone(&stop));
        let vault = Arc::clone(&self.vault);
        let _ = std::thread::Builder::new()
            .name("vault-heartbeat".into())
            .spawn(move || {
                let tick = Duration::from_secs(1);
                let mut waited = Duration::ZERO;
                while !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(tick);
                    waited += tick;
                    if waited >= HEARTBEAT {
                        waited = Duration::ZERO;
                        if let Some(v) = vault.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
                            let _ = v.touch_lock();
                        }
                    }
                }
            });
    }

    /// Drop the key, stop the watcher and heartbeat, and release this
    /// device's advisory lock. Safe to call when already locked.
    pub fn lock(&self) {
        if let Some(stop) = self
            .heartbeat_stop
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take()
        {
            stop.store(true, Ordering::Relaxed);
        }
        // Stop the watcher first so it cannot observe a half-locked state.
        self.watcher
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(v) = self.vault.lock().unwrap_or_else(|p| p.into_inner()).take() {
            v.save_highwater();
            let _ = v.release_lock();
            // `v` drops here and its key is zeroized.
        }
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

/// `Unlock` holds a borrowed secret or an owned key; re-borrow it so the
/// password is still available for a migration after a failed open.
fn unlock_clone<'a>(u: &Unlock<'a>) -> Unlock<'a> {
    match u {
        Unlock::Password(p) => Unlock::Password(p),
        Unlock::Recovery(r) => Unlock::Recovery(r),
        Unlock::Key(k) => Unlock::Key(k.clone()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::testutil::{kdf, opts};
    use crate::vault::Collection;

    #[test]
    fn unlocking_reports_a_folder_that_went_backwards() {
        use crate::vault::{Base, Collection};
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("vault");
        let s = Session::new();
        s.set_highwater_dir(dir.path().join("highwater"));
        let dev = DeviceInfo::new("PC-A");
        s.create(root.clone(), b"pw", opts(false), dev.clone(), |_| {}).unwrap();
        let id = s.with_vault(|v| v.insert(Collection::Snippets, &"ls".to_string()).map(|r| r.id)).unwrap();
        let path = s.with_vault(|v| Ok(v.record_path(Collection::Snippets, id))).unwrap();
        let old = std::fs::read(&path).unwrap();
        s.with_vault(|v| v.put(Collection::Snippets, id, &"ls -la".to_string(), Base::Rev(1)).map(|_| ())).unwrap();
        s.lock(); // writes down revision 2

        std::fs::write(&path, &old).unwrap(); // the sync service brings revision 1 back
        let report = s.unlock(root.clone(), Unlock::Password(b"pw"), dev.clone(), kdf(), |_| {}).unwrap();
        assert_eq!(report.rollbacks, 1);
        s.with_vault(|v| v.accept_rollbacks()).unwrap();
        s.lock();
        let report = s.unlock(root, Unlock::Password(b"pw"), dev, kdf(), |_| {}).unwrap();
        assert_eq!(report.rollbacks, 0, "accepted: the folder as it is now is the baseline");
    }

    #[test]
    fn status_transitions_and_unlock_paths() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("vault");
        let s = Session::new();
        let dev = DeviceInfo::new("PC-A");

        assert_eq!(s.status(None, false), VaultStatus::NotConfigured);
        assert_eq!(
            s.status(Some(&root), false),
            VaultStatus::NeedsSetup { path: root.clone() }
        );

        let rk = s
            .create(root.clone(), b"pw", opts(true), dev.clone(), |_| {})
            .unwrap()
            .unwrap();
        assert!(matches!(
            s.status(Some(&root), false),
            VaultStatus::Unlocked { .. }
        ));
        let key = s.with_vault(|v| Ok(v.master_key().clone())).unwrap();

        s.lock();
        assert!(matches!(
            s.status(Some(&root), true),
            VaultStatus::Locked {
                remembered: true,
                needs_upgrade: false,
                ..
            }
        ));
        assert!(matches!(
            s.with_vault(|_| Ok(())).unwrap_err(),
            SessionError::Locked
        ));

        assert!(matches!(
            s.unlock(
                root.clone(),
                Unlock::Password(b"nope"),
                dev.clone(),
                kdf(),
                |_| {}
            )
            .unwrap_err(),
            SessionError::Vault(VaultError::WrongPassword)
        ));
        s.unlock(
            root.clone(),
            Unlock::Password(b"pw"),
            dev.clone(),
            kdf(),
            |_| {},
        )
        .unwrap();
        s.with_vault(|v| {
            v.insert(Collection::Snippets, &"ls".to_string())
                .map(|_| ())
        })
        .unwrap();
        s.lock();

        // Device key (from the keychain) and recovery key both work.
        s.unlock(root.clone(), Unlock::Key(key), dev.clone(), kdf(), |_| {})
            .unwrap();
        s.lock();
        s.reset_with_recovery(
            root.clone(),
            &rk.display(),
            b"new-pw",
            dev.clone(),
            kdf(),
            |_| {},
        )
        .unwrap();
        s.lock();
        s.unlock(
            root.clone(),
            Unlock::Password(b"new-pw"),
            dev,
            kdf(),
            |_| {},
        )
        .unwrap();
        let n = s
            .with_vault(|v| {
                v.list::<String>(Collection::Snippets)
                    .map(|l| l.records.len())
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn unlocking_a_v1_vault_upgrades_it() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("vault");
        let v1 = crate::vault::legacy_v1::Vault::create(&root, b"pw", kdf()).unwrap();
        v1.insert(
            crate::vault::legacy_v1::Collection::Hosts,
            &serde_json::json!({"label": "old"}),
        )
        .unwrap();
        drop(v1);
        let s = Session::new();
        assert!(matches!(
            s.status(Some(&root), false),
            VaultStatus::Locked {
                needs_upgrade: true,
                ..
            }
        ));
        let rep = s
            .unlock(
                root.clone(),
                Unlock::Password(b"pw"),
                DeviceInfo::new("A"),
                kdf(),
                |_| {},
            )
            .unwrap();
        assert!(rep.migrated && rep.backed_up);
        let n = s
            .with_vault(|v| {
                v.list::<serde_json::Value>(Collection::Hosts)
                    .map(|l| l.records.len())
            })
            .unwrap();
        assert_eq!(n, 1);
    }
}
