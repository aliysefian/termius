//! One-time codes (TOTP, RFC 6238) for servers that ask for a "Verification code" after the password or key.
//!
//! This is opt-in twice over: it does nothing until it is switched on for this computer (`AppConfig::totp_enabled`),
//! and then only for identities that have a secret saved. A code goes only to a prompt that asks for one (see
//! [`is_code_prompt`]), never to a password prompt, and the saved password never goes to a code prompt (see `kbdint`).

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TotpError {
    #[error("The secret is empty")]
    Empty,
    #[error("The secret is not valid Base32 (letters A to Z and digits 2 to 7)")]
    NotBase32,
    #[error("The otpauth link has no secret")]
    NoSecret,
    #[error("Only time-based codes are supported (this link is for {0})")]
    NotTotp(String),
    #[error("Unsupported setting: {0}")]
    Unsupported(String),
}

/// A secret and how to turn it into codes. Cloned into each connection target; the secret is wiped when dropped.
#[derive(Clone, PartialEq, Eq)]
pub struct Totp {
    secret: Zeroizing<Vec<u8>>,
    digits: u32,
    period: u64,
    algorithm: Algorithm,
}

impl fmt::Debug for Totp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Totp").field("secret", &"<hidden>").field("digits", &self.digits).field("period", &self.period).finish()
    }
}

/// What the form shows next to the secret, to check it against the authenticator app.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Preview {
    pub code: String,
    pub seconds_left: u64,
}

impl Totp {
    /// A Base32 secret as authenticator apps show it (any case, spaces and dashes ignored), or the `otpauth://totp/…`
    /// link the QR code holds, which can also set the digits, period and algorithm.
    pub fn parse(input: &str) -> Result<Totp, TotpError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(TotpError::Empty);
        }
        if input.get(..10).is_some_and(|p| p.eq_ignore_ascii_case("otpauth://")) {
            return Self::from_link(input);
        }
        Ok(Totp { secret: decode_base32(input)?, digits: 6, period: 30, algorithm: Algorithm::Sha1 })
    }

    fn from_link(link: &str) -> Result<Totp, TotpError> {
        let rest = &link[10..];
        let end = rest.find(['/', '?']).unwrap_or(rest.len());
        let kind = &rest[..end];
        if !kind.eq_ignore_ascii_case("totp") {
            return Err(TotpError::NotTotp(kind.to_string()));
        }
        let query = rest.split_once('?').map(|(_, q)| q).unwrap_or("");
        let (mut secret, mut digits, mut period, mut algorithm) = (None, 6u32, 30u64, Algorithm::Sha1);
        for pair in query.split('&') {
            let Some((k, v)) = pair.split_once('=') else { continue };
            match k.to_ascii_lowercase().as_str() {
                "secret" => secret = Some(decode_base32(&percent_decode(v))?),
                "digits" => {
                    digits = v.parse().ok().filter(|d| (6..=8).contains(d)).ok_or_else(|| TotpError::Unsupported(format!("digits={v}")))?
                }
                "period" => {
                    period = v.parse().ok().filter(|p| (1..=300).contains(p)).ok_or_else(|| TotpError::Unsupported(format!("period={v}")))?
                }
                "algorithm" => {
                    algorithm = match v.to_ascii_uppercase().as_str() {
                        "SHA1" => Algorithm::Sha1,
                        "SHA256" => Algorithm::Sha256,
                        "SHA512" => Algorithm::Sha512,
                        _ => return Err(TotpError::Unsupported(format!("algorithm={v}"))),
                    }
                }
                _ => {}
            }
        }
        Ok(Totp { secret: secret.ok_or(TotpError::NoSecret)?, digits, period, algorithm })
    }

    pub fn period(&self) -> u64 {
        self.period
    }

    /// The counter for a moment: how many periods have passed since 1970.
    pub fn step_at(&self, unix_secs: u64) -> u64 {
        unix_secs / self.period
    }

    /// The code for a counter value (RFC 4226 dynamic truncation).
    pub fn code_for_step(&self, step: u64) -> String {
        let msg = step.to_be_bytes();
        let digest = match self.algorithm {
            Algorithm::Sha1 => finish(Hmac::<sha1::Sha1>::new_from_slice(&self.secret), &msg),
            Algorithm::Sha256 => finish(Hmac::<sha2::Sha256>::new_from_slice(&self.secret), &msg),
            Algorithm::Sha512 => finish(Hmac::<sha2::Sha512>::new_from_slice(&self.secret), &msg),
        };
        let offset = (digest[digest.len() - 1] & 0x0f) as usize;
        let bin = u32::from_be_bytes([digest[offset] & 0x7f, digest[offset + 1], digest[offset + 2], digest[offset + 3]]);
        let code = bin % 10u32.pow(self.digits);
        format!("{code:0width$}", width = self.digits as usize)
    }

    pub fn code_at(&self, unix_secs: u64) -> String {
        self.code_for_step(self.step_at(unix_secs))
    }

    pub fn seconds_left(&self, unix_secs: u64) -> u64 {
        self.period - unix_secs % self.period
    }

    pub fn preview(&self) -> Preview {
        let now = unix_now();
        Preview { code: self.code_at(now), seconds_left: self.seconds_left(now) }
    }
}

fn finish<M: Mac>(m: Result<M, hmac::digest::InvalidLength>, msg: &[u8]) -> Vec<u8> {
    let mut m = m.expect("HMAC accepts a key of any length");
    m.update(msg);
    m.finalize().into_bytes().to_vec()
}

/// What to store when an identity is saved. The form never receives the stored secret, so: `Some("")` keeps what is
/// stored, `None` removes it, and anything else is checked and replaces it.
pub fn merge_secret(incoming: Option<String>, stored: Option<&str>) -> Result<Option<String>, TotpError> {
    match incoming {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(stored.map(str::to_string)),
        Some(s) => {
            Totp::parse(&s)?;
            Ok(Some(s.trim().to_string()))
        }
    }
}

pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// RFC 4648 Base32, forgiving about what people paste: any case, spaces, dashes and `=` padding.
fn decode_base32(input: &str) -> Result<Zeroizing<Vec<u8>>, TotpError> {
    let mut out = Zeroizing::new(Vec::with_capacity(input.len() * 5 / 8));
    let (mut buf, mut bits) = (0u32, 0u32);
    let mut any = false;
    for c in input.chars() {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32,
            '2'..='7' => c as u32 - '2' as u32 + 26,
            ' ' | '-' | '=' | '\t' | '\n' | '\r' => continue,
            _ => return Err(TotpError::NotBase32),
        };
        any = true;
        buf = (buf << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    if !any || out.is_empty() {
        return Err(TotpError::Empty);
    }
    Ok(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let escaped = (b[i] == b'%' && i + 2 < b.len())
            .then(|| std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()))
            .flatten();
        match escaped {
            Some(v) => {
                out.push(v);
                i += 3;
            }
            None => {
                out.push(b[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Does this keyboard-interactive prompt ask for a one-time code? Narrower than "is not a password prompt": Duo's
/// "Passcode or option (1-3):", PIN prompts and push approvals are not answered by an authenticator code.
pub fn is_code_prompt(prompt: &str) -> bool {
    let p = prompt.to_lowercase();
    const NOT_A_CODE: [&str; 5] = ["duo", "option", "pin", "push", "approve"];
    if NOT_A_CODE.iter().any(|w| contains_word(&p, w)) {
        return false;
    }
    const CODE_WORDS: [&str; 9] = ["code", "otp", "totp", "authenticator", "2fa", "mfa", "two-factor", "one-time", "one time"];
    CODE_WORDS.iter().any(|w| contains_word(&p, w)) || p.contains("passcode")
}

/// `word` in `text` with no letter or digit right before or after it ("pin" in "Enter PIN:" but not in "pinned").
fn contains_word(text: &str, word: &str) -> bool {
    let mut from = 0;
    while let Some(i) = text[from..].find(word) {
        let start = from + i;
        let end = start + word.len();
        let before = text[..start].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
        let after = text[end..].chars().next().is_none_or(|c| !c.is_alphanumeric());
        if before && after {
            return true;
        }
        from = start + 1;
    }
    false
}

/// Remembers the last code sent to each login, so a second connection in the same period waits for the next code
/// instead of sending one the server has already used (Google Authenticator's PAM module refuses a repeat).
#[derive(Default)]
pub struct ReuseGuard {
    last: Mutex<HashMap<String, u64>>,
}

impl ReuseGuard {
    /// Take `step` for `login` if it is newer than the last one taken.
    pub fn claim(&self, login: &str, step: u64) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|p| p.into_inner());
        match last.get(login) {
            Some(&s) if s >= step => false,
            _ => {
                last.insert(login.to_string(), step);
                true
            }
        }
    }
}

static GUARD: std::sync::LazyLock<ReuseGuard> = std::sync::LazyLock::new(ReuseGuard::default);

impl Totp {
    /// `None` when `login` may use the code for `now` (and records that it did); otherwise how long until the next
    /// period starts, when a fresh code can be sent.
    fn wait_before_use(&self, guard: &ReuseGuard, login: &str, now: u64) -> Option<Duration> {
        if guard.claim(login, self.step_at(now)) {
            None
        } else {
            Some(Duration::from_secs(self.seconds_left(now)) + Duration::from_millis(300))
        }
    }

    /// The code to send now for `login` (`user@host:port`). If this login already used this period's code, waits for
    /// the next period first, which is at most one period.
    pub async fn next_code(&self, login: &str) -> String {
        for _ in 0..3 {
            let now = unix_now();
            match self.wait_before_use(&GUARD, login, now) {
                None => return self.code_at(now),
                Some(wait) => tokio::time::sleep(wait).await,
            }
        }
        // The clock keeps jumping about; send what it says rather than wait for ever.
        self.code_at(unix_now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 6238 appendix B: 8 digits, the secret is the ASCII string "12345678901234567890" (extended for SHA-256/512).
    fn rfc(algorithm: Algorithm, secret: &[u8]) -> Totp {
        Totp { secret: Zeroizing::new(secret.to_vec()), digits: 8, period: 30, algorithm }
    }

    #[test]
    fn matches_the_rfc_6238_test_vectors() {
        let sha1 = rfc(Algorithm::Sha1, b"12345678901234567890");
        let sha256 = rfc(Algorithm::Sha256, b"12345678901234567890123456789012");
        let sha512 = rfc(Algorithm::Sha512, b"1234567890123456789012345678901234567890123456789012345678901234");
        for (t, c1, c256, c512) in [
            (59u64, "94287082", "46119246", "90693936"),
            (1111111109, "07081804", "68084774", "25091201"),
            (1111111111, "14050471", "67062674", "99943326"),
            (1234567890, "89005924", "91819424", "93441116"),
            (2000000000, "69279037", "90698825", "38618901"),
            (20000000000, "65353130", "77737706", "47863826"),
        ] {
            assert_eq!(sha1.code_at(t), c1, "sha1 at {t}");
            assert_eq!(sha256.code_at(t), c256, "sha256 at {t}");
            assert_eq!(sha512.code_at(t), c512, "sha512 at {t}");
        }
    }

    #[test]
    fn reads_a_base32_secret_the_way_apps_show_it() {
        // "12345678901234567890" in Base32; apps group it in fours, in any case.
        let plain = Totp::parse("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap();
        let grouped = Totp::parse("gezd gnbv gy3t qojq-gezd gnbv gy3t qojq====").unwrap();
        assert_eq!(plain, grouped);
        assert_eq!((plain.digits, plain.period, plain.algorithm), (6, 30, Algorithm::Sha1));
        assert_eq!(plain.code_at(59), "287082");
    }

    #[test]
    fn rejects_what_is_not_a_secret() {
        assert_eq!(Totp::parse("  "), Err(TotpError::Empty));
        assert_eq!(Totp::parse("not base32!"), Err(TotpError::NotBase32));
        assert_eq!(Totp::parse("ABC1"), Err(TotpError::NotBase32), "1 is not in the alphabet");
        assert_eq!(Totp::parse("----"), Err(TotpError::Empty));
    }

    #[test]
    fn reads_an_otpauth_link() {
        let t = Totp::parse("otpauth://totp/Example:alice@example.com?secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ&issuer=Example&digits=8&period=60&algorithm=SHA1").unwrap();
        assert_eq!((t.digits, t.period, t.algorithm), (8, 60, Algorithm::Sha1));
        assert_eq!(t.step_at(120), 2);
        let sha256 = Totp::parse("OTPAUTH://TOTP/x?algorithm=sha256&secret=GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap();
        assert_eq!(sha256.algorithm, Algorithm::Sha256);
        assert_eq!(Totp::parse("otpauth://totp/x?secret=GEZD%20GNBV%0AGY3T").unwrap().secret.len(), 7, "percent escapes are undone");
    }

    #[test]
    fn rejects_links_it_cannot_use() {
        assert_eq!(Totp::parse("otpauth://hotp/x?secret=GEZDGNBV"), Err(TotpError::NotTotp("hotp".into())));
        assert_eq!(Totp::parse("otpauth://totp/x?issuer=a"), Err(TotpError::NoSecret));
        assert!(matches!(Totp::parse("otpauth://totp/x?secret=GEZDGNBV&digits=12"), Err(TotpError::Unsupported(_))));
        assert!(matches!(Totp::parse("otpauth://totp/x?secret=GEZDGNBV&period=0"), Err(TotpError::Unsupported(_))));
        assert!(matches!(Totp::parse("otpauth://totp/x?secret=GEZDGNBV&algorithm=MD5"), Err(TotpError::Unsupported(_))));
    }

    #[test]
    fn saving_keeps_replaces_or_removes_the_secret() {
        let old = Some("GEZDGNBVGY3TQOJQ");
        assert_eq!(merge_secret(Some(String::new()), old), Ok(old.map(str::to_string)), "empty keeps it");
        assert_eq!(merge_secret(Some("  ".into()), old), Ok(old.map(str::to_string)));
        assert_eq!(merge_secret(Some(String::new()), None), Ok(None), "nothing to keep");
        assert_eq!(merge_secret(None, old), Ok(None), "none removes it");
        assert_eq!(merge_secret(Some(" mfrg gzdf mzt wq2lk \n".into()), old), Ok(Some("mfrg gzdf mzt wq2lk".to_string())), "a new one replaces it, trimmed");
        assert_eq!(merge_secret(Some("not base32!".into()), old), Err(TotpError::NotBase32), "a bad one is refused, not stored");
        assert!(merge_secret(Some("otpauth://hotp/x?secret=GEZDGNBV".into()), None).is_err());
    }

    #[test]
    fn counts_the_seconds_left_in_a_period() {
        let t = Totp::parse("GEZDGNBVGY3TQOJQ").unwrap();
        assert_eq!((t.seconds_left(0), t.seconds_left(1), t.seconds_left(29), t.seconds_left(30)), (30, 29, 1, 30));
    }

    #[test]
    fn debug_never_prints_the_secret() {
        let t = Totp::parse("GEZDGNBVGY3TQOJQ").unwrap();
        let shown = format!("{t:?}");
        assert!(shown.contains("<hidden>") && !shown.contains("GEZD") && !shown.contains("49"), "{shown}");
    }

    #[test]
    fn recognises_prompts_that_ask_for_a_code() {
        for p in [
            "Verification code: ",
            "Verification code:",
            "One-time password (OTP):",
            "Authenticator code",
            "Enter your 2FA code",
            "MFA code: ",
            "Two-factor code:",
            "Passcode:",
            "Enter OTP: ",
            "Your one time code",
            "TOTP token code:",
        ] {
            assert!(is_code_prompt(p), "{p:?}");
        }
    }

    #[test]
    fn leaves_other_prompts_alone() {
        for p in [
            "Password: ",
            "user@host's password:",
            "Username: ",
            "Do you want to continue?",
            "",
            "Duo passcode or option (1-3):",
            "Passcode or option (1-3):",
            "PIN:",
            "Enter your PIN code",
            "Approve the push on your phone",
            "Select a hotspot",
        ] {
            assert!(!is_code_prompt(p), "{p:?}");
        }
    }

    #[test]
    fn a_login_cannot_claim_the_same_period_twice() {
        let g = ReuseGuard::default();
        assert!(g.claim("a@h:22", 10));
        assert!(!g.claim("a@h:22", 10), "same period");
        assert!(!g.claim("a@h:22", 9), "an older one");
        assert!(g.claim("a@h:22", 11), "the next period is free");
        assert!(g.claim("b@h:22", 10), "another login has its own record");
        assert!(g.claim("a@other:22", 10), "so does another host");
    }

    #[test]
    fn a_second_use_in_a_period_waits_for_the_next_one() {
        let t = Totp::parse("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ").unwrap();
        let g = ReuseGuard::default();
        // 100 s is 10 s into the fourth period, 20 s from the next.
        assert_eq!(t.wait_before_use(&g, "a@h:22", 100), None);
        assert_eq!(t.wait_before_use(&g, "a@h:22", 105), Some(Duration::from_millis(15_300)));
        assert_eq!(t.wait_before_use(&g, "a@h:22", 119), Some(Duration::from_millis(1_300)));
        assert_eq!(t.wait_before_use(&g, "a@h:22", 121), None, "after the boundary the next code is free");
        assert_ne!(t.code_at(100), t.code_at(121));
    }
}
