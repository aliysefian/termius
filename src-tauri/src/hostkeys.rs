//! Trusted server host keys, kept in the vault so every device agrees on
//! which keys are genuine.
//!
//! One record per `host:port`. Its ID is an HMAC of the host under a key
//! derived from the vault master key, so two devices trusting the same host
//! converge on the same record (and a conflict, not a duplicate, if they
//! disagree), while the file name reveals nothing about the host.

use std::path::Path;

use hmac::{Hmac, Mac};
use serde::Serialize;
use sha2::Sha256;
use uuid::Uuid;

use crate::models::{KnownHost, ReplacedKey};
use crate::ssh::{HostKeyStatus, PresentedKey, PreviousKey};
use crate::vault::{now_ms, Base, Collection, Result, Vault};

/// History entries kept per host.
const HISTORY: usize = 10;

fn normalize(host: &str) -> String {
    host.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Deterministic, secret-keyed record ID for `host:port`.
pub fn record_id(vault: &Vault, host: &str, port: u16) -> Uuid {
    let mut sub = Hmac::<Sha256>::new_from_slice(vault.master_key().as_bytes()).expect("any key length");
    sub.update(b"sshvault/v2/subkey/known-host-id");
    let sub = sub.finalize().into_bytes();
    let mut mac = Hmac::<Sha256>::new_from_slice(&sub).expect("any key length");
    mac.update(format!("{}\0{port}", normalize(host)).as_bytes());
    let digest = mac.finalize().into_bytes();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    uuid::Builder::from_random_bytes(bytes).into_uuid()
}

pub fn get(vault: &Vault, host: &str, port: u16) -> Result<Option<(u64, KnownHost)>> {
    let id = record_id(vault, host, port);
    match vault.get::<KnownHost>(Collection::KnownHosts, id) {
        Ok(r) => Ok(r.data.map(|d| (r.rev, d))),
        Err(crate::vault::VaultError::NotFound { .. } | crate::vault::VaultError::Deleted { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn check(vault: &Vault, key: &PresentedKey) -> Result<HostKeyStatus> {
    Ok(match get(vault, &key.host, key.port)? {
        None => HostKeyStatus::Unknown,
        Some((_, k)) if k.public_key.split_whitespace().take(2).eq(key.public_key.split_whitespace().take(2)) => {
            HostKeyStatus::Trusted
        }
        Some((_, k)) => HostKeyStatus::Changed {
            previous: PreviousKey {
                algorithm: k.algorithm,
                fingerprint: k.fingerprint,
            },
        },
    })
}

/// Trust `key` for its host. A different earlier key moves into history.
pub fn trust(vault: &Vault, key: &PresentedKey) -> Result<()> {
    let id = record_id(vault, &key.host, key.port);
    let device = vault.device().name.clone();
    let (base, history, note) = match get(vault, &key.host, key.port)? {
        Some((rev, old)) => {
            if old.fingerprint == key.fingerprint {
                return Ok(());
            }
            let mut history = old.history;
            history.insert(
                0,
                ReplacedKey {
                    algorithm: old.algorithm,
                    fingerprint: old.fingerprint,
                    replaced_at: now_ms(),
                    replaced_by: device.clone(),
                },
            );
            history.truncate(HISTORY);
            (Base::Rev(rev), history, old.note)
        }
        None => (Base::New, Vec::new(), String::new()),
    };
    let record = KnownHost {
        host: normalize(&key.host),
        port: key.port,
        algorithm: key.algorithm.clone(),
        public_key: key.public_key.clone(),
        fingerprint: key.fingerprint.clone(),
        trusted_at: now_ms(),
        trusted_by: device,
        history,
        note,
    };
    match vault.put(Collection::KnownHosts, id, &record, base) {
        Ok(_) => Ok(()),
        // Another device trusted the very same key meanwhile: nothing to do.
        Err(e @ crate::vault::VaultError::Conflict(_)) => match get(vault, &key.host, key.port)? {
            Some((_, k)) if k.fingerprint == key.fingerprint => Ok(()),
            _ => Err(e),
        },
        Err(e) => Err(e),
    }
}

pub fn forget(vault: &Vault, host: &str, port: u16) -> Result<bool> {
    if get(vault, host, port)?.is_none() {
        return Ok(false);
    }
    vault.delete(Collection::KnownHosts, record_id(vault, host, port), Base::Latest)?;
    Ok(true)
}

#[derive(Debug, Default, Clone, Serialize, PartialEq, Eq)]
pub struct ImportReport {
    pub added: usize,
    /// Already trusted (same or a different key; existing entries win).
    pub existing: usize,
    /// Hashed host names can't be mapped back to a host.
    pub hashed: usize,
    pub invalid: usize,
}

/// Copy entries from an OpenSSH `known_hosts` file. Entries the vault
/// already has are left alone, so an old local file can never override a
/// key that was deliberately replaced.
pub fn import_file(vault: &Vault, path: &Path) -> std::io::Result<Result<ImportReport>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Ok(ImportReport::default())),
        Err(e) => return Err(e),
    };
    Ok(import_text(vault, &text))
}

pub fn import_text(vault: &Vault, text: &str) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('@') {
            continue;
        }
        let mut cols = line.split_whitespace();
        let (Some(hosts), Some(alg), Some(b64)) = (cols.next(), cols.next(), cols.next()) else {
            report.invalid += 1;
            continue;
        };
        if hosts.starts_with("|1|") {
            report.hashed += 1;
            continue;
        }
        let Ok(pk) = russh::keys::ssh_key::PublicKey::from_openssh(&format!("{alg} {b64}")) else {
            report.invalid += 1;
            continue;
        };
        for token in hosts.split(',') {
            let (host, port) = parse_token(token);
            // Wildcard patterns aren't hosts.
            if host.contains(['*', '?', '!']) {
                continue;
            }
            let Ok(key) = PresentedKey::new(host, port, &pk) else {
                report.invalid += 1;
                continue;
            };
            if get(vault, host, port)?.is_some() {
                report.existing += 1;
                continue;
            }
            trust(vault, &key)?;
            report.added += 1;
        }
    }
    Ok(report)
}

fn parse_token(token: &str) -> (&str, u16) {
    if let Some(rest) = token.strip_prefix('[') {
        if let Some((host, port)) = rest.split_once("]:") {
            if let Ok(p) = port.parse() {
                return (host, p);
            }
        }
    }
    (token, 22)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{generate, KeyAlgorithm};
    use crate::vault::testutil::{new_vault, second_device};

    fn presented(host: &str, port: u16) -> PresentedKey {
        let m = generate(KeyAlgorithm::Ed25519, "", None).unwrap();
        let pk = russh::keys::ssh_key::PublicKey::from_openssh(&m.public_key).unwrap();
        PresentedKey::new(host, port, &pk).unwrap()
    }

    #[test]
    fn unknown_then_trusted_then_changed_with_history() {
        let (_d, v) = new_vault();
        let k1 = presented("Web-01.example.com", 22);
        assert_eq!(check(&v, &k1).unwrap(), HostKeyStatus::Unknown);
        trust(&v, &k1).unwrap();
        assert_eq!(check(&v, &k1).unwrap(), HostKeyStatus::Trusted);
        // Case-insensitive host, but the port matters.
        let mut lower = k1.clone();
        lower.host = "web-01.example.com".into();
        assert_eq!(check(&v, &lower).unwrap(), HostKeyStatus::Trusted);
        let mut other_port = k1.clone();
        other_port.port = 2222;
        assert_eq!(check(&v, &other_port).unwrap(), HostKeyStatus::Unknown);

        let k2 = presented("web-01.example.com", 22);
        match check(&v, &k2).unwrap() {
            HostKeyStatus::Changed { previous } => assert_eq!(previous.fingerprint, k1.fingerprint),
            s => panic!("{s:?}"),
        }
        trust(&v, &k2).unwrap();
        let (_, rec) = get(&v, "web-01.example.com", 22).unwrap().unwrap();
        assert_eq!(rec.fingerprint, k2.fingerprint);
        assert_eq!(rec.history[0].fingerprint, k1.fingerprint);
        assert!(forget(&v, "web-01.example.com", 22).unwrap());
        assert_eq!(check(&v, &k2).unwrap(), HostKeyStatus::Unknown);
    }

    #[test]
    fn devices_share_trust_and_ids_hide_hostnames() {
        let (_d, a) = new_vault();
        let b = second_device(&a, "PC-B");
        let k = presented("db-01.internal", 22);
        trust(&a, &k).unwrap();
        assert_eq!(check(&b, &k).unwrap(), HostKeyStatus::Trusted);
        // Both devices trusting the same key concurrently is not an error.
        trust(&b, &k).unwrap();
        let id = record_id(&a, "db-01.internal", 22);
        assert_eq!(id, record_id(&b, "db-01.internal", 22));
        let file = a.record_path(Collection::KnownHosts, id);
        assert!(!file.to_string_lossy().contains("db-01"));
        let raw = std::fs::read(file).unwrap();
        assert!(!raw.windows(5).any(|w| w == b"db-01"));
    }

    #[test]
    fn imports_openssh_known_hosts_without_overriding() {
        let (_d, v) = new_vault();
        let k = presented("a.example", 22);
        trust(&v, &k).unwrap();
        let other = presented("x", 22);
        let text = format!(
            "# comment\na.example,[b.example]:2222 {pk}\n|1|abc=|def= {pk}\n*.wild {pk}\nbroken line\n",
            pk = other.public_key
        );
        let r = import_text(&v, &text).unwrap();
        assert_eq!(r, ImportReport { added: 1, existing: 1, hashed: 1, invalid: 1 });
        // The existing (deliberately trusted) key was not replaced.
        assert_eq!(check(&v, &k).unwrap(), HostKeyStatus::Trusted);
        let mut b = other.clone();
        b.host = "b.example".into();
        b.port = 2222;
        assert_eq!(check(&v, &b).unwrap(), HostKeyStatus::Trusted);
    }
}
