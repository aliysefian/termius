//! Telegram and Bale. Bale's bot API is Telegram's with another address, so one channel serves both.
//!
//! The token is part of the request address (`/bot<token>/sendMessage`), and an HTTP library's error text includes
//! the address. So every error is stripped of its address first and then of the token itself, and the tests check
//! that no failure ever shows it.

use std::time::Duration;

use super::{BoxFuture, Channel, Notification, NotifyError};
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

impl BotChannel {
    /// `kind` is `Telegram` or `Bale`; the address comes from the config, else the service's own.
    pub fn new(kind: ChannelKind, cfg: &BotConfig) -> Result<Self, NotifyError> {
        let default = match kind {
            ChannelKind::Bale => BALE_BASE,
            _ => TELEGRAM_BASE,
        };
        let base = cfg.base_url.as_deref().map(str::trim).filter(|u| !u.is_empty()).unwrap_or(default).to_string();
        // The token is in the address. Saved settings can come from another device, so check again here.
        if !base_is_safe(&base) {
            return Err(NotifyError(format!("{}: the bot address must start with https://", kind.label())));
        }
        // rustls has two providers built in; say which one before the first client is made.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let client = reqwest::Client::builder()
            // A bot address that redirects is a bot address that is not what was typed in.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(20))
            .user_agent(concat!("SSHVault/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| NotifyError(format!("could not start the {} client: {}", kind.label(), describe(&e))))?;
        Ok(Self { kind, base, token: cfg.bot_token.trim().to_string(), chat_id: cfg.chat_id.trim().to_string(), client })
    }

    fn scrub(&self, s: &str) -> String {
        if self.token.is_empty() {
            s.to_string()
        } else {
            s.replace(&self.token, "***")
        }
    }

    fn network_error(&self, e: reqwest::Error) -> NotifyError {
        let e = e.without_url();
        let what = if e.is_timeout() {
            "did not answer in time".to_string()
        } else if e.is_connect() {
            format!("could not be reached ({})", describe(&e))
        } else {
            format!("request failed ({})", describe(&e))
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
        BotConfig { enabled: true, bot_token: "123456:SECRET-token".into(), chat_id: "-1001".into(), base_url: Some(base.to_string()), token_set: false }
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
        let ch = BotChannel::new(ChannelKind::Telegram, &cfg(&format!("http://127.0.0.1:{}", server.port))).unwrap();
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
        let bale = BotChannel::new(ChannelKind::Bale, &BotConfig { base_url: None, ..cfg("") }).unwrap();
        assert_eq!(bale.base, BALE_BASE);
        let tg = BotChannel::new(ChannelKind::Telegram, &BotConfig { base_url: None, ..cfg("") }).unwrap();
        assert_eq!(tg.base, TELEGRAM_BASE);
        assert_eq!(bale.kind(), ChannelKind::Bale);
    }

    #[tokio::test]
    async fn a_refusal_says_why_and_never_shows_the_token() {
        let server = fake::serve(|_| (401, r#"{"ok":false,"error_code":401,"description":"Unauthorized"}"#.to_string())).await;
        let ch = BotChannel::new(ChannelKind::Telegram, &cfg(&format!("http://127.0.0.1:{}", server.port))).unwrap();
        let e = ch.send(&note()).await.unwrap_err().0;
        assert!(e.contains("Unauthorized") && e.contains("401"), "{e}");
        assert!(!e.contains("SECRET"), "{e}");
    }

    #[tokio::test]
    async fn an_ok_false_answer_is_a_failure_even_with_status_200() {
        let server = fake::serve(|_| (200, r#"{"ok":false,"description":"chat not found"}"#.to_string())).await;
        let ch = BotChannel::new(ChannelKind::Bale, &cfg(&format!("http://127.0.0.1:{}", server.port))).unwrap();
        assert!(ch.send(&note()).await.unwrap_err().0.contains("chat not found"));
    }

    #[tokio::test]
    async fn an_unreachable_server_fails_without_leaking_the_token() {
        // Nothing listens on port 1.
        let ch = BotChannel::new(ChannelKind::Telegram, &cfg("http://127.0.0.1:1")).unwrap();
        let e = ch.send(&note()).await.unwrap_err().0;
        assert!(e.starts_with("Telegram"), "{e}");
        assert!(!e.contains("SECRET") && !e.contains("123456"), "{e}");
    }

    #[test]
    fn a_plain_http_address_is_refused_unless_it_is_this_computer() {
        for bad in ["http://api.example.org", "http://127.0.0.1.evil.example", "http://localhost.evil.example", "ftp://x", "https://a b"] {
            assert!(BotChannel::new(ChannelKind::Telegram, &cfg(bad)).is_err(), "{bad}");
        }
        for good in ["https://tg.example.org", "http://127.0.0.1:8080", "http://localhost", "http://[::1]:9/x"] {
            assert!(BotChannel::new(ChannelKind::Telegram, &cfg(good)).is_ok(), "{good}");
        }
    }

    #[test]
    fn scrubbing_removes_the_token_wherever_it_appears() {
        let ch = BotChannel::new(ChannelKind::Telegram, &cfg("https://x.example")).unwrap();
        assert_eq!(ch.scrub("bad url https://x.example/bot123456:SECRET-token/sendMessage"), "bad url https://x.example/bot***/sendMessage");
    }
}
