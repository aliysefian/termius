//! SFTP browser backend: remote operations over `russh-sftp`, matching local
//! file-system operations, and cancellable recursive transfers with progress.
//!
//! Like `ssh.rs`, this module is Tauri-agnostic. Sessions are keyed by an id
//! the frontend generates (one per remote pane).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;
// The tests below reach these through `use super::*`.
#[cfg(test)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(test)]
use std::time::Duration;

use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

use crate::ssh::{open_client, Client, LearnedKey, SshError, Target};


#[derive(Debug, thiserror::Error)]
pub enum SftpError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error("SSH channel error: {0}")]
    Channel(#[from] russh::Error),
    #[error("SFTP: {0}")]
    Sftp(#[from] russh_sftp::client::error::Error),
    #[error("{path}: {source}")]
    Local {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("no SFTP session with this id")]
    NoSession,
    #[error("transfer cancelled")]
    Cancelled,
    #[error("invalid path: {0}")]
    InvalidPath(String),
    /// A place that isn't SFTP said no, in its own words.
    #[error("{0}")]
    Backend(String),
    #[error("this place doesn't support {0}")]
    Unsupported(&'static str),
}

fn local_err(path: &Path) -> impl FnOnce(std::io::Error) -> SftpError + '_ {
    move |source| SftpError::Local {
        path: path.to_path_buf(),
        source,
    }
}

/// One row in either file pane.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    /// Unix seconds; `None` if unknown.
    pub modified: Option<u64>,
    /// POSIX mode bits (lower 12), if known.
    pub permissions: Option<u32>,
}

fn sort_entries(v: &mut [FileEntry]) {
    v.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Remote paths are always POSIX, regardless of the local OS.
pub fn remote_join(dir: &str, name: &str) -> String {
    if dir.is_empty() || dir == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", dir.trim_end_matches('/'))
    }
}

#[cfg(test)]
fn remote_basename(path: &str) -> Result<&str, SftpError> {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .ok_or_else(|| SftpError::InvalidPath(path.to_string()))
}

fn validate_name(name: &str) -> Result<(), SftpError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(SftpError::InvalidPath(name.to_string()));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Local file system
// ---------------------------------------------------------------------------

pub mod local {
    use super::*;

    pub fn home() -> PathBuf {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    pub fn list(dir: &Path) -> Result<Vec<FileEntry>, SftpError> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(dir).map_err(local_err(dir))? {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let Ok(link_meta) = entry
                .metadata()
                .or_else(|_| std::fs::symlink_metadata(&path))
            else {
                continue;
            };
            let is_symlink = std::fs::symlink_metadata(&path)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(false);
            // Follow symlinks for type/size so a link to a dir is navigable.
            let meta = std::fs::metadata(&path).unwrap_or(link_meta);
            #[cfg(unix)]
            let permissions = {
                use std::os::unix::fs::PermissionsExt;
                Some(meta.permissions().mode() & 0o7777)
            };
            #[cfg(not(unix))]
            let permissions = None;
            out.push(FileEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: path.to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                is_symlink,
                size: if meta.is_dir() { 0 } else { meta.len() },
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs()),
                permissions,
            });
        }
        sort_entries(&mut out);
        Ok(out)
    }

    pub fn mkdir(dir: &Path, name: &str) -> Result<(), SftpError> {
        validate_name(name)?;
        let p = dir.join(name);
        std::fs::create_dir(&p).map_err(local_err(&p))
    }

    pub fn rename(from: &Path, new_name: &str) -> Result<(), SftpError> {
        validate_name(new_name)?;
        let to = from
            .parent()
            .ok_or_else(|| SftpError::InvalidPath(from.display().to_string()))?
            .join(new_name);
        std::fs::rename(from, &to).map_err(local_err(from))
    }

    pub fn remove(path: &Path) -> Result<(), SftpError> {
        let meta = std::fs::symlink_metadata(path).map_err(local_err(path))?;
        if meta.is_dir() {
            std::fs::remove_dir_all(path).map_err(local_err(path))
        } else {
            std::fs::remove_file(path).map_err(local_err(path))
        }
    }
}

// ---------------------------------------------------------------------------
// Remote sessions
// ---------------------------------------------------------------------------

pub struct SftpConn {
    client: Client,
    sftp: SftpSession,
}

impl SftpConn {
    pub async fn open(target: &Target) -> Result<(Self, Vec<LearnedKey>), SftpError> {
        let (client, learned) = open_client(target, None).await?;
        let channel = client.channel_open_session().await?;
        channel.request_subsystem(true, "sftp").await?;
        let sftp = SftpSession::new(channel.into_stream()).await?;
        Ok((Self { client, sftp }, learned))
    }

    pub async fn home(&self) -> Result<String, SftpError> {
        Ok(self.sftp.canonicalize(".").await?)
    }

    pub async fn list(&self, dir: &str) -> Result<Vec<FileEntry>, SftpError> {
        let mut out = Vec::new();
        for entry in self.sftp.read_dir(dir).await? {
            let name = entry.file_name();
            if name == "." || name == ".." {
                continue;
            }
            let path = remote_join(dir, &name);
            let mut meta = entry.metadata();
            let is_symlink = meta.is_symlink();
            if is_symlink {
                // Resolve the link target so dirs behind links are navigable.
                if let Ok(target) = self.sftp.metadata(path.clone()).await {
                    meta = target;
                }
            }
            out.push(FileEntry {
                name,
                path,
                is_dir: meta.is_dir(),
                is_symlink,
                size: if meta.is_dir() {
                    0
                } else {
                    meta.size.unwrap_or(0)
                },
                modified: meta.mtime.map(u64::from),
                permissions: meta.permissions.map(|p| p & 0o7777),
            });
        }
        sort_entries(&mut out);
        Ok(out)
    }

    pub async fn mkdir(&self, dir: &str, name: &str) -> Result<(), SftpError> {
        validate_name(name)?;
        Ok(self.sftp.create_dir(remote_join(dir, name)).await?)
    }

    pub async fn rename(&self, from: &str, new_name: &str) -> Result<(), SftpError> {
        validate_name(new_name)?;
        let parent = from
            .trim_end_matches('/')
            .rsplit_once('/')
            .map(|(p, _)| if p.is_empty() { "/" } else { p })
            .ok_or_else(|| SftpError::InvalidPath(from.to_string()))?;
        Ok(self
            .sftp
            .rename(from.to_string(), remote_join(parent, new_name))
            .await?)
    }

    /// Remove a file, or a directory recursively.
    pub async fn remove(&self, path: &str) -> Result<(), SftpError> {
        let meta = self.sftp.symlink_metadata(path.to_string()).await?;
        if !meta.is_dir() {
            return Ok(self.sftp.remove_file(path.to_string()).await?);
        }
        // Iterative post-order walk to avoid async recursion.
        let mut stack = vec![(path.to_string(), false)];
        while let Some((dir, visited)) = stack.pop() {
            if visited {
                self.sftp.remove_dir(dir).await?;
                continue;
            }
            stack.push((dir.clone(), true));
            for entry in self.sftp.read_dir(dir.clone()).await? {
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                let child = remote_join(&dir, &name);
                if entry.metadata().is_dir() && !entry.metadata().is_symlink() {
                    stack.push((child, false));
                } else {
                    self.sftp.remove_file(child).await?;
                }
            }
        }
        Ok(())
    }

    /// Change permission bits (`chmod`).
    pub async fn chmod(&self, path: &str, mode: u32) -> Result<(), SftpError> {
        let mut meta = self.sftp.metadata(path.to_string()).await?;
        meta.permissions = Some((meta.permissions.unwrap_or(0) & !0o7777) | (mode & 0o7777));
        Ok(self.sftp.set_metadata(path.to_string(), meta).await?)
    }

    /// The first `max` bytes of a file, and whether there was more.
    pub async fn read_head(&self, path: &str, max: usize) -> Result<(Vec<u8>, bool), SftpError> {
        let mut f = self.sftp.open(path.to_string()).await?;
        let mut buf = Vec::with_capacity(max.min(1 << 20));
        (&mut f)
            .take(max as u64 + 1)
            .read_to_end(&mut buf)
            .await
            .map_err(|e| SftpError::Local {
                path: PathBuf::from(path),
                source: e,
            })?;
        let truncated = buf.len() > max;
        buf.truncate(max);
        Ok((buf, truncated))
    }

    /// Whole-file read, for editing.
    pub async fn read_file(&self, path: &str) -> Result<Vec<u8>, SftpError> {
        Ok(self.sftp.read(path.to_string()).await?)
    }

    /// Whole-file write. Uses `create`, which truncates: `SftpSession::write`
    /// does not, and would leave stale bytes after a shorter save.
    pub async fn write_file(&self, path: &str, data: &[u8]) -> Result<(), SftpError> {
        let mut f = self.sftp.create(path.to_string()).await?;
        f.write_all(data).await.map_err(|e| SftpError::Local {
            path: PathBuf::from(path),
            source: e,
        })?;
        f.shutdown().await.map_err(|e| SftpError::Local {
            path: PathBuf::from(path),
            source: e,
        })?;
        Ok(())
    }

    pub async fn close(self) {
        let _ = self.sftp.close().await;
        self.client.close().await;
    }
}

// ---------------------------------------------------------------------------
// The file-backend view of an SFTP session
// ---------------------------------------------------------------------------

use crate::files::{self, Caps, FileBackend, Reader, Stat, Writer};

#[async_trait::async_trait]
impl FileBackend for SftpConn {
    fn caps(&self) -> Caps {
        Caps { chmod: true, ..Caps::files() }
    }

    async fn home(&self) -> Result<String, SftpError> {
        SftpConn::home(self).await
    }

    async fn list(&self, dir: &str) -> Result<Vec<FileEntry>, SftpError> {
        SftpConn::list(self, dir).await
    }

    async fn stat(&self, path: &str) -> Result<Option<Stat>, SftpError> {
        use russh_sftp::client::error::Error as E;
        use russh_sftp::protocol::StatusCode;
        match self.sftp.metadata(path.to_string()).await {
            Ok(m) => Ok(Some(Stat { is_dir: m.is_dir(), size: if m.is_dir() { 0 } else { m.size.unwrap_or(0) } })),
            Err(E::Status(s)) if s.status_code == StatusCode::NoSuchFile => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    async fn mkdir(&self, dir: &str, name: &str) -> Result<(), SftpError> {
        SftpConn::mkdir(self, dir, name).await
    }

    async fn rename(&self, from: &str, new_name: &str) -> Result<(), SftpError> {
        SftpConn::rename(self, from, new_name).await
    }

    async fn remove(&self, path: &str) -> Result<(), SftpError> {
        SftpConn::remove(self, path).await
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<(), SftpError> {
        SftpConn::chmod(self, path, mode).await
    }

    async fn read(&self, path: &str, offset: u64) -> Result<Reader, SftpError> {
        let mut f = self.sftp.open(path.to_string()).await?;
        if offset > 0 {
            f.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| SftpError::Local { path: PathBuf::from(path), source: e })?;
        }
        Ok(Box::new(f))
    }

    async fn write(&self, path: &str, offset: u64) -> Result<Writer, SftpError> {
        if offset == 0 {
            return Ok(Box::new(self.sftp.create(path.to_string()).await?));
        }
        let mut w = self.sftp.open_with_flags(path.to_string(), russh_sftp::protocol::OpenFlags::WRITE).await?;
        w.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| SftpError::Local { path: PathBuf::from(path), source: e })?;
        Ok(Box::new(w))
    }

    async fn close(&self) {
        // The connection is closed when the session is dropped by its manager.
    }
}

// ---------------------------------------------------------------------------
// Transfers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Upload,
    Download,
}

pub use crate::files::engine::{ProgressSink, TransferCtl, TransferProgress};

#[cfg(test)]
const CHUNK: usize = crate::files::engine::CHUNK;

#[derive(Default)]
pub struct SftpManager {
    sessions: Mutex<HashMap<String, Arc<SftpConn>>>,
    registry: Arc<files::TransferRegistry>,
}

impl SftpManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The transfers running now, shared with the other file backends.
    pub fn registry(&self) -> Arc<files::TransferRegistry> {
        Arc::clone(&self.registry)
    }

    /// Open (or replace) the session `id`. Returns the remote home directory
    /// and any host keys learned for the first time (jump hosts included).
    pub async fn open(
        &self,
        id: String,
        target: &Target,
    ) -> Result<(String, Vec<LearnedKey>), SftpError> {
        let (conn, fp) = SftpConn::open(target).await?;
        let home = conn.home().await?;
        let old = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, Arc::new(conn));
        if let Some(old) = old.and_then(|c| Arc::try_unwrap(c).ok()) {
            old.close().await;
        }
        Ok((home, fp))
    }

    pub fn get(&self, id: &str) -> Result<Arc<SftpConn>, SftpError> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(id)
            .cloned()
            .ok_or(SftpError::NoSession)
    }

    pub async fn close(&self, id: &str) {
        let conn = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id);
        if let Some(conn) = conn.and_then(|c| Arc::try_unwrap(c).ok()) {
            conn.close().await;
        }
    }

    pub async fn close_all(&self) {
        let ids: Vec<String> = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .keys()
            .cloned()
            .collect();
        self.registry.cancel_all();
        for id in ids {
            self.close(&id).await;
        }
    }

    pub fn cancel(&self, transfer_id: &str) {
        self.registry.cancel(transfer_id);
    }

    /// Pause between chunks. The connection stays open; `resume` continues.
    pub fn pause(&self, transfer_id: &str) {
        self.registry.pause(transfer_id);
    }

    pub fn resume(&self, transfer_id: &str) {
        self.registry.resume(transfer_id);
    }

    /// Copy `sources` (files or directories) between this computer and the
    /// session, into `dest_dir` on the other side. A convenience over the
    /// general transfer in [`crate::files`], which can join any two places.
    #[allow(clippy::too_many_arguments)]
    pub async fn transfer(
        &self,
        session_id: &str,
        transfer_id: String,
        direction: Direction,
        sources: Vec<String>,
        dest_dir: String,
        resume: bool,
        sink: &dyn ProgressSink,
    ) {
        match self.get(session_id) {
            Ok(conn) => {
                let local = files::local::LocalBackend;
                let (src, dst): (&dyn FileBackend, &dyn FileBackend) = match direction {
                    Direction::Download => (&*conn, &local),
                    Direction::Upload => (&local, &*conn),
                };
                files::engine::transfer(&self.registry, transfer_id, src, dst, &sources, &dest_dir, resume, files::Conflict::Overwrite, sink).await
            }
            Err(e) => sink.report(TransferProgress::Failed { message: e.to_string() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::testutil::{spawn_sshd, target};

    #[test]
    fn remote_path_helpers() {
        assert_eq!(remote_join("/", "a"), "/a");
        assert_eq!(remote_join("/home/u/", "a"), "/home/u/a");
        assert_eq!(remote_basename("/x/y/").unwrap(), "y");
        assert!(remote_basename("/").is_err());
        assert!(validate_name("../x").is_err());
        assert!(validate_name("ok.txt").is_ok());
    }

    #[test]
    fn local_ops() {
        let dir = tempfile::TempDir::new().unwrap();
        local::mkdir(dir.path(), "sub").unwrap();
        std::fs::write(dir.path().join("b.txt"), b"hello").unwrap();
        let l = local::list(dir.path()).unwrap();
        assert_eq!(l[0].name, "sub"); // dirs first
        assert!(l[0].is_dir);
        assert_eq!(l[1].size, 5);
        local::rename(&dir.path().join("b.txt"), "c.txt").unwrap();
        assert!(dir.path().join("c.txt").exists());
        std::fs::write(dir.path().join("sub/x"), b"1").unwrap();
        local::remove(&dir.path().join("sub")).unwrap();
        assert!(!dir.path().join("sub").exists());
        assert!(local::mkdir(dir.path(), "a/b").is_err());
    }

    struct Collect(Mutex<Vec<TransferProgress>>);
    impl ProgressSink for Arc<Collect> {
        fn report(&self, p: TransferProgress) {
            self.0.lock().unwrap().push(p);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn remote_browse_and_recursive_transfers() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let m = SftpManager::new();
        let (home, fp) = m.open("s1".into(), &t).await.unwrap();
        assert!(home.starts_with('/'));
        assert_eq!(fp.len(), 1);
        let conn = m.get("s1").unwrap();

        // Work inside a scratch dir on the "remote" (same machine).
        let remote_root = dir.path().join("remote");
        std::fs::create_dir(&remote_root).unwrap();
        let rroot = remote_root.to_string_lossy().into_owned();

        conn.mkdir(&rroot, "made").await.unwrap();
        let listing = conn.list(&rroot).await.unwrap();
        assert_eq!(listing.len(), 1);
        assert!(listing[0].is_dir);
        conn.rename(&remote_join(&rroot, "made"), "renamed")
            .await
            .unwrap();
        assert!(remote_root.join("renamed").is_dir());

        // Upload a local tree: up/{a.txt, nested/b.bin}
        let up = dir.path().join("up");
        std::fs::create_dir_all(up.join("nested")).unwrap();
        std::fs::write(up.join("a.txt"), b"alpha").unwrap();
        let big: Vec<u8> = (0..(CHUNK * 3 + 17)).map(|i| (i % 251) as u8).collect();
        std::fs::write(up.join("nested/b.bin"), &big).unwrap();

        let sink = Arc::new(Collect(Mutex::new(Vec::new())));
        m.transfer(
            "s1",
            "t1".into(),
            Direction::Upload,
            vec![up.to_string_lossy().into_owned()],
            rroot.clone(),
            false,
            &sink,
        )
        .await;
        let events = sink.0.lock().unwrap().clone();
        assert!(matches!(
            events.first(),
            Some(TransferProgress::Started { total_files: 2, .. })
        ));
        assert!(
            matches!(events.last(), Some(TransferProgress::Done { files: 2, bytes }) if *bytes == 5 + big.len() as u64),
            "{events:?}"
        );
        assert_eq!(
            std::fs::read(remote_root.join("up/nested/b.bin")).unwrap(),
            big
        );

        // Download it back into a different local folder.
        let down = dir.path().join("down");
        std::fs::create_dir(&down).unwrap();
        let sink = Arc::new(Collect(Mutex::new(Vec::new())));
        m.transfer(
            "s1",
            "t2".into(),
            Direction::Download,
            vec![remote_join(&rroot, "up")],
            down.to_string_lossy().into_owned(),
            false,
            &sink,
        )
        .await;
        assert!(matches!(
            sink.0.lock().unwrap().last(),
            Some(TransferProgress::Done { files: 2, .. })
        ));
        assert_eq!(std::fs::read(down.join("up/a.txt")).unwrap(), b"alpha");
        assert_eq!(std::fs::read(down.join("up/nested/b.bin")).unwrap(), big);

        // Recursive remote delete.
        conn.remove(&remote_join(&rroot, "up")).await.unwrap();
        assert!(!remote_root.join("up").exists());

        // Errors surface as Failed, unknown sessions too.
        let sink = Arc::new(Collect(Mutex::new(Vec::new())));
        m.transfer(
            "nope",
            "t3".into(),
            Direction::Download,
            vec!["/x".into()],
            "/tmp".into(),
            false,
            &sink,
        )
        .await;
        assert!(matches!(
            sink.0.lock().unwrap().last(),
            Some(TransferProgress::Failed { .. })
        ));

        drop(conn);
        m.close("s1").await;
        assert!(m.get("s1").is_err());
    }

    /// Pauses itself on the first progress event and records everything.
    struct PauseOnce {
        m: Arc<SftpManager>,
        id: String,
        paused: AtomicBool,
        events: Mutex<Vec<TransferProgress>>,
    }
    impl ProgressSink for Arc<PauseOnce> {
        fn report(&self, p: TransferProgress) {
            if matches!(p, TransferProgress::Started { .. }) && !self.paused.swap(true, Ordering::SeqCst) {
                self.m.pause(&self.id);
            }
            self.events.lock().unwrap().push(p);
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn transfers_pause_resume_and_continue_partial_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let m = Arc::new(SftpManager::new());
        m.open("s".into(), &t).await.unwrap();
        let remote = dir.path().join("remote");
        std::fs::create_dir(&remote).unwrap();
        let rroot = remote.to_string_lossy().into_owned();
        let big: Vec<u8> = (0..(CHUNK * 4 + 99)).map(|i| (i * 7 % 253) as u8).collect();
        let src = dir.path().join("big.bin");
        std::fs::write(&src, &big).unwrap();

        // Pause as soon as it starts; nothing more happens until resumed.
        let sink = Arc::new(PauseOnce { m: Arc::clone(&m), id: "p".into(), paused: AtomicBool::new(false), events: Mutex::new(vec![]) });
        let (m2, sink2, srcs, root2) = (Arc::clone(&m), Arc::clone(&sink), vec![src.to_string_lossy().into_owned()], rroot.clone());
        let job = tokio::spawn(async move { m2.transfer("s", "p".into(), Direction::Upload, srcs, root2, false, &sink2).await });
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!job.is_finished(), "paused transfer must wait");
        assert!(sink.events.lock().unwrap().iter().any(|e| matches!(e, TransferProgress::Paused { .. })));
        m.resume("p");
        job.await.unwrap();
        assert!(matches!(sink.events.lock().unwrap().last(), Some(TransferProgress::Done { files: 1, .. })));
        assert_eq!(std::fs::read(remote.join("big.bin")).unwrap(), big);

        // An interrupted upload (half the file there) continues where it stopped.
        std::fs::write(remote.join("big.bin"), &big[..CHUNK + 5]).unwrap();
        let sink = Arc::new(Collect(Mutex::new(Vec::new())));
        m.transfer("s", "r1".into(), Direction::Upload, vec![src.to_string_lossy().into_owned()], rroot.clone(), true, &sink).await;
        assert!(matches!(sink.0.lock().unwrap().last(), Some(TransferProgress::Done { .. })), "{:?}", sink.0.lock().unwrap());
        assert_eq!(std::fs::read(remote.join("big.bin")).unwrap(), big);

        // Same for downloads; a complete file is skipped, not re-copied.
        let down = dir.path().join("down");
        std::fs::create_dir(&down).unwrap();
        std::fs::write(down.join("big.bin"), &big[..77]).unwrap();
        let rfile = remote_join(&rroot, "big.bin");
        let sink = Arc::new(Collect(Mutex::new(Vec::new())));
        m.transfer("s", "r2".into(), Direction::Download, vec![rfile.clone()], down.to_string_lossy().into_owned(), true, &sink).await;
        assert_eq!(std::fs::read(down.join("big.bin")).unwrap(), big);
        let sink = Arc::new(Collect(Mutex::new(Vec::new())));
        m.transfer("s", "r3".into(), Direction::Download, vec![rfile], down.to_string_lossy().into_owned(), true, &sink).await;
        assert!(matches!(sink.0.lock().unwrap().last(), Some(TransferProgress::Done { bytes, .. }) if *bytes == big.len() as u64));
        assert_eq!(std::fs::read(down.join("big.bin")).unwrap(), big);

        m.close("s").await;
    }
}
