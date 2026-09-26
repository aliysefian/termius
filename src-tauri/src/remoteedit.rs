//! Edit a remote file in a local editor: download it to a private temp
//! folder, watch that folder, and upload the file again whenever it is saved.
//!
//! Editors save in different ways (write in place, or write a new file and
//! rename it over the old one), so the whole folder is watched and changes
//! are debounced and compared by content before uploading.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use serde::Serialize;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::sftp::{SftpConn, SftpError};

const DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum EditEvent {
    Uploaded { bytes: u64 },
    Failed { message: String },
}

pub trait EditSink: Send + Sync + 'static {
    fn event(&self, edit_id: &str, e: EditEvent);
}

#[derive(Debug, Clone, Serialize)]
pub struct EditStarted {
    pub edit_id: String,
    pub local_path: String,
}

struct Edit {
    _watcher: RecommendedWatcher,
    task: tokio::task::AbortHandle,
    dir: PathBuf,
}

#[derive(Default)]
pub struct EditManager {
    edits: Mutex<HashMap<String, Edit>>,
}

fn file_name(remote: &str) -> Result<String, SftpError> {
    remote
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|n| !n.is_empty() && *n != "." && *n != "..")
        .map(str::to_string)
        .ok_or_else(|| SftpError::InvalidPath(remote.to_string()))
}

fn local_err(path: &Path, source: std::io::Error) -> SftpError {
    SftpError::Local {
        path: path.to_path_buf(),
        source,
    }
}

impl EditManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Download `remote_path` into `base/<edit id>/` and start syncing it
    /// back. The caller opens the returned local path in an editor.
    pub async fn start(
        &self,
        conn: Arc<SftpConn>,
        remote_path: String,
        base: &Path,
        sink: Arc<dyn EditSink>,
    ) -> Result<EditStarted, SftpError> {
        let name = file_name(&remote_path)?;
        let edit_id = Uuid::new_v4().to_string();
        let dir = base.join(&edit_id);
        // Owner-only from creation (no chmod race); the base is per-user.
        crate::vault::atomic::create_private_dir(&dir).map_err(|e| local_err(&dir, e))?;
        let local = dir.join(&name);

        let data = conn.read_file(&remote_path).await?;
        write_private(&local, &data).map_err(|e| local_err(&local, e))?;

        let (tx, mut rx) = mpsc::unbounded_channel::<()>();
        let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if res.is_ok() {
                let _ = tx.send(());
            }
        })
        .map_err(|e| local_err(&dir, std::io::Error::other(e.to_string())))?;
        watcher
            .watch(&dir, RecursiveMode::NonRecursive)
            .map_err(|e| local_err(&dir, std::io::Error::other(e.to_string())))?;

        let id = edit_id.clone();
        let file = local.clone();
        let task = tokio::spawn(async move {
            let mut last = data;
            while rx.recv().await.is_some() {
                // Let the save finish, and coalesce the burst of events.
                loop {
                    match tokio::time::timeout(DEBOUNCE, rx.recv()).await {
                        Ok(Some(())) => continue,
                        Ok(None) => return,
                        Err(_) => break,
                    }
                }
                let Ok(now) = tokio::fs::read(&file).await else {
                    continue; // mid-rename; the next event will catch it
                };
                if now == last {
                    continue;
                }
                match conn.write_file(&remote_path, &now).await {
                    Ok(()) => {
                        sink.event(
                            &id,
                            EditEvent::Uploaded {
                                bytes: now.len() as u64,
                            },
                        );
                        last = now;
                    }
                    Err(e) => sink.event(
                        &id,
                        EditEvent::Failed {
                            message: e.to_string(),
                        },
                    ),
                }
            }
        });

        self.edits.lock().unwrap_or_else(|p| p.into_inner()).insert(
            edit_id.clone(),
            Edit {
                _watcher: watcher,
                task: task.abort_handle(),
                dir,
            },
        );
        Ok(EditStarted {
            edit_id,
            local_path: local.to_string_lossy().into_owned(),
        })
    }

    /// Stop syncing and delete the local copy.
    pub fn stop(&self, edit_id: &str) {
        if let Some(e) = self
            .edits
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(edit_id)
        {
            e.task.abort();
            let _ = std::fs::remove_dir_all(&e.dir);
        }
    }

    pub fn stop_all(&self) {
        let all: Vec<String> = self
            .edits
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect();
        for id in all {
            self.stop(&id);
        }
    }
}

/// Local copies are plaintext remote files; keep them private on Unix.
/// Create a new owner-only file; never follows or reuses an existing path.
fn write_private(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)?.write_all(data)
}

/// Remove leftovers from a previous run that didn't shut down cleanly.
pub fn clean_base(base: &Path) {
    let _ = std::fs::remove_dir_all(base);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sftp::SftpManager;
    use crate::ssh::testutil::{spawn_sshd, target};

    struct Collect(std::sync::mpsc::Sender<EditEvent>);
    impl EditSink for Collect {
        fn event(&self, _: &str, e: EditEvent) {
            let _ = self.0.send(e);
        }
    }

    #[test]
    fn remote_names() {
        assert_eq!(file_name("/etc/nginx/nginx.conf").unwrap(), "nginx.conf");
        assert!(file_name("/").is_err());
        assert!(file_name("/x/..").is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn edits_sync_back_including_shorter_content_and_renames() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let m = SftpManager::new();
        m.open("s".into(), &t).await.unwrap();
        let conn = m.get("s").unwrap();

        let remote = dir.path().join("remote.conf");
        std::fs::write(&remote, "a long original line\n").unwrap();
        let rpath = remote.to_string_lossy().into_owned();

        let edits = EditManager::new();
        let (tx, rx) = std::sync::mpsc::channel();
        let base = dir.path().join("edits");
        let started = edits
            .start(
                Arc::clone(&conn),
                rpath.clone(),
                &base,
                Arc::new(Collect(tx)),
            )
            .await
            .unwrap();
        let local = PathBuf::from(&started.local_path);
        assert_eq!(
            std::fs::read_to_string(&local).unwrap(),
            "a long original line\n"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;

        // Save in place with SHORTER content: the remote must not keep a tail.
        std::fs::write(&local, "short\n").unwrap();
        let e = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("upload event");
        assert_eq!(e, EditEvent::Uploaded { bytes: 6 });
        assert_eq!(std::fs::read_to_string(&remote).unwrap(), "short\n");

        // Save the way vim and many editors do: write a temp file, rename over.
        let tmp = local.with_extension("swp");
        std::fs::write(&tmp, "renamed save\n").unwrap();
        std::fs::rename(&tmp, &local).unwrap();
        let e = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("upload event");
        assert_eq!(e, EditEvent::Uploaded { bytes: 13 });
        assert_eq!(std::fs::read_to_string(&remote).unwrap(), "renamed save\n");

        edits.stop(&started.edit_id);
        assert!(!local.exists());
    }
}
