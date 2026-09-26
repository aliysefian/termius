//! Cryptographic core: Argon2id key derivation and XChaCha20-Poly1305 AEAD.
//!
//! Everything that leaves this process and lands in the synced folder passes
//! through [`encrypt`]. The on-disk envelope is:
//!
//! ```text
//! +-------+---------+----------------+------------------------+
//! | magic | version | nonce (24 B)   | ciphertext || tag (16) |
//! | "SVLT"| 0x01    |                |                        |
//! +-------+---------+----------------+------------------------+
//! ```
//!
//! Associated data (AAD) is supplied by the caller and is authenticated but not
//! stored. The vault layer binds `"<collection>/<uuid>"` as AAD so a ciphertext
//! cannot be renamed or moved into another collection without detection.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Length of the symmetric key in bytes (XChaCha20-Poly1305 requires 32).
pub const KEY_LEN: usize = 32;
/// Length of the Argon2id salt in bytes.
pub const SALT_LEN: usize = 16;
/// Length of the XChaCha20 nonce in bytes (192-bit, safe for random generation).
pub const NONCE_LEN: usize = 24;
/// Poly1305 authentication tag length.
pub const TAG_LEN: usize = 16;

const MAGIC: &[u8; 4] = b"SVLT";
const FORMAT_VERSION: u8 = 1;
const HEADER_LEN: usize = MAGIC.len() + 1 + NONCE_LEN;

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("invalid Argon2 parameters: {0}")]
    InvalidKdfParams(String),

    #[error("key derivation failed: {0}")]
    KeyDerivation(String),

    #[error("salt must be exactly {SALT_LEN} bytes, got {0}")]
    InvalidSaltLength(usize),

    #[error("ciphertext is too short to contain a valid envelope")]
    TruncatedEnvelope,

    #[error("ciphertext does not carry the expected magic header")]
    BadMagic,

    #[error("unsupported envelope format version {0}")]
    UnsupportedVersion(u8),

    /// Returned for a wrong key, wrong AAD, or any bit-flip in the ciphertext.
    /// The AEAD deliberately does not distinguish between these cases.
    #[error("authentication failed: wrong key or corrupted data")]
    AuthenticationFailed,
}

/// Tunable Argon2id cost parameters. Persisted in the vault manifest so every
/// machine derives an identical key from the same password and salt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    /// Memory cost in KiB.
    pub m_cost_kib: u32,
    /// Number of iterations (time cost).
    pub t_cost: u32,
    /// Degree of parallelism (lanes).
    pub p_cost: u32,
}

impl Default for KdfParams {
    /// 64 MiB, 3 passes, 4 lanes. Roughly the OWASP / RFC 9106 "second
    /// recommended" profile; takes on the order of 100-300 ms on a desktop CPU.
    fn default() -> Self {
        Self {
            m_cost_kib: 64 * 1024,
            t_cost: 3,
            p_cost: 4,
        }
    }
}

impl KdfParams {
    /// Deliberately weak parameters for unit tests. Never use in production.
    #[cfg(test)]
    pub fn insecure_for_tests() -> Self {
        Self {
            m_cost_kib: 8,
            t_cost: 1,
            p_cost: 1,
        }
    }

    fn to_argon2(self) -> Result<Params, CryptoError> {
        Params::new(self.m_cost_kib, self.t_cost, self.p_cost, Some(KEY_LEN))
            .map_err(|e| CryptoError::InvalidKdfParams(e.to_string()))
    }
}

/// The symmetric master key. Zeroed on drop, and its `Debug` output never
/// reveals key material.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct MasterKey([u8; KEY_LEN]);

impl MasterKey {
    /// Wrap raw key bytes. Prefer [`derive_key`] or [`MasterKey::generate`].
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    /// Generate a fresh random key from the OS CSPRNG.
    pub fn generate() -> Self {
        let mut bytes = [0u8; KEY_LEN];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Borrow the raw key bytes. Callers must not copy these into long-lived
    /// non-zeroizing storage.
    pub fn as_bytes(&self) -> &[u8; KEY_LEN] {
        &self.0
    }

    /// Constant-time equality, so comparing keys does not leak timing.
    pub fn ct_eq(&self, other: &MasterKey) -> bool {
        let mut diff = 0u8;
        for (a, b) in self.0.iter().zip(other.0.iter()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey([REDACTED])")
    }
}

/// Generate a random Argon2id salt.
pub fn generate_salt() -> [u8; SALT_LEN] {
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);
    salt
}

/// Derive the master key from a password and salt with Argon2id.
///
/// The password is taken as bytes so the caller can zeroize its own buffer;
/// this function does not retain it.
pub fn derive_key(
    password: &[u8],
    salt: &[u8],
    params: KdfParams,
) -> Result<MasterKey, CryptoError> {
    if salt.len() != SALT_LEN {
        return Err(CryptoError::InvalidSaltLength(salt.len()));
    }

    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params.to_argon2()?);

    let mut out = [0u8; KEY_LEN];
    argon
        .hash_password_into(password, salt, &mut out)
        .map_err(|e| CryptoError::KeyDerivation(e.to_string()))?;

    Ok(MasterKey(out))
}

/// Encrypt `plaintext` under `key`, authenticating `aad` alongside it.
///
/// A fresh random 192-bit nonce is generated per call, which makes nonce reuse
/// statistically impossible even across many machines with no coordination.
pub fn encrypt(key: &MasterKey, plaintext: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());

    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = XNonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)?;

    let mut out = Vec::with_capacity(HEADER_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt an envelope produced by [`encrypt`]. `aad` must match exactly what
/// was supplied at encryption time.
pub fn decrypt(key: &MasterKey, envelope: &[u8], aad: &[u8]) -> Result<Vec<u8>, CryptoError> {
    if envelope.len() < HEADER_LEN + TAG_LEN {
        return Err(CryptoError::TruncatedEnvelope);
    }
    if &envelope[..MAGIC.len()] != MAGIC {
        return Err(CryptoError::BadMagic);
    }
    let version = envelope[MAGIC.len()];
    if version != FORMAT_VERSION {
        return Err(CryptoError::UnsupportedVersion(version));
    }

    let nonce = XNonce::from_slice(&envelope[MAGIC.len() + 1..HEADER_LEN]);
    let ciphertext = &envelope[HEADER_LEN..];

    let cipher = XChaCha20Poly1305::new(key.as_bytes().into());
    cipher
        .decrypt(
            nonce,
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| CryptoError::AuthenticationFailed)
}

/// Returns `true` if `data` starts with a valid envelope header. Useful for the
/// file watcher to ignore stray files without attempting a full decrypt.
pub fn looks_like_envelope(data: &[u8]) -> bool {
    data.len() >= HEADER_LEN + TAG_LEN
        && &data[..MAGIC.len()] == MAGIC
        && data[MAGIC.len()] == FORMAT_VERSION
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_key() -> MasterKey {
        derive_key(
            b"correct horse battery staple",
            &[7u8; SALT_LEN],
            KdfParams::insecure_for_tests(),
        )
        .unwrap()
    }

    #[test]
    fn kdf_is_deterministic_for_same_inputs() {
        let a = test_key();
        let b = test_key();
        assert!(a.ct_eq(&b));
    }

    #[test]
    fn kdf_changes_with_salt_and_password() {
        let p = KdfParams::insecure_for_tests();
        let base = derive_key(b"pw", &[1u8; SALT_LEN], p).unwrap();
        let other_salt = derive_key(b"pw", &[2u8; SALT_LEN], p).unwrap();
        let other_pw = derive_key(b"pw2", &[1u8; SALT_LEN], p).unwrap();
        assert!(!base.ct_eq(&other_salt));
        assert!(!base.ct_eq(&other_pw));
    }

    #[test]
    fn kdf_rejects_bad_salt_length() {
        let err = derive_key(b"pw", &[0u8; 5], KdfParams::insecure_for_tests()).unwrap_err();
        assert!(matches!(err, CryptoError::InvalidSaltLength(5)));
    }

    #[test]
    fn kdf_rejects_bad_params() {
        let bad = KdfParams {
            m_cost_kib: 1,
            t_cost: 0,
            p_cost: 0,
        };
        assert!(matches!(
            derive_key(b"pw", &[0u8; SALT_LEN], bad),
            Err(CryptoError::InvalidKdfParams(_))
        ));
    }

    #[test]
    fn roundtrip() {
        let key = test_key();
        let msg = b"{\"hostname\":\"10.0.0.1\"}";
        let env = encrypt(&key, msg, b"hosts/abc").unwrap();
        assert!(looks_like_envelope(&env));
        assert_eq!(env.len(), HEADER_LEN + msg.len() + TAG_LEN);
        let pt = decrypt(&key, &env, b"hosts/abc").unwrap();
        assert_eq!(pt, msg);
    }

    #[test]
    fn roundtrip_empty_plaintext() {
        let key = test_key();
        let env = encrypt(&key, b"", b"").unwrap();
        assert_eq!(decrypt(&key, &env, b"").unwrap(), b"");
    }

    #[test]
    fn nonces_are_unique_per_call() {
        let key = test_key();
        let a = encrypt(&key, b"same", b"").unwrap();
        let b = encrypt(&key, b"same", b"").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn wrong_key_fails() {
        let key = test_key();
        let env = encrypt(&key, b"secret", b"aad").unwrap();
        let other = MasterKey::generate();
        assert!(matches!(
            decrypt(&other, &env, b"aad"),
            Err(CryptoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn wrong_aad_fails() {
        let key = test_key();
        let env = encrypt(&key, b"secret", b"hosts/1").unwrap();
        assert!(matches!(
            decrypt(&key, &env, b"identities/1"),
            Err(CryptoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = test_key();
        let mut env = encrypt(&key, b"secret", b"").unwrap();
        let last = env.len() - 1;
        env[last] ^= 0x01;
        assert!(matches!(
            decrypt(&key, &env, b""),
            Err(CryptoError::AuthenticationFailed)
        ));
    }

    #[test]
    fn malformed_envelopes_are_rejected() {
        let key = test_key();
        assert!(matches!(
            decrypt(&key, b"short", b""),
            Err(CryptoError::TruncatedEnvelope)
        ));

        let mut env = encrypt(&key, b"x", b"").unwrap();
        env[0] = b'X';
        assert!(matches!(
            decrypt(&key, &env, b""),
            Err(CryptoError::BadMagic)
        ));

        let mut env = encrypt(&key, b"x", b"").unwrap();
        env[4] = 99;
        assert!(matches!(
            decrypt(&key, &env, b""),
            Err(CryptoError::UnsupportedVersion(99))
        ));
    }

    #[test]
    fn debug_does_not_leak_key() {
        let key = test_key();
        assert_eq!(format!("{key:?}"), "MasterKey([REDACTED])");
    }
}
