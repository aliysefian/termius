//! The Config Time-Machine's store: copies of remote files as they were before an edit, kept on this computer.
//!
//! Every snapshot is two files, both encrypted with the vault key (a configuration file can hold passwords, so
//! nothing here is plain text): `<ms>-<id>.meta` (what, when, who) and `<ms>-<id>.blob` (the content). Folders are
//! named by host and by a hash of the path, so the disk shows neither. Local only; never synced or backed up.
//! A new version is stored only when its SHA-256 differs from the newest one of that file.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::containers::transport::shell_quote;
use crate::crypto::{self, MasterKey};
use crate::ssh::Client;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Snapshot {
    pub id: String,
    pub host_id: Uuid,
    pub file_path: String,
    pub sha256: String,
    pub size_bytes: u64,
    /// `0644`, or empty if the host did not say.
    pub permissions: String,
    /// `root:root`, or empty.
    pub owner: String,
    /// Milliseconds since the epoch.
    pub captured_at: u64,
    /// `Terminal (vim)`, `SFTP Editor`, `Manual Pre-Rollback`…
    pub trigger_source: String,
    pub ssh_user: String,
    /// 1, 2, 3… for this file; never reused, even after old versions are pruned.
    pub version: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TrackedFile {
    pub host_id: Uuid,
    pub file_path: String,
    pub versions: usize,
    pub latest_at: u64,
    pub latest_size: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Usage {
    pub bytes: u64,
    pub snapshots: usize,
}

/// What the caller knows about the file besides its text.
#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub permissions: String,
    pub owner: String,
    pub trigger_source: String,
    pub ssh_user: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stored {
    Saved(Snapshot),
    /// The newest version already has this content.
    Unchanged(Snapshot),
}

fn other(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

fn aad(kind: &str, host: Uuid, id: &str) -> Vec<u8> {
    format!("sshvault.confighistory.{kind}/{host}/{id}").into_bytes()
}

/// Ids are UUIDs; anything else can't name a file here.
fn valid_id(id: &str) -> bool {
    Uuid::parse_str(id).is_ok()
}

#[derive(Clone)]
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn host_dir(&self, host: Uuid) -> PathBuf {
        self.dir.join(host.to_string())
    }

    fn file_dir(&self, host: Uuid, path: &str) -> PathBuf {
        self.host_dir(host).join(&crypto::sha256_hex(path.as_bytes())[..16])
    }

    /// `(captured ms, id)` of every snapshot in a folder, oldest first.
    fn entries(dir: &Path) -> Vec<(u64, String)> {
        let mut out: Vec<(u64, String)> = std::fs::read_dir(dir)
            .map(|d| {
                d.flatten()
                    .filter_map(|e| {
                        let name = e.file_name().to_string_lossy().to_string();
                        let stem = name.strip_suffix(".meta")?;
                        let (ts, id) = stem.split_once('-')?;
                        Some((ts.parse().ok()?, id.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }

    fn write(path: &Path, data: &[u8]) -> io::Result<()> {
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, data)?;
        std::fs::rename(&tmp, path)
    }

    fn read_meta(&self, key: &MasterKey, host: Uuid, dir: &Path, ts: u64, id: &str) -> Option<Snapshot> {
        let bytes = std::fs::read(dir.join(format!("{ts:013}-{id}.meta"))).ok()?;
        let plain = crypto::decrypt(key, &bytes, &aad("meta", host, id)).ok()?;
        serde_json::from_slice(&plain).ok()
    }

    /// Keep `content` as the newest version of `path` on `host`, unless it is what the newest version already holds.
    pub fn store(&self, key: &MasterKey, host: Uuid, path: &str, content: &[u8], meta: Meta, now_ms: u64) -> io::Result<Stored> {
        let dir = self.file_dir(host, path);
        let sha = crypto::sha256_hex(content);
        let entries = Self::entries(&dir);
        let mut version = 1;
        if let Some((ts, id)) = entries.last() {
            if let Some(last) = self.read_meta(key, host, &dir, *ts, id) {
                if last.sha256 == sha {
                    return Ok(Stored::Unchanged(last));
                }
                version = last.version + 1;
            } else {
                version = entries.len() as u32 + 1;
            }
        }
        std::fs::create_dir_all(&dir)?;
        let id = Uuid::new_v4().to_string();
        // A clock that went back must not reorder the history.
        let ts = entries.last().map_or(now_ms, |(t, _)| now_ms.max(*t + 1));
        let snap = Snapshot {
            id: id.clone(),
            host_id: host,
            file_path: path.to_string(),
            sha256: sha,
            size_bytes: content.len() as u64,
            permissions: meta.permissions,
            owner: meta.owner,
            captured_at: ts,
            trigger_source: meta.trigger_source,
            ssh_user: meta.ssh_user,
            version,
        };
        let blob = crypto::encrypt(key, content, &aad("blob", host, &id)).map_err(other)?;
        let head = crypto::encrypt(key, &serde_json::to_vec(&snap).map_err(other)?, &aad("meta", host, &id)).map_err(other)?;
        // Content first: a snapshot is listed only once its meta exists, so a crash between the two leaves no half entry.
        Self::write(&dir.join(format!("{ts:013}-{id}.blob")), &blob)?;
        Self::write(&dir.join(format!("{ts:013}-{id}.meta")), &head)?;
        Ok(Stored::Saved(snap))
    }

    /// Every version of one file, newest first.
    pub fn versions(&self, key: &MasterKey, host: Uuid, path: &str) -> Vec<Snapshot> {
        let dir = self.file_dir(host, path);
        Self::entries(&dir).iter().rev().filter_map(|(ts, id)| self.read_meta(key, host, &dir, *ts, id)).collect()
    }

    /// All tracked files, optionally of one host; most recently changed first.
    pub fn files(&self, key: &MasterKey, only: Option<Uuid>) -> Vec<TrackedFile> {
        let mut out = Vec::new();
        let hosts: Vec<PathBuf> = std::fs::read_dir(&self.dir).map(|d| d.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default();
        for h in hosts {
            let Some(host) = h.file_name().and_then(|n| n.to_str()).and_then(|n| Uuid::parse_str(n).ok()) else { continue };
            if only.is_some_and(|o| o != host) {
                continue;
            }
            let Ok(files) = std::fs::read_dir(&h) else { continue };
            for f in files.flatten().map(|e| e.path()).filter(|p| p.is_dir()) {
                let entries = Self::entries(&f);
                let Some((ts, id)) = entries.last() else { continue };
                if let Some(latest) = self.read_meta(key, host, &f, *ts, id) {
                    out.push(TrackedFile { host_id: host, file_path: latest.file_path, versions: entries.len(), latest_at: latest.captured_at, latest_size: latest.size_bytes });
                }
            }
        }
        out.sort_by(|a, b| b.latest_at.cmp(&a.latest_at));
        out
    }

    /// The content of one snapshot.
    pub fn read(&self, key: &MasterKey, host: Uuid, path: &str, id: &str) -> Option<Vec<u8>> {
        if !valid_id(id) {
            return None;
        }
        let dir = self.file_dir(host, path);
        let (ts, _) = Self::entries(&dir).into_iter().find(|(_, i)| i == id)?;
        let bytes = std::fs::read(dir.join(format!("{ts:013}-{id}.blob"))).ok()?;
        crypto::decrypt(key, &bytes, &aad("blob", host, id)).ok()
    }

    /// Forget one file's whole history.
    pub fn delete_file(&self, host: Uuid, path: &str) -> bool {
        std::fs::remove_dir_all(self.file_dir(host, path)).is_ok()
    }

    pub fn clear(&self) -> usize {
        let n = self.usage().snapshots;
        let _ = std::fs::remove_dir_all(&self.dir);
        n
    }

    pub fn usage(&self) -> Usage {
        let (mut bytes, mut snapshots) = (0, 0);
        for host in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            for file in std::fs::read_dir(host.path()).into_iter().flatten().flatten() {
                for e in std::fs::read_dir(file.path()).into_iter().flatten().flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.ends_with(".meta") {
                        snapshots += 1;
                    }
                    if name.ends_with(".meta") || name.ends_with(".blob") {
                        bytes += e.metadata().map(|m| m.len()).unwrap_or(0);
                    }
                }
            }
        }
        Usage { bytes, snapshots }
    }

    /// Drop snapshots older than `days` (0: keep everything). The newest version of each file always stays, so a
    /// file never loses its last known good copy to the clock.
    pub fn prune(&self, days: u32, now_ms: u64) -> usize {
        if days == 0 {
            return 0;
        }
        let cutoff = now_ms.saturating_sub(u64::from(days) * 86_400_000);
        let mut gone = 0;
        for host in std::fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            for file in std::fs::read_dir(host.path()).into_iter().flatten().flatten() {
                let dir = file.path();
                let entries = Self::entries(&dir);
                for (ts, id) in entries.iter().take(entries.len().saturating_sub(1)) {
                    if *ts < cutoff {
                        let _ = std::fs::remove_file(dir.join(format!("{ts:013}-{id}.blob")));
                        if std::fs::remove_file(dir.join(format!("{ts:013}-{id}.meta"))).is_ok() {
                            gone += 1;
                        }
                    }
                }
            }
        }
        gone
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// -- reading a file on the host ---------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteRead {
    File { content: Vec<u8>, permissions: String, owner: String },
    /// There is no such file (yet): nothing to keep.
    Missing,
    TooBig(u64),
    Binary,
    Denied,
    /// The host would not run the command.
    Unavailable,
}

/// `$1` is the path (`~/` is the home folder). Prints a header line and then the file; reads with `sudo -n` when the
/// user may not read it. Everything about the file comes before its text, so a big file is refused unread.
const READ_SCRIPT: &str = concat!(
    r##"case "$1" in "~") f=$HOME;; "~/"*) f=$HOME/${1#"~/"};; *) f=$1;; esac; "##,
    r##"[ -f "$f" ] || { echo '@@MISSING'; exit 0; }; "##,
    r##"m=$(stat -c '%s %a %U:%G' "$f" 2>/dev/null || stat -f '%z %Lp %Su:%Sg' "$f" 2>/dev/null) || m='0 - -'; "##,
    r##"set -- $m; "##,
    r##"[ "$1" -le "$MAXBYTES" ] 2>/dev/null || { echo "@@TOOBIG $1"; exit 0; }; "##,
    r##"echo "@@OK $m"; "##,
    r##"if [ -r "$f" ]; then cat "$f"; else sudo -n cat "$f" 2>/dev/null || exit 13; fi"##,
);

fn command(path: &str, max: u64) -> String {
    let script = format!("PATH=\"$PATH:/usr/local/bin:/opt/homebrew/bin\"; export PATH; MAXBYTES={max}; {READ_SCRIPT}");
    format!("sh -c {} sh {}", shell_quote(&script), shell_quote(path))
}

/// `0644` from `644`.
fn mode4(m: &str) -> String {
    if m.chars().all(|c| c.is_ascii_digit()) && !m.is_empty() {
        format!("{m:0>4}")
    } else {
        String::new()
    }
}

pub fn parse_read(out: &[u8], code: Option<u32>) -> RemoteRead {
    let nl = out.iter().position(|b| *b == b'\n');
    let head = String::from_utf8_lossy(&out[..nl.unwrap_or(out.len())]).to_string();
    if head.starts_with("@@MISSING") {
        return RemoteRead::Missing;
    }
    if let Some(n) = head.strip_prefix("@@TOOBIG ") {
        return RemoteRead::TooBig(n.trim().parse().unwrap_or(0));
    }
    let Some(rest) = head.strip_prefix("@@OK ") else {
        return if code == Some(13) { RemoteRead::Denied } else { RemoteRead::Unavailable };
    };
    if code == Some(13) {
        return RemoteRead::Denied;
    }
    let f: Vec<&str> = rest.split_whitespace().collect();
    let content = nl.map(|i| out[i + 1..].to_vec()).unwrap_or_default();
    if content.contains(&0) {
        return RemoteRead::Binary;
    }
    RemoteRead::File { content, permissions: f.get(1).map(|m| mode4(m)).unwrap_or_default(), owner: f.get(2).filter(|o| **o != "-").map(|o| o.to_string()).unwrap_or_default() }
}

/// Read a file over an extra channel of a connection (the user's own session is not touched).
pub async fn read_remote(client: &Client, path: &str, max: u64) -> RemoteRead {
    let limit = std::time::Duration::from_secs(15);
    match client.exec(&command(path, max), None, limit, (max as usize).saturating_add(4096)).await {
        Ok(o) if o.truncated => RemoteRead::TooBig(max + 1),
        Ok(o) => parse_read(&o.stdout, o.code),
        Err(_) => RemoteRead::Unavailable,
    }
}

/// A remote path we are willing to ask about: absolute or under `~/`, one line, no NUL.
pub fn valid_path(p: &str) -> bool {
    !p.is_empty() && p.len() <= 4096 && (p.starts_with('/') || p.starts_with("~/")) && !p.contains(['\0', '\n', '\r'])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> MasterKey {
        MasterKey::from_bytes([7u8; 32])
    }

    fn meta(trigger: &str) -> Meta {
        Meta { permissions: "0644".into(), owner: "root:root".into(), trigger_source: trigger.into(), ssh_user: "ali".into() }
    }

    #[test]
    fn a_changed_file_is_a_new_version_and_an_unchanged_one_is_not() {
        let d = tempfile::TempDir::new().unwrap();
        let s = Store::new(d.path());
        let host = Uuid::new_v4();
        let Stored::Saved(a) = s.store(&key(), host, "/etc/nginx/nginx.conf", b"one", meta("Terminal (vim)"), 1000).unwrap() else { panic!() };
        assert_eq!(a.version, 1);
        assert!(matches!(s.store(&key(), host, "/etc/nginx/nginx.conf", b"one", meta("SFTP Editor"), 2000).unwrap(), Stored::Unchanged(x) if x.id == a.id));
        let Stored::Saved(b) = s.store(&key(), host, "/etc/nginx/nginx.conf", b"two", meta("SFTP Editor"), 3000).unwrap() else { panic!() };
        assert_eq!(b.version, 2);
        // Going back to the first text is a change from the newest one, so it is kept.
        let Stored::Saved(c) = s.store(&key(), host, "/etc/nginx/nginx.conf", b"one", meta("Manual Pre-Rollback"), 4000).unwrap() else { panic!() };
        assert_eq!(c.version, 3);
        let v = s.versions(&key(), host, "/etc/nginx/nginx.conf");
        assert_eq!(v.iter().map(|x| x.version).collect::<Vec<_>>(), vec![3, 2, 1]);
        assert_eq!(v[2].permissions, "0644");
        assert_eq!(v[2].trigger_source, "Terminal (vim)");
        assert_eq!(s.read(&key(), host, "/etc/nginx/nginx.conf", &b.id).unwrap(), b"two");
    }

    #[test]
    fn nothing_on_disk_is_readable_without_the_key() {
        let d = tempfile::TempDir::new().unwrap();
        let s = Store::new(d.path());
        let host = Uuid::new_v4();
        s.store(&key(), host, "/etc/secret.conf", b"password=hunter2", meta("x"), 1).unwrap();
        for entry in walk(d.path()) {
            let bytes = std::fs::read(&entry).unwrap();
            let text = String::from_utf8_lossy(&bytes);
            assert!(!text.contains("hunter2") && !text.contains("secret.conf"), "{entry:?}");
        }
        let other = MasterKey::from_bytes([9u8; 32]);
        assert!(s.versions(&other, host, "/etc/secret.conf").is_empty());
        let id = s.versions(&key(), host, "/etc/secret.conf")[0].id.clone();
        assert!(s.read(&other, host, "/etc/secret.conf", &id).is_none());
    }

    fn walk(p: &Path) -> Vec<PathBuf> {
        let mut out = vec![];
        for e in std::fs::read_dir(p).unwrap().flatten() {
            if e.path().is_dir() {
                out.extend(walk(&e.path()));
            } else {
                out.push(e.path());
            }
        }
        out
    }

    #[test]
    fn files_are_listed_per_host_with_their_version_counts() {
        let d = tempfile::TempDir::new().unwrap();
        let s = Store::new(d.path());
        let (h1, h2) = (Uuid::new_v4(), Uuid::new_v4());
        s.store(&key(), h1, "/etc/a", b"1", meta("x"), 10).unwrap();
        s.store(&key(), h1, "/etc/a", b"2", meta("x"), 20).unwrap();
        s.store(&key(), h1, "/etc/b", b"1", meta("x"), 30).unwrap();
        s.store(&key(), h2, "/etc/a", b"1", meta("x"), 5).unwrap();
        let all = s.files(&key(), None);
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].file_path, "/etc/b");
        let one = s.files(&key(), Some(h1));
        assert_eq!(one.iter().map(|f| (f.file_path.as_str(), f.versions)).collect::<Vec<_>>(), vec![("/etc/b", 1), ("/etc/a", 2)]);
        assert_eq!(s.usage().snapshots, 4);
        assert!(s.usage().bytes > 0);
        assert!(s.delete_file(h1, "/etc/a"));
        assert_eq!(s.files(&key(), None).len(), 2);
        assert_eq!(s.clear(), 2);
        assert_eq!(s.usage(), Usage { bytes: 0, snapshots: 0 });
    }

    #[test]
    fn pruning_keeps_the_newest_version_of_every_file() {
        let d = tempfile::TempDir::new().unwrap();
        let s = Store::new(d.path());
        let host = Uuid::new_v4();
        let day = 86_400_000;
        s.store(&key(), host, "/etc/a", b"1", meta("x"), day).unwrap();
        s.store(&key(), host, "/etc/a", b"2", meta("x"), 2 * day).unwrap();
        s.store(&key(), host, "/etc/b", b"1", meta("x"), day).unwrap();
        assert_eq!(s.prune(0, 100 * day), 0);
        // Everything is older than 7 days, but each file keeps its last copy.
        assert_eq!(s.prune(7, 100 * day), 1);
        assert_eq!(s.versions(&key(), host, "/etc/a").iter().map(|v| v.version).collect::<Vec<_>>(), vec![2]);
        assert_eq!(s.versions(&key(), host, "/etc/b").len(), 1);
    }

    #[test]
    fn a_clock_that_goes_back_does_not_reorder_versions() {
        let d = tempfile::TempDir::new().unwrap();
        let s = Store::new(d.path());
        let host = Uuid::new_v4();
        s.store(&key(), host, "/etc/a", b"1", meta("x"), 5000).unwrap();
        s.store(&key(), host, "/etc/a", b"2", meta("x"), 1000).unwrap();
        assert_eq!(s.versions(&key(), host, "/etc/a")[0].version, 2);
    }

    #[test]
    fn ids_that_are_not_uuids_name_no_file() {
        let d = tempfile::TempDir::new().unwrap();
        let s = Store::new(d.path());
        assert!(s.read(&key(), Uuid::new_v4(), "/etc/a", "../../etc/passwd").is_none());
    }

    #[test]
    fn the_remote_listing_is_read_by_its_header() {
        assert_eq!(parse_read(b"@@MISSING\n", Some(0)), RemoteRead::Missing);
        assert_eq!(parse_read(b"@@TOOBIG 9999999\n", Some(0)), RemoteRead::TooBig(9_999_999));
        assert_eq!(parse_read(b"", Some(255)), RemoteRead::Unavailable);
        assert_eq!(parse_read(b"@@OK 4 644 root:root\n", Some(13)), RemoteRead::Denied);
        assert_eq!(
            parse_read(b"@@OK 6 644 root:root\nhello\n", Some(0)),
            RemoteRead::File { content: b"hello\n".to_vec(), permissions: "0644".into(), owner: "root:root".into() }
        );
        assert_eq!(parse_read(b"@@OK 3 600 ali:ali\na\0b", Some(0)), RemoteRead::Binary);
        // A file with no text is still a file.
        assert_eq!(parse_read(b"@@OK 0 600 ali:ali\n", Some(0)), RemoteRead::File { content: vec![], permissions: "0600".into(), owner: "ali:ali".into() });
    }

    #[test]
    fn only_plain_remote_paths_are_asked_about() {
        assert!(valid_path("/etc/nginx/nginx.conf"));
        assert!(valid_path("~/app/.env"));
        assert!(!valid_path("etc/passwd"));
        assert!(!valid_path("/etc/x\n; reboot"));
        assert!(!valid_path(""));
        let c = command("/etc/it's.conf", 100);
        assert!(c.starts_with("sh -c '") && c.ends_with("sh '/etc/it'\\''s.conf'"));
    }
}
