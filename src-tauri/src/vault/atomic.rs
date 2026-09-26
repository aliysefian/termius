//! Crash-safe file replacement for a directory that a sync service watches.
//!
//! 1. Write the new bytes to a temp file next to the target (same
//!    filesystem), created with `O_EXCL` and mode 0600 so a planted symlink
//!    can't redirect it.
//! 2. fsync the file.
//! 3. Read it back and compare, catching silent short writes.
//! 4. Rename over the target, which is atomic on POSIX and on Windows
//!    (`MoveFileEx` with replace).
//! 5. fsync the directory so the rename itself is durable.
//!
//! Any failure removes the temp file and leaves the target untouched. Temp
//! names start with `.` and end in `.tmp`, which Dropbox, OneDrive, Google
//! Drive, Nextcloud and Syncthing skip.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, SystemTime};

use rand::{rngs::OsRng, RngCore};

/// Stages where tests can inject a failure, to prove the target survives.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailAt {
    /// Simulates ENOSPC while writing the temp file.
    Write,
    /// fsync fails.
    Sync,
    /// Power loss after the temp file is complete but before the rename.
    BeforeRename,
}

#[cfg(test)]
thread_local! {
    static FAIL: std::cell::Cell<Option<FailAt>> = const { std::cell::Cell::new(None) };
}

/// Make the next atomic write on this thread fail at `stage`.
#[cfg(test)]
pub fn fail_next(stage: FailAt) {
    FAIL.with(|f| f.set(Some(stage)));
}

#[cfg(test)]
fn should_fail(stage: FailAt) -> bool {
    FAIL.with(|f| {
        if f.get() == Some(stage) {
            f.set(None);
            true
        } else {
            false
        }
    })
}

#[cfg(not(test))]
#[inline(always)]
fn should_fail<T>(_: T) -> bool {
    false
}

#[cfg(not(test))]
#[allow(dead_code)]
#[derive(Clone, Copy)]
enum FailAt {
    Write,
    Sync,
    BeforeRename,
}

fn create_private(path: &Path) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.mode(0o600);
    }
    o.open(path)
}

/// Atomically replace `path` with `data`. See the module docs for the steps.
pub fn write(path: &Path, data: &[u8]) -> io::Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| io::Error::other("path has no parent directory"))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("invalid file name"))?;
    let tmp = dir.join(format!(".{name}.{:016x}.tmp", OsRng.next_u64()));

    let result = (|| -> io::Result<()> {
        let mut f = create_private(&tmp)?;
        if should_fail(FailAt::Write) {
            // Leave a partial file behind, like a real disk-full error would.
            f.write_all(&data[..data.len() / 2])?;
            return Err(io::Error::new(
                io::ErrorKind::StorageFull,
                "no space left on device (simulated)",
            ));
        }
        f.write_all(data)?;
        if should_fail(FailAt::Sync) {
            return Err(io::Error::other("fsync failed (simulated)"));
        }
        f.sync_all()?;
        drop(f);

        let mut back = Vec::with_capacity(data.len());
        File::open(&tmp)?.read_to_end(&mut back)?;
        if back != data {
            return Err(io::Error::other(
                "written file does not match what was written",
            ));
        }
        if should_fail(FailAt::BeforeRename) {
            // Simulated power loss: the process dies here, so the temp file
            // stays on disk and the target keeps its old contents.
            return Err(io::Error::other("crashed before rename (simulated)"));
        }
        fs::rename(&tmp, path)?;
        sync_dir(dir);
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// fsync a directory so a completed rename survives power loss. Best-effort:
/// not every platform or filesystem supports it.
pub fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// Create a directory (and parents) readable only by the owner on Unix.
pub fn create_private_dir(dir: &Path) -> io::Result<()> {
    let mut b = fs::DirBuilder::new();
    b.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        // Owner-only from the moment it exists.
        b.mode(0o700);
    }
    b.create(dir)?;
    #[cfg(unix)]
    {
        // An already-existing folder is tightened too.
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Is this one of our temp files?
pub fn is_temp_name(name: &str) -> bool {
    name.starts_with('.') && name.ends_with(".tmp")
}

/// Remove temp files older than `age` left by a crash. Recent ones may belong
/// to a write in progress on this or another machine, so they're kept.
pub fn clean_stale_temps(dir: &Path, age: Duration) -> usize {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let now = SystemTime::now();
    let mut removed = 0;
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(name) = name.to_str() else { continue };
        if !is_temp_name(name) {
            continue;
        }
        let old = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|d| d >= age);
        if old && fs::remove_file(e.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temps(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| is_temp_name(n))
            .collect()
    }

    #[test]
    fn replaces_contents_and_leaves_no_temp() {
        let d = tempfile::TempDir::new().unwrap();
        let p = d.path().join("rec.enc");
        write(&p, b"one").unwrap();
        write(&p, b"two, longer").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two, longer");
        assert!(temps(d.path()).is_empty());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&p).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn failures_never_touch_the_target() {
        let d = tempfile::TempDir::new().unwrap();
        let p = d.path().join("rec.enc");
        write(&p, b"original").unwrap();
        for stage in [FailAt::Write, FailAt::Sync] {
            fail_next(stage);
            assert!(write(&p, b"replacement").is_err());
            assert_eq!(fs::read(&p).unwrap(), b"original", "{stage:?}");
            assert!(temps(d.path()).is_empty(), "{stage:?} left a temp file");
        }
        // Disk full reports the right kind.
        fail_next(FailAt::Write);
        assert_eq!(
            write(&p, b"x").unwrap_err().kind(),
            io::ErrorKind::StorageFull
        );
    }

    #[test]
    fn crash_before_rename_keeps_old_contents() {
        let d = tempfile::TempDir::new().unwrap();
        let p = d.path().join("rec.enc");
        write(&p, b"original").unwrap();
        fail_next(FailAt::BeforeRename);
        assert!(write(&p, b"replacement").is_err());
        assert_eq!(fs::read(&p).unwrap(), b"original");
    }

    #[test]
    fn stale_temps_are_cleaned_but_fresh_ones_kept() {
        let d = tempfile::TempDir::new().unwrap();
        fs::write(d.path().join(".rec.enc.0000.tmp"), b"junk").unwrap();
        fs::write(d.path().join("keep.enc"), b"real").unwrap();
        assert_eq!(clean_stale_temps(d.path(), Duration::from_secs(3600)), 0);
        assert_eq!(clean_stale_temps(d.path(), Duration::ZERO), 1);
        assert!(d.path().join("keep.enc").exists());
    }

    #[cfg(unix)]
    #[test]
    fn permission_denied_is_reported_cleanly() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::TempDir::new().unwrap();
        let ro = d.path().join("ro");
        fs::create_dir(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o500)).unwrap();
        // Root ignores permissions; the check only means something otherwise.
        if fs::write(ro.join("probe"), b"x").is_ok() {
            return;
        }
        let err = write(&ro.join("rec.enc"), b"x").unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o700)).unwrap();
    }
}
