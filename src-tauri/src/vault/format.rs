//! `vault.json`: the only plaintext file. It holds what's needed to *try* to
//! unlock the vault, and nothing about its contents.
//!
//! ```json
//! {
//!   "format": "sshvault",
//!   "version": 2,
//!   "vault_id": "…",
//!   "created_at": 1790000000000,
//!   "cipher": "xchacha20-poly1305",
//!   "slots": [
//!     { "kind": "password", "kdf": { "algorithm": "argon2id", "version": 19,
//!       "salt": "…", "m_cost_kib": 65536, "t_cost": 3, "p_cost": 4 },
//!       "wrapped_key": "…", "created_at": … },
//!     { "kind": "recovery", … }
//!   ],
//!   "key_check": "…"
//! }
//! ```
//!
//! Each slot wraps the same random vault master key (VMK) under a key
//! derived from its secret. The wrap's associated data binds the vault ID,
//! the slot kind and every KDF parameter, so weakening the parameters or
//! swapping slots between vaults makes unwrapping fail instead of silently
//! using a cheaper KDF. `key_check` is a constant encrypted with the VMK, used
//! to confirm a VMK read from the OS keychain belongs to this vault.

use std::path::{Path, PathBuf};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{self, CryptoError, KdfParams, MasterKey, SALT_LEN};

use super::{atomic, now_ms, VaultError};

pub const MANIFEST_FILE: &str = "vault.json";
pub const FORMAT_ID: &str = "sshvault";
/// The format this build reads and writes.
pub const FORMAT_VERSION: u32 = 2;
pub const CIPHER: &str = "xchacha20-poly1305";
pub const KDF_ALGORITHM: &str = "argon2id";
/// Argon2 version 1.3.
pub const ARGON2_VERSION: u32 = 0x13;
const KEY_CHECK_PLAINTEXT: &[u8] = b"sshvault-vmk-check-v2";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Password,
    Recovery,
}

impl SlotKind {
    fn as_str(self) -> &'static str {
        match self {
            SlotKind::Password => "password",
            SlotKind::Recovery => "recovery",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfSpec {
    pub algorithm: String,
    pub version: u32,
    pub salt: String,
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

impl KdfSpec {
    fn new(params: KdfParams) -> Self {
        Self {
            algorithm: KDF_ALGORITHM.into(),
            version: ARGON2_VERSION,
            salt: B64.encode(crypto::generate_salt()),
            m_cost_kib: params.m_cost_kib,
            t_cost: params.t_cost,
            p_cost: params.p_cost,
        }
    }

    pub fn params(&self) -> KdfParams {
        KdfParams {
            m_cost_kib: self.m_cost_kib,
            t_cost: self.t_cost,
            p_cost: self.p_cost,
        }
    }

    fn salt_bytes(&self) -> Result<[u8; SALT_LEN], VaultError> {
        B64.decode(&self.salt)
            .ok()
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| malformed("KDF salt is not 16 bytes of base64"))
    }

    fn validate(&self) -> Result<(), VaultError> {
        if self.algorithm != KDF_ALGORITHM || self.version != ARGON2_VERSION {
            return Err(malformed(format!(
                "unsupported KDF {} v{}",
                self.algorithm, self.version
            )));
        }
        // Floors that stop a tampered manifest from making a slot cheap to
        // brute-force. (A lower value also fails the AAD check below; this
        // gives a clearer error.) Test builds use tiny parameters.
        #[cfg(not(test))]
        if self.m_cost_kib < 19 * 1024 || self.t_cost < 2 {
            return Err(malformed("KDF parameters are below the safe minimum"));
        }
        self.salt_bytes().map(|_| ())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeySlot {
    pub kind: SlotKind,
    pub kdf: KdfSpec,
    pub wrapped_key: String,
    pub created_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub vault_id: Uuid,
    pub created_at: u64,
    pub cipher: String,
    pub slots: Vec<KeySlot>,
    pub key_check: String,
}

fn malformed(msg: impl Into<String>) -> VaultError {
    VaultError::MalformedManifest(PathBuf::from(MANIFEST_FILE), msg.into())
}

fn slot_aad(vault_id: Uuid, kind: SlotKind, kdf: &KdfSpec) -> Vec<u8> {
    format!(
        "sshvault/v{FORMAT_VERSION}/slot/{vault_id}/{}/{}/{}/{}/{}/{}/{}",
        kind.as_str(),
        kdf.algorithm,
        kdf.version,
        kdf.salt,
        kdf.m_cost_kib,
        kdf.t_cost,
        kdf.p_cost
    )
    .into_bytes()
}

fn key_check_aad(vault_id: Uuid) -> Vec<u8> {
    format!("sshvault/v{FORMAT_VERSION}/key-check/{vault_id}").into_bytes()
}

/// What `vault.json` says, before any key is involved.
#[derive(Debug)]
pub enum OnDisk {
    /// The original format: password-derived record key, no envelope.
    V1,
    V2(Manifest),
}

/// Read and validate `vault.json`. Never writes.
pub fn read(root: &Path) -> Result<OnDisk, VaultError> {
    let path = root.join(MANIFEST_FILE);
    let bytes = match super::read_regular_file(&path, 1024 * 1024) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(VaultError::NotInitialized(root.to_path_buf()))
        }
        Err(e) => return Err(VaultError::io(&path, e)),
    };
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|e| VaultError::MalformedManifest(path.clone(), e.to_string()))?;
    let version = value
        .get("version")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| malformed("missing version"))? as u32;
    match version {
        1 => Ok(OnDisk::V1),
        FORMAT_VERSION => {
            let m: Manifest = serde_json::from_value(value)
                .map_err(|e| VaultError::MalformedManifest(path.clone(), e.to_string()))?;
            validate(&m)?;
            Ok(OnDisk::V2(m))
        }
        // A newer app wrote this. Refuse to read *or* write it.
        v => Err(VaultError::UnsupportedManifestVersion(v)),
    }
}

fn validate(m: &Manifest) -> Result<(), VaultError> {
    if m.format != FORMAT_ID {
        return Err(malformed(format!(
            "not an SSHVault vault (format \"{}\")",
            m.format
        )));
    }
    if m.cipher != CIPHER {
        return Err(malformed(format!("unsupported cipher {}", m.cipher)));
    }
    if m.slots
        .iter()
        .filter(|s| s.kind == SlotKind::Password)
        .count()
        != 1
    {
        return Err(malformed("expected exactly one password slot"));
    }
    if m.slots
        .iter()
        .filter(|s| s.kind == SlotKind::Recovery)
        .count()
        > 1
    {
        return Err(malformed("more than one recovery slot"));
    }
    for s in &m.slots {
        s.kdf.validate()?;
        B64.decode(&s.wrapped_key)
            .map_err(|_| malformed("wrapped key is not base64"))?;
    }
    B64.decode(&m.key_check)
        .map_err(|_| malformed("key check is not base64"))?;
    Ok(())
}

/// Build a slot that wraps `vmk` under `secret`.
pub fn new_slot(
    vault_id: Uuid,
    kind: SlotKind,
    secret: &[u8],
    vmk: &MasterKey,
    params: KdfParams,
) -> Result<KeySlot, VaultError> {
    let kdf = KdfSpec::new(params);
    let kek = crypto::derive_key(secret, &kdf.salt_bytes()?, params)?;
    let wrapped = crypto::wrap_key(&kek, vmk, &slot_aad(vault_id, kind, &kdf))?;
    Ok(KeySlot {
        kind,
        kdf,
        wrapped_key: B64.encode(wrapped),
        created_at: now_ms(),
    })
}

/// Try `secret` against a slot. `None` means wrong secret (or tampered slot).
pub fn open_slot(
    vault_id: Uuid,
    slot: &KeySlot,
    secret: &[u8],
) -> Result<Option<MasterKey>, VaultError> {
    let kek = crypto::derive_key(secret, &slot.kdf.salt_bytes()?, slot.kdf.params())?;
    let wrapped = B64
        .decode(&slot.wrapped_key)
        .map_err(|_| malformed("wrapped key is not base64"))?;
    match crypto::unwrap_key(&kek, &wrapped, &slot_aad(vault_id, slot.kind, &slot.kdf)) {
        Ok(k) => Ok(Some(k)),
        Err(CryptoError::AuthenticationFailed) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn key_check(vault_id: Uuid, vmk: &MasterKey) -> Result<String, VaultError> {
    Ok(B64.encode(crypto::encrypt(
        vmk,
        KEY_CHECK_PLAINTEXT,
        &key_check_aad(vault_id),
    )?))
}

/// Does `vmk` belong to the vault described by `m`?
pub fn verify_key(m: &Manifest, vmk: &MasterKey) -> bool {
    B64.decode(&m.key_check)
        .ok()
        .and_then(|ct| crypto::decrypt(vmk, &ct, &key_check_aad(m.vault_id)).ok())
        .is_some_and(|pt| pt == KEY_CHECK_PLAINTEXT)
}

pub fn new_manifest(
    vault_id: Uuid,
    vmk: &MasterKey,
    slots: Vec<KeySlot>,
) -> Result<Manifest, VaultError> {
    Ok(Manifest {
        format: FORMAT_ID.into(),
        version: FORMAT_VERSION,
        vault_id,
        created_at: now_ms(),
        cipher: CIPHER.into(),
        slots,
        key_check: key_check(vault_id, vmk)?,
    })
}

/// Atomically replace `vault.json`.
pub fn write(root: &Path, m: &Manifest) -> Result<(), VaultError> {
    let path = root.join(MANIFEST_FILE);
    let json = serde_json::to_vec_pretty(m).map_err(|e| VaultError::json("manifest", e))?;
    atomic::write(&path, &json).map_err(|e| VaultError::io(&path, e))
}
