//! FTP and FTPS as a file backend, on `suppaftp`.
//!
//! FTP is one conversation at a time, so a pane keeps one connection for
//! browsing (list, rename, delete, …) and every file transfer opens its own,
//! in a task that owns it; the pane sees a reader or writer (see `pipe`).
//!
//! Security: plain FTP is labelled as unencrypted by the window. With TLS,
//! the server's certificate is pinned by fingerprint (see `certs`): an unknown
//! or changed one fails the connection with the details, before the password
//! is sent, and connecting again with that fingerprint accepted goes ahead.

use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use suppaftp::tokio::{AsyncRustlsConnector, AsyncRustlsFtpStream};
use suppaftp::types::FileType;
use suppaftp::{FtpError, Mode};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use super::pipe::{PipeReader, PipeWriter};
use super::{Caps, FileBackend, FileError, Reader, Stat, Writer};
use crate::certs::{self, CertCheck, CertInfo, Verdict};
use crate::sftp::FileEntry;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// A browsing connection idle this long is checked before it is used again.
const IDLE_CHECK: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FtpTls {
    /// Plain FTP: everything, the password included, crosses the network readable.
    None,
    /// Start plain and switch to TLS (`AUTH TLS`) before signing in.
    #[default]
    Explicit,
    /// TLS from the first byte, usually on port 990.
    Implicit,
}

#[derive(Debug, Clone)]
pub struct FtpSettings {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub tls: FtpTls,
    pub cert: CertCheck,
}

/// What the server announces for a passive data connection, corrected for NAT: a
/// private, loopback or link-local address that isn't the one the control
/// connection reached can't be right from here, so use the control address.
pub fn passive_target(control: IpAddr, announced: SocketAddr) -> SocketAddr {
    let unusable = match announced.ip() {
        IpAddr::V4(a) => a.is_private() || a.is_loopback() || a.is_link_local() || a.is_unspecified() || a.is_broadcast(),
        IpAddr::V6(a) => a.is_loopback() || a.is_unspecified() || (a.segments()[0] & 0xffc0) == 0xfe80 || (a.segments()[0] & 0xfe00) == 0xfc00,
    };
    if unusable && announced.ip() != control {
        SocketAddr::new(control, announced.port())
    } else {
        announced
    }
}

// -- listings -----------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FtpEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub size: u64,
    pub modified: Option<u64>,
    pub mode: Option<u32>,
}

/// `YYYYMMDDHHMMSS[.sss]` (UTC) as Unix seconds.
fn parse_modify(v: &str) -> Option<u64> {
    let digits = v.split('.').next()?;
    if digits.len() != 14 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n = |r: std::ops::Range<usize>| digits[r].parse::<i64>().ok();
    let (y, mo, d, h, mi, s) = (n(0..4)?, n(4..6)?, n(6..8)?, n(8..10)?, n(10..12)?, n(12..14)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }
    u64::try_from(super::days_from_civil(y, mo as u32, d as u32) * 86_400 + h * 3600 + mi * 60 + s).ok()
}

/// One `MLSD` or `MLST` line: `fact=value;fact=value; name`. Directory markers for `.` and `..` give `None`.
pub fn parse_mlsd(line: &str) -> Option<FtpEntry> {
    let line = line.trim_end_matches(['\r', '\n']);
    let (facts, name) = line.split_once(' ')?;
    if name.is_empty() {
        return None;
    }
    let (mut kind, mut size, mut modified, mut mode) = (String::new(), 0u64, None, None);
    for fact in facts.split(';').filter(|f| !f.is_empty()) {
        let (k, v) = fact.split_once('=')?;
        match k.to_ascii_lowercase().as_str() {
            "type" => kind = v.to_ascii_lowercase(),
            "size" => size = v.parse().ok()?,
            "modify" => modified = parse_modify(v),
            "unix.mode" => mode = u32::from_str_radix(v, 8).ok().map(|m| m & 0o7777),
            _ => {}
        }
    }
    if kind == "cdir" || kind == "pdir" {
        return None;
    }
    let is_symlink = kind.starts_with("os.unix=slink") || kind.starts_with("os.unix=symlink");
    let is_dir = kind == "dir";
    Some(FtpEntry { name: name.to_string(), is_dir, is_symlink, size: if is_dir { 0 } else { size }, modified, mode })
}

fn from_list_line(line: &str) -> Option<FtpEntry> {
    // `total N` heads a Unix listing; some parsers would read it as a file.
    if line.trim_start().starts_with("total ") {
        return None;
    }
    let f = suppaftp::list::File::try_from(line).ok()?;
    if f.name() == "." || f.name() == ".." {
        return None;
    }
    let modified = f.modified().duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs());
    Some(FtpEntry { name: f.name().to_string(), is_dir: f.is_directory(), is_symlink: f.is_symlink(), size: if f.is_directory() { 0 } else { f.size() as u64 }, modified, mode: None })
}

// -- TLS ---------------------------------------------------------------------------------------------------

/// Accepts a certificate only if it is the pinned one (or the one just accepted),
/// and remembers what the server showed so a refusal can say what it was.
#[derive(Debug)]
struct PinVerifier {
    check: CertCheck,
    seen: Mutex<Option<CertInfo>>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl rustls::client::danger::ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        let info = certs::cert_info_from_der(end_entity).map_err(rustls::Error::General)?;
        let verdict = self.check.verdict(&info.fingerprint);
        *self.seen.lock().unwrap_or_else(|p| p.into_inner()) = Some(info);
        match verdict {
            Verdict::Trusted => Ok(rustls::client::danger::ServerCertVerified::assertion()),
            _ => Err(rustls::Error::InvalidCertificate(rustls::CertificateError::ApplicationVerificationFailure)),
        }
    }

    fn verify_tls12_signature(&self, message: &[u8], cert: &rustls::pki_types::CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(&self, message: &[u8], cert: &rustls::pki_types::CertificateDer<'_>, dss: &rustls::DigitallySignedStruct) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

fn tls_connector(verifier: Arc<PinVerifier>) -> Result<AsyncRustlsConnector, FileError> {
    let provider = Arc::clone(&verifier.provider);
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| FileError::Backend(format!("TLS setup failed: {e}")))?
        .dangerous()
        .with_custom_certificate_verifier(verifier)
        .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)).into())
}

fn ftp_err(e: FtpError) -> FileError {
    match e {
        FtpError::UnexpectedResponse(r) => FileError::Backend(String::from_utf8_lossy(&r.body).trim().to_string()),
        other => FileError::Backend(other.to_string()),
    }
}

// -- the connection -------------------------------------------------------------------------------------------

struct Ctl {
    ftp: AsyncRustlsFtpStream,
    used: Instant,
    /// Where signing in landed; folders are checked by going there and back.
    home: String,
    mlsd: Option<bool>,
}

pub struct FtpConn {
    settings: FtpSettings,
    verifier: Arc<PinVerifier>,
    main: tokio::sync::Mutex<Option<Ctl>>,
}

impl FtpConn {
    /// Connect and sign in. Also says which certificate the server showed, when there was TLS.
    pub async fn open(settings: FtpSettings) -> Result<(Self, Option<CertInfo>), FileError> {
        let verifier = Arc::new(PinVerifier { check: settings.cert.clone(), seen: Mutex::new(None), provider: Arc::new(rustls::crypto::ring::default_provider()) });
        let conn = Self { settings, verifier, main: tokio::sync::Mutex::new(None) };
        let ctl = conn.connect().await?;
        *conn.main.lock().await = Some(ctl);
        let seen = conn.verifier.seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
        Ok((conn, seen))
    }

    /// Turn a failed secure connection into the certificate question when that was the reason.
    fn explain(&self, e: FtpError) -> FileError {
        let seen = self.verifier.seen.lock().unwrap_or_else(|p| p.into_inner()).clone();
        if let (Some(info), FtpError::SecureError(_)) = (&seen, &e) {
            match self.settings.cert.verdict(&info.fingerprint) {
                Verdict::Unknown => return FileError::UntrustedCertificate(Box::new(info.clone())),
                Verdict::Changed { expected } => return FileError::CertificateChanged { expected, found: Box::new(info.clone()) },
                Verdict::Trusted => {}
            }
        }
        ftp_err(e)
    }

    /// Make listings use `LIST`, as for a server that has no `MLSD`.
    #[cfg(test)]
    async fn without_mlsd(&self) {
        if let Some(c) = self.main.lock().await.as_mut() {
            c.mlsd = Some(false);
        }
    }

    /// A new, signed-in connection in binary mode.
    async fn connect(&self) -> Result<Ctl, FileError> {
        let s = &self.settings;
        let timeout = |what: &str| FileError::Backend(format!("timed out {what} {}:{}", s.host, s.port));
        let connector = || tls_connector(Arc::clone(&self.verifier));
        let mut ftp = match s.tls {
            FtpTls::None | FtpTls::Explicit => {
                let tcp = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((s.host.as_str(), s.port)))
                    .await
                    .map_err(|_| timeout("connecting to"))?
                    .map_err(|e| FileError::Backend(format!("couldn't reach {}:{}: {e}", s.host, s.port)))?;
                let plain = AsyncRustlsFtpStream::connect_with_stream(tcp).await.map_err(ftp_err)?;
                if s.tls == FtpTls::Explicit {
                    plain.into_secure(connector()?, &s.host).await.map_err(|e| self.explain(e))?
                } else {
                    plain
                }
            }
            FtpTls::Implicit => tokio::time::timeout(CONNECT_TIMEOUT, AsyncRustlsFtpStream::connect_secure_implicit((s.host.as_str(), s.port), connector()?, &s.host))
                .await
                .map_err(|_| timeout("connecting to"))?
                .map_err(|e| self.explain(e))?,
        };
        if s.tls == FtpTls::Implicit {
            // Explicit TLS does this itself; with implicit TLS the data connections stay in the clear unless asked.
            ftp.custom_command("PBSZ 0", &[suppaftp::Status::CommandOk]).await.map_err(ftp_err)?;
            ftp.custom_command("PROT P", &[suppaftp::Status::CommandOk]).await.map_err(ftp_err)?;
        }
        let control_ip = ftp.get_ref().await.peer_addr().map(|a| a.ip()).map_err(|e| FileError::Backend(e.to_string()))?;
        if control_ip.is_ipv6() {
            // PASV can't carry an IPv6 address; EPSV only names a port, so it is always right.
            ftp.set_mode(Mode::ExtendedPassive);
        } else {
            ftp.set_mode(Mode::Passive);
            ftp = ftp.passive_stream_builder(move |announced| {
                let target = passive_target(control_ip, announced);
                Box::pin(async move { TcpStream::connect(target).await.map_err(FtpError::ConnectionError) })
            });
        }
        ftp.login(s.user.as_str(), s.password.as_str()).await.map_err(|e| match e {
            FtpError::UnexpectedResponse(r) if r.status as u32 == 530 => FileError::Backend("the server refused this user name or password".into()),
            other => ftp_err(other),
        })?;
        ftp.transfer_type(FileType::Binary).await.map_err(ftp_err)?;
        let home = ftp.pwd().await.map_err(ftp_err)?;
        Ok(Ctl { ftp, used: Instant::now(), home, mlsd: None })
    }

    /// The browsing connection, replaced if it has died.
    async fn ctl(&self) -> Result<tokio::sync::MutexGuard<'_, Option<Ctl>>, FileError> {
        let mut guard = self.main.lock().await;
        let stale = match guard.as_mut() {
            Some(c) if c.used.elapsed() >= IDLE_CHECK => c.ftp.noop().await.is_err(),
            Some(_) => false,
            None => true,
        };
        if stale {
            *guard = Some(self.connect().await?);
        }
        if let Some(c) = guard.as_mut() {
            c.used = Instant::now();
        }
        Ok(guard)
    }
}

/// FTP is a text protocol: a line break in a path would end the command and start another.
fn safe(path: &str) -> Result<&str, FileError> {
    if path.contains(['\r', '\n', '\0']) {
        return Err(FileError::InvalidPath(format!("{path:?} has a line break or NUL in it, which FTP can't carry")));
    }
    Ok(path)
}

fn ctl_of<'a>(g: &'a mut tokio::sync::MutexGuard<'_, Option<Ctl>>) -> &'a mut Ctl {
    g.as_mut().expect("ctl() leaves a connection in place")
}

impl FtpConn {
    async fn entries(&self, dir: &str) -> Result<Vec<FtpEntry>, FileError> {
        safe(dir)?;
        let mut g = self.ctl().await?;
        let c = ctl_of(&mut g);
        if c.mlsd != Some(false) {
            match c.ftp.mlsd(Some(dir)).await {
                Ok(lines) => {
                    c.mlsd = Some(true);
                    return Ok(lines.iter().filter_map(|l| parse_mlsd(l)).collect());
                }
                // A server without MLSD says so with 500, 501 or 502; anything else is a real problem.
                Err(FtpError::UnexpectedResponse(r)) if matches!(r.status as u32, 500..=502) => c.mlsd = Some(false),
                Err(e) => return Err(ftp_err(e)),
            }
        }
        let lines = c.ftp.list(Some(dir)).await.map_err(ftp_err)?;
        Ok(lines.iter().filter_map(|l| from_list_line(l)).collect())
    }
}

#[async_trait]
impl FileBackend for FtpConn {
    fn caps(&self) -> Caps {
        // FTP has no portable way to change permissions, and editing in place is SFTP's.
        Caps { chmod: false, edit: false, ..Caps::files() }
    }

    async fn home(&self) -> Result<String, FileError> {
        let mut g = self.ctl().await?;
        Ok(ctl_of(&mut g).home.clone())
    }

    async fn list(&self, dir: &str) -> Result<Vec<FileEntry>, FileError> {
        let mut out: Vec<FileEntry> = self
            .entries(dir)
            .await?
            .into_iter()
            .map(|e| FileEntry { path: self.join(dir, &e.name), name: e.name, is_dir: e.is_dir, is_symlink: e.is_symlink, size: e.size, modified: e.modified, permissions: e.mode })
            .collect();
        out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(out)
    }

    async fn stat(&self, path: &str) -> Result<Option<Stat>, FileError> {
        safe(path)?;
        let mut g = self.ctl().await?;
        let c = ctl_of(&mut g);
        // A folder is something you can go into; go back where signing in landed afterwards.
        if c.ftp.cwd(path).await.is_ok() {
            let home = c.home.clone();
            c.ftp.cwd(&home).await.map_err(ftp_err)?;
            return Ok(Some(Stat { is_dir: true, size: 0 }));
        }
        match c.ftp.size(path).await {
            Ok(n) => Ok(Some(Stat { is_dir: false, size: n as u64 })),
            Err(_) => Ok(None),
        }
    }

    async fn mkdir(&self, dir: &str, name: &str) -> Result<(), FileError> {
        safe(dir)?;
        safe(name)?;
        if name.is_empty() || name == "." || name == ".." || name.contains('/') {
            return Err(FileError::InvalidPath(name.to_string()));
        }
        let mut g = self.ctl().await?;
        ctl_of(&mut g).ftp.mkdir(self.join(dir, name)).await.map_err(ftp_err)
    }

    async fn rename(&self, from: &str, new_name: &str) -> Result<(), FileError> {
        safe(from)?;
        safe(new_name)?;
        if new_name.is_empty() || new_name == "." || new_name == ".." || new_name.contains('/') {
            return Err(FileError::InvalidPath(new_name.to_string()));
        }
        let parent = self.parent(from).ok_or_else(|| FileError::InvalidPath(from.to_string()))?;
        let to = self.join(&parent, new_name);
        let mut g = self.ctl().await?;
        ctl_of(&mut g).ftp.rename(from, &to).await.map_err(ftp_err)
    }

    async fn remove(&self, path: &str) -> Result<(), FileError> {
        safe(path)?;
        if path.trim_matches('/').is_empty() || path == "." || path == ".." {
            return Err(FileError::InvalidPath(path.to_string()));
        }
        let Some(stat) = self.stat(path).await? else {
            return Err(FileError::Backend(format!("{path}: no such file or directory")));
        };
        if !stat.is_dir {
            let mut g = self.ctl().await?;
            return ctl_of(&mut g).ftp.rm(path).await.map_err(ftp_err);
        }
        // Post-order walk: files first, each folder once it is empty.
        let mut stack = vec![(path.to_string(), false)];
        while let Some((dir, visited)) = stack.pop() {
            if visited {
                let mut g = self.ctl().await?;
                ctl_of(&mut g).ftp.rmdir(&dir).await.map_err(ftp_err)?;
                continue;
            }
            stack.push((dir.clone(), true));
            for e in self.entries(&dir).await? {
                let child = self.join(&dir, &e.name);
                if e.is_dir && !e.is_symlink {
                    stack.push((child, false));
                } else {
                    let mut g = self.ctl().await?;
                    ctl_of(&mut g).ftp.rm(&child).await.map_err(ftp_err)?;
                }
            }
        }
        Ok(())
    }

    async fn read(&self, path: &str, offset: u64) -> Result<Reader, FileError> {
        safe(path)?;
        let mut ctl = self.connect().await?;
        let expected = ctl.ftp.size(path).await.ok().map(|n| (n as u64).saturating_sub(offset));
        if offset > 0 {
            ctl.ftp.resume_transfer(offset as usize).await.map_err(ftp_err)?;
        }
        let mut data = ctl.ftp.retr_as_stream(path).await.map_err(ftp_err)?;
        let (mut tx, rx) = tokio::io::duplex(256 * 1024);
        let failure = Arc::new(Mutex::new(None));
        let failed = Arc::clone(&failure);
        tokio::spawn(async move {
            let result: Result<(), FileError> = async {
                tokio::io::copy(&mut data, &mut tx).await.map_err(|e| FileError::Backend(e.to_string()))?;
                data.finish().await.map_err(ftp_err)?; // the server's "transfer complete"
                tx.shutdown().await.map_err(|e| FileError::Backend(e.to_string()))
            }
            .await;
            if let Err(e) = result {
                *failed.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.to_string());
            }
            let _ = ctl.ftp.quit().await;
        });
        Ok(Box::new(PipeReader::new(rx, expected, failure)))
    }

    async fn write(&self, path: &str, offset: u64, size: u64) -> Result<Writer, FileError> {
        safe(path)?;
        let mut ctl = self.connect().await?;
        let mut data = if offset > 0 { ctl.ftp.append_with_stream(path).await } else { ctl.ftp.put_with_stream(path).await }.map_err(ftp_err)?;
        let (tx, mut rx) = tokio::io::duplex(256 * 1024);
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();
        tokio::spawn(async move {
            let result: Result<(), FileError> = async {
                tokio::io::copy(&mut rx, &mut data).await.map_err(|e| FileError::Backend(e.to_string()))?;
                data.finish().await.map_err(ftp_err)
            }
            .await;
            let _ = ctl.ftp.quit().await;
            let _ = done_tx.send(result.map_err(|e| e.to_string()));
        });
        Ok(Box::new(PipeWriter::new(tx, size, done_rx)))
    }

    async fn close(&self) {
        if let Some(mut c) = self.main.lock().await.take() {
            let _ = c.ftp.quit().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn addr(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn a_private_passive_address_is_replaced_by_the_one_we_connected_to() {
        let control: IpAddr = "203.0.113.9".parse().unwrap();
        // The server sits behind NAT and announces its inside address.
        for inside in ["10.1.2.3:30000", "172.16.0.5:30001", "172.31.255.1:2", "192.168.1.10:30002", "127.0.0.1:30003", "169.254.1.1:4", "0.0.0.0:5"] {
            assert_eq!(passive_target(control, addr(inside)), SocketAddr::new(control, addr(inside).port()), "{inside}");
        }
        // A public address is believed, even if it differs from the control one.
        assert_eq!(passive_target(control, addr("198.51.100.7:30000")), addr("198.51.100.7:30000"));
        // The control address itself is fine.
        assert_eq!(passive_target(control, addr("203.0.113.9:30000")), addr("203.0.113.9:30000"));
        // A private control address (a LAN server) with the same private announcement is left alone.
        let lan: IpAddr = "192.168.1.10".parse().unwrap();
        assert_eq!(passive_target(lan, addr("192.168.1.10:30000")), addr("192.168.1.10:30000"));
        // Not private ranges that look close.
        assert_eq!(passive_target(control, addr("172.32.0.1:6")), addr("172.32.0.1:6"));
        assert_eq!(passive_target(control, addr("11.0.0.1:7")), addr("11.0.0.1:7"));
    }

    #[test]
    fn ipv6_private_ranges_are_replaced_too() {
        let control: IpAddr = "2001:db8::1".parse().unwrap();
        assert_eq!(passive_target(control, addr("[fe80::1]:30000")), SocketAddr::new(control, 30000));
        assert_eq!(passive_target(control, addr("[fd00::5]:30001")), SocketAddr::new(control, 30001));
        assert_eq!(passive_target(control, addr("[::1]:30002")), SocketAddr::new(control, 30002));
        assert_eq!(passive_target(control, addr("[2001:db8::99]:30003")), addr("[2001:db8::99]:30003"));
    }

    #[test]
    fn mlsd_lines_are_read() {
        let f = parse_mlsd("type=file;size=1234;modify=20260103101530.123;UNIX.mode=0644; my file.txt").unwrap();
        assert_eq!((f.name.as_str(), f.is_dir, f.size, f.mode), ("my file.txt", false, 1234, Some(0o644)));
        assert_eq!(f.modified, Some(1_767_435_330), "2026-01-03 10:15:30 UTC");
        let d = parse_mlsd("Type=DIR;Modify=20260101000000;Perm=flcdmpe; sub").unwrap();
        assert!(d.is_dir && d.size == 0 && d.modified == Some(1_767_225_600));
        let l = parse_mlsd("type=OS.unix=slink:/etc;size=4; link").unwrap();
        assert!(l.is_symlink && !l.is_dir);
        assert_eq!(parse_mlsd("type=file;size=1; name with  two  spaces").unwrap().name, "name with  two  spaces");
        assert_eq!(parse_mlsd("type=file;size=1;  leading").unwrap().name, " leading");
        assert_eq!(parse_mlsd("type=file;size=1; a;b=c.txt").unwrap().name, "a;b=c.txt", "semicolons after the first space belong to the name");
    }

    #[test]
    fn mlsd_noise_is_skipped() {
        assert_eq!(parse_mlsd("type=cdir;modify=20260101000000; ."), None);
        assert_eq!(parse_mlsd("type=pdir;modify=20260101000000; .."), None);
        assert_eq!(parse_mlsd(""), None);
        assert_eq!(parse_mlsd("type=file;size=notanumber; x"), None);
        assert_eq!(parse_mlsd("nofacts"), None);
        // A bad date doesn't hide the file, it just has no date.
        assert_eq!(parse_mlsd("type=file;size=1;modify=garbage; x").unwrap().modified, None);
        assert_eq!(parse_mlsd("type=file;size=1;modify=20261332250000; x").unwrap().modified, None);
    }

    #[test]
    fn unix_list_lines_are_the_fallback() {
        let f = from_list_line("-rw-r--r--    1 ftp      ftp          1234 Oct 06 04:28 hello.txt").unwrap();
        assert_eq!((f.name.as_str(), f.size, f.is_dir), ("hello.txt", 1234, false));
        let d = from_list_line("drwxr-xr-x    2 ftp      ftp          4096 Oct 06  2025 docs").unwrap();
        assert!(d.is_dir && d.size == 0);
        assert!(from_list_line("total 12").is_none());
        assert!(from_list_line("drwxr-xr-x 2 ftp ftp 4096 Oct 06 2025 .").is_none());
    }

    #[test]
    fn errors_say_what_the_server_said() {
        let r = suppaftp::types::Response::new(suppaftp::Status::FileUnavailable, b"550 No such file or directory.".to_vec());
        let e = ftp_err(FtpError::UnexpectedResponse(r));
        assert_eq!(e.to_string(), "550 No such file or directory.");
    }

    #[test]
    fn paths_cannot_smuggle_ftp_commands() {
        assert!(safe("/ok/a b'c$(x).txt").is_ok());
        assert!(safe("/héllo/東京").is_ok());
        for bad in ["a\r\nDELE important", "a\nb", "a\rb", "a\0b"] {
            assert!(safe(bad).is_err(), "{bad:?}");
        }
    }

    // -- against real FTP servers ----------------------------------------------------------------------------
    //
    // SSHVAULT_FTP_ADDR (host:port of a server that also offers explicit TLS), SSHVAULT_FTP_USER and
    // SSHVAULT_FTP_PASS; SSHVAULT_FTP_NAT_ADDR for a server that announces an unreachable private address in
    // its passive replies. For example two stilliard/pure-ftpd containers (see feat.md). Skipped without them.
    mod live {
        use super::*;
        use crate::files::engine::{self, Conflict, TransferProgress, TransferRegistry};
        use crate::files::local::LocalBackend;
        use crate::files::ProgressSink;

        fn env(name: &str) -> Option<String> {
            std::env::var(name).ok().filter(|v| !v.is_empty())
        }

        fn settings(addr_var: &str, tls: FtpTls, cert: CertCheck) -> Option<FtpSettings> {
            let addr = env(addr_var)?;
            let (host, port) = addr.rsplit_once(':')?;
            Some(FtpSettings { host: host.into(), port: port.parse().ok()?, user: env("SSHVAULT_FTP_USER")?, password: env("SSHVAULT_FTP_PASS")?, tls, cert })
        }

        #[derive(Default)]
        struct Collect(Mutex<Vec<TransferProgress>>);
        impl ProgressSink for Arc<Collect> {
            fn report(&self, p: TransferProgress) {
                self.0.lock().unwrap().push(p);
            }
        }

        async fn copy(src: &dyn FileBackend, dst: &dyn FileBackend, sources: &[String], dest: &str, resume: bool) -> TransferProgress {
            let sink = Arc::new(Collect::default());
            engine::transfer(&TransferRegistry::default(), "t".into(), src, dst, sources, dest, resume, Conflict::Overwrite, &sink).await;
            let last = sink.0.lock().unwrap().last().cloned().unwrap();
            last
        }

        /// Give a step a deadline, so a hang names where it is.
        async fn within<T>(what: &str, f: impl std::future::Future<Output = T>) -> T {
            tokio::time::timeout(Duration::from_secs(25), f).await.unwrap_or_else(|_| panic!("{what} hung"))
        }

        /// A fresh folder on the server for one test.
        async fn scratch(conn: &FtpConn, tag: &str) -> String {
            let home = conn.home().await.unwrap();
            let name = format!("{tag}-{}", std::process::id());
            let path = conn.join(&home, &name);
            if conn.stat(&path).await.unwrap().is_some() {
                conn.remove(&path).await.unwrap();
            }
            conn.mkdir(&home, &name).await.unwrap();
            path
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn browsing_and_changing_a_real_server() {
            let Some(s) = settings("SSHVAULT_FTP_ADDR", FtpTls::None, CertCheck::default()) else {
                eprintln!("skipping: SSHVAULT_FTP_ADDR isn't set");
                return;
            };
            let (conn, seen) = FtpConn::open(s).await.unwrap();
            assert!(seen.is_none(), "no TLS, no certificate");
            let root = scratch(&conn, "browse").await;

            conn.mkdir(&root, "made").await.unwrap();
            conn.mkdir(&conn.join(&root, "made"), "inner").await.unwrap();
            let tmp = tempfile::TempDir::new().unwrap();
            let local = LocalBackend;
            std::fs::write(tmp.path().join("a b.txt"), b"hello").unwrap();
            let r = copy(&local, &conn, &[tmp.path().join("a b.txt").to_string_lossy().into_owned()], &root, false).await;
            assert!(matches!(r, TransferProgress::Done { files: 1, bytes: 5 }), "{r:?}");

            let list = conn.list(&root).await.unwrap();
            let names: Vec<(&str, bool)> = list.iter().map(|e| (e.name.as_str(), e.is_dir)).collect();
            assert_eq!(names, [("made", true), ("a b.txt", false)], "folders first");
            assert_eq!(list[1].size, 5);
            assert!(list[1].modified.is_some());
            assert_eq!(conn.stat(&conn.join(&root, "a b.txt")).await.unwrap(), Some(Stat { is_dir: false, size: 5 }));
            assert_eq!(conn.stat(&conn.join(&root, "made")).await.unwrap(), Some(Stat { is_dir: true, size: 0 }));
            assert_eq!(conn.stat(&conn.join(&root, "nope")).await.unwrap(), None);
            // Checking a folder doesn't move where later relative things happen: the home is still the home.
            assert_eq!(conn.home().await.unwrap(), conn.home().await.unwrap());

            conn.rename(&conn.join(&root, "a b.txt"), "c.txt").await.unwrap();
            assert!(conn.stat(&conn.join(&root, "c.txt")).await.unwrap().is_some());
            assert!(conn.stat(&conn.join(&root, "a b.txt")).await.unwrap().is_none());

            // Deleting a folder takes what is inside.
            conn.mkdir(&conn.join(&root, "made/inner"), "deep").await.unwrap();
            let r = copy(&local, &conn, &[tmp.path().join("a b.txt").to_string_lossy().into_owned()], &conn.join(&root, "made/inner/deep"), false).await;
            assert!(matches!(r, TransferProgress::Done { .. }), "{r:?}");
            conn.remove(&conn.join(&root, "made")).await.unwrap();
            assert!(conn.stat(&conn.join(&root, "made")).await.unwrap().is_none());
            assert!(conn.remove("/").await.is_err());
            assert!(conn.list(&conn.join(&root, "nowhere")).await.is_err());
            // A line break can't start a second command.
            assert!(conn.remove(&format!("{root}/c.txt\r\nDELE {root}/c.txt")).await.is_err());
            assert!(conn.mkdir(&root, "x\r\nRMD y").await.is_err());
            assert!(conn.stat(&conn.join(&root, "c.txt")).await.unwrap().is_some(), "nothing was deleted by a smuggled command");

            conn.remove(&root).await.unwrap();
            conn.close().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn trees_and_big_files_go_up_and_down_and_downloads_resume() {
            let Some(s) = settings("SSHVAULT_FTP_ADDR", FtpTls::None, CertCheck::default()) else { return };
            let (conn, _) = FtpConn::open(s).await.unwrap();
            let root = scratch(&conn, "tree").await;
            let tmp = tempfile::TempDir::new().unwrap();
            let local = LocalBackend;
            let up = tmp.path().join("up");
            std::fs::create_dir_all(up.join("nested/deeper")).unwrap();
            std::fs::write(up.join("a.txt"), b"alpha").unwrap();
            let big: Vec<u8> = (0..(256 * 1024 * 3 + 17)).map(|i| (i % 251) as u8).collect();
            std::fs::write(up.join("nested/big.bin"), &big).unwrap();
            std::fs::write(up.join("nested/deeper/empty.dat"), b"").unwrap();
            std::fs::write(up.join("héllo 東京 'q' $x.txt"), b"unicode").unwrap();

            let r = copy(&local, &conn, &[up.to_string_lossy().into_owned()], &root, false).await;
            assert!(matches!(r, TransferProgress::Done { files: 4, .. }), "{r:?}");
            let r = copy(&conn, &local, &[conn.join(&root, "up")], &tmp.path().to_string_lossy(), false).await;
            assert!(matches!(r, TransferProgress::Done { files: 4, .. }), "{r:?}");
            // `up` already exists locally, so look at the copy made under a new name.
            let down = tmp.path().join("down");
            std::fs::create_dir(&down).unwrap();
            let r = copy(&conn, &local, &[conn.join(&root, "up")], &down.to_string_lossy(), false).await;
            assert!(matches!(r, TransferProgress::Done { files: 4, .. }), "{r:?}");
            assert_eq!(std::fs::read(down.join("up/nested/big.bin")).unwrap(), big);
            assert_eq!(std::fs::read(down.join("up/héllo 東京 'q' $x.txt")).unwrap(), b"unicode");
            assert_eq!(std::fs::metadata(down.join("up/nested/deeper/empty.dat")).unwrap().len(), 0);

            // An interrupted download continues instead of starting over.
            let resumed = tmp.path().join("resumed");
            std::fs::create_dir_all(resumed.join("up/nested")).unwrap();
            std::fs::write(resumed.join("up/nested/big.bin"), &big[..300_000]).unwrap();
            let r = copy(&conn, &local, &[conn.join(&root, "up/nested/big.bin")], &resumed.join("up/nested").to_string_lossy(), true).await;
            assert!(matches!(r, TransferProgress::Done { files: 1, .. }), "{r:?}");
            assert_eq!(std::fs::read(resumed.join("up/nested/big.bin")).unwrap(), big, "the missing part was appended");
            // …and an interrupted upload continues too (APPE).
            let part = conn.join(&root, "part.bin");
            let big_file = tmp.path().join("big.bin");
            std::fs::write(&big_file, &big).unwrap();
            let r = copy(&local, &conn, &[big_file.to_string_lossy().into_owned()], &root, false).await;
            assert!(matches!(r, TransferProgress::Done { .. }));
            conn.rename(&conn.join(&root, "big.bin"), "part.bin").await.unwrap();
            // Truncate the remote copy to 100,000 bytes by re-uploading a prefix, then resume the rest.
            std::fs::write(tmp.path().join("part.bin"), &big[..100_000]).unwrap();
            let r = copy(&local, &conn, &[tmp.path().join("part.bin").to_string_lossy().into_owned()], &root, false).await;
            assert!(matches!(r, TransferProgress::Done { .. }));
            assert_eq!(conn.stat(&part).await.unwrap().unwrap().size, 100_000);
            std::fs::write(tmp.path().join("part.bin"), &big).unwrap();
            let r = copy(&local, &conn, &[tmp.path().join("part.bin").to_string_lossy().into_owned()], &root, true).await;
            assert!(matches!(r, TransferProgress::Done { .. }), "{r:?}");
            let again = tmp.path().join("again");
            std::fs::create_dir(&again).unwrap();
            copy(&conn, &local, std::slice::from_ref(&part), &again.to_string_lossy(), false).await;
            assert_eq!(std::fs::read(again.join("part.bin")).unwrap(), big, "the upload was continued, not corrupted");

            // Reading what isn't there is an error.
            let bad = copy(&conn, &local, &[conn.join(&root, "gone.bin")], &down.to_string_lossy(), false).await;
            assert!(matches!(bad, TransferProgress::Failed { .. }), "{bad:?}");
            conn.remove(&root).await.unwrap();
            conn.close().await;
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_wrong_password_is_refused_plainly() {
            let Some(mut s) = settings("SSHVAULT_FTP_ADDR", FtpTls::None, CertCheck::default()) else { return };
            s.password = "definitely-wrong".into();
            let err = FtpConn::open(s).await.err().expect("must fail").to_string();
            assert!(err.contains("user name or password"), "{err}");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_server_that_announces_a_private_address_still_works_through_nat() {
            let Some(s) = settings("SSHVAULT_FTP_NAT_ADDR", FtpTls::None, CertCheck::default()) else { return };
            // This server tells clients to connect to 10.255.255.1, which nothing answers.
            let (conn, _) = FtpConn::open(s).await.unwrap();
            let root = scratch(&conn, "nat").await;
            let tmp = tempfile::TempDir::new().unwrap();
            std::fs::write(tmp.path().join("n.txt"), b"through the NAT").unwrap();
            let local = LocalBackend;
            let list_first = tokio::time::timeout(Duration::from_secs(20), conn.list(&root)).await.expect("the listing hung on the unreachable address").unwrap();
            assert!(list_first.is_empty());
            let r = tokio::time::timeout(Duration::from_secs(20), copy(&local, &conn, &[tmp.path().join("n.txt").to_string_lossy().into_owned()], &root, false)).await.expect("the upload hung");
            assert!(matches!(r, TransferProgress::Done { files: 1, .. }), "{r:?}");
            let back = tmp.path().join("back");
            std::fs::create_dir(&back).unwrap();
            let r = tokio::time::timeout(Duration::from_secs(20), copy(&conn, &local, &[conn.join(&root, "n.txt")], &back.to_string_lossy(), false)).await.expect("the download hung");
            assert!(matches!(r, TransferProgress::Done { files: 1, .. }), "{r:?}");
            assert_eq!(std::fs::read(back.join("n.txt")).unwrap(), b"through the NAT");
            conn.remove(&root).await.unwrap();
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_tls_server_shows_its_certificate_and_it_is_pinned() {
            let Some(_) = settings("SSHVAULT_FTP_ADDR", FtpTls::Explicit, CertCheck::default()) else { return };
            // Nothing known: refused, with the certificate to look at. (Nothing was signed in.)
            let first = FtpConn::open(settings("SSHVAULT_FTP_ADDR", FtpTls::Explicit, CertCheck::default()).unwrap()).await;
            let FileError::UntrustedCertificate(info) = first.err().expect("an unknown certificate must stop the connection") else { panic!("expected an untrusted certificate") };
            assert_eq!(info.fingerprint.len(), 64);
            assert!(info.subject.contains("CN="), "{}", info.subject);

            // Accepted: connects over TLS, and says which certificate it was.
            let (conn, seen) = FtpConn::open(settings("SSHVAULT_FTP_ADDR", FtpTls::Explicit, CertCheck { pinned: None, accept: Some(info.fingerprint.clone()) }).unwrap()).await.unwrap();
            assert_eq!(seen.unwrap().fingerprint, info.fingerprint);
            // Everything works over the encrypted connection, data included.
            let root = scratch(&conn, "tls").await;
            let tmp = tempfile::TempDir::new().unwrap();
            let big: Vec<u8> = (0..700_000).map(|i| (i % 253) as u8).collect();
            std::fs::write(tmp.path().join("secret.bin"), &big).unwrap();
            let local = LocalBackend;
            let r = copy(&local, &conn, &[tmp.path().join("secret.bin").to_string_lossy().into_owned()], &root, false).await;
            assert!(matches!(r, TransferProgress::Done { files: 1, .. }), "{r:?}");
            assert_eq!(conn.list(&root).await.unwrap()[0].size, big.len() as u64);
            let back = tmp.path().join("back");
            std::fs::create_dir(&back).unwrap();
            let r = copy(&conn, &local, &[conn.join(&root, "secret.bin")], &back.to_string_lossy(), false).await;
            assert!(matches!(r, TransferProgress::Done { files: 1, .. }), "{r:?}");
            assert_eq!(std::fs::read(back.join("secret.bin")).unwrap(), big);
            conn.remove(&root).await.unwrap();
            conn.close().await;

            // Pinned: connects without asking. A different pin is refused, naming both.
            let (c2, _) = FtpConn::open(settings("SSHVAULT_FTP_ADDR", FtpTls::Explicit, CertCheck { pinned: Some(info.fingerprint.clone()), accept: None }).unwrap()).await.unwrap();
            c2.close().await;
            let changed = FtpConn::open(settings("SSHVAULT_FTP_ADDR", FtpTls::Explicit, CertCheck { pinned: Some("0".repeat(64)), accept: None }).unwrap()).await;
            match changed.err().expect("a different certificate must be refused") {
                FileError::CertificateChanged { expected, found } => assert_eq!((expected, found.fingerprint), ("0".repeat(64), info.fingerprint)),
                other => panic!("{other}"),
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn implicit_tls_on_a_second_server_with_listings_by_list() {
            // SSHVAULT_FTPS_ADDR: a ProFTPD with implicit TLS (a different server from the others). Its listings are
            // read through LIST here, as they must be for servers without MLSD.
            let Some(_) = settings("SSHVAULT_FTPS_ADDR", FtpTls::Implicit, CertCheck::default()) else { return };
            let first = within("the first connect", FtpConn::open(settings("SSHVAULT_FTPS_ADDR", FtpTls::Implicit, CertCheck::default()).unwrap())).await;
            let e = first.err().expect("unknown certificate"); let FileError::UntrustedCertificate(info) = e else { panic!("expected an untrusted certificate, got: {e}") };
            let (conn, seen) = within("the accepted connect", FtpConn::open(settings("SSHVAULT_FTPS_ADDR", FtpTls::Implicit, CertCheck { pinned: None, accept: Some(info.fingerprint.clone()) }).unwrap())).await.unwrap();
            assert_eq!(seen.unwrap().fingerprint, info.fingerprint);
            conn.without_mlsd().await;
            let root = within("scratch", scratch(&conn, "implicit")).await;
            let tmp = tempfile::TempDir::new().unwrap();
            let local = LocalBackend;
            std::fs::create_dir_all(tmp.path().join("t/sub")).unwrap();
            std::fs::write(tmp.path().join("t/one.txt"), b"1").unwrap();
            let big: Vec<u8> = (0..300_000).map(|i| (i % 249) as u8).collect();
            std::fs::write(tmp.path().join("t/sub/big.bin"), &big).unwrap();
            let r = within("the upload", copy(&local, &conn, &[tmp.path().join("t").to_string_lossy().into_owned()], &root, false)).await;
            assert!(matches!(r, TransferProgress::Done { files: 2, .. }), "{r:?}");
            let list = within("the listing", conn.list(&conn.join(&root, "t"))).await.unwrap();
            assert_eq!(list.iter().map(|e| (e.name.as_str(), e.is_dir)).collect::<Vec<_>>(), [("sub", true), ("one.txt", false)], "LIST lines are understood");
            assert_eq!(list[1].size, 1);
            let down = tmp.path().join("down");
            std::fs::create_dir(&down).unwrap();
            let r = within("the download", copy(&conn, &local, &[conn.join(&root, "t")], &down.to_string_lossy(), false)).await;
            assert!(matches!(r, TransferProgress::Done { files: 2, .. }), "{r:?}");
            assert_eq!(std::fs::read(down.join("t/sub/big.bin")).unwrap(), big);
            conn.remove(&root).await.unwrap();
            conn.close().await;
        }
    }
}
