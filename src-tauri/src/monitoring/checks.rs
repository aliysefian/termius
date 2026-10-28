//! The port check. (Ping is in `ping.rs`, the SSH readings in `metrics.rs`.)

use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpResult {
    pub open: bool,
    /// How long the connection took to open.
    pub latency_ms: Option<u64>,
    /// Why it did not open, in a sentence.
    pub error: Option<String>,
}

/// Try to open a TCP connection and close it again. Nothing is sent, so the service sees only a dropped connection.
pub async fn tcp_check(host: &str, port: u16, limit: Duration) -> TcpResult {
    let start = Instant::now();
    let addr = format!("{host}:{port}");
    match tokio::time::timeout(limit, tokio::net::TcpStream::connect((host, port))).await {
        Ok(Ok(_stream)) => TcpResult { open: true, latency_ms: Some(start.elapsed().as_millis() as u64), error: None },
        Ok(Err(e)) => TcpResult { open: false, latency_ms: None, error: Some(crate::neterr::connect_text(&addr, &e)) },
        Err(_) => TcpResult { open: false, latency_ms: None, error: Some(format!("{addr} did not answer within {} s", limit.as_secs().max(1))) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_open_port_is_open_and_a_closed_one_says_why() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let open = tcp_check("127.0.0.1", port, Duration::from_secs(2)).await;
        assert!(open.open && open.error.is_none() && open.latency_ms.is_some(), "{open:?}");
        drop(listener);
        let closed = tcp_check("127.0.0.1", port, Duration::from_secs(2)).await;
        assert!(!closed.open && closed.latency_ms.is_none(), "{closed:?}");
        assert!(closed.error.unwrap().contains("127.0.0.1"));
    }
}
