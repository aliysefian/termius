//! Device registry and advisory locks.
//!
//! Each device keeps an encrypted record of itself (name, platform, last
//! seen), and while unlocked refreshes a short-lived lock record. Other
//! devices can then show "also open on Laptop, active 1 minute ago".
//!
//! Locks are **advisory only**. A sync service can deliver them late or not
//! at all, so they never block a save; per-record revisions do that. Locks
//! expire on their own if a device crashes.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{now_ms, Base, Collection, Record, Result, Vault};

/// How long a lock counts as active without a refresh.
pub const LOCK_TTL_MS: u64 = 2 * 60 * 1000;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeviceRecord {
    pub name: String,
    pub platform: String,
    pub app_version: String,
    pub first_seen: u64,
    pub last_seen: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockRecord {
    pub device_name: String,
    pub pid: u32,
    pub since: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActiveSession {
    pub device_id: Uuid,
    pub device_name: String,
    pub since: u64,
    pub expires_at: u64,
}

impl Vault {
    /// Create or refresh this device's registry entry.
    pub fn register_device(&self, platform: &str, app_version: &str) -> Result<()> {
        let id = self.device().id;
        let now = now_ms();
        let first_seen = self
            .get::<DeviceRecord>(Collection::Devices, id)
            .map(|r| r.data.map(|d| d.first_seen).unwrap_or(now))
            .unwrap_or(now);
        self.put(
            Collection::Devices,
            id,
            &DeviceRecord {
                name: self.device().name.clone(),
                platform: platform.into(),
                app_version: app_version.into(),
                first_seen,
                last_seen: now,
            },
            Base::Latest,
        )?;
        Ok(())
    }

    pub fn devices(&self) -> Result<Vec<Record<DeviceRecord>>> {
        Ok(self.list(Collection::Devices)?.records)
    }

    /// Write or refresh this device's advisory lock.
    pub fn touch_lock(&self) -> Result<()> {
        let id = self.device().id;
        let now = now_ms();
        let since = self
            .get::<LockRecord>(Collection::Locks, id)
            .ok()
            .and_then(|r| r.data)
            .filter(|l| l.expires_at > now)
            .map(|l| l.since)
            .unwrap_or(now);
        self.put(
            Collection::Locks,
            id,
            &LockRecord {
                device_name: self.device().name.clone(),
                pid: std::process::id(),
                since,
                expires_at: now + LOCK_TTL_MS,
            },
            Base::Latest,
        )?;
        Ok(())
    }

    /// Remove this device's lock (on lock or quit).
    pub fn release_lock(&self) -> Result<()> {
        self.remove_file(Collection::Locks, self.device().id)
    }

    /// Other devices whose lock hasn't expired.
    pub fn active_sessions(&self) -> Result<Vec<ActiveSession>> {
        let now = now_ms();
        Ok(self
            .list::<LockRecord>(Collection::Locks)?
            .records
            .into_iter()
            .filter(|r| r.id != self.device().id)
            .filter_map(|r| {
                let l = r.data?;
                (l.expires_at > now).then_some(ActiveSession {
                    device_id: r.id,
                    device_name: l.device_name,
                    since: l.since,
                    expires_at: l.expires_at,
                })
            })
            .collect())
    }
}
