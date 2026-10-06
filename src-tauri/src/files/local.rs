//! This computer's disk as a [`FileBackend`].

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use super::{Caps, FileBackend, FileError, Reader, Stat, Writer};
use crate::sftp::{local, FileEntry};

pub struct LocalBackend;

fn err(path: &Path, source: std::io::Error) -> FileError {
    FileError::Local { path: path.to_path_buf(), source }
}

#[async_trait]
impl FileBackend for LocalBackend {
    fn caps(&self) -> Caps {
        // Permissions are shown but not edited here: the OS file manager does that.
        Caps::files()
    }

    async fn home(&self) -> Result<String, FileError> {
        Ok(local::home().to_string_lossy().into_owned())
    }

    async fn list(&self, dir: &str) -> Result<Vec<FileEntry>, FileError> {
        let dir = PathBuf::from(dir);
        tokio::task::spawn_blocking(move || local::list(&dir)).await.map_err(|e| FileError::Local { path: PathBuf::new(), source: std::io::Error::other(e) })?
    }

    async fn stat(&self, path: &str) -> Result<Option<Stat>, FileError> {
        match tokio::fs::metadata(path).await {
            Ok(m) => Ok(Some(Stat { is_dir: m.is_dir(), size: if m.is_dir() { 0 } else { m.len() } })),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(err(Path::new(path), e)),
        }
    }

    async fn mkdir(&self, dir: &str, name: &str) -> Result<(), FileError> {
        local::mkdir(Path::new(dir), name)
    }

    async fn rename(&self, from: &str, new_name: &str) -> Result<(), FileError> {
        local::rename(Path::new(from), new_name)
    }

    async fn remove(&self, path: &str) -> Result<(), FileError> {
        let p = PathBuf::from(path);
        tokio::task::spawn_blocking(move || local::remove(&p)).await.map_err(|e| FileError::Local { path: PathBuf::new(), source: std::io::Error::other(e) })?
    }

    async fn read(&self, path: &str, offset: u64) -> Result<Reader, FileError> {
        use tokio::io::AsyncSeekExt;
        let p = Path::new(path);
        let mut f = tokio::fs::File::open(p).await.map_err(|e| err(p, e))?;
        if offset > 0 {
            f.seek(std::io::SeekFrom::Start(offset)).await.map_err(|e| err(p, e))?;
        }
        Ok(Box::new(f))
    }

    async fn write(&self, path: &str, offset: u64, _size: u64) -> Result<Writer, FileError> {
        let p = Path::new(path);
        let f = if offset > 0 {
            tokio::fs::OpenOptions::new().append(true).open(p).await
        } else {
            tokio::fs::File::create(p).await
        };
        Ok(Box::new(f.map_err(|e| err(p, e))?))
    }

    // Paths follow the operating system: backslashes and drive letters on Windows.

    fn join(&self, dir: &str, name: &str) -> String {
        Path::new(dir).join(name).to_string_lossy().into_owned()
    }

    fn basename(&self, path: &str) -> Result<String, FileError> {
        Path::new(path).file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| FileError::InvalidPath(path.to_string()))
    }

    fn parent(&self, path: &str) -> Option<String> {
        Path::new(path).parent().filter(|p| !p.as_os_str().is_empty()).map(|p| p.to_string_lossy().into_owned())
    }
}
