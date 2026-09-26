//! Read and edit the app's private `known_hosts` file (trust-on-first-use
//! pins written by `ssh.rs`). Same format as OpenSSH's.

use std::io;
use std::path::Path;

use russh::keys::ssh_key::{HashAlg, PublicKey};
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KnownHost {
    /// 1-based line number, used to identify the entry for removal.
    pub line: usize,
    /// Host field as written, e.g. `example.com` or `[10.0.0.5]:2222`.
    pub hosts: Vec<String>,
    pub hashed: bool,
    pub algorithm: String,
    /// SHA256 fingerprint, or `None` if the key could not be parsed.
    pub fingerprint: Option<String>,
}

/// The token OpenSSH (and russh) writes for a host/port pair.
pub fn host_token(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

pub fn list(path: &Path) -> io::Result<Vec<KnownHost>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.split_whitespace();
        let (Some(hosts), Some(algo), Some(b64)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        // Skip @cert-authority / @revoked markers' shifted columns.
        if hosts.starts_with('@') {
            continue;
        }
        let fingerprint = PublicKey::from_openssh(&format!("{algo} {b64}"))
            .ok()
            .map(|k| k.fingerprint(HashAlg::Sha256).to_string());
        out.push(KnownHost {
            line: i + 1,
            hosts: hosts.split(',').map(str::to_string).collect(),
            hashed: hosts.starts_with("|1|"),
            algorithm: algo.to_string(),
            fingerprint,
        });
    }
    Ok(out)
}

/// Remove the entry on `line` (as reported by [`list`]). Returns whether a
/// line was removed. Written atomically.
pub fn remove_line(path: &Path, line: usize) -> io::Result<bool> {
    rewrite(path, |i, _| i + 1 != line)
}

/// Forget every key recorded for `host:port`. Other hosts sharing a line
/// (comma-separated host lists) keep their entry. Returns the number of
/// lines changed.
pub fn forget(path: &Path, host: &str, port: u16) -> io::Result<usize> {
    let token = host_token(host, port);
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let mut changed = 0;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let mut cols = line.splitn(2, char::is_whitespace);
        let (Some(hosts), Some(rest)) = (cols.next(), cols.next()) else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let list: Vec<&str> = hosts.split(',').collect();
        if !list.iter().any(|h| *h == token) || line.trim_start().starts_with('#') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        changed += 1;
        let kept: Vec<&str> = list.into_iter().filter(|h| *h != token).collect();
        if !kept.is_empty() {
            out.push_str(&kept.join(","));
            out.push(' ');
            out.push_str(rest);
            out.push('\n');
        }
    }
    if changed > 0 {
        atomic_write(path, &out)?;
    }
    Ok(changed)
}

fn rewrite(path: &Path, keep: impl Fn(usize, &str) -> bool) -> io::Result<bool> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let mut removed = false;
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.lines().enumerate() {
        if keep(i, line) {
            out.push_str(line);
            out.push('\n');
        } else {
            removed = true;
        }
    }
    if removed {
        atomic_write(path, &out)?;
    }
    Ok(removed)
}

fn atomic_write(path: &Path, text: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> String {
        // "ssh-ed25519 <base64> comment" -> keep type and key only.
        let k = crate::keys::generate_ed25519("").unwrap().public_key;
        k.split_whitespace().take(2).collect::<Vec<_>>().join(" ")
    }

    fn setup() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("known_hosts");
        let (key_a, key_b) = (key(), key());
        std::fs::write(
            &path,
            format!(
                "# comment\nexample.com {key_a}\n[10.0.0.5]:2222,alias {key_b}\n|1|abc=|def= {key_a}\n\nbroken\n"
            ),
        )
        .unwrap();
        (dir, path)
    }

    #[test]
    fn lists_entries_with_fingerprints() {
        let (_d, path) = setup();
        let l = list(&path).unwrap();
        assert_eq!(l.len(), 3);
        assert_eq!(l[0].hosts, ["example.com"]);
        assert_eq!(l[0].line, 2);
        assert!(l[0].fingerprint.as_deref().unwrap().starts_with("SHA256:"));
        assert_eq!(l[1].hosts, ["[10.0.0.5]:2222", "alias"]);
        assert!(l[2].hashed);
        assert!(list(&path.with_file_name("missing")).unwrap().is_empty());
    }

    #[test]
    fn forget_keeps_other_hosts_on_the_line() {
        let (_d, path) = setup();
        assert_eq!(forget(&path, "10.0.0.5", 2222).unwrap(), 1);
        let l = list(&path).unwrap();
        assert_eq!(l[1].hosts, ["alias"]);
        assert_eq!(forget(&path, "example.com", 22).unwrap(), 1);
        assert!(list(&path)
            .unwrap()
            .iter()
            .all(|h| h.hosts != ["example.com"]));
        assert_eq!(forget(&path, "nope", 22).unwrap(), 0);
        // Comments survive.
        assert!(std::fs::read_to_string(&path)
            .unwrap()
            .starts_with("# comment"));
    }

    #[test]
    fn remove_by_line() {
        let (_d, path) = setup();
        assert!(remove_line(&path, 2).unwrap());
        assert_eq!(list(&path).unwrap().len(), 2);
        assert!(!remove_line(&path, 999).unwrap());
    }
}
