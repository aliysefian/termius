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
        };
        cfg.save(dir.path()).unwrap();
        assert_eq!(AppConfig::load(dir.path()).unwrap(), cfg);
    }
}
