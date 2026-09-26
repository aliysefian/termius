//! The sync-safe vault: a directory of individually encrypted, UUID-named
//! records that a file-sync service (Dropbox, Nextcloud, Syncthing, ...) can
//! replicate between machines without producing conflicted copies.
//!
//! Layout inside the user-chosen root:
//!
//! ```text
//! <root>/
//!   vault.json              plaintext manifest: salt, KDF params, key verifier
//!   hosts/<uuid>.enc        one encrypted record per host
//!   identities/<uuid>.enc   one encrypted record per identity
//!   snippets/<uuid>.enc     one encrypted record per snippet
//!   forwards/<uuid>.enc     one encrypted record per port-forwarding rule
//! ```
//!
//! Design rules that keep sync conflict-free:
//!
//! * **One file per record.** Two machines editing different records touch
//!   different files, so the sync service never has to merge anything.
//! * **Last-writer-wins per record.** Every record carries `updated_at`
//!   (Unix milliseconds). When two machines edit the *same* record while
//!   offline, the sync service picks one file; readers always trust the file
//!   on disk, so both sides converge. Optional conflicted copies are ignored
//!   because their filenames are not bare UUIDs.
//! * **Tombstones instead of deletes.** Deleting writes a tiny `deleted: true`
//!   record rather than removing the file, so a peer that was offline learns
//!   about the deletion instead of re-uploading its stale copy.
//! * **Atomic writes.** Records are written to a temporary file and renamed
//!   into place, so the sync client never uploads a half-written file.
//! * **Path-bound AEAD.** `"<collection>/<uuid>"` is the associated data, so a
//!   ciphertext cannot be moved, renamed, or swapped between collections.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::crypto::{self, CryptoError, KdfParams, MasterKey, SALT_LEN};

/// Name of the plaintext manifest at the vault root.
pub const MANIFEST_FILE: &str = "vault.json";
/// Extension of every encrypted record file.
pub const RECORD_EXT: &str = "enc";
const MANIFEST_VERSION: u32 = 1;
const VERIFIER_PLAINTEXT: &[u8] = b"sshvault-key-verifier-v1";
const VERIFIER_AAD: &[u8] = b"manifest/verifier";

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

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

    #[error("no vault manifest found at {0}")]
    NotInitialized(PathBuf),

    #[error("manifest at {0} is malformed: {1}")]
    MalformedManifest(PathBuf, String),

    #[error("unsupported manifest version {0}")]
    UnsupportedManifestVersion(u32),

    #[error("master password is incorrect")]
    WrongPassword,

    #[error("record {collection}/{id} not found")]
    NotFound { collection: Collection, id: Uuid },

    #[error("record {collection}/{id} has been deleted")]
    Deleted { collection: Collection, id: Uuid },

    #[error("record file {path} is not a valid encrypted record")]
    InvalidRecordFile { path: PathBuf },

    #[error("record body of {collection}/{id} does not match the requested id")]
    IdMismatch { collection: Collection, id: Uuid },
}

impl VaultError {
    fn io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    fn json(context: impl Into<String>, source: serde_json::Error) -> Self {
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

/// The record kinds stored in the vault, each in its own subdirectory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Collection {
    Hosts,
    Identities,
    Snippets,
    Forwards,
}

impl Collection {
    pub const ALL: [Collection; 4] = [
        Collection::Hosts,
        Collection::Identities,
        Collection::Snippets,
        Collection::Forwards,
    ];

    /// Subdirectory name under the vault root.
    pub fn dir_name(self) -> &'static str {
        match self {
            Collection::Hosts => "hosts",
            Collection::Identities => "identities",
            Collection::Snippets => "snippets",
            Collection::Forwards => "forwards",
        }
    }

    pub fn from_dir_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.dir_name() == name)
    }
}

impl std::fmt::Display for Collection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.dir_name())
    }
}

// ---------------------------------------------------------------------------
// Manifest (plaintext, contains no secrets)
// ---------------------------------------------------------------------------

/// The only unencrypted file in the vault. Holds what every machine needs to
/// derive the same key: the salt and KDF cost parameters. The verifier lets a
/// client confirm the password locally before touching any record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    /// Unique id of this vault, useful for the UI and for caching salts.
    pub vault_id: Uuid,
    /// Argon2id salt, base64.
    pub salt: String,
    pub kdf: KdfParams,
    /// `encrypt(key, VERIFIER_PLAINTEXT, VERIFIER_AAD)`, base64.
    pub verifier: String,
    pub created_at: u64,
}

impl Manifest {
    fn salt_bytes(&self) -> Result<[u8; SALT_LEN]> {
        let raw = B64.decode(&self.salt).map_err(|e| {
            VaultError::MalformedManifest(MANIFEST_FILE.into(), format!("salt: {e}"))
        })?;
        raw.try_into().map_err(|_| {
            VaultError::MalformedManifest(MANIFEST_FILE.into(), "salt has wrong length".into())
        })
    }

    fn verifier_bytes(&self) -> Result<Vec<u8>> {
        B64.decode(&self.verifier).map_err(|e| {
            VaultError::MalformedManifest(MANIFEST_FILE.into(), format!("verifier: {e}"))
        })
    }
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// Metadata wrapper around every stored payload. This is what gets serialized,
/// encrypted, and written to `<collection>/<id>.enc`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record<T> {
    pub id: Uuid,
    /// Unix time in milliseconds of the last write. Drives last-writer-wins.
    pub updated_at: u64,
    /// `true` for a tombstone; `data` is then `None`.
    #[serde(default)]
    pub deleted: bool,
    #[serde(default = "Option::default", skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
}

impl<T> Record<T> {
    pub fn is_tombstone(&self) -> bool {
        self.deleted
    }
}

/// A file inside a collection directory that could not be loaded. Listing
/// never fails as a whole because of one bad file; instead these are reported.
#[derive(Debug)]
pub struct SkippedFile {
    pub path: PathBuf,
    pub reason: VaultError,
}

/// Result of listing a collection.
#[derive(Debug)]
pub struct Listing<T> {
    pub records: Vec<Record<T>>,
    pub skipped: Vec<SkippedFile>,
}

// ---------------------------------------------------------------------------
// Vault
// ---------------------------------------------------------------------------

/// An unlocked vault. Holds the master key in memory; dropping it zeroizes the key.
pub struct Vault {
    root: PathBuf,
    key: MasterKey,
    manifest: Manifest,
}

impl std::fmt::Debug for Vault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vault")
            .field("root", &self.root)
            .field("vault_id", &self.manifest.vault_id)
            .finish_non_exhaustive()
    }
}

impl Vault {
    /// Whether `root` already contains a vault manifest.
    pub fn exists(root: impl AsRef<Path>) -> bool {
        root.as_ref().join(MANIFEST_FILE).is_file()
    }

    /// Create a brand-new vault at `root`. Fails if a manifest already exists
    /// there, so a second machine cannot accidentally overwrite the salt.
    pub fn create(root: impl Into<PathBuf>, password: &[u8], kdf: KdfParams) -> Result<Self> {
        let root = root.into();
        let manifest_path = root.join(MANIFEST_FILE);
        if manifest_path.exists() {
            return Err(VaultError::AlreadyExists(root));
        }

        fs::create_dir_all(&root).map_err(|e| VaultError::io(&root, e))?;
        for c in Collection::ALL {
            let dir = root.join(c.dir_name());
            fs::create_dir_all(&dir).map_err(|e| VaultError::io(&dir, e))?;
        }

        let salt = crypto::generate_salt();
        let key = crypto::derive_key(password, &salt, kdf)?;
        let verifier = crypto::encrypt(&key, VERIFIER_PLAINTEXT, VERIFIER_AAD)?;

        let manifest = Manifest {
            version: MANIFEST_VERSION,
            vault_id: Uuid::new_v4(),
            salt: B64.encode(salt),
            kdf,
            verifier: B64.encode(verifier),
            created_at: now_ms(),
        };

        let json =
            serde_json::to_vec_pretty(&manifest).map_err(|e| VaultError::json("manifest", e))?;
        // `create_new` guards against a race where another process created
        // the manifest between our existence check and this write.
        let mut f = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&manifest_path)
            .map_err(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    VaultError::AlreadyExists(root.clone())
                } else {
                    VaultError::io(&manifest_path, e)
                }
            })?;
        f.write_all(&json)
            .and_then(|_| f.sync_all())
            .map_err(|e| VaultError::io(&manifest_path, e))?;

        Ok(Self {
            root,
            key,
            manifest,
        })
    }

    /// Unlock an existing vault with the master password.
    pub fn open(root: impl Into<PathBuf>, password: &[u8]) -> Result<Self> {
        let root = root.into();
        let manifest = Self::read_manifest(&root)?;
        let salt = manifest.salt_bytes()?;
        let key = crypto::derive_key(password, &salt, manifest.kdf)?;

        match crypto::decrypt(&key, &manifest.verifier_bytes()?, VERIFIER_AAD) {
            Ok(pt) if pt == VERIFIER_PLAINTEXT => {}
            Ok(_) | Err(CryptoError::AuthenticationFailed) => {
                return Err(VaultError::WrongPassword)
            }
            Err(e) => return Err(e.into()),
        }

        // Make sure collection dirs exist even if a sync client only pulled
        // the manifest so far.
        for c in Collection::ALL {
            let dir = root.join(c.dir_name());
            fs::create_dir_all(&dir).map_err(|e| VaultError::io(&dir, e))?;
        }

        Ok(Self {
            root,
            key,
            manifest,
        })
    }

    /// Open if a manifest exists, otherwise create.
    pub fn open_or_create(
        root: impl Into<PathBuf>,
        password: &[u8],
        kdf: KdfParams,
    ) -> Result<Self> {
        let root = root.into();
        if Self::exists(&root) {
            Self::open(root, password)
        } else {
            Self::create(root, password, kdf)
        }
    }

    fn read_manifest(root: &Path) -> Result<Manifest> {
        let path = root.join(MANIFEST_FILE);
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(VaultError::NotInitialized(root.to_path_buf()))
            }
            Err(e) => return Err(VaultError::io(&path, e)),
        };
        let manifest: Manifest = serde_json::from_slice(&bytes)
            .map_err(|e| VaultError::MalformedManifest(path.clone(), e.to_string()))?;
        if manifest.version != MANIFEST_VERSION {
            return Err(VaultError::UnsupportedManifestVersion(manifest.version));
        }
        Ok(manifest)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn collection_dir(&self, collection: Collection) -> PathBuf {
        self.root.join(collection.dir_name())
    }

    /// Full path of a record file.
    pub fn record_path(&self, collection: Collection, id: Uuid) -> PathBuf {
        self.collection_dir(collection)
            .join(format!("{}.{RECORD_EXT}", id.as_hyphenated()))
    }

    /// Inverse of [`record_path`](Self::record_path). Returns `None` for
    /// anything that is not a bare `<uuid>.enc` inside a collection directory
    /// of this vault, which conveniently excludes Dropbox "conflicted copy"
    /// files, temp files, and unrelated clutter.
    pub fn parse_record_path(&self, path: &Path) -> Option<(Collection, Uuid)> {
        let rel = path.strip_prefix(&self.root).ok()?;
        let mut comps = rel.components();
        let dir = comps.next()?.as_os_str().to_str()?;
        let file = comps.next()?.as_os_str().to_str()?;
        if comps.next().is_some() {
            return None;
        }
        let collection = Collection::from_dir_name(dir)?;
        let stem = file.strip_suffix(&format!(".{RECORD_EXT}"))?;
        let id = Uuid::parse_str(stem).ok()?;
        Some((collection, id))
    }

    fn aad(collection: Collection, id: Uuid) -> Vec<u8> {
        format!("{collection}/{}", id.as_hyphenated()).into_bytes()
    }

    // -- writes -------------------------------------------------------------

    /// Encrypt and atomically write `data` as record `id`. Stamps `updated_at`
    /// with the current time. Returns the written record wrapper.
    pub fn put<T: Serialize + Clone>(
        &self,
        collection: Collection,
        id: Uuid,
        data: &T,
    ) -> Result<Record<T>> {
        let record = Record {
            id,
            updated_at: now_ms(),
            deleted: false,
            data: Some(data.clone()),
        };
        self.write_record(collection, &record)?;
        Ok(record)
    }

    /// Convenience: generate a new UUID and store `data` under it.
    pub fn insert<T: Serialize + Clone>(
        &self,
        collection: Collection,
        data: &T,
    ) -> Result<Record<T>> {
        self.put(collection, Uuid::new_v4(), data)
    }

    /// Replace the record with a tombstone so peers learn about the deletion.
    pub fn delete(&self, collection: Collection, id: Uuid) -> Result<()> {
        let record: Record<serde_json::Value> = Record {
            id,
            updated_at: now_ms(),
            deleted: true,
            data: None,
        };
        self.write_record(collection, &record)
    }

    /// Physically remove tombstone files older than `max_age_ms`. Call this
    /// sparingly (e.g. monthly); a peer that has been offline longer than
    /// `max_age_ms` could resurrect the record.
    pub fn purge_tombstones(&self, collection: Collection, max_age_ms: u64) -> Result<usize> {
        let cutoff = now_ms().saturating_sub(max_age_ms);
        let listing = self.list_with_tombstones::<serde_json::Value>(collection)?;
        let mut removed = 0;
        for r in listing
            .records
            .iter()
            .filter(|r| r.deleted && r.updated_at <= cutoff)
        {
            let path = self.record_path(collection, r.id);
            fs::remove_file(&path).map_err(|e| VaultError::io(&path, e))?;
            removed += 1;
        }
        Ok(removed)
    }

    fn write_record<T: Serialize>(&self, collection: Collection, record: &Record<T>) -> Result<()> {
        let plaintext = Zeroizing::new(
            serde_json::to_vec(record)
                .map_err(|e| VaultError::json(format!("{collection}/{}", record.id), e))?,
        );
        let envelope = crypto::encrypt(&self.key, &plaintext, &Self::aad(collection, record.id))?;
        let path = self.record_path(collection, record.id);
        atomic_write(&path, &envelope)
    }

    // -- reads --------------------------------------------------------------

    /// Read and decrypt a single record. Tombstones yield `VaultError::Deleted`.
    pub fn get<T: DeserializeOwned>(&self, collection: Collection, id: Uuid) -> Result<Record<T>> {
        let record = self.get_raw(collection, id)?;
        if record.deleted {
            return Err(VaultError::Deleted { collection, id });
        }
        Ok(record)
    }

    /// Like [`get`](Self::get) but returns tombstones instead of erroring.
    pub fn get_raw<T: DeserializeOwned>(
        &self,
        collection: Collection,
        id: Uuid,
    ) -> Result<Record<T>> {
        let path = self.record_path(collection, id);
        let envelope = match fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(VaultError::NotFound { collection, id })
            }
            Err(e) => return Err(VaultError::io(&path, e)),
        };
        self.decode_record(collection, id, &path, &envelope)
    }

    /// Decrypt an envelope that the caller already read from disk. This is what
    /// the file watcher uses when the sync client drops a new file.
    pub fn decode_record<T: DeserializeOwned>(
        &self,
        collection: Collection,
        id: Uuid,
        path: &Path,
        envelope: &[u8],
    ) -> Result<Record<T>> {
        if !crypto::looks_like_envelope(envelope) {
            return Err(VaultError::InvalidRecordFile {
                path: path.to_path_buf(),
            });
        }
        let plaintext = Zeroizing::new(crypto::decrypt(
            &self.key,
            envelope,
            &Self::aad(collection, id),
        )?);
        let record: Record<T> = serde_json::from_slice(&plaintext)
            .map_err(|e| VaultError::json(format!("{collection}/{id}"), e))?;
        if record.id != id {
            return Err(VaultError::IdMismatch { collection, id });
        }
        Ok(record)
    }

    /// Load every live (non-tombstone) record in a collection. Files that fail
    /// to parse or decrypt are reported in `skipped`, never fatal.
    pub fn list<T: DeserializeOwned>(&self, collection: Collection) -> Result<Listing<T>> {
        self.list_inner(collection, false)
    }

    /// Load every record including tombstones.
    pub fn list_with_tombstones<T: DeserializeOwned>(
        &self,
        collection: Collection,
    ) -> Result<Listing<T>> {
        self.list_inner(collection, true)
    }

    fn list_inner<T: DeserializeOwned>(
        &self,
        collection: Collection,
        include_tombstones: bool,
    ) -> Result<Listing<T>> {
        let dir = self.collection_dir(collection);
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(Listing {
                    records: Vec::new(),
                    skipped: Vec::new(),
                })
            }
            Err(e) => return Err(VaultError::io(&dir, e)),
        };

        let mut records = Vec::new();
        let mut skipped = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|e| VaultError::io(&dir, e))?;
            let path = entry.path();
            // Silently ignore anything that is not `<uuid>.enc`: temp files,
            // conflicted copies, `.DS_Store`, sync-client metadata.
            let Some((c, id)) = self.parse_record_path(&path) else {
                continue;
            };
            debug_assert_eq!(c, collection);

            match fs::read(&path)
                .map_err(|e| VaultError::io(&path, e))
                .and_then(|env| self.decode_record::<T>(collection, id, &path, &env))
            {
                Ok(r) if r.deleted && !include_tombstones => {}
                Ok(r) => records.push(r),
                Err(reason) => skipped.push(SkippedFile { path, reason }),
            }
        }

        records.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.id.cmp(&b.id)));
        Ok(Listing { records, skipped })
    }

    // -- key management -----------------------------------------------------

    /// Re-encrypt every record and the verifier under a key derived from
    /// `new_password` with a fresh salt.
    ///
    /// Not atomic across files: if the process dies mid-way, some records will
    /// be readable with the old password and some with the new one. The old
    /// manifest is kept as `vault.json.bak` until the operation completes so
    /// the user can recover either way.
    pub fn change_password(&mut self, new_password: &[u8], kdf: KdfParams) -> Result<()> {
        let salt = crypto::generate_salt();
        let new_key = crypto::derive_key(new_password, &salt, kdf)?;

        // 1. Read everything with the current key first, so a decrypt failure
        //    aborts before we have modified a single file.
        let mut all: Vec<(Collection, Uuid, Zeroizing<Vec<u8>>)> = Vec::new();
        for c in Collection::ALL {
            let listing = self.list_with_tombstones::<serde_json::Value>(c)?;
            if let Some(s) = listing.skipped.into_iter().next() {
                return Err(s.reason);
            }
            for r in listing.records {
                let pt = serde_json::to_vec(&r)
                    .map_err(|e| VaultError::json(format!("{c}/{}", r.id), e))?;
                all.push((c, r.id, Zeroizing::new(pt)));
            }
        }

        // 2. Back up the manifest.
        let manifest_path = self.root.join(MANIFEST_FILE);
        let backup_path = self.root.join(format!("{MANIFEST_FILE}.bak"));
        fs::copy(&manifest_path, &backup_path).map_err(|e| VaultError::io(&backup_path, e))?;

        // 3. Rewrite records under the new key.
        for (c, id, pt) in &all {
            let envelope = crypto::encrypt(&new_key, pt, &Self::aad(*c, *id))?;
            atomic_write(&self.record_path(*c, *id), &envelope)?;
        }

        // 4. Write the new manifest last, then drop the backup.
        let verifier = crypto::encrypt(&new_key, VERIFIER_PLAINTEXT, VERIFIER_AAD)?;
        let new_manifest = Manifest {
            salt: B64.encode(salt),
            kdf,
            verifier: B64.encode(verifier),
            ..self.manifest.clone()
        };
        let json = serde_json::to_vec_pretty(&new_manifest)
            .map_err(|e| VaultError::json("manifest", e))?;
        atomic_write(&manifest_path, &json)?;
        let _ = fs::remove_file(&backup_path);

        self.key = new_key;
        self.manifest = new_manifest;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Current Unix time in milliseconds.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Write `data` to `path` via a temp file in the same directory plus rename, so
/// readers (and the sync client) only ever observe the old or the complete new
/// file. The temp name starts with a dot and ends in `.tmp`, which Dropbox,
/// Nextcloud, and Syncthing all treat as ignorable by default.
fn atomic_write(path: &Path, data: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| VaultError::io(path, io::Error::other("path has no parent")))?;
    fs::create_dir_all(dir).map_err(|e| VaultError::io(dir, e))?;

    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| VaultError::io(path, io::Error::other("invalid file name")))?;
    let tmp_path = dir.join(format!(".{file_name}.{:08x}.tmp", OsRng.next_u32()));

    let result = (|| -> io::Result<()> {
        let mut f = File::create(&tmp_path)?;
        f.write_all(data)?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp_path, path)?;
        // Persist the directory entry too, so the rename survives a crash.
        #[cfg(unix)]
        {
            if let Ok(d) = File::open(dir) {
                let _ = d.sync_all();
            }
        }
        Ok(())
    })();

    if let Err(e) = result {
        let _ = fs::remove_file(&tmp_path);
        return Err(VaultError::io(path, e));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Host {
        label: String,
        hostname: String,
        port: u16,
        identity_id: Option<Uuid>,
    }

    fn sample_host() -> Host {
        Host {
            label: "prod-db".into(),
            hostname: "10.0.0.5".into(),
            port: 22,
            identity_id: Some(Uuid::new_v4()),
        }
    }

    fn kdf() -> KdfParams {
        KdfParams::insecure_for_tests()
    }

    fn new_vault() -> (TempDir, Vault) {
        let dir = TempDir::new().unwrap();
        let vault = Vault::create(dir.path().join("vault"), b"hunter2", kdf()).unwrap();
        (dir, vault)
    }

    #[test]
    fn create_lays_out_directories_and_manifest() {
        let (_dir, v) = new_vault();
        assert!(v.root().join(MANIFEST_FILE).is_file());
        for c in Collection::ALL {
            assert!(v.collection_dir(c).is_dir());
        }
        let manifest_text = fs::read_to_string(v.root().join(MANIFEST_FILE)).unwrap();
        assert!(!manifest_text.contains("hunter2"));
    }

    #[test]
    fn create_refuses_to_overwrite() {
        let (dir, _v) = new_vault();
        let err = Vault::create(dir.path().join("vault"), b"x", kdf()).unwrap_err();
        assert!(matches!(err, VaultError::AlreadyExists(_)));
    }

    #[test]
    fn open_with_correct_and_wrong_password() {
        let (dir, v) = new_vault();
        let root = v.root().to_path_buf();
        drop(v);

        assert!(Vault::open(&root, b"hunter2").is_ok());
        assert!(matches!(
            Vault::open(&root, b"wrong").unwrap_err(),
            VaultError::WrongPassword
        ));
        assert!(matches!(
            Vault::open(dir.path().join("nope"), b"x").unwrap_err(),
            VaultError::NotInitialized(_)
        ));
    }

    #[test]
    fn put_get_roundtrip_and_file_is_encrypted() {
        let (_dir, v) = new_vault();
        let host = sample_host();
        let rec = v.insert(Collection::Hosts, &host).unwrap();

        let path = v.record_path(Collection::Hosts, rec.id);
        assert!(path.is_file());
        let raw = fs::read(&path).unwrap();
        assert!(crypto::looks_like_envelope(&raw));
        assert!(!raw.windows(8).any(|w| w == b"10.0.0.5"));

        let loaded: Record<Host> = v.get(Collection::Hosts, rec.id).unwrap();
        assert_eq!(loaded.data.as_ref().unwrap(), &host);
        assert_eq!(loaded.id, rec.id);
        assert!(!loaded.deleted);
    }

    #[test]
    fn get_missing_returns_not_found() {
        let (_dir, v) = new_vault();
        let err = v
            .get::<Host>(Collection::Hosts, Uuid::new_v4())
            .unwrap_err();
        assert!(matches!(err, VaultError::NotFound { .. }));
    }

    #[test]
    fn record_cannot_be_moved_between_collections() {
        let (_dir, v) = new_vault();
        let rec = v.insert(Collection::Hosts, &sample_host()).unwrap();
        let src = v.record_path(Collection::Hosts, rec.id);
        let dst = v.record_path(Collection::Identities, rec.id);
        fs::copy(&src, &dst).unwrap();

        let err = v.get::<Host>(Collection::Identities, rec.id).unwrap_err();
        assert!(matches!(
            err,
            VaultError::Crypto(CryptoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn record_cannot_be_renamed_to_another_id() {
        let (_dir, v) = new_vault();
        let rec = v.insert(Collection::Hosts, &sample_host()).unwrap();
        let other = Uuid::new_v4();
        fs::rename(
            v.record_path(Collection::Hosts, rec.id),
            v.record_path(Collection::Hosts, other),
        )
        .unwrap();
        assert!(matches!(
            v.get::<Host>(Collection::Hosts, other).unwrap_err(),
            VaultError::Crypto(CryptoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn delete_writes_tombstone_and_list_hides_it() {
        let (_dir, v) = new_vault();
        let a = v.insert(Collection::Hosts, &sample_host()).unwrap();
        let b = v.insert(Collection::Hosts, &sample_host()).unwrap();

        v.delete(Collection::Hosts, a.id).unwrap();
        assert!(v.record_path(Collection::Hosts, a.id).is_file());

        assert!(matches!(
            v.get::<Host>(Collection::Hosts, a.id).unwrap_err(),
            VaultError::Deleted { .. }
        ));

        let live = v.list::<Host>(Collection::Hosts).unwrap();
        assert_eq!(live.records.len(), 1);
        assert_eq!(live.records[0].id, b.id);

        let all = v.list_with_tombstones::<Host>(Collection::Hosts).unwrap();
        assert_eq!(all.records.len(), 2);
        assert!(all.records.iter().any(|r| r.id == a.id && r.deleted));
    }

    #[test]
    fn purge_removes_only_old_tombstones() {
        let (_dir, v) = new_vault();
        let a = v.insert(Collection::Hosts, &sample_host()).unwrap();
        v.delete(Collection::Hosts, a.id).unwrap();

        assert_eq!(v.purge_tombstones(Collection::Hosts, 60_000).unwrap(), 0);
        assert_eq!(v.purge_tombstones(Collection::Hosts, 0).unwrap(), 1);
        assert!(!v.record_path(Collection::Hosts, a.id).exists());
    }

    #[test]
    fn list_skips_clutter_and_reports_corrupt_files() {
        let (_dir, v) = new_vault();
        let good = v.insert(Collection::Hosts, &sample_host()).unwrap();
        let dir = v.collection_dir(Collection::Hosts);

        // Dropbox-style conflicted copy, a temp file, and junk: all ignored.
        fs::write(
            dir.join(format!("{} (conflicted copy).enc", good.id)),
            b"junk",
        )
        .unwrap();
        fs::write(dir.join(".something.tmp"), b"junk").unwrap();
        fs::write(dir.join("notes.txt"), b"junk").unwrap();
        // A well-named but corrupt record: reported, not fatal.
        let corrupt_id = Uuid::new_v4();
        fs::write(v.record_path(Collection::Hosts, corrupt_id), b"garbage").unwrap();

        let listing = v.list::<Host>(Collection::Hosts).unwrap();
        assert_eq!(listing.records.len(), 1);
        assert_eq!(listing.records[0].id, good.id);
        assert_eq!(listing.skipped.len(), 1);
        assert!(matches!(
            listing.skipped[0].reason,
            VaultError::InvalidRecordFile { .. }
        ));
    }

    #[test]
    fn list_is_sorted_newest_first() {
        let (_dir, v) = new_vault();
        let first = v
            .insert(Collection::Snippets, &"echo 1".to_string())
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        let second = v
            .insert(Collection::Snippets, &"echo 2".to_string())
            .unwrap();

        let l = v.list::<String>(Collection::Snippets).unwrap();
        assert_eq!(l.records[0].id, second.id);
        assert_eq!(l.records[1].id, first.id);
    }

    #[test]
    fn put_overwrites_and_bumps_timestamp() {
        let (_dir, v) = new_vault();
        let mut host = sample_host();
        let r1 = v.insert(Collection::Hosts, &host).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        host.port = 2222;
        let r2 = v.put(Collection::Hosts, r1.id, &host).unwrap();
        assert!(r2.updated_at > r1.updated_at);

        let loaded: Record<Host> = v.get(Collection::Hosts, r1.id).unwrap();
        assert_eq!(loaded.data.unwrap().port, 2222);
        // No stray temp files left behind.
        let leftovers: Vec<_> = fs::read_dir(v.collection_dir(Collection::Hosts))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn parse_record_path_roundtrip_and_rejections() {
        let (_dir, v) = new_vault();
        let id = Uuid::new_v4();
        let p = v.record_path(Collection::Identities, id);
        assert_eq!(v.parse_record_path(&p), Some((Collection::Identities, id)));

        assert_eq!(
            v.parse_record_path(&v.root().join("hosts").join("x.enc")),
            None
        );
        assert_eq!(
            v.parse_record_path(&v.root().join("other").join(format!("{id}.enc"))),
            None
        );
        assert_eq!(
            v.parse_record_path(&v.root().join("hosts").join("sub").join(format!("{id}.enc"))),
            None
        );
        assert_eq!(
            v.parse_record_path(Path::new("/elsewhere/hosts/x.enc")),
            None
        );
    }

    #[test]
    fn second_machine_opens_same_vault_and_reads_records() {
        // Simulates PC #2 after Dropbox pulled the folder.
        let (_dir, v1) = new_vault();
        let rec = v1
            .insert(Collection::Identities, &"alice".to_string())
            .unwrap();
        let root = v1.root().to_path_buf();
        drop(v1);

        let v2 = Vault::open(&root, b"hunter2").unwrap();
        let got: Record<String> = v2.get(Collection::Identities, rec.id).unwrap();
        assert_eq!(got.data.unwrap(), "alice");
    }

    #[test]
    fn change_password_reencrypts_everything() {
        let (_dir, mut v) = new_vault();
        let host = sample_host();
        let rec = v.insert(Collection::Hosts, &host).unwrap();
        let del = v.insert(Collection::Snippets, &"ls".to_string()).unwrap();
        v.delete(Collection::Snippets, del.id).unwrap();

        v.change_password(b"new-pass", kdf()).unwrap();
        let root = v.root().to_path_buf();
        assert!(!root.join(format!("{MANIFEST_FILE}.bak")).exists());
        drop(v);

        assert!(matches!(
            Vault::open(&root, b"hunter2").unwrap_err(),
            VaultError::WrongPassword
        ));
        let v = Vault::open(&root, b"new-pass").unwrap();
        let got: Record<Host> = v.get(Collection::Hosts, rec.id).unwrap();
        assert_eq!(got.data.unwrap(), host);
        // Tombstones survive the rotation.
        let l = v
            .list_with_tombstones::<String>(Collection::Snippets)
            .unwrap();
        assert!(l.records.iter().any(|r| r.id == del.id && r.deleted));
    }

    #[test]
    fn debug_output_hides_key() {
        let (_dir, v) = new_vault();
        let s = format!("{v:?}");
        assert!(s.contains("Vault"));
        assert!(!s.contains("key"));
    }
}
