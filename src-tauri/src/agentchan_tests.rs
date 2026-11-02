//! A server that opens an agent-forwarding channel nobody asked for. Every connection carries the host's "forward
//! agent" setting, but only a terminal session sends the request (and has the on/off gate and the approval prompts).
//! A connection that never asked must refuse the channel, or any server could use the agent on it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::server::{self, Auth, Handler, Session};
use tokio::net::TcpListener;

use crate::models::AuthMethod;
use crate::ssh::{open_client, testutil_target, HostKeyPolicy, Target};

/// Whether the client accepted the agent channel the server opened on its own, once known.
type Outcome = Arc<Mutex<Option<bool>>>;

struct Probe {
    outcome: Outcome,
}

impl Handler for Probe {
    type Error = russh::Error;

    async fn auth_password(&mut self, _user: &str, _password: &str) -> Result<Auth, Self::Error> {
        Ok(Auth::Accept)
    }

    async fn auth_succeeded(&mut self, session: &mut Session) -> Result<(), Self::Error> {
        let handle = session.handle();
        let outcome = self.outcome.clone();
        tokio::spawn(async move {
            let accepted = handle.channel_open_agent().await.is_ok();
            *outcome.lock().unwrap() = Some(accepted);
        });
        Ok(())
    }
}

async fn serve() -> (u16, Outcome) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let outcome: Outcome = Arc::new(Mutex::new(None));
    let host_key = russh::keys::decode_secret_key(&crate::keys::generate_ed25519("host").unwrap().private_key, None).unwrap();
    let config = Arc::new(server::Config { keys: vec![host_key], auth_rejection_time: Duration::from_millis(1), auth_rejection_time_initial: Some(Duration::ZERO), ..Default::default() });
    let record = outcome.clone();
    tokio::spawn(async move {
        let Ok((socket, _)) = listener.accept().await else { return };
        if let Ok(session) = server::run_stream(config, socket, Probe { outcome: record }).await {
            let _ = session.await;
        }
    });
    (port, outcome)
}

fn target(port: u16, dir: &std::path::Path) -> Target {
    Target {
        hostname: "127.0.0.1".into(),
        port,
        username: "probe".into(),
        auth: AuthMethod::Password { password: "pw".into() },
        host_keys: HostKeyPolicy::tofu_file(dir.join("known_hosts")),
        // The host has agent forwarding switched on, as the vulnerable configuration needs.
        forward_agent: true,
        ..testutil_target()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_agent_channel_the_connection_never_asked_for_is_refused() {
    let dir = tempfile::TempDir::new().unwrap();
    let (port, outcome) = serve().await;
    // `open_client` is what SFTP, tunnels, jump hops, run jobs and monitoring use: it never sends the agent request.
    let (_client, _) = open_client(&target(port, dir.path()), None).await.expect("log in");
    let started = std::time::Instant::now();
    let answer = loop {
        if let Some(a) = *outcome.lock().unwrap() {
            break a;
        }
        assert!(started.elapsed() < Duration::from_secs(10), "the server never learned the client's answer");
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(!answer, "the client must refuse an agent channel it did not request");
}
