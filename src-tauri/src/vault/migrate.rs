//! Format migrations. Currently v1 → v2.
//!
//! v1 encrypted records directly with a password-derived key and had no
//! revisions. v2 adds a random vault master key (envelope encryption),
//! vault-bound AAD and per-record revisions.
//!
//! The steps never touch the only copy:
//!
//! 1. Unlock v1 with the password (proves it's right before changing anything).
//! 2. Copy the v1 files as-is into `backups/v1-<stamp>/`. They stay encrypted.
//! 3. Re-encrypt every record into `.staging-v2/` under a new VMK.
//! 4. Decrypt the whole staging copy again and compare it to the v1 data.
//! 5. Journal `swap`, then move each v1 folder to `.old-v1/` and its staged
//!    replacement into place, then atomically replace `vault.json`.
//! 6. Remove the journal and the temporary folders.
//!
//! If anything stops between steps 5 and 6, the next open sees the journal:
//! if `vault.json` is still v1 it rolls the folders back, and if it's v2 it
//! finishes the clean-up. Another device that sees a fresh journal from
//! someone else waits rather than interfering.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::crypto::{KdfParams, MasterKey};

use super::{
    atomic, backup, format, legacy_v1, now_ms, Collection, DeviceInfo, Result, SlotKind, Unlock,
    Vault, VaultError, MANIFEST_FILE,
};

pub const JOURNAL: &str = ".migration-v2.json";
pub const STAGING: &str = ".staging-v2";
pub const OLD: &str = ".old-v1";
/// A journal from another device younger than this means "in progress".
const IN_PROGRESS_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Serialize, Deserialize)]
struct Journal {
    device_id: uuid::Uuid,
    device_name: String,
    started_at: u64,
    step: String,
}

// Test hook: stop the migration after the Nth folder swap, as if the
// machine lost power.
#[cfg(test)]
thread_local! {
    pub static CRASH_AFTER_SWAPS: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

fn v1_collections() -> [(legacy_v1::Collection, Collection); 4] {
    [
        (legacy_v1::Collection::Hosts, Collection::Hosts),
        (legacy_v1::Collection::Identities, Collection::Identities),
        (legacy_v1::Collection::Snippets, Collection::Snippets),
        (legacy_v1::Collection::Forwards, Collection::Forwards),
    ]
}

fn io_err(p: &Path) -> impl FnOnce(std::io::Error) -> VaultError + '_ {
    move |e| VaultError::io(p, e)
}

fn copy_dir_files(from: &Path, to: &Path) -> Result<()> {
    atomic::create_private_dir(to).map_err(io_err(to))?;
    let Ok(entries) = fs::read_dir(from) else {
        return Ok(());
    };
    for e in entries.flatten() {
        if e.path().is_file() {
            let dest = to.join(e.file_name());
            fs::copy(e.path(), &dest).map_err(io_err(&dest))?;
        }
    }
    Ok(())
}

/// Called before every open: recover from an interrupted migration.
pub fn check_journal(root: &Path, device: &DeviceInfo) -> Result<()> {
    let jpath = root.join(JOURNAL);
    let Ok(bytes) = super::read_regular_file(&jpath, 64 * 1024) else {
        return Ok(());
    };
    let j: Journal = serde_json::from_slice(&bytes).unwrap_or(Journal {
        device_id: device.id,
        device_name: device.name.clone(),
        started_at: 0,
        step: "unknown".into(),
    });
    if j.device_id != device.id && now_ms().saturating_sub(j.started_at) < IN_PROGRESS_MS {
        return Err(VaultError::MigrationInProgress {
            device: j.device_name,
        });
    }
    recover(root)
}

fn recover(root: &Path) -> Result<()> {
    let old = root.join(OLD);
    let staging = root.join(STAGING);
    match format::read(root) {
        Ok(format::OnDisk::V2(_)) => {
            // The swap completed; only clean-up was left.
        }
        _ => {
            // Still v1: put the original folders back.
            for (_, c) in v1_collections() {
                let orig = old.join(c.dir_name());
                if orig.exists() {
                    let cur = root.join(c.dir_name());
                    if cur.exists() {
                        fs::remove_dir_all(&cur).map_err(io_err(&cur))?;
                    }
                    fs::rename(&orig, &cur).map_err(io_err(&cur))?;
                }
            }
        }
    }
    for dir in [&old, &staging] {
        if dir.exists() {
            fs::remove_dir_all(dir).map_err(io_err(dir))?;
        }
    }
    let _ = fs::remove_file(root.join(JOURNAL));
    Ok(())
}

/// Upgrade a v1 vault to v2 and return it unlocked.
pub fn migrate_v1(
    root: &Path,
    password: &[u8],
    device: DeviceInfo,
    kdf: KdfParams,
) -> Result<Vault> {
    check_journal(root, &device)?;
    let v1 = legacy_v1::Vault::open(root, password).map_err(|e| match e {
        legacy_v1::VaultError::WrongPassword => VaultError::WrongPassword,
        other => VaultError::Backup(format!("could not read the old vault: {other}")),
    })?;
    let vault_id = v1.manifest().vault_id;

    // 1-2. Read everything first, then take the raw backup.
    let mut data: Vec<(Collection, legacy_v1::Record<Value>)> = Vec::new();
    for (lc, c) in v1_collections() {
        let listing = v1
            .list_with_tombstones::<Value>(lc)
            .map_err(|e| VaultError::Backup(format!("could not read {c}: {e}")))?;
        if let Some(bad) = listing.skipped.first() {
            return Err(VaultError::Backup(format!(
                "refusing to upgrade: {} could not be read ({})",
                bad.path.display(),
                bad.reason
            )));
        }
        data.extend(listing.records.into_iter().map(|r| (c, r)));
    }
    let raw = root
        .join(backup::BACKUP_DIR)
        .join(format!("v1-{}", backup::stamp(now_ms())));
    atomic::create_private_dir(&raw).map_err(io_err(&raw))?;
    fs::copy(root.join(MANIFEST_FILE), raw.join(MANIFEST_FILE)).map_err(io_err(&raw))?;
    for (_, c) in v1_collections() {
        copy_dir_files(&root.join(c.dir_name()), &raw.join(c.dir_name()))?;
    }

    let journal = |step: &str| -> Result<()> {
        let j = Journal {
            device_id: device.id,
            device_name: device.name.clone(),
            started_at: now_ms(),
            step: step.into(),
        };
        let p = root.join(JOURNAL);
        atomic::write(
            &p,
            &serde_json::to_vec(&j).map_err(|e| VaultError::json("journal", e))?,
        )
        .map_err(io_err(&p))
    };
    journal("staging")?;

    // 3. Re-encrypt into staging under a fresh VMK.
    let staging = root.join(STAGING);
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(io_err(&staging))?;
    }
    let vmk = MasterKey::generate();
    let slot = format::new_slot(vault_id, SlotKind::Password, password, &vmk, kdf)?;
    let mut manifest = format::new_manifest(vault_id, &vmk, vec![slot])?;
    manifest.created_at = v1.manifest().created_at;
    let staged = Vault {
        root: staging.clone(),
        vmk: vmk.clone(),
        manifest: manifest.clone(),
        device: device.clone(),
    };
    for c in Collection::ALL {
        let d = staging.join(c.dir_name());
        atomic::create_private_dir(&d).map_err(io_err(&d))?;
    }
    for (c, r) in &data {
        let mut env = staged.new_envelope(
            r.id,
            1,
            0,
            if r.deleted { None } else { r.data.clone() },
            None,
        );
        env.updated_at = r.updated_at;
        staged.write_envelope(*c, &env)?;
    }

    // 4. Validate the staged copy against what we read.
    for (c, r) in &data {
        let env = staged
            .read_envelope(*c, r.id)?
            .ok_or_else(|| VaultError::Backup(format!("staged {c}/{} is missing", r.id)))?;
        let expected = if r.deleted { None } else { r.data.as_ref() };
        if env.data.as_ref() != expected || env.deleted != r.deleted {
            return Err(VaultError::Backup(format!(
                "staged {c}/{} does not match the original",
                r.id
            )));
        }
    }
    drop(v1);

    // 5. Swap folders, then the manifest.
    journal("swap")?;
    let old = root.join(OLD);
    atomic::create_private_dir(&old).map_err(io_err(&old))?;
    let mut swaps = 0;
    for c in Collection::ALL {
        let cur = root.join(c.dir_name());
        if cur.exists() {
            fs::rename(&cur, old.join(c.dir_name())).map_err(io_err(&cur))?;
        }
        fs::rename(staging.join(c.dir_name()), &cur).map_err(io_err(&cur))?;
        swaps += 1;
        #[cfg(test)]
        if CRASH_AFTER_SWAPS.with(|s| s.get()) == Some(swaps) {
            return Err(VaultError::Backup(
                "simulated crash during migration".into(),
            ));
        }
    }
    let _ = swaps;
    format::write(root, &manifest)?;

    // 6. Clean up.
    for dir in [&old, &staging] {
        let _ = fs::remove_dir_all(dir);
    }
    let _ = fs::remove_file(root.join(JOURNAL));
    Vault::open(root, Unlock::Key(vmk), device)
}
