//! Copying between any two [`FileBackend`]s: scanning the source, making the
//! folders, streaming each file with progress, and stopping, pausing or
//! resuming on request. Nothing here knows which protocols are involved.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{FileBackend, FileError};

pub const CHUNK: usize = 256 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TransferProgress {
    /// Totals are known once the source tree has been scanned.
    Started { total_bytes: u64, total_files: u64 },
    Progress { bytes: u64, total_bytes: u64, files_done: u64, total_files: u64, current: String },
    /// Waiting for `resume`. Nothing is transferred meanwhile.
    Paused { bytes: u64, total_bytes: u64 },
    Done { bytes: u64, files: u64 },
    Failed { message: String },
    Cancelled,
}

pub trait ProgressSink: Send + Sync + 'static {
    fn report(&self, p: TransferProgress);
}

/// What to do when something is already at the destination.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// Replace it.
    #[default]
    Overwrite,
    /// Leave it, and don't copy that file.
    Skip,
    /// Copy beside it under a new name: `name (1).ext`.
    Rename,
}

/// Cancel and pause switches for one transfer.
#[derive(Default)]
pub struct TransferCtl {
    cancel: AtomicBool,
    paused: AtomicBool,
    wake: tokio::sync::Notify,
}

impl TransferCtl {
    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::SeqCst);
        self.wake.notify_waiters();
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        self.wake.notify_waiters();
    }
}

/// The transfers running now, by id, so the window can stop or pause one.
#[derive(Default)]
pub struct TransferRegistry {
    running: Mutex<HashMap<String, Arc<TransferCtl>>>,
}

impl TransferRegistry {
    fn add(&self, id: &str) -> Arc<TransferCtl> {
        let ctl = Arc::new(TransferCtl::default());
        self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(id.to_string(), Arc::clone(&ctl));
        ctl
    }
    fn remove(&self, id: &str) {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(id);
    }
    fn get(&self, id: &str) -> Option<Arc<TransferCtl>> {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).get(id).cloned()
    }
    pub fn cancel(&self, id: &str) {
        if let Some(c) = self.get(id) {
            c.cancel();
        }
    }
    pub fn cancel_all(&self) {
        for c in self.running.lock().unwrap_or_else(|p| p.into_inner()).values() {
            c.cancel();
        }
    }
    /// Pause between chunks. The connection stays open.
    pub fn pause(&self, id: &str) {
        if let Some(c) = self.get(id) {
            c.set_paused(true);
        }
    }
    pub fn resume(&self, id: &str) {
        if let Some(c) = self.get(id) {
            c.set_paused(false);
        }
    }
    pub fn len(&self) -> usize {
        self.running.lock().unwrap_or_else(|p| p.into_inner()).len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

struct Progress<'a> {
    sink: &'a dyn ProgressSink,
    ctl: &'a TransferCtl,
    bytes: u64,
    total: u64,
    files_done: u64,
    total_files: u64,
    last: Instant,
}

impl Progress<'_> {
    /// Stop if cancelled; wait here while paused.
    async fn check(&mut self, current: &str) -> Result<(), FileError> {
        if self.ctl.paused.load(Ordering::SeqCst) && !self.ctl.cancel.load(Ordering::SeqCst) {
            self.sink.report(TransferProgress::Paused { bytes: self.bytes, total_bytes: self.total });
            loop {
                // Register before re-checking, so a resume can't slip by.
                let woken = self.ctl.wake.notified();
                if !self.ctl.paused.load(Ordering::SeqCst) || self.ctl.cancel.load(Ordering::SeqCst) {
                    break;
                }
                woken.await;
            }
            self.emit(current);
        }
        if self.ctl.cancel.load(Ordering::SeqCst) {
            Err(FileError::Cancelled)
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

struct Plan {
    /// Files left alone because "skip" found them already there.
    skipped: u64,
    /// Folders to create at the destination, parents first.
    dirs: Vec<String>,
    /// (source, destination, size) for every file.
    files: Vec<(String, String, u64)>,
    total: u64,
}

/// `name (1).ext`, `name (2).ext`, … : the first that nothing is using.
pub async fn unique_name(dst: &dyn FileBackend, dir: &str, name: &str) -> Result<String, FileError> {
    if dst.stat(&dst.join(dir, name)).await?.is_none() {
        return Ok(name.to_string());
    }
    // The extension stays last: "archive.tar.gz" becomes "archive.tar (1).gz".
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 1..10_000 {
        let candidate = format!("{stem} ({n}){ext}");
        if dst.stat(&dst.join(dir, &candidate)).await?.is_none() {
            return Ok(candidate);
        }
    }
    Err(FileError::InvalidPath(format!("no free name for {name}")))
}

async fn plan(src: &dyn FileBackend, dst: &dyn FileBackend, sources: &[String], dest_dir: &str, conflict: Conflict, resume: bool) -> Result<Plan, FileError> {
    let mut plan = Plan { skipped: 0, dirs: Vec::new(), files: Vec::new(), total: 0 };
    // Resuming picks up files that are already there, so it never renames or skips.
    let conflict = if resume { Conflict::Overwrite } else { conflict };
    for source in sources {
        let name = src.basename(source)?;
        let name = if conflict == Conflict::Rename { unique_name(dst, dest_dir, &name).await? } else { name };
        let top = dst.join(dest_dir, &name);
        let stat = src.stat(source).await?.ok_or_else(|| FileError::InvalidPath(format!("{source} is gone")))?;
        if !stat.is_dir {
            add_file(&mut plan, dst, source.clone(), top, stat.size, conflict).await?;
            continue;
        }
        let mut queue = vec![(source.clone(), top)];
        while let Some((sdir, ddir)) = queue.pop() {
            plan.dirs.push(ddir.clone());
            for e in src.list(&sdir).await? {
                if e.is_symlink {
                    continue; // links aren't followed in a recursive copy
                }
                if !super::is_plain_name(&e.name) {
                    plan.skipped += 1; // a name that could point outside the destination folder
                    continue;
                }
                let (schild, dchild) = (src.join(&sdir, &e.name), dst.join(&ddir, &e.name));
                if e.is_dir {
                    queue.push((schild, dchild));
                } else {
                    add_file(&mut plan, dst, schild, dchild, e.size, conflict).await?;
                }
            }
        }
    }
    Ok(plan)
}

async fn add_file(plan: &mut Plan, dst: &dyn FileBackend, from: String, to: String, size: u64, conflict: Conflict) -> Result<(), FileError> {
    if conflict == Conflict::Skip && dst.stat(&to).await?.is_some() {
        plan.skipped += 1;
        return Ok(());
    }
    plan.total += size;
    plan.files.push((from, to, size));
    Ok(())
}

fn io_err(path: &str) -> impl FnOnce(std::io::Error) -> FileError + '_ {
    move |source| FileError::Local { path: PathBuf::from(path), source }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    src: &dyn FileBackend,
    dst: &dyn FileBackend,
    sources: &[String],
    dest_dir: &str,
    resume: bool,
    conflict: Conflict,
    sink: &dyn ProgressSink,
    ctl: &TransferCtl,
) -> Result<Copied, FileError> {
    let plan = plan(src, dst, sources, dest_dir, conflict, resume).await?;
    sink.report(TransferProgress::Started { total_bytes: plan.total, total_files: plan.files.len() as u64 });
    let mut p = Progress { sink, ctl, bytes: 0, total: plan.total, files_done: 0, total_files: plan.files.len() as u64, last: Instant::now() };
    // A side that can't continue a file starts it over.
    let can_resume = resume && src.caps().resume && dst.caps().resume;

    for dir in &plan.dirs {
        p.check(dir).await?;
        dst.ensure_dir(dir).await?;
    }

    let mut buf = vec![0u8; CHUNK];
    for (from, to, size) in &plan.files {
        p.check(from).await?;
        // How much of this file is already at the destination.
        let have = if can_resume { dst.stat(to).await?.map(|s| s.size).unwrap_or(0) } else { 0 };
        // Larger than the source means it's a different file: start over.
        let offset = if have <= *size { have } else { 0 };
        if can_resume && offset == *size && have == *size {
            p.add(*size, from);
            p.files_done += 1;
            p.emit(from);
            continue;
        }
        p.add(offset, from);
        let mut r = src.read(from, offset).await?;
        let mut w = dst.write(to, offset, *size).await?;
        loop {
            p.check(from).await?;
            let n = r.read(&mut buf).await.map_err(io_err(from))?;
            if n == 0 {
                break;
            }
            w.write_all(&buf[..n]).await.map_err(io_err(to))?;
            p.add(n as u64, from);
        }
        w.flush().await.map_err(io_err(to))?;
        w.shutdown().await.map_err(io_err(to))?;
        p.files_done += 1;
        p.emit(from);
    }
    Ok(Copied { bytes: p.bytes, files: p.files_done, skipped: plan.skipped })
}

struct Copied {
    bytes: u64,
    files: u64,
    skipped: u64,
}

/// Copy `sources` into `dest_dir`. Always ends by reporting `Done`, `Failed`
/// or `Cancelled` to `sink`.
///
/// With `resume`, files already at the destination continue from where they
/// stop (a partial file is appended to, a complete one skipped) instead of
/// being copied again. That's how "Retry" picks up a transfer that failed or
/// was cancelled.
#[allow(clippy::too_many_arguments)]
pub async fn transfer(
    registry: &TransferRegistry,
    transfer_id: String,
    src: &dyn FileBackend,
    dst: &dyn FileBackend,
    sources: &[String],
    dest_dir: &str,
    resume: bool,
    conflict: Conflict,
    sink: &dyn ProgressSink,
) {
    let ctl = registry.add(&transfer_id);
    let result = run(src, dst, sources, dest_dir, resume, conflict, sink, &ctl).await;
    report(sink, result.map(|c| (c.bytes, c.files)));
    registry.remove(&transfer_id);
}

fn report(sink: &dyn ProgressSink, result: Result<(u64, u64), FileError>) {
    match result {
        Ok((bytes, files)) => sink.report(TransferProgress::Done { bytes, files }),
        Err(FileError::Cancelled) => sink.report(TransferProgress::Cancelled),
        Err(e) => sink.report(TransferProgress::Failed { message: e.to_string() }),
    }
}

/// Move: copy, then delete the originals. The originals go only if every file
/// was copied: with "skip", anything that was left alone keeps all of them, so
/// nothing is lost to a name clash. Works between any two places.
#[allow(clippy::too_many_arguments)]
pub async fn move_items(
    registry: &TransferRegistry,
    transfer_id: String,
    src: &dyn FileBackend,
    dst: &dyn FileBackend,
    sources: &[String],
    dest_dir: &str,
    conflict: Conflict,
    sink: &dyn ProgressSink,
) {
    let ctl = registry.add(&transfer_id);
    let copied = run(src, dst, sources, dest_dir, false, conflict, sink, &ctl).await;
    let result = match copied {
        Ok(c) if c.skipped == 0 => remove_all(src, sources).await.map(|()| (c.bytes, c.files)),
        Ok(c) => Ok((c.bytes, c.files)),
        Err(e) => Err(e),
    };
    report(sink, result);
    registry.remove(&transfer_id);
}

async fn remove_all(src: &dyn FileBackend, paths: &[String]) -> Result<(), FileError> {
    for p in paths {
        src.remove(p).await?;
    }
    Ok(())
}
