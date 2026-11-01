//! Port forwarding: local (-L), remote (-R) and dynamic SOCKS5 (-D).
//!
//! Each running rule owns its own SSH connection and a supervising task.
//! Stopping a rule (or losing the connection) tears down its listener and
//! every tunnelled socket. Tauri-agnostic, like the other engine modules.

use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::models::ForwardKind;
use crate::ssh::{open_client, Client, RemoteForwards, SshError, Target};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ForwardStatus {
    Starting,
    /// Listening. `port` is the actually bound port (useful when 0 was asked).
    Active {
        port: u16,
    },
    Stopped,
    Error {
        message: String,
    },
}

pub trait StatusSink: Send + Sync + 'static {
    fn status(&self, rule_id: Uuid, status: ForwardStatus);
    /// A line of the tunnel's diagnostic log, as it is written. Not every sink wants them.
    fn log(&self, _rule_id: Uuid, _entry: &ForwardLog) {}
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

/// One line of a tunnel's diagnostic log. Messages are written by this module (never raw protocol data) and are
/// scrubbed of anything that looks like a secret before they are kept.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForwardLog {
    /// Milliseconds since the Unix epoch.
    pub at_ms: u64,
    pub level: LogLevel,
    pub message: String,
}

/// How many lines one tunnel keeps; older ones are dropped.
pub const MAX_LOG_ENTRIES: usize = 200;
/// The longest line kept.
pub const MAX_LOG_LEN: usize = 500;

/// What a running tunnel knows about itself beyond its state.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ForwardRuntime {
    /// When it became active (milliseconds since the Unix epoch); none unless it is active now.
    pub active_since_ms: Option<u64>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// A log line with anything secret-looking removed: private key blocks, and the value after `password`,
/// `passphrase`, `token`, `secret` or `authorization` (as `key=value`, `key: value` or `key value`). Cut to
/// [`MAX_LOG_LEN`] characters.
pub fn sanitize(message: &str) -> String {
    let mut text = message.to_string();
    // A private key block, whole or cut off.
    while let Some(start) = text.find("-----BEGIN") {
        let end = text[start..].find("-----END").and_then(|e| text[start + e + 8..].find("-----").map(|t| start + e + 8 + t + 5));
        match end {
            Some(end) => text.replace_range(start..end, "[private key removed]"),
            None => {
                text.truncate(start);
                text.push_str("[private key removed]");
            }
        }
    }
    for word in ["password", "passphrase", "token", "secret", "authorization"] {
        let mut from = 0;
        loop {
            let lower = text.to_ascii_lowercase();
            let Some(found) = lower[from..].find(word) else { break };
            let after = from + found + word.len();
            let rest = &text[after..];
            let skip = rest.len() - rest.trim_start_matches([' ', ':', '=', '"', '\'']).len();
            let value_start = after + skip;
            let value_len = text[value_start..].find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ',' | ';' | ')')).unwrap_or(text.len() - value_start);
            // Only a word followed by a separator and a value is a secret being shown; "password" alone is prose.
            if skip > 0 && value_len > 0 && !text[value_start..value_start + value_len].eq_ignore_ascii_case("[redacted]") && !matches!(&lower[value_start..value_start + value_len], "for" | "is" | "was" | "required" | "rejected" | "failed" | "authentication") {
                // "Authorization: Bearer abc": the scheme is not the secret, the word after it is.
                let scheme = matches!(lower[value_start..value_start + value_len].as_ref(), "bearer" | "basic");
                text.replace_range(value_start..value_start + value_len, "[redacted]");
                from = value_start + "[redacted]".len();
                if scheme {
                    let rest = &text[from..];
                    let gap = rest.len() - rest.trim_start().len();
                    let len = rest[gap..].find(|c: char| c.is_whitespace()).unwrap_or(rest.len() - gap);
                    if len > 0 {
                        text.replace_range(from + gap..from + gap + len, "[redacted]");
                        from += gap + "[redacted]".len();
                    }
                }
            } else {
                from = after;
            }
            if from >= text.len() {
                break;
            }
        }
    }
    if text.chars().count() > MAX_LOG_LEN {
        text = text.chars().take(MAX_LOG_LEN).collect::<String>() + "…";
    }
    text
}

#[derive(Debug, thiserror::Error)]
pub enum ForwardError {
    #[error(transparent)]
    Ssh(#[from] SshError),
    #[error("SSH: {0}")]
    Russh(#[from] russh::Error),
    #[error("cannot listen on {addr}: {source}")]
    Bind {
        addr: String,
        #[source]
        source: std::io::Error,
    },
    #[error("server refused to listen on {0}")]
    RemoteRefused(String),
    #[error("rule is already running")]
    AlreadyRunning,
    #[error("the SSH connection to the host was lost")]
    ConnectionLost,
}

struct Running {
    stop: oneshot::Sender<()>,
    /// Which start this is, so a task that outlives its stop never removes (or reports over) a newer start.
    generation: u64,
    task: Option<tokio::task::JoinHandle<()>>,
}

#[derive(Default)]
pub struct ForwardManager {
    running: Mutex<HashMap<Uuid, Running>>,
    last: Mutex<HashMap<Uuid, ForwardStatus>>,
    logs: Mutex<HashMap<Uuid, VecDeque<ForwardLog>>>,
    active_since: Mutex<HashMap<Uuid, u64>>,
    next_generation: AtomicU64,
}

impl ForwardManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current status of every rule that has run in this app session.
    pub fn statuses(&self) -> HashMap<Uuid, ForwardStatus> {
        self.last.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// What each tunnel that is active now knows about itself.
    pub fn runtime(&self) -> HashMap<Uuid, ForwardRuntime> {
        self.active_since
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .map(|(id, at)| (*id, ForwardRuntime { active_since_ms: Some(*at) }))
            .collect()
    }

    /// A tunnel's diagnostic log, oldest first.
    pub fn logs(&self, rule_id: Uuid) -> Vec<ForwardLog> {
        self.logs.lock().unwrap_or_else(|p| p.into_inner()).get(&rule_id).map(|l| l.iter().cloned().collect()).unwrap_or_default()
    }

    /// Drop everything kept about a tunnel that no longer exists.
    pub fn forget(&self, rule_id: Uuid) {
        self.logs.lock().unwrap_or_else(|p| p.into_inner()).remove(&rule_id);
        self.last.lock().unwrap_or_else(|p| p.into_inner()).remove(&rule_id);
        self.active_since.lock().unwrap_or_else(|p| p.into_inner()).remove(&rule_id);
    }

    /// Add a line to a tunnel's log (scrubbed, bounded, and a line that repeats the last one is not added twice).
    fn log(&self, sink: &dyn StatusSink, id: Uuid, level: LogLevel, message: &str) {
        let entry = ForwardLog { at_ms: now_ms(), level, message: sanitize(message) };
        {
            let mut logs = self.logs.lock().unwrap_or_else(|p| p.into_inner());
            let list = logs.entry(id).or_default();
            if list.back().is_some_and(|last| last.message == entry.message && last.level == entry.level) {
                return;
            }
            if list.len() >= MAX_LOG_ENTRIES {
                list.pop_front();
            }
            list.push_back(entry.clone());
        }
        sink.log(id, &entry);
    }

    fn set(&self, sink: &dyn StatusSink, id: Uuid, st: ForwardStatus) {
        {
            let mut since = self.active_since.lock().unwrap_or_else(|p| p.into_inner());
            match &st {
                ForwardStatus::Active { .. } => {
                    since.entry(id).or_insert_with(now_ms);
                }
                _ => {
                    since.remove(&id);
                }
            }
        }
        match &st {
            ForwardStatus::Starting => self.log(sink, id, LogLevel::Info, "Starting tunnel"),
            ForwardStatus::Stopped => self.log(sink, id, LogLevel::Info, "Tunnel stopped"),
            ForwardStatus::Error { message } => self.log(sink, id, LogLevel::Error, message),
            ForwardStatus::Active { .. } => {}
        }
        self.last
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id, st.clone());
        sink.status(id, st);
    }

    /// Start a rule in the background. Returns immediately; progress is
    /// reported through `sink`. Must be called inside a tokio runtime.
    pub fn start(
        self: &Arc<Self>,
        rule_id: Uuid,
        target: Target,
        kind: ForwardKind,
        sink: Arc<dyn StatusSink>,
    ) -> Result<(), ForwardError> {
        let (stop_tx, stop_rx) = oneshot::channel();
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        {
            let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
            if running.contains_key(&rule_id) {
                return Err(ForwardError::AlreadyRunning);
            }
            running.insert(rule_id, Running { stop: stop_tx, generation, task: None });
        }
        self.set(sink.as_ref(), rule_id, ForwardStatus::Starting);

        let me = Arc::clone(self);
        // spawn-ok: only called from the async forward_start command
        let task = tokio::spawn(async move {
            let end = match run_rule(&me, rule_id, &target, &kind, &sink, stop_rx).await {
                Ok(()) => ForwardStatus::Stopped,
                Err(e) => ForwardStatus::Error {
                    message: e.to_string(),
                },
            };
            // A newer start of the same rule owns the entry and the status now: this one leaves them alone.
            let mut running = me.running.lock().unwrap_or_else(|p| p.into_inner());
            match running.get(&rule_id).map(|r| r.generation) {
                Some(g) if g != generation => return,
                Some(_) => {
                    running.remove(&rule_id);
                }
                None => {}
            }
            drop(running);
            me.set(sink.as_ref(), rule_id, end);
        });
        if let Some(r) = self.running.lock().unwrap_or_else(|p| p.into_inner()).get_mut(&rule_id) {
            if r.generation == generation {
                r.task = Some(task);
            }
        }
        Ok(())
    }

    /// Stop a rule and wait (up to `wait`) until its listener and connection are really gone, so a start that
    /// follows can bind the same port.
    pub async fn stop_and_wait(&self, rule_id: Uuid, wait: std::time::Duration) {
        let entry = self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&rule_id);
        if let Some(mut r) = entry {
            let _ = r.stop.send(());
            if let Some(task) = r.task.take() {
                let _ = tokio::time::timeout(wait, task).await;
            }
        }
    }

    pub fn stop(&self, rule_id: Uuid) {
        if let Some(r) = self
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(&rule_id)
        {
            let _ = r.stop.send(());
        }
    }

    pub fn stop_all(&self) {
        let all: Vec<Running> = self
            .running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .drain()
            .map(|(_, r)| r)
            .collect();
        for r in all {
            let _ = r.stop.send(());
        }
    }

    pub fn is_running(&self, rule_id: Uuid) -> bool {
        self.running
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .contains_key(&rule_id)
    }
}

/// Whether `addr` can be listened on: an IP address, `localhost`, a plain host name, or (for a remote forward only)
/// `*` or empty, which the server reads as "every interface".
fn bind_addr_ok(addr: &str, remote: bool) -> bool {
    if remote && (addr.is_empty() || addr == "*") {
        return true;
    }
    host_ok(addr)
}

fn host_ok(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 || host.chars().any(char::is_whitespace) {
        return false;
    }
    if host.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    host.split('.').all(|label| !label.is_empty() && label.len() <= 63 && !label.starts_with('-') && !label.ends_with('-') && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
}

/// Do two bind addresses listen on the same interface? A wildcard overlaps everything.
fn overlaps(a: &str, b: &str) -> bool {
    let wild = |x: &str| matches!(x, "0.0.0.0" | "::" | "[::]" | "*" | "");
    let local = |x: &str| matches!(x, "127.0.0.1" | "::1" | "localhost");
    a == b || wild(a) || wild(b) || (local(a) && local(b) && (a == "localhost" || b == "localhost"))
}

/// Why a rule can't be saved, in words for the person who wrote it, or `Ok`. `others` are the other saved rules,
/// for ports that two of them would both want. Port 0 means "pick a free port" and is allowed.
pub fn validate_rule(rule: &crate::models::ForwardRule, id: Option<Uuid>, others: &[(Uuid, crate::models::ForwardRule)]) -> Result<(), String> {
    use crate::models::ForwardKind::{Dynamic, Local, Remote};
    if rule.label.trim().is_empty() {
        return Err("a name is required".into());
    }
    let (bind_addr, bind_port, dest) = match &rule.kind {
        Local { bind_addr, bind_port, dest_host, dest_port } | Remote { bind_addr, bind_port, dest_host, dest_port } => (bind_addr, *bind_port, Some((dest_host, *dest_port))),
        Dynamic { bind_addr, bind_port } => (bind_addr, *bind_port, None),
    };
    let remote = matches!(rule.kind, Remote { .. });
    if !bind_addr_ok(bind_addr.trim(), remote) {
        return Err(format!("\"{bind_addr}\" isn't an address that can be listened on: use an IP address such as 127.0.0.1, or a host name"));
    }
    if let Some((host, port)) = dest {
        if !host_ok(host.trim()) {
            return Err(format!("\"{host}\" isn't a valid destination host"));
        }
        if port == 0 {
            return Err("the destination port must be 1 to 65535".into());
        }
    }
    if bind_port != 0 {
        for (other_id, other) in others {
            if Some(*other_id) == id {
                continue;
            }
            let clash = match (&rule.kind, &other.kind) {
                // Two rules listening on this computer want the same port on the same interface.
                (Local { .. } | Dynamic { .. }, Local { bind_addr: oa, bind_port: op, .. } | Dynamic { bind_addr: oa, bind_port: op }) => *op == bind_port && overlaps(bind_addr.trim(), oa.trim()),
                // Two remote rules on the same server want the same port there.
                (Remote { .. }, Remote { bind_addr: oa, bind_port: op, .. }) => *op == bind_port && other.host_id == rule.host_id && overlaps(bind_addr.trim(), oa.trim()),
                _ => false,
            };
            if clash {
                return Err(format!("port {bind_port} is already used by the tunnel \"{}\"", other.label));
            }
        }
    }
    Ok(())
}

async fn run_rule(
    mgr: &Arc<ForwardManager>,
    rule_id: Uuid,
    target: &Target,
    kind: &ForwardKind,
    sink: &Arc<dyn StatusSink>,
    mut stop: oneshot::Receiver<()>,
) -> Result<(), ForwardError> {
    let say = |level: LogLevel, text: &str| mgr.log(sink.as_ref(), rule_id, level, text);
    // What went wrong with one carried connection, for the log. Ordinary hang-ups are not news.
    let report: ConnReport = {
        let (mgr, sink) = (Arc::clone(mgr), Arc::clone(sink));
        Arc::new(move |text: &str| mgr.log(sink.as_ref(), rule_id, LogLevel::Warn, text))
    };
    let remote_table: RemoteForwards = Arc::new(Mutex::new(HashMap::new()));
    say(LogLevel::Info, &format!("Connecting to {}:{} as {}", target.hostname, target.port, target.username));
    let (handle, _) = open_client(target, Some(Arc::clone(&remote_table))).await?;
    let handle = Arc::new(handle);
    say(LogLevel::Info, "SSH connection established");

    let result = match kind {
        ForwardKind::Local {
            bind_addr,
            bind_port,
            dest_host,
            dest_port,
        } => {
            let listener = bind(bind_addr, *bind_port).await?;
            let port = listener
                .local_addr()
                .map(|a| a.port())
                .unwrap_or(*bind_port);
            say(LogLevel::Info, &format!("Listening on {bind_addr}:{port}, forwarding to {dest_host}:{dest_port} through the host"));
            mgr.set(sink.as_ref(), rule_id, ForwardStatus::Active { port });
            accept_loop(listener, &handle, &mut stop, Some(report), {
                let dest_host = dest_host.clone();
                let dest_port = *dest_port;
                move |_| Some((dest_host.clone(), dest_port))
            })
            .await
        }
        ForwardKind::Dynamic {
            bind_addr,
            bind_port,
        } => {
            let listener = bind(bind_addr, *bind_port).await?;
            let port = listener
                .local_addr()
                .map(|a| a.port())
                .unwrap_or(*bind_port);
            say(LogLevel::Info, &format!("SOCKS5 proxy listening on {bind_addr}:{port}"));
            mgr.set(sink.as_ref(), rule_id, ForwardStatus::Active { port });
            accept_loop(listener, &handle, &mut stop, Some(report), |_| None).await
        }
        ForwardKind::Remote {
            bind_addr,
            bind_port,
            dest_host,
            dest_port,
        } => {
            let bound = handle
                .tcpip_forward(bind_addr.clone(), u32::from(*bind_port))
                .await
                .map_err(|_| ForwardError::RemoteRefused(format!("{bind_addr}:{bind_port}")))?;
            // Servers reply with the port only when 0 was requested.
            let port = if *bind_port == 0 {
                bound
            } else {
                u32::from(*bind_port)
            };
            remote_table
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(port, (dest_host.clone(), *dest_port));
            say(LogLevel::Info, &format!("The server accepted the remote forward: it listens on {bind_addr}:{port}, connections go to {dest_host}:{dest_port} from this computer"));
            mgr.set(sink.as_ref(), rule_id, ForwardStatus::Active { port: port as u16 });
            let stopped = wait_until_stopped_or_closed(&handle, &mut stop).await;
            let _ = handle.cancel_tcpip_forward(bind_addr.clone(), port).await;
            if stopped {
                Ok(())
            } else {
                // The connection ended without anyone stopping the tunnel: that is a failure, not a stop.
                Err(ForwardError::ConnectionLost)
            }
        }
    };

    handle.close().await;
    result
}

/// A loopback listener that carries each connection over SSH to one fixed
/// destination, for as long as this value lives. The database client uses
/// it so a database port never has to be reachable from this computer.
pub struct LocalTunnel {
    port: u16,
    stop: Option<oneshot::Sender<()>>,
}

impl LocalTunnel {
    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for LocalTunnel {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Open an SSH connection through `target` and listen on a free loopback
/// port that forwards to `dest_host:dest_port` as seen from the SSH server.
pub async fn open_local_tunnel(target: &Target, dest_host: String, dest_port: u16) -> Result<LocalTunnel, ForwardError> {
    let (handle, _) = open_client(target, None).await?;
    let handle = Arc::new(handle);
    let listener = bind("127.0.0.1", 0).await?;
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
    let (stop_tx, mut stop_rx) = oneshot::channel();
    tokio::spawn(async move {
        let _ = accept_loop(listener, &handle, &mut stop_rx, None, move |_| Some((dest_host.clone(), dest_port))).await;
        handle.close().await;
    });
    Ok(LocalTunnel { port, stop: Some(stop_tx) })
}

async fn bind(addr: &str, port: u16) -> Result<TcpListener, ForwardError> {
    TcpListener::bind((addr, port))
        .await
        .map_err(|source| ForwardError::Bind {
            addr: format!("{addr}:{port}"),
            source,
        })
}

/// Waits until the tunnel is stopped (true) or its SSH connection closes by itself (false).
async fn wait_until_stopped_or_closed(handle: &Client, stop: &mut oneshot::Receiver<()>) -> bool {
    loop {
        tokio::select! {
            _ = &mut *stop => return true,
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                if handle.is_closed() { return false; }
            }
        }
    }
}

/// Accept TCP clients until stopped. `fixed_dest` returns the destination for
/// a plain forward, or `None` to run a SOCKS5 handshake per client.
/// Connections one forward carries at once.
const MAX_TUNNELS: usize = 256;
const SOCKS_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Where a carried connection's failure is written, for a tunnel that keeps a log.
type ConnReport = Arc<dyn Fn(&str) + Send + Sync>;

async fn accept_loop<F>(
    listener: TcpListener,
    handle: &Arc<Client>,
    stop: &mut oneshot::Receiver<()>,
    report: Option<ConnReport>,
    fixed_dest: F,
) -> Result<(), ForwardError>
where
    F: Fn(&TcpStream) -> Option<(String, u16)>,
{
    let mut tasks = tokio::task::JoinSet::new();
    let result = loop {
        tokio::select! {
            _ = &mut *stop => break Ok(()),
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                if handle.is_closed() {
                    break Err(ForwardError::ConnectionLost);
                }
            }
            accepted = listener.accept() => {
                let Ok((sock, peer)) = accepted else {
                    // A listener that keeps failing (out of file descriptors, say) must not spin the CPU.
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    continue;
                };
                if tasks.len() >= MAX_TUNNELS {
                    drop(sock); // refuse: too many connections through this one forward
                    continue;
                }
                let _ = sock.set_nodelay(true);
                let dest = fixed_dest(&sock);
                let handle = Arc::clone(handle);
                let report = report.clone();
                tasks.spawn(async move {
                    if let Err(e) = tunnel(sock, peer, dest, &handle).await {
                        use std::io::ErrorKind::{BrokenPipe, ConnectionAborted, ConnectionReset, UnexpectedEof};
                        if let (Some(report), false) = (&report, matches!(e.kind(), BrokenPipe | ConnectionReset | ConnectionAborted | UnexpectedEof)) {
                            report(&format!("Connection from {peer} failed: {e}"));
                        }
                    }
                });
            }
            // Reap finished tunnels so the set doesn't grow unbounded.
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
        }
    };
    tasks.abort_all();
    result
}

async fn tunnel(
    mut sock: TcpStream,
    peer: std::net::SocketAddr,
    dest: Option<(String, u16)>,
    handle: &Client,
) -> std::io::Result<()> {
    let (host, port) = match dest {
        Some(d) => d,
        // A local program that connects and then says nothing must not hold a task for ever.
        None => tokio::time::timeout(SOCKS_HANDSHAKE_TIMEOUT, socks5_handshake(&mut sock))
            .await
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::TimedOut, "the SOCKS handshake timed out"))??,
    };
    let channel = handle
        .channel_open_direct_tcpip(
            host,
            u32::from(port),
            peer.ip().to_string(),
            u32::from(peer.port()),
        )
        .await
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut remote = channel.into_stream();
    tokio::io::copy_bidirectional(&mut sock, &mut remote).await?;
    Ok(())
}

/// Minimal SOCKS5 server side: no-auth method, CONNECT command, IPv4 / IPv6 /
/// domain addresses (RFC 1928). Replies "succeeded" optimistically once the
/// request is parsed; a failed channel open then just closes the socket.
async fn socks5_handshake(sock: &mut TcpStream) -> std::io::Result<(String, u16)> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, m.to_string());

    let mut hdr = [0u8; 2];
    sock.read_exact(&mut hdr).await?;
    if hdr[0] != 5 {
        return Err(bad("not SOCKS5"));
    }
    let mut methods = vec![0u8; hdr[1] as usize];
    sock.read_exact(&mut methods).await?;
    if !methods.contains(&0) {
        sock.write_all(&[5, 0xff]).await?;
        return Err(bad("client requires authentication"));
    }
    sock.write_all(&[5, 0]).await?;

    let mut req = [0u8; 4];
    sock.read_exact(&mut req).await?;
    if req[0] != 5 {
        return Err(bad("bad request version"));
    }
    if req[1] != 1 {
        // Only CONNECT is supported.
        sock.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        return Err(bad("unsupported SOCKS command"));
    }
    let host = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            sock.read_exact(&mut a).await?;
            Ipv4Addr::from(a).to_string()
        }
        3 => {
            let mut len = [0u8; 1];
            sock.read_exact(&mut len).await?;
            let mut name = vec![0u8; len[0] as usize];
            sock.read_exact(&mut name).await?;
            String::from_utf8(name).map_err(|_| bad("bad domain"))?
        }
        4 => {
            let mut a = [0u8; 16];
            sock.read_exact(&mut a).await?;
            Ipv6Addr::from(a).to_string()
        }
        _ => {
            sock.write_all(&[5, 8, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
            return Err(bad("unsupported address type"));
        }
    };
    let mut port = [0u8; 2];
    sock.read_exact(&mut port).await?;
    sock.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    Ok((host, u16::from_be_bytes(port)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssh::testutil::{spawn_sshd, target};
    use std::time::Duration;

    struct Collect(std::sync::mpsc::Sender<(Uuid, ForwardStatus)>);
    impl StatusSink for Collect {
        fn status(&self, id: Uuid, s: ForwardStatus) {
            let _ = self.0.send((id, s));
        }
    }

    async fn echo_server() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
        port
    }

    fn wait_active(rx: &std::sync::mpsc::Receiver<(Uuid, ForwardStatus)>) -> u16 {
        loop {
            match rx.recv_timeout(Duration::from_secs(15)).expect("status") {
                (_, ForwardStatus::Active { port }) => return port,
                (_, ForwardStatus::Error { message }) => panic!("forward error: {message}"),
                _ => {}
            }
        }
    }

    async fn roundtrip(mut s: TcpStream, msg: &[u8]) {
        s.write_all(msg).await.unwrap();
        let mut buf = vec![0u8; msg.len()];
        tokio::time::timeout(Duration::from_secs(10), s.read_exact(&mut buf))
            .await
            .expect("echo timed out")
            .unwrap();
        assert_eq!(buf, msg);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn local_remote_and_socks_forwarding() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let echo = echo_server().await;
        let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let mgr = Arc::new(ForwardManager::new());
        let (tx, rx) = std::sync::mpsc::channel();
        let sink: Arc<dyn StatusSink> = Arc::new(Collect(tx));

        // -L: local ephemeral port -> server -> echo
        let l_id = Uuid::new_v4();
        mgr.start(
            l_id,
            t.clone(),
            ForwardKind::Local {
                bind_addr: "127.0.0.1".into(),
                bind_port: 0,
                dest_host: "127.0.0.1".into(),
                dest_port: echo,
            },
            Arc::clone(&sink),
        )
        .unwrap();
        let lport = wait_active(&rx);
        roundtrip(
            TcpStream::connect(("127.0.0.1", lport)).await.unwrap(),
            b"via -L",
        )
        .await;
        assert!(mgr
            .start(
                l_id,
                t.clone(),
                ForwardKind::Dynamic {
                    bind_addr: "127.0.0.1".into(),
                    bind_port: 0
                },
                Arc::clone(&sink)
            )
            .is_err());

        // -D: SOCKS5 CONNECT by domain name.
        mgr.start(
            Uuid::new_v4(),
            t.clone(),
            ForwardKind::Dynamic {
                bind_addr: "127.0.0.1".into(),
                bind_port: 0,
            },
            Arc::clone(&sink),
        )
        .unwrap();
        let dport = wait_active(&rx);
        let mut s = TcpStream::connect(("127.0.0.1", dport)).await.unwrap();
        s.write_all(&[5, 1, 0]).await.unwrap();
        let mut r = [0u8; 2];
        s.read_exact(&mut r).await.unwrap();
        assert_eq!(r, [5, 0]);
        let host = b"localhost";
        let mut req = vec![5, 1, 0, 3, host.len() as u8];
        req.extend_from_slice(host);
        req.extend_from_slice(&echo.to_be_bytes());
        s.write_all(&req).await.unwrap();
        let mut rep = [0u8; 10];
        s.read_exact(&mut rep).await.unwrap();
        assert_eq!(rep[1], 0, "SOCKS reply code");
        roundtrip(s, b"via -D").await;

        // -R: server listens on an ephemeral port and dials back to echo.
        let r_id = Uuid::new_v4();
        mgr.start(
            r_id,
            t.clone(),
            ForwardKind::Remote {
                bind_addr: "127.0.0.1".into(),
                bind_port: 0,
                dest_host: "127.0.0.1".into(),
                dest_port: echo,
            },
            Arc::clone(&sink),
        )
        .unwrap();
        let rport = wait_active(&rx);
        roundtrip(
            TcpStream::connect(("127.0.0.1", rport)).await.unwrap(),
            b"via -R",
        )
        .await;

        // -L through a jump host (the test server bastions for itself).
        let mut jumped = t.clone();
        jumped.jump = Some(Box::new(t.clone()));
        mgr.start(
            Uuid::new_v4(),
            jumped,
            ForwardKind::Local {
                bind_addr: "127.0.0.1".into(),
                bind_port: 0,
                dest_host: "127.0.0.1".into(),
                dest_port: echo,
            },
            Arc::clone(&sink),
        )
        .unwrap();
        let jport = wait_active(&rx);
        roundtrip(
            TcpStream::connect(("127.0.0.1", jport)).await.unwrap(),
            b"via jump -L",
        )
        .await;

        // Stopping closes the local listener.
        mgr.stop(l_id);
        loop {
            if let (id, ForwardStatus::Stopped) = rx.recv_timeout(Duration::from_secs(10)).unwrap()
            {
                if id == l_id {
                    break;
                }
            }
        }
        assert!(TcpStream::connect(("127.0.0.1", lport)).await.is_err());
        assert!(!mgr.is_running(l_id));
        assert!(matches!(
            mgr.statuses().get(&l_id),
            Some(ForwardStatus::Stopped)
        ));

        mgr.stop_all();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(!mgr.is_running(r_id));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bind_conflict_is_reported() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            return;
        };
        let taken = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = taken.local_addr().unwrap().port();
        let mgr = Arc::new(ForwardManager::new());
        let (tx, rx) = std::sync::mpsc::channel();
        mgr.start(
            Uuid::new_v4(),
            target(&sshd, &sshd.client_key, dir.path().join("kh")),
            ForwardKind::Dynamic {
                bind_addr: "127.0.0.1".into(),
                bind_port: port,
            },
            Arc::new(Collect(tx)),
        )
        .unwrap();
        let msg = loop {
            if let (_, ForwardStatus::Error { message }) =
                rx.recv_timeout(Duration::from_secs(15)).unwrap()
            {
                break message;
            }
        };
        assert!(msg.contains("cannot listen"), "{msg}");
    }

    #[test]
    fn log_lines_never_carry_secrets() {
        let cases = [
            ("auth failed, password=hunter2 rejected", "hunter2"),
            ("login with password: s3cret!", "s3cret"),
            ("token \"abc.def.ghi\" expired", "abc.def"),
            ("passphrase hunter3", "hunter3"),
            ("Authorization: Bearer abcdef", "abcdef"),
            ("-----BEGIN OPENSSH PRIVATE KEY-----\nAAAAB3Nza\n-----END OPENSSH PRIVATE KEY----- then more", "AAAAB3Nza"),
            ("-----BEGIN RSA PRIVATE KEY----- cut off with no end MIIEowIB", "MIIEowIB"),
        ];
        for (input, secret) in cases {
            let out = sanitize(input);
            assert!(!out.contains(secret), "{input:?} leaked: {out:?}");
        }
        // Ordinary lines are left as they are, including the word on its own.
        assert_eq!(sanitize("Listening on 127.0.0.1:8000"), "Listening on 127.0.0.1:8000");
        assert_eq!(sanitize("password authentication failed"), "password authentication failed");
        assert!(sanitize(&"x".repeat(2000)).chars().count() <= MAX_LOG_LEN + 1);
    }

    struct Quiet;
    impl StatusSink for Quiet {
        fn status(&self, _: Uuid, _: ForwardStatus) {}
    }

    #[test]
    fn a_tunnel_keeps_a_bounded_log_without_repeats() {
        let m = ForwardManager::new();
        let id = Uuid::new_v4();
        for i in 0..(MAX_LOG_ENTRIES + 50) {
            m.log(&Quiet, id, LogLevel::Info, &format!("line {i}"));
        }
        let logs = m.logs(id);
        assert_eq!(logs.len(), MAX_LOG_ENTRIES);
        assert_eq!(logs.last().unwrap().message, format!("line {}", MAX_LOG_ENTRIES + 49));
        assert_eq!(logs.first().unwrap().message, "line 50", "the oldest lines are the ones dropped");
        m.log(&Quiet, id, LogLevel::Info, "line 249");
        assert_eq!(m.logs(id).len(), MAX_LOG_ENTRIES, "the same line twice in a row is kept once");
        m.forget(id);
        assert!(m.logs(id).is_empty());
    }

    #[test]
    fn active_since_follows_the_state() {
        let m = ForwardManager::new();
        let id = Uuid::new_v4();
        assert!(m.runtime().is_empty());
        m.set(&Quiet, id, ForwardStatus::Starting);
        assert!(m.runtime().is_empty(), "starting is not active yet");
        m.set(&Quiet, id, ForwardStatus::Active { port: 1 });
        let first = m.runtime()[&id].active_since_ms.unwrap();
        m.set(&Quiet, id, ForwardStatus::Active { port: 1 });
        assert_eq!(m.runtime()[&id].active_since_ms, Some(first), "a repeated report doesn't restart the clock");
        m.set(&Quiet, id, ForwardStatus::Error { message: "x".into() });
        assert!(m.runtime().is_empty());
        let log = m.logs(id);
        assert_eq!(log.iter().map(|l| (l.level, l.message.as_str())).collect::<Vec<_>>(), vec![(LogLevel::Info, "Starting tunnel"), (LogLevel::Error, "x")]);
    }

    fn rule(label: &str, kind: crate::models::ForwardKind) -> crate::models::ForwardRule {
        crate::models::ForwardRule { label: label.into(), host_id: Uuid::nil(), auto_start: false, description: String::new(), kind }
    }
    fn local(addr: &str, port: u16, dest: &str, dport: u16) -> crate::models::ForwardKind {
        crate::models::ForwardKind::Local { bind_addr: addr.into(), bind_port: port, dest_host: dest.into(), dest_port: dport }
    }

    #[test]
    fn validates_a_rule() {
        let ok = rule("db", local("127.0.0.1", 5432, "db.internal", 5432));
        assert!(validate_rule(&ok, None, &[]).is_ok());
        assert!(validate_rule(&rule(" ", local("127.0.0.1", 1, "h", 1)), None, &[]).unwrap_err().contains("name"));
        assert!(validate_rule(&rule("x", local("127.0.0.1", 0, "h", 1)), None, &[]).is_ok(), "port 0 picks a free port");
        assert!(validate_rule(&rule("x", local("127.0.0.1", 80, "h", 0)), None, &[]).unwrap_err().contains("destination port"));
        assert!(validate_rule(&rule("x", local("not an address", 80, "h", 80)), None, &[]).unwrap_err().contains("listened on"));
        assert!(validate_rule(&rule("x", local("::1", 80, "h", 80)), None, &[]).is_ok());
        assert!(validate_rule(&rule("x", local("127.0.0.1", 80, "bad host", 80)), None, &[]).unwrap_err().contains("destination host"));
        let remote = |addr: &str| rule("r", crate::models::ForwardKind::Remote { bind_addr: addr.into(), bind_port: 9000, dest_host: "127.0.0.1".into(), dest_port: 80 });
        assert!(validate_rule(&remote(""), None, &[]).is_ok() && validate_rule(&remote("*"), None, &[]).is_ok());
        assert!(validate_rule(&rule("x", local("", 80, "h", 80)), None, &[]).is_err(), "only a remote forward may leave the address empty");
    }

    #[test]
    fn two_rules_may_not_want_the_same_local_port() {
        let first = Uuid::new_v4();
        let others = vec![(first, rule("web", local("127.0.0.1", 8000, "h", 80)))];
        let dup = rule("other", local("127.0.0.1", 8000, "h2", 80));
        assert!(validate_rule(&dup, None, &others).unwrap_err().contains("already used by the tunnel \"web\""));
        // Editing the rule itself is not a clash with itself.
        assert!(validate_rule(&dup, Some(first), &others).is_ok());
        // A wildcard overlaps a specific address; another port does not clash.
        assert!(validate_rule(&rule("w", local("0.0.0.0", 8000, "h", 80)), None, &others).is_err());
        assert!(validate_rule(&rule("p", local("127.0.0.1", 8001, "h", 80)), None, &others).is_ok());
        // A SOCKS listener wants a port too.
        let socks = rule("s", crate::models::ForwardKind::Dynamic { bind_addr: "127.0.0.1".into(), bind_port: 8000 });
        assert!(validate_rule(&socks, None, &others).is_err());
    }

    /// The whole life of a tunnel against a real server: start, no double start, the port really carries data,
    /// restart onto the same fixed port, stop frees it, and the log tells the story without any secret.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_tunnel_starts_restarts_and_stops_cleanly_and_logs_what_happened() {
        let dir = tempfile::TempDir::new().unwrap();
        let Some(sshd) = spawn_sshd(dir.path()) else {
            eprintln!("skipping: no usable sshd on this machine");
            return;
        };
        let echo = echo_server().await;
        // A port that is free now, asked for by number so a restart must rebind the very same one.
        let fixed = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let t = target(&sshd, &sshd.client_key, dir.path().join("kh"));
        let mgr = Arc::new(ForwardManager::new());
        let (tx, rx) = std::sync::mpsc::channel();
        let sink: Arc<dyn StatusSink> = Arc::new(Collect(tx));
        let kind = ForwardKind::Local { bind_addr: "127.0.0.1".into(), bind_port: fixed, dest_host: "127.0.0.1".into(), dest_port: echo };
        let id = Uuid::new_v4();

        mgr.start(id, t.clone(), kind.clone(), Arc::clone(&sink)).unwrap();
        assert_eq!(wait_active(&rx), fixed);
        assert!(matches!(mgr.start(id, t.clone(), kind.clone(), Arc::clone(&sink)), Err(ForwardError::AlreadyRunning)), "no second process for the same tunnel");
        assert!(mgr.runtime()[&id].active_since_ms.is_some());
        roundtrip(TcpStream::connect(("127.0.0.1", fixed)).await.unwrap(), b"before restart").await;

        // Restart: stop, wait until the listener is gone, start again on the same port.
        mgr.stop_and_wait(id, Duration::from_secs(5)).await;
        assert!(!mgr.is_running(id));
        assert!(TcpStream::connect(("127.0.0.1", fixed)).await.is_err(), "the port is released once stopped");
        mgr.start(id, t.clone(), kind.clone(), Arc::clone(&sink)).unwrap();
        assert_eq!(wait_active(&rx), fixed);
        roundtrip(TcpStream::connect(("127.0.0.1", fixed)).await.unwrap(), b"after restart").await;

        // Stopping releases everything and leaves the tunnel stopped, not failed.
        mgr.stop_and_wait(id, Duration::from_secs(5)).await;
        assert!(TcpStream::connect(("127.0.0.1", fixed)).await.is_err());
        for _ in 0..50 {
            if mgr.statuses().get(&id) == Some(&ForwardStatus::Stopped) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert_eq!(mgr.statuses().get(&id), Some(&ForwardStatus::Stopped));
        assert!(mgr.runtime().is_empty());

        let log: Vec<String> = mgr.logs(id).into_iter().map(|l| l.message).collect();
        let text = log.join("\n");
        for expected in ["Starting tunnel", "Connecting to 127.0.0.1", "SSH connection established", "Listening on 127.0.0.1", "Tunnel stopped"] {
            assert!(text.contains(expected), "missing {expected:?} in:\n{text}");
        }
        assert!(!text.to_lowercase().contains("private key") || text.contains("[private key removed]"));
    }

    /// A tunnel whose SSH connection fails shows why, as a failure with a reason and a log line.
    #[tokio::test]
    async fn a_tunnel_that_cannot_connect_fails_with_a_reason() {
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let mut t = crate::ssh::testutil_target();
        t.hostname = "127.0.0.1".into();
        t.port = dead;
        let mgr = Arc::new(ForwardManager::new());
        let (tx, rx) = std::sync::mpsc::channel();
        let sink: Arc<dyn StatusSink> = Arc::new(Collect(tx));
        let id = Uuid::new_v4();
        mgr.start(id, t, ForwardKind::Dynamic { bind_addr: "127.0.0.1".into(), bind_port: 0 }, sink).unwrap();
        let mut failure = None;
        for _ in 0..100 {
            if let Ok((_, ForwardStatus::Error { message })) = rx.try_recv() {
                failure = Some(message);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let failure = failure.expect("the tunnel should fail");
        assert!(!failure.is_empty());
        assert!(!mgr.is_running(id), "a failed tunnel is not left registered as running");
        assert!(mgr.logs(id).iter().any(|l| l.level == LogLevel::Error && l.message == sanitize(&failure)));
    }
}
