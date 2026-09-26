//! Reachability checks: can we open a TCP connection to a host's SSH port,
//! how long does it take, and which SSH server answers? No login happens,
//! so no credentials are needed and nothing is logged on the server beyond
//! a dropped connection.

use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;
use uuid::Uuid;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(4);
const BANNER_TIMEOUT: Duration = Duration::from_secs(3);
/// Hosts probed at the same time.
pub const CONCURRENCY: usize = 32;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Health {
    /// Port open. `banner` is the server's identification line, if it sent
    /// one in time (e.g. "SSH-2.0-OpenSSH_9.6p1 Ubuntu-3ubuntu13").
    Up {
        latency_ms: u64,
        banner: Option<String>,
    },
    Down {
        reason: String,
    },
    /// Reached only through a jump host, which we don't open for a probe.
    ViaJump,
}

#[derive(Debug, Clone, Serialize)]
pub struct HealthResult {
    pub host_id: Uuid,
    #[serde(flatten)]
    pub health: Health,
}

/// Probe one address.
pub async fn probe(host: &str, port: u16) -> Health {
    let start = Instant::now();
    let mut stream =
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect((host, port))).await {
            Err(_) => {
                return Health::Down {
                    reason: format!("no answer within {}s", CONNECT_TIMEOUT.as_secs()),
                }
            }
            Ok(Err(e)) => {
                return Health::Down {
                    reason: e.to_string(),
                }
            }
            Ok(Ok(s)) => s,
        };
    let latency_ms = start.elapsed().as_millis() as u64;

    // RFC 4253: the server sends "SSH-2.0-..." first, possibly after other
    // lines. Read a little and pick the SSH line.
    let mut buf = vec![0u8; 512];
    let mut len = 0;
    let banner = tokio::time::timeout(BANNER_TIMEOUT, async {
        loop {
            let n = stream.read(&mut buf[len..]).await.ok()?;
            if n == 0 {
                return None;
            }
            len += n;
            let text = String::from_utf8_lossy(&buf[..len]).into_owned();
            if let Some(line) = text.lines().find(|l| l.starts_with("SSH-")) {
                if text.contains('\n') || len == buf.len() {
                    return Some(line.trim_end().to_string());
                }
            }
            if len == buf.len() {
                return None;
            }
        }
    })
    .await
    .ok()
    .flatten();
    Health::Up { latency_ms, banner }
}

/// Probe many `(host_id, address, port)` targets concurrently. `None` as the
/// address means the host sits behind a jump host.
pub async fn probe_all(targets: Vec<(Uuid, Option<(String, u16)>)>) -> Vec<HealthResult> {
    use tokio::sync::Semaphore;
    let limit = std::sync::Arc::new(Semaphore::new(CONCURRENCY));
    let mut set = tokio::task::JoinSet::new();
    for (host_id, addr) in targets {
        let limit = std::sync::Arc::clone(&limit);
        set.spawn(async move {
            let health = match addr {
                None => Health::ViaJump,
                Some((h, p)) => {
                    let _permit = limit.acquire_owned().await;
                    probe(&h, p).await
                }
            };
            HealthResult { host_id, health }
        });
    }
    let mut out = Vec::new();
    while let Some(r) = set.join_next().await {
        if let Ok(r) = r {
            out.push(r);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn reports_banner_latency_and_failures() {
        // A fake SSH server that greets like OpenSSH.
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                let _ = s.write_all(b"SSH-2.0-OpenSSH_9.6p1 Ubuntu-3\r\n").await;
            }
        });
        match probe("127.0.0.1", port).await {
            Health::Up { banner, latency_ms } => {
                assert_eq!(banner.as_deref(), Some("SSH-2.0-OpenSSH_9.6p1 Ubuntu-3"));
                assert!(latency_ms < 2000);
            }
            other => panic!("{other:?}"),
        }

        // Open port that never speaks: up, no banner.
        let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let sport = silent.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut keep = Vec::new();
            while let Ok((s, _)) = silent.accept().await {
                keep.push(s);
            }
        });
        assert!(matches!(
            probe("127.0.0.1", sport).await,
            Health::Up { banner: None, .. }
        ));

        // Closed port.
        let closed = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        assert!(matches!(
            probe("127.0.0.1", closed).await,
            Health::Down { .. }
        ));

        let ids: Vec<Uuid> = (0..3).map(|_| Uuid::new_v4()).collect();
        let res = probe_all(vec![
            (ids[0], Some(("127.0.0.1".into(), port))),
            (ids[1], Some(("127.0.0.1".into(), closed))),
            (ids[2], None),
        ])
        .await;
        let find = |id| res.iter().find(|r| r.host_id == id).unwrap().health.clone();
        assert!(matches!(find(ids[0]), Health::Up { .. }));
        assert!(matches!(find(ids[1]), Health::Down { .. }));
        assert_eq!(find(ids[2]), Health::ViaJump);
    }
}
