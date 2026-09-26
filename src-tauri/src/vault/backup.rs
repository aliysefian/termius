//! Encrypted backups.
//!
//! A backup is one file, `backups/<UTC stamp>-<device>.enc`: a JSON snapshot
//! of every record (including tombstones, excluding previous versions and
//! locks) encrypted with the vault master key. Its associated data binds the
//! vault ID, so a backup can't be restored into another vault.
//!
//! Restoring verifies the whole backup first (authentication, vault ID,
//! format version, every record's hash), takes a fresh "before restore"
//! backup, and then writes the old contents as *new revisions*. That way
//! other devices pick the restore up as an ordinary change, instead of the
//! restore fighting their newer revision numbers.

use std::collections::{BTreeMap, HashMap};
use std::fs;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::crypto;

use super::{
    atomic, content_hash, now_ms, Base, Collection, Envelope, Result, Vault, VaultError,
    FORMAT_VERSION,
};

pub const BACKUP_DIR: &str = "backups";
const BACKUP_FORMAT: &str = "sshvault-backup";
pub const DEFAULT_RETENTION: usize = 30;
pub const DEFAULT_INTERVAL_MS: u64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Serialize, Deserialize)]
struct Payload {
    format: String,
    version: u32,
    vault_id: Uuid,
    created_at: u64,
    device_id: Uuid,
    device_name: String,
    reason: String,
    records: BTreeMap<Collection, Vec<Envelope>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupInfo {
    pub file_name: String,
    pub created_at: u64,
    pub size: u64,
    /// `None` when the backup failed verification.
    pub device_name: Option<String>,
    pub reason: Option<String>,
    pub records: usize,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RestoreReport {
    pub restored: usize,
    pub removed: usize,
    pub unchanged: usize,
    /// The backup taken just before restoring, to undo it.
    pub safety_backup: String,
}

/// `YYYYMMDDTHHMMSSZ` from Unix milliseconds (proleptic Gregorian, UTC).
pub fn stamp(ms: u64) -> String {
    let secs = ms / 1000;
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Howard Hinnant's days_from_civil inverse.
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn is_backup_name(name: &str) -> bool {
    // 16-char stamp, '-', 8 hex chars of device ID, optional "-N", ".enc".
    name.len() >= 16 + 1 + 8 + 4
        && name.ends_with(".enc")
        && name.as_bytes()[8] == b'T'
        && name.as_bytes()[15] == b'Z'
        && !name.contains(['/', '\\'])
        && !name.starts_with('.')
}

impl Vault {
    fn backup_dir(&self) -> std::path::PathBuf {
        self.root().join(BACKUP_DIR)
    }

    fn backup_aad(&self) -> Vec<u8> {
        format!("sshvault/v2/{}/backup", self.vault_id()).into_bytes()
    }

    /// Snapshot everything into a new encrypted backup, then prune to the
    /// newest `retention` backups.
    pub fn create_backup(&self, reason: &str, retention: usize) -> Result<BackupInfo> {
        let mut records = BTreeMap::new();
        for c in Collection::ALL.into_iter().filter(|c| c.is_synced_data()) {
            let mut envs = Vec::new();
            let listing = self.list_with_tombstones::<Value>(c)?;
            for r in listing.records {
                if let Some(mut e) = self.read_envelope(c, r.id)? {
                    e.prev = None;
                    envs.push(e);
                }
            }
            records.insert(c, envs);
        }
        let created_at = now_ms();
        let count = records.values().map(Vec::len).sum();
        let payload = Payload {
            format: BACKUP_FORMAT.into(),
            version: FORMAT_VERSION,
            vault_id: self.vault_id(),
            created_at,
            device_id: self.device().id,
            device_name: self.device().name.clone(),
            reason: reason.into(),
            records,
        };
        let plain = Zeroizing::new(
            serde_json::to_vec(&payload).map_err(|e| VaultError::json("backup", e))?,
        );
        let bytes = crypto::encrypt(self.master_key(), &plain, &self.backup_aad())?;

        let dir = self.backup_dir();
        atomic::create_private_dir(&dir).map_err(|e| VaultError::io(&dir, e))?;
        let device = &self.device().id.simple().to_string()[..8];
        let mut name = format!("{}-{device}.enc", stamp(created_at));
        let mut n = 1;
        while dir.join(&name).exists() {
            n += 1;
            name = format!("{}-{device}-{n}.enc", stamp(created_at));
        }
        let path = dir.join(&name);
        atomic::write(&path, &bytes).map_err(|e| VaultError::io(&path, e))?;
        self.prune_backups(retention)?;
        Ok(BackupInfo {
            file_name: name,
            created_at,
            size: bytes.len() as u64,
            device_name: Some(self.device().name.clone()),
            reason: Some(reason.into()),
            records: count,
            error: None,
        })
    }

    fn backup_names(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.backup_dir())
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_file())
                    .filter_map(|e| e.file_name().to_str().map(str::to_string))
                    .filter(|n| is_backup_name(n))
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names.reverse(); // newest first (stamps sort lexically)
        names
    }

    fn prune_backups(&self, retention: usize) -> Result<()> {
        for old in self.backup_names().into_iter().skip(retention.max(1)) {
            let p = self.backup_dir().join(old);
            let _ = fs::remove_file(p);
        }
        Ok(())
    }

    /// Decrypt and fully verify a backup. Never writes.
    fn read_backup(&self, file_name: &str) -> Result<Payload> {
        if !is_backup_name(file_name) {
            return Err(VaultError::Backup(format!(
                "\"{file_name}\" is not a backup file name"
            )));
        }
        let path = self.backup_dir().join(file_name);
        let bytes = super::read_regular_file(&path, 512 * 1024 * 1024).map_err(|e| VaultError::io(&path, e))?;
        let plain = Zeroizing::new(
            crypto::decrypt(self.master_key(), &bytes, &self.backup_aad()).map_err(|_| {
                VaultError::Backup(format!(
                    "{file_name} is corrupted or belongs to another vault"
                ))
            })?,
        );
        let p: Payload = serde_json::from_slice(&plain)
            .map_err(|e| VaultError::Backup(format!("{file_name}: {e}")))?;
        if p.format != BACKUP_FORMAT || p.vault_id != self.vault_id() {
            return Err(VaultError::Backup(format!(
                "{file_name} is not a backup of this vault"
            )));
        }
        if p.version > FORMAT_VERSION {
            return Err(VaultError::Backup(format!(
                "{file_name} was made by a newer app version"
            )));
        }
        for (c, envs) in &p.records {
            let mut seen = std::collections::HashSet::new();
            for e in envs {
                if !seen.insert(e.id) || e.content_hash != content_hash(e.deleted, e.data.as_ref())
                {
                    return Err(VaultError::Backup(format!(
                        "{file_name}: record {c}/{} failed verification",
                        e.id
                    )));
                }
            }
        }
        Ok(p)
    }

    /// Verify a backup without restoring it.
    pub fn verify_backup(&self, file_name: &str) -> Result<BackupInfo> {
        // Validates the name before anything touches the path.
        let p = self.read_backup(file_name)?;
        let size = fs::metadata(self.backup_dir().join(file_name)).map(|m| m.len()).unwrap_or(0);
        Ok(BackupInfo {
            file_name: file_name.into(),
            created_at: p.created_at,
            size,
            device_name: Some(p.device_name),
            reason: Some(p.reason),
            records: p.records.values().map(Vec::len).sum(),
            error: None,
        })
    }

    /// All backups, newest first, each verified.
    pub fn list_backups(&self) -> Vec<BackupInfo> {
        self.backup_names()
            .into_iter()
            .map(|n| match self.verify_backup(&n) {
                Ok(i) => i,
                Err(e) => BackupInfo {
                    size: fs::metadata(self.backup_dir().join(&n))
                        .map(|m| m.len())
                        .unwrap_or(0),
                    file_name: n,
                    created_at: 0,
                    device_name: None,
                    reason: None,
                    records: 0,
                    error: Some(e.to_string()),
                },
            })
            .collect()
    }

    /// Newest backup time from file names, cheap (no decryption).
    pub fn last_backup_at(&self) -> Option<u64> {
        self.list_backups()
            .into_iter()
            .find(|b| b.error.is_none())
            .map(|b| b.created_at)
    }

    /// Take a backup if the newest one is older than `interval_ms`.
    pub fn auto_backup(&self, interval_ms: u64, retention: usize) -> Result<Option<BackupInfo>> {
        let due = self
            .last_backup_at()
            .is_none_or(|t| now_ms().saturating_sub(t) >= interval_ms);
        if due {
            self.create_backup("automatic", retention).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Restore a verified backup. Refuses anything that doesn't verify.
    pub fn restore_backup(&self, file_name: &str, retention: usize) -> Result<RestoreReport> {
        let payload = self.read_backup(file_name)?;
        // Keep the safety backup even if retention is tight.
        let safety =
            self.create_backup(&format!("before restoring {file_name}"), retention.max(2))?;
        let mut report = RestoreReport {
            safety_backup: safety.file_name,
            ..Default::default()
        };
        for c in Collection::ALL
            .into_iter()
            .filter(|c| c.is_synced_data() && *c != Collection::Devices)
        {
            let target: HashMap<Uuid, &Envelope> = payload
                .records
                .get(&c)
                .map(|v| v.iter().map(|e| (e.id, e)).collect())
                .unwrap_or_default();
            let current = self.list_with_tombstones::<Value>(c)?.records;
            let current_ids: std::collections::HashSet<Uuid> =
                current.iter().map(|r| r.id).collect();
            for r in &current {
                match target.get(&r.id) {
                    Some(t) if !t.deleted => {
                        let now = self.read_envelope(c, r.id)?.map(|e| e.content_hash);
                        if now.as_deref() == Some(t.content_hash.as_str()) {
                            report.unchanged += 1;
                        } else {
                            self.put(
                                c,
                                r.id,
                                t.data.as_ref().unwrap_or(&Value::Null),
                                Base::Latest,
                            )?;
                            report.restored += 1;
                        }
                    }
                    _ if !r.deleted => {
                        self.delete(c, r.id, Base::Latest)?;
                        report.removed += 1;
                    }
                    _ => report.unchanged += 1,
                }
            }
            for (id, t) in &target {
                if !current_ids.contains(id) && !t.deleted {
                    self.put(
                        c,
                        *id,
                        t.data.as_ref().unwrap_or(&Value::Null),
                        Base::Latest,
                    )?;
                    report.restored += 1;
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_utc_and_sortable() {
        assert_eq!(stamp(0), "19700101T000000Z");
        assert_eq!(stamp(1_790_000_000_000), "20260921T141320Z");
        assert!(stamp(1_000) < stamp(2_000));
        assert!(is_backup_name(&format!("{}-0123abcd.enc", stamp(0))));
        assert!(!is_backup_name("../../etc/passwd"));
        assert!(!is_backup_name("notes.enc"));
    }
}
