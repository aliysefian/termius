//! Import SSH bookmarks from MobaXterm (`MobaXterm.ini`, a `.mobaconf` configuration export, or an exported
//! `.mxtsessions` file). Each `[Bookmarks…]` section is a folder
//! (`SubRep=Parent\Child`); each SSH session is a line such as
//!
//! ```text
//! web-01=#109#0%10.0.0.5%22%ops%%-1%-1%%%%%0%0%0%_ProfileDir_\.ssh\id.ppk%…
//! ```
//!
//! where `109` means SSH and the `%` fields start with host, port and user.
//!
//! Passwords are not on those lines. MobaXterm keeps them in `MobaXterm.ini` (and in a `.mobaconf` export, which has the
//! same sections) under `[Passwords]` (one per `ssh22:user@host`) and `[Credentials]` (named user-and-password pairs).
//! There are three ways a value can be written, and which one is in use depends on the MobaXterm version and on whether
//! a master password was set:
//!
//! * **No master password** (the default): not encryption but a substitution that needs no secret. Each byte becomes two
//!   letters picked from a sixteen-letter table made from the `SessionP` line of `[Misc]`, which rotates one place after
//!   every letter, with random filler between them. See [`session_table`].
//! * **A master password, before MobaXterm 25:** AES-256 in CFB mode on single bytes, the key the first 32 bytes of
//!   SHA-512 of the master password, and the IV that key's encryption of a block of zeros.
//! * **A master password, from MobaXterm 25** (which fixed CVE-2025-0714, that fixed IV): the same cipher and key, with
//!   the IV written at the front of each value, which starts with `_@`.
//!
//! The first two were checked against the examples published with the HyperSine reverse-engineering notes; the third
//! follows an independent decryptor's description and has no published example. An exported `.mxtsessions` file holds no
//! passwords at all. See [`Secrets`].

use aes::cipher::{Block, BlockCipherEncrypt, KeyInit};
use aes::Aes256;
use base64::{
    alphabet,
    engine::{general_purpose::GeneralPurpose, DecodePaddingMode, GeneralPurposeConfig},
    Engine,
};
use sha2::{Digest, Sha512};
use std::collections::{BTreeMap, HashMap};
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

/// `[Bookmarks]`, `[Bookmarks_1]`…: where MobaXterm keeps its sessions, one section per folder.
fn is_bookmarks(section: &str) -> bool {
    section.get(..9).is_some_and(|p| p.eq_ignore_ascii_case("bookmarks"))
}

pub fn parse(text: &str, profile_dir: Option<&str>) -> Parsed {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut out = Parsed::default();
    let mut in_bookmarks = false;
    let mut folder = String::new();
    let mut skipped_other = 0usize;
    let mut bookmark_sections = 0usize;
    let mut unreadable = 0usize;
    let mut other_kinds: BTreeMap<String, usize> = BTreeMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_bookmarks = is_bookmarks(&line[1..line.len() - 1]);
            bookmark_sections += usize::from(in_bookmarks);
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
        let Some((kind, params)) = value.strip_prefix('#').and_then(|rest| rest.split_once('#')) else {
            unreadable += 1;
            continue;
        };
        if kind != SSH {
            skipped_other += 1;
            *other_kinds.entry(kind.to_string()).or_default() += 1;
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
    let renamed = tell_repeats_apart(&mut out.hosts);
    if renamed > 0 {
        out.warnings.push(format!("{renamed} session(s) share a name with another one, so their names now say which folder they are in"));
    }
    if out.hosts.is_empty() {
        out.warnings.push(why_no_hosts(text, bookmark_sections, unreadable, &other_kinds));
    }
    out
}

/// MobaXterm needs a session's name to be unique only within its folder, so `web-01` can be in `Prod/EU` and in
/// `Stage/EU`. A host is known by its name alone from here on (it is the label in the vault, and the import skips a
/// label that is taken), so a name that repeats gets its folder added: `web-01 (Prod/EU)`. Two of one name in one
/// folder, which only a hand-merged file can have, are numbered. Returns how many hosts were renamed.
fn tell_repeats_apart(hosts: &mut [ImportedHost]) -> usize {
    let mut uses: HashMap<String, usize> = HashMap::new();
    for h in hosts.iter() {
        *uses.entry(h.alias.to_lowercase()).or_default() += 1;
    }
    let mut renamed = 0;
    for h in hosts.iter_mut().filter(|h| uses[&h.alias.to_lowercase()] > 1) {
        h.alias = format!("{} ({})", h.alias, h.group.as_deref().unwrap_or("top level"));
        renamed += 1;
    }
    let mut seen: HashMap<String, usize> = HashMap::new();
    for h in hosts.iter_mut() {
        let n = seen.entry(h.alias.to_lowercase()).or_default();
        *n += 1;
        if *n > 1 {
            h.alias = format!("{} #{n}", h.alias);
            renamed += 1;
        }
    }
    renamed
}

/// Said when a file gave no SSH bookmark, so it is plain which part of the file this app could not use. It names
/// sections and session type numbers only, never a host, user or password.
fn why_no_hosts(text: &str, bookmark_sections: usize, unreadable: usize, other_kinds: &BTreeMap<String, usize>) -> String {
    if bookmark_sections == 0 {
        let sections = section_list(text).unwrap_or_else(|| "It has no [sections] at all, so it does not look like a MobaXterm file.".into());
        return format!("This file has no [Bookmarks] section, which is where MobaXterm keeps its sessions; it may have been exported without them. {sections}");
    }
    let mut why = Vec::new();
    if !other_kinds.is_empty() {
        let kinds: Vec<String> = other_kinds.iter().map(|(k, n)| format!("#{k} ×{n}")).collect();
        why.push(format!("its sessions are all of other types ({}), and an SSH session is #{SSH}", kinds.join(", ")));
    }
    if unreadable > 0 {
        why.push(format!("{unreadable} line(s) in its [Bookmarks] sections are not in the form name=#{SSH}#0%host%22%user…"));
    }
    if why.is_empty() {
        why.push("its [Bookmarks] sections hold no sessions".into());
    }
    let mut why = why.join("; ");
    why[..1].make_ascii_uppercase();
    format!("{why}.")
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

// -- reading the file ----------------------------------------------------------------------------------------------

/// The text of a MobaXterm file. It is usually in Windows' ANSI code page, so a name with an accent is not UTF-8; some
/// versions write UTF-8 (with or without a byte-order mark) or UTF-16. Anything that is not UTF-8 is read one byte to a
/// character, which keeps every ASCII line, and so every section, key and password, exactly as written.
pub fn decode_text(bytes: &[u8]) -> String {
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| String::from_utf16_lossy(&rest.as_chunks::<2>().0.iter().map(|c| unit(*c)).collect::<Vec<u16>>());
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return utf16(rest, u16::from_be_bytes);
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| b as char).collect(),
    }
}

/// What this is, if the file is an archive or other binary rather than the text MobaXterm writes (a zip's first bytes
/// would otherwise just read as "no sessions"). UTF-16 text is full of zero bytes and is not this.
pub fn not_text(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"PK\x03\x04") {
        return Some("a zip archive");
    }
    if bytes.starts_with(&[0x1F, 0x8B]) {
        return Some("a gzip archive");
    }
    if bytes.starts_with(b"7z\xBC\xAF") {
        return Some("a 7-Zip archive");
    }
    // What MobaXterm 25 and later writes in front of anything it encrypts with a password, here the whole file.
    if bytes.starts_with(b"_@") {
        return Some("an encrypted export (it starts with _@)");
    }
    let utf16 = bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]);
    (!utf16 && bytes.iter().take(4096).any(|&b| b == 0)).then_some("a binary file")
}

/// The sections of a file other than the bookmark ones, as "Its sections: A, B and 3 more". Names only, never a value.
fn section_list(text: &str) -> Option<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut names: Vec<&str> = Vec::new();
    for line in text.lines() {
        let name = line.trim().strip_prefix('[').and_then(|l| l.strip_suffix(']'));
        if let Some(name) = name.filter(|n| !is_bookmarks(n) && !names.contains(n)) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return None;
    }
    let more = names.len().saturating_sub(24);
    names.truncate(24);
    Some(format!("Its sections: {}{}.", names.join(", "), if more > 0 { format!(" and {more} more") } else { String::new() }))
}

/// What to say when a file that is more than a list of bookmarks has no saved passwords in it: the sections it does
/// have, so it is plain whether they are somewhere this app does not look.
pub fn no_passwords_note(text: &str) -> Option<String> {
    section_list(text).map(|sections| format!("No saved passwords were found in this file. {sections}"))
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

/// The encrypted passwords in a `MobaXterm.ini`. Nothing here is decrypted until [`attach_passwords`] is called.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Secrets {
    passwords: Vec<PasswordEntry>,
    credentials: Vec<CredentialEntry>,
    /// `SessionP` from `[Misc]`: what the table for a file with no master password is made from. It is not a secret
    /// (MobaXterm stores it in the clear beside the passwords it scrambles).
    session_p: Option<String>,
}

/// What was found for one host: the user name the password belongs to, and the ciphertext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub user: String,
    cipher: String,
}

impl Secrets {
    pub fn parse(text: &str) -> Secrets {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
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
            if key.eq_ignore_ascii_case("SessionP") && out.session_p.is_none() {
                out.session_p = Some(value.to_string());
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
    /// A master password was given and it does not turn the saved passwords into text.
    WrongKey,
    /// The saved passwords need the master password, and none was given.
    NeedsMaster,
    /// A saved value is not base64 of a sensible length.
    NotEncrypted,
}

impl std::fmt::Display for DecryptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecryptError::WrongKey => write!(f, "the master password does not decrypt the saved passwords"),
            DecryptError::NeedsMaster => write!(f, "the saved passwords are protected by a master password"),
            DecryptError::NotEncrypted => write!(f, "a saved password is not in a form this app can read"),
        }
    }
}

/// Base64 as MobaXterm writes it: the usual letters, with `@` and `_` standing in for `+` and `/` in the newer values,
/// and the `=` padding sometimes left off.
const B64: GeneralPurpose = GeneralPurpose::new(&alphabet::STANDARD, GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent).with_decode_allow_trailing_bits(true));

fn from_b64(s: &str) -> Result<Vec<u8>, DecryptError> {
    let plain_alphabet: String = s.trim().chars().map(|c| match c {
        '@' => '+',
        '_' => '/',
        c => c,
    }).collect();
    B64.decode(plain_alphabet).map_err(|_| DecryptError::NotEncrypted)
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

/// The IV of the values written before MobaXterm 25: the key's encryption of a block of zeros.
fn fixed_iv(key: &[u8; 32]) -> [u8; 16] {
    aes_block(&Aes256::new_from_slice(key).expect("an AES-256 key is thirty-two bytes"), &[0u8; 16])
}

/// CFB on single bytes: each byte is XORed with the first byte of the encrypted shift register, and the register then
/// takes the ciphertext byte in. The same loop encrypts and decrypts; only which byte is fed back differs.
fn cfb8(key: &[u8; 32], iv: [u8; 16], input: &[u8], decrypt: bool) -> Zeroizing<Vec<u8>> {
    let cipher = Aes256::new_from_slice(key).expect("an AES-256 key is thirty-two bytes");
    let mut shift = iv;
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

/// Decrypted bytes as a password. A wrong key (or a wrong table) gives bytes that are not text, which is how it is told apart.
fn as_secret(plain: &[u8]) -> Result<ImportedSecret, DecryptError> {
    let text = std::str::from_utf8(plain).map_err(|_| DecryptError::WrongKey)?;
    if text.is_empty() || text.chars().any(char::is_control) {
        return Err(DecryptError::WrongKey);
    }
    Ok(ImportedSecret(Zeroizing::new(text.to_string())))
}

/// A value written before MobaXterm 25, with a master password: base64 of the ciphertext, under the fixed IV.
pub fn decrypt(cipher_b64: &str, key: &[u8; 32]) -> Result<ImportedSecret, DecryptError> {
    let data = from_b64(cipher_b64)?;
    if data.is_empty() || data.len() > 1024 {
        return Err(DecryptError::NotEncrypted);
    }
    as_secret(&cfb8(key, fixed_iv(key), &data, true))
}

/// A value from MobaXterm 25 or later, with a master password: `_@`, then sixteen characters that are the IV, and the
/// whole string is base64 whose first fifteen bytes are that header; the rest is the ciphertext.
fn decrypt_v25(stored: &str, key: &[u8; 32]) -> Result<ImportedSecret, DecryptError> {
    let stored = stored.trim();
    let iv: [u8; 16] = stored.as_bytes().get(2..18).and_then(|b| b.try_into().ok()).ok_or(DecryptError::NotEncrypted)?;
    let data = from_b64(stored)?;
    let body = data.get(15..).filter(|b| !b.is_empty() && b.len() <= 1024).ok_or(DecryptError::NotEncrypted)?;
    as_secret(&cfb8(key, iv, body, true))
}

/// What MobaXterm writes for a password under a master password before version 25; for the tests, and to show the
/// scheme is the same both ways.
pub fn encrypt(plain: &str, key: &[u8; 32]) -> String {
    use base64::engine::general_purpose::STANDARD;
    STANDARD.encode(cfb8(key, fixed_iv(key), plain.as_bytes(), false).as_slice())
}

// -- no master password: the substitution table ------------------------------------------------------------------

const TABLE_SEED: &[u8; 16] = b"0d5e9n1348/U2+67";
const TABLE_LETTERS: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz+/";

/// The sixteen letters a file without a master password is written in, made from its `SessionP`: the fixed seed
/// `0d5e9n1348/U2+67`, with the letters of `SessionP` (stretched to twenty or more by repeating it, in capitals for
/// the even places and lower case for the odd) written over it wherever one is not in the table yet.
fn session_table(session_p: &str) -> Option<[u8; 16]> {
    let mut s = session_p.trim().as_bytes().to_vec();
    if s.is_empty() {
        return None;
    }
    while s.len() < 20 {
        let again = s.clone();
        s.extend(again);
    }
    let (upper, lower) = (s.to_ascii_uppercase(), s.to_ascii_lowercase());
    let space = [&upper, &upper, &lower, &lower];
    let mut table = *TABLE_SEED;
    for i in 0..table.len() {
        let b = space[(i + 1) % space.len()][i];
        if !table.contains(&b) && TABLE_LETTERS.contains(&b) {
            table[i] = b;
        }
    }
    Some(table)
}

/// Undo the substitution: keep the letters that are in the table, read them in pairs (low half first, then high), and
/// turn the table by one place after each letter. Everything else in the value is filler.
fn table_decrypt(stored: &str, table: &[u8; 16]) -> Vec<u8> {
    let mut table = *table;
    let letters: Vec<u8> = stored.bytes().filter(|b| table.contains(b)).collect();
    if !letters.len().is_multiple_of(2) {
        return Vec::new();
    }
    let place = |table: &[u8; 16], letter: u8| table.iter().position(|&t| t == letter).expect("the letter was kept because it is in the table") as u8;
    letters
        .chunks(2)
        .map(|pair| {
            let low = place(&table, pair[0]);
            table.rotate_right(1);
            let high = place(&table, pair[1]);
            table.rotate_right(1);
            high * 16 + low
        })
        .collect()
}

/// A value from a file with no master password.
fn decrypt_keyless(stored: &str, table: &[u8; 16]) -> Result<ImportedSecret, DecryptError> {
    as_secret(&table_decrypt(stored, table))
}

// -- deciding which scheme a value is in -------------------------------------------------------------------------

/// What can open a saved value: the key a master password makes, if one was given, and the table `SessionP` makes.
struct Keys {
    aes: Option<Zeroizing<[u8; 32]>>,
    table: Option<[u8; 16]>,
}

impl Keys {
    fn new(master: Option<&str>, session_p: Option<&str>) -> Keys {
        Keys { aes: master.map(str::trim).filter(|m| !m.is_empty()).map(derive_key), table: session_p.and_then(session_table) }
    }

    /// A saved value as text. A value that is in the newer layout can only be a master password's. Otherwise the master
    /// password's scheme is tried first (the keyless one would "decrypt" nearly anything into something), then the keyless one.
    fn open(&self, stored: &str) -> Result<ImportedSecret, DecryptError> {
        let stored = stored.trim();
        if stored.starts_with("_@") {
            return match &self.aes {
                Some(key) => decrypt_v25(stored, key),
                None => Err(DecryptError::NeedsMaster),
            };
        }
        if let Some(key) = &self.aes {
            if let Ok(secret) = decrypt(stored, key) {
                return Ok(secret);
            }
        }
        if let Some(table) = &self.table {
            if let Ok(secret) = decrypt_keyless(stored, table) {
                return Ok(secret);
            }
        }
        Err(if self.aes.is_some() { DecryptError::WrongKey } else { DecryptError::NeedsMaster })
    }
}

/// How an attempt to attach the saved passwords went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attached {
    /// Hosts that now carry their password.
    pub attached: usize,
    /// Hosts whose saved password could not be read, and so were left without one.
    pub failed: usize,
}

/// Decrypt the saved password of every host that has one and put it on the host (and the user name it belongs to,
/// when the session named none). `master` is the master password, if MobaXterm has one; a file without one needs
/// nothing. A host whose password does not decrypt is left without it. If none decrypts, that is an error, since
/// that is what a missing or wrong master password looks like, and nothing is attached.
pub fn attach_passwords(hosts: &mut [ImportedHost], secrets: &Secrets, master: Option<&str>) -> Result<Attached, DecryptError> {
    let keys = Keys::new(master, secrets.session_p.as_deref());
    let mut found: Vec<(usize, Found, ImportedSecret)> = Vec::new();
    let mut failed = 0usize;
    let mut first_error = None;
    for (i, h) in hosts.iter().enumerate() {
        let Some(f) = secrets.find(h) else { continue };
        match keys.open(&f.cipher) {
            Ok(secret) => found.push((i, f, secret)),
            Err(e) => {
                failed += 1;
                first_error.get_or_insert(e);
            }
        }
    }
    if found.is_empty() {
        if let Some(e) = first_error {
            return Err(e);
        }
    }
    let attached = found.len();
    for (i, f, secret) in found {
        let h = &mut hosts[i];
        if h.user.is_none() {
            h.user = Some(f.user);
        }
        h.password = Some(secret);
        h.has_password = true;
    }
    Ok(Attached { attached, failed })
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
        &["mxtsessions", "ini", "mobaconf"]
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

    /// What MobaXterm writes with no master password: the table, and random letters from outside it between the pairs.
    fn keyless(plain: &str, session_p: &str) -> String {
        let mut table = session_table(session_p).unwrap();
        let filler: Vec<char> = "ghijklmopqrstvwxyzGHIJKLMOPQRSTVWXYZ".chars().filter(|c| !table.contains(&(*c as u8))).collect();
        let mut out = String::new();
        for (n, b) in plain.bytes().enumerate() {
            out.push(table[(b & 15) as usize] as char);
            table.rotate_right(1);
            out.push(table[(b >> 4) as usize] as char);
            table.rotate_right(1);
            out.push(filler[(n * 7) % filler.len()]);
        }
        out
    }

    /// A value in the layout of MobaXterm 25: `_@`, the sixteen IV characters, two more, then the base64 ciphertext.
    fn v25(plain: &str, key: &[u8; 32], iv: &str) -> String {
        use base64::engine::general_purpose::STANDARD;
        assert_eq!(iv.len(), 16);
        let body = STANDARD.encode(cfb8(key, iv.as_bytes().try_into().unwrap(), plain.as_bytes(), false).as_slice());
        format!("_@{iv}AA{body}").replace('+', "@").replace('/', "_")
    }

    #[test]
    fn the_published_examples_decrypt() {
        // From the HyperSine notes on how MobaXterm encrypts passwords (master password 12345678, SessionP 165821882556840).
        let key = derive_key("12345678");
        assert_eq!(decrypt("0XROpGmLAYVx", &key).unwrap().0.as_str(), "HyperSine");
        assert_eq!(decrypt("1du11XKQBOxud/FWh4ouWA==", &key).unwrap().0.as_str(), "Lw3+cZ2s.w@U@f]U");
        let table = session_table("165821882556840").unwrap();
        let cipher = "bSj4VWbHezNH3tTY9Nil2RzJX57p7/S6KqMw8VsiT/WH+I8p03pqnInAu";
        assert_eq!(decrypt_keyless(cipher, &table).unwrap().0.as_str(), "HyperSine");
        // The same value through the front door, with and without a master password typed in.
        assert_eq!(Keys::new(None, Some("165821882556840")).open(cipher).unwrap().0.as_str(), "HyperSine");
        assert_eq!(Keys::new(Some("12345678"), Some("165821882556840")).open(cipher).unwrap().0.as_str(), "HyperSine");
    }

    #[test]
    fn the_table_is_made_from_session_p() {
        assert_eq!(session_table(""), None);
        assert_eq!(session_table("   "), None);
        // A numeric SessionP, as MobaXterm writes it, has only digits, and the seed already holds every digit.
        let t = session_table("165821882556840").unwrap();
        assert_eq!(&t, TABLE_SEED);
        // Letters are written over the seed where they are not in the table yet: capitals for the even places, lower case
        // for the odd. (Worked out by a second implementation of the published steps; there is no published example.)
        assert_eq!(&session_table("pT9vbXd2a1").unwrap(), b"Pt5VBxd3A1pT9vbX");
        // Sixteen different letters from the alphabet: the table is a permutation, so every nibble has a letter.
        let mut seen = t.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 16);
        assert!(t.iter().all(|b| TABLE_LETTERS.contains(b)));
        assert_ne!(session_table("a"), session_table("b"));
        // A short SessionP is repeated, so "ab" and "ababababababababababab" give one table.
        assert_eq!(session_table("ab"), session_table(&"ab".repeat(10)));
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
    fn the_keyless_scheme_round_trips_with_filler_in_it() {
        for plain in ["hunter2", "p@ss wörd ✓", "x", &"long".repeat(50)] {
            let c = keyless(plain, "pT9vbXd2a1");
            assert_eq!(Keys::new(None, Some("pT9vbXd2a1")).open(&c).unwrap().0.as_str(), plain);
        }
        // Another SessionP is another table: it never gives the password back, though it may give some text.
        let c = keyless("correct horse battery staple", "pT9vbXd2a1");
        assert_ne!(Keys::new(None, Some("zzzzzzzzzz")).open(&c).ok().map(|s| s.0.as_str().to_string()), Some("correct horse battery staple".into()));
    }

    #[test]
    fn a_file_with_no_master_password_needs_none() {
        // What a default MobaXterm writes: a SessionP, and passwords that are not encrypted with anything.
        let sp = "165821882556840";
        let ini = format!(
            "[Misc]\r\nSessionP={sp}\r\n\r\n[Bookmarks]\r\nSubRep=\r\nweb=#109#0%10.0.0.5%22%ops%%-1\r\nci=#109#0%10.0.0.7%22%<example.com>%%-1\r\nbare=#109#0%10.0.0.8%22%%%-1\r\n\r\n[Passwords]\r\nssh22:ops@10.0.0.5={}\r\nssh22:root@10.0.0.8={}\r\n\r\n[Credentials]\r\nexample.com=root:bSj4VWbHezNH3tTY9Nil2RzJX57p7/S6KqMw8VsiT/WH+I8p03pqnInAu\r\n",
            keyless("web-pass", sp),
            keyless("bare-pass", sp)
        );
        let secrets = Secrets::parse(&ini);
        assert_eq!(secrets.len(), 3);
        let mut hosts = parse(&ini, None).hosts;
        assert_eq!(hosts.len(), 3);
        mark_passwords(&mut hosts, &secrets);
        assert!(hosts.iter().all(|h| h.has_password && h.password.is_none()));
        assert_eq!(attach_passwords(&mut hosts, &secrets, None), Ok(Attached { attached: 3, failed: 0 }));
        let pw = |i: usize| hosts[i].password.as_ref().unwrap().0.as_str().to_string();
        assert_eq!(pw(0), "web-pass");
        assert_eq!(pw(1), "HyperSine", "a saved credential, by its name");
        assert_eq!(pw(2), "bare-pass");
        assert_eq!(hosts[2].user.as_deref(), Some("root"), "a session with no user takes the password's");
        // A master password typed in for a file that has none changes nothing.
        let mut again = parse(&ini, None).hosts;
        assert_eq!(attach_passwords(&mut again, &secrets, Some("whatever")), Ok(Attached { attached: 3, failed: 0 }));
        assert_eq!(again[0].password.as_ref().unwrap().0.as_str(), "web-pass");
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

        assert_eq!(attach_passwords(&mut hosts, &s, Some("m")), Ok(Attached { attached: 5, failed: 0 }));
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
        assert_eq!(attach_passwords(&mut hosts, &s, Some("wrong")), Err(DecryptError::WrongKey));
        assert!(hosts.iter().all(|h| h.password.is_none()));
        // And with none typed in, the file says what it needs.
        assert_eq!(attach_passwords(&mut hosts, &s, None), Err(DecryptError::NeedsMaster));
        assert_eq!(attach_passwords(&mut hosts, &s, Some("  ")), Err(DecryptError::NeedsMaster), "blank is none");
        assert!(hosts.iter().all(|h| h.password.is_none()));
        assert_eq!(attach_passwords(&mut hosts, &s, Some("right")), Ok(Attached { attached: 2, failed: 0 }));
    }

    #[test]
    fn mobaxterm_25_values_need_the_master_password_and_carry_their_own_iv() {
        let key = derive_key("right");
        let ini = format!("[Passwords]\nssh22:ops@h={}\nssh22:ops@h2={}\n", v25("one", &key, "AbCdEfGh12345678"), v25("a much longer password here", &key, "ZyXwVuTs87654321"));
        let s = Secrets::parse(&ini);
        let hosts = || vec![host("h", "h", 22, Some("ops")), host("h2", "h2", 22, Some("ops"))];
        let mut a = hosts();
        assert_eq!(attach_passwords(&mut a, &s, Some("right")), Ok(Attached { attached: 2, failed: 0 }));
        assert_eq!(a[0].password.as_ref().unwrap().0.as_str(), "one");
        assert_eq!(a[1].password.as_ref().unwrap().0.as_str(), "a much longer password here");
        assert_eq!(attach_passwords(&mut hosts(), &s, Some("wrong")), Err(DecryptError::WrongKey));
        assert_eq!(attach_passwords(&mut hosts(), &s, None), Err(DecryptError::NeedsMaster));
        // The same password under two IVs is two different values, which the fixed IV of the older scheme was not.
        assert_ne!(v25("same", &key, "AbCdEfGh12345678"), v25("same", &key, "ZyXwVuTs87654321"));
        assert_eq!(decrypt_v25("_@short", &key), Err(DecryptError::NotEncrypted));
    }

    #[test]
    fn one_password_that_cannot_be_read_does_not_stop_the_others() {
        let sp = "pT9vbXd2a1";
        let ini = format!("[Misc]\nSessionP={sp}\n[Passwords]\nssh22:ops@h={}\nssh22:ops@h2=!!!!\nssh22:ops@h3={}\n", keyless("one", sp), keyless("three", sp));
        let s = Secrets::parse(&ini);
        let mut hosts = vec![host("h", "h", 22, Some("ops")), host("h2", "h2", 22, Some("ops")), host("h3", "h3", 22, Some("ops"))];
        assert_eq!(attach_passwords(&mut hosts, &s, None), Ok(Attached { attached: 2, failed: 1 }));
        assert_eq!(hosts[0].password.as_ref().unwrap().0.as_str(), "one");
        assert!(hosts[1].password.is_none());
        assert_eq!(hosts[2].password.as_ref().unwrap().0.as_str(), "three");
    }

    #[test]
    fn session_p_is_found_whatever_its_case_and_section() {
        assert_eq!(Secrets::parse("[Misc]\nSessionP=abc123\n").session_p.as_deref(), Some("abc123"));
        assert_eq!(Secrets::parse("[Misc]\nsessionp = abc123 \n").session_p.as_deref(), Some("abc123"));
        assert_eq!(Secrets::parse("[Other]\nSessionP=abc123\n").session_p.as_deref(), Some("abc123"));
        assert_eq!(Secrets::parse("[Misc]\nSessionP=\n").session_p, None);
        assert_eq!(Secrets::parse("[Misc]\nFoo=bar\n").session_p, None);
    }

    #[test]
    fn an_exported_sessions_file_has_no_passwords() {
        assert!(Secrets::parse(INI).is_empty());
    }

    #[test]
    fn a_byte_order_mark_does_not_hide_the_sessions_or_the_passwords() {
        let sp = "165821882556840";
        let ini = format!("\u{feff}[Bookmarks]\nSubRep=\nweb=#109#0%10.0.0.5%22%ops%%-1\n[Misc]\nSessionP={sp}\n[Passwords]\nssh22:ops@10.0.0.5={}\n", keyless("web-pass", sp));
        let mut p = parse(&ini, None);
        assert_eq!(p.hosts.len(), 1, "the first section was not read: {p:?}");
        let s = Secrets::parse(&ini);
        assert_eq!(attach_passwords(&mut p.hosts, &s, None), Ok(Attached { attached: 1, failed: 0 }));
        assert_eq!(p.hosts[0].password.as_ref().unwrap().0.as_str(), "web-pass");
    }

    #[test]
    fn a_file_is_read_whatever_its_encoding() {
        let text = "[Bookmarks]\r\nSubRep=Caf\u{e9}\r\nweb=#109#0%h%22%ops\r\n";
        assert_eq!(decode_text(text.as_bytes()), text);
        assert_eq!(decode_text(&[&[0xEF, 0xBB, 0xBF][..], text.as_bytes()].concat()), text, "a UTF-8 byte-order mark is dropped");
        let le: Vec<u8> = [0xFF, 0xFE].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect();
        let be: Vec<u8> = [0xFE, 0xFF].into_iter().chain(text.encode_utf16().flat_map(u16::to_be_bytes)).collect();
        assert_eq!(decode_text(&le), text);
        assert_eq!(decode_text(&be), text);
        // Windows' ANSI code page: an accented letter is the single byte E9, which is not UTF-8.
        let ansi: Vec<u8> = text.chars().map(|c| c as u32 as u8).collect();
        assert!(std::str::from_utf8(&ansi).is_err());
        assert_eq!(decode_text(&ansi), text);
        assert_eq!(parse(&decode_text(&ansi), None).hosts[0].group.as_deref(), Some("Caf\u{e9}"));
    }

    #[test]
    fn a_configuration_without_passwords_says_which_sections_it_has() {
        let ini = "\u{feff}[Misc]\nSessionP=1\n[Bookmarks]\nSubRep=\n[Bookmarks_1]\n[Display]\n[Misc]\n";
        assert_eq!(no_passwords_note(ini).as_deref(), Some("No saved passwords were found in this file. Its sections: Misc, Display."));
        assert_eq!(no_passwords_note(INI.replace("[Misc]", "[Bookmarks_9]").as_str()), None, "a plain bookmarks file has nothing to explain");
        let many: String = (0..30).map(|n| format!("[S{n}]\n")).collect();
        assert!(no_passwords_note(&many).unwrap().ends_with("S23 and 6 more."));
    }

    #[test]
    fn a_file_with_no_ssh_bookmark_says_why() {
        let note = |ini: &str| {
            let p = parse(ini, None);
            assert!(p.hosts.is_empty());
            p.warnings.join(" | ")
        };
        let w = note("[Misc]\nSessionP=1\n[Passwords]\nssh22:ops@h=x\n");
        assert!(w.contains("no [Bookmarks] section") && w.contains("Its sections: Misc, Passwords."), "{w}");
        assert!(note("just some text\n").contains("no [sections] at all"));
        assert_eq!(note("[Bookmarks]\nSubRep=\nImgNum=42\n[Bookmarks_1]\nSubRep=x\n"), "Its [Bookmarks] sections hold no sessions.");
        let w = note("[bookmarks]\nSubRep=\nrdp=#91#4%h%3389%u\nrdp2=#91#4%h%3389%u\nvnc=#128#5%h%5900\n");
        assert!(w.contains("3 non-SSH") && w.contains("#128 ×1, #91 ×2") && w.contains("an SSH session is #109"), "{w}");
        let w = note("[Bookmarks]\nweb=10.0.0.5\n");
        assert!(w.contains("1 line(s)") && w.contains("name=#109#0%host%22%user"), "{w}");
        assert!(!w.contains("10.0.0.5"), "a value must not be repeated: {w}");
        // Nothing to explain once a host was found.
        assert!(!parse(INI, None).warnings.iter().any(|w| w.contains("[Bookmarks]")));
    }

    #[test]
    fn the_same_name_in_two_folders_stays_two_hosts() {
        let ini = "[Bookmarks_1]\nSubRep=Prod\\EU\\Web\nweb-01=#109#0%10.0.0.1%22%ops\nweb-02=#109#0%10.0.0.2%22%ops\n\
                   [Bookmarks_2]\nSubRep=Stage\\EU\nWeb-01=#109#0%10.1.0.1%22%ops\n\
                   [Bookmarks]\nSubRep=\nweb-01=#109#0%10.2.0.1%22%ops\n";
        let p = parse(ini, None);
        let names: Vec<&str> = p.hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, ["web-01 (Prod/EU/Web)", "web-02", "Web-01 (Stage/EU)", "web-01 (top level)"]);
        let hosts: Vec<&str> = p.hosts.iter().map(|h| h.hostname.as_str()).collect();
        assert_eq!(hosts, ["10.0.0.1", "10.0.0.2", "10.1.0.1", "10.2.0.1"], "every host keeps its own address");
        assert!(p.warnings.iter().any(|w| w.starts_with("3 session(s) share a name")), "{:?}", p.warnings);
        let unique: std::collections::HashSet<String> = p.hosts.iter().map(|h| h.alias.to_lowercase()).collect();
        assert_eq!(unique.len(), p.hosts.len());
    }

    #[test]
    fn two_of_one_name_in_one_folder_are_numbered() {
        let p = parse("[Bookmarks]\nSubRep=A\nweb=#109#0%h1%22%u\nweb=#109#0%h2%22%u\nother=#109#0%h3%22%u\n", None);
        let names: Vec<&str> = p.hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(names, ["web (A)", "web (A) #2", "other"]);
    }

    #[test]
    fn names_that_do_not_repeat_are_left_alone() {
        let p = parse(INI, Some("C:\\Users\\me"));
        assert_eq!(p.hosts[0].alias, "web-01");
        assert!(!p.warnings.iter().any(|w| w.contains("share a name")), "{:?}", p.warnings);
    }

    #[test]
    fn an_archive_is_not_taken_for_an_empty_bookmarks_file() {
        assert_eq!(not_text(b"PK\x03\x04\x14\x00rest"), Some("a zip archive"));
        assert_eq!(not_text(&[0x1F, 0x8B, 8, 0]), Some("a gzip archive"));
        assert_eq!(not_text(b"7z\xBC\xAF\x27\x1C"), Some("a 7-Zip archive"));
        assert_eq!(not_text(b"_@etAbCdEfGhIjKlMnAA"), Some("an encrypted export (it starts with _@)"));
        assert_eq!(not_text(b"[Bookmarks]\r\nSubRep=\r\n\x00"), Some("a binary file"));
        assert_eq!(not_text(b"[Bookmarks]\r\nSubRep=\r\n"), None);
        let utf16: Vec<u8> = [0xFF, 0xFE].into_iter().chain("[Bookmarks]".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(not_text(&utf16), None, "UTF-16 text has zero bytes and is still text");
    }

    #[test]
    fn a_mobaconf_file_is_one_of_the_formats_files() {
        assert!(MobaXterm.extensions().contains(&"mobaconf"));
        assert_eq!(MobaXterm.extensions()[0], "mxtsessions", "the first is what an export is saved as");
    }
}
