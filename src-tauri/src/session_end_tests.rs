//! How a terminal session ends, as the app tells it apart: the other end finishing the session (`exit`, a closed channel,
//! with or without an exit status) versus the connection dying under it. Only the second is worth reconnecting. A small
//! SSH server of our own ends the shell in each way, and the real client reports what it saw.

use std::sync::mpsc as std_mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use russh::server::{self, Auth, Handler, Msg, Session};
use russh::{Channel, ChannelId, Disconnect};
use tokio::net::TcpListener;
use tokio::sync::Notify;

use crate::models::AuthMethod;
use crate::ssh::{testutil_target, ConnectParams, HostKeyPolicy, SessionStatus, SshManager, TermSink};

/// What the server does right after the shell starts.
#[derive(Clone, Copy)]
enum End {
    /// `exit 3`: the status, EOF, then close (what OpenSSH sends).
    Status,
    /// EOF and close with no status (network gear, some `su` and `sudo -i` chains).
    EofOnly,
    /// A close message and nothing else.
    CloseOnly,
    /// EOF first, the status a moment later.
    LateStatus,
    /// The shell was killed by a signal.
    Signal,
    /// The server says goodbye properly (a shutdown) without ending the channel first.
    Disconnect,
    /// The socket just goes away (a cable pulled, a NAT that forgot the connection).
    Vanish,
}

struct Fake {
    end: End,
    vanish: Arc<Notify>,
}

impl Handler for Fake {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn channel_open_session(&mut self, _channel: Channel<Msg>, reply: server::ChannelOpenHandle, _session: &mut Session) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        _cols: u32,
        _rows: u32,
        _pix_w: u32,
        _pix_h: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        Ok(())
    }

    async fn shell_request(&mut self, channel: ChannelId, session: &mut Session) -> Result<(), Self::Error> {
        session.channel_success(channel)?;
        let handle = session.handle();
        let (end, vanish) = (self.end, self.vanish.clone());
        tokio::spawn(async move {
            // Let the client settle into its read loop first.
            tokio::time::sleep(Duration::from_millis(200)).await;
            match end {
                End::Status => {
                    let _ = handle.exit_status_request(channel, 3).await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                }
                End::EofOnly => {
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                }
                End::CloseOnly => {
                    let _ = handle.close(channel).await;
                }
                End::LateStatus => {
                    let _ = handle.eof(channel).await;
                    tokio::time::sleep(Duration::from_millis(150)).await;
                    let _ = handle.exit_status_request(channel, 5).await;
                    let _ = handle.close(channel).await;
                }
                End::Signal => {
                    let _ = handle.exit_signal_request(channel, russh::Sig::KILL, false, "killed".into(), "".into()).await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                }
                End::Disconnect => {
                    let _ = handle.disconnect(Disconnect::ByApplication, "the server is shutting down".into(), "".into()).await;
                }
                End::Vanish => vanish.notify_one(),
            }
        });
        Ok(())
    }
}

/// A server whose every shell ends the given way, behind a relay that can cut the link. Returns the relay's port.
async fn serve(end: End) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let server_port = listener.local_addr().unwrap().port();
    let host_key = russh::keys::decode_secret_key(&crate::keys::generate_ed25519("host").unwrap().private_key, None).unwrap();
    let config = Arc::new(server::Config { keys: vec![host_key], auth_rejection_time: Duration::from_millis(1), auth_rejection_time_initial: Some(Duration::ZERO), ..Default::default() });
    let cut = Arc::new(Notify::new());
    let signal = cut.clone();
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else { return };
            let (config, signal) = (config.clone(), signal.clone());
            tokio::spawn(async move {
                if let Ok(session) = server::run_stream(config, socket, Fake { end, vanish: signal }).await {
                    let _ = session.await;
                }
            });
        }
    });
    // Dropping a russh session does not close its socket, so the link is cut where a cable would be: between the two.
    let relay = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = relay.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut client, _)) = relay.accept().await else { return };
            let cut = cut.clone();
            tokio::spawn(async move {
                let Ok(mut server) = tokio::net::TcpStream::connect(("127.0.0.1", server_port)).await else { return };
                tokio::select! {
                    _ = tokio::io::copy_bidirectional(&mut client, &mut server) => {}
                    _ = cut.notified() => {}
                }
            });
        }
    });
    port
}

struct Sink(std_mpsc::Sender<SessionStatus>);

impl TermSink for Sink {
    fn data(&self, _bytes: &[u8]) {}
    fn status(&self, status: SessionStatus) {
        let _ = self.0.send(status);
    }
}

/// Connect to the fake server and return how the session reported its end.
async fn ended_how(end: End) -> SessionStatus {
    let dir = tempfile::TempDir::new().unwrap();
    let port = serve(end).await;
    let target = crate::ssh::Target {
        hostname: "127.0.0.1".into(),
        port,
        username: "tester".into(),
        auth: AuthMethod::Password { password: "pw".into() },
        host_keys: HostKeyPolicy::tofu_file(dir.path().join("known_hosts")),
        ..testutil_target()
    };
    let (tx, rx) = std_mpsc::channel();
    let manager = Arc::new(SshManager::new());
    manager.connect("pane".into(), ConnectParams { target, cols: 80, rows: 24 }, Arc::new(Sink(tx))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left).expect("the session never reported its end") {
            s @ SessionStatus::Disconnected { .. } => return s,
            SessionStatus::Error { message } => panic!("unexpected error: {message}"),
            _ => {}
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shell_that_exits_with_a_status_has_ended() {
    assert_eq!(ended_how(End::Status).await, SessionStatus::Disconnected { code: Some(3), ended: true });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn eof_and_close_without_a_status_is_the_shell_ending_not_a_drop() {
    assert_eq!(ended_how(End::EofOnly).await, SessionStatus::Disconnected { code: None, ended: true });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_close_with_nothing_else_is_the_shell_ending_too() {
    assert_eq!(ended_how(End::CloseOnly).await, SessionStatus::Disconnected { code: None, ended: true });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_status_that_arrives_after_eof_is_still_reported() {
    assert_eq!(ended_how(End::LateStatus).await, SessionStatus::Disconnected { code: Some(5), ended: true });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shell_killed_by_a_signal_has_ended() {
    assert_eq!(ended_how(End::Signal).await, SessionStatus::Disconnected { code: None, ended: true });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_that_shuts_down_without_closing_the_channel_is_a_drop() {
    assert_eq!(ended_how(End::Disconnect).await, SessionStatus::Disconnected { code: None, ended: false });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_connection_that_vanishes_is_a_drop() {
    assert_eq!(ended_how(End::Vanish).await, SessionStatus::Disconnected { code: None, ended: false });
}
