//! A small HTTP client for the engines that speak JSON over HTTP (rqlite, Elasticsearch): one base address, one
//! kind of sign-in, a cap on how much an answer may hold, and requests that can be cancelled.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::Notify;
use uuid::Uuid;

use super::{ConnectSpec, DbError, DbResult, TlsMode, CONNECT_TIMEOUT};

/// The most an answer may hold; a bigger one is refused rather than held in memory.
pub const MAX_BODY: usize = 64 * 1024 * 1024;

#[derive(Clone)]
pub enum Auth {
    None,
    Basic { user: String, password: String },
    Bearer(String),
    /// Elasticsearch's own header: `Authorization: ApiKey <value>`.
    ApiKey(String),
}

pub struct Http {
    client: reqwest::Client,
    base: String,
    auth: Auth,
    running: Mutex<HashMap<Uuid, Arc<Notify>>>,
}

pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
}

/// `http://host:port` or `https://host:port`, from the connection: the address is dialled directly, or through
/// the tunnel's loopback port with the name still used for the certificate check.
pub fn base_url(spec: &ConnectSpec) -> String {
    let scheme = if spec.tls == TlsMode::Disable { "http" } else { "https" };
    let (host, port) = spec.dial();
    let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host };
    format!("{scheme}://{host}:{port}")
}

impl Http {
    pub fn new(spec: &ConnectSpec, auth: Auth) -> DbResult<Self> {
        // reqwest is built without a bundled crypto provider; install ring once, as the updater does.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut b = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT).user_agent(concat!("SSHVault/", env!("CARGO_PKG_VERSION")));
        match spec.tls {
            TlsMode::Disable => {}
            TlsMode::Require => b = b.tls_danger_accept_invalid_certs(true),
            // Through a tunnel the address is loopback, so the name can't be matched; the chain is still checked.
            TlsMode::VerifyFull => {
                if spec.tunnel_port.is_some() {
                    b = b.tls_danger_accept_invalid_hostnames(true);
                }
            }
        }
        let client = b.build().map_err(|e| DbError::Invalid(format!("couldn't start the HTTP client: {e}")))?;
        Ok(Self { client, base: base_url(spec), auth, running: Mutex::default() })
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    fn describe(&self, e: &reqwest::Error) -> String {
        // The whole chain, so a TLS or socket failure shows its real reason.
        let mut text = e.to_string();
        let mut source = std::error::Error::source(e);
        while let Some(s) = source {
            text.push_str(": ");
            text.push_str(&s.to_string());
            source = s.source();
        }
        text
    }

    /// One request. `qid` makes it cancellable. A non-2xx status is returned, not an error: each engine words
    /// its own failures from the body.
    pub async fn send(&self, qid: Option<Uuid>, method: &str, path: &str, body: Option<Vec<u8>>, timeout: Duration) -> DbResult<Reply> {
        let method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|_| DbError::Invalid(format!("\"{method}\" isn't an HTTP method")))?;
        let url = format!("{}{}", self.base, if path.starts_with('/') { path.to_string() } else { format!("/{path}") });
        let mut req = self.client.request(method, &url).timeout(timeout).header("Accept", "application/json");
        req = match &self.auth {
            Auth::None => req,
            Auth::Basic { user, password } => req.basic_auth(user, Some(password)),
            Auth::Bearer(t) => req.bearer_auth(t),
            Auth::ApiKey(k) => req.header("Authorization", format!("ApiKey {k}")),
        };
        if let Some(b) = body {
            req = req.header("Content-Type", "application/json").body(b);
        }
        let notify = qid.map(|q| {
            let n = Arc::new(Notify::new());
            self.running.lock().unwrap_or_else(|p| p.into_inner()).insert(q, Arc::clone(&n));
            n
        });
        let work = async {
            let mut res = req.send().await.map_err(|e| {
                if e.is_timeout() {
                    DbError::Timeout(self.base.clone())
                } else if e.is_connect() {
                    DbError::Connect { addr: self.base.clone(), reason: self.describe(&e) }
                } else {
                    DbError::Server(self.describe(&e))
                }
            })?;
            let status = res.status().as_u16();
            let mut body = Vec::new();
            while let Some(chunk) = res.chunk().await.map_err(|e| DbError::Server(self.describe(&e)))? {
                if body.len() + chunk.len() > MAX_BODY {
                    return Err(DbError::Server(format!("the answer is larger than {} MB; ask for less (a limit, a filter)", MAX_BODY / (1024 * 1024))));
                }
                body.extend_from_slice(&chunk);
            }
            Ok(Reply { status, body })
        };
        let out = match &notify {
            Some(n) => tokio::select! {
                r = work => r,
                _ = n.notified() => Err(DbError::Cancelled),
            },
            None => work.await,
        };
        if let Some(q) = qid {
            self.running.lock().unwrap_or_else(|p| p.into_inner()).remove(&q);
        }
        out
    }

    pub fn cancel(&self, qid: Uuid) {
        if let Some(n) = self.running.lock().unwrap_or_else(|p| p.into_inner()).get(&qid) {
            n.notify_one();
        }
    }
}

/// A stand-in HTTP server for the engines' tests: answers each request from a function and records what it got.
#[cfg(test)]
pub(crate) mod fake {
    use std::sync::{Arc, Mutex};

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    pub struct Request {
        pub method: String,
        pub path: String,
        pub authorization: Option<String>,
        pub body: String,
    }

    pub struct Fake {
        pub port: u16,
        pub seen: Arc<Mutex<Vec<Request>>>,
    }

    pub async fn serve(answer: impl Fn(&Request) -> (u16, String) + Send + Sync + 'static) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Arc<Mutex<Vec<Request>>> = Arc::default();
        let log = Arc::clone(&seen);
        let answer = Arc::new(answer);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let (log, answer) = (Arc::clone(&log), Arc::clone(&answer));
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let (head, body) = loop {
                        let n = s.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&buf[..i]).to_string();
                            let want: usize = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                            while buf.len() < i + 4 + want {
                                let n = s.read(&mut chunk).await.unwrap_or(0);
                                if n == 0 {
                                    break;
                                }
                                buf.extend_from_slice(&chunk[..n]);
                            }
                            break (head, String::from_utf8_lossy(&buf[i + 4..]).to_string());
                        }
                    };
                    let line = head.lines().next().unwrap_or("").to_string();
                    let mut parts = line.split(' ');
                    let req = Request {
                        method: parts.next().unwrap_or("").to_string(),
                        path: parts.next().unwrap_or("").to_string(),
                        authorization: head.lines().find_map(|l| l.to_ascii_lowercase().starts_with("authorization:").then(|| l.to_string())),
                        body,
                    };
                    let (status, text) = answer(&req);
                    log.lock().unwrap().push(req);
                    let reply = format!("HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}", text.len());
                    let _ = s.write_all(reply.as_bytes()).await;
                });
            }
        });
        Fake { port, seen }
    }
}
