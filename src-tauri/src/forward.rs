//! Port forwarding: local (-L), remote (-R) and dynamic SOCKS5 (-D).
//!
//! Each running rule owns its own SSH connection and a supervising task.
//! Stopping a rule (or losing the connection) tears down its listener and
//! every tunnelled socket. Tauri-agnostic, like the other engine modules.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr};
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
}

struct Running {
    stop: oneshot::Sender<()>,
}

#[derive(Default)]
pub struct ForwardManager {
    running: Mutex<HashMap<Uuid, Running>>,
    last: Mutex<HashMap<Uuid, ForwardStatus>>,
}

impl ForwardManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Current status of every rule that has run in this app session.
    pub fn statuses(&self) -> HashMap<Uuid, ForwardStatus> {
        self.last.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    fn set(&self, sink: &dyn StatusSink, id: Uuid, st: ForwardStatus) {
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
        {
            let mut running = self.running.lock().unwrap_or_else(|p| p.into_inner());
            if running.contains_key(&rule_id) {
                return Err(ForwardError::AlreadyRunning);
            }
            running.insert(rule_id, Running { stop: stop_tx });
        }
        self.set(sink.as_ref(), rule_id, ForwardStatus::Starting);

        let me = Arc::clone(self);
        tokio::spawn(async move {
            let end = match run_rule(&me, rule_id, &target, &kind, sink.as_ref(), stop_rx).await {
                Ok(()) => ForwardStatus::Stopped,
                Err(e) => ForwardStatus::Error {
                    message: e.to_string(),
                },
            };
            me.running
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&rule_id);
            me.set(sink.as_ref(), rule_id, end);
        });
        Ok(())
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

async fn run_rule(
    mgr: &ForwardManager,
    rule_id: Uuid,
    target: &Target,
    kind: &ForwardKind,
    sink: &dyn StatusSink,
    mut stop: oneshot::Receiver<()>,
) -> Result<(), ForwardError> {
    let remote_table: RemoteForwards = Arc::new(Mutex::new(HashMap::new()));
    let (handle, _) = open_client(target, Some(Arc::clone(&remote_table))).await?;
    let handle = Arc::new(handle);

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
            mgr.set(sink, rule_id, ForwardStatus::Active { port });
            accept_loop(listener, &handle, &mut stop, {
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
            mgr.set(sink, rule_id, ForwardStatus::Active { port });
            accept_loop(listener, &handle, &mut stop, |_| None).await
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
            mgr.set(sink, rule_id, ForwardStatus::Active { port: port as u16 });
            wait_until_stopped_or_closed(&handle, &mut stop).await;
            let _ = handle.cancel_tcpip_forward(bind_addr.clone(), port).await;
            Ok(())
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
        let _ = accept_loop(listener, &handle, &mut stop_rx, move |_| Some((dest_host.clone(), dest_port))).await;
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

async fn wait_until_stopped_or_closed(handle: &Client, stop: &mut oneshot::Receiver<()>) {
    loop {
        tokio::select! {
            _ = &mut *stop => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {
                if handle.is_closed() { return; }
            }
        }
    }
}

/// Accept TCP clients until stopped. `fixed_dest` returns the destination for
/// a plain forward, or `None` to run a SOCKS5 handshake per client.
/// Connections one forward carries at once.
const MAX_TUNNELS: usize = 256;
const SOCKS_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn accept_loop<F>(
    listener: TcpListener,
    handle: &Arc<Client>,
    stop: &mut oneshot::Receiver<()>,
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
                    break Err(ForwardError::Ssh(SshError::Gone));
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
                tasks.spawn(async move {
                    let _ = tunnel(sock, peer, dest, &handle).await;
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
}
