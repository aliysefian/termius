//! Telegram and Bale. Bale's bot API is Telegram's with another address, so one channel serves both.
//!
//! The token is part of the request address (`/bot<token>/sendMessage`), and an HTTP library's error text includes
//! the address. So every error is stripped of its address first and then of the token itself, and the tests check
//! that no failure ever shows it.
//!
//! Where a network blocks the service, a bot can be set to go out through one of the saved SOCKS5 or HTTP proxies
//! (the same list the SSH hosts use). A proxy that is set but cannot be used stops the message; it never falls back
//! to a direct connection, which would be the very thing the proxy is there to avoid.

use std::time::Duration;

use super::{BoxFuture, Channel, Notification, NotifyError, Proxies};
use crate::models::ProxySpec;
use crate::monitoring::model::{BotConfig, ChannelKind};

pub const TELEGRAM_BASE: &str = "https://api.telegram.org";
pub const BALE_BASE: &str = "https://tapi.bale.ai";
/// Telegram refuses more than 4096 characters.
const MAX_TEXT: usize = 4000;
const MAX_REPLY: usize = 64 * 1024;

pub struct BotChannel {
    kind: ChannelKind,
    base: String,
    token: String,
    chat_id: String,
    client: reqwest::Client,
    /// What a proxy is called in an error, such as `the SOCKS5 proxy 10.0.0.2:1080`. `None`: sent directly.
    via: Option<String>,
    /// The proxy's password, kept out of every error like the token is.
    proxy_password: Option<String>,
}

fn describe(e: &(dyn std::error::Error + 'static)) -> String {
    let mut text = e.to_string();
    let mut cur = e.source();
    while let Some(inner) = cur {
        let t = inner.to_string();
        if !text.contains(&t) {
            text.push_str(": ");
            text.push_str(&t);
        }
        cur = inner.source();
    }
    text
}

async fn read_capped(mut res: reqwest::Response) -> Vec<u8> {
    let mut out = Vec::new();
    while let Ok(Some(chunk)) = res.chunk().await {
        out.extend_from_slice(&chunk);
        if out.len() >= MAX_REPLY {
            break;
        }
    }
    out
}

/// `https://` anywhere; plain `http://` only for this computer (a test server, a local proxy).
fn base_is_safe(base: &str) -> bool {
    if base.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let lower = base.to_ascii_lowercase();
    lower.starts_with("https://") || ["http://127.0.0.1", "http://localhost", "http://[::1]"].iter().any(|p| lower.strip_prefix(p).is_some_and(|rest| rest.is_empty() || rest.starts_with(':') || rest.starts_with('/')))
}

/// The proxy a bot is set to use, if any. A proxy that is set but gone, or that is not SOCKS5 or HTTP, is an error.
pub fn proxy_for<'a>(kind: ChannelKind, cfg: &BotConfig, proxies: &'a Proxies) -> Result<Option<&'a ProxySpec>, NotifyError> {
    let Some(id) = cfg.proxy_id else { return Ok(None) };
    let label = kind.label();
    let Some(saved) = proxies.get(&id) else {
        return Err(NotifyError(format!("{label}: the proxy chosen for it is no longer saved. Choose another one, or none.")));
    };
    match &saved.spec {
        ProxySpec::Socks5 { .. } | ProxySpec::Http { .. } => Ok(Some(&saved.spec)),
        ProxySpec::Command { .. } => Err(NotifyError(format!("{label}: the proxy \"{}\" runs a command, which alerts can't use. Choose a SOCKS5 or HTTP proxy.", saved.name))),
    }
}

/// The address reqwest connects to, and what to call it. `socks5h` lets the proxy look the service's name up, so no
/// lookup on this computer shows which service is being reached.
fn proxy_address(spec: &ProxySpec) -> Result<(reqwest::Url, String), String> {
    let (scheme, label, host, port, username, password) = match spec {
        ProxySpec::Socks5 { host, port, username, password } => ("socks5h", "SOCKS5", host, *port, username, password),
        ProxySpec::Http { host, port, username, password } => ("http", "HTTP", host, *port, username, password),
        ProxySpec::Command { .. } => return Err("a proxy that runs a command can't be used".into()),
    };
    let host = host.trim();
    if host.is_empty() || port == 0 {
        return Err(format!("the {label} proxy needs an address and a port"));
    }
    // `set_host` takes a name or an address and nothing else, so a saved host can't carry a path or a login of its own.
    let host_part = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_string() };
    let mut url = reqwest::Url::parse(&format!("{scheme}://127.0.0.1")).map_err(|e| e.to_string())?;
    url.set_host(Some(&host_part)).map_err(|_| format!("the {label} proxy's address \"{host}\" is not valid"))?;
    url.set_port(Some(port)).map_err(|_| format!("the {label} proxy's port is not valid"))?;
    if let Some(user) = username.as_deref().filter(|u| !u.is_empty()) {
        // These are percent-encoded on the way in, so a password may hold any character.
        url.set_username(user).map_err(|_| "the proxy's user name is not valid".to_string())?;
        url.set_password(Some(password.as_deref().unwrap_or(""))).map_err(|_| "the proxy's password is not valid".to_string())?;
    }
    Ok((url, format!("the {label} proxy {host_part}:{port}")))
}

impl BotChannel {
    /// `kind` is `Telegram` or `Bale`; the address comes from the config, else the service's own. `proxies` is where
    /// the config's proxy, if it names one, is looked up.
    pub fn new(kind: ChannelKind, cfg: &BotConfig, proxies: &Proxies) -> Result<Self, NotifyError> {
        let default = match kind {
            ChannelKind::Bale => BALE_BASE,
            _ => TELEGRAM_BASE,
        };
        let base = cfg.base_url.as_deref().map(str::trim).filter(|u| !u.is_empty()).unwrap_or(default).to_string();
        // The token is in the address. Saved settings can come from another device, so check again here.
        if !base_is_safe(&base) {
            return Err(NotifyError(format!("{}: the bot address must start with https://", kind.label())));
        }
        let proxy = proxy_for(kind, cfg, proxies)?;
        // rustls has two providers built in; say which one before the first client is made.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut builder = reqwest::Client::builder()
            // A bot address that redirects is a bot address that is not what was typed in.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("SSHVault/", env!("CARGO_PKG_VERSION")));
        let (mut via, mut proxy_password) = (None, None);
        if let Some(spec) = proxy {
            let (url, name) = proxy_address(spec).map_err(|m| NotifyError(format!("{}: {m}", kind.label())))?;
            // Every request, whatever its address: the proxy is not optional once it is chosen.
            let p = reqwest::Proxy::all(url).map_err(|e| NotifyError(format!("{}: the proxy can't be used ({})", kind.label(), describe(&e))))?;
            builder = builder.proxy(p);
            via = Some(name);
            proxy_password = match spec {
                ProxySpec::Socks5 { password, .. } | ProxySpec::Http { password, .. } => password.clone().filter(|p| !p.is_empty()),
                ProxySpec::Command { .. } => None,
            };
        }
        let client = builder.build().map_err(|e| NotifyError(format!("could not start the {} client: {}", kind.label(), describe(&e))))?;
        Ok(Self { kind, base, token: cfg.bot_token.trim().to_string(), chat_id: cfg.chat_id.trim().to_string(), client, via, proxy_password })
    }

    fn scrub(&self, s: &str) -> String {
        let mut out = s.to_string();
        for secret in std::iter::once(&self.token).chain(self.proxy_password.as_ref()) {
            if !secret.is_empty() {
                out = out.replace(secret.as_str(), "***");
            }
        }
        out
    }

    fn network_error(&self, e: reqwest::Error) -> NotifyError {
        let e = e.without_url();
        let through = self.via.as_deref().map(|v| format!(" through {v}")).unwrap_or_default();
        let what = if e.is_timeout() {
            format!("did not answer in time{through}")
        } else if e.is_connect() {
            format!("could not be reached{through} ({})", describe(&e))
        } else {
            format!("request failed{through} ({})", describe(&e))
        };
        NotifyError(self.scrub(&format!("{} {what}", self.kind.label())))
    }

    async fn post(&self, text: String) -> Result<(), NotifyError> {
        let text: String = text.chars().take(MAX_TEXT).collect();
        let body = serde_json::to_vec(&serde_json::json!({
            "chat_id": self.chat_id,
            "text": text,
            "disable_web_page_preview": true,
        }))
        .map_err(|e| NotifyError(format!("could not write the message: {e}")))?;
        let url = format!("{}/bot{}/sendMessage", self.base.trim_end_matches('/'), self.token);
        let res = self.client.post(url).header("Content-Type", "application/json").body(body).send().await.map_err(|e| self.network_error(e))?;
        let status = res.status();
        let reply = read_capped(res).await;
        let json: Option<serde_json::Value> = serde_json::from_slice(&reply).ok();
        let said = json.as_ref().and_then(|v| v.get("description")).and_then(|d| d.as_str()).map(str::to_string);
        let refused = json.as_ref().and_then(|v| v.get("ok")).and_then(|o| o.as_bool()) == Some(false);
        if status.is_success() && !refused {
            return Ok(());
        }
        let reason = said.unwrap_or_else(|| "no reason given".to_string());
        Err(NotifyError(self.scrub(&format!("{} refused the message: {reason} (HTTP {})", self.kind.label(), status.as_u16()))))
    }
}

impl Channel for BotChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }

    fn send<'a>(&'a self, n: &'a Notification) -> BoxFuture<'a, Result<(), NotifyError>> {
        Box::pin(self.post(n.text()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::http::fake;
    use crate::monitoring::rules::Status;

    fn cfg(base: &str) -> BotConfig {
        BotConfig { enabled: true, bot_token: "123456:SECRET-token".into(), chat_id: "-1001".into(), base_url: Some(base.to_string()), proxy_id: None, token_set: false }
    }

    fn note() -> Notification {
        Notification {
            status: Status::Critical,
            previous: Status::Ok,
            target: "web-01".into(),
            address: None,
            rule: "Ping loss".into(),
            summary: "Ping packet loss is 100% (limit 0%)".into(),
            at: 0,
            note: None,
        }
    }

    #[tokio::test]
    async fn posts_the_message_to_the_bot_api() {
        let server = fake::serve(|_| (200, r#"{"ok":true,"result":{}}"#.to_string())).await;
        let ch = BotChannel::new(ChannelKind::Telegram, &cfg(&format!("http://127.0.0.1:{}", server.port)), &Proxies::new()).unwrap();
        ch.send(&note()).await.unwrap();
        let seen = server.seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].method.as_str(), seen[0].path.as_str()), ("POST", "/bot123456:SECRET-token/sendMessage"));
        let body: serde_json::Value = serde_json::from_str(&seen[0].body).unwrap();
        assert_eq!(body["chat_id"], "-1001");
        assert!(body["text"].as_str().unwrap().contains("CRITICAL: web-01"), "{body}");
    }

    #[tokio::test]
    async fn bale_uses_its_own_address_unless_told_otherwise() {
        let bale = BotChannel::new(ChannelKind::Bale, &BotConfig { base_url: None, ..cfg("") }, &Proxies::new()).unwrap();
        assert_eq!(bale.base, BALE_BASE);
        let tg = BotChannel::new(ChannelKind::Telegram, &BotConfig { base_url: None, ..cfg("") }, &Proxies::new()).unwrap();
        assert_eq!(tg.base, TELEGRAM_BASE);
        assert_eq!(bale.kind(), ChannelKind::Bale);
    }

    #[tokio::test]
    async fn a_refusal_says_why_and_never_shows_the_token() {
        let server = fake::serve(|_| (401, r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#.to_string())).await;
        let ch = BotChannel::new(ChannelKind::Telegram, &cfg(&format!("http://127.0.0.1:{}", server.port)), &Proxies::new()).unwrap();
        let e = ch.send(&note()).await.unwrap_err().0;
        assert!(e.contains("Unauthorized") && e.contains("401"), "{e}");
        assert!(!e.contains("SECRET"), "{e}");
    }

    #[tokio::test]
    async fn an_ok_false_answer_is_a_failure_even_with_status_200() {
        let server = fake::serve(|_| (200, r#"{"ok":false,"description":"chat not found"}"#.to_string())).await;
        let ch = BotChannel::new(ChannelKind::Bale, &cfg(&format!("http://127.0.0.1:{}", server.port)), &Proxies::new()).unwrap();
        assert!(ch.send(&note()).await.unwrap_err().0.contains("chat not found"));
    }

    #[tokio::test]
    async fn an_unreachable_server_fails_without_leaking_the_token() {
        // Nothing listens on port 1.
        let ch = BotChannel::new(ChannelKind::Telegram, &cfg("http://127.0.0.1:1"), &Proxies::new()).unwrap();
        let e = ch.send(&note()).await.unwrap_err().0;
        assert!(e.starts_with("Telegram"), "{e}");
        assert!(!e.contains("SECRET") && !e.contains("123456"), "{e}");
    }

    #[test]
    fn a_plain_http_address_is_refused_unless_it_is_this_computer() {
        for bad in ["http://api.example.org", "http://127.0.0.1.evil.example", "http://localhost.evil.example", "ftp://x", "https://a b"] {
            assert!(BotChannel::new(ChannelKind::Telegram, &cfg(bad), &Proxies::new()).is_err(), "{bad}");
        }
        for good in ["https://tg.example.org", "http://127.0.0.1:8080", "http://localhost", "http://[::1]:9/x"] {
            assert!(BotChannel::new(ChannelKind::Telegram, &cfg(good), &Proxies::new()).is_ok(), "{good}");
        }
    }

    // -- through a proxy ------------------------------------------------------------------------------------------

    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use uuid::Uuid;

    use crate::models::Proxy;

    const PROXY_USER: &str = "bob";
    /// Characters that would break an address if they were pasted into one unescaped.
    const PROXY_PASS: &str = "p@ss:w/rd#é";

    /// For each connection: the login offered (if any) and the destination asked for, as `name:port`.
    type SocksSeen = Vec<(Option<(String, String)>, String)>;

    struct Socks {
        port: u16,
        seen: Arc<Mutex<SocksSeen>>,
    }

    /// A SOCKS5 proxy (RFC 1928/1929). With `login` it insists on that user name and password. With `forward` it
    /// connects through to the destination; without, it answers "connection refused" once it knows where to.
    async fn socks5_proxy(login: Option<(&str, &str)>, forward: bool) -> Socks {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Arc<Mutex<SocksSeen>> = Arc::default();
        let log = Arc::clone(&seen);
        let login = login.map(|(u, p)| (u.to_string(), p.to_string()));
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let (log, login) = (Arc::clone(&log), login.clone());
                tokio::spawn(async move {
                    let _ = socks5_session(&mut s, login, forward, &log).await;
                });
            }
        });
        Socks { port, seen }
    }

    async fn socks5_session(s: &mut TcpStream, login: Option<(String, String)>, forward: bool, log: &Mutex<SocksSeen>) -> std::io::Result<()> {
        let mut head = [0u8; 2];
        s.read_exact(&mut head).await?;
        let mut methods = vec![0u8; head[1] as usize];
        s.read_exact(&mut methods).await?;
        let mut offered = None;
        if let Some((user, pass)) = &login {
            if !methods.contains(&2) {
                return s.write_all(&[5, 0xFF]).await;
            }
            s.write_all(&[5, 2]).await?;
            let mut v = [0u8; 2];
            s.read_exact(&mut v).await?;
            let mut u = vec![0u8; v[1] as usize];
            s.read_exact(&mut u).await?;
            let mut l = [0u8; 1];
            s.read_exact(&mut l).await?;
            let mut p = vec![0u8; l[0] as usize];
            s.read_exact(&mut p).await?;
            let (u, p) = (String::from_utf8_lossy(&u).to_string(), String::from_utf8_lossy(&p).to_string());
            let good = &u == user && &p == pass;
            offered = Some((u, p));
            s.write_all(&[1, if good { 0 } else { 1 }]).await?;
            if !good {
                return Ok(());
            }
        } else {
            s.write_all(&[5, 0]).await?;
        }
        let mut req = [0u8; 4];
        s.read_exact(&mut req).await?;
        let host = match req[3] {
            1 => {
                let mut a = [0u8; 4];
                s.read_exact(&mut a).await?;
                std::net::Ipv4Addr::from(a).to_string()
            }
            3 => {
                let mut n = [0u8; 1];
                s.read_exact(&mut n).await?;
                let mut name = vec![0u8; n[0] as usize];
                s.read_exact(&mut name).await?;
                String::from_utf8_lossy(&name).to_string()
            }
            _ => return Ok(()),
        };
        let mut port = [0u8; 2];
        s.read_exact(&mut port).await?;
        let dest = format!("{host}:{}", u16::from_be_bytes(port));
        log.lock().unwrap().push((offered, dest.clone()));
        if !forward {
            return s.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await;
        }
        let mut out = TcpStream::connect(&dest).await?;
        s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        tokio::io::copy_bidirectional(s, &mut out).await.map(|_| ())
    }

    /// An HTTP proxy that writes down each request's head and answers `reply` to all of them.
    async fn http_proxy(reply: &'static str) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = listener.accept().await else { return };
                let log = Arc::clone(&log);
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        let n = s.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    log.lock().unwrap().push(String::from_utf8_lossy(&buf).to_string());
                    let _ = s.write_all(reply.as_bytes()).await;
                });
            }
        });
        (port, seen)
    }

    fn saved(spec: ProxySpec) -> (Uuid, Proxies) {
        let id = Uuid::new_v4();
        (id, Proxies::from([(id, Proxy { name: "Office".into(), spec })]))
    }

    fn socks_spec(port: u16, password: &str) -> ProxySpec {
        ProxySpec::Socks5 { host: "127.0.0.1".into(), port, username: Some(PROXY_USER.into()), password: Some(password.into()) }
    }

    fn http_spec(port: u16) -> ProxySpec {
        ProxySpec::Http { host: "127.0.0.1".into(), port, username: Some(PROXY_USER.into()), password: Some(PROXY_PASS.into()) }
    }

    fn via(id: Uuid, base: Option<&str>) -> BotConfig {
        BotConfig { proxy_id: Some(id), base_url: base.map(str::to_string), ..cfg("") }
    }

    #[tokio::test]
    async fn a_socks5_proxy_carries_the_message_and_gets_the_login() {
        let bot = fake::serve(|_| (200, r#"{"ok":true,"result":{}}"#.to_string())).await;
        let proxy = socks5_proxy(Some((PROXY_USER, PROXY_PASS)), true).await;
        let (id, proxies) = saved(socks_spec(proxy.port, PROXY_PASS));
        let ch = BotChannel::new(ChannelKind::Telegram, &via(id, Some(&format!("http://127.0.0.1:{}", bot.port))), &proxies).unwrap();
        ch.send(&note()).await.unwrap();
        assert_eq!(bot.seen.lock().unwrap().len(), 1, "the bot server got the message");
        let seen = proxy.seen.lock().unwrap();
        assert_eq!(seen.len(), 1, "and it came through the proxy");
        assert_eq!(seen[0], (Some((PROXY_USER.to_string(), PROXY_PASS.to_string())), format!("127.0.0.1:{}", bot.port)));
    }

    #[tokio::test]
    async fn a_socks5_proxy_looks_the_service_up_itself() {
        let proxy = socks5_proxy(None, false).await;
        let (id, proxies) = saved(ProxySpec::Socks5 { host: "127.0.0.1".into(), port: proxy.port, username: None, password: None });
        let e = BotChannel::new(ChannelKind::Telegram, &via(id, None), &proxies).unwrap().send(&note()).await.unwrap_err().0;
        assert_eq!(proxy.seen.lock().unwrap().as_slice(), &[(None, "api.telegram.org:443".to_string())], "asked for the name, not an address");
        assert!(e.starts_with("Telegram") && e.contains(&format!("the SOCKS5 proxy 127.0.0.1:{}", proxy.port)), "{e}");
        assert!(!e.contains("SECRET") && !e.contains("123456"), "{e}");
    }

    #[tokio::test]
    async fn a_refused_proxy_login_says_so_and_never_shows_the_password() {
        let proxy = socks5_proxy(Some((PROXY_USER, "the-right-one")), true).await;
        let (id, proxies) = saved(socks_spec(proxy.port, "the-wrong-one"));
        let e = BotChannel::new(ChannelKind::Telegram, &via(id, None), &proxies).unwrap().send(&note()).await.unwrap_err().0;
        assert!(e.contains("the SOCKS5 proxy"), "{e}");
        assert!(!e.contains("wrong-one") && !e.contains("SECRET"), "{e}");
    }

    #[tokio::test]
    async fn an_http_proxy_is_asked_to_connect_to_the_service_with_the_login() {
        let (port, seen) = http_proxy("HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        let (id, proxies) = saved(http_spec(port));
        let e = BotChannel::new(ChannelKind::Bale, &via(id, None), &proxies).unwrap().send(&note()).await.unwrap_err().0;
        let head = seen.lock().unwrap()[0].clone();
        assert!(head.starts_with("CONNECT tapi.bale.ai:443 HTTP/1.1"), "{head}");
        let login = base64_of(&format!("{PROXY_USER}:{PROXY_PASS}"));
        assert!(head.to_ascii_lowercase().contains(&format!("proxy-authorization: basic {}", login.to_ascii_lowercase())), "{head}");
        assert!(e.starts_with("Bale") && e.contains(&format!("the HTTP proxy 127.0.0.1:{port}")), "{e}");
        assert!(!e.contains(PROXY_PASS) && !e.contains(&login), "{e}");
    }

    #[tokio::test]
    async fn an_http_proxy_forwards_a_plain_http_bot_address() {
        let (port, seen) = http_proxy("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 11\r\nConnection: close\r\n\r\n{\"ok\":true}").await;
        let (id, proxies) = saved(http_spec(port));
        BotChannel::new(ChannelKind::Telegram, &via(id, Some("http://127.0.0.1:9")), &proxies).unwrap().send(&note()).await.unwrap();
        let head = seen.lock().unwrap()[0].clone();
        assert!(head.starts_with("POST http://127.0.0.1:9/bot123456:SECRET-token/sendMessage HTTP/1.1"), "{head}");
    }

    #[tokio::test]
    async fn a_proxy_that_is_down_never_falls_back_to_a_direct_connection() {
        let bot = fake::serve(|_| (200, r#"{"ok":true}"#.to_string())).await;
        let (id, proxies) = saved(ProxySpec::Socks5 { host: "127.0.0.1".into(), port: 1, username: None, password: None });
        let ch = BotChannel::new(ChannelKind::Telegram, &via(id, Some(&format!("http://127.0.0.1:{}", bot.port))), &proxies).unwrap();
        let e = ch.send(&note()).await.unwrap_err().0;
        assert!(e.contains("the SOCKS5 proxy 127.0.0.1:1"), "{e}");
        assert!(bot.seen.lock().unwrap().is_empty(), "the bot server, which could be reached directly, got nothing");
    }

    #[test]
    fn a_proxy_that_is_gone_or_cannot_be_used_stops_the_channel() {
        let missing = BotChannel::new(ChannelKind::Telegram, &via(Uuid::new_v4(), None), &Proxies::new()).err().unwrap().0;
        assert!(missing.starts_with("Telegram") && missing.contains("no longer saved"), "{missing}");
        let (id, proxies) = saved(ProxySpec::Command { command: "nc %h %p".into(), approved: true });
        let command = BotChannel::new(ChannelKind::Bale, &via(id, None), &proxies).err().unwrap().0;
        assert!(command.starts_with("Bale") && command.contains("\"Office\"") && command.contains("SOCKS5 or HTTP"), "{command}");
        let (id, proxies) = saved(ProxySpec::Http { host: "  ".into(), port: 8080, username: None, password: None });
        assert!(BotChannel::new(ChannelKind::Telegram, &via(id, None), &proxies).err().unwrap().0.contains("needs an address"));
        // No proxy chosen is not an error, whatever else is saved.
        assert!(BotChannel::new(ChannelKind::Telegram, &cfg("https://x.example"), &proxies).is_ok());
    }

    #[test]
    fn a_proxy_address_is_a_name_and_a_port_and_nothing_else() {
        let url = |host: &str, port: u16| proxy_address(&ProxySpec::Socks5 { host: host.into(), port, username: None, password: None });
        assert_eq!(url("proxy.example", 1080).unwrap().0.as_str(), "socks5h://proxy.example:1080");
        assert_eq!(url("::1", 1080).unwrap().0.host_str(), Some("[::1]"));
        assert_eq!(url("[::1]", 1080).unwrap().0.host_str(), Some("[::1]"));
        assert_eq!(url(" 10.0.0.2 ", 1080).unwrap().1, "the SOCKS5 proxy 10.0.0.2:1080");
        for bad in ["", "evil.example/x@y", "a b", "user:pw@host", "h?x"] {
            assert!(url(bad, 1080).is_err(), "{bad:?}");
        }
        assert!(url("proxy.example", 0).is_err());
    }

    fn base64_of(s: &str) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    #[test]
    fn scrubbing_removes_the_token_wherever_it_appears() {
        let ch = BotChannel::new(ChannelKind::Telegram, &cfg("https://x.example"), &Proxies::new()).unwrap();
        assert_eq!(ch.scrub("bad url https://x.example/bot123456:SECRET-token/sendMessage"), "bad url https://x.example/bot***/sendMessage");
    }
}
