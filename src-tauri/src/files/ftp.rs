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
/// Links in one folder checked for being folders.
const MAX_LINK_CHECKS: usize = 64;

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
        // A fact this doesn't understand never hides the file; only a line with no name is dropped.
        let Some((k, v)) = fact.split_once('=') else { continue };
        match k.to_ascii_lowercase().as_str() {
            "type" => kind = v.to_ascii_lowercase(),
            "size" => size = v.parse().unwrap_or(0),
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

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// Whitespace-separated fields with where each starts, so a name with spaces in it can be cut from the line as it was.
fn fields(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = None;
    for (i, c) in line.char_indices() {
        match (c.is_whitespace(), start) {
            (false, None) => start = Some(i),
            (true, Some(s)) => {
                out.push((s, &line[s..i]));
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push((s, &line[s..]));
    }
    out
}

/// The text after field `i`, without the one space that separates it from the name.
fn after<'a>(line: &'a str, f: &[(usize, &str)], i: usize) -> &'a str {
    let end = f[i].0 + f[i].1.len();
    let rest = &line[end..];
    rest.strip_prefix(' ').or_else(|| rest.strip_prefix('\t')).unwrap_or(rest)
}

/// `-rwxr-xr-x` (a file type first, perhaps a `+` after) as mode bits, setuid, setgid and sticky included.
fn mode_from_perms(p: &str) -> Option<u32> {
    let b: Vec<char> = p.chars().take(10).collect();
    if b.len() < 10 {
        return None;
    }
    let mut mode = 0u32;
    for (i, c) in b[1..].iter().enumerate() {
        let bit = 1u32 << (8 - i);
        let special = match i {
            2 => 0o4000,
            5 => 0o2000,
            8 => 0o1000,
            _ => 0,
        };
        match c {
            'r' | 'w' | 'x' => mode |= bit,
            's' | 't' => mode |= bit | special,
            'S' | 'T' => mode |= special,
            '-' | '?' | 'l' | 'L' => {}
            _ => return None,
        }
    }
    Some(mode)
}

fn month_number(s: &str) -> Option<u32> {
    let l = s.to_ascii_lowercase();
    MONTHS.iter().position(|m| l == *m).map(|i| i as u32 + 1)
}

/// `Jan  5 12:00` or `Jan  5  2025`, read against `now` (Unix seconds): a time without a year means the last
/// twelve months.
fn unix_date(month: u32, day: u32, time_or_year: &str, now: u64) -> Option<u64> {
    let (hh, mm) = match time_or_year.split_once(':') {
        Some((h, m)) => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?),
        None => (0, 0),
    };
    if !(1..=31).contains(&day) || hh > 23 || mm > 59 {
        return None;
    }
    let year = if time_or_year.contains(':') {
        let now_days = (now / 86_400) as i64;
        let this_year = civil_year(now_days);
        let candidate = super::days_from_civil(this_year, month, day) * 86_400 + hh * 3600 + mm * 60;
        // ls shows the time for dates in the past six months and the year for older ones; a time "in the future"
        // therefore belongs to last year.
        if candidate > now as i64 + 86_400 { this_year - 1 } else { this_year }
    } else {
        time_or_year.parse::<i64>().ok().filter(|y| (1900..=9999).contains(y))?
    };
    u64::try_from(super::days_from_civil(year, month, day) * 86_400 + hh * 3600 + mm * 60).ok()
}

/// The civil year of a day count since 1970-01-01.
fn civil_year(days: i64) -> i64 {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    yoe + era * 400 + i64::from(m <= 2)
}

/// One line of a `LIST` reply: Unix `ls -l` (with ACL marks, no group column, ISO dates) or the Windows/IIS style.
/// `Ok(None)` for lines that are no entry (`total`, `.` and `..`); `Err` for a line in a format this can't read.
fn list_line(line: &str, now: u64) -> Result<Option<FtpEntry>, ()> {
    let line = line.trim_end_matches(['\r', '\n']);
    let f = fields(line);
    let Some(first) = f.first().map(|f| f.1) else { return Ok(None) };
    if first == "total" {
        return Ok(None);
    }
    let unix = first.len() >= 10 && first.chars().next().is_some_and(|c| "-dlbcpsD".contains(c)) && first.chars().skip(1).take(9).all(|c| "rwxsStTlL-?".contains(c));
    let entry = if unix { parse_unix(line, &f, now) } else { parse_dos(line, &f) }.ok_or(())?;
    Ok((entry.name != "." && entry.name != ".." && !entry.name.is_empty()).then_some(entry))
}

/// `2025-01-05 12:00[:30]` (some servers, and `ls --time-style=long-iso`) as Unix seconds.
fn iso_date(date: &str, time: &str) -> Option<u64> {
    let d: Vec<&str> = date.split('-').collect();
    if d.len() != 3 || d[0].len() != 4 {
        return None;
    }
    let (y, mo, da) = (d[0].parse::<i64>().ok()?, d[1].parse::<u32>().ok()?, d[2].parse::<u32>().ok()?);
    let mut t = time.split(':');
    let (h, m) = (t.next()?.parse::<i64>().ok()?, t.next()?.parse::<i64>().ok()?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&da) || h > 23 || m > 59 {
        return None;
    }
    u64::try_from(super::days_from_civil(y, mo, da) * 86_400 + h * 3600 + m * 60).ok()
}

fn parse_unix(line: &str, f: &[(usize, &str)], now: u64) -> Option<FtpEntry> {
    let perms = f[0].1;
    // The size is the number just before the date; the owner and group (the group is missing on some servers)
    // come before it. `found` is where the size is, the date, and which field the name follows.
    let mut found = None;
    for i in 2..f.len() {
        if !f[i - 1].1.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if let (Some(mo), Some(day), Some(t)) = (month_number(f[i].1), f.get(i + 1).and_then(|d| d.1.parse::<u32>().ok()), f.get(i + 2)) {
            found = Some((i, unix_date(mo, day, t.1, now), i + 2));
            break;
        }
        if let Some(ts) = f.get(i + 1).and_then(|t| iso_date(f[i].1, t.1)) {
            found = Some((i, Some(ts), i + 1));
            break;
        }
    }
    let (i, modified, name_from) = found?;
    let size: u64 = f[i - 1].1.parse().unwrap_or(0);
    let rest = after(line, f, name_from);
    let kind = perms.chars().next()?;
    let is_symlink = kind == 'l';
    let name = if is_symlink { rest.split_once(" -> ").map_or(rest, |(n, _)| n) } else { rest };
    let is_dir = kind == 'd';
    Some(FtpEntry { name: name.to_string(), is_dir, is_symlink, size: if is_dir { 0 } else { size }, modified, mode: mode_from_perms(perms) })
}

/// `01-05-25  12:00PM  <DIR>  name` or `01-05-2025  12:00PM  1234  name`.
fn parse_dos(line: &str, f: &[(usize, &str)]) -> Option<FtpEntry> {
    if f.len() < 4 {
        return None;
    }
    let d: Vec<&str> = f[0].1.split(['-', '/']).collect();
    if d.len() != 3 || !d.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        return None;
    }
    let (mo, da, mut y) = (d[0].parse::<u32>().ok()?, d[1].parse::<u32>().ok()?, d[2].parse::<i64>().ok()?);
    if d[2].len() <= 2 {
        y += if y < 70 { 2000 } else { 1900 };
    }
    let t = f[1].1.to_ascii_uppercase();
    let (clock, pm) = match t.strip_suffix("PM") {
        Some(c) => (c.to_string(), Some(true)),
        None => match t.strip_suffix("AM") {
            Some(c) => (c.to_string(), Some(false)),
            None => (t.clone(), None),
        },
    };
    let (mut h, m) = clock.split_once(':').and_then(|(h, m)| Some((h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)))?;
    match pm {
        Some(true) if h < 12 => h += 12,
        Some(false) if h == 12 => h = 0,
        _ => {}
    }
    let modified = if (1..=12).contains(&mo) && (1..=31).contains(&da) && h < 24 && m < 60 { u64::try_from(super::days_from_civil(y, mo, da) * 86_400 + h * 3600 + m * 60).ok() } else { None };
    let (is_dir, size) = if f[2].1.eq_ignore_ascii_case("<DIR>") {
        (true, 0)
    } else {
        (false, f[2].1.replace(',', "").parse::<u64>().ok()?)
    };
    // IIS pads the columns with runs of spaces.
    Some(FtpEntry { name: after(line, f, 2).trim_start().to_string(), is_dir, is_symlink: false, size, modified, mode: None })
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

fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn is_mlsd_marker(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("type=cdir") || l.contains("type=pdir")
}

/// A reply with lines but not one entry in it is a format this can't read. Say so: showing it as an empty folder
/// would be wrong, and the files are right there.
fn unreadable(what: &str, line: &str) -> FileError {
    FileError::Backend(format!("couldn't read this server's {what} listing, which looks like: {}", line.trim()))
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
                    let entries: Vec<FtpEntry> = lines.iter().filter_map(|l| parse_mlsd(l)).collect();
                    if entries.is_empty() {
                        if let Some(bad) = lines.iter().find(|l| !l.trim().is_empty() && !is_mlsd_marker(l)) {
                            return Err(unreadable("MLSD", bad));
                        }
                    }
                    return Ok(entries);
                }
                // A server without MLSD says so with 500, 501 or 502; anything else is a real problem.
                Err(FtpError::UnexpectedResponse(r)) if matches!(r.status as u32, 500..=502) => c.mlsd = Some(false),
                Err(e) => return Err(ftp_err(e)),
            }
        }
        let lines = c.ftp.list(Some(dir)).await.map_err(ftp_err)?;
        let now = unix_now();
        let mut entries = Vec::new();
        let mut bad = None;
        for l in &lines {
            match list_line(l, now) {
                Ok(Some(e)) => entries.push(e),
                Ok(None) => {}
                Err(()) => bad = bad.or(Some(l)),
            }
        }
        if let (true, Some(bad)) = (entries.is_empty(), bad) {
            return Err(unreadable("LIST", bad));
        }
        Ok(entries)
    }

    /// Whether `path` can be gone into, for links (a listing says only that it is a link). Back to where signing in
    /// landed afterwards.
    async fn link_is_dir(&self, path: &str) -> bool {
        let Ok(mut g) = self.ctl().await else { return false };
        let c = ctl_of(&mut g);
        if c.ftp.cwd(path).await.is_err() {
            return false;
        }
        let home = c.home.clone();
        let _ = c.ftp.cwd(&home).await;
        true
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
        let mut entries = self.entries(dir).await?;
        // A link to a folder has to open like one. Only so many are checked, a round trip each.
        for e in entries.iter_mut().filter(|e| e.is_symlink && !e.is_dir).take(MAX_LINK_CHECKS) {
            e.is_dir = self.link_is_dir(&self.join(dir, &e.name)).await;
            if e.is_dir {
                e.size = 0;
            }
        }
        let mut out: Vec<FileEntry> = entries
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
                    // Some servers list a link to a folder as a folder. Deleting it as a file removes the link and
                    // nothing it points at, and a real folder refuses, so only a real one is gone into.
                    let was_link = ctl_of(&mut self.ctl().await?).ftp.rm(&child).await.is_ok();
                    if !was_link {
                        stack.push((child, false));
                    }
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
        assert_eq!(parse_mlsd("nofacts"), None);
        // A bad date doesn't hide the file, it just has no date.
        assert_eq!(parse_mlsd("type=file;size=1;modify=garbage; x").unwrap().modified, None);
        assert_eq!(parse_mlsd("type=file;size=1;modify=20261332250000; x").unwrap().modified, None);
    }

    // 2026-10-16 12:00 UTC
    const NOW: u64 = 1_792_152_000;

    fn parsed(line: &str) -> FtpEntry {
        list_line(line, NOW).unwrap().unwrap_or_else(|| panic!("skipped: {line}"))
    }

    #[test]
    fn unix_list_lines_are_the_fallback() {
        let f = parsed("-rw-r--r--    1 ftp      ftp          1234 Oct 06 04:28 hello.txt");
        assert_eq!((f.name.as_str(), f.size, f.is_dir, f.mode), ("hello.txt", 1234, false, Some(0o644)));
        let d = parsed("drwxr-xr-x    2 ftp      ftp          4096 Oct 06  2025 docs");
        assert!(d.is_dir && d.size == 0 && d.mode == Some(0o755));
        assert_eq!(list_line("total 12", NOW), Ok(None));
        assert_eq!(list_line("drwxr-xr-x 2 ftp ftp 4096 Oct 06 2025 .", NOW), Ok(None));
        assert_eq!(list_line("drwxr-xr-x 2 ftp ftp 4096 Oct 06 2025 ..", NOW), Ok(None));
        assert_eq!(list_line("", NOW), Ok(None));
    }

    #[test]
    fn list_lines_from_other_servers_are_read() {
        // Names keep their spaces, even runs of them.
        assert_eq!(parsed("-rw-r--r-- 1 user group 12 Jan  5 12:00 two  spaces.txt").name, "two  spaces.txt");
        assert_eq!(parsed("drwxr-xr-x 2 u g 4096 Jan 5 12:00 dir with space").name, "dir with space");
        assert_eq!(parsed("-rw-r--r-- 1 u g 12 Jan  5 2025 año 東京.txt").name, "año 東京.txt");
        // An ACL or SELinux mark after the permissions.
        let acl = parsed("-rw-r--r--+ 1 u g 12 Jan  5 2025 acl.txt");
        assert_eq!((acl.name.as_str(), acl.size), ("acl.txt", 12));
        // No group column.
        assert_eq!(parsed("-rw-r--r-- 1 owner 12 Jan  5 2025 nogroup.txt").name, "nogroup.txt");
        // ISO dates.
        let iso = parsed("-rw-r--r-- 1 u g 12 2025-01-05 12:00 isodate.txt");
        assert_eq!((iso.name.as_str(), iso.size, iso.modified), ("isodate.txt", 12, Some(1_736_078_400)));
        // A link is a link, named without its target.
        let l = parsed("lrwxrwxrwx 1 u g 4 Jan 5 12:00 link -> some/target");
        assert!(l.is_symlink && !l.is_dir && l.name == "link");
        // setuid, setgid and sticky.
        assert_eq!(parsed("-rwsr-sr-t 1 u g 1 Jan 5 2025 x").mode, Some(0o7755));
        assert_eq!(parsed("-rwSr-Sr-T 1 u g 1 Jan 5 2025 x").mode, Some(0o7644));
        // Windows / IIS.
        let d = parsed("01-05-25  12:00PM       <DIR>          iis dir");
        assert!(d.is_dir && d.name == "iis dir", "{d:?}");
        assert_eq!(d.modified, Some(1_736_078_400));
        let f = parsed("01-05-2025  12:00AM                1,234 iis file.txt");
        assert_eq!((f.name.as_str(), f.size, f.is_dir, f.modified), ("iis file.txt", 1234, false, Some(1_736_035_200)));
        assert_eq!(parsed("12-31-98  11:59PM       <DIR>          old").modified, Some(915_148_740));
    }

    #[test]
    fn list_dates_without_a_year_mean_the_last_twelve_months() {
        // Today is 2026-10-16: "Oct 20" can't be next week, so it was last year; "Mar 1" was this year.
        let a = parsed("-rw-r--r-- 1 u g 1 Oct 20 10:00 a");
        let b = parsed("-rw-r--r-- 1 u g 1 Mar  1 10:00 b");
        assert_eq!(a.modified, Some(1_760_954_400), "2025-10-20 10:00");
        assert_eq!(b.modified, Some(1_772_359_200), "2026-03-01 10:00");
    }

    #[test]
    fn a_listing_in_an_unknown_format_is_an_error_not_an_empty_folder() {
        assert!(list_line("this is not a listing line at all", NOW).is_err());
        assert!(list_line("-rw-r--r-- 1 u g notasize Jan 5 2025 x", NOW).is_err());
    }

    #[test]
    fn mlsd_facts_this_does_not_understand_do_not_hide_files() {
        assert_eq!(parse_mlsd("type=file;size=;modify=20260101000000; empty size").unwrap().name, "empty size");
        assert_eq!(parse_mlsd("type=file;weird;size=3; odd fact").unwrap().size, 3);
        assert!(is_mlsd_marker("type=cdir;modify=20260101000000; ."));
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

        #[cfg(unix)]
        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn listings_by_list_show_everything_and_a_link_to_a_folder_opens() {
            // SSHVAULT_FTP_ROOT: the folder the server at SSHVAULT_FTP_ADDR serves (its home), to make a link in.
            let (Some(s), Some(root_dir)) = (settings("SSHVAULT_FTP_ADDR", FtpTls::None, CertCheck::default()), env("SSHVAULT_FTP_ROOT")) else { return };
            let (conn, _) = FtpConn::open(s).await.unwrap();
            for use_list in [false, true] {
                if use_list {
                    conn.without_mlsd().await;
                }
                let root = scratch(&conn, if use_list { "links-list" } else { "links-mlsd" }).await;
                let real = std::path::Path::new(&root_dir).join(root.trim_start_matches('/'));
                std::fs::create_dir(real.join("target dir")).unwrap();
                std::fs::write(real.join("target dir/inside.txt"), b"x").unwrap();
                std::os::unix::fs::symlink("target dir", real.join("a link")).unwrap();
                std::os::unix::fs::symlink("target dir/inside.txt", real.join("file link")).unwrap();
                for i in 0..150 {
                    std::fs::write(real.join(format!("f{i:03} with  spaces.txt")), b"1").unwrap();
                }
                let list = within("the listing", conn.list(&root)).await.unwrap();
                assert_eq!(list.len(), 153, "use_list={use_list}: every file and folder is listed");
                let link = list.iter().find(|e| e.name == "a link").unwrap();
                // pyftpdlib's MLSD follows links itself; LIST says "l" and the folder check has to find out.
                assert!(link.is_dir && (!use_list || link.is_symlink), "use_list={use_list}: a link to a folder opens like a folder: {link:?}");
                let flink = list.iter().find(|e| e.name == "file link").unwrap();
                assert!(!flink.is_dir && (!use_list || flink.is_symlink), "use_list={use_list}: {flink:?}");
                assert!(list.iter().any(|e| e.name == "f149 with  spaces.txt"));
                let inside = within("the listing through the link", conn.list(&conn.join(&root, "a link"))).await.unwrap();
                assert_eq!(inside.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["inside.txt"]);
                conn.remove(&root).await.unwrap();
            }
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
