//! Credentials entered directly in the host form (like Termius), stored as a
//! host-owned [`Identity`] so the Host/Identity separation still holds.
//!
//! The form picks one of four modes:
//!
//! * **Inline**: username plus password, SSH key (pasted, read from a file,
//!   or generated here), or ssh-agent. Stored as an identity with
//!   `for_host` set, or as a shared Keychain identity if the user asks.
//! * **Identity**: link an existing Keychain identity.
//! * **Ask**: no stored credentials; prompt at connect time.
//! * **Keep**: leave whatever the host already has.
//!
//! Secrets left empty while editing mean "keep the stored one", so the form
//! never needs to receive existing secrets. Key files and generated keys are
//! read or created here, so those private keys never reach the webview.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::keys;
use crate::models::{AuthMethod, Host, Identity};
use crate::vault::{Collection, Record, Vault, VaultError};

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum HostCredentials {
    Keep,
    Ask,
    Identity {
        identity_id: Uuid,
    },
    Inline {
        username: String,
        auth: InlineAuth,
        /// When set, store as a shared Keychain identity with this label.
        #[serde(default)]
        save_to_keychain: Option<String>,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InlineAuth {
    /// `None` or empty keeps the stored password.
    Password {
        password: Option<String>,
    },
    /// `None` or empty keeps the stored key. A `None` passphrase keeps the
    /// stored one too.
    PrivateKey {
        private_key: Option<String>,
        passphrase: Option<String>,
    },
    /// Read the key from a file on this computer.
    KeyFile {
        path: String,
        passphrase: Option<String>,
    },
    /// Generate a new Ed25519 key; its public half is returned.
    GenerateKey,
    Agent,
}

#[derive(Debug, Clone, Serialize)]
pub struct SaveOutcome {
    pub host: Record<Host>,
    /// Set when a key was generated, for the user to install on the server.
    pub public_key: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum HostCredError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("{0}")]
    Validation(String),
}

fn invalid(msg: impl Into<String>) -> HostCredError {
    HostCredError::Validation(msg.into())
}

fn non_empty(s: &Option<String>) -> Option<&str> {
    s.as_deref().filter(|v| !v.is_empty())
}

/// Check a private key decodes (with its passphrase) before storing it, so
/// typos surface in the form instead of at connect time.
fn validated_key(pem: String, passphrase: Option<String>) -> Result<AuthMethod, HostCredError> {
    keys::public_key_of(&pem, passphrase.as_deref()).map_err(|e| invalid(e.to_string()))?;
    Ok(AuthMethod::PrivateKey {
        private_key: pem,
        passphrase: passphrase.filter(|p| !p.is_empty()),
    })
}

/// Turn the form's auth into a stored [`AuthMethod`], reusing secrets from
/// `previous` where the form left them empty.
fn resolve_auth(
    auth: InlineAuth,
    previous: Option<&AuthMethod>,
    comment: &str,
) -> Result<(AuthMethod, Option<String>), HostCredError> {
    Ok(match auth {
        InlineAuth::Password { password } => match (non_empty(&password), previous) {
            (Some(p), _) => (
                AuthMethod::Password {
                    password: p.to_string(),
                },
                None,
            ),
            (None, Some(prev @ AuthMethod::Password { .. })) => (prev.clone(), None),
            (None, _) => return Err(invalid("Enter a password")),
        },
        InlineAuth::PrivateKey {
            private_key,
            passphrase,
        } => match (non_empty(&private_key), previous) {
            (Some(k), _) => (validated_key(k.to_string(), passphrase)?, None),
            (
                None,
                Some(AuthMethod::PrivateKey {
                    private_key: old_key,
                    passphrase: old_pass,
                }),
            ) => {
                // Keep the key; a newly typed passphrase replaces the old one.
                let pass = if non_empty(&passphrase).is_some() {
                    passphrase
                } else {
                    old_pass.clone()
                };
                (validated_key(old_key.clone(), pass)?, None)
            }
            (None, _) => {
                return Err(invalid(
                    "Paste a private key, load one from a file, or generate one",
                ))
            }
        },
        InlineAuth::KeyFile { path, passphrase } => {
            let pem = std::fs::read_to_string(&path)
                .map_err(|e| invalid(format!("Could not read {path}: {e}")))?;
            (validated_key(pem, passphrase)?, None)
        }
        InlineAuth::GenerateKey => {
            let k = keys::generate_ed25519(comment).map_err(|e| invalid(e.to_string()))?;
            (
                AuthMethod::PrivateKey {
                    private_key: k.private_key,
                    passphrase: None,
                },
                Some(k.public_key),
            )
        }
        InlineAuth::Agent => (AuthMethod::Agent, None),
    })
}

/// Remove a host-owned identity, unless another host still uses it (after a
/// duplicate); then it becomes an ordinary Keychain identity instead.
fn release_owned(vault: &Vault, identity_id: Uuid, host_id: Uuid) -> Result<(), VaultError> {
    let still_used = vault
        .list::<Host>(Collection::Hosts)?
        .records
        .iter()
        .any(|r| {
            r.id != host_id && r.data.as_ref().and_then(|d| d.identity_id) == Some(identity_id)
        });
    if still_used {
        let mut rec = vault.get::<Identity>(Collection::Identities, identity_id)?;
        if let Some(mut data) = rec.data.take() {
            data.for_host = None;
            vault.put(Collection::Identities, identity_id, &data)?;
        }
        Ok(())
    } else {
        vault.delete(Collection::Identities, identity_id)
    }
}

/// Save `host` under `id` (new if `None`) with the chosen credentials.
pub fn save_host_with_credentials(
    vault: &Vault,
    id: Option<Uuid>,
    mut host: Host,
    creds: HostCredentials,
) -> Result<SaveOutcome, HostCredError> {
    let host_id = id.unwrap_or_else(Uuid::new_v4);

    // What the host has now, and whether that identity belongs to it.
    let previous_identity = match id {
        Some(id) => match vault.get::<Host>(Collection::Hosts, id) {
            Ok(r) => r.data.and_then(|d| d.identity_id),
            Err(VaultError::NotFound { .. }) | Err(VaultError::Deleted { .. }) => None,
            Err(e) => return Err(e.into()),
        },
        None => None,
    };
    let owned: Option<(Uuid, Identity)> = previous_identity.and_then(|iid| {
        vault
            .get::<Identity>(Collection::Identities, iid)
            .ok()
            .and_then(|r| r.data)
            .filter(|d| d.for_host == Some(host_id))
            .map(|d| (iid, d))
    });

    let mut public_key = None;
    let mut release: Option<Uuid> = None;

    host.identity_id = match creds {
        HostCredentials::Keep => previous_identity,
        HostCredentials::Ask => {
            release = owned.map(|(iid, _)| iid);
            None
        }
        HostCredentials::Identity { identity_id } => {
            vault
                .get::<Identity>(Collection::Identities, identity_id)
                .map_err(|_| invalid("That identity no longer exists"))?;
            release = owned
                .as_ref()
                .map(|(iid, _)| *iid)
                .filter(|iid| *iid != identity_id);
            Some(identity_id)
        }
        HostCredentials::Inline {
            username,
            auth,
            save_to_keychain,
        } => {
            let username = username.trim().to_string();
            if username.is_empty() {
                return Err(invalid("Enter a username"));
            }
            let comment = format!("{username}@{}", host.label.trim());
            let (auth, pk) = resolve_auth(auth, owned.as_ref().map(|(_, d)| &d.auth), &comment)?;
            public_key = pk;

            match save_to_keychain
                .as_deref()
                .map(str::trim)
                .filter(|l| !l.is_empty())
            {
                Some(label) => {
                    let rec = vault.insert(
                        Collection::Identities,
                        &Identity {
                            label: label.to_string(),
                            username,
                            auth,
                            notes: String::new(),
                            for_host: None,
                        },
                    )?;
                    release = owned.map(|(iid, _)| iid);
                    Some(rec.id)
                }
                None => {
                    let iid = owned
                        .as_ref()
                        .map(|(iid, _)| *iid)
                        .unwrap_or_else(Uuid::new_v4);
                    vault.put(
                        Collection::Identities,
                        iid,
                        &Identity {
                            label: host.label.trim().to_string(),
                            username,
                            auth,
                            notes: owned.map(|(_, d)| d.notes).unwrap_or_default(),
                            for_host: Some(host_id),
                        },
                    )?;
                    Some(iid)
                }
            }
        }
    };

    let rec = vault.put(Collection::Hosts, host_id, &host)?;
    if let Some(iid) = release {
        release_owned(vault, iid, host_id)?;
    }
    Ok(SaveOutcome {
        host: rec,
        public_key,
    })
}

/// Clean up after a host is deleted: its own identity goes too.
pub fn release_for_deleted_host(
    vault: &Vault,
    host: &Host,
    host_id: Uuid,
) -> Result<(), VaultError> {
    let Some(iid) = host.identity_id else {
        return Ok(());
    };
    match vault.get::<Identity>(Collection::Identities, iid) {
        Ok(r) if r.data.as_ref().and_then(|d| d.for_host) == Some(host_id) => {
            release_owned(vault, iid, host_id)
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::KdfParams;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::TempDir::new().unwrap();
        let v =
            Vault::create(dir.path().join("v"), b"pw", KdfParams::insecure_for_tests()).unwrap();
        (dir, v)
    }

    fn host(label: &str) -> Host {
        Host {
            label: label.into(),
            hostname: "10.0.0.1".into(),
            port: 22,
            identity_id: None,
            jump_host_id: None,
            forward_agent: false,
            forward_x11: false,
            group: String::new(),
            tags: vec![],
            color: None,
            notes: String::new(),
        }
    }

    fn inline_pw(user: &str, pw: Option<&str>) -> HostCredentials {
        HostCredentials::Inline {
            username: user.into(),
            auth: InlineAuth::Password {
                password: pw.map(str::to_string),
            },
            save_to_keychain: None,
        }
    }

    fn identity_of(v: &Vault, h: &Record<Host>) -> Identity {
        let iid = h.data.as_ref().unwrap().identity_id.unwrap();
        v.get::<Identity>(Collection::Identities, iid)
            .unwrap()
            .data
            .unwrap()
    }

    #[test]
    fn inline_password_creates_then_updates_one_owned_identity() {
        let (_d, v) = vault();
        let out =
            save_host_with_credentials(&v, None, host("web"), inline_pw("root", Some("s3cret")))
                .unwrap();
        let id = identity_of(&v, &out.host);
        assert_eq!(id.username, "root");
        assert_eq!(id.for_host, Some(out.host.id));
        assert_eq!(
            id.auth,
            AuthMethod::Password {
                password: "s3cret".into()
            }
        );

        // Editing with an empty password keeps it; username changes apply.
        let again = save_host_with_credentials(
            &v,
            Some(out.host.id),
            host("web"),
            inline_pw("admin", None),
        )
        .unwrap();
        assert_eq!(
            again.host.data.as_ref().unwrap().identity_id,
            out.host.data.as_ref().unwrap().identity_id
        );
        let id = identity_of(&v, &again.host);
        assert_eq!(id.username, "admin");
        assert_eq!(
            id.auth,
            AuthMethod::Password {
                password: "s3cret".into()
            }
        );
        assert_eq!(
            v.list::<Identity>(Collection::Identities)
                .unwrap()
                .records
                .len(),
            1
        );
    }

    #[test]
    fn empty_secret_without_a_stored_one_is_rejected() {
        let (_d, v) = vault();
        let err = save_host_with_credentials(&v, None, host("h"), inline_pw("root", Some("")))
            .unwrap_err();
        assert!(err.to_string().contains("password"), "{err}");
        let err =
            save_host_with_credentials(&v, None, host("h"), inline_pw(" ", Some("x"))).unwrap_err();
        assert!(err.to_string().contains("username"), "{err}");
        // Switching an owned password identity to key auth needs a key.
        let out =
            save_host_with_credentials(&v, None, host("h"), inline_pw("root", Some("x"))).unwrap();
        let err = save_host_with_credentials(
            &v,
            Some(out.host.id),
            host("h"),
            HostCredentials::Inline {
                username: "root".into(),
                auth: InlineAuth::PrivateKey {
                    private_key: None,
                    passphrase: None,
                },
                save_to_keychain: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("private key"), "{err}");
    }

    #[test]
    fn generated_and_file_keys_are_stored_and_validated() {
        let (d, v) = vault();
        let out = save_host_with_credentials(
            &v,
            None,
            host("db"),
            HostCredentials::Inline {
                username: "ops".into(),
                auth: InlineAuth::GenerateKey,
                save_to_keychain: None,
            },
        )
        .unwrap();
        let pk = out.public_key.clone().unwrap();
        assert!(pk.starts_with("ssh-ed25519 ") && pk.ends_with("ops@db"));
        let AuthMethod::PrivateKey { private_key, .. } = identity_of(&v, &out.host).auth else {
            panic!("expected key auth");
        };

        // Keep the stored key when the field is left empty.
        let kept = save_host_with_credentials(
            &v,
            Some(out.host.id),
            host("db"),
            HostCredentials::Inline {
                username: "ops".into(),
                auth: InlineAuth::PrivateKey {
                    private_key: None,
                    passphrase: None,
                },
                save_to_keychain: None,
            },
        )
        .unwrap();
        assert!(
            matches!(identity_of(&v, &kept.host).auth, AuthMethod::PrivateKey { private_key: ref k, .. } if *k == private_key)
        );

        // Load from file; a garbage file is rejected with a clear error.
        let path = d.path().join("id_test");
        std::fs::write(&path, &private_key).unwrap();
        let from_file = save_host_with_credentials(
            &v,
            None,
            host("db2"),
            HostCredentials::Inline {
                username: "ops".into(),
                auth: InlineAuth::KeyFile {
                    path: path.to_string_lossy().into(),
                    passphrase: None,
                },
                save_to_keychain: None,
            },
        )
        .unwrap();
        assert!(matches!(
            identity_of(&v, &from_file.host).auth,
            AuthMethod::PrivateKey { .. }
        ));
        std::fs::write(&path, "junk").unwrap();
        let err = save_host_with_credentials(
            &v,
            None,
            host("db3"),
            HostCredentials::Inline {
                username: "ops".into(),
                auth: InlineAuth::KeyFile {
                    path: path.to_string_lossy().into(),
                    passphrase: None,
                },
                save_to_keychain: None,
            },
        )
        .unwrap_err();
        assert!(err.to_string().contains("private key"), "{err}");
    }

    #[test]
    fn switching_modes_releases_the_owned_identity() {
        let (_d, v) = vault();
        let shared = v
            .insert(
                Collection::Identities,
                &Identity {
                    label: "deploy".into(),
                    username: "deploy".into(),
                    auth: AuthMethod::Agent,
                    notes: String::new(),
                    for_host: None,
                },
            )
            .unwrap();
        let out =
            save_host_with_credentials(&v, None, host("h"), inline_pw("root", Some("x"))).unwrap();
        let owned = out.host.data.as_ref().unwrap().identity_id.unwrap();

        let linked = save_host_with_credentials(
            &v,
            Some(out.host.id),
            host("h"),
            HostCredentials::Identity {
                identity_id: shared.id,
            },
        )
        .unwrap();
        assert_eq!(
            linked.host.data.as_ref().unwrap().identity_id,
            Some(shared.id)
        );
        assert!(matches!(
            v.get::<Identity>(Collection::Identities, owned),
            Err(VaultError::Deleted { .. })
        ));

        let asked =
            save_host_with_credentials(&v, Some(out.host.id), host("h"), HostCredentials::Ask)
                .unwrap();
        assert_eq!(asked.host.data.as_ref().unwrap().identity_id, None);
        // The shared identity is never deleted by host edits.
        assert!(v.get::<Identity>(Collection::Identities, shared.id).is_ok());

        // Keep leaves things as they are.
        let kept =
            save_host_with_credentials(&v, Some(out.host.id), host("h"), HostCredentials::Keep)
                .unwrap();
        assert_eq!(kept.host.data.as_ref().unwrap().identity_id, None);
    }

    #[test]
    fn save_to_keychain_makes_a_shared_identity() {
        let (_d, v) = vault();
        let out = save_host_with_credentials(
            &v,
            None,
            host("h"),
            HostCredentials::Inline {
                username: "root".into(),
                auth: InlineAuth::Password {
                    password: Some("x".into()),
                },
                save_to_keychain: Some("Root password".into()),
            },
        )
        .unwrap();
        let id = identity_of(&v, &out.host);
        assert_eq!(id.label, "Root password");
        assert_eq!(id.for_host, None);
    }

    #[test]
    fn deleting_a_host_removes_its_own_identity_unless_shared_by_a_duplicate() {
        let (_d, v) = vault();
        let a =
            save_host_with_credentials(&v, None, host("a"), inline_pw("root", Some("x"))).unwrap();
        let iid = a.host.data.as_ref().unwrap().identity_id.unwrap();
        // A duplicate of `a` points at the same owned identity.
        let mut dup = host("a (copy)");
        dup.identity_id = Some(iid);
        let b = v.insert(Collection::Hosts, &dup).unwrap();

        release_for_deleted_host(&v, a.host.data.as_ref().unwrap(), a.host.id).unwrap();
        v.delete(Collection::Hosts, a.host.id).unwrap();
        // Still used by the duplicate: kept, but no longer owned by `a`.
        let kept = v
            .get::<Identity>(Collection::Identities, iid)
            .unwrap()
            .data
            .unwrap();
        assert_eq!(kept.for_host, None);

        // A host with its own identity and no duplicates takes it along.
        let c = save_host_with_credentials(&v, None, host("c"), inline_pw("u", Some("p"))).unwrap();
        let ciid = c.host.data.as_ref().unwrap().identity_id.unwrap();
        release_for_deleted_host(&v, c.host.data.as_ref().unwrap(), c.host.id).unwrap();
        assert!(matches!(
            v.get::<Identity>(Collection::Identities, ciid),
            Err(VaultError::Deleted { .. })
        ));
        let _ = b;
    }
}
