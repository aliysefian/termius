//! Import SSH bookmarks from MobaXterm (`MobaXterm.ini`, or an exported
//! `.mxtsessions` file). Each `[Bookmarks…]` section is a folder
//! (`SubRep=Parent\Child`); each SSH session is a line such as
//!
//! ```text
//! web-01=#109#0%10.0.0.5%22%ops%%-1%-1%%%%%0%0%0%_ProfileDir_\.ssh\id.ppk%…
//! ```
//!
//! where `109` means SSH and the `%` fields start with host, port and user.
//!
//! Passwords are not on those lines. MobaXterm keeps them in `MobaXterm.ini` under `[Passwords]` (one per
//! `ssh22:user@host`) and `[Credentials]` (named user-and-password pairs), encrypted with a key made from the master
//! password: AES-256 in CFB mode on single bytes, the key the first 32 bytes of SHA-512 of the master password, and
//! the IV that key's encryption of a block of zeros. Without a master password MobaXterm uses the Windows user name
//! and computer name as `user@computer`. An exported `.mxtsessions` file holds no passwords at all. See [`Secrets`].

use aes::cipher::{Block, BlockCipherEncrypt, KeyInit};
use aes::Aes256;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Digest, Sha512};
use zeroize::Zeroizing;

use crate::interchange::{ExportHost, HostFormat, ImportContext};
use crate::sshconfig::{ImportedHost, ImportedSecret};

const SSH: &str = "109";

pub use crate::interchange::Parsed;

/// MobaXterm writes some characters in names as `__PIPE__`, `__DIEZE__`…
fn unescape(s: &str) -> String {
    s.replace("__PIPE__", "|")
        .replace("__DIEZE__", "#")
        .replace("__PTVIRG__", ";")
        .replace("__DBLQUO__", "\"")
        .replace("__APOS__", "'")
        .replace("__PERCENT__", "%")
}

/// A private-key path, if one of the fields looks like one we can use.
/// MobaXterm's `_ProfileDir_` / `_CurrentDrive_` placeholders are expanded
/// when `profile_dir` is known.
fn key_path(fields: &[&str], profile_dir: Option<&str>) -> Option<String> {
    fields.iter().find_map(|f| {
        let f = f.trim();
        let looks = f.ends_with(".ppk") || f.ends_with(".pem") || f.ends_with(".key") || f.contains("id_");
        if !looks || f.len() < 4 {
            return None;
        }
        let expanded = match (f.strip_prefix("_ProfileDir_"), profile_dir) {
            (Some(rest), Some(dir)) => format!("{dir}{rest}"),
            (Some(_), None) => return None,
            (None, _) => f.to_string(),
        };
        (!expanded.contains("_CurrentDrive_")).then_some(expanded)
    })
}

pub fn parse(text: &str, profile_dir: Option<&str>) -> Parsed {
    let mut out = Parsed::default();
    let mut in_bookmarks = false;
    let mut folder = String::new();
    let mut skipped_other = 0usize;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_bookmarks = line[1..line.len() - 1].starts_with("Bookmarks");
            folder.clear();
            continue;
        }
        if !in_bookmarks || line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        match key {
            "SubRep" => {
                folder = value.replace('\\', "/").trim_matches('/').to_string();
                continue;
            }
            "ImgNum" => continue,
            _ => {}
        }
        // "#109#0%host%port%user%…"
        let Some(rest) = value.strip_prefix('#') else { continue };
        let Some((kind, params)) = rest.split_once('#') else { continue };
        if kind != SSH {
            skipped_other += 1;
            continue;
        }
        let fields: Vec<&str> = params.split('%').collect();
        let host = fields.get(1).map(|s| s.trim()).unwrap_or_default();
        if host.is_empty() {
            out.warnings.push(format!("{}: no host name; skipped", unescape(key)));
            continue;
        }
        let port = fields.get(2).and_then(|p| p.trim().parse::<u16>().ok()).filter(|p| *p > 0).unwrap_or(22);
        let user = fields.get(3).map(|s| s.trim()).filter(|s| !s.is_empty()).map(str::to_string);
        out.hosts.push(ImportedHost {
            alias: unescape(key.trim()),
            hostname: host.to_string(),
            port,
            user,
            identity_file: key_path(&fields[4.min(fields.len())..], profile_dir),
            group: (!folder.is_empty()).then(|| folder.clone()),
            ..Default::default()
        });
    }
    if skipped_other > 0 {
        out.warnings.push(format!(
            "{skipped_other} non-SSH bookmark(s) (RDP, VNC, serial, …) were skipped"
        ));
    }
    out
}

/// The inverse of [`unescape`], and `=` / line breaks, which would end the name early.
fn escape(s: &str) -> String {
    s.replace('%', "__PERCENT__")
        .replace('|', "__PIPE__")
        .replace('#', "__DIEZE__")
        .replace(';', "__PTVIRG__")
        .replace('"', "__DBLQUO__")
        .replace('\'', "__APOS__")
        .replace(['=', '\r', '\n'], "-")
}

/// A value inside a `%`-separated field list can't hold `%` or a line break.
fn field(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '%' | '#' | '\r' | '\n')).collect()
}

/// The bookmark file MobaXterm reads with Import / "Sessions > Import sessions from file". One `[Bookmarks_N]`
/// section per folder; the top level is `[Bookmarks]`. Only what the format can hold is written: no passwords, and
/// terminal settings are MobaXterm's defaults.
pub fn export(hosts: &[ExportHost]) -> String {
    use std::collections::BTreeMap;
    use std::fmt::Write as _;
    // BTreeMap keeps folders in a stable order; "" sorts first, which is the top level.
    let mut folders: BTreeMap<String, Vec<&ExportHost>> = BTreeMap::new();
    folders.entry(String::new()).or_default();
    for h in hosts {
        folders.entry(h.group.replace('/', "\\").trim_matches('\\').to_string()).or_default().push(h);
    }
    let mut out = String::new();
    for (n, (folder, members)) in folders.iter().enumerate() {
        let _ = write!(out, "[Bookmarks{}]\r\nSubRep={folder}\r\nImgNum={}\r\n", if n == 0 { String::new() } else { format!("_{n}") }, if n == 0 { 42 } else { 41 });
        for h in members {
            let port = if h.port == 0 { 22 } else { h.port };
            let key = h.key_file.as_deref().map(field).unwrap_or_default();
            let _ = write!(
                out,
                "{}=#{SSH}#0%{}%{port}%{}%%-1%-1%%%%%0%0%0%{key}%%-1%0%0%0%%1080%%0%0%1%%0%%%%%0#MobaFont%10%0%0%-1%15%236,236,236%30,30,30%180,180,192%0%-1%0%%xterm%-1%-1%_Std_Colors_0_%80%24%0%1%-1%<none>%%0%1%-1#0# #-1\r\n",
                escape(h.name.trim()),
                field(h.hostname.trim()),
                field(h.user.as_deref().unwrap_or_default()),
            );
        }
        out.push_str("\r\n");
    }
    out
}

// -- the password store ------------------------------------------------------------------------------------------

/// One line of `[Passwords]`: `ssh22:user@host=…`, or just `user@host=…`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PasswordEntry {
    protocol: String,
    port: Option<u16>,
    user: String,
    host: String,
    cipher: String,
}

/// One line of `[Credentials]`: `Name=user:…`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CredentialEntry {
    name: String,
    user: String,
    cipher: String,
}

/// The encrypted passwords in a `MobaXterm.ini`. Nothing here is decrypted until [`attach_passwords`] is given a key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    passwords: Vec<PasswordEntry>,
    credentials: Vec<CredentialEntry>,
}

/// What was found for one host: the user name the password belongs to, and the ciphertext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub user: String,
    cipher: String,
}

impl Secrets {
    pub fn parse(text: &str) -> Secrets {
        let mut out = Secrets::default();
        let mut section = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.starts_with('[') && line.ends_with(']') {
                section = line[1..line.len() - 1].to_ascii_lowercase();
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let (key, value) = (unescape(key.trim()), value.trim());
            if value.is_empty() {
                continue;
            }
            match section.as_str() {
                "passwords" => {
                    // `ssh22:user@host`; the part before the colon is optional.
                    let (proto_port, who) = match key.split_once(':') {
                        Some((p, w)) if w.contains('@') => (p, w),
                        _ => ("", key.as_str()),
                    };
                    let Some((user, host)) = who.rsplit_once('@') else { continue };
                    if host.is_empty() {
                        continue;
                    }
                    let digits_at = proto_port.find(|c: char| c.is_ascii_digit()).unwrap_or(proto_port.len());
                    out.passwords.push(PasswordEntry {
                        protocol: proto_port[..digits_at].to_ascii_lowercase(),
                        port: proto_port[digits_at..].parse().ok(),
                        user: user.to_string(),
                        host: host.to_string(),
                        cipher: value.to_string(),
                    });
                }
                "credentials" => {
                    let Some((user, cipher)) = value.split_once(':') else { continue };
                    if !cipher.is_empty() {
                        out.credentials.push(CredentialEntry { name: key, user: user.to_string(), cipher: cipher.to_string() });
                    }
                }
                _ => {}
            }
        }
        out
    }

    pub fn is_empty(&self) -> bool {
        self.passwords.is_empty() && self.credentials.is_empty()
    }

    pub fn len(&self) -> usize {
        self.passwords.len() + self.credentials.len()
    }

    /// The saved password for a host, if there is one. A session's user name that is the name of a saved credential
    /// means that credential; otherwise the `[Passwords]` line for the same host (and user, when the session names
    /// one), preferring the one for SSH on the session's port.
    pub fn find(&self, host: &ImportedHost) -> Option<Found> {
        if let Some(user) = host.user.as_deref() {
            let name = user.trim().trim_matches(['<', '>']);
            if let Some(c) = self.credentials.iter().find(|c| c.name.eq_ignore_ascii_case(name)) {
                return Some(Found { user: c.user.clone(), cipher: c.cipher.clone() });
            }
        }
        let mut best: Option<(u8, &PasswordEntry)> = None;
        for e in &self.passwords {
            if !e.host.eq_ignore_ascii_case(host.hostname.trim()) {
                continue;
            }
            let user_rank = match host.user.as_deref() {
                Some(u) if u == e.user => 2,
                Some(u) if u.eq_ignore_ascii_case(&e.user) => 1,
                Some(_) => continue,
                None => 0,
            };
            let port_rank = match e.port {
                Some(p) if p == host.port => 2,
                None => 1,
                Some(_) => 0,
            };
            let proto_rank = u8::from(e.protocol.is_empty() || e.protocol == "ssh");
            let rank = user_rank * 16 + port_rank * 4 + proto_rank;
            if best.is_none_or(|(r, _)| rank > r) {
                best = Some((rank, e));
            }
        }
        best.map(|(_, e)| Found { user: e.user.clone(), cipher: e.cipher.clone() })
    }
}

/// Say on each host whether a password is saved for it, without decrypting anything.
pub fn mark_passwords(hosts: &mut [ImportedHost], secrets: &Secrets) {
    for h in hosts {
        h.has_password = secrets.find(h).is_some();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecryptError {
    /// The key does not turn the saved passwords into text.
    WrongKey,
    /// A saved value is not base64 of a sensible length.
    NotEncrypted,
}

impl std::fmt::Display for DecryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecryptError::WrongKey => write!(f, "the master password does not decrypt the saved passwords"),
            DecryptError::NotEncrypted => write!(f, "a saved password is not in a form this app can read"),
        }
    }
}

/// The AES key for a master password: the first 32 bytes of its SHA-512.
pub fn derive_key(master: &str) -> Zeroizing<[u8; 32]> {
    let digest = Sha512::digest(master.as_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&digest[..32]);
    key
}

fn aes_block(cipher: &Aes256, input: &[u8; 16]) -> [u8; 16] {
    let mut b = Block::<Aes256>::try_from(&input[..]).expect("an AES block is sixteen bytes");
    cipher.encrypt_block(&mut b);
    let mut out = [0u8; 16];
    out.copy_from_slice(&b);
    out
}

/// CFB on single bytes: each byte is XORed with the first byte of the encrypted shift register, and the register then
/// takes the ciphertext byte in. The same loop encrypts and decrypts; only which byte is fed back differs.
fn cfb8(key: &[u8; 32], input: &[u8], decrypt: bool) -> Zeroizing<Vec<u8>> {
    let cipher = Aes256::new_from_slice(key).expect("an AES-256 key is thirty-two bytes");
    let mut shift = aes_block(&cipher, &[0u8; 16]);
    let mut out = Zeroizing::new(Vec::with_capacity(input.len()));
    for &b in input {
        let k = aes_block(&cipher, &shift)[0];
        let o = b ^ k;
        out.push(o);
        shift.copy_within(1.., 0);
        shift[15] = if decrypt { b } else { o };
    }
    out
}

/// A saved value as text. A wrong key gives bytes that are not text, which is how it is told apart.
pub fn decrypt(cipher_b64: &str, key: &[u8; 32]) -> Result<ImportedSecret, DecryptError> {
    let data = B64.decode(cipher_b64.trim()).map_err(|_| DecryptError::NotEncrypted)?;
    if data.is_empty() || data.len() > 1024 {
        return Err(DecryptError::NotEncrypted);
    }
    let plain = cfb8(key, &data, true);
    let text = std::str::from_utf8(&plain).map_err(|_| DecryptError::WrongKey)?;
    if text.chars().any(char::is_control) {
        return Err(DecryptError::WrongKey);
    }
    Ok(ImportedSecret(Zeroizing::new(text.to_string())))
}

/// What MobaXterm writes for a password; for the tests, and to show the scheme is the same both ways.
pub fn encrypt(plain: &str, key: &[u8; 32]) -> String {
    B64.encode(cfb8(key, plain.as_bytes(), false).as_slice())
}

/// Decrypt the saved password of every host that has one and put it on the host (and the user name it belongs to,
/// when the session named none). Returns how many. Nothing is attached unless every password decrypts: a key that
/// is wrong for one is wrong for all.
pub fn attach_passwords(hosts: &mut [ImportedHost], secrets: &Secrets, key: &[u8; 32]) -> Result<usize, DecryptError> {
    let mut found: Vec<(usize, Found, ImportedSecret)> = Vec::new();
    for (i, h) in hosts.iter().enumerate() {
        if let Some(f) = secrets.find(h) {
            let secret = decrypt(&f.cipher, key)?;
            found.push((i, f, secret));
        }
    }
    let n = found.len();
    for (i, f, secret) in found {
        let h = &mut hosts[i];
        if h.user.is_none() {
            h.user = Some(f.user);
        }
        h.password = Some(secret);
        h.has_password = true;
    }
    Ok(n)
}

/// What MobaXterm uses as the master password when none was set: the Windows user and computer names.
pub fn default_master() -> Option<String> {
    let user = std::env::var("USERNAME").ok().filter(|u| !u.is_empty())?;
    let computer = std::env::var("COMPUTERNAME").ok().filter(|c| !c.is_empty())?;
    Some(format!("{user}@{computer}"))
}

pub struct MobaXterm;

impl HostFormat for MobaXterm {
    fn id(&self) -> &'static str {
        "mobaxterm"
    }
    fn name(&self) -> &'static str {
        "MobaXterm"
    }
    fn extensions(&self) -> &'static [&'static str] {
        &["mxtsessions", "ini"]
    }
    fn parse(&self, text: &str, ctx: &ImportContext) -> Parsed {
        parse(text, ctx.profile_dir.as_deref())
    }
    fn export(&self, hosts: &[ExportHost]) -> String {
        export(hosts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = r#"
[Misc]
Foo=bar

[Bookmarks]
SubRep=
ImgNum=42
web-01=#109#0%10.0.0.5%22%ops%%-1%-1%%%%%0%0%0%_ProfileDir_\.ssh\id_ed25519%%-1%0%0%0%%1080%%0%0%1
desk=#91#4%192.168.1.9%3389%me%0%-1%-1

[Bookmarks_1]
SubRep=Production\Databases
ImgNum=41
db__PIPE__primary=#109#0%db.example.com%2222%%%-1%-1%%%%%0%0%0%%%-1
broken=#109#0%%22%x
"#;

    #[test]
    fn ssh_bookmarks_with_folders_ports_users_and_keys() {
        let p = parse(INI, Some("C:\\Users\\me"));
        assert_eq!(p.hosts.len(), 2, "{p:?}");
        let web = &p.hosts[0];
        assert_eq!((web.alias.as_str(), web.hostname.as_str(), web.port), ("web-01", "10.0.0.5", 22));
        assert_eq!(web.user.as_deref(), Some("ops"));
        assert_eq!(web.identity_file.as_deref(), Some("C:\\Users\\me\\.ssh\\id_ed25519"));
        assert_eq!(web.group, None);
        let db = &p.hosts[1];
        assert_eq!((db.alias.as_str(), db.port), ("db|primary", 2222));
        assert_eq!(db.user, None);
        assert_eq!(db.group.as_deref(), Some("Production/Databases"));
        assert!(p.warnings.iter().any(|w| w.contains("non-SSH")), "{:?}", p.warnings);
        assert!(p.warnings.iter().any(|w| w.contains("broken")));
    }

    #[test]
    fn placeholders_without_a_profile_dir_are_dropped() {
        let p = parse(INI, None);
        assert_eq!(p.hosts[0].identity_file, None);
    }
    #[test]
    fn export_round_trips_through_the_parser() {
        let hosts = vec![
            ExportHost { name: "web-01".into(), hostname: "10.0.0.5".into(), port: 22, user: Some("ops".into()), ..Default::default() },
            ExportHost { name: "db|primary".into(), hostname: "db.example.com".into(), port: 2222, group: "Production/Databases".into(), key_file: Some("/home/me/.ssh/id_ed25519".into()), ..Default::default() },
            ExportHost { name: "no-port".into(), hostname: "h".into(), port: 0, group: "Production/Databases".into(), ..Default::default() },
        ];
        let p = parse(&export(&hosts), None);
        assert_eq!(p.hosts.len(), 3, "{p:?}");
        assert_eq!(p.warnings, Vec::<String>::new());
        let by = |n: &str| p.hosts.iter().find(|h| h.alias == n).unwrap();
        assert_eq!((by("web-01").port, by("web-01").user.as_deref(), by("web-01").group.clone()), (22, Some("ops"), None));
        let db = by("db|primary");
        assert_eq!((db.hostname.as_str(), db.port, db.group.as_deref()), ("db.example.com", 2222, Some("Production/Databases")));
        assert_eq!(db.identity_file.as_deref(), Some("/home/me/.ssh/id_ed25519"));
        assert_eq!(by("no-port").port, 22);
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_first_section() {
        let f = crate::interchange::find("mobaxterm").unwrap();
        let p = crate::interchange::parse(f, &format!("\u{feff}{INI}"), &ImportContext::default());
        assert_eq!(p.hosts.len(), 2);
    }

    // -- passwords --------------------------------------------------------------------------------------------------

    fn host(alias: &str, hostname: &str, port: u16, user: Option<&str>) -> ImportedHost {
        ImportedHost { alias: alias.into(), hostname: hostname.into(), port, user: user.map(str::to_string), ..Default::default() }
    }

    #[test]
    fn the_cipher_round_trips_and_a_wrong_key_is_noticed() {
        let key = derive_key("my master password");
        for plain in ["hunter2", "p@ss wörd ✓", "x", &"long".repeat(50)] {
            let c = encrypt(plain, &key);
            assert_eq!(decrypt(&c, &key).unwrap().0.as_str(), plain);
        }
        let c = encrypt("correct horse battery staple", &key);
        assert_eq!(decrypt(&c, &derive_key("another")), Err(DecryptError::WrongKey));
        assert_eq!(decrypt("not base64!!", &key), Err(DecryptError::NotEncrypted));
        assert_eq!(decrypt("", &key), Err(DecryptError::NotEncrypted));
        // The key is the first half of SHA-512, so different passwords give different keys.
        assert_ne!(*derive_key("a"), *derive_key("b"));
        assert_eq!(derive_key("a").len(), 32);
    }

    #[test]
    fn the_password_store_is_read_and_matched_to_sessions() {
        let key = derive_key("m");
        let ini = format!(
            "[Bookmarks]\nSubRep=\nweb=#109#0%10.0.0.5%22%ops%%-1\n\n[Passwords]\nssh22:ops@10.0.0.5={}\nssh2222:ops@10.0.0.5={}\nrdp3389:admin@10.0.0.9={}\nroot@db.example.com={}\nnotapassword=\n\n[Credentials]\nDeploy=deploy:{}\n",
            encrypt("web-pass", &key),
            encrypt("other-port", &key),
            encrypt("rdp-pass", &key),
            encrypt("db-pass", &key),
            encrypt("deploy-pass", &key)
        );
        let s = Secrets::parse(&ini);
        assert_eq!(s.len(), 5);
        let mut hosts = vec![
            host("web", "10.0.0.5", 22, Some("ops")),
            host("web-alt", "10.0.0.5", 2222, Some("ops")),
            host("db", "DB.example.com", 22, None),
            host("desk", "10.0.0.9", 22, Some("admin")),
            host("deployer", "10.0.0.7", 22, Some("<Deploy>")),
            host("nothing", "10.0.0.8", 22, Some("ops")),
        ];
        mark_passwords(&mut hosts, &s);
        assert_eq!(hosts.iter().map(|h| h.has_password).collect::<Vec<_>>(), [true, true, true, true, true, false]);
        assert!(hosts.iter().all(|h| h.password.is_none()), "marking decrypts nothing");

        assert_eq!(attach_passwords(&mut hosts, &s, &key).unwrap(), 5);
        let pw = |i: usize| hosts[i].password.as_ref().unwrap().0.as_str().to_string();
        assert_eq!(pw(0), "web-pass", "the entry on the session's port");
        assert_eq!(pw(1), "other-port");
        assert_eq!(pw(2), "db-pass");
        assert_eq!(hosts[2].user.as_deref(), Some("root"), "a session without a user takes the password's");
        assert_eq!(pw(3), "rdp-pass", "an RDP password still counts when it is the only one for that host");
        assert_eq!(pw(4), "deploy-pass");
        assert_eq!(hosts[4].user.as_deref(), Some("<Deploy>"), "the session's own user is kept");
        assert!(hosts[5].password.is_none());
        // Secrets stay out of what is printed and what is sent to the window.
        let shown = format!("{:?}", hosts[0]);
        assert!(!shown.contains("web-pass"), "{shown}");
        let json = serde_json::to_string(&hosts[0]).unwrap();
        assert!(!json.contains("web-pass") && json.contains("\"has_password\":true"), "{json}");
    }

    #[test]
    fn a_wrong_master_password_attaches_nothing() {
        let key = derive_key("right");
        let ini = format!("[Passwords]\nssh22:ops@h={}\nssh22:ops@h2={}\n", encrypt("one", &key), encrypt("a much longer password here", &key));
        let s = Secrets::parse(&ini);
        let mut hosts = vec![host("h", "h", 22, Some("ops")), host("h2", "h2", 22, Some("ops"))];
        assert_eq!(attach_passwords(&mut hosts, &s, &derive_key("wrong")), Err(DecryptError::WrongKey));
        assert!(hosts.iter().all(|h| h.password.is_none()));
    }

    #[test]
    fn an_exported_sessions_file_has_no_passwords() {
        assert!(Secrets::parse(INI).is_empty());
        assert_eq!(default_master().is_some(), std::env::var("USERNAME").is_ok() && std::env::var("COMPUTERNAME").is_ok());
    }
}
