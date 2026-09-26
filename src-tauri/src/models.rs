//! Domain types stored in the vault. These are the *payloads* of
//! [`crate::vault::Record`]; the record wrapper supplies `id` / `updated_at`.
//!
//! A `Host` never embeds credentials. It references an `Identity` by UUID, so
//! one key or password can be shared by many hosts and rotated in one place.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How to authenticate when connecting with an [`Identity`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Plain password authentication.
    Password { password: String },
    /// Private key in PEM / OpenSSH format, optionally passphrase-protected.
    PrivateKey {
        private_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase: Option<String>,
    },
    /// Defer to a running ssh-agent on the local machine.
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub label: String,
    pub username: String,
    pub auth: AuthMethod,
    #[serde(default)]
    pub notes: String,
    /// Set when the credentials were entered in a host's own form rather
    /// than created in the Keychain. Such an identity belongs to that host:
    /// it is updated from the host form and removed with the host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_host: Option<Uuid>,
}

impl Identity {
    /// A copy safe to hand to the webview for lists and live updates: the
    /// auth *type* survives, the password / key / passphrase do not.
    pub fn redacted(&self) -> Self {
        let auth = match &self.auth {
            AuthMethod::Password { .. } => AuthMethod::Password {
                password: String::new(),
            },
            AuthMethod::PrivateKey { .. } => AuthMethod::PrivateKey {
                private_key: String::new(),
                passphrase: None,
            },
            AuthMethod::Agent => AuthMethod::Agent,
        };
        Self {
            auth,
            ..self.clone()
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Host {
    pub label: String,
    pub hostname: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// UUID of the [`Identity`] to authenticate with. `None` means the user
    /// will be prompted at connect time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_id: Option<Uuid>,
    /// Slash-separated group path such as `"Production/Databases"`. The host
    /// tree is derived from this, so groups need no records of their own.
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Host to tunnel through first, like OpenSSH `ProxyJump`. The jump host
    /// may have its own jump, forming a chain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jump_host_id: Option<Uuid>,
    /// Forward this computer's ssh-agent to the host (`ssh -A`). Anyone with
    /// root on the host can use the agent while the session is open.
    #[serde(default)]
    pub forward_agent: bool,
    /// Show the host's graphical programs on this computer (`ssh -X`).
    #[serde(default)]
    pub forward_x11: bool,
    /// Deployment environment: "production", "staging", "development", or
    /// empty. Production hosts get warnings before bulk actions.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub environment: String,
    /// Typed into the shell right after connecting, e.g. `sudo -i` or
    /// `cd /srv/app && tmux attach`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub startup_command: String,
    /// Optional accent color as a CSS hex string, e.g. `"#7B61FF"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub notes: String,
}

fn default_port() -> u16 {
    22
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    pub label: String,
    pub command: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JumpChainError {
    #[error("jump chain loops back to a host already in the chain")]
    Loop,
    #[error("jump host {0} no longer exists")]
    Missing(Uuid),
    #[error("jump chain is longer than {0} hops")]
    TooLong(usize),
}

/// Walk the jump chain starting at `host_id`, whose own jump is `first`.
/// `lookup(id)` returns `Some(jump_of_id)` for an existing host or `None` if
/// it is missing. Returns jump host ids nearest-first (the last element is
/// the one dialled directly).
pub fn jump_chain(
    host_id: Option<Uuid>,
    first: Option<Uuid>,
    max: usize,
    lookup: impl Fn(Uuid) -> Option<Option<Uuid>>,
) -> Result<Vec<Uuid>, JumpChainError> {
    let mut chain = Vec::new();
    let mut seen: std::collections::HashSet<Uuid> = host_id.into_iter().collect();
    let mut next = first;
    while let Some(id) = next {
        if !seen.insert(id) {
            return Err(JumpChainError::Loop);
        }
        if chain.len() == max {
            return Err(JumpChainError::TooLong(max));
        }
        chain.push(id);
        next = lookup(id).ok_or(JumpChainError::Missing(id))?;
    }
    Ok(chain)
}

/// A saved port-forwarding rule, bound to a host (and through it, an identity).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardRule {
    pub label: String,
    pub host_id: Uuid,
    /// Start this rule automatically whenever the vault is unlocked.
    #[serde(default)]
    pub auto_start: bool,
    #[serde(flatten)]
    pub kind: ForwardKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ForwardKind {
    /// `ssh -L bind_addr:bind_port:dest_host:dest_port`: listen locally,
    /// connect onward from the server.
    Local {
        bind_addr: String,
        bind_port: u16,
        dest_host: String,
        dest_port: u16,
    },
    /// `ssh -R bind_addr:bind_port:dest_host:dest_port`: the server listens,
    /// connections come back and are dialled from this machine.
    Remote {
        bind_addr: String,
        bind_port: u16,
        dest_host: String,
        dest_port: u16,
    },
    /// `ssh -D bind_addr:bind_port`: local SOCKS5 proxy through the server.
    Dynamic { bind_addr: String, bind_port: u16 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_defaults_fill_in_when_missing() {
        let h: Host = serde_json::from_str(r#"{"label":"a","hostname":"b"}"#).unwrap();
        assert_eq!(h.port, 22);
        assert!(h.identity_id.is_none());
        assert_eq!(h.group, "");
    }

    #[test]
    fn jump_chain_walks_detects_loops_and_missing() {
        let (a, b, c, d) = (
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
        );
        // a -> b -> c (c dials directly)
        let jumps = |id: Uuid| -> Option<Option<Uuid>> {
            if id == b {
                Some(Some(c))
            } else if id == c {
                Some(None)
            } else if id == a {
                Some(Some(b))
            } else {
                None
            }
        };
        assert_eq!(jump_chain(Some(a), Some(b), 8, jumps).unwrap(), vec![b, c]);
        assert_eq!(
            jump_chain(Some(a), None, 8, jumps).unwrap(),
            Vec::<Uuid>::new()
        );
        // Saving c with jump a would close the loop c -> a -> b -> c.
        assert_eq!(
            jump_chain(Some(c), Some(a), 8, jumps),
            Err(JumpChainError::Loop)
        );
        // Self-jump.
        assert_eq!(
            jump_chain(Some(a), Some(a), 8, jumps),
            Err(JumpChainError::Loop)
        );
        assert_eq!(
            jump_chain(Some(a), Some(d), 8, jumps),
            Err(JumpChainError::Missing(d))
        );
        assert_eq!(
            jump_chain(Some(a), Some(b), 1, jumps),
            Err(JumpChainError::TooLong(1))
        );
        // New (unsaved) host: no id of its own yet.
        assert_eq!(jump_chain(None, Some(b), 8, jumps).unwrap(), vec![b, c]);
    }

    #[test]
    fn forward_rule_is_flat_and_tagged() {
        let r = ForwardRule {
            label: "db".into(),
            host_id: Uuid::nil(),
            auto_start: true,
            kind: ForwardKind::Dynamic {
                bind_addr: "127.0.0.1".into(),
                bind_port: 1080,
            },
        };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["kind"], "dynamic");
        assert_eq!(v["bind_port"], 1080);
        assert_eq!(serde_json::from_value::<ForwardRule>(v).unwrap(), r);
    }

    #[test]
    fn redaction_strips_every_secret() {
        let with_key = Identity {
            label: "k".into(),
            username: "u".into(),
            auth: AuthMethod::PrivateKey {
                private_key: "SECRET".into(),
                passphrase: Some("PASS".into()),
            },
            notes: "n".into(),
            for_host: None,
        };
        let r = serde_json::to_string(&with_key.redacted()).unwrap();
        assert!(!r.contains("SECRET") && !r.contains("PASS"));
        assert!(r.contains("private_key"));
        let pw = Identity {
            auth: AuthMethod::Password {
                password: "hunter2".into(),
            },
            ..with_key
        };
        assert!(!serde_json::to_string(&pw.redacted())
            .unwrap()
            .contains("hunter2"));
        // Rules saved before auto_start existed still load.
        let old: ForwardRule = serde_json::from_str(
            r#"{"label":"x","host_id":"00000000-0000-0000-0000-000000000000","kind":"dynamic","bind_addr":"127.0.0.1","bind_port":1080}"#,
        )
        .unwrap();
        assert!(!old.auto_start);
    }

    #[test]
    fn auth_method_is_tagged() {
        let a = AuthMethod::PrivateKey {
            private_key: "-----BEGIN".into(),
            passphrase: None,
        };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains(r#""type":"private_key""#));
        assert!(!json.contains("passphrase"));
        let back: AuthMethod = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }
}
