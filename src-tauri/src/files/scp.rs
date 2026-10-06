//! SCP as a file backend, for servers whose SFTP subsystem is turned off.
//!
//! Everything goes over ordinary SSH exec channels on one connection:
//!
//! - Listing, `stat`, folders, renames, deletes and permissions run small
//!   shell commands (`ls`, `mkdir`, `mv`, `rm`, `chmod`). Every path reaches
//!   the shell single-quoted, so a name like `a b'$(x).txt` is only ever data.
//! - File contents use the SCP protocol itself (`scp -f` to read, `scp -t` to
//!   write), one file per channel. Names travel inside the protocol, not on a
//!   command line.
//!
//! SCP can't continue a partly copied file, so `resume` is off.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::pipe::{PipeReader, PipeWriter};
use super::{Caps, FileBackend, FileError, Reader, Stat, Writer};
use crate::containers::transport::shell_quote;
use crate::sftp::FileEntry;
use crate::ssh::{open_client, Client, LearnedKey, Target};

/// One `ls -l` line, understood.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LsEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub mode: u32,
    pub modified: Option<u64>,
}

// -- parsing `ls -lAn` ----------------------------------------------------------------------

/// The next whitespace-separated word, and what follows its single separator.
fn word(s: &str) -> Option<(&str, &str)> {
    let s = s.trim_start_matches([' ', '\t']);
    if s.is_empty() {
        return None;
    }
    let end = s.find([' ', '\t']).unwrap_or(s.len());
    Some((&s[..end], s.get(end + 1..).unwrap_or("")))
}

fn mode_bits(perms: &str) -> Option<u32> {
    let b = perms.as_bytes();
    if b.len() < 10 {
        return None;
    }
    let mut mode = 0u32;
    for (i, shift) in (0..9).zip((0..9).rev()) {
        let on = |c: u8| c != b'-';
        let c = b[1 + i];
        let bit = match (i % 3, c) {
            (0, c) => on(c) && c == b'r',
            (1, c) => on(c) && c == b'w',
            // The execute slot also shows setuid, setgid and sticky: s S t T.
            (_, b'x' | b's' | b't') => true,
            _ => false,
        };
        if bit {
            mode |= 1 << shift;
        }
    }
    if matches!(b[3], b's' | b'S') {
        mode |= 0o4000;
    }
    if matches!(b[6], b's' | b'S') {
        mode |= 0o2000;
    }
    if matches!(b[9], b't' | b'T') {
        mode |= 0o1000;
    }
    Some(mode)
}

fn month(name: &str) -> Option<u32> {
    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"].iter().position(|m| m.eq_ignore_ascii_case(name)).map(|i| i as u32 + 1)
}

/// `Oct  6 04:28` (this year, or last if that would be in the future) or `Oct  6  2025`, as Unix seconds.
fn parse_mtime(mon: &str, day: &str, time_or_year: &str, now: u64) -> Option<u64> {
    let (m, d) = (month(mon)?, day.parse::<u32>().ok().filter(|d| (1..=31).contains(d))?);
    let now_year = 1970 + (now / 86_400) as i64 * 400 / 146_097; // close enough to pick the year
    let (year, h, min) = if let Some((hh, mm)) = time_or_year.split_once(':') {
        let (h, mi) = (hh.parse::<i64>().ok()?, mm.parse::<i64>().ok()?);
        let this = super::days_from_civil(now_year, m, d) * 86_400 + h * 3600 + mi * 60;
        (if this as u64 > now + 86_400 { now_year - 1 } else { now_year }, h, mi)
    } else {
        (time_or_year.parse::<i64>().ok()?, 0, 0)
    };
    u64::try_from(super::days_from_civil(year, m, d) * 86_400 + h * 3600 + min * 60).ok()
}

/// Read the output of `LC_ALL=C ls -lAn`. Lines that aren't an entry (`total`, devices) are skipped.
pub fn parse_ls(output: &str, now: u64) -> Vec<LsEntry> {
    let mut out = Vec::new();
    for line in output.lines() {
        let Some((perms, rest)) = word(line) else { continue };
        if perms == "total" || perms.len() < 10 || !matches!(perms.as_bytes()[0], b'-' | b'd' | b'l') {
            continue;
        }
        let Some(mode) = mode_bits(perms) else { continue };
        // links, owner, group, size, month, day, time-or-year, then the name.
        let mut rest = rest;
        let mut f = [""; 7];
        let mut ok = true;
        for slot in &mut f {
            match word(rest) {
                Some((w, r)) => (*slot, rest) = (w, r),
                None => ok = false,
            }
        }
        let Ok(size) = f[3].parse::<u64>() else { continue };
        if !ok || rest.is_empty() {
            continue;
        }
        let is_symlink = perms.starts_with('l');
        // A link shows as `name -> target`; the name is what comes before the arrow.
        let name = if is_symlink { rest.split(" -> ").next().unwrap_or(rest) } else { rest };
        if name == "." || name == ".." {
            continue;
        }
        out.push(LsEntry { name: name.to_string(), is_dir: perms.starts_with('d'), is_symlink, size, mode, modified: parse_mtime(f[4], f[5], f[6], now) });
    }
    out
}

/// What the `stat` script prints: `d`, `f <size>` or `none`.
pub fn parse_stat(output: &str) -> Option<Option<Stat>> {
    let t = output.trim();
    if t == "none" {
        return Some(None);
    }
    if t == "d" || t == "d 0" {
        return Some(Some(Stat { is_dir: true, size: 0 }));
    }
    let size = t.strip_prefix("f ")?.trim().parse().ok()?;
    Some(Some(Stat { is_dir: false, size }))
}

// -- the SCP protocol --------------------------------------------------------------------------

/// A protocol line from the server: `C0644 12 name`, a warning (`\x01msg`) or an error (`\x02msg`).
#[derive(Debug, PartialEq, Eq)]
pub enum Header {
    File { mode: u32, size: u64, name: String },
    Failure(String),
}

pub fn parse_header(line: &[u8]) -> Result<Header, FileError> {
    let bad = |m: &str| FileError::Backend(format!("the server sent something SCP can't read: {m}"));
    let text = String::from_utf8_lossy(line);
    let text = text.trim_end_matches('\n');
    match line.first() {
        Some(1) | Some(2) => Ok(Header::Failure(text[1..].to_string())),
        Some(b'C') => {
            let mut parts = text[1..].splitn(3, ' ');
            let mode = u32::from_str_radix(parts.next().unwrap_or(""), 8).map_err(|_| bad("the file mode"))?;
            let size = parts.next().unwrap_or("").parse().map_err(|_| bad("the file size"))?;
            let name = parts.next().ok_or_else(|| bad("the file name"))?.to_string();
            Ok(Header::File { mode, size, name })
        }
        Some(b'D') | Some(b'E') | Some(b'T') => Err(bad("a folder or time record (only single files are expected)")),
        _ => Err(bad("an unknown record")),
    }
}

/// The line that announces a file to `scp -t`. Names can't hold a line break or a slash.
pub fn file_header(name: &str, size: u64) -> Result<String, FileError> {
    if name.is_empty() || name.contains('\n') || name.contains('/') {
        return Err(FileError::InvalidPath(format!("{name:?} can't be sent over SCP (line breaks and slashes aren't allowed in a name)")));
    }
    Ok(format!("C0644 {size} {name}\n"))
}

async fn read_byte<S: AsyncRead + Unpin>(s: &mut S) -> Result<u8, FileError> {
    let mut b = [0u8; 1];
    s.read_exact(&mut b).await.map_err(|e| FileError::Backend(format!("the server closed the SCP connection: {e}")))?;
    Ok(b[0])
}

async fn read_line<S: AsyncRead + Unpin>(s: &mut S) -> Result<Vec<u8>, FileError> {
    let mut line = Vec::new();
    loop {
        let b = read_byte(s).await?;
        line.push(b);
        if b == b'\n' || line.len() > 4096 {
            return Ok(line);
        }
    }
}

/// An acknowledgement: 0 is yes; 1 or 2 is followed by the server's words.
async fn read_ack<S: AsyncRead + Unpin>(s: &mut S) -> Result<(), FileError> {
    match read_byte(s).await? {
        0 => Ok(()),
        _ => Err(FileError::Backend(String::from_utf8_lossy(&read_line(s).await?).trim().to_string())),
    }
}

// -- the connection ------------------------------------------------------------------------------------

pub struct ScpConn {
    client: Arc<Client>,
}

/// A script for the remote shell, run through `sh -c` so any login shell reads it the same way.
fn sh(script: &str) -> String {
    format!("sh -c {}", shell_quote(script))
}

fn chan_err(e: russh::Error) -> FileError {
    FileError::Channel(e)
}

impl ScpConn {
    pub async fn open(target: &Target) -> Result<(Self, Vec<LearnedKey>), FileError> {
        let (client, learned) = open_client(target, None).await?;
        Ok((Self { client: Arc::new(client) }, learned))
    }

    /// Run a command and collect what it says.
    async fn run(&self, command: &str) -> Result<(String, String, Option<u32>), FileError> {
        let mut ch = self.client.channel_open_session().await.map_err(chan_err)?;
        ch.exec(true, command.as_bytes()).await.map_err(chan_err)?;
        let (mut out, mut err, mut code) = (Vec::new(), Vec::new(), None);
        while let Some(msg) = ch.wait().await {
            match msg {
                russh::ChannelMsg::Data { data } => out.extend_from_slice(&data),
                russh::ChannelMsg::ExtendedData { data, ext: 1 } => err.extend_from_slice(&data),
                russh::ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                russh::ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok((String::from_utf8_lossy(&out).into_owned(), String::from_utf8_lossy(&err).into_owned(), code))
    }

    /// Run a script that must succeed; its words are the error otherwise.
    async fn check(&self, script: &str) -> Result<String, FileError> {
        let (out, err, code) = self.run(&sh(script)).await?;
        if code == Some(0) {
            Ok(out)
        } else {
            let said = err.trim();
            Err(FileError::Backend(if said.is_empty() { format!("the command failed (exit {})", code.map_or("?".into(), |c| c.to_string())) } else { said.to_string() }))
        }
    }

    async fn open_stream(&self, command: &str) -> Result<russh::ChannelStream<russh::client::Msg>, FileError> {
        let ch = self.client.channel_open_session().await.map_err(chan_err)?;
        ch.exec(true, command.as_bytes()).await.map_err(chan_err)?;
        Ok(ch.into_stream())
    }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[async_trait]
impl FileBackend for ScpConn {
    fn caps(&self) -> Caps {
        // No continuing a partial file, and the editor save-back is SFTP's.
        Caps { chmod: true, resume: false, edit: false, ..Caps::files() }
    }

    async fn home(&self) -> Result<String, FileError> {
        Ok(self.check("pwd").await?.trim().to_string())
    }

    async fn list(&self, dir: &str) -> Result<Vec<FileEntry>, FileError> {
        let out = self.check(&format!("LC_ALL=C; export LC_ALL; ls -lAn -- {}", shell_quote(dir))).await?;
        let parsed = parse_ls(&out, now());
        // A link shows as a link: ask the system whether what it points at is a folder.
        let links: Vec<&LsEntry> = parsed.iter().filter(|e| e.is_symlink).collect();
        let mut link_is_dir = std::collections::HashMap::new();
        if !links.is_empty() {
            let paths: Vec<String> = links.iter().map(|e| shell_quote(&self.join(dir, &e.name))).collect();
            let script = format!("for p in {}; do if [ -d \"$p\" ]; then echo d; else echo f; fi; done", paths.join(" "));
            let answers = self.check(&script).await.unwrap_or_default();
            for (e, a) in links.iter().zip(answers.lines()) {
                link_is_dir.insert(e.name.clone(), a == "d");
            }
        }
        let mut entries: Vec<FileEntry> = parsed
            .into_iter()
            .map(|e| {
                let is_dir = if e.is_symlink { link_is_dir.get(&e.name).copied().unwrap_or(false) } else { e.is_dir };
                FileEntry {
                    path: self.join(dir, &e.name),
                    name: e.name,
                    is_dir,
                    is_symlink: e.is_symlink,
                    size: if is_dir { 0 } else { e.size },
                    modified: e.modified,
                    permissions: Some(e.mode),
                }
            })
            .collect();
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(entries)
    }

    async fn stat(&self, path: &str) -> Result<Option<Stat>, FileError> {
        let p = shell_quote(path);
        let script = format!("p={p}; if [ -d \"$p\" ]; then echo d; elif [ -e \"$p\" ]; then printf 'f '; wc -c < \"$p\" | tr -d ' '; else echo none; fi");
        let out = self.check(&script).await?;
        parse_stat(&out).ok_or_else(|| FileError::Backend(format!("couldn't tell what {path} is: {}", out.trim())))
    }

    async fn mkdir(&self, dir: &str, name: &str) -> Result<(), FileError> {
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(FileError::InvalidPath(name.to_string()));
        }
        self.check(&format!("mkdir -- {}", shell_quote(&self.join(dir, name)))).await.map(drop)
    }

    async fn rename(&self, from: &str, new_name: &str) -> Result<(), FileError> {
        if new_name.is_empty() || new_name == "." || new_name == ".." || new_name.contains('/') {
            return Err(FileError::InvalidPath(new_name.to_string()));
        }
        let parent = self.parent(from).ok_or_else(|| FileError::InvalidPath(from.to_string()))?;
        self.check(&format!("mv -- {} {}", shell_quote(from), shell_quote(&self.join(&parent, new_name)))).await.map(drop)
    }

    async fn remove(&self, path: &str) -> Result<(), FileError> {
        // Never the top of the file system, or an empty path that some shells read as "here".
        if path.trim_matches('/').is_empty() || path == "." || path == ".." {
            return Err(FileError::InvalidPath(path.to_string()));
        }
        self.check(&format!("rm -rf -- {}", shell_quote(path))).await.map(drop)
    }

    async fn chmod(&self, path: &str, mode: u32) -> Result<(), FileError> {
        self.check(&format!("chmod {:o} -- {}", mode & 0o7777, shell_quote(path))).await.map(drop)
    }

    async fn read(&self, path: &str, offset: u64) -> Result<Reader, FileError> {
        if offset > 0 {
            return Err(FileError::Unsupported("continuing a partial download"));
        }
        let mut s = self.open_stream(&format!("scp -f -- {}", shell_quote(path))).await?;
        s.write_all(&[0]).await.map_err(|e| FileError::Backend(e.to_string()))?;
        let size = match parse_header(&read_line(&mut s).await?)? {
            Header::File { size, .. } => size,
            Header::Failure(m) => return Err(FileError::Backend(m)),
        };
        s.write_all(&[0]).await.map_err(|e| FileError::Backend(e.to_string()))?;

        let (tx, rx) = tokio::io::duplex(256 * 1024);
        let shared = Arc::new(Mutex::new(None));
        let failed = Arc::clone(&shared);
        tokio::spawn(async move {
            let result: Result<(), FileError> = async {
                let mut tx = tx;
                let copied = tokio::io::copy(&mut (&mut s).take(size), &mut tx).await.map_err(|e| FileError::Backend(e.to_string()))?;
                if copied < size {
                    return Err(FileError::Backend("the connection closed before the whole file arrived".into()));
                }
                read_ack(&mut s).await?; // the server's "all sent"
                let _ = s.write_all(&[0]).await;
                tx.shutdown().await.map_err(|e| FileError::Backend(e.to_string()))
            }
            .await;
            if let Err(e) = result {
                *failed.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.to_string());
            }
        });
        Ok(Box::new(PipeReader::new(rx, Some(size), shared)))
    }

    async fn write(&self, path: &str, offset: u64, size: u64) -> Result<Writer, FileError> {
        if offset > 0 {
            return Err(FileError::Unsupported("continuing a partial upload"));
        }
        let parent = self.parent(path).ok_or_else(|| FileError::InvalidPath(path.to_string()))?;
        let name = self.basename(path)?;
        let header = file_header(&name, size)?;
        let mut s = self.open_stream(&format!("scp -t -- {}", shell_quote(&parent))).await?;
        read_ack(&mut s).await?;
        s.write_all(header.as_bytes()).await.map_err(|e| FileError::Backend(e.to_string()))?;
        read_ack(&mut s).await?;

        let (tx, rx) = tokio::io::duplex(256 * 1024);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let result: Result<(), FileError> = async {
                let mut rx = rx;
                let sent = tokio::io::copy(&mut rx, &mut s).await.map_err(|e| FileError::Backend(e.to_string()))?;
                if sent != size {
                    return Err(FileError::Backend(format!("the file changed while it was being copied ({sent} of {size} bytes)")));
                }
                s.write_all(&[0]).await.map_err(|e| FileError::Backend(e.to_string()))?;
                read_ack(&mut s).await
            }
            .await;
            let _ = done_tx.send(result.map_err(|e| e.to_string()));
        });
        Ok(Box::new(PipeWriter::new(tx, size, done_rx)))
    }

    async fn close(&self) {
        self.client.close().await;
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_791_200_000; // 2026-10-05

    #[test]
    fn gnu_listings_are_read() {
        let out = "total 12\ndrwxr-xr-x 2 1000 1000 4096 Oct  6 04:28 docs\n-rw-r--r-- 1 1000 1000    5 Jan  3  2024 a b.txt\n-rwxr-sr-x 1 0 0 12 Oct  5 10:00 run\nlrwxrwxrwx 1 1000 1000    7 Oct  6 04:28 link -> docs/x\n-rw-r--r--. 1 1000 1000 0 Oct  6 04:28 selinux.txt\n";
        let e = parse_ls(out, NOW);
        let names: Vec<&str> = e.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["docs", "a b.txt", "run", "link", "selinux.txt"]);
        assert!(e[0].is_dir && e[0].mode == 0o755 && e[0].size == 4096);
        assert_eq!((e[1].size, e[1].mode, e[1].is_dir), (5, 0o644, false));
        assert_eq!(e[2].mode, 0o2755, "setgid shows");
        assert!(e[3].is_symlink && e[3].name == "link", "the arrow and target aren't part of the name");
        assert_eq!(e[4].mode, 0o644, "an SELinux dot after the mode is ignored");
    }

    #[test]
    fn names_with_odd_characters_keep_them() {
        let out = "-rw-r--r-- 1 1 1 3 Oct  6 04:28 it's $(x)  two  spaces\n-rw-r--r-- 1 1 1 3 Oct  6 04:28  leading space\n-rw-r--r-- 1 1 1 3 Oct  6 04:28 a -> b.txt\n-rw-r--r-- 1 1 1 3 Oct  6 04:28 tab\there\n";
        let e = parse_ls(out, NOW);
        assert_eq!(e[0].name, "it's $(x)  two  spaces");
        assert_eq!(e[1].name, " leading space");
        assert_eq!(e[2].name, "a -> b.txt", "an arrow in a regular file's name is part of it");
        assert_eq!(e[3].name, "tab\there");
    }

    #[test]
    fn bsd_listings_and_noise_are_read_or_skipped() {
        let out = "total 8\ndrwxr-xr-x  2 501  20  64 Oct  6 04:28 sub\n-rw-r--r--  1 501  20  12 Oct  6  2025 f.txt\ncrw-rw-rw-  1 0  0  1, 3 Oct  6 04:28 null\nls: ./denied: Permission denied\n\n";
        let e = parse_ls(out, NOW);
        assert_eq!(e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["sub", "f.txt"], "devices and error lines are skipped");
        assert!(parse_ls("", NOW).is_empty());
        assert!(parse_ls("total 0\n", NOW).is_empty());
    }

    #[test]
    fn times_are_this_year_unless_that_is_the_future() {
        let e = parse_ls("-rw-r--r-- 1 1 1 1 Oct  6 04:28 a\n-rw-r--r-- 1 1 1 1 Dec 25 10:00 b\n-rw-r--r-- 1 1 1 1 Jan  3  2024 c\n", NOW);
        let (a, b, c) = (e[0].modified.unwrap(), e[1].modified.unwrap(), e[2].modified.unwrap());
        assert_eq!(c, 1_704_240_000, "Jan 3 2024 00:00 UTC");
        assert!(a <= NOW + 86_400 && a > NOW - 86_400 * 2, "Oct 6 this year");
        assert!(b < NOW, "Dec 25 hasn't happened yet this year, so it is last year's");
    }

    #[test]
    fn the_stat_script_answers_are_read() {
        assert_eq!(parse_stat("none\n"), Some(None));
        assert_eq!(parse_stat("d\n"), Some(Some(Stat { is_dir: true, size: 0 })));
        assert_eq!(parse_stat("f 1234\n"), Some(Some(Stat { is_dir: false, size: 1234 })));
        assert_eq!(parse_stat("f 0"), Some(Some(Stat { is_dir: false, size: 0 })));
        assert_eq!(parse_stat("garbage"), None);
        assert_eq!(parse_stat("f x"), None);
    }

    #[test]
    fn protocol_headers() {
        assert_eq!(parse_header(b"C0644 12 hello.txt\n").unwrap(), Header::File { mode: 0o644, size: 12, name: "hello.txt".into() });
        assert_eq!(parse_header(b"C0755 0 a b'c.txt\n").unwrap(), Header::File { mode: 0o755, size: 0, name: "a b'c.txt".into() });
        assert_eq!(parse_header(b"\x01scp: warning\n").unwrap(), Header::Failure("scp: warning".into()));
        assert_eq!(parse_header(b"\x02scp: /x: No such file or directory\n").unwrap(), Header::Failure("scp: /x: No such file or directory".into()));
        for bad in [&b"C0644\n"[..], b"Cxyz 1 n\n", b"C0644 big n\n", b"D0755 0 dir\n", b"T1 0 1 0\n", b"Zjunk\n", b""] {
            assert!(parse_header(bad).is_err(), "{:?}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn announcing_a_file_refuses_names_that_could_break_the_protocol() {
        assert_eq!(file_header("a b'$(x).txt", 5).unwrap(), "C0644 5 a b'$(x).txt\n");
        assert_eq!(file_header("héllo 東京.txt", 1).unwrap(), "C0644 1 héllo 東京.txt\n");
        for bad in ["", "a\nb", "dir/file", "../x"] {
            assert!(file_header(bad, 1).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn every_script_reaches_the_shell_as_one_quoted_word() {
        let nasty = "a b'$(touch PWNED).txt";
        let q = shell_quote(nasty);
        assert!(q.starts_with('\'') && q.ends_with('\''), "{q}");
        // Single quotes inside are closed, escaped and reopened, never left bare.
        assert_eq!(q, "'a b'\\''$(touch PWNED).txt'");
        let wrapped = sh(&format!("mkdir -- {q}"));
        assert!(wrapped.starts_with("sh -c '"), "{wrapped}");
    }

    // -- against a real sshd with no SFTP at all --------------------------------------------------------

    #[cfg(unix)]
    mod live {
        use super::*;
        use crate::files::engine::{self, Conflict, TransferProgress, TransferRegistry};
        use crate::files::local::LocalBackend;
        use crate::files::ProgressSink;
        use crate::ssh::testutil::{spawn_sshd_without_sftp, target};

        #[derive(Default)]
        struct Collect(Mutex<Vec<TransferProgress>>);
        impl ProgressSink for Arc<Collect> {
            fn report(&self, p: TransferProgress) {
                self.0.lock().unwrap().push(p);
            }
        }

        async fn setup(dir: &std::path::Path) -> Option<(crate::ssh::testutil::Sshd, ScpConn)> {
            let sshd = spawn_sshd_without_sftp(dir)?;
            if std::process::Command::new("scp").arg("-h").output().is_err() {
                eprintln!("skipping: no scp binary on this machine");
                return None;
            }
            let (conn, _) = ScpConn::open(&target(&sshd, &sshd.client_key, dir.join("kh"))).await.ok()?;
            Some((sshd, conn))
        }

        async fn copy(src: &dyn FileBackend, dst: &dyn FileBackend, sources: &[String], dest: &str) -> TransferProgress {
            let sink = Arc::new(Collect::default());
            engine::transfer(&TransferRegistry::default(), "t".into(), src, dst, sources, dest, false, Conflict::Overwrite, &sink).await;
            let last = sink.0.lock().unwrap().last().cloned().unwrap();
            last
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn the_server_really_has_no_sftp_and_scp_still_browses_and_edits_the_tree() {
            let dir = tempfile::TempDir::new().unwrap();
            let Some((sshd, conn)) = setup(dir.path()).await else {
                eprintln!("skipping: no usable sshd on this machine");
                return;
            };
            // Proof that SFTP is off: the normal opener is refused.
            let refused = crate::sftp::SftpConn::open(&target(&sshd, &sshd.client_key, dir.path().join("kh2"))).await;
            match refused {
                Err(e) => assert!(matches!(e, FileError::NoSubsystem), "SFTP being off is reported as exactly that: {e}"),
                Ok(_) => panic!("the test server must not offer SFTP"),
            }

            let root = dir.path().join("work");
            std::fs::create_dir(&root).unwrap();
            let root = root.to_string_lossy().into_owned();
            assert!(conn.home().await.unwrap().starts_with('/'));

            conn.mkdir(&root, "made").await.unwrap();
            std::fs::write(format!("{root}/b.txt"), b"hello").unwrap();
            std::os::unix::fs::symlink(format!("{root}/made"), format!("{root}/linkdir")).unwrap();
            std::os::unix::fs::symlink(format!("{root}/b.txt"), format!("{root}/linkfile")).unwrap();
            let list = conn.list(&root).await.unwrap();
            let names: Vec<(&str, bool, bool)> = list.iter().map(|e| (e.name.as_str(), e.is_dir, e.is_symlink)).collect();
            assert_eq!(names, [("linkdir", true, true), ("made", true, false), ("b.txt", false, false), ("linkfile", false, true)], "folders first, links resolved: {names:?}");
            assert_eq!(list.iter().find(|e| e.name == "b.txt").unwrap().size, 5);
            use std::os::unix::fs::PermissionsExt;
            let on_disk = std::fs::metadata(format!("{root}/b.txt")).unwrap().permissions().mode() & 0o7777;
            assert_eq!(list.iter().find(|e| e.name == "b.txt").unwrap().permissions, Some(on_disk), "the mode shown is the real one");

            assert_eq!(conn.stat(&format!("{root}/b.txt")).await.unwrap(), Some(Stat { is_dir: false, size: 5 }));
            assert_eq!(conn.stat(&format!("{root}/made")).await.unwrap(), Some(Stat { is_dir: true, size: 0 }));
            assert_eq!(conn.stat(&format!("{root}/nope")).await.unwrap(), None);

            conn.rename(&format!("{root}/b.txt"), "c.txt").await.unwrap();
            assert!(std::path::Path::new(&format!("{root}/c.txt")).exists());
            conn.chmod(&format!("{root}/c.txt"), 0o600).await.unwrap();
            assert_eq!(std::fs::metadata(format!("{root}/c.txt")).unwrap().permissions().mode() & 0o777, 0o600);
            std::fs::create_dir_all(format!("{root}/gone/inner")).unwrap();
            std::fs::write(format!("{root}/gone/inner/x"), b"1").unwrap();
            conn.remove(&format!("{root}/gone")).await.unwrap();
            assert!(!std::path::Path::new(&format!("{root}/gone")).exists());
            // Things it must refuse.
            assert!(conn.remove("/").await.is_err());
            assert!(conn.mkdir(&root, "a/b").await.is_err());
            assert!(conn.rename(&format!("{root}/c.txt"), "../escape").await.is_err());
            let err = conn.list(&format!("{root}/nowhere")).await.unwrap_err().to_string();
            assert!(err.contains("nowhere"), "{err}");
            conn.close().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_file_and_a_folder_go_up_and_come_down_over_scp() {
            let dir = tempfile::TempDir::new().unwrap();
            let Some((_sshd, conn)) = setup(dir.path()).await else {
                eprintln!("skipping: no usable sshd on this machine");
                return;
            };
            let local = LocalBackend;
            let up = dir.path().join("up");
            std::fs::create_dir_all(up.join("nested/deeper")).unwrap();
            std::fs::write(up.join("a.txt"), b"alpha").unwrap();
            let big: Vec<u8> = (0..(256 * 1024 * 3 + 17)).map(|i| (i % 251) as u8).collect();
            std::fs::write(up.join("nested/big.bin"), &big).unwrap();
            std::fs::write(up.join("nested/deeper/empty.dat"), b"").unwrap();
            let remote_root = dir.path().join("remote");
            std::fs::create_dir(&remote_root).unwrap();
            let rroot = remote_root.to_string_lossy().into_owned();

            // Up: one file, then the whole folder.
            let one = copy(&local, &conn, &[up.join("a.txt").to_string_lossy().into_owned()], &rroot).await;
            assert!(matches!(one, TransferProgress::Done { files: 1, bytes: 5 }), "{one:?}");
            let all = copy(&local, &conn, &[up.to_string_lossy().into_owned()], &rroot).await;
            assert!(matches!(all, TransferProgress::Done { files: 3, .. }), "{all:?}");
            assert_eq!(std::fs::read(remote_root.join("a.txt")).unwrap(), b"alpha");
            assert_eq!(std::fs::read(remote_root.join("up/nested/big.bin")).unwrap(), big);
            assert!(remote_root.join("up/nested/deeper/empty.dat").is_file());
            assert_eq!(std::fs::metadata(remote_root.join("up/nested/deeper/empty.dat")).unwrap().len(), 0);

            // Down: the folder to a fresh place.
            let down = dir.path().join("down");
            std::fs::create_dir(&down).unwrap();
            let got = copy(&conn, &local, &[format!("{rroot}/up")], &down.to_string_lossy()).await;
            assert!(matches!(got, TransferProgress::Done { files: 3, .. }), "{got:?}");
            assert_eq!(std::fs::read(down.join("up/nested/big.bin")).unwrap(), big);
            assert_eq!(std::fs::read(down.join("up/a.txt")).unwrap(), b"alpha");
            // And a single file down.
            let one_down = dir.path().join("one");
            std::fs::create_dir(&one_down).unwrap();
            let r = copy(&conn, &local, &[format!("{rroot}/a.txt")], &one_down.to_string_lossy()).await;
            assert!(matches!(r, TransferProgress::Done { files: 1, bytes: 5 }), "{r:?}");

            // Reading a file that isn't there is an error, not a hang or an empty file.
            let missing = conn.read(&format!("{rroot}/nope.txt"), 0).await;
            assert!(missing.is_err());
            // A file that vanished mid-plan or can't be written into a folder that isn't there fails cleanly.
            let bad = copy(&local, &conn, &[up.join("a.txt").to_string_lossy().into_owned()], &format!("{rroot}/no/such/dir")).await;
            assert!(matches!(bad, TransferProgress::Failed { .. }), "{bad:?}");
            conn.close().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_hostile_file_name_is_only_ever_a_name() {
            let dir = tempfile::TempDir::new().unwrap();
            let Some((_sshd, conn)) = setup(dir.path()).await else {
                eprintln!("skipping: no usable sshd on this machine");
                return;
            };
            let local = LocalBackend;
            let remote_root = dir.path().join("remote");
            std::fs::create_dir(&remote_root).unwrap();
            let rroot = remote_root.to_string_lossy().into_owned();
            // A command that ran would run in the server user's home folder (here, the same as ours).
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            let canary = format!("SSHVAULT_CANARY_{}", std::process::id());
            let canary_path = std::path::Path::new(&home).join(&canary);
            let _ = std::fs::remove_file(&canary_path);

            // Names that would run something if any quoting were wrong.
            let names = ["a b'$(x).txt".to_string(), format!("q'; touch {canary}; echo '.txt"), format!("$(touch {canary}).txt"), format!("`touch {canary}`.txt"), "back\\slash \"dq\".txt".to_string(), "héllo 東京 😀.txt".to_string(), "-rf".to_string()];
            let src = dir.path().join("src");
            std::fs::create_dir(&src).unwrap();
            for n in &names {
                std::fs::write(src.join(n), format!("data of {n}")).unwrap();
            }
            // Upload each by itself, so a name that broke the protocol would show.
            for n in &names {
                let r = copy(&local, &conn, &[src.join(n).to_string_lossy().into_owned()], &rroot).await;
                assert!(matches!(r, TransferProgress::Done { files: 1, .. }), "{n:?}: {r:?}");
                assert_eq!(std::fs::read_to_string(remote_root.join(n)).unwrap(), format!("data of {n}"), "{n:?}");
            }
            // Listing shows them exactly.
            let listed: Vec<String> = conn.list(&rroot).await.unwrap().into_iter().map(|e| e.name).collect();
            for n in &names {
                assert!(listed.contains(n), "{n:?} missing from {listed:?}");
            }
            // Download them back through `scp -f`.
            let back = dir.path().join("back");
            std::fs::create_dir(&back).unwrap();
            for n in &names {
                let r = copy(&conn, &local, &[format!("{rroot}/{n}")], &back.to_string_lossy()).await;
                assert!(matches!(r, TransferProgress::Done { files: 1, .. }), "{n:?}: {r:?}");
                assert_eq!(std::fs::read_to_string(back.join(n)).unwrap(), format!("data of {n}"));
            }
            // The other operations quote too: rename to, and remove, a hostile name.
            conn.rename(&format!("{rroot}/a b'$(x).txt"), &format!("$(touch {canary}).renamed")).await.unwrap();
            conn.remove(&format!("{rroot}/{}", names[1])).await.unwrap();
            conn.mkdir(&rroot, &format!("dir'; touch {canary}; '")).await.unwrap();
            assert!(!canary_path.exists(), "a name ran a command on the server");
            conn.close().await;
        }
    }
}
