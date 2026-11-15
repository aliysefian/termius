//! A client for the SSH agent that already runs on this computer: OpenSSH's `ssh-agent`, gpg-agent, a password
//! manager's agent, Pageant. It is how keys that never leave their device are used: FIDO2 security keys
//! (`sk-ssh-ed25519@openssh.com`, `sk-ecdsa-sha2-nistp256@openssh.com`), smart cards and PKCS#11 tokens.
//!
//! It speaks the agent protocol itself instead of going through the SSH library's client, for three reasons:
//!
//! * One key the library cannot read (a type it doesn't know) made it drop the whole list. Here such a key is
//!   listed, marked unusable, and the rest are still offered.
//! * The agent's signature is handed to the server exactly as the agent wrote it. A hardware key's signature carries
//!   extra fields after the signature itself (flags and a counter; more for WebAuthn), and nothing here needs to
//!   understand them.
//! * A key the agent declines to sign with (nobody touched it, the token is unplugged) is one failed key, not the end
//!   of the login: the next key is tried.
//!
//! Which agent: the socket set in Settings, else `SSH_AUTH_SOCK`, else `IdentityAgent` from `~/.ssh/config`, else the
//! usual places agents put their sockets. An app started from a desktop menu often has no `SSH_AUTH_SOCK` even
//! though a terminal has one.

use std::sync::RwLock;
use std::time::Duration;

use base64::Engine;
use russh::keys::agent::client::AgentStream;
use russh::keys::agent::AgentIdentity;
use russh::keys::ssh_key::public::KeyData;
use russh::keys::ssh_key::{Certificate, HashAlg, PublicKey};
use serde::Serialize;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
const RSA_SHA2_256: u32 = 2;
const RSA_SHA2_512: u32 = 4;

/// An agent holding hundreds of keys still answers well under this.
const MAX_MESSAGE: usize = 1024 * 1024;
/// Listing keys needs nobody's attention, so an agent that says nothing is stuck.
const LIST_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a signature may take before the person is told what the agent is probably waiting for.
const QUIET_FOR: Duration = Duration::from_millis(1500);

/// Tells the person what a login is waiting for. Written into the terminal that is connecting.
pub type Notice = std::sync::Arc<dyn Fn(&str) + Send + Sync>;

type Stream = Box<dyn AgentStream + Send + Unpin>;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    /// No agent could be reached. The text says where it looked.
    #[error("{0}")]
    Unavailable(String),
    #[error("the ssh-agent at {source_name} stopped answering: {reason}")]
    Io { source_name: String, reason: String },
    #[error("the ssh-agent at {0} sent an answer that makes no sense")]
    Protocol(String),
    /// The agent answered "no": the key was not touched, its device is unplugged, or the request was denied.
    #[error("the ssh-agent would not sign")]
    Refused,
    /// The SSH connection went away while the agent was signing.
    #[error("the connection closed during the login")]
    Send,
}

impl From<russh::SendError> for AgentError {
    fn from(_: russh::SendError) -> Self {
        AgentError::Send
    }
}

/// What a listed key can be used as.
#[derive(Clone)]
pub enum Usable {
    Key(PublicKey),
    Certificate(Box<Certificate>),
}

/// One key the agent listed.
#[derive(Clone)]
pub struct Identity {
    /// The key exactly as the agent sent it; this is what a signature is asked for with.
    pub blob: Vec<u8>,
    pub comment: String,
    /// The key type the blob names, such as `ssh-ed25519` or `sk-ssh-ed25519@openssh.com`.
    pub algorithm: String,
    /// None when this build cannot read the key type; such a key is shown but not offered to servers.
    pub usable: Option<Usable>,
}

impl Identity {
    fn parse(blob: Vec<u8>, comment: String) -> Self {
        let algorithm = algorithm_of(&blob);
        let usable = if algorithm.contains("-cert-v0") {
            Certificate::from_bytes(&blob).ok().map(|c| Usable::Certificate(Box::new(c)))
        } else {
            // A type the key library does not know is read as an opaque blob; there is nothing to offer a server.
            PublicKey::from_bytes(&blob).ok().filter(|k| !matches!(k.key_data(), KeyData::Other(_))).map(Usable::Key)
        };
        Identity { blob, comment, algorithm, usable }
    }

    /// A key whose private half lives in a security key and needs a touch (FIDO2).
    pub fn is_security_key(&self) -> bool {
        self.algorithm.starts_with("sk-") || self.algorithm.starts_with("webauthn-sk-")
    }

    fn is_rsa(&self) -> bool {
        self.algorithm == "ssh-rsa" || self.algorithm.starts_with("ssh-rsa-cert-")
    }

    /// `SHA256:…` as `ssh-add -l` prints it. For a certificate that is the fingerprint of the key inside it.
    pub fn fingerprint(&self) -> String {
        match &self.usable {
            Some(Usable::Key(k)) => k.fingerprint(HashAlg::Sha256).to_string(),
            Some(Usable::Certificate(c)) => PublicKey::new(c.public_key().clone(), "").fingerprint(HashAlg::Sha256).to_string(),
            None => format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(Sha256::digest(&self.blob))),
        }
    }

    /// How the key is named in messages: its comment when it has one, else its fingerprint.
    pub fn label(&self) -> String {
        if self.comment.trim().is_empty() {
            self.fingerprint()
        } else {
            self.comment.trim().to_string()
        }
    }
}

fn put_string(out: &mut Vec<u8>, b: &[u8]) {
    out.extend_from_slice(&(b.len() as u32).to_be_bytes());
    out.extend_from_slice(b);
}

fn take_string<'a>(buf: &mut &'a [u8]) -> Option<&'a [u8]> {
    let n = u32::from_be_bytes(buf.get(..4)?.try_into().ok()?) as usize;
    let s = buf.get(4..4usize.checked_add(n)?)?;
    *buf = &buf[4 + n..];
    Some(s)
}

/// The key type a public-key blob starts with; empty when the blob is not one.
fn algorithm_of(blob: &[u8]) -> String {
    let mut b = blob;
    take_string(&mut b).and_then(|s| std::str::from_utf8(s).ok()).unwrap_or_default().to_string()
}

/// Read an `IDENTITIES_ANSWER`. A key this build cannot read is kept (as not usable), never an error.
fn parse_identities(answer: &[u8]) -> Option<Vec<Identity>> {
    let (&kind, mut rest) = answer.split_first()?;
    if kind != IDENTITIES_ANSWER {
        return None;
    }
    let n = u32::from_be_bytes(rest.get(..4)?.try_into().ok()?);
    rest = &rest[4..];
    let mut out = Vec::new();
    for _ in 0..n {
        let blob = take_string(&mut rest)?.to_vec();
        let comment = String::from_utf8_lossy(take_string(&mut rest)?).into_owned();
        out.push(Identity::parse(blob, comment));
    }
    Some(out)
}

/// A connection to one agent.
pub struct Client {
    stream: Stream,
    /// Where this agent was found, for messages: a socket path, a pipe name or "Pageant".
    pub source: String,
}

impl Client {
    async fn request(&mut self, body: &[u8]) -> Result<Vec<u8>, AgentError> {
        let io = |source: &str, e: std::io::Error| AgentError::Io { source_name: source.to_string(), reason: e.to_string() };
        let mut frame = Vec::with_capacity(4 + body.len());
        put_string(&mut frame, body);
        self.stream.write_all(&frame).await.map_err(|e| io(&self.source, e))?;
        self.stream.flush().await.map_err(|e| io(&self.source, e))?;
        let mut len = [0u8; 4];
        self.stream.read_exact(&mut len).await.map_err(|e| io(&self.source, e))?;
        let len = u32::from_be_bytes(len) as usize;
        if len == 0 || len > MAX_MESSAGE {
            return Err(AgentError::Protocol(self.source.clone()));
        }
        let mut answer = vec![0u8; len];
        self.stream.read_exact(&mut answer).await.map_err(|e| io(&self.source, e))?;
        Ok(answer)
    }

    /// Every key the agent holds, in the agent's order.
    pub async fn identities(&mut self) -> Result<Vec<Identity>, AgentError> {
        let answer = tokio::time::timeout(LIST_TIMEOUT, self.request(&[REQUEST_IDENTITIES]))
            .await
            .map_err(|_| AgentError::Io { source_name: self.source.clone(), reason: "no answer when asked for its keys".into() })??;
        parse_identities(&answer).ok_or_else(|| AgentError::Protocol(self.source.clone()))
    }

    /// Ask for a signature over `data`. Returns the signature exactly as the agent encoded it. This waits as long as
    /// the agent does: a security key answers when it is touched.
    pub async fn sign(&mut self, blob: &[u8], data: &[u8], flags: u32) -> Result<Vec<u8>, AgentError> {
        let mut body = vec![SIGN_REQUEST];
        put_string(&mut body, blob);
        put_string(&mut body, data);
        body.extend_from_slice(&flags.to_be_bytes());
        let answer = self.request(&body).await?;
        match answer.split_first() {
            Some((&SIGN_RESPONSE, mut rest)) => match take_string(&mut rest) {
                Some(sig) if !sig.is_empty() => Ok(sig.to_vec()),
                _ => Err(AgentError::Protocol(self.source.clone())),
            },
            Some((&FAILURE, _)) => Err(AgentError::Refused),
            _ => Err(AgentError::Protocol(self.source.clone())),
        }
    }

    /// Signs one login attempt with `identity`.
    pub fn signer<'a>(&'a mut self, identity: &'a Identity, notice: Option<&'a Notice>) -> KeySigner<'a> {
        KeySigner { client: self, identity, notice }
    }
}

/// Signs with one of the agent's keys during a login.
pub struct KeySigner<'a> {
    client: &'a mut Client,
    identity: &'a Identity,
    notice: Option<&'a Notice>,
}

/// What to tell the person when a signature is taking a while.
fn waiting_text(identity: &Identity) -> String {
    if identity.is_security_key() {
        format!("Waiting for the security key \"{}\": touch it when it blinks (it may ask for its PIN first).", identity.label())
    } else {
        format!("Waiting for the ssh-agent to approve the key \"{}\" (a prompt, a PIN or a touch may be needed).", identity.label())
    }
}

impl russh::Signer for KeySigner<'_> {
    type Error = AgentError;

    async fn auth_sign(&mut self, _key: &AgentIdentity, hash_alg: Option<HashAlg>, to_sign: Vec<u8>) -> Result<Vec<u8>, AgentError> {
        let flags = match (self.identity.is_rsa(), hash_alg) {
            (true, Some(HashAlg::Sha512)) => RSA_SHA2_512,
            (true, Some(HashAlg::Sha256)) => RSA_SHA2_256,
            _ => 0,
        };
        let signature = {
            let signing = self.client.sign(&self.identity.blob, &to_sign, flags);
            tokio::pin!(signing);
            match tokio::time::timeout(QUIET_FOR, &mut signing).await {
                Ok(done) => done?,
                Err(_) => {
                    if let Some(notice) = self.notice {
                        notice(&waiting_text(self.identity));
                    }
                    signing.await?
                }
            }
        };
        // The login message wants the signed data followed by the signature as one string; the agent's answer is
        // already that signature in wire form, whatever the key type.
        let mut out = to_sign;
        put_string(&mut out, &signature);
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Which agent
// ---------------------------------------------------------------------------

static SOCKET_SETTING: RwLock<Option<String>> = RwLock::new(None);

/// Use this agent socket (or, on Windows, named pipe) instead of looking for one. Empty or None: look for one.
pub fn set_socket(path: Option<String>) {
    let path = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    *SOCKET_SETTING.write().unwrap_or_else(|p| p.into_inner()) = path;
}

fn socket_setting() -> Option<String> {
    SOCKET_SETTING.read().unwrap_or_else(|p| p.into_inner()).clone()
}

/// A place an agent may be listening.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Place {
    pub path: String,
    /// Why this place is looked at: "Settings", "SSH_AUTH_SOCK", "~/.ssh/config", or the agent's name.
    pub from: String,
}

#[cfg_attr(not(unix), allow(dead_code))]
fn home() -> Option<std::path::PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(std::path::PathBuf::from)
}

/// `~`, `%d` and `${VAR}` in a path from Settings or `~/.ssh/config`.
#[cfg_attr(not(unix), allow(dead_code))]
fn expand(path: &str, home: Option<&std::path::Path>) -> String {
    let mut p = path.trim().trim_matches('"').to_string();
    if let Some(h) = home.map(|h| h.to_string_lossy().into_owned()) {
        if p == "~" {
            p = h.clone();
        } else if let Some(rest) = p.strip_prefix("~/") {
            p = format!("{h}/{rest}");
        }
        p = p.replace("%d", &h);
    }
    while let Some(start) = p.find("${") {
        let Some(end) = p[start..].find('}') else { break };
        let value = std::env::var(&p[start + 2..start + end]).unwrap_or_default();
        p.replace_range(start..=start + end, &value);
    }
    p
}

/// `IdentityAgent` as it applies to every host: set before the first `Host`/`Match` line, or under `Host *` or
/// `Match all`. `none` and `SSH_AUTH_SOCK` are not paths and give nothing.
#[cfg_attr(not(unix), allow(dead_code))]
fn identity_agent_from(config: &str, home: Option<&std::path::Path>) -> Option<String> {
    let mut applies = true;
    for line in config.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = match line.split_once(|c: char| c.is_whitespace() || c == '=') {
            Some((k, v)) => (k.to_ascii_lowercase(), v.trim().trim_start_matches('=').trim()),
            None => continue,
        };
        match key.as_str() {
            "host" => applies = value.split_whitespace().any(|p| p == "*"),
            "match" => applies = value.eq_ignore_ascii_case("all"),
            "identityagent" if applies => {
                let v = value.trim_matches('"');
                if v.eq_ignore_ascii_case("none") || v == "SSH_AUTH_SOCK" {
                    return None;
                }
                // "$VAR" names an environment variable holding the path.
                if let Some(var) = v.strip_prefix('$').filter(|n| !n.starts_with('{')) {
                    return std::env::var(var).ok().filter(|p| !p.is_empty());
                }
                return Some(expand(v, home));
            }
            _ => {}
        }
    }
    None
}

/// Where agents usually put their sockets, most ordinary first. Only the ones that exist are returned.
#[cfg(unix)]
fn usual_places(home: Option<&std::path::Path>, runtime: Option<&std::path::Path>) -> Vec<Place> {
    let mut all: Vec<(std::path::PathBuf, &str)> = Vec::new();
    if let Some(r) = runtime {
        all.push((r.join("ssh-agent.socket"), "ssh-agent"));
        all.push((r.join("gcr/ssh"), "GNOME Keyring"));
        all.push((r.join("keyring/ssh"), "GNOME Keyring"));
        all.push((r.join("yubikey-agent/yubikey-agent.sock"), "yubikey-agent"));
    }
    all.push(("/opt/homebrew/var/run/yubikey-agent.sock".into(), "yubikey-agent"));
    all.push(("/usr/local/var/run/yubikey-agent.sock".into(), "yubikey-agent"));
    if let Some(h) = home {
        all.push((h.join(".1password/agent.sock"), "1Password"));
        all.push((h.join("Library/Group Containers/2BUA8C4S2C.com.1password/t/agent.sock"), "1Password"));
        all.push((h.join("Library/Containers/com.maxgoedjen.Secretive.SecretAgent/Data/socket.ssh"), "Secretive"));
        all.push((h.join(".bitwarden-ssh-agent.sock"), "Bitwarden"));
    }
    if let Some(r) = runtime {
        all.push((r.join("gnupg/S.gpg-agent.ssh"), "gpg-agent"));
    }
    if let Some(h) = home {
        all.push((h.join(".gnupg/S.gpg-agent.ssh"), "gpg-agent"));
    }
    all.into_iter()
        .filter(|(p, _)| p.exists())
        .map(|(p, from)| Place { path: p.to_string_lossy().into_owned(), from: from.to_string() })
        .collect()
}

/// The agents to try, best first. The first entry is the one the person chose when there is such a choice
/// (Settings, then `SSH_AUTH_SOCK`, then `~/.ssh/config`); the usual places follow only when nothing was chosen.
#[cfg(unix)]
pub fn places() -> Vec<Place> {
    let home = home();
    if let Some(path) = socket_setting() {
        return vec![Place { path: expand(&path, home.as_deref()), from: "Settings".into() }];
    }
    if let Some(path) = std::env::var("SSH_AUTH_SOCK").ok().filter(|p| !p.is_empty()) {
        return vec![Place { path, from: "SSH_AUTH_SOCK".into() }];
    }
    let config = home.as_ref().and_then(|h| std::fs::read_to_string(h.join(".ssh/config")).ok());
    if let Some(path) = config.and_then(|c| identity_agent_from(&c, home.as_deref())) {
        return vec![Place { path, from: "~/.ssh/config".into() }];
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
    usual_places(home.as_deref(), runtime.as_deref())
}

#[cfg(windows)]
const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// The agents to try, best first: the pipe set in Settings, else OpenSSH's agent and then Pageant.
#[cfg(windows)]
pub fn places() -> Vec<Place> {
    if let Some(path) = socket_setting() {
        return vec![Place { path, from: "Settings".into() }];
    }
    vec![
        Place { path: OPENSSH_PIPE.into(), from: "OpenSSH agent".into() },
        Place { path: "Pageant".into(), from: "Pageant".into() },
    ]
}

#[cfg(unix)]
async fn connect(place: &Place) -> std::io::Result<Stream> {
    Ok(Box::new(tokio::net::UnixStream::connect(&place.path).await?))
}

#[cfg(windows)]
async fn connect(place: &Place) -> std::io::Result<Stream> {
    use russh::keys::agent::client::AgentClient;
    if place.path == "Pageant" {
        return AgentClient::connect_pageant().await.map(|c| c.into_inner()).map_err(std::io::Error::other);
    }
    AgentClient::connect_named_pipe(&place.path).await.map(|c| c.into_inner()).map_err(std::io::Error::other)
}

/// A raw connection to the agent, for agent forwarding: the first place that answers.
pub async fn stream() -> std::io::Result<Box<dyn AgentStream + Send + Unpin>> {
    let places = places();
    let mut last = None;
    for place in &places {
        match connect(place).await {
            Ok(s) => return Ok(s),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other(nowhere())))
}

fn nowhere() -> String {
    if cfg!(windows) {
        "no ssh-agent is running (neither OpenSSH's agent service nor Pageant)".into()
    } else {
        "no ssh-agent was found: SSH_AUTH_SOCK is not set and no agent socket is in the usual places. Set the agent socket in Settings".into()
    }
}

/// Connect to this computer's agent. With several candidates (nothing chosen, several agents installed) the first
/// one that holds keys wins, so an agent that runs but is empty does not hide the one with the hardware key.
pub async fn open() -> Result<Client, AgentError> {
    let places = places();
    let mut errors = Vec::new();
    let mut empty: Option<Client> = None;
    for place in &places {
        match connect(place).await {
            Ok(stream) => {
                let mut client = Client { stream, source: place.path.clone() };
                if places.len() == 1 {
                    return Ok(client);
                }
                // Asking twice on one connection is fine; the caller lists the keys again.
                match client.identities().await {
                    Ok(ids) if !ids.is_empty() => return Ok(client),
                    Ok(_) => empty = empty.or(Some(client)),
                    Err(e) => errors.push(e.to_string()),
                }
            }
            Err(e) => errors.push(format!("{} ({}): {e}", place.path, place.from)),
        }
    }
    if let Some(client) = empty {
        return Ok(client);
    }
    Err(AgentError::Unavailable(if errors.is_empty() { nowhere() } else { errors.join("; ") }))
}

/// One key, as the Settings page shows it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KeyInfo {
    pub algorithm: String,
    pub fingerprint: String,
    pub comment: String,
    /// A FIDO2 security key: logging in with it needs a touch.
    pub security_key: bool,
    pub certificate: bool,
    /// False for a key type this build cannot offer to a server.
    pub usable: bool,
}

/// What the agent on this computer looks like right now.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Status {
    /// The agent that answered; None when none did.
    pub source: Option<String>,
    pub keys: Vec<KeyInfo>,
    pub error: Option<String>,
    /// Every place that would be tried, in order.
    pub places: Vec<Place>,
    /// The socket chosen in Settings, if any.
    pub setting: Option<String>,
}

/// Look at the agent without signing anything.
pub async fn status() -> Status {
    let mut status = Status { source: None, keys: vec![], error: None, places: places(), setting: socket_setting() };
    match open().await {
        Ok(mut client) => {
            status.source = Some(client.source.clone());
            match client.identities().await {
                Ok(ids) => {
                    status.keys = ids
                        .iter()
                        .map(|id| KeyInfo {
                            algorithm: id.algorithm.clone(),
                            fingerprint: id.fingerprint(),
                            comment: id.comment.clone(),
                            security_key: id.is_security_key(),
                            certificate: matches!(id.usable, Some(Usable::Certificate(_))),
                            usable: id.usable.is_some(),
                        })
                        .collect()
                }
                Err(e) => status.error = Some(e.to_string()),
            }
        }
        Err(e) => status.error = Some(e.to_string()),
    }
    status
}

#[cfg(test)]
pub(crate) mod testutil {
    //! A stand-in agent for tests, holding keys the way hardware does: it can make FIDO2 signatures, list key types
    //! nobody knows, and decline to sign.
    #![cfg_attr(not(unix), allow(dead_code))]
    use super::*;
    use russh::keys::signature::Signer as _;
    use russh::keys::ssh_key::private::Ed25519Keypair;
    use std::sync::Arc;

    pub const SK_ED25519: &str = "sk-ssh-ed25519@openssh.com";

    #[derive(Clone)]
    pub enum Held {
        /// A FIDO2 Ed25519 key. `touch` false: nobody touches it, so the agent answers "no".
        SecurityKey { pair: Arc<Ed25519Keypair>, touch: bool, delay: Duration },
        /// An ordinary key: `blob` is its public key, `pair` signs.
        Plain { pair: Arc<Ed25519Keypair> },
        /// A key type no SSH library reads.
        Unknown,
    }

    impl Held {
        pub fn security_key() -> Self {
            Held::SecurityKey { pair: Arc::new(Ed25519Keypair::from_seed(&rand::random())), touch: true, delay: Duration::ZERO }
        }

        pub fn plain() -> Self {
            Held::Plain { pair: Arc::new(Ed25519Keypair::from_seed(&rand::random())) }
        }

        pub fn blob(&self) -> Vec<u8> {
            let mut b = Vec::new();
            match self {
                Held::SecurityKey { pair, .. } => {
                    put_string(&mut b, SK_ED25519.as_bytes());
                    put_string(&mut b, pair.public.as_ref());
                    put_string(&mut b, b"ssh:");
                }
                Held::Plain { pair } => {
                    put_string(&mut b, b"ssh-ed25519");
                    put_string(&mut b, pair.public.as_ref());
                }
                Held::Unknown => {
                    put_string(&mut b, b"ssh-future-key@example.com");
                    put_string(&mut b, &[7u8; 40]);
                }
            }
            b
        }

        /// The line for `authorized_keys`.
        pub fn authorized_line(&self) -> String {
            let blob = self.blob();
            format!("{} {}\n", algorithm_of(&blob), base64::engine::general_purpose::STANDARD.encode(&blob))
        }

        async fn sign(&self, data: &[u8]) -> Option<Vec<u8>> {
            let mut out = Vec::new();
            match self {
                Held::SecurityKey { pair, touch, delay } => {
                    tokio::time::sleep(*delay).await;
                    if !*touch {
                        return None;
                    }
                    // What the device signs: SHA-256("ssh:") || flags || counter || SHA-256(message).
                    let (flags, counter) = (0x01u8, 7u32);
                    let mut signed = Sha256::digest(b"ssh:").to_vec();
                    signed.push(flags);
                    signed.extend_from_slice(&counter.to_be_bytes());
                    signed.extend_from_slice(&Sha256::digest(data));
                    let sig = pair.try_sign(&signed).ok()?;
                    put_string(&mut out, SK_ED25519.as_bytes());
                    put_string(&mut out, sig.as_bytes());
                    out.push(flags);
                    out.extend_from_slice(&counter.to_be_bytes());
                }
                Held::Plain { pair } => {
                    let sig = pair.try_sign(data).ok()?;
                    put_string(&mut out, b"ssh-ed25519");
                    put_string(&mut out, sig.as_bytes());
                }
                Held::Unknown => return None,
            }
            Some(out)
        }
    }

    async fn answer(keys: &[Held], request: &[u8]) -> Vec<u8> {
        match request.split_first() {
            Some((&REQUEST_IDENTITIES, _)) => {
                let mut out = vec![IDENTITIES_ANSWER];
                out.extend_from_slice(&(keys.len() as u32).to_be_bytes());
                for (i, k) in keys.iter().enumerate() {
                    put_string(&mut out, &k.blob());
                    put_string(&mut out, format!("key-{i}").as_bytes());
                }
                out
            }
            Some((&SIGN_REQUEST, mut rest)) => {
                let (Some(blob), Some(data)) = (take_string(&mut rest), take_string(&mut rest)) else { return vec![FAILURE] };
                let Some(key) = keys.iter().find(|k| k.blob() == blob) else { return vec![FAILURE] };
                match key.sign(data).await {
                    Some(sig) => {
                        let mut out = vec![SIGN_RESPONSE];
                        put_string(&mut out, &sig);
                        out
                    }
                    None => vec![FAILURE],
                }
            }
            _ => vec![FAILURE],
        }
    }

    /// Serve `keys` on a Unix socket at `path` until the test ends.
    #[cfg(unix)]
    pub fn serve(path: &std::path::Path, keys: Vec<Held>) {
        let listener = tokio::net::UnixListener::bind(path).expect("bind the test agent");
        tokio::spawn(async move {
            while let Ok((mut conn, _)) = listener.accept().await {
                let keys = keys.clone();
                tokio::spawn(async move {
                    loop {
                        let mut len = [0u8; 4];
                        if conn.read_exact(&mut len).await.is_err() {
                            return;
                        }
                        let mut request = vec![0u8; u32::from_be_bytes(len) as usize];
                        if conn.read_exact(&mut request).await.is_err() {
                            return;
                        }
                        let mut frame = Vec::new();
                        put_string(&mut frame, &answer(&keys, &request).await);
                        if conn.write_all(&frame).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;
    use std::path::Path;

    #[test]
    fn an_unreadable_key_is_listed_and_does_not_hide_the_others() {
        let keys = [Held::Unknown, Held::security_key()];
        let mut answer = vec![IDENTITIES_ANSWER];
        answer.extend_from_slice(&2u32.to_be_bytes());
        for k in &keys {
            put_string(&mut answer, &k.blob());
            put_string(&mut answer, b"c");
        }
        let ids = parse_identities(&answer).unwrap();
        assert_eq!(ids.len(), 2);
        assert!(ids[0].usable.is_none());
        assert_eq!(ids[0].algorithm, "ssh-future-key@example.com");
        assert!(ids[0].fingerprint().starts_with("SHA256:"));
        assert!(matches!(ids[1].usable, Some(Usable::Key(_))));
        assert!(ids[1].is_security_key());
        assert!(!ids[0].is_security_key());
    }

    #[test]
    fn a_broken_answer_is_an_error_not_a_panic() {
        assert!(parse_identities(&[]).is_none());
        assert!(parse_identities(&[FAILURE]).is_none());
        assert!(parse_identities(&[IDENTITIES_ANSWER, 0, 0, 0, 2, 0, 0, 0, 9, 1]).is_none());
        // A length that would wrap around.
        assert!(parse_identities(&[IDENTITIES_ANSWER, 0, 0, 0, 1, 0xff, 0xff, 0xff, 0xff]).is_none());
        assert_eq!(parse_identities(&[IDENTITIES_ANSWER, 0, 0, 0, 0]).map(|v| v.len()), Some(0));
    }

    #[test]
    fn identity_agent_is_read_only_where_it_applies_to_every_host() {
        let home = Path::new("/home/u");
        let read = |c: &str| identity_agent_from(c, Some(home));
        assert_eq!(read("IdentityAgent ~/.1password/agent.sock\n"), Some("/home/u/.1password/agent.sock".into()));
        assert_eq!(read("Host *\n  IdentityAgent \"%d/x y/agent.sock\"\n"), Some("/home/u/x y/agent.sock".into()));
        assert_eq!(read("Host web\n  IdentityAgent /only/web\nHost *\n  identityagent=/all\n"), Some("/all".into()));
        assert_eq!(read("Match all\n  IdentityAgent /m\n"), Some("/m".into()));
        assert_eq!(read("Host web\n  IdentityAgent /only/web\n"), None);
        assert_eq!(read("IdentityAgent none\n"), None);
        assert_eq!(read("IdentityAgent SSH_AUTH_SOCK\n"), None);
        assert_eq!(read("# IdentityAgent /commented\n"), None);
    }

    #[test]
    fn paths_expand_home_and_variables() {
        std::env::set_var("SYSAGENT_TEST_DIR", "/run/x");
        assert_eq!(expand("~/a.sock", Some(Path::new("/h"))), "/h/a.sock");
        assert_eq!(expand("${SYSAGENT_TEST_DIR}/a.sock", None), "/run/x/a.sock");
        assert_eq!(expand(" /plain ", None), "/plain");
        assert_eq!(expand("${UNCLOSED", None), "${UNCLOSED");
    }

    #[cfg(unix)]
    #[test]
    fn only_sockets_that_exist_are_candidates_and_gpg_comes_last() {
        let dir = tempfile::TempDir::new().unwrap();
        let (home, runtime) = (dir.path().join("home"), dir.path().join("run"));
        std::fs::create_dir_all(home.join(".1password")).unwrap();
        std::fs::create_dir_all(runtime.join("gnupg")).unwrap();
        std::fs::write(home.join(".1password/agent.sock"), b"").unwrap();
        std::fs::write(runtime.join("gnupg/S.gpg-agent.ssh"), b"").unwrap();
        let found = usual_places(Some(&home), Some(&runtime));
        assert_eq!(found.iter().map(|p| p.from.as_str()).collect::<Vec<_>>(), ["1Password", "gpg-agent"]);
        assert!(usual_places(Some(&dir.path().join("nobody")), None).iter().all(|p| !p.path.contains("nobody")));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_security_key_signature_comes_back_whole_and_a_refusal_is_refused() {
        let dir = tempfile::TempDir::new().unwrap();
        let sock = dir.path().join("a.sock");
        let good = Held::security_key();
        let Held::SecurityKey { pair, .. } = Held::security_key() else { unreachable!() };
        let untouched = Held::SecurityKey { pair, touch: false, delay: Duration::ZERO };
        serve(&sock, vec![good.clone(), untouched.clone()]);
        let place = Place { path: sock.to_string_lossy().into_owned(), from: "test".into() };
        let mut client = Client { stream: connect(&place).await.unwrap(), source: place.path.clone() };

        let ids = client.identities().await.unwrap();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0].comment, "key-0");
        let sig = client.sign(&ids[0].blob, b"hello", 0).await.unwrap();
        // type, signature, then the 5 bytes only a security key adds: flags and counter.
        let mut rest = &sig[..];
        assert_eq!(take_string(&mut rest), Some(SK_ED25519.as_bytes()));
        assert_eq!(take_string(&mut rest).map(|s| s.len()), Some(64));
        assert_eq!(rest, [1, 0, 0, 0, 7]);

        assert!(matches!(client.sign(&ids[1].blob, b"hello", 0).await, Err(AgentError::Refused)));
        // The connection is still good after a refusal.
        assert!(client.sign(&ids[0].blob, b"again", 0).await.is_ok());
    }
}
