//! Copying a folder tree from one host straight to another with `tar` over two SSH channels, for trees of many small
//! files, where the one-file-at-a-time SFTP copy spends nearly all its time waiting on round trips.
//!
//! The source host's `tar` writes the archive, this program passes it on to the destination host's `tar`, and
//! nothing is written on this computer. That makes the source host's archive something the destination will unpack,
//! so every byte of it goes through a [`TarGuard`] first: only the regular files and folders this program planned,
//! under exactly the names planned, are let through. A hostile or broken source cannot add a link, climb out of the
//! destination folder with `..` or an absolute name, or slip in a file nobody asked for: the copy stops instead.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use russh::ChannelMsg;

use super::engine::Progress;
use super::{FileBackend, FileError};
use crate::ssh::Client;

/// Fewer files than this and per-file copying is no slower to speak of.
pub const MIN_FILES: usize = 50;
/// Bigger files than this on average are limited by bandwidth, not round trips.
pub const MAX_AVERAGE_BYTES: u64 = 1 << 20;
const BLOCK: usize = 512;
/// The most a long name or an extended header may hold.
const MAX_META: usize = 1 << 20;

/// One thing the archive must contain: a path relative to the folder copied into, and whether it is a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub dir: bool,
}

enum State {
    Header,
    /// Data blocks of a file, still to go by (padded to whole blocks).
    Skip(u64),
    /// A long name or extended header: its data is read into `data` before it applies to the next entry.
    Meta { kind: u8, size: usize, data: Vec<u8> },
    /// Past the two empty blocks that end an archive.
    End,
}

/// Checks a tar stream as it goes by. See the module notes.
pub struct TarGuard {
    expected: HashMap<String, bool>,
    seen: HashSet<String>,
    buf: Vec<u8>,
    state: State,
    zero_blocks: u32,
    /// Name and size from a long-name or extended header, for the entry that follows it.
    next_name: Option<String>,
    next_size: Option<u64>,
}

impl TarGuard {
    pub fn new(entries: &[Entry]) -> Self {
        Self {
            expected: entries.iter().map(|e| (e.path.clone(), e.dir)).collect(),
            seen: HashSet::new(),
            buf: Vec::new(),
            state: State::Header,
            zero_blocks: 0,
            next_name: None,
            next_size: None,
        }
    }

    /// Take the next bytes of the archive. An error means the stream must not go on to the destination.
    pub fn feed(&mut self, chunk: &[u8]) -> Result<(), String> {
        self.buf.extend_from_slice(chunk);
        loop {
            match &mut self.state {
                State::End => {
                    if self.buf.iter().any(|b| *b != 0) {
                        return Err("there is data after the end of the archive".into());
                    }
                    self.buf.clear();
                    return Ok(());
                }
                State::Skip(left) => {
                    let n = (*left).min(self.buf.len() as u64) as usize;
                    self.buf.drain(..n);
                    *left -= n as u64;
                    if *left > 0 {
                        return Ok(());
                    }
                    self.state = State::Header;
                }
                State::Meta { size, data, .. } => {
                    let padded = size.div_ceil(BLOCK) * BLOCK;
                    let need = padded - data.len();
                    let take = need.min(self.buf.len());
                    data.extend(self.buf.drain(..take));
                    if data.len() < padded {
                        return Ok(());
                    }
                    let State::Meta { kind, size, mut data } = std::mem::replace(&mut self.state, State::Header) else { unreachable!() };
                    data.truncate(size);
                    self.apply_meta(kind, &data)?;
                }
                State::Header => {
                    if self.buf.len() < BLOCK {
                        return Ok(());
                    }
                    let header: Vec<u8> = self.buf.drain(..BLOCK).collect();
                    self.header(&header)?;
                }
            }
        }
    }

    /// The archive is over: it must have ended properly and held everything that was planned.
    pub fn finish(&self) -> Result<(), String> {
        if !matches!(self.state, State::End) {
            return Err("the archive ended early".into());
        }
        if self.seen.len() != self.expected.len() {
            let missing = self.expected.keys().filter(|k| !self.seen.contains(*k)).take(3).cloned().collect::<Vec<_>>();
            return Err(format!("the archive is missing {} of the files it should hold, such as {}", self.expected.len() - self.seen.len(), missing.join(", ")));
        }
        Ok(())
    }

    fn apply_meta(&mut self, kind: u8, data: &[u8]) -> Result<(), String> {
        match kind {
            // GNU long name: the name, NUL-terminated.
            b'L' => {
                let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
                self.next_name = Some(String::from_utf8(data[..end].to_vec()).map_err(|_| "a name in the archive is not text".to_string())?);
            }
            // pax: `<length> <key>=<value>\n` records.
            b'x' => {
                let mut rest = data;
                while !rest.is_empty() {
                    let space = rest.iter().position(|b| *b == b' ').ok_or("a malformed extended header")?;
                    let len: usize = std::str::from_utf8(&rest[..space]).ok().and_then(|s| s.parse().ok()).ok_or("a malformed extended header")?;
                    if len <= space + 1 || len > rest.len() {
                        return Err("a malformed extended header".into());
                    }
                    let record = &rest[space + 1..len];
                    let record = record.strip_suffix(b"\n").unwrap_or(record);
                    if let Some(eq) = record.iter().position(|b| *b == b'=') {
                        let (key, value) = (&record[..eq], &record[eq + 1..]);
                        match key {
                            b"path" => self.next_name = Some(String::from_utf8(value.to_vec()).map_err(|_| "a name in the archive is not text".to_string())?),
                            b"size" => self.next_size = Some(std::str::from_utf8(value).ok().and_then(|s| s.parse().ok()).ok_or("a malformed size")?),
                            _ => {}
                        }
                    }
                    rest = &rest[len..];
                }
            }
            // GNU long link name, and pax global headers: nothing here depends on them.
            _ => {}
        }
        Ok(())
    }

    fn header(&mut self, h: &[u8]) -> Result<(), String> {
        if h.iter().all(|b| *b == 0) {
            self.zero_blocks += 1;
            if self.zero_blocks >= 2 {
                self.state = State::End;
            }
            return Ok(());
        }
        self.zero_blocks = 0;
        // The checksum is the sum of every header byte, with its own field counted as spaces.
        let sum: u32 = h.iter().enumerate().map(|(i, b)| if (148..156).contains(&i) { 32 } else { *b as u32 }).sum();
        let stored = parse_octal(&h[148..156]).ok_or("not a tar archive (bad checksum field)")?;
        if stored != sum as u64 {
            return Err("not a tar archive (the checksum is wrong)".into());
        }
        let kind = h[156];
        match kind {
            b'L' | b'x' | b'K' | b'g' => {
                let size = parse_size(&h[124..136]).ok_or("a malformed size in the archive")?;
                if size > MAX_META as u64 {
                    return Err("an extended header in the archive is far too big".into());
                }
                self.state = State::Meta { kind, size: size as usize, data: Vec::new() };
                return Ok(());
            }
            b'0' | 0 | b'7' | b'5' => {}
            b'1' | b'2' => return Err("the archive holds a link, which is not copied".into()),
            other => return Err(format!("the archive holds an entry of a kind that is not copied (type {})", other as char)),
        }
        let is_dir = kind == b'5';
        let name = match self.next_name.take() {
            Some(n) => n,
            None => {
                let name = cstr(&h[0..100])?;
                // ustar splits a long name into a prefix and a name.
                let ustar = h[257..262] == *b"ustar";
                let prefix = if ustar { cstr(&h[345..500])? } else { String::new() };
                if prefix.is_empty() { name } else { format!("{prefix}/{name}") }
            }
        };
        let size = match self.next_size.take() {
            Some(s) => s,
            None => parse_size(&h[124..136]).ok_or("a malformed size in the archive")?,
        };
        let path = clean(&name)?;
        match self.expected.get(&path) {
            None => return Err(format!("the archive holds {path}, which was not asked for")),
            Some(dir) if *dir != is_dir => return Err(format!("the archive holds {path} as the wrong kind of entry")),
            Some(_) => {}
        }
        if !self.seen.insert(path.clone()) {
            return Err(format!("the archive holds {path} twice"));
        }
        if is_dir {
            if size != 0 {
                return Err(format!("the folder {path} has data in the archive"));
            }
        } else if size > 0 {
            self.state = State::Skip(size.div_ceil(BLOCK as u64) * BLOCK as u64);
        }
        Ok(())
    }
}

/// A NUL-terminated text field.
fn cstr(field: &[u8]) -> Result<String, String> {
    let end = field.iter().position(|b| *b == 0).unwrap_or(field.len());
    String::from_utf8(field[..end].to_vec()).map_err(|_| "a name in the archive is not text".to_string())
}

fn parse_octal(field: &[u8]) -> Option<u64> {
    let s = std::str::from_utf8(field).ok()?;
    let s = s.trim_matches(|c: char| c == '\0' || c == ' ');
    if s.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(s, 8).ok()
}

/// Octal, or (for files over 8 GiB) the GNU big-endian binary form with the top bit set.
fn parse_size(field: &[u8]) -> Option<u64> {
    if field[0] & 0x80 != 0 {
        let mut v: u64 = (field[0] & 0x7f) as u64;
        for b in &field[1..] {
            v = v.checked_mul(256)?.checked_add(*b as u64)?;
        }
        return Some(v);
    }
    parse_octal(field)
}

/// A member name as a plain relative path: no leading `/`, no `..`, no empty parts. `./a/b/` becomes `a/b`.
fn clean(name: &str) -> Result<String, String> {
    let name = name.trim_end_matches('/');
    let mut parts = Vec::new();
    for (i, part) in name.split('/').enumerate() {
        match part {
            "" if i == 0 => return Err(format!("the archive holds an absolute name ({name})")),
            "." if i == 0 => continue,
            "" | "." => return Err(format!("the archive holds an odd name ({name})")),
            ".." => return Err(format!("the archive holds a name that climbs out of the folder ({name})")),
            p if p.contains('\0') => return Err("a name in the archive holds a NUL".into()),
            p => parts.push(p),
        }
    }
    if parts.is_empty() {
        return Err("the archive holds an empty name".into());
    }
    Ok(parts.join("/"))
}

// -- running it -------------------------------------------------------------------------------------------------------

/// Which `tar` is on a host: only GNU tar and bsdtar are used, since the options below are theirs.
async fn has_tar(client: &Client) -> bool {
    match client.exec("tar --version", None, Duration::from_secs(10), 4096).await {
        Ok(out) if out.code == Some(0) => {
            let v = String::from_utf8_lossy(&out.stdout).to_lowercase();
            v.contains("gnu tar") || v.contains("bsdtar")
        }
        _ => false,
    }
}

/// Whether both places are reached over SSH and both hosts have a `tar` this can use.
pub async fn available(src: &dyn FileBackend, dst: &dyn FileBackend) -> bool {
    match (src.ssh(), dst.ssh()) {
        (Some(a), Some(b)) => has_tar(a).await && has_tar(b).await,
        _ => false,
    }
}

fn q(s: &str) -> String {
    crate::containers::transport::shell_quote(s)
}

/// Copy `entries` (paths relative to `src_dir`) into `dst_dir` on the other host. Folders come first in the list, so
/// they exist (with their modes) before what goes in them. `progress` is told how many archive bytes went by, and
/// answers whether to go on (false: stopped by the person).
pub(super) async fn relay(
    src: &Client,
    dst: &Client,
    src_dir: &str,
    dst_dir: &str,
    entries: &[Entry],
    progress: &mut Progress<'_>,
) -> Result<(), FileError> {
    let bad = |why: String| FileError::Backend(format!("copying with tar failed: {why}"));
    // Names go to tar on its input, NUL-separated: no name can be mistaken for an option or split in two.
    let mut list = Vec::new();
    for e in entries {
        list.extend_from_slice(e.path.as_bytes());
        list.push(0);
    }
    let make = format!("sh -c {} sh {}", q("cd \"$1\" && exec tar -cf - --no-recursion --null -T -"), q(src_dir));
    let unpack = format!("sh -c {} sh {}", q("cd \"$1\" && exec tar -xpf - --no-same-owner"), q(dst_dir));
    let a = src.exec_channel(&make).await.map_err(|e| bad(e.to_string()))?;
    let b = dst.exec_channel(&unpack).await.map_err(|e| bad(e.to_string()))?;
    let (mut a_read, a_write) = a.split();
    let (mut b_read, b_write) = b.split();

    a_write.data_bytes(list).await.map_err(|e| bad(e.to_string()))?;
    a_write.eof().await.map_err(|e| bad(e.to_string()))?;

    // What the unpacking side says is read as it arrives, so it can never fill up and stall the pipe.
    let unpack_side = tokio::spawn(async move {
        let (mut code, mut said) = (None, Vec::new());
        while let Some(msg) = b_read.wait().await {
            match msg {
                ChannelMsg::ExtendedData { data, .. } | ChannelMsg::Data { data } => {
                    let room = 4096usize.saturating_sub(said.len());
                    said.extend_from_slice(&data[..data.len().min(room)]);
                }
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        (code, String::from_utf8_lossy(&said).trim().to_string())
    });

    let mut guard = TarGuard::new(entries);
    let (mut make_code, mut make_said) = (None, Vec::new());
    let outcome: Result<(), FileError> = async {
        loop {
            // A quiet source must not keep a cancel waiting.
            let msg = match tokio::time::timeout(Duration::from_millis(250), a_read.wait()).await {
                Ok(m) => m,
                Err(_) => {
                    progress.check("tar").await?;
                    continue;
                }
            };
            match msg {
                Some(ChannelMsg::Data { data }) => {
                    progress.check("tar").await?;
                    guard.feed(&data).map_err(|why| bad(format!("{why}; nothing more was copied")))?;
                    b_write.data_bytes(data.to_vec()).await.map_err(|e| bad(e.to_string()))?;
                    progress.add(data.len() as u64, "tar");
                }
                Some(ChannelMsg::ExtendedData { data, .. }) => {
                    let room = 4096usize.saturating_sub(make_said.len());
                    make_said.extend_from_slice(&data[..data.len().min(room)]);
                }
                Some(ChannelMsg::ExitStatus { exit_status }) => make_code = Some(exit_status),
                Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            }
        }
        if make_code != Some(0) {
            let said = String::from_utf8_lossy(&make_said).trim().to_string();
            return Err(bad(format!("tar on the source host stopped ({}){}", make_code.map_or("no exit status".to_string(), |c| format!("exit {c}")), if said.is_empty() { String::new() } else { format!(": {said}") })));
        }
        guard.finish().map_err(bad)?;
        Ok(())
    }
    .await;

    match outcome {
        Ok(()) => {
            b_write.eof().await.map_err(|e| bad(e.to_string()))?;
            let (code, said) = unpack_side.await.map_err(|e| bad(e.to_string()))?;
            if code != Some(0) {
                return Err(bad(format!("tar on the destination host stopped ({}){}", code.map_or("no exit status".to_string(), |c| format!("exit {c}")), if said.is_empty() { String::new() } else { format!(": {said}") })));
            }
            Ok(())
        }
        Err(e) => {
            // Stopped, or the stream was refused: both ends are closed at once, so neither tar goes on.
            let _ = a_write.close().await;
            let _ = b_write.close().await;
            unpack_side.abort();
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(list: &[(&str, bool)]) -> Vec<Entry> {
        list.iter().map(|(p, d)| Entry { path: p.to_string(), dir: *d }).collect()
    }

    /// Feed in pieces of every size, to be sure no boundary matters.
    fn run(entries: &[Entry], bytes: &[u8], piece: usize) -> Result<(), String> {
        let mut g = TarGuard::new(entries);
        for c in bytes.chunks(piece) {
            g.feed(c)?;
        }
        g.finish()
    }

    fn planned_archive(names: &[&str], setup: impl FnOnce(&std::path::Path)) -> (Vec<u8>, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        setup(dir.path());
        let out = std::process::Command::new("tar").args(["-cf", "-", "--no-recursion", "-C"]).arg(dir.path()).args(names).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        (out.stdout, dir)
    }

    #[test]
    fn what_was_planned_passes_however_the_stream_is_cut_up() {
        let (tar, _d) = planned_archive(&["a.txt", "sub", "sub/b.bin", "sub/deeper", "sub/deeper/c"], |p| {
            std::fs::create_dir_all(p.join("sub/deeper")).unwrap();
            std::fs::write(p.join("a.txt"), "hello").unwrap();
            std::fs::write(p.join("sub/b.bin"), "x".repeat(5000)).unwrap();
            std::fs::write(p.join("sub/deeper/c"), "").unwrap();
        });
        let want = entries(&[("a.txt", false), ("sub", true), ("sub/b.bin", false), ("sub/deeper", true), ("sub/deeper/c", false)]);
        for piece in [1, 7, 511, 512, 513, 4096, 1 << 20] {
            assert_eq!(run(&want, &tar, piece), Ok(()), "piece {piece}");
        }
    }

    #[test]
    fn long_names_are_read_in_gnu_and_pax_form() {
        // Over 100 characters either way. On Windows the whole path must stay under 260, which the system's tar needs.
        let (dl, fl) = if cfg!(windows) { (45, 70) } else { (90, 120) };
        let long = format!("{}/{}", "d".repeat(dl), "f".repeat(fl));
        for format in ["gnu", "pax"] {
            let dir = tempfile::TempDir::new().unwrap();
            std::fs::create_dir_all(dir.path().join("d".repeat(dl))).unwrap();
            std::fs::write(dir.path().join(&long), "x").unwrap();
            let names = ["d".repeat(dl), long.clone()];
            let out = std::process::Command::new("tar").args(["-cf", "-", &format!("--format={format}"), "--no-recursion", "-C"]).arg(dir.path()).args(&names).output().unwrap();
            assert!(out.status.success(), "tar --format={format}: {}", String::from_utf8_lossy(&out.stderr));
            assert_eq!(run(&entries(&[(&names[0], true), (&names[1], false)]), &out.stdout, 333), Ok(()), "{format}");
        }
    }

    #[test]
    fn a_hostile_or_wrong_archive_is_refused() {
        let want = entries(&[("a.txt", false)]);

        // Something nobody asked for, beside what was asked for.
        let (tar, _d) = planned_archive(&["a.txt", "extra.txt"], |p| {
            std::fs::write(p.join("a.txt"), "1").unwrap();
            std::fs::write(p.join("extra.txt"), "2").unwrap();
        });
        assert!(run(&want, &tar, 4096).unwrap_err().contains("extra.txt"));

        // A link, even one with a harmless name.
        #[cfg(unix)]
        {
            let (tar, _d) = planned_archive(&["a.txt"], |p| std::os::unix::fs::symlink("/etc/passwd", p.join("a.txt")).unwrap());
            assert!(run(&want, &tar, 4096).unwrap_err().contains("link"));
        }

        // A hard link.
        let (tar, _d) = planned_archive(&["a.txt", "b.txt"], |p| {
            std::fs::write(p.join("a.txt"), "1").unwrap();
            std::fs::hard_link(p.join("a.txt"), p.join("b.txt")).unwrap();
        });
        assert!(run(&entries(&[("a.txt", false), ("b.txt", false)]), &tar, 4096).unwrap_err().contains("link"));

        // The wrong kind: a folder where a file was planned, and the reverse.
        let (tar, _d) = planned_archive(&["a.txt"], |p| std::fs::create_dir(p.join("a.txt")).unwrap());
        assert!(run(&want, &tar, 4096).unwrap_err().contains("wrong kind"));

        // Names that climb out, or are absolute.
        for evil in ["../a.txt", "/etc/a.txt", "x/../../a.txt", "./../a.txt"] {
            let tar = forged(evil, b'0', 1);
            let err = run(&entries(&[(evil, false)]), &tar, 4096).unwrap_err();
            assert!(err.contains("climbs") || err.contains("absolute"), "{evil}: {err}");
        }

        // Not tar at all, a bad checksum, a duplicate, a short archive, data after the end.
        assert!(run(&want, &[0x41; 1024], 512).unwrap_err().contains("tar archive"));
        let mut tar = forged("a.txt", b'0', 1);
        tar[10] ^= 0xff;
        assert!(run(&want, &tar, 512).unwrap_err().contains("checksum"));
        let one = forged("a.txt", b'0', 1);
        let mut twice = one[..BLOCK + BLOCK].to_vec();
        twice.extend_from_slice(&one[..BLOCK + BLOCK]);
        twice.extend_from_slice(&[0u8; 1024]);
        assert!(run(&want, &twice, 512).unwrap_err().contains("twice"));
        assert!(run(&entries(&[("a.txt", false), ("b.txt", false)]), &one, 512).unwrap_err().contains("missing"));
        assert!(run(&want, &one[..BLOCK + 100], 512).unwrap_err().contains("early"));
        let mut tail = one.clone();
        tail.extend_from_slice(b"trailing junk");
        assert!(run(&want, &tail, 512).unwrap_err().contains("after the end"));
        // A device node.
        assert!(run(&want, &forged("a.txt", b'3', 0), 512).unwrap_err().contains("kind"));
    }

    /// A one-entry archive with a header written by hand (so a name tar itself would refuse can be tried), a
    /// `size`-byte body and the closing empty blocks.
    fn forged(name: &str, kind: u8, size: usize) -> Vec<u8> {
        let mut h = vec![0u8; BLOCK];
        h[..name.len().min(100)].copy_from_slice(&name.as_bytes()[..name.len().min(100)]);
        h[100..107].copy_from_slice(b"0000644");
        h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| *b as u32).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        h[155] = b' ';
        let mut out = h;
        out.extend(std::iter::repeat_n(b'x', size));
        out.resize(BLOCK + size.div_ceil(BLOCK) * BLOCK, 0);
        out.extend_from_slice(&[0u8; 1024]);
        out
    }

    #[test]
    fn names_are_made_plain_or_refused() {
        assert_eq!(clean("./a/b/").as_deref(), Ok("a/b"));
        assert_eq!(clean("a").as_deref(), Ok("a"));
        assert!(clean("/a").is_err());
        assert!(clean("a/../b").is_err());
        assert!(clean("a//b").is_err());
        assert!(clean("a/./b").is_err());
        assert!(clean("..").is_err());
        assert!(clean("./").is_err());
    }

    #[test]
    fn sizes_come_in_octal_and_in_gnu_binary() {
        assert_eq!(parse_size(b"00000000144\0"), Some(100));
        let mut big = [0u8; 12];
        big[0] = 0x80;
        big[11..].copy_from_slice(&[0x2c]);
        big[10] = 0x01;
        assert_eq!(parse_size(&big), Some(0x12c));
        assert_eq!(parse_octal(b"      \0 "), Some(0));
    }

    // Two real sessions to a throw-away sshd, with Unix file modes; only where those exist.
    #[cfg(unix)]
    mod over_ssh {
        use super::*;
        // -- over two real SSH sessions -----------------------------------------------------------------------------

        use crate::sftp::{SftpManager, TransferProgress};
        use crate::ssh::testutil::{spawn_sshd, target};
        use std::sync::atomic::Ordering;
        use std::sync::{Arc, Mutex};

        struct Collect(Mutex<Vec<TransferProgress>>);
        impl crate::files::ProgressSink for Arc<Collect> {
            fn report(&self, p: TransferProgress) {
                self.0.lock().unwrap().push(p);
            }
        }

        fn tree(root: &std::path::Path, files: usize) {
            use std::os::unix::fs::PermissionsExt;
            for i in 0..files {
                let dir = root.join(format!("d{}/e{}", i % 7, i % 3));
                std::fs::create_dir_all(&dir).unwrap();
                let f = dir.join(format!("file {i}.txt"));
                std::fs::write(&f, format!("content of file {i}\n").repeat(1 + i % 40)).unwrap();
                if i % 11 == 0 {
                    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
                }
            }
            std::fs::create_dir_all(root.join("empty folder")).unwrap();
        }

        /// Same names and contents; with `modes`, same permission bits too (only the tar path keeps those).
        fn same_tree(a: &std::path::Path, b: &std::path::Path, modes: bool) {
            use std::os::unix::fs::PermissionsExt;
            let list = |root: &std::path::Path| {
                let mut out = Vec::new();
                let mut stack = vec![root.to_path_buf()];
                while let Some(d) = stack.pop() {
                    for e in std::fs::read_dir(&d).unwrap().flatten() {
                        let p = e.path();
                        let rel = p.strip_prefix(root).unwrap().to_string_lossy().into_owned();
                        if p.is_dir() {
                            out.push((rel, None, 0));
                            stack.push(p);
                        } else {
                            out.push((rel, Some(std::fs::read(&p).unwrap()), if modes { std::fs::metadata(&p).unwrap().permissions().mode() & 0o777 } else { 0 }));
                        }
                    }
                }
                out.sort();
                out
            };
            assert_eq!(list(a), list(b));
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn many_small_files_go_between_two_hosts_in_one_tar_stream_and_arrive_identical() {
            let dir = tempfile::TempDir::new().unwrap();
            let Some(sshd) = spawn_sshd(dir.path()) else {
                eprintln!("skipping: no usable sshd on this machine");
                return;
            };
            let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
            let m = SftpManager::new();
            m.open("a".into(), &t).await.unwrap();
            m.open("b".into(), &t).await.unwrap();
            let (a, b) = (m.get("a").unwrap(), m.get("b").unwrap());
            let (a, b): (&dyn FileBackend, &dyn FileBackend) = (a.as_ref(), b.as_ref());
            let registry = m.registry();

            let src = dir.path().join("proj");
            tree(&src, 400);
            let dst = dir.path().join("dst");
            std::fs::create_dir(&dst).unwrap();

            let before = crate::files::engine::TAR_RELAYS.load(Ordering::SeqCst);
            let sink = Arc::new(Collect(Mutex::new(Vec::new())));
            crate::files::engine::transfer(&registry, "t1".into(), a, b, &[src.to_string_lossy().into_owned()], &dst.to_string_lossy(), false, crate::files::Conflict::Overwrite, &sink).await;
            let last = sink.0.lock().unwrap().last().cloned();
            assert!(matches!(last, Some(TransferProgress::Done { files: 400, .. })), "{last:?}");
            assert_eq!(crate::files::engine::TAR_RELAYS.load(Ordering::SeqCst), before + 1, "the tar relay did the copying");
            same_tree(&src, &dst.join("proj"), true);
            // Progress ran from the start to the end, in order.
            let bytes: Vec<u64> = sink.0.lock().unwrap().iter().filter_map(|p| if let TransferProgress::Progress { bytes, .. } = p { Some(*bytes) } else { None }).collect();
            assert!(bytes.windows(2).all(|w| w[0] <= w[1]));

            // Few files, resume, skip, keep-both and mixed parents all take the per-file path.
            let before = crate::files::engine::TAR_RELAYS.load(Ordering::SeqCst);
            let few = dir.path().join("few");
            tree(&few, 5);
            let out = dir.path().join("out");
            std::fs::create_dir(&out).unwrap();
            for (id, resume, conflict) in [("few", false, crate::files::Conflict::Overwrite), ("resume", true, crate::files::Conflict::Overwrite)] {
                let sink = Arc::new(Collect(Mutex::new(Vec::new())));
                crate::files::engine::transfer(&registry, id.into(), a, b, &[few.to_string_lossy().into_owned()], &out.to_string_lossy(), resume, conflict, &sink).await;
            }
            let sink = Arc::new(Collect(Mutex::new(Vec::new())));
            let dst2 = dir.path().join("dst2");
            std::fs::create_dir(&dst2).unwrap();
            crate::files::engine::transfer(&registry, "keep".into(), a, b, &[src.to_string_lossy().into_owned()], &dst2.to_string_lossy(), false, crate::files::Conflict::Rename, &sink).await;
            assert_eq!(crate::files::engine::TAR_RELAYS.load(Ordering::SeqCst), before, "those did not use it");
            same_tree(&src, &dst2.join("proj"), false);
            assert!(registry.is_empty());
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_tar_relay_can_be_cancelled_and_both_hosts_carry_on() {
            let dir = tempfile::TempDir::new().unwrap();
            let Some(sshd) = spawn_sshd(dir.path()) else {
                eprintln!("skipping: no usable sshd on this machine");
                return;
            };
            let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
            let m = SftpManager::new();
            m.open("a".into(), &t).await.unwrap();
            m.open("b".into(), &t).await.unwrap();
            let (a, b) = (m.get("a").unwrap(), m.get("b").unwrap());
            let (a, b): (&dyn FileBackend, &dyn FileBackend) = (a.as_ref(), b.as_ref());
            let registry = m.registry();
            let src = dir.path().join("big");
            // Enough bytes (small files, 40 KB each, 48 MB) that the copy is still going when it is stopped.
            std::fs::create_dir_all(&src).unwrap();
            for i in 0..1200 {
                std::fs::write(src.join(format!("f{i}")), vec![b'z'; 40_000]).unwrap();
            }
            let dst = dir.path().join("dst");
            std::fs::create_dir(&dst).unwrap();
            let sink = Arc::new(Collect(Mutex::new(Vec::new())));
            let reg = Arc::clone(&registry);
            let watcher = {
                let sink = Arc::clone(&sink);
                tokio::spawn(async move {
                    for _ in 0..600 {
                        if sink.0.lock().unwrap().iter().any(|p| matches!(p, TransferProgress::Progress { bytes, .. } if *bytes > 0)) {
                            reg.cancel("c1");
                            return;
                        }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                })
            };
            crate::files::engine::transfer(&registry, "c1".into(), a, b, &[src.to_string_lossy().into_owned()], &dst.to_string_lossy(), false, crate::files::Conflict::Overwrite, &sink).await;
            watcher.await.unwrap();
            let last = sink.0.lock().unwrap().last().cloned();
            assert!(matches!(last, Some(TransferProgress::Cancelled)), "{last:?}");
            let got = std::fs::read_dir(dst.join("big")).map(|d| d.count()).unwrap_or(0);
            assert!(got < 1200, "stopped before the end: {got} files");
            // Both sessions still work, and the same copy can be finished with resume (which goes file by file).
            assert!(a.list(&dir.path().to_string_lossy()).await.is_ok());
            assert!(b.list(&dir.path().to_string_lossy()).await.is_ok());
            let sink = Arc::new(Collect(Mutex::new(Vec::new())));
            crate::files::engine::transfer(&registry, "c2".into(), a, b, &[src.to_string_lossy().into_owned()], &dst.to_string_lossy(), true, crate::files::Conflict::Overwrite, &sink).await;
            let last = sink.0.lock().unwrap().last().cloned();
            assert!(matches!(last, Some(TransferProgress::Done { .. })), "{last:?}");
            assert_eq!(std::fs::read_dir(dst.join("big")).unwrap().count(), 1200);
        }
    }
}
