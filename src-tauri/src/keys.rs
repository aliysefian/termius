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
    fn garbage_is_an_error() {
        assert!(matches!(
            public_key_of("not a key", None),
            Err(KeyError::Decode(_))
        ));
    }
}
