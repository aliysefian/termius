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
    /// Stored credentials reference the Key Manager instead ([`AuthMethod::Key`]);
    /// this form remains for connection targets and older records.
    PrivateKey {
        private_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        passphrase: Option<String>,
        /// OpenSSH certificate for this key, if the server uses a CA.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        certificate: Option<String>,
    },
    /// A key from the Key Manager (the `keys` collection).
    Key { key_id: Uuid },
    /// A key file on *this* computer, referenced by path rather than copied
    /// into the vault. Other devices need the same file at the same path.
    KeyFile {
        path: String,
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
            // Certificates are public; keep them so the form can show one.
            AuthMethod::PrivateKey { certificate, .. } => AuthMethod::PrivateKey {
                private_key: String::new(),
                passphrase: None,
                certificate: certificate.clone(),
            },
            AuthMethod::Key { key_id } => AuthMethod::Key { key_id: *key_id },
            AuthMethod::KeyFile { path, passphrase } => AuthMethod::KeyFile {
                path: path.clone(),
                // Present-but-hidden, like SshKey::redacted.
                passphrase: passphrase.as_ref().map(|_| String::new()),
            },
            AuthMethod::Agent => AuthMethod::Agent,
        };
        Self {
            auth,
            ..self.clone()
        }
    }
}

/// How to open an FTP host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FtpOptions {
    #[serde(default)]
    pub tls: crate::files::ftp::FtpTls,
    /// Sign in as `anonymous` instead of with a credential.
    #[serde(default)]
    pub anonymous: bool,
    /// SHA-256 of the server's certificate, trusted on first use. Empty until pinned.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cert_sha256: String,
}

/// How to open a Remote Desktop host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RdpOptions {
    /// Windows domain; empty when the account is local or the user name carries it.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub domain: String,
    /// Desktop size in pixels; 0 means "fit the tab".
    #[serde(default)]
    pub width: u16,
    #[serde(default)]
    pub height: u16,
    /// 16 or 32 bits per pixel.
    #[serde(default = "default_color_depth")]
    pub color_depth: u8,
    #[serde(default)]
    pub security: crate::rdp::Security,
    /// SHA-256 of the server's certificate, trusted on first use. Empty until pinned.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cert_sha256: String,
}

fn default_color_depth() -> u8 {
    32
}

impl Default for RdpOptions {
    fn default() -> Self {
        Self { domain: String::new(), width: 0, height: 0, color_depth: 32, security: crate::rdp::Security::Auto, cert_sha256: String::new() }
    }
}

/// A terminal's look on one host. Anything left empty follows the settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalProfile {
    /// Id of a built-in or imported terminal theme; empty follows the settings.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub theme: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scrollback: Option<u32>,
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
    #[serde(default)]
    pub favorite: bool,
    /// SOCKS/HTTP proxy or ProxyCommand used to reach this host (or the
    /// first hop of its jump chain).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_id: Option<Uuid>,
    /// Connect without the group's default jump host, even when a group sets one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_group_jump: bool,
    /// Connect without the group's default proxy, even when a group sets one.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_group_proxy: bool,
    /// Send a keep-alive every N seconds (OpenSSH `ServerAliveInterval`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keepalive_secs: Option<u32>,
    /// Connect with Mosh (`mosh-client` on this computer, `mosh-server` on the
    /// host) instead of a plain SSH session.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub mosh: bool,
    /// Settings for an FTP host (`protocol` = "ftp").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ftp: Option<FtpOptions>,
    /// Smart completion on this host: empty follows the settings (production hosts: history
    /// only); "on", "off" or "history" overrides them.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub completion: String,
    /// How to browse this host's files: empty for SFTP, "scp" when SFTP is off on the server.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file_protocol: String,
    /// Settings for a Remote Desktop host (`protocol` = "rdp").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rdp: Option<RdpOptions>,
    /// How this host's terminal looks, over the settings (a theme, text size, scrollback).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<TerminalProfile>,
    /// "telnet" for Telnet hosts (network gear); empty means SSH.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub protocol: String,
    /// Free-form key/value metadata, e.g. owner, ticket, cost centre.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub custom: std::collections::BTreeMap<String, String>,
}

/// An entry in the Key Manager.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshKey {
    pub name: String,
    /// e.g. "ssh-ed25519", "ssh-rsa", "ecdsa-sha2-nistp256".
    pub algorithm: String,
    /// One `authorized_keys` line.
    pub public_key: String,
    pub fingerprint: String,
    /// OpenSSH or PEM private key. `None` for public-only entries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    /// Passphrase of `private_key`, if it's encrypted and the user saved it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub passphrase: Option<String>,
    /// The private key text is itself passphrase-encrypted.
    #[serde(default)]
    pub encrypted: bool,
    /// OpenSSH certificate (`...-cert-v01@openssh.com ...`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub certificate: Option<String>,
    #[serde(default)]
    pub comment: String,
    #[serde(default)]
    pub created_at: u64,
    /// Created from a host's own form rather than the Key Manager.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub for_host: Option<Uuid>,
    /// Whether the vault's SSH agent offers this key to other programs.
    #[serde(default, skip_serializing_if = "AgentUse::is_off")]
    pub agent: AgentUse,
}

/// Agent exposure for one key. Off unless the user opts in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentUse {
    #[default]
    Off,
    /// Every signature asks for approval in the app.
    Ask,
    /// Signs without asking while the vault is unlocked.
    Allow,
}

impl AgentUse {
    pub fn is_off(&self) -> bool {
        *self == AgentUse::Off
    }
}

impl SshKey {
    /// Safe for the webview: whether a private key and passphrase exist, but
    /// never their contents.
    pub fn redacted(&self) -> Self {
        Self {
            private_key: self.private_key.as_ref().map(|_| String::new()),
            passphrase: self.passphrase.as_ref().map(|_| String::new()),
            ..self.clone()
        }
    }
}

/// A folder in the host tree, with defaults its hosts inherit.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostGroup {
    /// Slash-separated path, matching [`Host::group`].
    pub path: String,
    /// Used by hosts in this group (or below) that have no identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_identity_id: Option<Uuid>,
    /// Gateway for hosts in this group that have no jump host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_jump_host_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub environment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub notes: String,
}

/// A trusted server key, synced through the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnownHost {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    /// OpenSSH public key line.
    pub public_key: String,
    pub fingerprint: String,
    pub trusted_at: u64,
    /// Device that trusted it.
    pub trusted_by: String,
    /// Earlier keys this entry replaced, newest first.
    #[serde(default)]
    pub history: Vec<ReplacedKey>,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacedKey {
    pub algorithm: String,
    pub fingerprint: String,
    pub replaced_at: u64,
    pub replaced_by: String,
}

/// How to reach a host (or the first hop of its jump chain).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProxySpec {
    Socks5 {
        host: String,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
    Http {
        host: String,
        port: u16,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        username: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
    /// OpenSSH `ProxyCommand`. Runs a local program, so it only takes effect
    /// once a user has explicitly approved it (imported ones start unapproved).
    Command {
        command: String,
        #[serde(default)]
        approved: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proxy {
    pub name: String,
    pub spec: ProxySpec,
}

impl Proxy {
    pub fn redacted(&self) -> Self {
        let mut p = self.clone();
        if let ProxySpec::Socks5 { password, .. } | ProxySpec::Http { password, .. } = &mut p.spec {
            *password = password.as_ref().map(|_| String::new());
        }
        p
    }
}

/// A saved database connection. The password lives here, encrypted with
/// the rest of the vault; lists sent to the window carry it blanked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DbConnection {
    pub name: String,
    /// Which driver: `"mysql"` (also MariaDB).
    pub engine: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    /// Database to start in; empty means none.
    #[serde(default)]
    pub database: String,
    #[serde(default)]
    pub tls: crate::db::TlsMode,
    /// Reach the database through this saved SSH host, so its port is never
    /// exposed. `host` and `port` are then as seen from that host.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_host_id: Option<Uuid>,
    #[serde(default)]
    pub group: String,
    /// "production", "staging", "development" or empty, as on hosts.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub environment: String,
    #[serde(default)]
    pub notes: String,
}

impl DbConnection {
    pub fn redacted(&self) -> Self {
        let mut c = self.clone();
        c.password = c.password.as_ref().map(|_| String::new());
        c
    }
}

/// A saved set of tabs and split layouts. The frontend owns the layout
/// format; the backend just stores it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    #[serde(default)]
    pub tabs: Vec<serde_json::Value>,
}

/// Settings shared by every device using the vault.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultSettings {
    #[serde(default = "default_retention")]
    pub backup_retention: usize,
    /// Regular expressions for commands that get an extra confirmation on
    /// production hosts. A safety net, not a guarantee.
    #[serde(default = "default_destructive")]
    pub destructive_patterns: Vec<String>,
    /// Ask before pasting this many lines or more (0 = never ask).
    #[serde(default = "default_paste_lines")]
    pub paste_confirm_lines: u32,
    /// Clear copied secrets from the clipboard after this many seconds.
    #[serde(default = "default_clipboard_secs")]
    pub clipboard_clear_secs: u32,
}

/// The settings record's fixed ID.
pub const SETTINGS_ID: Uuid = Uuid::from_u128(1);

fn default_retention() -> usize {
    crate::vault::backup::DEFAULT_RETENTION
}
fn default_paste_lines() -> u32 {
    2
}
fn default_clipboard_secs() -> u32 {
    30
}
pub fn default_destructive() -> Vec<String> {
    [
        r"\brm\s+(-[a-zA-Z]*[rf][a-zA-Z]*\s+)+/",
        r"\bmkfs(\.\w+)?\b",
        r"\bdd\b.*\bof=/dev/",
        r"(^|[;&|]|\bsudo)\s*(shutdown|reboot|poweroff|halt)(\s|$)",
        r"(?i)\bdrop\s+(database|table|schema)\b",
        r"(?i)\btruncate\s+table\b",
        r"\bkubectl\s+delete\b",
        r"\bterraform\s+destroy\b",
        r"\bgit\s+push\b.*(--force|\s-f\b)",
        r"\bsystemctl\s+(stop|disable|mask)\b",
        r":\(\)\s*\{\s*:\|:&\s*\};:",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

impl Default for VaultSettings {
    fn default() -> Self {
        Self {
            backup_retention: default_retention(),
            destructive_patterns: default_destructive(),
            paste_confirm_lines: default_paste_lines(),
            clipboard_clear_secs: default_clipboard_secs(),
        }
    }
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
    /// Slash-separated folder, e.g. "Kubernetes/Debug".
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
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

/// A record as the webview may see it: secrets replaced by empty
/// placeholders, exactly as the list commands return them. `None` when the
/// data can't be read (it's then not sent at all rather than sent as-is).
pub fn redact_record(c: crate::vault::Collection, v: serde_json::Value) -> Option<serde_json::Value> {
    use crate::vault::Collection;
    fn via<T: Serialize + serde::de::DeserializeOwned>(
        v: serde_json::Value,
        f: impl FnOnce(&T) -> T,
    ) -> Option<serde_json::Value> {
        let t = serde_json::from_value::<T>(v).ok()?;
        serde_json::to_value(f(&t)).ok()
    }
    match c {
        Collection::Identities => via::<Identity>(v, Identity::redacted),
        Collection::Keys => via::<SshKey>(v, SshKey::redacted),
        Collection::Proxies => via::<Proxy>(v, Proxy::redacted),
        Collection::Databases => via::<DbConnection>(v, DbConnection::redacted),
        // Locks are internal; the rest hold no secrets.
        Collection::Locks => None,
        _ => Some(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_terminal_profile_is_optional_and_old_hosts_still_load() {
        let old: Host = serde_json::from_str(r#"{"label":"a","hostname":"h"}"#).unwrap();
        assert_eq!(old.profile, None);
        let plain = serde_json::to_string(&old).unwrap();
        assert!(!plain.contains("profile"), "nothing is written for a host without one: {plain}");
        let host = Host { label: "a".into(), hostname: "h".into(), profile: Some(TerminalProfile { theme: "dracula".into(), font_size: Some(16), scrollback: None }), ..Default::default() };
        let back: Host = serde_json::from_str(&serde_json::to_string(&host).unwrap()).unwrap();
        assert_eq!(back.profile, host.profile);
        assert!(!serde_json::to_string(&host).unwrap().contains("scrollback"));
    }

    #[test]
    fn live_records_are_redacted_like_lists() {
        use crate::vault::Collection;
        let key = SshKey {
            name: "k".into(),
            private_key: Some("-----BEGIN OPENSSH PRIVATE KEY-----".into()),
            passphrase: Some("hunter2".into()),
            ..Default::default()
        };
        let out = redact_record(Collection::Keys, serde_json::to_value(&key).unwrap()).unwrap();
        let text = out.to_string();
        assert!(!text.contains("BEGIN") && !text.contains("hunter2"), "{text}");
        assert_eq!(out["private_key"], "", "presence is still visible");

        let proxy = Proxy {
            name: "p".into(),
            spec: ProxySpec::Socks5 { host: "h".into(), port: 1, username: Some("u".into()), password: Some("s3cret".into()) },
        };
        let out = redact_record(Collection::Proxies, serde_json::to_value(&proxy).unwrap()).unwrap();
        assert!(!out.to_string().contains("s3cret"));

        let db = DbConnection {
            name: "d".into(),
            engine: "mysql".into(),
            host: "h".into(),
            port: 3306,
            username: "u".into(),
            password: Some("dbs3cret".into()),
            database: String::new(),
            tls: Default::default(),
            ssh_host_id: None,
            group: String::new(),
            environment: String::new(),
            notes: String::new(),
        };
        let out = redact_record(Collection::Databases, serde_json::to_value(&db).unwrap()).unwrap();
        assert!(!out.to_string().contains("dbs3cret"));
        assert_eq!(out["password"], "", "presence is still visible");

        let ident = Identity {
            label: "l".into(),
            username: "u".into(),
            auth: AuthMethod::Password { password: "pw!".into() },
            notes: String::new(),
            for_host: None,
        };
        let out = redact_record(Collection::Identities, serde_json::to_value(&ident).unwrap()).unwrap();
        assert!(!out.to_string().contains("pw!"));
        // Garbage in a secret-bearing collection is dropped, not forwarded.
        assert!(redact_record(Collection::Keys, serde_json::json!({"private_key": "x"})).is_none());
        assert!(redact_record(Collection::Locks, serde_json::json!({})).is_none());
    }

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
                certificate: None,
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
            certificate: None,
        };
        let json = serde_json::to_string(&a).unwrap();
        assert!(json.contains(r#""type":"private_key""#));
        assert!(!json.contains("passphrase"));
        let back: AuthMethod = serde_json::from_str(&json).unwrap();
        assert_eq!(back, a);
    }
}
