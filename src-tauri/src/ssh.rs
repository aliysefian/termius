//! SSH terminal engine built on `russh`.
//!
//! One [`SshManager`] lives in app state. Each terminal pane owns exactly one
//! SSH session, keyed by the pane id the frontend generates. The session runs
//! as a tokio task: PTY output flows out through a [`TermSink`], and
//! keystrokes / resizes flow in through an mpsc command channel.
//!
//! This module knows nothing about Tauri, so it can be compiled and exercised
//! on its own; `commands.rs` adapts it to IPC channels and events.

use std::collections::HashMap;
use std::future::Future;
use std::ops::Deref;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::known_hosts::learn_known_hosts_path;
use russh::keys::{
    self, check_known_hosts_path, HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate,
};
use russh::{ChannelMsg, Disconnect};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use zeroize::Zeroizing;

use crate::models::AuthMethod;

const TERM: &str = "xterm-256color";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
const KEEPALIVE: Duration = Duration::from_secs(30);
/// Longest jump chain we will follow (a sanity bound against misconfiguration).
pub const MAX_JUMPS: usize = 8;

#[derive(Debug, thiserror::Error)]
pub enum SshError {
    #[error("{0}")]
    Ssh(#[from] russh::Error),

    #[error("key error: {0}")]
    Key(#[from] keys::Error),

    #[error("could not resolve or reach {addr}: {source}")]
    Connect {
        addr: String,
        #[source]
        source: std::io::Error,
    },

    #[error("connection to {0} timed out")]
    Timeout(String),

    #[error("authentication failed for {user}@{host} ({method})")]
    AuthFailed {
        user: String,
        host: String,
        method: &'static str,
    },

    #[error("host key for {host} has CHANGED (fingerprint {fingerprint}). Possible man-in-the-middle; the connection was stopped")]
    HostKeyChanged {
        host: String,
        port: u16,
        fingerprint: String,
    },

    #[error("proxy {proxy}: {reason}")]
    Proxy { proxy: String, reason: String },

    #[error("{host} is not a trusted host yet (key {fingerprint}); the connection was stopped")]
    HostKeyUnknown { host: String, fingerprint: String },

    #[error("could not check the host key of {host}: {reason}")]
    HostKeyStore { host: String, reason: String },

    #[error("via jump host {host}: {source}")]
    Jump {
        host: String,
        #[source]
        source: Box<SshError>,
    },

    #[error("jump host {jump} could not reach {addr}: {reason}")]
    JumpUnreachable {
        jump: String,
        addr: String,
        reason: String,
    },

    #[error("jump chain is longer than {MAX_JUMPS} hops")]
    JumpChainTooLong,

    #[error("could not use key {path}: {reason}")]
    KeyFile { path: String, reason: String },

    #[error("no ssh-agent available: {0}")]
    NoAgent(String),

    #[error("ssh-agent holds no usable keys")]
    AgentNoKeys,

    #[error("a session for this pane already exists")]
    AlreadyConnected,

    #[error("no session for this pane")]
    NotConnected,

    #[error("session task is gone")]
    Gone,
}

/// Where terminal bytes and lifecycle notices go. Implemented over a Tauri
/// channel in production and over an mpsc in tests.
pub trait TermSink: Send + Sync + 'static {
    fn data(&self, bytes: &[u8]);
    fn status(&self, status: SessionStatus);
}

/// Lifecycle notices for one pane.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SessionStatus {
    Connecting,
    /// Emitted once per host (target or jump) whose key is seen for the first
    /// time and recorded.
    NewHostKey {
        host: String,
        fingerprint: String,
    },
    Connected,
    /// The remote shell exited or the connection dropped. `code` is the exit
    /// status when the server reported one.
    Disconnected {
        code: Option<u32>,
    },
    /// A pinned host key no longer matches. Carries enough for the UI to
    /// offer a deliberate "trust the new key" action.
    HostKeyChanged {
        host: String,
        port: u16,
        fingerprint: String,
    },
    Error {
        message: String,
    },
}

impl SshError {
    /// The changed-host-key error inside this one, looking through jump
    /// wrappers.
    pub fn host_key_changed(&self) -> Option<(&str, u16, &str)> {
        match self {
            SshError::HostKeyChanged {
                host,
                port,
                fingerprint,
                ..
            } => Some((host, *port, fingerprint)),
            SshError::Jump { source, .. } => source.host_key_changed(),
            _ => None,
        }
    }
}

/// Where to connect and how to log in. Shared by terminals, SFTP and
/// port forwarding.
#[derive(Clone)]
pub struct Target {
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    /// Where trusted host keys live and who to ask about new or changed ones.
    pub host_keys: HostKeyPolicy,
    /// Optional bastion to tunnel through (like OpenSSH `ProxyJump`). May
    /// itself have a jump, forming a chain; the outermost hop is dialled
    /// directly.
    pub jump: Option<Box<Target>>,
    /// Let the remote shell use this computer's ssh-agent (`ssh -A`). Only
    /// honoured for the final hop's interactive session.
    pub forward_agent: bool,
    /// Serve forwarded agent requests from this backend (the vault agent)
    /// instead of the system's ssh-agent.
    pub agent_backend: Option<Arc<dyn crate::agent::Backend>>,
    /// Show the host's graphical programs on this computer's X server
    /// (`ssh -X`). Only honoured for the final hop's interactive session.
    pub forward_x11: bool,
    /// How to reach this hop when it's dialled directly (the outermost hop).
    pub proxy: Option<crate::models::ProxySpec>,
    /// Keep-alive interval (OpenSSH `ServerAliveInterval`); default 30 s.
    pub keepalive_secs: Option<u32>,
}

/// A host key recorded for the first time during a connection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LearnedKey {
    pub host: String,
    pub fingerprint: String,
}

/// A live, authenticated connection plus any jump-host connections it is
/// tunnelled through. Derefs to the final hop's russh handle.
pub struct Client {
    handle: Handle<ClientHandler>,
    /// Outermost first. Must outlive `handle`, which rides on their channels.
    jumps: Vec<(String, Handle<ClientHandler>)>,
    /// X11 forwarding state when requested, or why it couldn't be set up.
    x11: Option<Result<Arc<crate::x11::X11Setup>, String>>,
}

impl Deref for Client {
    type Target = Handle<ClientHandler>;
    fn deref(&self) -> &Self::Target {
        &self.handle
    }
}

impl Client {
    /// True if the final hop or any jump in front of it has gone away.
    pub fn is_closed(&self) -> bool {
        self.handle.is_closed() || self.jumps.iter().any(|(_, j)| j.is_closed())
    }

    /// Names of the jump hosts, outermost first.
    pub fn jump_hosts(&self) -> Vec<String> {
        self.jumps.iter().map(|(h, _)| h.clone()).collect()
    }

    /// Disconnect the final hop, then each jump from the inside out.
    pub async fn close(&self) {
        let _ = self
            .handle
            .disconnect(Disconnect::ByApplication, "", "en")
            .await;
        for (_, j) in self.jumps.iter().rev() {
            let _ = j.disconnect(Disconnect::ByApplication, "", "en").await;
        }
    }
}

/// Everything needed to open one interactive shell.
pub struct ConnectParams {
    pub target: Target,
    pub cols: u32,
    pub rows: u32,
}

/// Remote-forward table: server-side bound port -> local destination.
/// Consulted when the server opens a `forwarded-tcpip` channel to us.
pub type RemoteForwards = Arc<Mutex<HashMap<u32, (String, u16)>>>;

/// Messages from the UI into a running session.
#[derive(Debug)]
pub enum Cmd {
    Write(Vec<u8>),
    Resize { cols: u32, rows: u32 },
    Close,
}

struct SessionHandle {
    tx: mpsc::Sender<Cmd>,
    /// The live connection, once there is one, for questions that need a channel of their own.
    client: Arc<std::sync::OnceLock<Arc<Client>>>,
    lookups: Arc<crate::completion::Lookups>,
}

/// Registry of live sessions keyed by pane id.
#[derive(Default)]
pub struct SshManager {
    sessions: Mutex<HashMap<String, SessionHandle>>,
}

impl SshManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn a session task for `pane_id`. Must be called from within a tokio
    /// runtime. Returns as soon as the task is registered; connection progress
    /// is reported through `sink`.
    pub fn connect(
        self: &Arc<Self>,
        pane_id: String,
        params: ConnectParams,
        sink: Arc<dyn TermSink>,
    ) -> Result<(), SshError> {
        let (tx, rx) = mpsc::channel::<Cmd>(256);
        let client_cell: Arc<std::sync::OnceLock<Arc<Client>>> = Arc::default();
        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            if sessions.contains_key(&pane_id) {
                return Err(SshError::AlreadyConnected);
            }
            sessions.insert(pane_id.clone(), SessionHandle { tx, client: Arc::clone(&client_cell), lookups: Arc::new(crate::completion::Lookups::new()) });
        }

        let manager = Arc::clone(self);
        tokio::spawn(async move {
            sink.status(SessionStatus::Connecting);
            let outcome = run_session(params, rx, sink.as_ref(), client_cell).await;
            match outcome {
                Ok(code) => sink.status(SessionStatus::Disconnected { code }),
                Err(e) => match e.host_key_changed() {
                    Some((host, port, fingerprint)) => sink.status(SessionStatus::HostKeyChanged {
                        host: host.to_string(),
                        port,
                        fingerprint: fingerprint.to_string(),
                    }),
                    None => sink.status(SessionStatus::Error {
                        message: e.to_string(),
                    }),
                },
            }
            manager
                .sessions
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&pane_id);
        });
        Ok(())
    }

    fn sender(&self, pane_id: &str) -> Result<mpsc::Sender<Cmd>, SshError> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(pane_id)
            .map(|s| s.tx.clone())
            .ok_or(SshError::NotConnected)
    }

    /// Ask the pane's host a question over an extra channel of the pane's connection (see `completion`).
    /// A pane that isn't an SSH session, or isn't connected yet, answers `Refused`, quietly.
    pub async fn lookup(
        &self,
        pane_id: &str,
        request: &crate::completion::Request,
    ) -> Result<crate::completion::Reply, crate::completion::LookupError> {
        let (client, lookups) = {
            let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            let s = sessions.get(pane_id).ok_or(crate::completion::LookupError::Refused)?;
            (s.client.get().cloned(), Arc::clone(&s.lookups))
        };
        let client = client.ok_or(crate::completion::LookupError::Refused)?;
        lookups.lookup(&client, request).await
    }

    pub async fn write(&self, pane_id: &str, bytes: Vec<u8>) -> Result<(), SshError> {
        self.sender(pane_id)?
            .send(Cmd::Write(bytes))
            .await
            .map_err(|_| SshError::Gone)
    }

    pub async fn resize(&self, pane_id: &str, cols: u32, rows: u32) -> Result<(), SshError> {
        self.sender(pane_id)?
            .send(Cmd::Resize { cols, rows })
            .await
            .map_err(|_| SshError::Gone)
    }

    /// Ask the session to close. A pane that is not connected is not an error,
    /// because the UI calls this unconditionally on unmount.
    pub async fn disconnect(&self, pane_id: &str) {
        if let Ok(tx) = self.sender(pane_id) {
            let _ = tx.send(Cmd::Close).await;
        }
    }

    pub fn is_connected(&self, pane_id: &str) -> bool {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(pane_id)
    }

    /// Close every session, e.g. when the vault is locked.
    pub async fn disconnect_all(&self) {
        let senders: Vec<_> = self
            .sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .map(|s| s.tx.clone())
            .collect();
        for tx in senders {
            let _ = tx.send(Cmd::Close).await;
        }
    }
}

// ---------------------------------------------------------------------------
// Host key verification
// ---------------------------------------------------------------------------

/// A server's host key as presented during the handshake.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PresentedKey {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    /// OpenSSH public key line (`ssh-ed25519 AAAA...`).
    pub public_key: String,
}

impl PresentedKey {
    pub fn new(host: &str, port: u16, key: &PublicKey) -> Result<Self, keys::ssh_key::Error> {
        Ok(Self {
            host: host.to_string(),
            port,
            algorithm: key.algorithm().to_string(),
            fingerprint: key.fingerprint(HashAlg::Sha256).to_string(),
            public_key: PublicKey::new(key.key_data().clone(), "").to_openssh()?,
        })
    }
}

/// The key that was trusted before a change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreviousKey {
    pub algorithm: String,
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostKeyStatus {
    Trusted,
    /// Never seen: needs an explicit "trust" from the user.
    Unknown,
    /// A different key was trusted for this host. Possible attack.
    Changed { previous: PreviousKey },
}

/// Where trusted host keys are kept: the vault in the app, a file in tests.
pub trait HostKeyStore: Send + Sync {
    fn check(&self, key: &PresentedKey) -> Result<HostKeyStatus, String>;
    /// Trust `key` for its host, replacing any earlier key.
    fn trust(&self, key: &PresentedKey) -> Result<(), String>;
}

/// What the user is asked when a key is new or has changed.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HostKeyQuestion {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    pub fingerprint: String,
    /// Set when the key CHANGED: the one trusted before.
    pub previous: Option<PreviousKey>,
}

pub type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Asks the user to trust a host key. `true` = trust (or replace).
pub trait HostKeyPrompter: Send + Sync {
    fn ask(&self, question: HostKeyQuestion) -> BoxFuture<bool>;
}

#[derive(Clone)]
pub struct HostKeyPolicy {
    pub store: Arc<dyn HostKeyStore>,
    /// `None` refuses unknown and changed keys without asking.
    pub prompter: Option<Arc<dyn HostKeyPrompter>>,
}

/// OpenSSH-format `known_hosts` file. Used by tests and by tools that
/// check against the user's own `~/.ssh/known_hosts`.
pub struct FileHostKeys(pub PathBuf);

impl HostKeyStore for FileHostKeys {
    fn check(&self, key: &PresentedKey) -> Result<HostKeyStatus, String> {
        let pk = PublicKey::from_openssh(&key.public_key).map_err(|e| e.to_string())?;
        match check_known_hosts_path(&key.host, key.port, &pk, &self.0) {
            Ok(true) => Ok(HostKeyStatus::Trusted),
            Ok(false) => Ok(HostKeyStatus::Unknown),
            Err(keys::Error::KeyChanged { line }) => {
                let token = crate::knownhosts::host_token(&key.host, key.port);
                let previous = crate::knownhosts::list(&self.0)
                    .ok()
                    .and_then(|l| {
                        l.into_iter()
                            .find(|k| k.line == line || k.hosts.contains(&token))
                    })
                    .map(|k| PreviousKey {
                        algorithm: k.algorithm,
                        fingerprint: k.fingerprint.unwrap_or_default(),
                    })
                    .unwrap_or(PreviousKey {
                        algorithm: String::new(),
                        fingerprint: String::new(),
                    });
                Ok(HostKeyStatus::Changed { previous })
            }
            Err(keys::Error::IO(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok(HostKeyStatus::Unknown)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    fn trust(&self, key: &PresentedKey) -> Result<(), String> {
        let pk = PublicKey::from_openssh(&key.public_key).map_err(|e| e.to_string())?;
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = crate::knownhosts::forget(&self.0, &key.host, key.port);
        learn_known_hosts_path(&key.host, key.port, &pk, &self.0).map_err(|e| e.to_string())
    }
}

/// Answers every question the same way; for tests and scripted tools.
pub struct FixedAnswer {
    pub trust_unknown: bool,
    pub replace_changed: bool,
}

impl HostKeyPrompter for FixedAnswer {
    fn ask(&self, q: HostKeyQuestion) -> BoxFuture<bool> {
        let yes = if q.previous.is_some() {
            self.replace_changed
        } else {
            self.trust_unknown
        };
        Box::pin(async move { yes })
    }
}

impl HostKeyPolicy {
    /// Trust-on-first-use against a file; changed keys are refused.
    pub fn tofu_file(path: PathBuf) -> Self {
        Self {
            store: Arc::new(FileHostKeys(path)),
            prompter: Some(Arc::new(FixedAnswer {
                trust_unknown: true,
                replace_changed: false,
            })),
        }
    }
}

pub struct ClientHandler {
    host: String,
    port: u16,
    host_keys: HostKeyPolicy,
    /// Set when a key was learned during this connection. Shared with the
    /// session task, which reports it after authentication succeeds.
    learned_fingerprint: Arc<Mutex<Option<String>>>,
    remote_forwards: Option<RemoteForwards>,
    /// Whether this connection asked for agent forwarding. The server may
    /// only open agent channels when it did.
    forward_agent: bool,
    agent_backend: Option<Arc<dyn crate::agent::Backend>>,
    /// Set only when this connection asked for X11 forwarding. The server may
    /// only open X11 channels when it is.
    x11: Option<Arc<crate::x11::X11Setup>>,
}

impl ClientHandler {
    fn store_err(&self, reason: String) -> SshError {
        SshError::HostKeyStore {
            host: self.host.clone(),
            reason,
        }
    }

    fn learn(&mut self, key: &PresentedKey) -> Result<bool, SshError> {
        self.host_keys.store.trust(key).map_err(|e| self.store_err(e))?;
        *self
            .learned_fingerprint
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(key.fingerprint.clone());
        Ok(true)
    }
}

impl client::Handler for ClientHandler {
    type Error = SshError;

    async fn check_server_key(
        &mut self,
        server_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key: PublicKey = match server_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key.clone(),
            PublicKeyOrCertificate::Certificate(cert) => {
                PublicKey::new(cert.public_key().clone(), "")
            }
        };
        let presented = PresentedKey::new(&self.host, self.port, &key)
            .map_err(|e| self.store_err(e.to_string()))?;
        let status = self
            .host_keys
            .store
            .check(&presented)
            .map_err(|e| self.store_err(e))?;
        let previous = match status {
            HostKeyStatus::Trusted => return Ok(true),
            HostKeyStatus::Unknown => None,
            HostKeyStatus::Changed { previous } => Some(previous),
        };
        let changed = previous.is_some();
        // Nobody to ask (e.g. a background job): refuse rather than guess.
        let trusted = match &self.host_keys.prompter {
            Some(p) => {
                p.ask(HostKeyQuestion {
                    host: self.host.clone(),
                    port: self.port,
                    algorithm: presented.algorithm.clone(),
                    fingerprint: presented.fingerprint.clone(),
                    previous,
                })
                .await
            }
            None => false,
        };
        if trusted {
            return self.learn(&presented);
        }
        Err(if changed {
            SshError::HostKeyChanged {
                host: self.host.clone(),
                port: self.port,
                fingerprint: presented.fingerprint,
            }
        } else {
            SshError::HostKeyUnknown {
                host: display_host_port(&self.host, self.port),
                fingerprint: presented.fingerprint,
            }
        })
    }

    /// Like agent channels, russh accepts X11 channels by default. Only accept
    /// them when this connection requested X11 forwarding.
    async fn server_channel_open_x11(
        &mut self,
        channel: russh::Channel<client::Msg>,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let Some(setup) = self.x11.clone() else {
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            let _ = crate::x11::forward(
                channel.into_stream(),
                setup.display.clone(),
                setup.fake.clone(),
                setup.real.clone(),
            )
            .await;
        });
        Ok(())
    }

    /// russh's default accepts these unconditionally, which would hand our
    /// agent to any server that asks. Only accept when we requested it.
    async fn server_channel_open_agent_forward(
        &mut self,
        channel: russh::Channel<client::Msg>,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        if !self.forward_agent {
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        reply.accept().await;
        if let Some(backend) = self.agent_backend.clone() {
            // The vault agent answers, naming this host in any approval prompt.
            let origin = display_host_port(&self.host, self.port);
            tokio::spawn(crate::agent::serve_conn_from(channel.into_stream(), backend, Some(origin)));
            return Ok(());
        }
        tokio::spawn(async move {
            match local_agent().await {
                Ok(mut agent) => {
                    let mut remote = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut agent, &mut remote).await;
                }
                Err(_) => {
                    let _ = channel.close().await;
                }
            }
        });
        Ok(())
    }

    async fn server_channel_open_forwarded_tcpip(
        &mut self,
        channel: russh::Channel<client::Msg>,
        connected_address: &str,
        connected_port: u32,
        _originator_address: &str,
        _originator_port: u32,
        reply: client::ChannelOpenHandle,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let dest = self.remote_forwards.as_ref().and_then(|t| {
            t.lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&connected_port)
                .cloned()
        });
        let Some((host, port)) = dest else {
            let _ = connected_address;
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        };
        reply.accept().await;
        tokio::spawn(async move {
            match tokio::net::TcpStream::connect((host.as_str(), port)).await {
                Ok(mut local) => {
                    let mut remote = channel.into_stream();
                    let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
                }
                Err(_) => {
                    let _ = channel.close().await;
                }
            }
        });
        Ok(())
    }
}

/// Connect, verify host keys, and authenticate, tunnelling through the
/// target's jump chain if it has one. Returns the live client and every host
/// key learned along the way (jumps first).
pub async fn open_client(
    target: &Target,
    remote_forwards: Option<RemoteForwards>,
) -> Result<(Client, Vec<LearnedKey>), SshError> {
    // Flatten the chain, outermost hop first.
    let mut hops: Vec<&Target> = Vec::new();
    let mut cur = target;
    while let Some(j) = cur.jump.as_deref() {
        hops.push(j);
        if hops.len() > MAX_JUMPS {
            return Err(SshError::JumpChainTooLong);
        }
        cur = j;
    }
    hops.reverse();

    let mut learned = Vec::new();
    let mut jumps: Vec<(String, Handle<ClientHandler>)> = Vec::new();
    for hop in hops {
        let via = jumps.last().map(|(name, h)| (name.as_str(), h));
        let (h, fp) = connect_hop(hop, via, None, None)
            .await
            .map_err(|e| match e {
                // Already names the jump that failed to reach this hop.
                e @ SshError::JumpUnreachable { .. } => e,
                e => SshError::Jump {
                    host: display_host(hop),
                    source: Box::new(e),
                },
            })?;
        learned.extend(fp);
        jumps.push((display_host(hop), h));
    }

    let x11 = target
        .forward_x11
        .then(|| crate::x11::X11Setup::from_env().map(Arc::new));
    let x11_setup = x11.as_ref().and_then(|r| r.as_ref().ok()).cloned();
    let via = jumps.last().map(|(name, h)| (name.as_str(), h));
    let (handle, fp) = connect_hop(target, via, remote_forwards, x11_setup).await?;
    learned.extend(fp);
    Ok((Client { handle, jumps, x11 }, learned))
}

fn display_host(t: &Target) -> String {
    display_host_port(&t.hostname, t.port)
}

fn display_host_port(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("{host}:{port}")
    }
}

/// One hop: dial directly, or through `via` with a `direct-tcpip` channel.
async fn connect_hop(
    target: &Target,
    via: Option<(&str, &Handle<ClientHandler>)>,
    remote_forwards: Option<RemoteForwards>,
    x11: Option<Arc<crate::x11::X11Setup>>,
) -> Result<(Handle<ClientHandler>, Option<LearnedKey>), SshError> {
    let addr = format!("{}:{}", target.hostname, target.port);
    let learned = Arc::new(Mutex::new(None));
    let handler = ClientHandler {
        host: target.hostname.clone(),
        port: target.port,
        host_keys: target.host_keys.clone(),
        learned_fingerprint: Arc::clone(&learned),
        remote_forwards,
        forward_agent: target.forward_agent,
        agent_backend: target.agent_backend.clone(),
        x11,
    };
    let timeout = || SshError::Timeout(addr.clone());

    let mut handle = match via {
        None => {
            let stream = tokio::time::timeout(CONNECT_TIMEOUT, crate::dial::dial(target))
                .await
                .map_err(|_| timeout())??;
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                client::connect_stream(client_config(target), stream, handler),
            )
            .await
            .map_err(|_| timeout())??
        }
        Some((jump_name, jump)) => {
            let channel = tokio::time::timeout(
                CONNECT_TIMEOUT,
                jump.channel_open_direct_tcpip(
                    target.hostname.clone(),
                    u32::from(target.port),
                    "127.0.0.1",
                    0,
                ),
            )
            .await
            .map_err(|_| timeout())?
            .map_err(|e| SshError::JumpUnreachable {
                jump: jump_name.to_string(),
                addr: addr.clone(),
                reason: e.to_string(),
            })?;
            tokio::time::timeout(
                CONNECT_TIMEOUT,
                client::connect_stream(client_config(target), channel.into_stream(), handler),
            )
            .await
            .map_err(|_| timeout())??
        }
    };

    authenticate(&mut handle, target).await?;
    let fp = learned
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .take()
        .map(|fingerprint| LearnedKey {
            host: display_host(target),
            fingerprint,
        });
    Ok((handle, fp))
}

/// Connect to this computer's ssh-agent, for agent forwarding.
#[cfg(unix)]
async fn local_agent() -> std::io::Result<tokio::net::UnixStream> {
    let path = std::env::var_os("SSH_AUTH_SOCK")
        .ok_or_else(|| std::io::Error::other("SSH_AUTH_SOCK is not set"))?;
    tokio::net::UnixStream::connect(path).await
}

#[cfg(windows)]
async fn local_agent() -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    tokio::net::windows::named_pipe::ClientOptions::new().open(r"\\.\pipe\openssh-ssh-agent")
}

// ---------------------------------------------------------------------------
// Session task
// ---------------------------------------------------------------------------

fn client_config(target: &Target) -> Arc<client::Config> {
    let keepalive = match target.keepalive_secs {
        Some(0) => None,
        Some(s) => Some(Duration::from_secs(u64::from(s))),
        None => Some(KEEPALIVE),
    };
    Arc::new(client::Config {
        keepalive_interval: keepalive,
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    })
}

async fn run_session(
    params: ConnectParams,
    mut rx: mpsc::Receiver<Cmd>,
    sink: &dyn TermSink,
    client_cell: Arc<std::sync::OnceLock<Arc<Client>>>,
) -> Result<Option<u32>, SshError> {
    let (handle, learned) = open_client(&params.target, None).await?;
    let handle = Arc::new(handle);
    let _ = client_cell.set(Arc::clone(&handle));

    // Host key notices are only worth showing once we know the login worked.
    for k in learned {
        sink.status(SessionStatus::NewHostKey {
            host: k.host,
            fingerprint: k.fingerprint,
        });
    }

    let channel = handle.channel_open_session().await?;
    channel
        .request_pty(true, TERM, params.cols, params.rows, 0, 0, &[])
        .await?;
    if params.target.forward_agent {
        channel.agent_forward(true).await?;
    }
    match &handle.x11 {
        Some(Ok(x)) => {
            channel
                .request_x11(
                    true,
                    false,
                    crate::x11::PROTO,
                    crate::x11::hex_encode(&x.fake),
                    x.display.screen(),
                )
                .await?;
        }
        Some(Err(why)) => {
            sink.data(format!("\x1b[33m[X11 forwarding is off: {why}]\x1b[0m\r\n").as_bytes())
        }
        None => {}
    }
    channel.request_shell(true).await?;
    sink.status(SessionStatus::Connected);

    let (mut reader, writer) = channel.split();
    let mut exit_code: Option<u32> = None;

    loop {
        tokio::select! {
            msg = reader.wait() => match msg {
                Some(ChannelMsg::Data { data }) => sink.data(&data),
                Some(ChannelMsg::ExtendedData { data, .. }) => sink.data(&data),
                Some(ChannelMsg::ExitStatus { exit_status }) => exit_code = Some(exit_status),
                Some(ChannelMsg::Eof) | Some(ChannelMsg::Close) | None => break,
                Some(_) => {}
            },
            cmd = rx.recv() => match cmd {
                Some(Cmd::Write(bytes)) => {
                    if writer.data_bytes(bytes).await.is_err() { break; }
                }
                Some(Cmd::Resize { cols, rows }) => {
                    let _ = writer.window_change(cols, rows, 0, 0).await;
                }
                Some(Cmd::Close) | None => {
                    let _ = writer.eof().await;
                    let _ = writer.close().await;
                    break;
                }
            },
        }
    }

    handle.close().await;
    Ok(exit_code)
}

async fn authenticate(handle: &mut Handle<ClientHandler>, params: &Target) -> Result<(), SshError> {
    let user = params.username.as_str();
    let fail = |method: &'static str| SshError::AuthFailed {
        user: params.username.clone(),
        host: params.hostname.clone(),
        method,
    };

    match &params.auth {
        AuthMethod::Password { password } => {
            let password = Zeroizing::new(password.clone());
            if handle
                .authenticate_password(user, password.as_str())
                .await?
                .success()
            {
                return Ok(());
            }
            // Many servers only offer keyboard-interactive for passwords.
            if keyboard_interactive(handle, user, &password).await? {
                return Ok(());
            }
            Err(fail("password"))
        }

        AuthMethod::PrivateKey {
            private_key,
            passphrase,
            certificate,
        } => {
            let pem = Zeroizing::new(private_key.clone());
            key_login(handle, params, &pem, passphrase.as_deref(), certificate.as_deref()).await
        }

        AuthMethod::KeyFile { path, passphrase } => {
            // Read at connect time: the file stays on this computer, never
            // in the synced vault.
            let pem = Zeroizing::new(std::fs::read_to_string(path).map_err(|e| SshError::KeyFile {
                path: path.clone(),
                reason: e.to_string(),
            })?);
            key_login(handle, params, &pem, passphrase.as_deref(), None).await
        }

        // Key Manager references are resolved into `PrivateKey` before a
        // connection is attempted; reaching here is a programming error.
        AuthMethod::Key { .. } => Err(SshError::KeyFile {
            path: "(key manager)".into(),
            reason: "key reference was not resolved".into(),
        }),

        AuthMethod::Agent => authenticate_with_agent(handle, params).await,
    }
}

/// Public-key login, with an OpenSSH certificate when one is attached.
async fn key_login(
    handle: &mut Handle<ClientHandler>,
    params: &Target,
    pem: &str,
    passphrase: Option<&str>,
    certificate: Option<&str>,
) -> Result<(), SshError> {
    let key = Arc::new(keys::decode_secret_key(pem, passphrase)?);
    let user = params.username.as_str();
    let result = match certificate {
        Some(cert) => {
            let cert = keys::Certificate::from_openssh(cert.trim()).map_err(|e| SshError::KeyFile {
                path: "(certificate)".into(),
                reason: e.to_string(),
            })?;
            handle.authenticate_openssh_cert(user, key, cert).await?
        }
        None => {
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            handle
                .authenticate_publickey(user, PrivateKeyWithHashAlg::new(key, hash))
                .await?
        }
    };
    if result.success() {
        Ok(())
    } else {
        Err(SshError::AuthFailed {
            user: params.username.clone(),
            host: params.hostname.clone(),
            method: if certificate.is_some() { "certificate" } else { "private key" },
        })
    }
}

async fn keyboard_interactive(
    handle: &mut Handle<ClientHandler>,
    user: &str,
    password: &str,
) -> Result<bool, SshError> {
    use russh::client::KeyboardInteractiveAuthResponse as R;
    let mut resp = handle
        .authenticate_keyboard_interactive_start(user, None)
        .await?;
    for _ in 0..5 {
        match resp {
            R::Success => return Ok(true),
            R::Failure { .. } => return Ok(false),
            R::InfoRequest { prompts, .. } => {
                let answers: Vec<String> = prompts.iter().map(|_| password.to_string()).collect();
                resp = handle
                    .authenticate_keyboard_interactive_respond(answers)
                    .await?;
            }
        }
    }
    Ok(false)
}

#[cfg(unix)]
async fn authenticate_with_agent(
    handle: &mut Handle<ClientHandler>,
    params: &Target,
) -> Result<(), SshError> {
    use keys::agent::client::AgentClient;
    let mut agent = AgentClient::connect_env()
        .await
        .map_err(|e| SshError::NoAgent(e.to_string()))?;
    agent_auth_loop(handle, params, &mut agent).await
}

#[cfg(windows)]
async fn authenticate_with_agent(
    handle: &mut Handle<ClientHandler>,
    params: &Target,
) -> Result<(), SshError> {
    use keys::agent::client::AgentClient;
    // OpenSSH for Windows exposes the agent on a well-known named pipe.
    match AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent").await {
        Ok(mut agent) => agent_auth_loop(handle, params, &mut agent).await,
        Err(pipe_err) => match AgentClient::connect_pageant().await {
            Ok(mut agent) => agent_auth_loop(handle, params, &mut agent).await,
            Err(_) => Err(SshError::NoAgent(pipe_err.to_string())),
        },
    }
}

async fn agent_auth_loop<S>(
    handle: &mut Handle<ClientHandler>,
    params: &Target,
    agent: &mut keys::agent::client::AgentClient<S>,
) -> Result<(), SshError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send,
{
    let identities = agent
        .request_identities()
        .await
        .map_err(|e| SshError::NoAgent(e.to_string()))?;
    if identities.is_empty() {
        return Err(SshError::AgentNoKeys);
    }
    let hash = handle.best_supported_rsa_hash().await?.flatten();
    for id in identities {
        let key = id.public_key().into_owned();
        let result = handle
            .authenticate_publickey_with(params.username.as_str(), key, hash, agent)
            .await
            .map_err(|e| SshError::NoAgent(e.to_string()))?;
        if result.success() {
            return Ok(());
        }
    }
    Err(SshError::AuthFailed {
        user: params.username.clone(),
        host: params.hostname.clone(),
        method: "ssh-agent",
    })
}

/// Shared test fixture: a throw-away, user-level `sshd` on a random port.
#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use std::process::{Child, Command, Stdio};
    use std::time::Instant;

    pub struct Sshd {
        child: Child,
        pub port: u16,
        pub user: String,
        pub client_key: String,
        pub other_key: String,
        /// OpenSSH user certificate for `other_key`, signed by a CA the
        /// server trusts. `other_key` itself isn't in authorized_keys.
        pub other_cert: Option<String>,
    }
    impl Drop for Sshd {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Returns `None` (and the caller should skip) when sshd is unavailable,
    /// or when `SSHVAULT_SKIP_SSHD_TESTS` is set (used on macOS CI, whose
    /// sshd needs a different setup).
    pub fn spawn_sshd(dir: &std::path::Path) -> Option<Sshd> {
        spawn_sshd_with(dir, true, "")
    }

    /// A server with extra lines in its `sshd_config` (for example `MaxSessions 1`).
    pub fn spawn_sshd_config(dir: &std::path::Path, extra: &str) -> Option<Sshd> {
        spawn_sshd_with(dir, true, extra)
    }

    /// A server with the SFTP subsystem removed, as some hosts are set up.
    #[cfg_attr(not(unix), allow(dead_code))]
    pub fn spawn_sshd_without_sftp(dir: &std::path::Path) -> Option<Sshd> {
        spawn_sshd_with(dir, false, "")
    }

    fn spawn_sshd_with(dir: &std::path::Path, with_sftp: bool, extra: &str) -> Option<Sshd> {
        // CI sets the variable on every OS, empty where tests should run.
        if std::env::var_os("SSHVAULT_SKIP_SSHD_TESTS").is_some_and(|v| !v.is_empty()) {
            return None;
        }
        let sshd = ["/usr/sbin/sshd", "/usr/bin/sshd", "/usr/local/sbin/sshd"]
            .iter()
            .find(|p| std::path::Path::new(p).exists())?;
        let keygen = |name: &str| {
            Command::new("ssh-keygen")
                .args(["-q", "-t", "ed25519", "-N", "", "-f"])
                .arg(dir.join(name))
                .status()
                .ok()
                .filter(|s| s.success())
        };
        keygen("host_key")?;
        keygen("client_key")?;
        keygen("other_key")?;
        keygen("user_ca")?;
        std::fs::copy(dir.join("client_key.pub"), dir.join("authorized_keys")).ok()?;
        let user = std::env::var("USER").ok().or_else(|| {
            Command::new("id")
                .arg("-un")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })?;
        // Certificate for other_key, valid for this user for an hour.
        let signed = Command::new("ssh-keygen")
            .args(["-q", "-s"])
            .arg(dir.join("user_ca"))
            .args(["-I", "sshvault-test", "-n", &user, "-V", "-5m:+1h"])
            .arg(dir.join("other_key.pub"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());

        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .ok()?
            .local_addr()
            .ok()?
            .port();
        let sftp_server = [
            "/usr/lib/openssh/sftp-server",
            "/usr/libexec/openssh/sftp-server",
            "/usr/libexec/sftp-server",
        ]
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .map(|p| format!("Subsystem sftp {p}\n"))
        .unwrap_or_else(|| "Subsystem sftp internal-sftp\n".into());
        let sftp_server = if with_sftp { sftp_server } else { String::new() };
        let config = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host_key\nAuthorizedKeysFile {d}/authorized_keys\n\
             TrustedUserCAKeys {d}/user_ca.pub\n\
             PasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\nPidFile none\n\
             AllowTcpForwarding yes\nX11Forwarding yes\nX11UseLocalhost yes\n{sftp_server}{extra}\n",
            d = dir.display()
        );
        std::fs::write(dir.join("sshd_config"), config).ok()?;
        let child = Command::new(sshd)
            .args(["-f"])
            .arg(dir.join("sshd_config"))
            .args(["-D", "-e"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;

        let deadline = Instant::now() + Duration::from_secs(5);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            if Instant::now() > deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Some(Sshd {
            child,
            port,
            user,
            client_key: std::fs::read_to_string(dir.join("client_key")).ok()?,
            other_key: std::fs::read_to_string(dir.join("other_key")).ok()?,
            other_cert: signed
                .then(|| std::fs::read_to_string(dir.join("other_key-cert.pub")).ok())
                .flatten(),
        })
    }

    pub fn target(s: &Sshd, key: &str, known_hosts: PathBuf) -> Target {
        Target {
            hostname: "127.0.0.1".into(),
            port: s.port,
            username: s.user.clone(),
            auth: AuthMethod::PrivateKey {
                private_key: key.to_string(),
                passphrase: None,
                certificate: None,
            },
            host_keys: HostKeyPolicy::tofu_file(known_hosts),
            jump: None,
            forward_agent: false,
            agent_backend: None,
            forward_x11: false,
            proxy: None,
            keepalive_secs: None,
        }
    }
}

/// A placeholder target for unit tests of the dialling code.
#[cfg(test)]
pub fn testutil_target() -> Target {
    Target {
        hostname: String::new(),
        port: 22,
        username: "test".into(),
        auth: AuthMethod::Agent,
        host_keys: HostKeyPolicy::tofu_file(PathBuf::new()),
        jump: None,
        forward_agent: false,
        agent_backend: None,
        forward_x11: false,
        proxy: None,
        keepalive_secs: None,
    }
}

#[cfg(test)]
mod tests {
    //! End-to-end against a throw-away, user-level `sshd` on a random port.
    //! Skipped (not failed) when `sshd` or `ssh-keygen` is unavailable.
    use super::testutil::*;
    use super::*;
    use std::sync::mpsc as std_mpsc;
    use std::time::Instant;

    #[derive(Debug)]
    enum Ev {
        Data(Vec<u8>),
        Status(SessionStatus),
    }

    struct TestSink(std_mpsc::Sender<Ev>);
    impl TermSink for TestSink {
        fn data(&self, bytes: &[u8]) {
            let _ = self.0.send(Ev::Data(bytes.to_vec()));
        }
        fn status(&self, status: SessionStatus) {
            let _ = self.0.send(Ev::Status(status));
        }
    }

    fn params(s: &Sshd, key: &str, known_hosts: PathBuf) -> ConnectParams {
        ConnectParams {
            target: target(s, key, known_hosts),
            cols: 80,
            rows: 24,
        }
    }

    fn wait_status(
        rx: &std_mpsc::Receiver<Ev>,
        pred: impl Fn(&SessionStatus) -> bool,
    ) -> SessionStatus {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx
                .recv_timeout(remaining)
                .expect("timed out waiting for status")
            {
                Ev::Status(s) if pred(&s) => return s,
                Ev::Status(SessionStatus::Error { message }) => {
                    panic!("unexpected error: {message}")
                }
                _ => {}
            }
        }
    }

    fn wait_output(rx: &std_mpsc::Receiver<Ev>, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut buf = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match rx
                .recv_timeout(remaining)
                .expect("timed out waiting for output")
            {
                Ev::Data(d) => {
                    buf.extend_from_slice(&d);
                    let s = String::from_utf8_lossy(&buf);
                    if s.contains(needle) {
                        return s.into_owned();
                    }
                }
                Ev::Status(SessionStatus::Error { message }) => {
                    panic!("unexpected error: {message}")
                }
                _ => {}
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shell_roundtrip_tofu_and_auth_failures() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let known_hosts = dir.path().join("app").join("known_hosts");
        let manager = Arc::new(SshManager::new());

        // 1. First connection: key learned, shell works, clean close.
        let (tx, rx) = std_mpsc::channel();
        manager
            .connect(
                "p1".into(),
                params(&sshd, &sshd.client_key, known_hosts.clone()),
                Arc::new(TestSink(tx)),
            )
            .unwrap();
        wait_status(&rx, |s| matches!(s, SessionStatus::Connecting));
        let new_key = wait_status(&rx, |s| matches!(s, SessionStatus::NewHostKey { .. }));
        assert!(
            matches!(new_key, SessionStatus::NewHostKey { ref fingerprint, .. } if fingerprint.starts_with("SHA256:"))
        );
        wait_status(&rx, |s| matches!(s, SessionStatus::Connected));
        assert!(manager.is_connected("p1"));
        assert!(manager
            .connect(
                "p1".into(),
                params(&sshd, &sshd.client_key, known_hosts.clone()),
                Arc::new(TestSink(std_mpsc::channel().0))
            )
            .is_err());

        manager
            .write("p1", b"printf 'A%sB\\n' _OK_\n".to_vec())
            .await
            .unwrap();
        let out = wait_output(&rx, "A_OK_B");
        assert!(out.contains("A_OK_B"));
        manager.resize("p1", 120, 40).await.unwrap();
        manager.write("p1", b"stty size\n".to_vec()).await.unwrap();
        wait_output(&rx, "40 120");

        manager.disconnect("p1").await;
        wait_status(&rx, |s| matches!(s, SessionStatus::Disconnected { .. }));
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!manager.is_connected("p1"));
        assert!(known_hosts.is_file());

        // 2. Second connection: key already known, no NewHostKey notice.
        let (tx, rx) = std_mpsc::channel();
        manager
            .connect(
                "p2".into(),
                params(&sshd, &sshd.client_key, known_hosts.clone()),
                Arc::new(TestSink(tx)),
            )
            .unwrap();
        let s = wait_status(&rx, |s| {
            matches!(
                s,
                SessionStatus::Connected | SessionStatus::NewHostKey { .. }
            )
        });
        assert!(matches!(s, SessionStatus::Connected));
        manager.disconnect("p2").await;
        wait_status(&rx, |s| matches!(s, SessionStatus::Disconnected { .. }));

        // 3. Wrong key: authentication failure surfaces as an error status.
        let (tx, rx) = std_mpsc::channel();
        manager
            .connect(
                "p3".into(),
                params(&sshd, &sshd.other_key, known_hosts.clone()),
                Arc::new(TestSink(tx)),
            )
            .unwrap();
        let err = loop {
            if let Ev::Status(SessionStatus::Error { message }) =
                rx.recv_timeout(Duration::from_secs(15)).unwrap()
            {
                break message;
            }
        };
        assert!(err.contains("authentication failed"), "{err}");

        // 4. Host key changed: swap the recorded key for a different one.
        let other_pub = std::fs::read_to_string(dir.path().join("other_key.pub")).unwrap();
        std::fs::write(
            &known_hosts,
            format!("[127.0.0.1]:{} {}", sshd.port, other_pub),
        )
        .unwrap();
        let (tx, rx) = std_mpsc::channel();
        manager
            .connect(
                "p4".into(),
                params(&sshd, &sshd.client_key, known_hosts.clone()),
                Arc::new(TestSink(tx)),
            )
            .unwrap();
        let changed = loop {
            if let Ev::Status(s @ SessionStatus::HostKeyChanged { .. }) =
                rx.recv_timeout(Duration::from_secs(15)).unwrap()
            {
                break s;
            }
        };
        assert!(
            matches!(&changed, SessionStatus::HostKeyChanged { host, port, fingerprint }
                if host == "127.0.0.1" && *port == sshd.port && fingerprint.starts_with("SHA256:")),
            "{changed:?}"
        );

        // Forgetting the pin lets the next connection learn the real key.
        assert_eq!(
            crate::knownhosts::forget(&known_hosts, "127.0.0.1", sshd.port).unwrap(),
            1
        );
        let (tx, rx) = std_mpsc::channel();
        manager
            .connect(
                "p5".into(),
                params(&sshd, &sshd.client_key, known_hosts.clone()),
                Arc::new(TestSink(tx)),
            )
            .unwrap();
        wait_status(&rx, |s| matches!(s, SessionStatus::NewHostKey { .. }));
        wait_status(&rx, |s| matches!(s, SessionStatus::Connected));
        manager.disconnect("p5").await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn certificates_authenticate_keys_the_server_does_not_list() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let Some(cert) = sshd.other_cert.clone() else {
            eprintln!("skipping: ssh-keygen could not sign a certificate");
            return;
        };
        let known_hosts = dir.path().join("kh");
        // Without its certificate the key is refused...
        let plain = target(&sshd, &sshd.other_key, known_hosts.clone());
        assert!(matches!(open_client(&plain, None).await, Err(SshError::AuthFailed { .. })));
        // ...with it, the server's CA vouches for the key.
        let mut with_cert = plain.clone();
        with_cert.auth = AuthMethod::PrivateKey {
            private_key: sshd.other_key.clone(),
            passphrase: None,
            certificate: Some(cert.clone()),
        };
        open_client(&with_cert, None).await.expect("certificate login");

        // The Key Manager accepts it for the matching key only.
        let other_pub = std::fs::read_to_string(dir.path().join("other_key.pub")).unwrap();
        let client_pub = std::fs::read_to_string(dir.path().join("client_key.pub")).unwrap();
        assert!(crate::keys::check_certificate(&cert, &other_pub).unwrap().contains(&sshd.user));
        assert!(crate::keys::check_certificate(&cert, &client_pub).is_err());
    }

    /// Records every question and answers from a script.
    struct Scripted {
        asked: Mutex<Vec<HostKeyQuestion>>,
        trust_unknown: bool,
        replace_changed: bool,
    }
    impl HostKeyPrompter for Scripted {
        fn ask(&self, q: HostKeyQuestion) -> BoxFuture<bool> {
            let yes = if q.previous.is_some() { self.replace_changed } else { self.trust_unknown };
            self.asked.lock().unwrap().push(q);
            Box::pin(async move { yes })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn unknown_and_changed_keys_need_explicit_trust() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let known_hosts = dir.path().join("kh");
        let with = |p: Arc<Scripted>| {
            let mut t = target(&sshd, &sshd.client_key, known_hosts.clone());
            t.host_keys = HostKeyPolicy {
                store: Arc::new(FileHostKeys(known_hosts.clone())),
                prompter: Some(p),
            };
            t
        };
        let script = |trust_unknown, replace_changed| {
            Arc::new(Scripted { asked: Mutex::new(vec![]), trust_unknown, replace_changed })
        };

        // Declined: nothing stored, connection refused.
        let p = script(false, false);
        let err = open_client(&with(p.clone()), None).await.err().expect("refused");
        assert!(matches!(err, SshError::HostKeyUnknown { .. }), "{err}");
        assert!(p.asked.lock().unwrap()[0].fingerprint.starts_with("SHA256:"));
        assert!(!known_hosts.exists());

        // No prompter at all (background job): refused without guessing.
        let mut t = with(p.clone());
        t.host_keys.prompter = None;
        assert!(matches!(open_client(&t, None).await, Err(SshError::HostKeyUnknown { .. })));

        // Accepted: stored and connected; next time nobody is asked.
        let p = script(true, false);
        open_client(&with(p.clone()), None).await.unwrap();
        let p2 = script(false, false);
        open_client(&with(p2.clone()), None).await.unwrap();
        assert!(p2.asked.lock().unwrap().is_empty());

        // Changed: the question carries the old fingerprint; cancel refuses,
        // replace connects and re-pins.
        let other_pub = std::fs::read_to_string(dir.path().join("other_key.pub")).unwrap();
        std::fs::write(&known_hosts, format!("[127.0.0.1]:{} {}", sshd.port, other_pub)).unwrap();
        let old_fp = PublicKey::from_openssh(other_pub.trim()).unwrap().fingerprint(HashAlg::Sha256).to_string();
        let p = script(true, false);
        assert!(matches!(open_client(&with(p.clone()), None).await, Err(SshError::HostKeyChanged { .. })));
        let q = p.asked.lock().unwrap()[0].clone();
        assert_eq!(q.previous.unwrap().fingerprint, old_fp);
        let p = script(false, true);
        open_client(&with(p), None).await.unwrap();
        let p = script(false, false);
        open_client(&with(p.clone()), None).await.unwrap();
        assert!(p.asked.lock().unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn x11_reaches_the_local_display_only_when_enabled() {
        use std::process::{Command, Stdio};
        let dir = tempfile::TempDir::new().unwrap();
        let tools = ["Xvfb", "xauth", "xkbcomp"].iter().all(|t| {
            Command::new("sh")
                .args(["-c", &format!("command -v {t}")])
                .stdout(Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        });
        if !tools {
            eprintln!("skipping: needs Xvfb, xauth and xkbcomp");
            return;
        }
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };

        // A private X server that only accepts our real cookie.
        let n = (90..200)
            .find(|n| {
                !std::path::Path::new(&format!("/tmp/.X11-unix/X{n}")).exists()
                    && !std::path::Path::new(&format!("/tmp/.X{n}-lock")).exists()
            })
            .unwrap();
        let display = format!(":{n}");
        let xauthority = dir.path().join("Xauthority");
        let cookie = crate::x11::hex_encode(&crate::x11::fake_cookie());
        let added = Command::new("xauth")
            .arg("-f")
            .arg(&xauthority)
            .args(["add", &display, ".", &cookie])
            .status()
            .unwrap()
            .success();
        assert!(added);
        let mut xvfb = Command::new("Xvfb")
            .arg(&display)
            .arg("-auth")
            .arg(&xauthority)
            .args(["-nolisten", "tcp"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let sock = format!("/tmp/.X11-unix/X{n}");
        for _ in 0..100 {
            if std::path::Path::new(&sock).exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        std::env::set_var("DISPLAY", &display);
        std::env::set_var("XAUTHORITY", &xauthority);

        // The real cookie was read and differs from what the server gets.
        let setup = crate::x11::X11Setup::from_env().unwrap();
        assert_eq!(
            setup.real.as_deref().map(crate::x11::hex_encode),
            Some(cookie.clone())
        );
        assert_ne!(setup.real.as_deref(), Some(setup.fake.as_slice()));

        let manager = Arc::new(SshManager::new());
        let kh = dir.path().join("kh");
        let run = |pane: &'static str, x11: bool| {
            let mut t = target(&sshd, &sshd.client_key, kh.clone());
            t.forward_x11 = x11;
            let (tx, rx) = std_mpsc::channel();
            manager
                .connect(
                    pane.into(),
                    ConnectParams {
                        target: t,
                        cols: 120,
                        rows: 24,
                    },
                    Arc::new(TestSink(tx)),
                )
                .unwrap();
            rx
        };

        // Remote xkbcomp reads the keymap from $DISPLAY: through the tunnel,
        // with the fake cookie swapped for the real one on the way.
        let rx = run("x1", true);
        wait_status(&rx, |s| matches!(s, SessionStatus::Connected));
        manager
            .write(
                "x1",
                format!(
                    "xkbcomp -w0 \"$DISPLAY\" {}/km.xkb >/dev/null 2>&1; echo XRC=$?\n",
                    dir.path().display()
                )
                .into_bytes(),
            )
            .await
            .unwrap();
        wait_output(&rx, "XRC=0");
        assert!(dir.path().join("km.xkb").exists());
        manager.disconnect("x1").await;

        // Without X11, the remote shell has no DISPLAY at all.
        let rx = run("x2", false);
        wait_status(&rx, |s| matches!(s, SessionStatus::Connected));
        manager
            .write("x2", b"echo D=[${DISPLAY:-none}]\n".to_vec())
            .await
            .unwrap();
        wait_output(&rx, "D=[none]");
        manager.disconnect("x2").await;

        let _ = xvfb.kill();
        let _ = xvfb.wait();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn agent_is_forwarded_only_when_enabled() {
        use std::process::{Command, Stdio};
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        // A private agent holding the test key, so the result doesn't depend
        // on whatever agent the developer has running.
        let sock = dir.path().join("agent.sock");
        let Ok(mut agent) = Command::new("ssh-agent")
            .args(["-D", "-a"])
            .arg(&sock)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            eprintln!("skipping: no ssh-agent");
            return;
        };
        for _ in 0..50 {
            if sock.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let added = Command::new("ssh-add")
            .env("SSH_AUTH_SOCK", &sock)
            .arg(dir.path().join("client_key"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !added {
            let _ = agent.kill();
            eprintln!("skipping: ssh-add failed");
            return;
        }
        std::env::set_var("SSH_AUTH_SOCK", &sock);

        let manager = Arc::new(SshManager::new());
        let kh = dir.path().join("kh");

        let mut forwarded = target(&sshd, &sshd.client_key, kh.clone());
        forwarded.forward_agent = true;
        let (tx, rx) = std_mpsc::channel();
        manager
            .connect(
                "fa".into(),
                ConnectParams {
                    target: forwarded,
                    cols: 120,
                    rows: 24,
                },
                Arc::new(TestSink(tx)),
            )
            .unwrap();
        wait_status(&rx, |s| matches!(s, SessionStatus::Connected));
        manager
            .write("fa", b"ssh-add -l; echo DONE$((1+1))\n".to_vec())
            .await
            .unwrap();
        let out = wait_output(&rx, "DONE2");
        assert!(out.contains("SHA256:"), "remote should see the key: {out}");
        manager.disconnect("fa").await;

        let (tx, rx) = std_mpsc::channel();
        manager
            .connect(
                "na".into(),
                ConnectParams {
                    target: target(&sshd, &sshd.client_key, kh),
                    cols: 120,
                    rows: 24,
                },
                Arc::new(TestSink(tx)),
            )
            .unwrap();
        wait_status(&rx, |s| matches!(s, SessionStatus::Connected));
        // Exit code 2 means ssh-add could not reach an agent at all.
        manager
            .write("na", b"ssh-add -l >/dev/null 2>&1; echo RC=$?\n".to_vec())
            .await
            .unwrap();
        wait_output(&rx, "RC=2");
        manager.disconnect("na").await;
        let _ = agent.kill();
    }

    /// Offers one key and records where each approval request came from.
    struct VaultStub {
        key: russh::keys::PrivateKey,
        origins: Mutex<Vec<Option<String>>>,
    }
    impl crate::agent::Backend for VaultStub {
        fn identities(&self) -> Vec<crate::agent::Offered> {
            vec![crate::agent::Offered { public: self.key.public_key().clone(), comment: "sshvault:test".into() }]
        }
        fn private(&self, _: &PublicKey) -> Option<russh::keys::PrivateKey> {
            Some(self.key.clone())
        }
        fn approve(&self, _: &PublicKey, _: &str, origin: Option<&str>) -> crate::agent::BoxFuture<bool> {
            self.origins.lock().unwrap().push(origin.map(str::to_string));
            Box::pin(async { true })
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn forwarded_agent_can_be_the_vault_agent() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let key = russh::keys::PrivateKey::from_openssh(&sshd.client_key).unwrap();
        let stub = Arc::new(VaultStub { key: key.clone(), origins: Mutex::new(vec![]) });
        let mut t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        t.forward_agent = true;
        t.agent_backend = Some(stub.clone());

        let manager = Arc::new(SshManager::new());
        let (tx, rx) = std_mpsc::channel();
        manager
            .connect("va".into(), ConnectParams { target: t, cols: 160, rows: 24 }, Arc::new(TestSink(tx)))
            .unwrap();
        wait_status(&rx, |s| matches!(s, SessionStatus::Connected));
        // The remote sees the vault key, and can log back in with it
        // (a hop from the server to itself), which needs a signature.
        let cmd = format!(
            "ssh-add -L | grep -c sshvault:test; ssh -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -p {} 127.0.0.1 echo HOP$((2+2)) 2>/dev/null; echo DONE$((1+1))\n",
            sshd.port
        );
        manager.write("va", cmd.into_bytes()).await.unwrap();
        let out = wait_output(&rx, "DONE2");
        assert!(out.contains("HOP4"), "second hop signed through the vault agent: {out}");
        let origins = stub.origins.lock().unwrap().clone();
        assert!(!origins.is_empty());
        assert!(origins.iter().all(|o| o.as_deref() == Some(&*format!("127.0.0.1:{}", sshd.port))), "{origins:?}");
        manager.disconnect("va").await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shell_through_one_and_two_jump_hosts() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let kh = dir.path().join("kh");
        let direct = target(&sshd, &sshd.client_key, kh.clone());
        // The test server doubles as its own bastion: the final hop is dialled
        // from inside the jump, so it must use an address the jump can reach.
        let mut via_one = direct.clone();
        via_one.jump = Some(Box::new(direct.clone()));
        let mut via_two = direct.clone();
        via_two.jump = Some(Box::new(via_one.clone()));

        let (client, learned) = open_client(&via_one, None).await.unwrap();
        // Both hops share host:port, so the key is learned once, at the jump.
        assert_eq!(learned.len(), 1);
        assert_eq!(
            client.jump_hosts(),
            vec![format!("127.0.0.1:{}", sshd.port)]
        );
        client.close().await;

        let manager = Arc::new(SshManager::new());
        let (tx, rx) = std_mpsc::channel();
        manager
            .connect(
                "j2".into(),
                ConnectParams {
                    target: via_two,
                    cols: 80,
                    rows: 24,
                },
                Arc::new(TestSink(tx)),
            )
            .unwrap();
        wait_status(&rx, |s| matches!(s, SessionStatus::Connected));
        manager
            .write("j2", b"echo J$((1+1))MP\n".to_vec())
            .await
            .unwrap();
        wait_output(&rx, "J2MP");
        manager.disconnect("j2").await;
        wait_status(&rx, |s| matches!(s, SessionStatus::Disconnected { .. }));

        // Destination unreachable from the jump names the jump in the error.
        let dead_port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut unreachable = direct.clone();
        unreachable.port = dead_port;
        unreachable.jump = Some(Box::new(direct.clone()));
        let err = open_client(&unreachable, None)
            .await
            .err()
            .expect("should fail");
        assert!(matches!(err, SshError::JumpUnreachable { .. }), "{err}");
        assert!(err.to_string().contains("could not reach"), "{err}");

        // Bad credentials on the jump are attributed to the jump.
        let mut bad_jump = direct.clone();
        bad_jump.jump = Some(Box::new(target(&sshd, &sshd.other_key, kh.clone())));
        let err = open_client(&bad_jump, None)
            .await
            .err()
            .expect("should fail");
        assert!(matches!(err, SshError::Jump { .. }), "{err}");
        assert!(err.to_string().contains("authentication failed"), "{err}");
    }
}
