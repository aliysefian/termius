//! SSH key utilities: generate Ed25519 keys and derive public keys from
//! stored private keys (for pasting into `authorized_keys`).

use rand::{rngs::OsRng, RngCore};
use russh::keys::ssh_key::private::{Ed25519Keypair, KeypairData};
use russh::keys::ssh_key::{HashAlg, LineEnding, PrivateKey};
use serde::Serialize;
use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    #[error("could not read private key: {0}")]
    Decode(String),
    #[error("key encoding failed: {0}")]
    Encode(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct GeneratedKey {
    /// OpenSSH PEM, unencrypted (it is stored inside the encrypted vault).
    pub private_key: String,
    /// One `authorized_keys` line.
    pub public_key: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PublicKeyInfo {
    pub public_key: String,
    pub fingerprint: String,
    pub algorithm: String,
}

/// Generate a fresh Ed25519 key pair from the OS CSPRNG.
pub fn generate_ed25519(comment: &str) -> Result<GeneratedKey, KeyError> {
    let mut seed = Zeroizing::new([0u8; 32]);
    OsRng.fill_bytes(seed.as_mut());
    let pair = Ed25519Keypair::from_seed(&seed);
    let key = PrivateKey::new(KeypairData::Ed25519(pair), comment)
        .map_err(|e| KeyError::Encode(e.to_string()))?;
    let private_key = key
        .to_openssh(LineEnding::LF)
        .map_err(|e| KeyError::Encode(e.to_string()))?
        .to_string();
    let public = key.public_key();
    Ok(GeneratedKey {
        private_key,
        public_key: public
            .to_openssh()
            .map_err(|e| KeyError::Encode(e.to_string()))?,
        fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
    })
}

/// Derive the public half of a stored private key (any format russh reads:
/// OpenSSH, PEM, PKCS#8, PuTTY).
pub fn public_key_of(
    private_key: &str,
    passphrase: Option<&str>,
) -> Result<PublicKeyInfo, KeyError> {
    let key = russh::keys::decode_secret_key(private_key, passphrase)
        .map_err(|e| KeyError::Decode(e.to_string()))?;
    let public = key.public_key();
    Ok(PublicKeyInfo {
        public_key: public
            .to_openssh()
            .map_err(|e| KeyError::Encode(e.to_string()))?,
        fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        algorithm: key.algorithm().to_string(),
    })
}

// ---------------------------------------------------------------------------
// Key Manager
// ---------------------------------------------------------------------------

/// Algorithms the Key Manager can generate. Ed25519 is the default; RSA and
/// ECDSA exist for servers that don't accept Ed25519.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyAlgorithm {
    Ed25519,
    EcdsaP256,
    EcdsaP384,
    Rsa3072,
    Rsa4096,
}

/// Everything the Key Manager records about a key.
/// Not `Serialize`: the private half must never reach the UI by accident.
#[derive(Debug, Clone)]
pub struct KeyMaterial {
    /// OpenSSH text; `None` for public-only keys.
    pub private_key: Option<Zeroizing<String>>,
    pub public_key: String,
    pub fingerprint: String,
    pub algorithm: String,
    pub encrypted: bool,
    pub comment: String,
}

/// The OS CSPRNG in the `rand_core` flavour `ssh-key` expects.
fn os_rng() -> russh::keys::ssh_key::rand_core::UnwrapErr<getrandom::SysRng> {
    russh::keys::ssh_key::rand_core::UnwrapErr(getrandom::SysRng)
}

fn encode_err(e: impl std::fmt::Display) -> KeyError {
    KeyError::Encode(e.to_string())
}

fn material(key: &PrivateKey, text: Option<Zeroizing<String>>) -> Result<KeyMaterial, KeyError> {
    let public = key.public_key();
    Ok(KeyMaterial {
        private_key: text,
        public_key: public.to_openssh().map_err(encode_err)?,
        fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        algorithm: key.algorithm().to_string(),
        encrypted: key.is_encrypted(),
        comment: key.comment().to_string(),
    })
}

/// Generate a key, optionally encrypted with `passphrase` (OpenSSH format,
/// bcrypt-pbkdf + AES-256-CTR, as `ssh-keygen` does).
pub fn generate(algorithm: KeyAlgorithm, comment: &str, passphrase: Option<&str>) -> Result<KeyMaterial, KeyError> {
    use russh::keys::ssh_key::private::{EcdsaKeypair, RsaKeypair};
    use russh::keys::ssh_key::EcdsaCurve;
    let mut rng = os_rng();
    let data = match algorithm {
        KeyAlgorithm::Ed25519 => KeypairData::Ed25519(Ed25519Keypair::random(&mut rng)),
        KeyAlgorithm::EcdsaP256 => KeypairData::Ecdsa(EcdsaKeypair::random(&mut rng, EcdsaCurve::NistP256).map_err(encode_err)?),
        KeyAlgorithm::EcdsaP384 => KeypairData::Ecdsa(EcdsaKeypair::random(&mut rng, EcdsaCurve::NistP384).map_err(encode_err)?),
        KeyAlgorithm::Rsa3072 => KeypairData::Rsa(RsaKeypair::random(&mut rng, 3072).map_err(encode_err)?),
        KeyAlgorithm::Rsa4096 => KeypairData::Rsa(RsaKeypair::random(&mut rng, 4096).map_err(encode_err)?),
    };
    let mut key = PrivateKey::new(data, comment).map_err(encode_err)?;
    if let Some(p) = passphrase.filter(|p| !p.is_empty()) {
        key = key.encrypt(&mut rng, p).map_err(encode_err)?;
    }
    let text = key.to_openssh(LineEnding::LF).map_err(encode_err)?;
    let mut m = material(&key, Some(Zeroizing::new(text.to_string())))?;
    // An encrypted key hides its comment; keep the one we were given.
    m.comment = comment.to_string();
    Ok(m)
}

/// Inspect a private key the user imported. OpenSSH keys (encrypted or not)
/// reveal their public half without the passphrase; other formats (PEM,
/// PKCS#8, PuTTY) need it if they're encrypted. When a passphrase is given
/// it's checked, so a typo is caught at import instead of at connect time.
pub fn inspect_private(text: &str, passphrase: Option<&str>) -> Result<KeyMaterial, KeyError> {
    let text = text.trim();
    let passphrase = passphrase.filter(|p| !p.is_empty());
    if let Ok(key) = PrivateKey::from_openssh(text) {
        if key.is_encrypted() {
            if let Some(p) = passphrase {
                key.decrypt(p).map_err(|_| KeyError::Decode("the passphrase is wrong".into()))?;
            }
        }
        return material(&key, Some(Zeroizing::new(format!("{text}\n"))));
    }
    let key = russh::keys::decode_secret_key(text, passphrase).map_err(|e| match e {
        russh::keys::Error::KeyIsEncrypted => KeyError::Decode("this key is encrypted; enter its passphrase".into()),
        e => KeyError::Decode(e.to_string()),
    })?;
    let mut m = material(&key, Some(Zeroizing::new(format!("{text}\n"))))?;
    // The original text stays as the user gave it (it may be encrypted PEM).
    m.encrypted = passphrase.is_some();
    Ok(m)
}

/// A public-only entry, e.g. a colleague's key to add to servers.
pub fn inspect_public(line: &str) -> Result<KeyMaterial, KeyError> {
    let key = russh::keys::ssh_key::PublicKey::from_openssh(line.trim()).map_err(|e| KeyError::Decode(e.to_string()))?;
    Ok(KeyMaterial {
        private_key: None,
        public_key: key.to_openssh().map_err(encode_err)?,
        fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
        algorithm: key.algorithm().to_string(),
        encrypted: false,
        comment: key.comment().to_string(),
    })
}

/// Re-encrypt a private key under a new passphrase (or none). The result is
/// OpenSSH format regardless of the input format.
pub fn change_passphrase(text: &str, old: Option<&str>, new: Option<&str>) -> Result<Zeroizing<String>, KeyError> {
    let key = russh::keys::decode_secret_key(text.trim(), old.filter(|p| !p.is_empty()))
        .map_err(|_| KeyError::Decode("the current passphrase is wrong".into()))?;
    let key = match new.filter(|p| !p.is_empty()) {
        Some(p) => key.encrypt(&mut os_rng(), p).map_err(encode_err)?,
        None => key,
    };
    Ok(Zeroizing::new(key.to_openssh(LineEnding::LF).map_err(encode_err)?.to_string()))
}

/// Check an OpenSSH certificate belongs to `public_key`.
pub fn check_certificate(certificate: &str, public_key: &str) -> Result<String, KeyError> {
    let cert = russh::keys::Certificate::from_openssh(certificate.trim()).map_err(|e| KeyError::Decode(e.to_string()))?;
    let key = russh::keys::ssh_key::PublicKey::from_openssh(public_key.trim()).map_err(|e| KeyError::Decode(e.to_string()))?;
    if cert.public_key() != key.key_data() {
        return Err(KeyError::Decode("the certificate was issued for a different key".into()));
    }
    Ok(format!(
        "{} certificate \"{}\" for {}",
        match cert.cert_type() {
            russh::keys::ssh_key::certificate::CertType::User => "user",
            russh::keys::ssh_key::certificate::CertType::Host => "host",
        },
        cert.key_id(),
        cert.valid_principals().join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_then_derive_roundtrip() {
        let k = generate_ed25519("me@laptop").unwrap();
        assert!(k
            .private_key
            .starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        assert!(k.public_key.starts_with("ssh-ed25519 "));
        assert!(k.public_key.ends_with(" me@laptop"));
        assert!(k.fingerprint.starts_with("SHA256:"));

        let info = public_key_of(&k.private_key, None).unwrap();
        assert_eq!(info.public_key, k.public_key);
        assert_eq!(info.fingerprint, k.fingerprint);
        assert_eq!(info.algorithm, "ssh-ed25519");

        // Two generations never collide.
        assert_ne!(generate_ed25519("").unwrap().private_key, k.private_key);
    }

    #[test]
    fn every_algorithm_generates_and_reimports() {
        for alg in [KeyAlgorithm::Ed25519, KeyAlgorithm::EcdsaP256, KeyAlgorithm::EcdsaP384, KeyAlgorithm::Rsa3072] {
            let k = generate(alg, "t@x", None).unwrap();
            let again = inspect_private(k.private_key.as_ref().unwrap(), None).unwrap();
            assert_eq!(again.fingerprint, k.fingerprint, "{alg:?}");
            assert!(!k.encrypted);
        }
    }

    #[test]
    fn passphrases_protect_change_and_are_checked() {
        let k = generate(KeyAlgorithm::Ed25519, "t@x", Some("old-pass")).unwrap();
        let pem = k.private_key.clone().unwrap();
        assert!(k.encrypted);
        assert!(pem.contains("OPENSSH PRIVATE KEY"));
        // Public half is readable without the passphrase; a wrong one is caught.
        assert_eq!(inspect_private(&pem, None).unwrap().fingerprint, k.fingerprint);
        assert!(inspect_private(&pem, Some("nope")).is_err());
        // Change it, then remove it.
        let new = change_passphrase(&pem, Some("old-pass"), Some("new-pass")).unwrap();
        assert!(change_passphrase(&new, Some("old-pass"), None).is_err());
        let plain = change_passphrase(&new, Some("new-pass"), None).unwrap();
        let m = inspect_private(&plain, None).unwrap();
        assert!(!m.encrypted);
        assert_eq!(m.fingerprint, k.fingerprint);
        // russh can use the encrypted key with its passphrase.
        assert!(russh::keys::decode_secret_key(&new, Some("new-pass")).is_ok());
    }

    #[test]
    fn public_only_import() {
        let k = generate(KeyAlgorithm::Ed25519, "friend@x", None).unwrap();
        let p = inspect_public(&k.public_key).unwrap();
        assert!(p.private_key.is_none());
        assert_eq!(p.fingerprint, k.fingerprint);
        assert!(inspect_public("ssh-ed25519 garbage").is_err());
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(matches!(
            public_key_of("not a key", None),
            Err(KeyError::Decode(_))
        ));
    }
}
