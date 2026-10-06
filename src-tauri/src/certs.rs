//! Server certificates that are trusted by pinning, like SSH host keys: a
//! certificate is accepted when its SHA-256 fingerprint matches the one saved
//! for the host, or when the person has just said to trust that exact one.
//! Nothing checks a certificate authority; these are for servers (Remote
//! Desktop, FTP) that mostly have self-signed certificates.

use serde::Serialize;
use sha2::{Digest, Sha256};
use x509_cert::der::{Decode, Encode};

/// What a server's certificate looks like, for the person to decide on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CertInfo {
    /// SHA-256 of the certificate, as 64 lowercase hex digits.
    pub fingerprint: String,
    pub subject: String,
    pub issuer: String,
    pub not_before: String,
    pub not_after: String,
}

pub fn fingerprint(der: &[u8]) -> String {
    Sha256::digest(der).iter().map(|b| format!("{b:02x}")).collect()
}

pub fn cert_info_from_der(der: &[u8]) -> Result<CertInfo, String> {
    let cert = x509_cert::Certificate::from_der(der).map_err(|e| format!("unreadable server certificate: {e}"))?;
    Ok(describe(&cert, fingerprint(der)))
}

pub fn cert_info(cert: &x509_cert::Certificate) -> Result<CertInfo, String> {
    let der = cert.to_der().map_err(|e| format!("unreadable server certificate: {e}"))?;
    Ok(describe(cert, fingerprint(&der)))
}

fn describe(cert: &x509_cert::Certificate, fingerprint: String) -> CertInfo {
    let validity = &cert.tbs_certificate.validity;
    CertInfo {
        fingerprint,
        subject: cert.tbs_certificate.subject.to_string(),
        issuer: cert.tbs_certificate.issuer.to_string(),
        not_before: validity.not_before.to_date_time().to_string(),
        not_after: validity.not_after.to_date_time().to_string(),
    }
}

/// What is already known about a server's certificate.
#[derive(Debug, Clone, Default)]
pub struct CertCheck {
    /// The fingerprint trusted before, if any.
    pub pinned: Option<String>,
    /// A fingerprint the person has just accepted, for a new or changed certificate.
    pub accept: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Trusted,
    /// Never seen: ask.
    Unknown,
    /// Not the one trusted before: ask, showing both.
    Changed { expected: String },
}

impl CertCheck {
    /// Whether to go ahead with a server presenting the certificate with this fingerprint.
    pub fn verdict(&self, fingerprint: &str) -> Verdict {
        let accepted = self.accept.as_deref() == Some(fingerprint);
        match &self.pinned {
            Some(p) if p == fingerprint => Verdict::Trusted,
            Some(p) if !accepted => Verdict::Changed { expected: p.clone() },
            None if !accepted => Verdict::Unknown,
            _ => Verdict::Trusted,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_certificate_waits_to_be_accepted() {
        assert_eq!(CertCheck::default().verdict("aa"), Verdict::Unknown);
        // Accepting a different fingerprint doesn't let this one through.
        assert_eq!(CertCheck { pinned: None, accept: Some("bb".into()) }.verdict("aa"), Verdict::Unknown);
        assert_eq!(CertCheck { pinned: None, accept: Some("aa".into()) }.verdict("aa"), Verdict::Trusted);
    }

    #[test]
    fn a_pinned_certificate_must_match_and_a_change_needs_a_fresh_accept() {
        let pinned = CertCheck { pinned: Some("aa".into()), accept: None };
        assert_eq!(pinned.verdict("aa"), Verdict::Trusted);
        assert_eq!(pinned.verdict("bb"), Verdict::Changed { expected: "aa".into() });
        // Accepting the old one doesn't help a changed certificate; accepting the new one does.
        assert_eq!(CertCheck { pinned: Some("aa".into()), accept: Some("aa".into()) }.verdict("bb"), Verdict::Changed { expected: "aa".into() });
        assert_eq!(CertCheck { pinned: Some("aa".into()), accept: Some("bb".into()) }.verdict("bb"), Verdict::Trusted);
    }

    #[test]
    fn fingerprints_are_lowercase_hex_sha256() {
        assert_eq!(fingerprint(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert!(cert_info_from_der(b"not a certificate").is_err());
    }
}
