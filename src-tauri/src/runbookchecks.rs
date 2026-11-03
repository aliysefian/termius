//! The checks a runbook step can make from this computer: a web address answers as expected, a TCP port accepts a
//! connection. They are made from here, not from the host the step is "on", so they tell you what a person on this
//! network would see; a service that only listens inside a private network is checked with a `run` step instead.

use std::time::{Duration, Instant};

use crate::runbook::{HttpMethod, Output};

/// Longest one attempt may take, whatever the step asks for.
pub const MAX_ATTEMPT: Duration = Duration::from_secs(120);

fn finish(ok: bool, started: Instant, said: String, why: String) -> Output {
    Output { stdout: said, stderr: why, exit_code: Some(u32::from(!ok)), truncated: false, duration_ms: started.elapsed().as_millis() as u64 }
}

/// Whether `expect` (0 or none: any 2xx or 3xx) accepts `got`.
pub fn status_accepted(expect: Option<u16>, got: u16) -> bool {
    match expect {
        None | Some(0) => (200..400).contains(&got),
        Some(e) => got == e,
    }
}

/// One request. A site that doesn't answer is a failed result, not an error. Redirects are not followed, so a site
/// that answers `301` is seen as `301`.
pub async fn http_once(method: HttpMethod, url: &str, expect: Option<u16>, contains: Option<&str>, limit: Duration) -> Output {
    let started = Instant::now();
    let limit = limit.min(MAX_ATTEMPT);
    if !crate::monitoring::http::valid_url(url) {
        return finish(false, started, String::new(), "that is not an http:// or https:// address".into());
    }
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = match reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(limit)
        .timeout(limit)
        .user_agent(concat!("SSHVault-runbook/", env!("CARGO_PKG_VERSION")))
        .build()
    {
        Ok(c) => c,
        Err(e) => return finish(false, started, String::new(), format!("could not start the web client: {}", e.without_url())),
    };
    let request = match method {
        HttpMethod::Get => client.get(url),
        HttpMethod::Head => client.head(url),
    };
    let response = match request.send().await {
        Ok(r) => r,
        Err(e) => {
            // The address can carry a token in its query, and doesn't belong in a message that is kept.
            let e = e.without_url();
            let mut why = e.to_string();
            let mut cur = std::error::Error::source(&e);
            while let Some(inner) = cur {
                let t = inner.to_string();
                if !why.contains(&t) {
                    why.push_str(": ");
                    why.push_str(&t);
                }
                cur = inner.source();
            }
            return finish(false, started, String::new(), why);
        }
    };
    let code = response.status().as_u16();
    let first_byte = started.elapsed().as_millis();
    let mut said = format!("HTTP {code} in {first_byte} ms");
    if !status_accepted(expect, code) {
        let want = match expect {
            None | Some(0) => "a 2xx or 3xx status".to_string(),
            Some(e) => e.to_string(),
        };
        return finish(false, started, said, format!("expected {want}, got {code}"));
    }
    if let Some(text) = contains {
        // Only the start of the body is read: enough to find a marker, bounded so a large download can't stall the run.
        let body = match response.bytes().await {
            Ok(b) => b,
            Err(e) => return finish(false, started, said, format!("the answer was cut short: {}", e.without_url())),
        };
        let head = &body[..body.len().min(1024 * 1024)];
        if !String::from_utf8_lossy(head).contains(text) {
            return finish(false, started, said, format!("the page does not contain {text:?}"));
        }
        said.push_str(&format!(", contains {text:?}"));
    }
    finish(true, started, said, String::new())
}

/// One TCP connection, closed at once: nothing is sent.
pub async fn tcp_once(host: &str, port: u16, limit: Duration) -> Output {
    let started = Instant::now();
    let r = crate::monitoring::checks::tcp_check(host, port, limit.min(MAX_ATTEMPT)).await;
    if r.open {
        finish(true, started, format!("{host}:{port} accepted a connection in {} ms", r.latency_ms.unwrap_or(0)), String::new())
    } else {
        finish(false, started, String::new(), r.error.unwrap_or_else(|| format!("{host}:{port} did not accept a connection")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot HTTP server answering every connection with `status` and `body`.
    async fn serve(status: &'static str, body: &'static str) -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf).await;
                let reply = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = s.write_all(reply.as_bytes()).await;
            }
        });
        port
    }

    #[test]
    fn a_status_is_accepted_by_range_or_exactly() {
        assert!(status_accepted(None, 200) && status_accepted(Some(0), 301) && !status_accepted(None, 404));
        assert!(status_accepted(Some(204), 204) && !status_accepted(Some(204), 200));
    }

    #[tokio::test]
    async fn a_healthy_page_passes_and_says_how_fast() {
        let port = serve("200 OK", "all good").await;
        let r = http_once(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"), None, Some("good"), Duration::from_secs(5)).await;
        assert_eq!(r.exit_code, Some(0), "{r:?}");
        assert!(r.stdout.starts_with("HTTP 200 in ") && r.stdout.contains("contains \"good\""));
    }

    #[tokio::test]
    async fn the_wrong_status_or_missing_text_fails_with_the_reason() {
        let port = serve("503 Service Unavailable", "down").await;
        let url = format!("http://127.0.0.1:{port}/");
        let r = http_once(HttpMethod::Get, &url, None, None, Duration::from_secs(5)).await;
        assert_eq!((r.exit_code, r.stderr.as_str()), (Some(1), "expected a 2xx or 3xx status, got 503"));
        let r = http_once(HttpMethod::Get, &url, Some(503), None, Duration::from_secs(5)).await;
        assert_eq!(r.exit_code, Some(0));
        let port = serve("200 OK", "hello").await;
        let r = http_once(HttpMethod::Head, &format!("http://127.0.0.1:{port}/"), Some(200), None, Duration::from_secs(5)).await;
        assert_eq!(r.exit_code, Some(0));
        let r = http_once(HttpMethod::Get, &format!("http://127.0.0.1:{port}/"), Some(200), Some("world"), Duration::from_secs(5)).await;
        assert!(r.stderr.contains("does not contain"), "{r:?}");
    }

    #[tokio::test]
    async fn an_address_that_is_not_http_or_does_not_answer_fails_cleanly() {
        let r = http_once(HttpMethod::Get, "ftp://x", None, None, Duration::from_secs(1)).await;
        assert_eq!(r.exit_code, Some(1));
        let r = http_once(HttpMethod::Get, "http://127.0.0.1:1/", None, None, Duration::from_secs(2)).await;
        assert_eq!(r.exit_code, Some(1));
        assert!(!r.stderr.is_empty());
    }

    #[tokio::test]
    async fn a_port_is_open_or_it_says_why_not() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let open = tcp_once("127.0.0.1", port, Duration::from_secs(2)).await;
        assert_eq!(open.exit_code, Some(0));
        drop(listener);
        let closed = tcp_once("127.0.0.1", port, Duration::from_secs(2)).await;
        assert_eq!(closed.exit_code, Some(1));
        assert!(!closed.stderr.is_empty());
    }
}
