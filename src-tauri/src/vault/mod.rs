//! Portable encrypted vault, format v2.
//!
//! A directory the user keeps in Dropbox, OneDrive, Google Drive, Nextcloud,
//! Syncthing or a NAS. Everything in it except `vault.json` is ciphertext.
//!
//! ```text
//! <vault>/
//!   vault.json                 key slots and parameters (no secrets, no inventory)
//!   <collection>/<uuid>.enc    one authenticated-encrypted record per object
//!   backups/<stamp>.enc        encrypted snapshots
//! ```
//!
//! Key hierarchy: master password → Argon2id → key-encryption key → unwraps
//! the random vault master key (VMK) → encrypts records. An optional
//! recovery key wraps the same VMK. See `docs/vault-architecture.md`.
//!
//! Records carry a revision, the revision they were based on, the device that
//! wrote them, a content hash and their previous version, so concurrent edits
//! from two computers are merged when they touch different fields and
//! reported as conflicts when they don't. Nothing is silently overwritten.

pub mod atomic;
pub mod backup;
pub mod conflicts;
pub mod devices;
pub mod format;
pub mod integrity;
pub mod legacy_v1;
pub mod merge;
pub mod migrate;

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::crypto::{self, CryptoError, KdfParams, MasterKey, RecoveryKey};

pub use format::{Manifest, SlotKind, FORMAT_VERSION, MANIFEST_FILE};

/// Extension of every encrypted record file.
pub const RECORD_EXT: &str = "enc";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictReason {
    /// Someone else saved a newer version of this record.
    ChangedElsewhere,
    /// Someone else deleted it.
    DeletedElsewhere,
    /// A record with this ID already exists.
    AlreadyExists,
}

/// Details of a refused save, for the UI to explain and resolve.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictError {
    pub collection: Collection,
    pub id: Uuid,
    pub reason: ConflictReason,
    /// Fields both sides changed differently. Empty when unknown.
    pub fields: Vec<String>,
    pub their_device: Uuid,
    pub their_rev: u64,
    pub their_updated_at: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    Crypto(#[from] CryptoError),
    #[error("JSON error for {context}: {source}")]
    Json {
        context: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("a vault already exists at {0}")]
    AlreadyExists(PathBuf),
    #[error("no vault found at {0}")]
    NotInitialized(PathBuf),
    #[error("vault metadata in {0} is invalid: {1}")]
    MalformedManifest(PathBuf, String),
    #[error("this vault uses format version {0}, which is newer than this app supports; update SSHVault")]
    UnsupportedManifestVersion(u32),
    #[error("master password is incorrect")]
    WrongPassword,
    #[error("recovery key is incorrect")]
    WrongRecoveryKey,
    #[error("the stored device key does not belong to this vault")]
    KeyMismatch,
    #[error("this vault has no recovery key")]
    NoRecoverySlot,
    #[error("this vault uses the old format and must be upgraded")]
    NeedsMigration,
    #[error("the vault is being upgraded on {device}; try again once that finishes")]
    MigrationInProgress { device: String },
    #[error("record {collection}/{id} not found")]
    NotFound { collection: Collection, id: Uuid },
    #[error("record {collection}/{id} has been deleted")]
    Deleted { collection: Collection, id: Uuid },
    #[error("record file {path} is not a valid encrypted record")]
    InvalidRecordFile { path: PathBuf },
    #[error("record {collection}/{id} failed its integrity check")]
    IdMismatch { collection: Collection, id: Uuid },
    #[error("changed on another device")]
    Conflict(Box<ConflictError>),
    #[error("backup error: {0}")]
    Backup(String),
}

impl VaultError {
    pub(crate) fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn json(context: impl Into<String>, source: serde_json::Error) -> Self {
        Self::Json {
            context: context.into(),
            source,
        }
    }
}

pub type Result<T> = std::result::Result<T, VaultError>;

// ---------------------------------------------------------------------------
// Collections
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Collection {
    Hosts,
    Identities,
    Snippets,
    Forwards,
    Groups,
    Keys,
    KnownHosts,
    Proxies,
    Workspaces,
    Settings,
    Devices,
    /// Advisory "open on this device" markers. Not backed up.
    Locks,
}

impl Collection {
    pub const ALL: [Collection; 12] = [
        Collection::Hosts,
        Collection::Identities,
        Collection::Snippets,
        Collection::Forwards,
        Collection::Groups,
        Collection::Keys,
        Collection::KnownHosts,
        Collection::Proxies,
        Collection::Workspaces,
        Collection::Settings,
        Collection::Devices,
        Collection::Locks,
    ];

    pub fn dir_name(self) -> &'static str {
        match self {
            Collection::Hosts => "hosts",
            Collection::Identities => "identities",
            Collection::Snippets => "snippets",
            Collection::Forwards => "forwards",
            Collection::Groups => "groups",
            Collection::Keys => "keys",
            Collection::KnownHosts => "known_hosts",
            Collection::Proxies => "proxies",
            Collection::Workspaces => "workspaces",
            Collection::Settings => "settings",
            Collection::Devices => "devices",
            Collection::Locks => "locks",
        }
    }

    pub fn from_dir_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.dir_name() == name)
    }

    /// Collections included in backups and conflict handling.
    pub fn is_synced_data(self) -> bool {
        !matches!(self, Collection::Locks)
    }
}

impl std::fmt::Display for Collection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.dir_name())
    }
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// The previous version kept inside a record, for three-way merges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Prev {
    pub rev: u64,
    pub deleted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// What is encrypted into each record file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Envelope {
    pub id: Uuid,
    pub rev: u64,
    pub base_rev: u64,
    pub device_id: Uuid,
    pub updated_at: u64,
    #[serde(default)]
    pub deleted: bool,
    pub content_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<Prev>,
}

pub(crate) fn content_hash(deleted: bool, data: Option<&Value>) -> String {
    let body = serde_json::to_vec(&(deleted, data)).unwrap_or_default();
    crypto::sha256_hex(&body)
}

/// A decrypted record as the rest of the app sees it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record<T> {
    pub id: Uuid,
    /// Revision, starting at 1. Saves send back the revision they edited.
    pub rev: u64,
    pub updated_at: u64,
    #[serde(default)]
    pub deleted: bool,
    /// The device that wrote this revision.
    pub device_id: Uuid,
    #[serde(default = "Option::default", skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> Record<T> {
    pub fn is_tombstone(&self) -> bool {
        self.deleted
    }
}

impl Envelope {
    fn to_record<T: DeserializeOwned>(&self, c: Collection) -> Result<Record<T>> {
        let data = match &self.data {
            Some(v) => Some(
                serde_json::from_value(v.clone())
                    .map_err(|e| VaultError::json(format!("{c}/{}", self.id), e))?,
            ),
            None => None,
        };
        Ok(Record {
            id: self.id,
            rev: self.rev,
            updated_at: self.updated_at,
            deleted: self.deleted,
            device_id: self.device_id,
            data,
        })
    }
}

/// What a save is based on, for optimistic concurrency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// A new record: fails if a live record with this ID exists.
    New,
    /// Edited from revision N: merged or refused if someone else saved since.
    Rev(u64),
    /// Overwrite whatever is current. Only for data this device owns (its own
    /// device record and lock) or an explicit user choice after a conflict.
    Latest,
}

/// A file that couldn't be loaded. Listing never fails because of one file.
#[derive(Debug)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub reason: VaultError,
}

#[derive(Debug)]
pub struct Listing<T> {
    pub records: Vec<Record<T>>,
    pub skipped: Vec<SkippedFile>,
    /// Sync-service conflicted copies found next to records. See `conflicts`.
    pub conflict_copies: Vec<PathBuf>,
}

// ---------------------------------------------------------------------------
// Devices and unlocking
// ---------------------------------------------------------------------------

/// This installation. The ID is generated once per device and kept in local
/// (unsynced) settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub id: Uuid,
    pub name: String,
}

impl DeviceInfo {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
        }
    }
}

/// How to unlock.
pub enum Unlock<'a> {
    Password(&'a [u8]),
    Recovery(&'a str),
    /// A VMK from the OS keychain ("remember on this device").
    Key(MasterKey),
}

#[derive(Default)]
pub struct CreateOptions {
    pub kdf: KdfParams,
    pub with_recovery: bool,
}

/// An unlocked vault. Dropping it zeroizes the key.
pub struct Vault {
    root: PathBuf,
    vmk: MasterKey,
    manifest: Manifest,
    device: DeviceInfo,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("root", &self.root)
            .field("vault_id", &self.manifest.vault_id)
            .finish_non_exhaustive()
    }
}

/// How long a temp file must be untouched before it counts as crash debris.
const STALE_TEMP: Duration = Duration::from_secs(3600);

impl Vault {
    /// Is there a vault (any version) at `root`?
    pub fn exists(root: impl AsRef<Path>) -> bool {
        root.as_ref().join(MANIFEST_FILE).is_file()
    }

    /// Create a vault. Returns the recovery key when one was requested: it
    /// is shown to the user once and never stored.
    pub fn create(
        root: impl Into<PathBuf>,
        password: &[u8],
        opts: CreateOptions,
        device: DeviceInfo,
    ) -> Result<(Self, Option<RecoveryKey>)> {
        let root = root.into();
        if Self::exists(&root) {
            return Err(VaultError::AlreadyExists(root));
        }
        atomic::create_private_dir(&root).map_err(|e| VaultError::io(&root, e))?;
        for c in Collection::ALL {
            let dir = root.join(c.dir_name());
            atomic::create_private_dir(&dir).map_err(|e| VaultError::io(&dir, e))?;
        }

        let vault_id = Uuid::new_v4();
        let vmk = MasterKey::generate();
        let mut slots = vec![format::new_slot(
            vault_id,
            SlotKind::Password,
            password,
            &vmk,
            opts.kdf,
        )?];
        let recovery = if opts.with_recovery {
            let rk = RecoveryKey::generate();
            slots.push(format::new_slot(
                vault_id,
                SlotKind::Recovery,
                rk.as_bytes(),
                &vmk,
                opts.kdf,
            )?);
            Some(rk)
        } else {
            None
        };
        let manifest = format::new_manifest(vault_id, &vmk, slots)?;

        // `create_new` guards against two machines creating at once.
        let path = root.join(MANIFEST_FILE);
        let json =
            serde_json::to_vec_pretty(&manifest).map_err(|e| VaultError::json("manifest", e))?;
        {
            use std::io::Write;
            let mut f = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| {
                    if e.kind() == io::ErrorKind::AlreadyExists {
                        VaultError::AlreadyExists(root.clone())
                    } else {
                        VaultError::io(&path, e)
                    }
                })?;
            f.write_all(&json)
                .and_then(|_| f.sync_all())
                .map_err(|e| VaultError::io(&path, e))?;
        }
        atomic::sync_dir(&root);

        Ok((
            Self {
                root,
                vmk,
                manifest,
                device,
            },
            recovery,
        ))
    }

    /// Unlock an existing v2 vault. A wrong secret fails without writing
    /// anything. v1 vaults return [`VaultError::NeedsMigration`]; use
    /// [`migrate::migrate_v1`].
    pub fn open(root: impl Into<PathBuf>, unlock: Unlock<'_>, device: DeviceInfo) -> Result<Self> {
        let root = root.into();
        migrate::check_journal(&root, &device)?;
        let manifest = match format::read(&root)? {
            format::OnDisk::V1 => return Err(VaultError::NeedsMigration),
            format::OnDisk::V2(m) => m,
        };
        let vmk = match unlock {
            Unlock::Password(pw) => {
                let slot = manifest
                    .slots
                    .iter()
                    .find(|s| s.kind == SlotKind::Password)
                    .expect("validated");
                format::open_slot(manifest.vault_id, slot, pw)?.ok_or(VaultError::WrongPassword)?
            }
            Unlock::Recovery(text) => {
                let slot = manifest
                    .slots
                    .iter()
                    .find(|s| s.kind == SlotKind::Recovery)
                    .ok_or(VaultError::NoRecoverySlot)?;
                let rk = RecoveryKey::parse(text).map_err(|_| VaultError::WrongRecoveryKey)?;
                format::open_slot(manifest.vault_id, slot, rk.as_bytes())?
                    .ok_or(VaultError::WrongRecoveryKey)?
            }
            Unlock::Key(k) => {
                if !format::verify_key(&manifest, &k) {
                    return Err(VaultError::KeyMismatch);
                }
                k
            }
        };
        // Make sure newer collection folders exist (another device may have
        // created the vault with an older build).
        for c in Collection::ALL {
            let dir = root.join(c.dir_name());
            if !dir.exists() {
                atomic::create_private_dir(&dir).map_err(|e| VaultError::io(&dir, e))?;
            }
            atomic::clean_stale_temps(&dir, STALE_TEMP);
        }
        atomic::clean_stale_temps(&root, STALE_TEMP);
        Ok(Self {
            root,
            vmk,
            manifest,
            device,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn vault_id(&self) -> Uuid {
        self.manifest.vault_id
    }

    pub fn device(&self) -> &DeviceInfo {
        &self.device
    }

    /// The VMK, for the OS keychain. Handle with care.
    pub fn master_key(&self) -> &MasterKey {
        &self.vmk
    }

    pub fn has_recovery(&self) -> bool {
        self.manifest
            .slots
            .iter()
            .any(|s| s.kind == SlotKind::Recovery)
    }

    /// Salt and parameters of the password slot, to re-check the password
    /// (e.g. before revealing a secret) without holding the vault.
    pub fn password_slot(&self) -> format::KeySlot {
        self.manifest
            .slots
            .iter()
            .find(|s| s.kind == SlotKind::Password)
            .cloned()
            .expect("validated")
    }

    /// Does `password` open this vault? Slow on purpose (Argon2id).
    pub fn check_password(&self, password: &[u8]) -> Result<bool> {
        Ok(
            format::open_slot(self.vault_id(), &self.password_slot(), password)?
                .is_some_and(|k| k.ct_eq(&self.vmk)),
        )
    }

    fn replace_slot(&mut self, kind: SlotKind, slot: Option<format::KeySlot>) -> Result<()> {
        let mut m = self.manifest.clone();
        m.slots.retain(|s| s.kind != kind);
        if let Some(s) = slot {
            m.slots.push(s);
        }
        format::write(&self.root, &m)?;
        self.manifest = m;
        Ok(())
    }

    /// Change the master password by rewrapping the VMK under a new salt.
    /// Records are untouched. The current password is checked first; if the
    /// write is interrupted, the old manifest (and so the old password)
    /// remains valid.
    pub fn change_password(&mut self, current: &[u8], new: &[u8], kdf: KdfParams) -> Result<()> {
        if !self.check_password(current)? {
            return Err(VaultError::WrongPassword);
        }
        self.set_password(new, kdf)
    }

    /// Set a new password without the old one. Only reachable after
    /// unlocking with the recovery key (see [`Vault::reset_password_with_recovery`]).
    fn set_password(&mut self, new: &[u8], kdf: KdfParams) -> Result<()> {
        let slot = format::new_slot(self.vault_id(), SlotKind::Password, new, &self.vmk, kdf)?;
        self.replace_slot(SlotKind::Password, Some(slot))
    }

    /// "Forgot master password": unlock with the recovery key and set a new
    /// password.
    pub fn reset_password_with_recovery(
        root: impl Into<PathBuf>,
        recovery: &str,
        new_password: &[u8],
        kdf: KdfParams,
        device: DeviceInfo,
    ) -> Result<Self> {
        let mut v = Self::open(root, Unlock::Recovery(recovery), device)?;
        v.set_password(new_password, kdf)?;
        Ok(v)
    }

    /// Create or replace the recovery key. The old one stops working.
    pub fn set_recovery(&mut self, kdf: KdfParams) -> Result<RecoveryKey> {
        let rk = RecoveryKey::generate();
        let slot = format::new_slot(
            self.vault_id(),
            SlotKind::Recovery,
            rk.as_bytes(),
            &self.vmk,
            kdf,
        )?;
        self.replace_slot(SlotKind::Recovery, Some(slot))?;
        Ok(rk)
    }

    pub fn remove_recovery(&mut self) -> Result<()> {
        self.replace_slot(SlotKind::Recovery, None)
    }

    // -- paths --------------------------------------------------------------

    pub fn collection_dir(&self, c: Collection) -> PathBuf {
        self.root.join(c.dir_name())
    }

    pub fn record_path(&self, c: Collection, id: Uuid) -> PathBuf {
        self.collection_dir(c)
            .join(format!("{}.{RECORD_EXT}", id.as_hyphenated()))
    }

    /// `<root>/<collection>/<uuid>.enc` → (collection, id). Anything else
    /// (temp files, conflicted copies, clutter) is `None`.
    pub fn parse_record_path(&self, path: &Path) -> Option<(Collection, Uuid)> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let mut parts = rel.components();
        let dir = parts.next()?.as_os_str().to_str()?;
        let file = parts.next()?.as_os_str().to_str()?;
        if parts.next().is_some() {
            return None;
        }
        let c = Collection::from_dir_name(dir)?;
        let id = Uuid::parse_str(file.strip_suffix(&format!(".{RECORD_EXT}"))?).ok()?;
        (file.len() == 36 + 1 + RECORD_EXT.len()).then_some((c, id))
    }

    pub(crate) fn aad(&self, c: Collection, id: Uuid) -> Vec<u8> {
        format!("sshvault/v2/{}/{c}/{}", self.vault_id(), id.as_hyphenated()).into_bytes()
    }

    // -- low-level record I/O -------------------------------------------------

    pub(crate) fn decode_envelope(
        &self,
        c: Collection,
        id: Uuid,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Envelope> {
        if !crypto::looks_like_envelope(bytes) {
            return Err(VaultError::InvalidRecordFile {
                path: path.to_path_buf(),
            });
        }
        let plain = Zeroizing::new(crypto::decrypt(&self.vmk, bytes, &self.aad(c, id))?);
        let env: Envelope =
            serde_json::from_slice(&plain).map_err(|e| VaultError::json(format!("{c}/{id}"), e))?;
        if env.id != id || env.content_hash != content_hash(env.deleted, env.data.as_ref()) {
            return Err(VaultError::IdMismatch { collection: c, id });
        }
        Ok(env)
    }

    pub(crate) fn read_envelope(&self, c: Collection, id: Uuid) -> Result<Option<Envelope>> {
        let path = self.record_path(c, id);
        match read_record_file(&path) {
            Ok(b) => self.decode_envelope(c, id, &path, &b).map(Some),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(VaultError::io(&path, e)),
        }
    }

    pub(crate) fn encode_envelope(&self, c: Collection, env: &Envelope) -> Result<Vec<u8>> {
        let plain = Zeroizing::new(
            serde_json::to_vec(env).map_err(|e| VaultError::json(format!("{c}/{}", env.id), e))?,
        );
        Ok(crypto::encrypt(&self.vmk, &plain, &self.aad(c, env.id))?)
    }

    pub(crate) fn write_envelope_at(
        &self,
        c: Collection,
        env: &Envelope,
        path: &Path,
    ) -> Result<()> {
        let bytes = self.encode_envelope(c, env)?;
        atomic::write(path, &bytes).map_err(|e| VaultError::io(path, e))
    }

    pub(crate) fn write_envelope(&self, c: Collection, env: &Envelope) -> Result<()> {
        self.write_envelope_at(c, env, &self.record_path(c, env.id))
    }

    pub(crate) fn new_envelope(
        &self,
        id: Uuid,
        rev: u64,
        base_rev: u64,
        data: Option<Value>,
        prev: Option<Prev>,
    ) -> Envelope {
        let deleted = data.is_none();
        Envelope {
            id,
            rev,
            base_rev,
            device_id: self.device.id,
            updated_at: now_ms(),
            deleted,
            content_hash: content_hash(deleted, data.as_ref()),
            data,
            prev,
        }
    }

    // -- writes ---------------------------------------------------------------

    /// Save `data` as record `id`, based on `base`. When another device saved
    /// in the meantime, non-overlapping field changes are merged; overlapping
    /// ones return [`VaultError::Conflict`] and nothing is written.
    pub fn put<T: Serialize + DeserializeOwned>(
        &self,
        c: Collection,
        id: Uuid,
        data: &T,
        base: Base,
    ) -> Result<Record<T>> {
        let ours =
            serde_json::to_value(data).map_err(|e| VaultError::json(format!("{c}/{id}"), e))?;
        let current = self.read_envelope(c, id)?;
        let conflict = |cur: &Envelope, reason, fields| {
            VaultError::Conflict(Box::new(ConflictError {
                collection: c,
                id,
                reason,
                fields,
                their_device: cur.device_id,
                their_rev: cur.rev,
                their_updated_at: cur.updated_at,
            }))
        };
        let prev_of = |cur: &Envelope| Prev {
            rev: cur.rev,
            deleted: cur.deleted,
            data: cur.data.clone(),
        };

        let (rev, base_rev, value, prev) = match (&current, base) {
            (None, _) => (1, 0, ours, None),
            (Some(cur), Base::Latest) => (cur.rev + 1, cur.rev, ours, Some(prev_of(cur))),
            (Some(cur), Base::New) if cur.deleted => (cur.rev + 1, cur.rev, ours, None),
            (Some(cur), Base::New) => {
                return Err(conflict(cur, ConflictReason::AlreadyExists, vec![]))
            }
            (Some(cur), Base::Rev(b)) if cur.rev == b => {
                if cur.deleted {
                    // Resurrecting something deleted from this exact revision.
                    (b + 1, b, ours, None)
                } else {
                    (b + 1, b, ours, Some(prev_of(cur)))
                }
            }
            (Some(cur), Base::Rev(b)) => {
                // Someone else moved it on (or the file went backwards).
                if !cur.deleted && cur.data.as_ref() == Some(&ours) {
                    return cur.to_record(c);
                }
                if cur.deleted {
                    return Err(conflict(cur, ConflictReason::DeletedElsewhere, vec![]));
                }
                let base_data = cur
                    .prev
                    .as_ref()
                    .filter(|p| p.rev == b && !p.deleted)
                    .and_then(|p| p.data.clone());
                let Some(base_data) = base_data else {
                    return Err(conflict(cur, ConflictReason::ChangedElsewhere, vec![]));
                };
                let theirs = cur.data.clone().unwrap_or(Value::Null);
                match merge::merge3(&base_data, &ours, &theirs) {
                    Ok(merged) => (cur.rev + 1, cur.rev, merged, Some(prev_of(cur))),
                    Err(fields) => {
                        return Err(conflict(cur, ConflictReason::ChangedElsewhere, fields))
                    }
                }
            }
        };
        let env = self.new_envelope(id, rev, base_rev, Some(value), prev);
        self.write_envelope(c, &env)?;
        env.to_record(c)
    }

    /// New record with a fresh UUID.
    pub fn insert<T: Serialize + DeserializeOwned>(
        &self,
        c: Collection,
        data: &T,
    ) -> Result<Record<T>> {
        self.put(c, Uuid::new_v4(), data, Base::New)
    }

    /// Delete by writing a tombstone, so offline devices learn about it.
    /// Refused if someone else changed the record since `base`. The
    /// tombstone keeps no data, so deleted secrets don't linger.
    pub fn delete(&self, c: Collection, id: Uuid, base: Base) -> Result<()> {
        let Some(cur) = self.read_envelope(c, id)? else {
            return Err(VaultError::NotFound { collection: c, id });
        };
        if cur.deleted {
            return Ok(());
        }
        if let Base::Rev(b) = base {
            if cur.rev != b {
                return Err(VaultError::Conflict(Box::new(ConflictError {
                    collection: c,
                    id,
                    reason: ConflictReason::ChangedElsewhere,
                    fields: vec![],
                    their_device: cur.device_id,
                    their_rev: cur.rev,
                    their_updated_at: cur.updated_at,
                })));
            }
        }
        let prev = Prev {
            rev: cur.rev,
            deleted: false,
            data: None,
        };
        let env = self.new_envelope(id, cur.rev + 1, cur.rev, None, Some(prev));
        self.write_envelope(c, &env)
    }

    /// Physically remove a file (used for this device's lock, and tombstone
    /// purging). Not for normal deletes.
    pub(crate) fn remove_file(&self, c: Collection, id: Uuid) -> Result<()> {
        let p = self.record_path(c, id);
        match fs::remove_file(&p) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(VaultError::io(&p, e)),
        }
    }

    /// Remove tombstones older than `max_age_ms`. A device offline for longer
    /// could resurrect such a record, so keep this generous.
    pub fn purge_tombstones(&self, c: Collection, max_age_ms: u64) -> Result<usize> {
        let cutoff = now_ms().saturating_sub(max_age_ms);
        let mut n = 0;
        for r in self.list_with_tombstones::<Value>(c)?.records {
            if r.deleted && r.updated_at <= cutoff {
                self.remove_file(c, r.id)?;
                n += 1;
            }
        }
        Ok(n)
    }

    // -- reads ----------------------------------------------------------------

    pub fn get<T: DeserializeOwned>(&self, c: Collection, id: Uuid) -> Result<Record<T>> {
        let r = self.get_raw(c, id)?;
        if r.deleted {
            return Err(VaultError::Deleted { collection: c, id });
        }
        Ok(r)
    }

    pub fn get_raw<T: DeserializeOwned>(&self, c: Collection, id: Uuid) -> Result<Record<T>> {
        self.read_envelope(c, id)?
            .ok_or(VaultError::NotFound { collection: c, id })?
            .to_record(c)
    }

    /// Decrypt bytes the caller already read (the file watcher uses this).
    pub fn decode_record<T: DeserializeOwned>(
        &self,
        c: Collection,
        id: Uuid,
        path: &Path,
        bytes: &[u8],
    ) -> Result<Record<T>> {
        self.decode_envelope(c, id, path, bytes)?.to_record(c)
    }

    pub fn list<T: DeserializeOwned>(&self, c: Collection) -> Result<Listing<T>> {
        self.list_inner(c, false)
    }

    pub fn list_with_tombstones<T: DeserializeOwned>(&self, c: Collection) -> Result<Listing<T>> {
        self.list_inner(c, true)
    }

    fn list_inner<T: DeserializeOwned>(
        &self,
        c: Collection,
        tombstones: bool,
    ) -> Result<Listing<T>> {
        let dir = self.collection_dir(c);
        let mut out = Listing {
            records: Vec::new(),
            skipped: Vec::new(),
            conflict_copies: Vec::new(),
        };
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(out),
            Err(e) => return Err(VaultError::io(&dir, e)),
        };
        for entry in entries {
            let entry = entry.map_err(|e| VaultError::io(&dir, e))?;
            let path = entry.path();
            let Some((_, id)) = self.parse_record_path(&path) else {
                if conflicts::copy_id(&path).is_some() {
                    out.conflict_copies.push(path);
                }
                continue;
            };
            match read_record_file(&path)
                .map_err(|e| VaultError::io(&path, e))
                .and_then(|b| self.decode_record::<T>(c, id, &path, &b))
            {
                Ok(r) if r.deleted && !tombstones => {}
                Ok(r) => out.records.push(r),
                Err(reason) => out.skipped.push(SkippedFile { path, reason }),
            }
        }
        out.records
            .sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        Ok(out)
    }

    /// A short fingerprint of the whole vault state: every live record's ID
    /// and revision, hashed. Two devices showing the same value have the same
    /// data.
    pub fn state_hash(&self) -> Result<(String, usize, u64)> {
        let mut parts = Vec::new();
        let mut last = 0;
        for c in Collection::ALL.into_iter().filter(|c| c.is_synced_data()) {
            for r in self.list_with_tombstones::<Value>(c)?.records {
                last = last.max(r.updated_at);
                parts.push(format!("{c}/{}/{}", r.id, r.rev));
            }
        }
        parts.sort();
        Ok((
            crypto::sha256_hex(parts.join("\n").as_bytes())[..16].to_string(),
            parts.len(),
            last,
        ))
    }
}

/// Encode a VMK for the OS keychain.
/// Largest record file accepted. Real records are a few KB; the cap stops a
/// hostile or broken sync folder from exhausting memory.
const MAX_RECORD_BYTES: u64 = 16 * 1024 * 1024;

/// Read a record file, refusing anything but a regular file of sane size.
/// Symlinks are never followed: a link planted in the synced folder can't
/// point the app at `/dev/zero` or at files outside the vault.
fn read_record_file(path: &Path) -> io::Result<Vec<u8>> {
    read_regular_file(path, MAX_RECORD_BYTES)
}

pub(crate) fn read_regular_file(path: &Path, max: u64) -> io::Result<Vec<u8>> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a regular file"));
    }
    if meta.len() > max {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file is too large"));
    }
    fs::read(path)
}

pub fn encode_key(k: &MasterKey) -> Zeroizing<String> {
    Zeroizing::new(B64.encode(k.as_bytes()))
}

pub fn decode_key(s: &str) -> Option<MasterKey> {
    let raw = Zeroizing::new(B64.decode(s.trim()).ok()?);
    let bytes: [u8; crypto::KEY_LEN] = raw.as_slice().try_into().ok()?;
    Some(MasterKey::from_bytes(bytes))
}

/// Current Unix time in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    pub fn kdf() -> KdfParams {
        KdfParams::insecure_for_tests()
    }

    pub fn opts(recovery: bool) -> CreateOptions {
        CreateOptions {
            kdf: kdf(),
            with_recovery: recovery,
        }
    }

    /// A fresh vault in a temp dir, as "PC A".
    pub fn new_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::TempDir::new().unwrap();
        let (v, _) = Vault::create(
            dir.path().join("vault"),
            b"hunter2",
            opts(false),
            DeviceInfo::new("PC-A"),
        )
        .unwrap();
        (dir, v)
    }

    /// Open the same folder as another device.
    pub fn second_device(v: &Vault, name: &str) -> Vault {
        Vault::open(
            v.root(),
            Unlock::Password(b"hunter2"),
            DeviceInfo::new(name),
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests;
