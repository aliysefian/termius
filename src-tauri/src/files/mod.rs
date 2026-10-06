//! Places that hold files, behind one interface.
//!
//! A file pane shows a [`FileBackend`]: this computer's disk, an SFTP server,
//! and later SCP, FTP and others. Everything the panes do (browse, make a
//! folder, rename, delete, copy between two panes) is written once against the
//! trait, so a new protocol is one implementation, not a new set of commands.
//!
//! A backend says what it can do ([`Caps`]); the window hides the rest.

pub mod engine;
pub mod local;
#[cfg(test)]
pub mod memory;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncWrite};

pub use crate::sftp::FileEntry;
use crate::sftp::{SftpError, SftpManager};
pub use engine::{Conflict, ProgressSink, TransferCtl, TransferProgress, TransferRegistry};

/// The id of this computer's own disk, which needs no session.
pub const LOCAL: &str = "local";

pub type FileError = SftpError;

/// What a backend can do. A pane offers only these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Caps {
    pub mkdir: bool,
    pub rename: bool,
    pub delete: bool,
    /// Change permission bits.
    pub chmod: bool,
    /// Continue a partly copied file instead of starting again.
    pub resume: bool,
    /// Read the start of a file for a quick look.
    pub preview: bool,
    /// Open a file in a local editor and save it back.
    pub edit: bool,
}

impl Caps {
    /// Everything but permissions, the common case.
    pub const fn files() -> Self {
        Self { mkdir: true, rename: true, delete: true, chmod: false, resume: true, preview: true, edit: true }
    }
}

/// What is known about one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stat {
    pub is_dir: bool,
    pub size: u64,
}

pub type Reader = Box<dyn AsyncRead + Send + Unpin>;
pub type Writer = Box<dyn AsyncWrite + Send + Unpin>;

#[async_trait]
pub trait FileBackend: Send + Sync + 'static {
    fn caps(&self) -> Caps;

    /// Where a pane starts.
    async fn home(&self) -> Result<String, FileError>;

    /// The entries of a folder, folders first, each group by name.
    async fn list(&self, dir: &str) -> Result<Vec<FileEntry>, FileError>;

    /// What a path is (following links), or `None` if nothing is there.
    async fn stat(&self, path: &str) -> Result<Option<Stat>, FileError>;

    async fn mkdir(&self, dir: &str, name: &str) -> Result<(), FileError>;

    /// Rename within the same folder.
    async fn rename(&self, from: &str, new_name: &str) -> Result<(), FileError>;

    /// Delete a file, or a folder with everything in it.
    async fn remove(&self, path: &str) -> Result<(), FileError>;

    async fn chmod(&self, _path: &str, _mode: u32) -> Result<(), FileError> {
        Err(FileError::Unsupported("changing permissions"))
    }

    /// A file's bytes from `offset`.
    async fn read(&self, path: &str, offset: u64) -> Result<Reader, FileError>;

    /// Write a file. At offset 0 it is created or emptied first; above 0 it
    /// continues a partial file from there.
    async fn write(&self, path: &str, offset: u64) -> Result<Writer, FileError>;

    async fn close(&self) {}

    // -- paths: POSIX unless the backend says otherwise ----------------------------------

    fn join(&self, dir: &str, name: &str) -> String {
        crate::sftp::remote_join(dir, name)
    }

    /// The last part of a path, or an error for `/`, `.` and `..`.
    fn basename(&self, path: &str) -> Result<String, FileError> {
        path.trim_end_matches('/')
            .rsplit('/')
            .next()
            .filter(|s| !s.is_empty() && *s != "." && *s != "..")
            .map(str::to_string)
            .ok_or_else(|| FileError::InvalidPath(path.to_string()))
    }

    /// The folder holding a path, or `None` at the top.
    fn parent(&self, path: &str) -> Option<String> {
        let trimmed = path.trim_end_matches('/');
        let (p, _) = trimmed.rsplit_once('/')?;
        Some(if p.is_empty() { "/".to_string() } else { p.to_string() })
    }

    // -- built on the above ----------------------------------------------------------------

    /// The first `max` bytes of a file, and whether there was more.
    async fn read_head(&self, path: &str, max: usize) -> Result<(Vec<u8>, bool), FileError> {
        use tokio::io::AsyncReadExt;
        let mut r = self.read(path, 0).await?;
        let mut buf = Vec::with_capacity(max.min(1 << 20));
        (&mut r).take(max as u64 + 1).read_to_end(&mut buf).await.map_err(|e| FileError::Local { path: path.into(), source: e })?;
        let truncated = buf.len() > max;
        buf.truncate(max);
        Ok((buf, truncated))
    }

    /// Make a folder unless it is already there. Parents must exist.
    async fn ensure_dir(&self, path: &str) -> Result<(), FileError> {
        match self.stat(path).await? {
            Some(s) if s.is_dir => Ok(()),
            Some(_) => Err(FileError::InvalidPath(format!("{path} is a file, not a folder"))),
            None => {
                let parent = self.parent(path).ok_or_else(|| FileError::InvalidPath(path.to_string()))?;
                self.mkdir(&parent, &self.basename(path)?).await
            }
        }
    }
}

/// Every open place, by the id the window gave its pane. This computer's disk
/// is always there as [`LOCAL`]; SFTP sessions live in the [`SftpManager`].
pub struct FileManager {
    sftp: Arc<SftpManager>,
    local: Arc<dyn FileBackend>,
    others: Mutex<HashMap<String, Arc<dyn FileBackend>>>,
}

impl FileManager {
    pub fn new(sftp: Arc<SftpManager>) -> Self {
        Self { sftp, local: Arc::new(local::LocalBackend), others: Mutex::new(HashMap::new()) }
    }

    pub fn registry(&self) -> Arc<TransferRegistry> {
        self.sftp.registry()
    }

    /// Start using `backend` for pane `id`, closing what it replaces.
    pub async fn insert(&self, id: String, backend: Arc<dyn FileBackend>) {
        let old = self.others.lock().unwrap_or_else(|p| p.into_inner()).insert(id, backend);
        if let Some(old) = old {
            old.close().await;
        }
    }

    pub fn get(&self, id: &str) -> Result<Arc<dyn FileBackend>, FileError> {
        if id == LOCAL {
            return Ok(Arc::clone(&self.local));
        }
        if let Some(b) = self.others.lock().unwrap_or_else(|p| p.into_inner()).get(id) {
            return Ok(Arc::clone(b));
        }
        Ok(self.sftp.get(id)? as Arc<dyn FileBackend>)
    }

    pub async fn close(&self, id: &str) {
        let other = self.others.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
        match other {
            Some(b) => b.close().await,
            None => self.sftp.close(id).await,
        }
    }

    pub async fn close_all(&self) {
        let all: Vec<_> = self.others.lock().unwrap_or_else(|p| p.into_inner()).drain().collect();
        for (_, b) in all {
            b.close().await;
        }
    }

    /// Move `sources` from pane `source_id` into `dest_dir` of pane `dest_id`:
    /// copy, then delete the originals if everything was copied.
    #[allow(clippy::too_many_arguments)]
    pub async fn move_items(&self, source_id: &str, dest_id: &str, transfer_id: String, sources: Vec<String>, dest_dir: String, conflict: Conflict, sink: &dyn ProgressSink) {
        let ends = self.get(source_id).and_then(|s| Ok((s, self.get(dest_id)?)));
        match ends {
            Ok((src, dst)) => engine::move_items(&self.registry(), transfer_id, &*src, &*dst, &sources, &dest_dir, conflict, sink).await,
            Err(e) => sink.report(TransferProgress::Failed { message: e.to_string() }),
        }
    }

    /// Copy `sources` from pane `source_id` into `dest_dir` of pane `dest_id`.
    /// Runs to completion; progress and the outcome go to `sink`.
    #[allow(clippy::too_many_arguments)]
    pub async fn transfer(
        &self,
        source_id: &str,
        dest_id: &str,
        transfer_id: String,
        sources: Vec<String>,
        dest_dir: String,
        resume: bool,
        conflict: Conflict,
        sink: &dyn ProgressSink,
    ) {
        let ends = self.get(source_id).and_then(|s| Ok((s, self.get(dest_id)?)));
        match ends {
            Ok((src, dst)) => engine::transfer(&self.registry(), transfer_id, &*src, &*dst, &sources, &dest_dir, resume, conflict, sink).await,
            Err(e) => sink.report(TransferProgress::Failed { message: e.to_string() }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::memory::MemoryBackend;
    use super::*;
    use std::time::Duration;

    #[derive(Default)]
    struct Collect(Mutex<Vec<TransferProgress>>);
    impl ProgressSink for Arc<Collect> {
        fn report(&self, p: TransferProgress) {
            self.0.lock().unwrap().push(p);
        }
    }
    impl Collect {
        fn last(&self) -> TransferProgress {
            self.0.lock().unwrap().last().cloned().expect("a report")
        }
        fn all(&self) -> Vec<TransferProgress> {
            self.0.lock().unwrap().clone()
        }
    }

    fn src_tree() -> MemoryBackend {
        let m = MemoryBackend::new();
        m.put("/src/a.txt", b"alpha");
        m.put("/src/sub/b.bin", &vec![7u8; 300_000]);
        m.put("/src/sub/deeper/c.txt", b"gamma");
        m.mkdirs("/src/empty");
        m
    }

    async fn copy(src: &dyn FileBackend, dst: &dyn FileBackend, sources: &[&str], dest: &str, resume: bool, conflict: Conflict) -> Arc<Collect> {
        let sink = Arc::new(Collect::default());
        let reg = TransferRegistry::default();
        engine::transfer(&reg, "t".into(), src, dst, &sources.iter().map(|s| s.to_string()).collect::<Vec<_>>(), dest, resume, conflict, &sink).await;
        assert!(reg.is_empty(), "the transfer unregisters itself");
        sink
    }

    #[tokio::test]
    async fn copying_a_folder_between_two_backends_keeps_the_tree() {
        let (a, b) = (src_tree(), MemoryBackend::new());
        b.mkdirs("/in");
        let sink = copy(&a, &b, &["/src", "/src/a.txt"], "/in", false, Conflict::Overwrite).await;
        assert!(matches!(sink.last(), TransferProgress::Done { files: 4, bytes } if bytes == 5 + 300_000 + 5 + 5), "{:?}", sink.last());
        assert_eq!(b.get("/in/src/a.txt").unwrap(), b"alpha");
        assert_eq!(b.get("/in/src/sub/b.bin").unwrap().len(), 300_000);
        assert_eq!(b.get("/in/src/sub/deeper/c.txt").unwrap(), b"gamma");
        assert!(b.exists("/in/src/empty"), "empty folders come too");
        assert_eq!(b.get("/in/a.txt").unwrap(), b"alpha", "a single file lands directly in the folder");
        // The source is untouched.
        assert!(a.exists("/src/sub/b.bin"));
        // Progress starts with the totals and ends at all of them.
        let events = sink.all();
        assert!(matches!(events[0], TransferProgress::Started { total_files: 4, .. }));
        assert!(events.iter().any(|e| matches!(e, TransferProgress::Progress { .. })));
    }

    #[tokio::test]
    async fn this_computers_disk_and_another_backend_swap_files() {
        let dir = tempfile::TempDir::new().unwrap();
        let local = local::LocalBackend;
        let root = dir.path().to_string_lossy().into_owned();
        std::fs::create_dir_all(dir.path().join("up/inner")).unwrap();
        std::fs::write(dir.path().join("up/inner/x.txt"), b"from disk").unwrap();
        let remote = MemoryBackend::new();
        remote.mkdirs("/r");
        let up = local.join(&root, "up");
        let sink = copy(&local, &remote, &[&up], "/r", false, Conflict::Overwrite).await;
        assert!(matches!(sink.last(), TransferProgress::Done { files: 1, .. }));
        assert_eq!(remote.get("/r/up/inner/x.txt").unwrap(), b"from disk");
        // And back down, to a new place on disk.
        let down = local.join(&root, "down");
        std::fs::create_dir(&down).unwrap();
        let sink = copy(&remote, &local, &["/r/up"], &down, false, Conflict::Overwrite).await;
        assert!(matches!(sink.last(), TransferProgress::Done { files: 1, .. }), "{:?}", sink.last());
        assert_eq!(std::fs::read(dir.path().join("down/up/inner/x.txt")).unwrap(), b"from disk");
    }

    #[tokio::test]
    async fn conflicts_overwrite_skip_or_rename() {
        let a = MemoryBackend::new();
        a.put("/s/f.txt", b"new");
        a.put("/s/g.txt", b"new-g");
        a.put("/s/dir/h.txt", b"new-h");
        let fresh = || {
            let b = MemoryBackend::new();
            b.put("/d/f.txt", b"old");
            b.put("/d/dir/h.txt", b"old-h");
            b.put("/d/dir/keep.txt", b"kept");
            b
        };
        // Overwrite replaces, and merges folders.
        let b = fresh();
        copy(&a, &b, &["/s/f.txt", "/s/g.txt", "/s/dir"], "/d", false, Conflict::Overwrite).await;
        assert_eq!((b.get("/d/f.txt").unwrap(), b.get("/d/dir/h.txt").unwrap(), b.get("/d/dir/keep.txt").unwrap()), (b"new".to_vec(), b"new-h".to_vec(), b"kept".to_vec()));
        // Skip leaves what is there and copies the rest.
        let b = fresh();
        let sink = copy(&a, &b, &["/s/f.txt", "/s/g.txt", "/s/dir"], "/d", false, Conflict::Skip).await;
        assert_eq!((b.get("/d/f.txt").unwrap(), b.get("/d/g.txt").unwrap(), b.get("/d/dir/h.txt").unwrap()), (b"old".to_vec(), b"new-g".to_vec(), b"old-h".to_vec()));
        assert!(matches!(sink.last(), TransferProgress::Done { files: 1, .. }), "only g.txt was copied: {:?}", sink.last());
        // Rename keeps both, and a renamed folder never merges.
        let b = fresh();
        copy(&a, &b, &["/s/f.txt", "/s/dir"], "/d", false, Conflict::Rename).await;
        assert_eq!(b.get("/d/f.txt").unwrap(), b"old");
        assert_eq!(b.get("/d/f (1).txt").unwrap(), b"new");
        assert_eq!(b.get("/d/dir/h.txt").unwrap(), b"old-h");
        assert_eq!(b.get("/d/dir (1)/h.txt").unwrap(), b"new-h");
        assert!(!b.exists("/d/dir (1)/keep.txt"));
        // A second copy finds the next free name.
        copy(&a, &b, &["/s/f.txt"], "/d", false, Conflict::Rename).await;
        assert_eq!(b.get("/d/f (2).txt").unwrap(), b"new");
    }

    #[tokio::test]
    async fn unique_names_keep_the_extension_last() {
        let b = MemoryBackend::new();
        b.put("/d/archive.tar.gz", b"");
        b.put("/d/.hidden", b"");
        b.put("/d/noext", b"");
        assert_eq!(engine::unique_name(&b, "/d", "archive.tar.gz").await.unwrap(), "archive.tar (1).gz");
        assert_eq!(engine::unique_name(&b, "/d", "noext").await.unwrap(), "noext (1)");
        assert_eq!(engine::unique_name(&b, "/d", ".hidden").await.unwrap(), ".hidden (1)");
        assert_eq!(engine::unique_name(&b, "/d", "free.txt").await.unwrap(), "free.txt");
    }

    #[tokio::test]
    async fn a_partial_copy_continues_and_a_complete_one_is_not_repeated() {
        let a = MemoryBackend::new();
        let big: Vec<u8> = (0..600_000).map(|i| (i % 251) as u8).collect();
        a.put("/s/big.bin", &big);
        a.put("/s/done.txt", b"complete");
        let b = MemoryBackend::new();
        b.put("/d/s/big.bin", &big[..250_000]);
        b.put("/d/s/done.txt", b"complete");
        let sink = copy(&a, &b, &["/s"], "/d", true, Conflict::Overwrite).await;
        assert!(matches!(sink.last(), TransferProgress::Done { files: 2, .. }), "{:?}", sink.last());
        assert_eq!(b.get("/d/s/big.bin").unwrap(), big, "the rest was appended");
        let reads = a.reads();
        assert!(reads.contains(&("/s/big.bin".to_string(), 250_000)), "{reads:?}");
        assert!(!reads.iter().any(|(p, _)| p == "/s/done.txt"), "a finished file isn't read again: {reads:?}");
        // A destination file longer than the source is a different file: start over.
        let c = MemoryBackend::new();
        c.put("/d/s/done.txt", b"much longer than the source");
        copy(&a, &c, &["/s/done.txt"], "/d/s", true, Conflict::Overwrite).await;
        assert_eq!(c.get("/d/s/done.txt").unwrap(), b"complete");
    }

    #[tokio::test]
    async fn a_backend_that_cannot_resume_starts_files_over() {
        let a = MemoryBackend::new();
        a.put("/s/f.bin", b"0123456789");
        let b = MemoryBackend::new().with_caps(Caps { resume: false, ..Caps::files() });
        b.put("/d/f.bin", b"0123");
        let sink = copy(&a, &b, &["/s/f.bin"], "/d", true, Conflict::Overwrite).await;
        assert!(matches!(sink.last(), TransferProgress::Done { .. }));
        assert_eq!(b.get("/d/f.bin").unwrap(), b"0123456789");
        assert_eq!(a.reads(), vec![("/s/f.bin".to_string(), 0)]);
    }

    #[tokio::test]
    async fn moving_copies_then_removes_the_originals() {
        let (a, b) = (src_tree(), MemoryBackend::new());
        b.mkdirs("/in");
        let sink = Arc::new(Collect::default());
        engine::move_items(&TransferRegistry::default(), "m".into(), &a, &b, &["/src".to_string()], "/in", Conflict::Overwrite, &sink).await;
        assert!(matches!(sink.last(), TransferProgress::Done { files: 3, .. }), "{:?}", sink.last());
        assert!(!a.exists("/src"), "the originals are gone");
        assert!(b.exists("/in/src/sub/deeper/c.txt"));
    }

    #[tokio::test]
    async fn a_move_that_skipped_something_keeps_every_original() {
        let (a, b) = (MemoryBackend::new(), MemoryBackend::new());
        a.put("/s/one.txt", b"1");
        a.put("/s/two.txt", b"2");
        b.put("/d/one.txt", b"already here");
        let sink = Arc::new(Collect::default());
        engine::move_items(&TransferRegistry::default(), "m".into(), &a, &b, &["/s/one.txt".to_string(), "/s/two.txt".to_string()], "/d", Conflict::Skip, &sink).await;
        assert!(matches!(sink.last(), TransferProgress::Done { files: 1, .. }));
        assert!(a.exists("/s/one.txt") && a.exists("/s/two.txt"), "nothing is deleted when something was left behind");
        assert_eq!(b.get("/d/one.txt").unwrap(), b"already here");
        assert_eq!(b.get("/d/two.txt").unwrap(), b"2");
    }

    #[tokio::test]
    async fn a_failed_move_deletes_nothing() {
        let (a, b) = (MemoryBackend::new(), MemoryBackend::new());
        a.put("/s/one.txt", b"1");
        a.put("/s/two.txt", b"2");
        b.mkdirs("/d");
        b.refuse.lock().unwrap().push("/d/two.txt".into());
        let sink = Arc::new(Collect::default());
        engine::move_items(&TransferRegistry::default(), "m".into(), &a, &b, &["/s/one.txt".to_string(), "/s/two.txt".to_string()], "/d", Conflict::Overwrite, &sink).await;
        assert!(matches!(sink.last(), TransferProgress::Failed { .. }), "{:?}", sink.last());
        assert!(a.exists("/s/one.txt") && a.exists("/s/two.txt"));
    }

    #[tokio::test]
    async fn deleting_a_folder_takes_everything_in_it() {
        let a = src_tree();
        a.remove("/src/sub").await.unwrap();
        assert_eq!(a.paths(), vec!["/src", "/src/a.txt", "/src/empty"]);
        assert!(a.remove("/src/sub").await.is_err(), "deleting what is gone says so");
        a.remove("/src").await.unwrap();
        assert!(a.paths().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cancelling_stops_the_copy_and_reports_it() {
        let a = MemoryBackend::new();
        for i in 0..60 {
            a.put(&format!("/s/f{i:02}.txt"), b"x");
        }
        let b = Arc::new(MemoryBackend::new().with_open_delay(Duration::from_millis(15)));
        b.mkdirs("/d");
        let a = Arc::new(a);
        let reg = Arc::new(TransferRegistry::default());
        let sink = Arc::new(Collect::default());
        let task = {
            let (a, b, reg, sink) = (Arc::clone(&a), Arc::clone(&b), Arc::clone(&reg), Arc::clone(&sink));
            tokio::spawn(async move { engine::transfer(&reg, "c".into(), &*a, &*b, &["/s".to_string()], "/d", false, Conflict::Overwrite, &sink).await })
        };
        // Wait until it is under way, then cancel.
        for _ in 0..200 {
            if reg.len() == 1 && b.paths().len() > 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        reg.cancel("c");
        task.await.unwrap();
        assert_eq!(sink.last(), TransferProgress::Cancelled);
        let copied = b.paths().iter().filter(|p| p.ends_with(".txt")).count();
        assert!(copied > 0 && copied < 60, "stopped part way: {copied} of 60");
        assert!(reg.is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pausing_holds_the_copy_until_resumed() {
        let a = MemoryBackend::new();
        for i in 0..40 {
            a.put(&format!("/s/f{i:02}.txt"), b"x");
        }
        let (a, b) = (Arc::new(a), Arc::new(MemoryBackend::new().with_open_delay(Duration::from_millis(10))));
        b.mkdirs("/d");
        let reg = Arc::new(TransferRegistry::default());
        let sink = Arc::new(Collect::default());
        let task = {
            let (a, b, reg, sink) = (Arc::clone(&a), Arc::clone(&b), Arc::clone(&reg), Arc::clone(&sink));
            tokio::spawn(async move { engine::transfer(&reg, "p".into(), &*a, &*b, &["/s".to_string()], "/d", false, Conflict::Overwrite, &sink).await })
        };
        for _ in 0..200 {
            if b.paths().len() > 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        reg.pause("p");
        tokio::time::sleep(Duration::from_millis(100)).await;
        let held = b.paths().len();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(b.paths().len(), held, "nothing moves while paused");
        assert!(sink.all().iter().any(|e| matches!(e, TransferProgress::Paused { .. })));
        reg.resume("p");
        task.await.unwrap();
        assert!(matches!(sink.last(), TransferProgress::Done { files: 40, .. }), "{:?}", sink.last());
    }

    #[tokio::test]
    async fn a_missing_source_fails_the_transfer_cleanly() {
        let (a, b) = (MemoryBackend::new(), MemoryBackend::new());
        b.mkdirs("/d");
        let sink = copy(&a, &b, &["/nope"], "/d", false, Conflict::Overwrite).await;
        assert!(matches!(sink.last(), TransferProgress::Failed { .. }), "{:?}", sink.last());
    }

    #[test]
    fn what_a_backend_cannot_do_is_declared_and_refused() {
        let readonly = MemoryBackend::new().with_caps(Caps { mkdir: false, rename: false, delete: false, ..Caps::files() });
        let c = readonly.caps();
        assert!(!c.chmod && !c.mkdir && !c.rename && !c.delete && c.preview);
        // The default for a backend with no permissions is a plain refusal, not a panic.
        let r = tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(readonly.chmod("/x", 0o644));
        assert!(matches!(r, Err(FileError::Unsupported(_))), "{r:?}");
        assert!(Caps::files().resume && !Caps::files().chmod);
    }

    #[tokio::test]
    async fn the_manager_finds_this_computer_and_reports_unknown_panes() {
        let m = FileManager::new(Arc::new(SftpManager::new()));
        assert!(m.get(LOCAL).is_ok());
        assert!(matches!(m.get("nope"), Err(FileError::NoSession)));
        m.insert("mem".into(), Arc::new(MemoryBackend::new())).await;
        assert!(m.get("mem").is_ok());
        // A transfer naming a pane that isn't open fails in the report, not by panicking.
        let sink = Arc::new(Collect::default());
        m.transfer("mem", "gone", "t".into(), vec!["/x".into()], "/".into(), false, Conflict::Overwrite, &sink).await;
        assert!(matches!(sink.last(), TransferProgress::Failed { .. }));
        m.close("mem").await;
        assert!(m.get("mem").is_err());
    }

    #[tokio::test]
    async fn read_head_returns_a_prefix_and_says_when_cut() {
        let b = MemoryBackend::new();
        b.put("/f", &[1u8; 100]);
        let (head, cut) = b.read_head("/f", 10).await.unwrap();
        assert_eq!((head.len(), cut), (10, true));
        let (all, cut) = b.read_head("/f", 1000).await.unwrap();
        assert_eq!((all.len(), cut), (100, false));
    }
}
