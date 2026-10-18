//! What the in-app editor and the overwrite dialog need from a file place: a text file read into memory with its
//! owner and permissions, written back in place (and refused if someone else changed it meanwhile), written through
//! `sudo` when the person has no right to the file, and a SHA-256 to tell whether two files are the same.
//!
//! Tauri-agnostic, and written against [`FileBackend`], so it works on every place that can read and write.

use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::containers::transport::shell_quote;
use crate::files::{FileBackend, FileError};

/// The biggest text file the editor opens. Past this a local editor is the right tool.
pub const MAX_TEXT_BYTES: usize = 8 << 20;
/// The biggest file a checksum is computed for by reading it (when the host cannot be asked).
pub const MAX_STREAM_HASH_BYTES: u64 = 1 << 30;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("this is not a text file: {0}")]
    NotText(String),
    #[error("this file is {size} bytes; the editor opens files up to {max}")]
    TooLarge { size: u64, max: u64 },
    /// The file on the host is not the one that was opened: someone else saved it since.
    #[error("the file was changed on the host after it was opened")]
    Changed,
    #[error(transparent)]
    File(#[from] FileError),
}

/// A text file as the editor holds it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TextFile {
    /// With `\n` line breaks; `crlf` says to put `\r\n` back when saving.
    pub text: String,
    pub crlf: bool,
    pub size: u64,
    /// Unix seconds, to notice a change made by someone else before saving over it.
    pub modified: Option<u64>,
    /// POSIX mode bits.
    pub permissions: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Saved {
    pub size: u64,
    pub modified: Option<u64>,
}

/// `sh -c '<script>' sh <args…>`: one command line every shell reads the same way, with each argument quoted, so a
/// path with a quote or a space in it stays one word (and the login shell's own syntax never gets a say).
fn sh(script: &str, args: &[&str]) -> String {
    let mut cmd = format!("sh -c {} sh", shell_quote(script));
    for a in args {
        cmd.push(' ');
        cmd.push_str(&shell_quote(a));
    }
    cmd
}

/// Whether a file error is "you may not": the cue to offer saving through `sudo`.
pub fn is_permission_denied(e: &FileError) -> bool {
    use russh_sftp::client::error::Error as E;
    use russh_sftp::protocol::StatusCode;
    match e {
        FileError::Sftp(E::Status(s)) => s.status_code == StatusCode::PermissionDenied,
        FileError::Local { source, .. } => source.kind() == std::io::ErrorKind::PermissionDenied,
        FileError::Backend(m) => m.to_lowercase().contains("permission denied"),
        FileError::Sftp(other) => other.to_string().to_lowercase().contains("permission denied"),
        _ => false,
    }
}

/// Owner and group of a path, asked of the host (GNU `stat`, then the BSD one). `None` where the place has no shell or
/// neither command answers.
async fn owner_of(b: &dyn FileBackend, path: &str) -> Option<(String, String)> {
    let script = r#"stat -c '%U:%G' -- "$1" 2>/dev/null || stat -f '%Su:%Sg' "$1" 2>/dev/null"#;
    let out = b.exec(&sh(script, &[path]), None).await.ok()?;
    if out.code != Some(0) {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let (user, group) = text.trim().split_once(':')?;
    (!user.is_empty() && !group.is_empty()).then(|| (user.to_string(), group.to_string()))
}

/// Read a file as text. Refuses what is not text (a NUL byte, or bytes that are not UTF-8) and what is bigger than
/// `max`, so the editor never shows garbage or a file it could not save back faithfully.
pub async fn read_text(b: &dyn FileBackend, path: &str, max: usize) -> Result<TextFile, EditError> {
    let meta = b.metadata(path).await?;
    if let Some(m) = &meta {
        if m.is_dir {
            return Err(EditError::NotText("it is a folder".into()));
        }
        if m.size > max as u64 {
            return Err(EditError::TooLarge { size: m.size, max: max as u64 });
        }
    }
    let mut reader = b.read(path, 0).await?;
    let mut bytes = Vec::new();
    // One more than the limit: a file that grew since it was listed is still caught.
    (&mut reader).take(max as u64 + 1).read_to_end(&mut bytes).await.map_err(|e| FileError::Local { path: path.into(), source: e })?;
    if bytes.len() > max {
        return Err(EditError::TooLarge { size: bytes.len() as u64, max: max as u64 });
    }
    if bytes.contains(&0) {
        return Err(EditError::NotText("it contains binary data".into()));
    }
    let text = String::from_utf8(bytes.clone()).map_err(|e| EditError::NotText(format!("it is not valid UTF-8 (byte {})", e.utf8_error().valid_up_to())))?;
    // CRLF files keep their line endings: seen as `\n` while editing, `\r\n` again on save. A file that mixes them
    // is treated as LF (a stray `\r\n` is then saved as `\n`).
    let crlf_count = text.matches("\r\n").count();
    let lf_count = text.matches('\n').count();
    let crlf = crlf_count > 0 && crlf_count == lf_count;
    let text = if crlf { text.replace("\r\n", "\n") } else { text };
    let (owner, group) = match owner_of(b, path).await {
        Some((o, g)) => (Some(o), Some(g)),
        None => (None, None),
    };
    Ok(TextFile { text, crlf, size: bytes.len() as u64, modified: meta.as_ref().and_then(|m| m.modified), permissions: meta.as_ref().and_then(|m| m.permissions), owner, group })
}

fn encode(text: &str, crlf: bool) -> Vec<u8> {
    if crlf {
        text.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes()
    } else {
        text.as_bytes().to_vec()
    }
}

/// Save `text` over `path`, in place: the file keeps its owner, mode and inode. With `expect_modified`, a file whose
/// time is no longer that one is not touched ([`EditError::Changed`]): the person is asked first.
pub async fn write_text(b: &dyn FileBackend, path: &str, text: &str, crlf: bool, expect_modified: Option<u64>) -> Result<Saved, EditError> {
    if let Some(expected) = expect_modified {
        if let Some(now) = b.metadata(path).await? {
            if now.modified.is_some_and(|m| m != expected) {
                return Err(EditError::Changed);
            }
        }
    }
    let bytes = encode(text, crlf);
    let mut w = b.write(path, 0, bytes.len() as u64).await?;
    w.write_all(&bytes).await.map_err(|e| FileError::Local { path: path.into(), source: e })?;
    w.shutdown().await.map_err(|e| FileError::Local { path: path.into(), source: e })?;
    let modified = b.metadata(path).await.ok().flatten().and_then(|m| m.modified);
    Ok(Saved { size: bytes.len() as u64, modified })
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RootError {
    /// `sudo` wants a password and none was given.
    #[error("sudo needs a password")]
    NeedsPassword,
    #[error("sudo did not accept that password")]
    WrongPassword,
    #[error("this user is not allowed to use sudo here: {0}")]
    NotAllowed(String),
    #[error("this place cannot run commands, so it cannot use sudo")]
    Unsupported,
    #[error("saving with sudo failed: {0}")]
    Failed(String),
}

/// What `sudo` said, as the cause of a failure.
pub fn classify_sudo_failure(stderr: &str, had_password: bool) -> RootError {
    let s = stderr.to_lowercase();
    if s.contains("incorrect password") || s.contains("sorry, try again") || (had_password && s.contains("no password was provided")) {
        return RootError::WrongPassword;
    }
    if s.contains("a password is required") || s.contains("no tty present") || s.contains("a terminal is required") || s.contains("no password was provided") {
        return RootError::NeedsPassword;
    }
    if s.contains("not in the sudoers") || s.contains("is not allowed to") || s.contains("may not run sudo") {
        return RootError::NotAllowed(stderr.trim().to_string());
    }
    RootError::Failed(stderr.trim().to_string())
}

/// Save `text` over `path` as root, for a file the person may not write: the text goes through `sudo` into a new file
/// beside it and is copied over the old one (not moved over it, which would hand the file to root and lose its mode
/// and group), then the new file is removed. `password` is fed to `sudo -S` on its input, ahead of the text, and is
/// never put in a command line; `None` tries `sudo -n`, which works where no password is asked.
pub async fn write_as_root(b: &dyn FileBackend, path: &str, text: &str, crlf: bool, password: Option<&str>) -> Result<(), RootError> {
    let script = r#"tee "$1.svnew" >/dev/null && cat "$1.svnew" > "$1"; rc=$?; rm -f "$1.svnew"; exit $rc"#;
    let inner = sh(script, &[path]);
    let (command, mut stdin) = match password {
        Some(p) => (format!("sudo -S -p '' {inner}"), format!("{p}\n").into_bytes()),
        None => (format!("sudo -n {inner}"), Vec::new()),
    };
    stdin.extend_from_slice(&encode(text, crlf));
    let out = match b.exec(&command, Some(&stdin)).await {
        Ok(o) => o,
        Err(FileError::Unsupported(_)) => return Err(RootError::Unsupported),
        Err(e) => return Err(RootError::Failed(e.to_string())),
    };
    if out.code == Some(0) {
        return Ok(());
    }
    Err(classify_sudo_failure(&String::from_utf8_lossy(&out.stderr), password.is_some()))
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChecksumMethod {
    /// The host computed it (`sha256sum`), so the file did not have to travel.
    Host,
    /// The file was read and hashed here.
    Read,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Checksum {
    /// 64 lowercase hex digits.
    pub sha256: String,
    pub method: ChecksumMethod,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The first 64-hex-digit word of a tool's output (`sha256sum`, `shasum -a 256`, `openssl dgst -r` all start with it).
pub fn parse_digest(out: &str) -> Option<String> {
    let words: Vec<&str> = out.split_whitespace().collect();
    // `sha256sum` and `shasum` print the digest first; `openssl dgst` prints `SHA256(file)= <digest>`, so it is last.
    let found = [words.first(), words.last()].into_iter().flatten().find(|w| w.len() == 64 && w.chars().all(|c| c.is_ascii_hexdigit())).map(|w| w.to_lowercase());
    found
}

/// SHA-256 of a file: asked of the host where it can run commands (so a big file does not cross the network just to be
/// compared), else read and hashed here (files up to [`MAX_STREAM_HASH_BYTES`]).
pub async fn checksum(b: &dyn FileBackend, path: &str) -> Result<Checksum, EditError> {
    let script = r#"sha256sum -- "$1" 2>/dev/null || shasum -a 256 -- "$1" 2>/dev/null || openssl dgst -sha256 -r -- "$1" 2>/dev/null"#;
    if let Ok(out) = b.exec(&sh(script, &[path]), None).await {
        if out.code == Some(0) {
            if let Some(sha256) = parse_digest(&String::from_utf8_lossy(&out.stdout)) {
                return Ok(Checksum { sha256, method: ChecksumMethod::Host });
            }
        }
    }
    if let Some(m) = b.metadata(path).await? {
        if m.size > MAX_STREAM_HASH_BYTES {
            return Err(EditError::TooLarge { size: m.size, max: MAX_STREAM_HASH_BYTES });
        }
    }
    let mut r = b.read(path, 0).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = r.read(&mut buf).await.map_err(|e| FileError::Local { path: path.into(), source: e })?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(Checksum { sha256: hex(&hasher.finalize()), method: ChecksumMethod::Read })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::local::LocalBackend;

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    #[test]
    fn digests_are_read_from_each_tools_output() {
        assert_eq!(parse_digest(&format!("{ABC}  /etc/hosts\n")).as_deref(), Some(ABC));
        assert_eq!(parse_digest(&format!("{ABC} *file\n")).as_deref(), Some(ABC));
        assert_eq!(parse_digest(&format!("SHA256(/x)= {ABC}\n")).as_deref(), Some(ABC));
        assert_eq!(parse_digest(&format!("{}  f\n", ABC.to_uppercase())).as_deref(), Some(ABC));
        assert_eq!(parse_digest("sha256sum: x: No such file or directory\n"), None);
        assert_eq!(parse_digest(""), None);
        assert_eq!(parse_digest("abc123  f"), None);
    }

    #[test]
    fn commands_keep_every_value_as_one_word() {
        let c = sh(r#"echo "$1""#, &["/tmp/it's a file; rm -rf ~", "$(id)"]);
        assert!(c.starts_with("sh -c 'echo \"$1\"' sh "), "{c}");
        assert!(c.contains(r#"'/tmp/it'\''s a file; rm -rf ~'"#), "{c}");
        assert!(c.ends_with("'$(id)'"), "{c}");
        assert!(!c.contains('\n'));
    }

    #[test]
    fn what_sudo_says_is_sorted() {
        use RootError::*;
        assert_eq!(classify_sudo_failure("sudo: a password is required\n", false), NeedsPassword);
        assert_eq!(classify_sudo_failure("sudo: no tty present and no askpass program specified\n", false), NeedsPassword);
        assert_eq!(classify_sudo_failure("Sorry, try again.\nsudo: no password was provided\nsudo: 1 incorrect password attempt\n", true), WrongPassword);
        assert_eq!(classify_sudo_failure("sudo: no password was provided\n", true), WrongPassword);
        assert_eq!(classify_sudo_failure("sudo: no password was provided\n", false), NeedsPassword);
        assert!(matches!(classify_sudo_failure("alice is not in the sudoers file.  This incident will be reported.\n", true), NotAllowed(_)));
        assert!(matches!(classify_sudo_failure("tee: /etc/x: Read-only file system\n", true), Failed(m) if m.contains("Read-only")));
    }

    #[tokio::test]
    async fn text_round_trips_with_its_line_endings_and_binary_is_refused() {
        let dir = tempfile::TempDir::new().unwrap();
        let b = LocalBackend;
        let p = |n: &str| dir.path().join(n).to_string_lossy().into_owned();

        std::fs::write(p("lf.conf"), "a\nb\n").unwrap();
        let f = read_text(&b, &p("lf.conf"), 1024).await.unwrap();
        assert_eq!((f.text.as_str(), f.crlf, f.size), ("a\nb\n", false, 4));

        std::fs::write(p("crlf.txt"), "a\r\nb\r\n").unwrap();
        let f = read_text(&b, &p("crlf.txt"), 1024).await.unwrap();
        assert_eq!((f.text.as_str(), f.crlf), ("a\nb\n", true), "seen as LF while editing");
        write_text(&b, &p("crlf.txt"), "a\nb\nc\n", f.crlf, None).await.unwrap();
        assert_eq!(std::fs::read(p("crlf.txt")).unwrap(), b"a\r\nb\r\nc\r\n", "and saved as CRLF again");

        std::fs::write(p("mixed.txt"), "a\r\nb\nc\r\n").unwrap();
        assert!(!read_text(&b, &p("mixed.txt"), 1024).await.unwrap().crlf, "a mix is treated as LF");

        std::fs::write(p("bin"), b"abc\0def").unwrap();
        assert!(matches!(read_text(&b, &p("bin"), 1024).await, Err(EditError::NotText(m)) if m.contains("binary")));
        std::fs::write(p("latin1"), [b'c', b'a', b'f', 0xe9]).unwrap();
        assert!(matches!(read_text(&b, &p("latin1"), 1024).await, Err(EditError::NotText(m)) if m.contains("UTF-8")));
        std::fs::write(p("big"), vec![b'x'; 2048]).unwrap();
        assert!(matches!(read_text(&b, &p("big"), 1024).await, Err(EditError::TooLarge { size: 2048, max: 1024 })));
        std::fs::create_dir(p("d")).unwrap();
        assert!(matches!(read_text(&b, &p("d"), 1024).await, Err(EditError::NotText(_))));
        assert!(matches!(read_text(&b, &p("missing"), 1024).await, Err(EditError::File(_))));
    }

    #[tokio::test]
    async fn a_file_changed_since_it_was_opened_is_not_overwritten() {
        let dir = tempfile::TempDir::new().unwrap();
        let b = LocalBackend;
        let path = dir.path().join("f").to_string_lossy().into_owned();
        std::fs::write(&path, "one\n").unwrap();
        let opened = read_text(&b, &path, 1024).await.unwrap();
        // Somebody else saves it, a little later.
        std::fs::write(&path, "theirs\n").unwrap();
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
        assert!(matches!(write_text(&b, &path, "mine\n", false, opened.modified).await, Err(EditError::Changed)));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n", "their change is still there");
        // Told to go ahead (no expectation), it saves.
        write_text(&b, &path, "mine\n", false, None).await.unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "mine\n");
    }

    #[tokio::test]
    async fn a_checksum_by_reading_matches_the_known_digest() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("abc").to_string_lossy().into_owned();
        std::fs::write(&path, "abc").unwrap();
        let c = checksum(&LocalBackend, &path).await.unwrap();
        assert_eq!((c.sha256.as_str(), c.method), (ABC, ChecksumMethod::Read));
    }

    // These lean on Unix permissions, owners and a POSIX shell, so they only exist where those do.
    #[cfg(unix)]
    mod unix_hosts {
        use super::*;
        // -- over a real SFTP connection --------------------------------------------------------------------------

        use crate::ssh::testutil::{spawn_sshd, target};

        fn me() -> String {
            String::from_utf8(std::process::Command::new("id").arg("-un").output().unwrap().stdout).unwrap().trim().to_string()
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn over_sftp_a_file_is_read_with_its_owner_saved_in_place_and_checksummed_by_the_host() {
            use std::os::unix::fs::PermissionsExt;
            let dir = tempfile::TempDir::new().unwrap();
            let Some(sshd) = spawn_sshd(dir.path()) else {
                eprintln!("skipping: no usable sshd on this machine");
                return;
            };
            let (conn, _) = crate::sftp::SftpConn::open(&target(&sshd, &sshd.client_key, dir.path().join("kh"))).await.unwrap();
            let path = dir.path().join("nginx it's.conf");
            std::fs::write(&path, "server {\n  listen 80;\n}\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
            let p = path.to_string_lossy().into_owned();

            let f = read_text(&conn, &p, 1024).await.unwrap();
            assert_eq!(f.text, "server {\n  listen 80;\n}\n");
            assert_eq!(f.permissions, Some(0o640));
            assert_eq!(f.owner.as_deref(), Some(me().as_str()), "the owner comes from the host");
            assert!(f.group.as_deref().is_some_and(|g| !g.is_empty()));
            assert!(f.modified.is_some());

            // Saved in place: same inode, same mode.
            use std::os::unix::fs::MetadataExt;
            let inode = std::fs::metadata(&path).unwrap().ino();
            let saved = write_text(&conn, &p, "server {\n  listen 8080;\n}\n", false, f.modified).await.unwrap();
            assert_eq!(saved.size, 26);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "server {\n  listen 8080;\n}\n");
            assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode, "same file, not a replacement");
            assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777, 0o640, "same mode");

            // The host computes the checksum, and it is the right one.
            let c = checksum(&conn, &p).await.unwrap();
            assert_eq!(c.method, ChecksumMethod::Host);
            let mut h = Sha256::new();
            h.update(std::fs::read(&path).unwrap());
            assert_eq!(c.sha256, hex(&h.finalize()));

            // Not writable by this user: it is recognised as a permission problem (skipped when running as root).
            if me() != "root" {
                let ro = dir.path().join("ro.conf");
                std::fs::write(&ro, "x\n").unwrap();
                std::fs::set_permissions(&ro, std::fs::Permissions::from_mode(0o444)).unwrap();
                let err = write_text(&conn, &ro.to_string_lossy(), "y\n", false, None).await.expect_err("refused");
                assert!(matches!(&err, EditError::File(e) if is_permission_denied(e)), "{err:?}");
                assert_eq!(std::fs::read_to_string(&ro).unwrap(), "x\n");
                // And an ordinary failure is not mistaken for it.
                let missing = write_text(&conn, "/no/such/dir/x", "y\n", false, None).await.expect_err("refused");
                assert!(matches!(&missing, EditError::File(e) if !is_permission_denied(e)), "{missing:?}");
                // What sudo does here depends on the machine: asking for a password this test does not have (refused, and
                // nothing is written) or, as on CI runners, not asking at all (written). Either way the answer and the file agree.
                let r = write_as_root(&conn, &ro.to_string_lossy(), "y\n", false, None).await;
                let now = std::fs::read_to_string(&ro).unwrap();
                match &r {
                    Ok(_) => assert_eq!(now, "y\n", "{r:?}"),
                    Err(_) => assert_eq!(now, "x\n", "{r:?}"),
                }
            }
        }

        // -- sudo, against a stand-in that reads its password the way the real one does ------------------------------

        /// A local place whose commands run through a `sudo` that wants the password "secret" on its first input line.
        struct Shim {
            bin: std::path::PathBuf,
        }

        #[async_trait::async_trait]
        impl FileBackend for Shim {
            fn caps(&self) -> crate::files::Caps {
                LocalBackend.caps()
            }
            async fn home(&self) -> Result<String, FileError> {
                LocalBackend.home().await
            }
            async fn list(&self, dir: &str) -> Result<Vec<crate::files::FileEntry>, FileError> {
                LocalBackend.list(dir).await
            }
            async fn stat(&self, path: &str) -> Result<Option<crate::files::Stat>, FileError> {
                LocalBackend.stat(path).await
            }
            async fn mkdir(&self, dir: &str, name: &str) -> Result<(), FileError> {
                LocalBackend.mkdir(dir, name).await
            }
            async fn rename(&self, from: &str, new_name: &str) -> Result<(), FileError> {
                LocalBackend.rename(from, new_name).await
            }
            async fn remove(&self, path: &str) -> Result<(), FileError> {
                LocalBackend.remove(path).await
            }
            async fn read(&self, path: &str, offset: u64) -> Result<crate::files::Reader, FileError> {
                LocalBackend.read(path, offset).await
            }
            async fn write(&self, path: &str, offset: u64, size: u64) -> Result<crate::files::Writer, FileError> {
                LocalBackend.write(path, offset, size).await
            }
            async fn exec(&self, command: &str, stdin: Option<&[u8]>) -> Result<crate::ssh::ExecOutput, FileError> {
                use std::process::Stdio;
                let mut child = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .env("PATH", format!("{}:{}", self.bin.display(), std::env::var("PATH").unwrap_or_default()))
                    .stdin(Stdio::piped())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .map_err(|e| FileError::Backend(e.to_string()))?;
                let mut input = child.stdin.take().unwrap();
                if let Some(bytes) = stdin {
                    let _ = input.write_all(bytes).await;
                }
                drop(input);
                let out = child.wait_with_output().await.map_err(|e| FileError::Backend(e.to_string()))?;
                Ok(crate::ssh::ExecOutput { stdout: out.stdout, stderr: out.stderr, code: out.status.code().map(|c| c as u32), truncated: false })
            }
        }

        fn shim(dir: &std::path::Path) -> Shim {
            use std::os::unix::fs::PermissionsExt;
            let bin = dir.join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            let script = "#!/bin/sh\nif [ \"$1\" = \"-n\" ]; then echo 'sudo: a password is required' >&2; exit 1; fi\nif [ \"$1\" = \"-S\" ]; then shift 3; read pw; if [ \"$pw\" != \"secret\" ]; then echo 'Sorry, try again.' >&2; echo 'sudo: 1 incorrect password attempt' >&2; exit 1; fi; fi\nexec \"$@\"\n";
            std::fs::write(bin.join("sudo"), script).unwrap();
            std::fs::set_permissions(bin.join("sudo"), std::fs::Permissions::from_mode(0o755)).unwrap();
            Shim { bin }
        }

        #[tokio::test]
        async fn saving_with_sudo_feeds_the_password_first_and_keeps_the_files_owner_and_mode() {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let dir = tempfile::TempDir::new().unwrap();
            let b = shim(dir.path());
            let path = dir.path().join("it's a file; $(id).conf");
            std::fs::write(&path, "old\n").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
            let p = path.to_string_lossy().into_owned();
            let (inode, uid) = {
                let m = std::fs::metadata(&path).unwrap();
                (m.ino(), m.uid())
            };

            // No password: sudo asks, and nothing is touched.
            assert_eq!(write_as_root(&b, &p, "new\n", false, None).await, Err(RootError::NeedsPassword));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
            // The wrong one.
            assert_eq!(write_as_root(&b, &p, "new\n", false, Some("nope")).await, Err(RootError::WrongPassword));
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "old\n");
            // The right one: the text arrives whole, after the password line, and is not mistaken for part of it.
            write_as_root(&b, &p, "line one\nline two\n\nlast without newline", false, Some("secret")).await.unwrap();
            assert_eq!(std::fs::read_to_string(&path).unwrap(), "line one\nline two\n\nlast without newline");
            let m = std::fs::metadata(&path).unwrap();
            assert_eq!((m.ino(), m.uid(), m.permissions().mode() & 0o7777), (inode, uid, 0o640), "the same file: owner, mode and inode kept");
            assert!(!dir.path().join("it's a file; $(id).conf.svnew").exists(), "no temporary file is left");
            // CRLF is put back, and a password never appears in what is run.
            write_as_root(&b, &p, "a\nb\n", true, Some("secret")).await.unwrap();
            assert_eq!(std::fs::read(&path).unwrap(), b"a\r\nb\r\n");
        }
    }

    #[tokio::test]
    async fn a_place_that_cannot_run_commands_says_so() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("f").to_string_lossy().into_owned();
        assert_eq!(write_as_root(&LocalBackend, &path, "x", false, Some("pw")).await, Err(RootError::Unsupported));
    }
}
