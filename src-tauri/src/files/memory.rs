//! A file backend that lives in memory, for tests: no disk, no network, and
//! knobs for what it supports and how slowly it answers.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncWrite;

use super::{Caps, FileBackend, FileError, Reader, Stat, Writer};
use crate::sftp::FileEntry;

#[derive(Clone)]
enum Node {
    Dir,
    File(Vec<u8>),
}

#[derive(Default)]
struct Tree {
    nodes: BTreeMap<String, Node>,
}

/// POSIX paths, `/` at the top.
pub struct MemoryBackend {
    tree: Arc<Mutex<Tree>>,
    caps: Caps,
    /// How long opening a file for writing takes (to make a transfer slow enough to cancel).
    open_delay: Duration,
    /// Paths that refuse to be written.
    pub refuse: Mutex<Vec<String>>,
    reads: Mutex<Vec<(String, u64)>>,
}

impl Default for MemoryBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryBackend {
    pub fn new() -> Self {
        let tree = Tree { nodes: BTreeMap::from([("/".to_string(), Node::Dir)]) };
        Self { tree: Arc::new(Mutex::new(tree)), caps: Caps::files(), open_delay: Duration::ZERO, refuse: Mutex::new(Vec::new()), reads: Mutex::new(Vec::new()) }
    }

    pub fn with_caps(mut self, caps: Caps) -> Self {
        self.caps = caps;
        self
    }

    pub fn with_open_delay(mut self, d: Duration) -> Self {
        self.open_delay = d;
        self
    }

    /// Put a file here, making its folders.
    pub fn put(&self, path: &str, data: &[u8]) {
        let mut t = self.tree.lock().unwrap();
        let mut dir = String::new();
        for part in path.trim_matches('/').split('/').collect::<Vec<_>>().split_last().map(|(_, p)| p.to_vec()).unwrap_or_default() {
            dir = format!("{dir}/{part}");
            t.nodes.entry(dir.clone()).or_insert(Node::Dir);
        }
        t.nodes.insert(path.to_string(), Node::File(data.to_vec()));
    }

    pub fn mkdirs(&self, path: &str) {
        let mut t = self.tree.lock().unwrap();
        let mut dir = String::new();
        for part in path.trim_matches('/').split('/').filter(|p| !p.is_empty()) {
            dir = format!("{dir}/{part}");
            t.nodes.entry(dir.clone()).or_insert(Node::Dir);
        }
    }

    pub fn get(&self, path: &str) -> Option<Vec<u8>> {
        match self.tree.lock().unwrap().nodes.get(path) {
            Some(Node::File(d)) => Some(d.clone()),
            _ => None,
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.tree.lock().unwrap().nodes.contains_key(path)
    }

    /// Every path under `/`, sorted.
    pub fn paths(&self) -> Vec<String> {
        self.tree.lock().unwrap().nodes.keys().filter(|k| *k != "/").cloned().collect()
    }

    /// The (path, offset) of every read so far.
    pub fn reads(&self) -> Vec<(String, u64)> {
        self.reads.lock().unwrap().clone()
    }
}

fn gone(path: &str) -> FileError {
    FileError::Backend(format!("{path}: no such file or directory"))
}

struct MemWriter {
    tree: Arc<Mutex<Tree>>,
    path: String,
}

impl AsyncWrite for MemWriter {
    fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        if let Some(Node::File(d)) = self.tree.lock().unwrap().nodes.get_mut(&self.path) {
            d.extend_from_slice(buf);
        }
        Poll::Ready(Ok(buf.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

#[async_trait]
impl FileBackend for MemoryBackend {
    fn caps(&self) -> Caps {
        self.caps
    }

    async fn home(&self) -> Result<String, FileError> {
        Ok("/".into())
    }

    async fn list(&self, dir: &str) -> Result<Vec<FileEntry>, FileError> {
        let t = self.tree.lock().unwrap();
        if !matches!(t.nodes.get(dir), Some(Node::Dir)) {
            return Err(gone(dir));
        }
        let prefix = if dir == "/" { "/".to_string() } else { format!("{dir}/") };
        let mut out: Vec<FileEntry> = t
            .nodes
            .iter()
            .filter_map(|(p, n)| {
                let name = p.strip_prefix(&prefix)?;
                if name.is_empty() || name.contains('/') {
                    return None;
                }
                Some(FileEntry {
                    name: name.to_string(),
                    path: p.clone(),
                    is_dir: matches!(n, Node::Dir),
                    is_symlink: false,
                    size: if let Node::File(d) = n { d.len() as u64 } else { 0 },
                    modified: None,
                    permissions: None,
                })
            })
            .collect();
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(out)
    }

    async fn stat(&self, path: &str) -> Result<Option<Stat>, FileError> {
        Ok(self.tree.lock().unwrap().nodes.get(path).map(|n| match n {
            Node::Dir => Stat { is_dir: true, size: 0 },
            Node::File(d) => Stat { is_dir: false, size: d.len() as u64 },
        }))
    }

    async fn mkdir(&self, dir: &str, name: &str) -> Result<(), FileError> {
        let mut t = self.tree.lock().unwrap();
        let path = self.join(dir, name);
        if !matches!(t.nodes.get(dir), Some(Node::Dir)) {
            return Err(gone(dir));
        }
        if t.nodes.contains_key(&path) {
            return Err(FileError::Backend(format!("{path}: already exists")));
        }
        t.nodes.insert(path, Node::Dir);
        Ok(())
    }

    async fn rename(&self, from: &str, new_name: &str) -> Result<(), FileError> {
        let mut t = self.tree.lock().unwrap();
        let parent = self.parent(from).ok_or_else(|| FileError::InvalidPath(from.to_string()))?;
        let to = self.join(&parent, new_name);
        if !t.nodes.contains_key(from) {
            return Err(gone(from));
        }
        let moved: Vec<String> = t.nodes.keys().filter(|k| *k == from || k.starts_with(&format!("{from}/"))).cloned().collect();
        for old in moved {
            let node = t.nodes.remove(&old).unwrap();
            t.nodes.insert(format!("{to}{}", &old[from.len()..]), node);
        }
        Ok(())
    }

    async fn remove(&self, path: &str) -> Result<(), FileError> {
        let mut t = self.tree.lock().unwrap();
        if !t.nodes.contains_key(path) {
            return Err(gone(path));
        }
        t.nodes.retain(|k, _| k != path && !k.starts_with(&format!("{path}/")));
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64) -> Result<Reader, FileError> {
        self.reads.lock().unwrap().push((path.to_string(), offset));
        match self.tree.lock().unwrap().nodes.get(path) {
            Some(Node::File(d)) => Ok(Box::new(Cursor::new(d[(offset as usize).min(d.len())..].to_vec()))),
            _ => Err(gone(path)),
        }
    }

    async fn write(&self, path: &str, offset: u64, _size: u64) -> Result<Writer, FileError> {
        if !self.open_delay.is_zero() {
            tokio::time::sleep(self.open_delay).await;
        }
        if self.refuse.lock().unwrap().iter().any(|p| p == path) {
            return Err(FileError::Backend(format!("{path}: permission denied")));
        }
        let mut t = self.tree.lock().unwrap();
        let parent = self.parent(path).ok_or_else(|| FileError::InvalidPath(path.to_string()))?;
        if !matches!(t.nodes.get(&parent), Some(Node::Dir)) {
            return Err(gone(&parent));
        }
        if offset == 0 {
            t.nodes.insert(path.to_string(), Node::File(Vec::new()));
        } else if !matches!(t.nodes.get(path), Some(Node::File(d)) if d.len() as u64 == offset) {
            return Err(FileError::Backend(format!("{path}: can't continue from {offset}")));
        }
        Ok(Box::new(MemWriter { tree: Arc::clone(&self.tree), path: path.to_string() }))
    }
}
