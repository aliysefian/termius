//! File-system watcher that turns sync-client activity (Dropbox, Nextcloud,
//! ...) into decrypted record change notifications.
//!
//! Sync clients typically write a temp file and rename it into place, and
//! often touch the same path several times in quick succession. Events are
//! therefore coalesced per path over a short debounce window and the file is
//! re-read from disk when the window closes, so we always report its final
//! state rather than an intermediate one.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use uuid::Uuid;

use crate::vault::{Collection, Record, Vault};

/// A record whose file changed on disk. `record` is `None` when the file was
/// removed (or purged), in which case the UI should drop the entry.
#[derive(Debug, Clone, Serialize)]
pub struct RecordChange {
    pub collection: Collection,
    pub id: Uuid,
    pub record: Option<Record<serde_json::Value>>,
}

/// Shared handle to the currently unlocked vault. `None` while locked.
pub type SharedVault = Arc<Mutex<Option<Vault>>>;

/// Keeps the OS watcher alive; dropping it stops the background thread.
pub struct VaultWatcher {
    _watcher: RecommendedWatcher,
    root: PathBuf,
}

impl VaultWatcher {
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl std::fmt::Debug for VaultWatcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultWatcher")
            .field("root", &self.root)
            .finish()
    }
}

/// Start watching `root` recursively. Every debounced change to a
/// `<collection>/<uuid>.enc` file is decrypted with the vault in `shared` and
/// handed to `on_change`. Files that fail to decrypt are ignored: a sync
/// client may momentarily expose a partially transferred file, and the next
/// event for that path will pick up the complete one.
pub fn watch<F>(
    shared: SharedVault,
    root: PathBuf,
    debounce: Duration,
    on_change: F,
) -> notify::Result<VaultWatcher>
where
    F: Fn(RecordChange) + Send + 'static,
{
    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        // Receiver gone means the debounce thread has exited; nothing to do.
        let _ = tx.send(res);
    })?;
    watcher.watch(&root, RecursiveMode::Recursive)?;

    let thread_root = root.clone();
    thread::Builder::new()
        .name("vault-watcher".into())
        .spawn(move || debounce_loop(rx, shared, thread_root, debounce, on_change))
        .map_err(notify::Error::io)?;

    Ok(VaultWatcher {
        _watcher: watcher,
        root,
    })
}

fn debounce_loop<F>(
    rx: mpsc::Receiver<notify::Result<Event>>,
    shared: SharedVault,
    root: PathBuf,
    debounce: Duration,
    on_change: F,
) where
    F: Fn(RecordChange),
{
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut deadline: Option<Instant> = None;

    loop {
        let wait = deadline
            .map(|d| d.saturating_duration_since(Instant::now()))
            .unwrap_or(Duration::from_secs(3600));

        match rx.recv_timeout(wait) {
            Ok(Ok(event)) => {
                if is_relevant(&event.kind) {
                    pending.extend(event.paths);
                    deadline = Some(Instant::now() + debounce);
                }
            }
            Ok(Err(_)) => {
                // Watcher backend error for one event; keep going.
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if deadline.is_some_and(|d| Instant::now() >= d) {
            deadline = None;
            let paths: Vec<PathBuf> = pending.drain().collect();
            for path in paths {
                if let Some(change) = load_change(&shared, &root, &path) {
                    on_change(change);
                }
            }
        }
    }
}

fn is_relevant(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
    )
}

fn load_change(shared: &SharedVault, root: &Path, path: &Path) -> Option<RecordChange> {
    // Fast reject before taking the lock: must be `<root>/<collection>/<uuid>.enc`.
    if path.extension().and_then(|e| e.to_str()) != Some(crate::vault::RECORD_EXT) {
        return None;
    }
    if !path.starts_with(root) {
        return None;
    }

    let guard = shared.lock().ok()?;
    let vault = guard.as_ref()?;
    let (collection, id) = vault.parse_record_path(path)?;

    match fs::read(path) {
        Ok(bytes) => vault
            .decode_record::<serde_json::Value>(collection, id, path, &bytes)
            .ok()
            .map(|record| RecordChange {
                collection,
                id,
                record: Some(record),
            }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Some(RecordChange {
            collection,
            id,
            record: None,
        }),
        Err(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;
    use std::sync::mpsc::channel;

    #[test]
    fn detects_record_written_by_another_instance() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("vault");
        let kdf = KdfParams::insecure_for_tests();

        // "PC 1": the running app with an unlocked vault and a watcher.
        let pc1 = Vault::create(&root, b"pw", kdf).unwrap();
        let shared: SharedVault = Arc::new(Mutex::new(Some(pc1)));
        let (tx, rx) = channel::<RecordChange>();
        let _watcher = watch(
            shared.clone(),
            root.clone(),
            Duration::from_millis(100),
            move |c| {
                let _ = tx.send(c);
            },
        )
        .unwrap();
        // Give the OS watcher a moment to register.
        thread::sleep(Duration::from_millis(200));

        // "PC 2": a second instance (or Dropbox) drops a new file into place.
        let pc2 = Vault::open(&root, b"pw").unwrap();
        let rec = pc2
            .insert(Collection::Hosts, &serde_json::json!({"label": "from-pc2"}))
            .unwrap();

        let change = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("change event");
        assert_eq!(change.collection, Collection::Hosts);
        assert_eq!(change.id, rec.id);
        let record = change.record.expect("record present");
        assert_eq!(record.data.unwrap()["label"], "from-pc2");

        // Deleting the file entirely reports `record: None`.
        fs::remove_file(pc2.record_path(Collection::Hosts, rec.id)).unwrap();
        let change = loop {
            let c = rx
                .recv_timeout(Duration::from_secs(5))
                .expect("remove event");
            if c.record.is_none() {
                break c;
            }
        };
        assert_eq!(change.id, rec.id);
    }

    #[test]
    fn ignores_clutter_and_undecryptable_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path().join("vault");
        let kdf = KdfParams::insecure_for_tests();
        let vault = Vault::create(&root, b"pw", kdf).unwrap();
        let hosts = vault.collection_dir(Collection::Hosts);
        let shared: SharedVault = Arc::new(Mutex::new(Some(vault)));

        let (tx, rx) = channel::<RecordChange>();
        let _watcher = watch(shared, root.clone(), Duration::from_millis(100), move |c| {
            let _ = tx.send(c);
        })
        .unwrap();
        thread::sleep(Duration::from_millis(200));

        fs::write(hosts.join("notes.txt"), b"x").unwrap();
        fs::write(
            hosts.join(format!("{} (conflicted copy).enc", Uuid::new_v4())),
            b"x",
        )
        .unwrap();
        fs::write(hosts.join(format!("{}.enc", Uuid::new_v4())), b"garbage").unwrap();

        assert!(rx.recv_timeout(Duration::from_millis(700)).is_err());
    }
}
