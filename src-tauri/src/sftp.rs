//! SFTP browser backend: remote operations over `russh-sftp`, matching local
//! file-system operations, and cancellable recursive transfers with progress.
//!
//! Like `ssh.rs`, this module is Tauri-agnostic. Sessions are keyed by an id
//! the frontend generates (one per remote pane).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, UNIX_EPOCH};

use russh_sftp::client::SftpSession;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::ssh::{open_client, Client, LearnedKey, SshError, Target};

const CHUNK: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

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
// Transfers
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Upload,
    Download,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TransferProgress {
    /// Totals are known once the source tree has been scanned.
    Started {
        total_bytes: u64,
        total_files: u64,
    },
    Progress {
        bytes: u64,
        total_bytes: u64,
        files_done: u64,
        total_files: u64,
        current: String,
    },
    Done {
        bytes: u64,
        files: u64,
    },
    Failed {
        message: String,
    },
    Cancelled,
}

pub trait ProgressSink: Send + Sync + 'static {
    fn report(&self, p: TransferProgress);
}

struct Plan {
    /// Directories to create at the destination, parents first.
    dirs: Vec<String>,
    /// (source, destination, size) for every file.
    files: Vec<(String, String, u64)>,
    total: u64,
}

struct Progress<'a> {
    sink: &'a dyn ProgressSink,
    cancel: &'a AtomicBool,
    bytes: u64,
    total: u64,
    files_done: u64,
    total_files: u64,
    last: Instant,
}

impl Progress<'_> {
    fn check(&self) -> Result<(), SftpError> {
        if self.cancel.load(Ordering::Relaxed) {
            Err(SftpError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn add(&mut self, n: u64, current: &str) {
        self.bytes += n;
        if self.last.elapsed() >= PROGRESS_INTERVAL {
            self.last = Instant::now();
            self.emit(current);
        }
    }
    fn emit(&self, current: &str) {
        self.sink.report(TransferProgress::Progress {
            bytes: self.bytes,
            total_bytes: self.total,
            files_done: self.files_done,
            total_files: self.total_files,
            current: current.to_string(),
        });
    }
}

#[derive(Default)]
pub struct SftpManager {
    sessions: Mutex<HashMap<String, Arc<SftpConn>>>,
    transfers: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl SftpManager {
    pub fn new() -> Self {
        Self::default()
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
        for t in self
            .transfers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
        {
            t.store(true, Ordering::Relaxed);
        }
        for id in ids {
            self.close(&id).await;
        }
    }

    pub fn cancel(&self, transfer_id: &str) {
        if let Some(flag) = self
            .transfers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(transfer_id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Copy `sources` (files or directories) into `dest_dir` on the other
    /// side. Runs to completion; progress and the final outcome go to `sink`.
    pub async fn transfer(
        &self,
        session_id: &str,
        transfer_id: String,
        direction: Direction,
        sources: Vec<String>,
        dest_dir: String,
        sink: &dyn ProgressSink,
    ) {
        let cancel = Arc::new(AtomicBool::new(false));
        self.transfers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(transfer_id.clone(), Arc::clone(&cancel));

        let result = match self.get(session_id) {
            Ok(conn) => run_transfer(&conn, direction, &sources, &dest_dir, sink, &cancel).await,
            Err(e) => Err(e),
        };
        match result {
            Ok((bytes, files)) => sink.report(TransferProgress::Done { bytes, files }),
            Err(SftpError::Cancelled) => sink.report(TransferProgress::Cancelled),
            Err(e) => sink.report(TransferProgress::Failed {
                message: e.to_string(),
            }),
        }
        self.transfers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&transfer_id);
    }
}

async fn run_transfer(
    conn: &SftpConn,
    direction: Direction,
    sources: &[String],
    dest_dir: &str,
    sink: &dyn ProgressSink,
    cancel: &AtomicBool,
) -> Result<(u64, u64), SftpError> {
    let plan = match direction {
        Direction::Download => plan_download(conn, sources, dest_dir).await?,
        Direction::Upload => plan_upload(sources, dest_dir)?,
    };
    sink.report(TransferProgress::Started {
        total_bytes: plan.total,
        total_files: plan.files.len() as u64,
    });
    let mut p = Progress {
        sink,
        cancel,
        bytes: 0,
        total: plan.total,
        files_done: 0,
        total_files: plan.files.len() as u64,
        last: Instant::now(),
    };

    for dir in &plan.dirs {
        p.check()?;
        match direction {
            Direction::Download => {
                let d = Path::new(dir);
                std::fs::create_dir_all(d).map_err(local_err(d))?;
            }
            Direction::Upload => {
                if !conn.sftp.try_exists(dir.clone()).await.unwrap_or(false) {
                    conn.sftp.create_dir(dir.clone()).await?;
                }
            }
        }
    }

    let mut buf = vec![0u8; CHUNK];
    for (src, dst, _) in &plan.files {
        p.check()?;
        match direction {
            Direction::Download => {
                let mut r = conn.sftp.open(src.clone()).await?;
                let dst_path = Path::new(dst);
                let mut w = tokio::fs::File::create(dst_path)
                    .await
                    .map_err(local_err(dst_path))?;
                loop {
                    p.check()?;
                    let n = r.read(&mut buf).await.map_err(|e| SftpError::Local {
                        path: PathBuf::from(src),
                        source: e,
                    })?;
                    if n == 0 {
                        break;
                    }
                    w.write_all(&buf[..n]).await.map_err(local_err(dst_path))?;
                    p.add(n as u64, src);
                }
                w.flush().await.map_err(local_err(dst_path))?;
            }
            Direction::Upload => {
                let src_path = Path::new(src);
                let mut r = tokio::fs::File::open(src_path)
                    .await
                    .map_err(local_err(src_path))?;
                let mut w = conn.sftp.create(dst.clone()).await?;
                loop {
                    p.check()?;
                    let n = r.read(&mut buf).await.map_err(local_err(src_path))?;
                    if n == 0 {
                        break;
                    }
                    w.write_all(&buf[..n]).await.map_err(|e| SftpError::Local {
                        path: PathBuf::from(dst),
                        source: e,
                    })?;
                    p.add(n as u64, src);
                }
                w.shutdown().await.map_err(|e| SftpError::Local {
                    path: PathBuf::from(dst),
                    source: e,
                })?;
            }
        }
        p.files_done += 1;
        p.emit(src);
    }
    Ok((p.bytes, p.files_done))
}

async fn plan_download(
    conn: &SftpConn,
    sources: &[String],
    dest_dir: &str,
) -> Result<Plan, SftpError> {
    let mut plan = Plan {
        dirs: Vec::new(),
        files: Vec::new(),
        total: 0,
    };
    for src in sources {
        let name = remote_basename(src)?;
        let dst = Path::new(dest_dir).join(name);
        let meta = conn.sftp.metadata(src.clone()).await?;
        if !meta.is_dir() {
            let size = meta.size.unwrap_or(0);
            plan.total += size;
            plan.files
                .push((src.clone(), dst.to_string_lossy().into_owned(), size));
            continue;
        }
        let mut queue = vec![(src.clone(), dst)];
        while let Some((rdir, ldir)) = queue.pop() {
            plan.dirs.push(ldir.to_string_lossy().into_owned());
            for e in conn.sftp.read_dir(rdir.clone()).await? {
                let n = e.file_name();
                if n == "." || n == ".." {
                    continue;
                }
                let m = e.metadata();
                if m.is_symlink() {
                    continue; // don't follow links during recursive copies
                }
                let rchild = remote_join(&rdir, &n);
                let lchild = ldir.join(&n);
                if m.is_dir() {
                    queue.push((rchild, lchild));
                } else {
                    let size = m.size.unwrap_or(0);
                    plan.total += size;
                    plan.files
                        .push((rchild, lchild.to_string_lossy().into_owned(), size));
                }
            }
        }
    }
    Ok(plan)
}

fn plan_upload(sources: &[String], dest_dir: &str) -> Result<Plan, SftpError> {
    let mut plan = Plan {
        dirs: Vec::new(),
        files: Vec::new(),
        total: 0,
    };
    for src in sources {
        let src_path = PathBuf::from(src);
        let name = src_path
            .file_name()
            .ok_or_else(|| SftpError::InvalidPath(src.clone()))?
            .to_string_lossy()
            .into_owned();
        let dst = remote_join(dest_dir, &name);
        let meta = std::fs::metadata(&src_path).map_err(local_err(&src_path))?;
        if !meta.is_dir() {
            plan.total += meta.len();
            plan.files.push((src.clone(), dst, meta.len()));
            continue;
        }
        let mut queue = vec![(src_path, dst)];
        while let Some((ldir, rdir)) = queue.pop() {
            plan.dirs.push(rdir.clone());
            for e in std::fs::read_dir(&ldir).map_err(local_err(&ldir))? {
                let Ok(e) = e else { continue };
                let Ok(ft) = e.file_type() else { continue };
                if ft.is_symlink() {
                    continue;
                }
                let n = e.file_name().to_string_lossy().into_owned();
                let rchild = remote_join(&rdir, &n);
                if ft.is_dir() {
                    queue.push((e.path(), rchild));
                } else {
                    let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                    plan.total += size;
                    plan.files
                        .push((e.path().to_string_lossy().into_owned(), rchild, size));
                }
            }
        }
    }
    Ok(plan)
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
}
