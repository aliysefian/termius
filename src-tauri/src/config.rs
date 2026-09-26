//! Per-machine, unencrypted application settings. Lives in the OS config
//! directory, *not* in the synced vault, because it holds the local path to
//! the vault which differs on every PC.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppConfig {
    /// Absolute path of the vault root inside the user's synced folder.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vault_path: Option<PathBuf>,
    /// This installation's identity in the vault's device list. Generated
    /// once; never synced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_id: Option<uuid::Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device_name: Option<String>,
    /// The vault whose key this device keeps in the OS keychain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remember_vault: Option<uuid::Uuid>,
}

/// A readable name for this computer, for "also open on …".
pub fn default_device_name() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .ok()
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "This computer".into())
}

impl AppConfig {
    /// This device, creating (and saving) its ID on first use.
    pub fn device(&mut self, dir: &Path) -> Result<crate::vault::DeviceInfo, ConfigError> {
        if self.device_id.is_none() {
            self.device_id = Some(uuid::Uuid::new_v4());
            self.save(dir)?;
        }
        Ok(crate::vault::DeviceInfo {
            id: self.device_id.expect("set above"),
            name: self.device_name.clone().unwrap_or_else(default_device_name),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("config at {path} is malformed: {source}")]
    Malformed {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

impl AppConfig {
    /// Load from `dir/config.json`; a missing file yields the default config.
    pub fn load(dir: &Path) -> Result<Self, ConfigError> {
        let path = dir.join(CONFIG_FILE);
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|source| ConfigError::Malformed { path, source }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Io { path, source }),
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), ConfigError> {
        fs::create_dir_all(dir).map_err(|source| ConfigError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let path = dir.join(CONFIG_FILE);
        let json = serde_json::to_vec_pretty(self).map_err(|source| ConfigError::Malformed {
            path: path.clone(),
            source,
        })?;
        fs::write(&path, json).map_err(|source| ConfigError::Io { path, source })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_missing_is_default() {
        let dir = tempfile::TempDir::new().unwrap();
        assert_eq!(AppConfig::load(dir.path()).unwrap(), AppConfig::default());
        let cfg = AppConfig {
            vault_path: Some("/x/y".into()),
            ..Default::default()
        };
        cfg.save(dir.path()).unwrap();
        assert_eq!(AppConfig::load(dir.path()).unwrap(), cfg);
    }
}
