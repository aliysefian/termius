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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// A saved port-forwarding rule, bound to a host (and through it, an identity).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForwardRule {
    pub label: String,
    pub host_id: Uuid,
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
    fn forward_rule_is_flat_and_tagged() {
        let r = ForwardRule {
            label: "db".into(),
            host_id: Uuid::nil(),
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
