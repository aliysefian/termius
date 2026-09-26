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
use std::path::PathBuf;
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

    #[error("host key for {host} has CHANGED (fingerprint {fingerprint}). Possible man-in-the-middle; remove the old entry from {known_hosts} if this is expected")]
    HostKeyChanged {
        host: String,
        fingerprint: String,
        known_hosts: PathBuf,
    },

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
    /// Emitted once when a host key is seen for the first time and recorded.
    NewHostKey {
        fingerprint: String,
    },
    Connected,
    /// The remote shell exited or the connection dropped. `code` is the exit
    /// status when the server reported one.
    Disconnected {
        code: Option<u32>,
    },
    Error {
        message: String,
    },
}

/// Where to connect and how to log in. Shared by terminals, SFTP and
/// port forwarding.
#[derive(Clone)]
pub struct Target {
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub auth: AuthMethod,
    /// App-private known_hosts file used for trust-on-first-use.
    pub known_hosts: PathBuf,
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
        {
            let mut sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
            if sessions.contains_key(&pane_id) {
                return Err(SshError::AlreadyConnected);
            }
            sessions.insert(pane_id.clone(), SessionHandle { tx });
        }

        let manager = Arc::clone(self);
        tokio::spawn(async move {
            sink.status(SessionStatus::Connecting);
            let outcome = run_session(params, rx, sink.as_ref()).await;
            match outcome {
                Ok(code) => sink.status(SessionStatus::Disconnected { code }),
                Err(e) => sink.status(SessionStatus::Error {
                    message: e.to_string(),
                }),
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
// Host key verification (trust on first use)
// ---------------------------------------------------------------------------

pub struct ClientHandler {
    host: String,
    port: u16,
    known_hosts: PathBuf,
    /// Set when a key was learned during this connection. Shared with the
    /// session task, which reports it after authentication succeeds.
    learned_fingerprint: Arc<Mutex<Option<String>>>,
    remote_forwards: Option<RemoteForwards>,
}

impl ClientHandler {
    fn learn(&mut self, key: &PublicKey, fingerprint: String) -> Result<bool, SshError> {
        if let Some(dir) = self.known_hosts.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        learn_known_hosts_path(&self.host, self.port, key, &self.known_hosts)?;
        *self
            .learned_fingerprint
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = Some(fingerprint);
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
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();

        match check_known_hosts_path(&self.host, self.port, &key, &self.known_hosts) {
            Ok(true) => Ok(true),
            Ok(false) => self.learn(&key, fingerprint),
            Err(keys::Error::KeyChanged { .. }) => Err(SshError::HostKeyChanged {
                host: self.host.clone(),
                fingerprint,
                known_hosts: self.known_hosts.clone(),
            }),
            // A missing file is the first-ever connection from this machine.
            Err(keys::Error::IO(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                self.learn(&key, fingerprint)
            }
            Err(e) => Err(e.into()),
        }
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

/// Connect, verify the host key, and authenticate. Returns the live handle
/// and, if the host key was seen for the first time, its SHA256 fingerprint.
pub async fn open_client(
    target: &Target,
    remote_forwards: Option<RemoteForwards>,
) -> Result<(Handle<ClientHandler>, Option<String>), SshError> {
    let addr = format!("{}:{}", target.hostname, target.port);
    let learned = Arc::new(Mutex::new(None));
    let handler = ClientHandler {
        host: target.hostname.clone(),
        port: target.port,
        known_hosts: target.known_hosts.clone(),
        learned_fingerprint: Arc::clone(&learned),
        remote_forwards,
    };

    let stream = tokio::time::timeout(
        CONNECT_TIMEOUT,
        tokio::net::TcpStream::connect((target.hostname.as_str(), target.port)),
    )
    .await
    .map_err(|_| SshError::Timeout(addr.clone()))?
    .map_err(|source| SshError::Connect {
        addr: addr.clone(),
        source,
    })?;
    let _ = stream.set_nodelay(true);

    let mut handle = tokio::time::timeout(
        CONNECT_TIMEOUT,
        client::connect_stream(client_config(), stream, handler),
    )
    .await
    .map_err(|_| SshError::Timeout(addr.clone()))??;

    authenticate(&mut handle, target).await?;
    let fp = learned.lock().unwrap_or_else(|p| p.into_inner()).take();
    Ok((handle, fp))
}

// ---------------------------------------------------------------------------
// Session task
// ---------------------------------------------------------------------------

fn client_config() -> Arc<client::Config> {
    Arc::new(client::Config {
        keepalive_interval: Some(KEEPALIVE),
        keepalive_max: 3,
        nodelay: true,
        ..Default::default()
    })
}

async fn run_session(
    params: ConnectParams,
    mut rx: mpsc::Receiver<Cmd>,
    sink: &dyn TermSink,
) -> Result<Option<u32>, SshError> {
    let (handle, learned) = open_client(&params.target, None).await?;

    // Host key notice is only worth showing once we know the login worked.
    if let Some(fp) = learned {
        sink.status(SessionStatus::NewHostKey { fingerprint: fp });
    }

    let channel = handle.channel_open_session().await?;
    channel
        .request_pty(true, TERM, params.cols, params.rows, 0, 0, &[])
        .await?;
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

    let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
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
        } => {
            let pem = Zeroizing::new(private_key.clone());
            let key = keys::decode_secret_key(&pem, passphrase.as_deref())?;
            let hash = handle.best_supported_rsa_hash().await?.flatten();
            let result = handle
                .authenticate_publickey(user, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
                .await?;
            if result.success() {
                Ok(())
            } else {
                Err(fail("private key"))
            }
        }

        AuthMethod::Agent => authenticate_with_agent(handle, params).await,
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
    }
    impl Drop for Sshd {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// Returns `None` (and the caller should skip) when sshd is unavailable.
    pub fn spawn_sshd(dir: &std::path::Path) -> Option<Sshd> {
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
        std::fs::copy(dir.join("client_key.pub"), dir.join("authorized_keys")).ok()?;

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
        let config = format!(
            "Port {port}\nListenAddress 127.0.0.1\nHostKey {d}/host_key\nAuthorizedKeysFile {d}/authorized_keys\n\
             PasswordAuthentication no\nKbdInteractiveAuthentication no\nUsePAM no\nStrictModes no\nPidFile none\n\
             AllowTcpForwarding yes\n{sftp_server}",
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
        let user = std::env::var("USER").ok().or_else(|| {
            Command::new("id")
                .arg("-un")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        })?;
        Some(Sshd {
            child,
            port,
            user,
            client_key: std::fs::read_to_string(dir.join("client_key")).ok()?,
            other_key: std::fs::read_to_string(dir.join("other_key")).ok()?,
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
            },
            known_hosts,
        }
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
            matches!(new_key, SessionStatus::NewHostKey { ref fingerprint } if fingerprint.starts_with("SHA256:"))
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
        let err = loop {
            if let Ev::Status(SessionStatus::Error { message }) =
                rx.recv_timeout(Duration::from_secs(15)).unwrap()
            {
                break message;
            }
        };
        assert!(err.contains("CHANGED"), "{err}");
    }
}
