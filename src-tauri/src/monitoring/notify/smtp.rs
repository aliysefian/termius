//! Email over SMTP, written against the TLS the app already carries (rustls with the system's trust store) so that
//! no mail library has to be added to the build.
//!
//! Three ways to connect: TLS from the first byte (port 465), a plain connection upgraded with STARTTLS (587), and no
//! encryption at all, which is allowed only when the server is this computer. The password is sent with AUTH PLAIN
//! or AUTH LOGIN and only after the connection is encrypted; a server that does not offer STARTTLS is an error, never
//! a reason to carry on in the clear. The message is UTF-8, base64 encoded, with the subject in an RFC 2047 word.

use std::net::IpAddr;
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;

use super::{civil_from_days, BoxFuture, Channel, Notification, NotifyError};
use crate::monitoring::model::{valid_email, ChannelKind, EmailConfig, SmtpSecurity};

const STEP: Duration = Duration::from_secs(30);
const CONNECT: Duration = Duration::from_secs(15);
const MAX_REPLY: usize = 64 * 1024;
const MAX_REPLY_LINES: usize = 200;

trait Wire: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Wire for T {}

struct Reply {
    code: u16,
    lines: Vec<String>,
}

struct Conn {
    io: Box<dyn Wire>,
    buf: Vec<u8>,
}

impl Conn {
    fn new(io: Box<dyn Wire>) -> Self {
        Self { io, buf: Vec::new() }
    }

    async fn read_reply(&mut self) -> Result<Reply, String> {
        let mut lines = Vec::new();
        loop {
            while let Some(pos) = self.buf.iter().position(|b| *b == b'\n') {
                let raw: Vec<u8> = self.buf.drain(..=pos).collect();
                let line = String::from_utf8_lossy(&raw).trim_end().to_string();
                let code: u16 = line.get(..3).and_then(|c| c.parse().ok()).ok_or_else(|| "the server sent something that is not an SMTP reply".to_string())?;
                lines.push(line.get(4..).unwrap_or_default().to_string());
                if lines.len() > MAX_REPLY_LINES {
                    return Err("the server's reply was too long".into());
                }
                if line.as_bytes().get(3) != Some(&b'-') {
                    return Ok(Reply { code, lines });
                }
            }
            let mut chunk = [0u8; 2048];
            let n = match tokio::time::timeout(STEP, self.io.read(&mut chunk)).await {
                Err(_) => return Err("the server stopped answering".into()),
                Ok(Err(e)) => return Err(format!("the connection failed: {e}")),
                Ok(Ok(0)) => return Err("the server closed the connection".into()),
                Ok(Ok(n)) => n,
            };
            self.buf.extend_from_slice(&chunk[..n]);
            if self.buf.len() > MAX_REPLY {
                return Err("the server's reply was too long".into());
            }
        }
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<(), String> {
        match tokio::time::timeout(STEP, async {
            self.io.write_all(bytes).await?;
            self.io.flush().await
        })
        .await
        {
            Err(_) => Err("the server stopped accepting data".into()),
            Ok(Err(e)) => Err(format!("the connection failed: {e}")),
            Ok(Ok(())) => Ok(()),
        }
    }

    /// Send a command and require one of `expect`. The error carries the server's words, never what was sent: an
    /// AUTH line holds the password.
    async fn command(&mut self, line: &str, expect: &[u16]) -> Result<Reply, String> {
        self.write(format!("{line}\r\n").as_bytes()).await?;
        let reply = self.read_reply().await?;
        if expect.contains(&reply.code) {
            Ok(reply)
        } else {
            Err(format!("the server answered {} {}", reply.code, reply.lines.first().map(String::as_str).unwrap_or_default()))
        }
    }

    /// The server's capabilities, upper case.
    async fn ehlo(&mut self) -> Result<Vec<String>, String> {
        let r = self.command("EHLO localhost", &[250]).await?;
        Ok(r.lines.iter().skip(1).map(|l| l.to_ascii_uppercase()).collect())
    }
}

async fn tls_upgrade(io: Box<dyn Wire>, host: &str) -> Result<Box<dyn Wire>, String> {
    let config = crate::db::tlsconf::client_config(crate::db::TlsMode::VerifyFull).map_err(|e| e.to_string())?.ok_or_else(|| "TLS is not available".to_string())?;
    let name = rustls::pki_types::ServerName::try_from(host.to_string()).map_err(|_| format!("\"{host}\" can't be checked as a server name"))?;
    let tls = match tokio::time::timeout(STEP, tokio_rustls::TlsConnector::from(config).connect(name, io)).await {
        Err(_) => return Err("the secure connection took too long to start".into()),
        Ok(Err(e)) => return Err(format!("the secure connection failed: {e}")),
        Ok(Ok(tls)) => tls,
    };
    Ok(Box::new(tls))
}

fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.trim_matches(['[', ']']).parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

pub struct Mail {
    pub from: String,
    pub to: Vec<String>,
    /// The whole message, headers and body, with CRLF line ends.
    pub message: String,
}

/// Send one message. Every step has a time limit; nothing is logged.
pub async fn deliver(cfg: &EmailConfig, mail: &Mail) -> Result<(), String> {
    let host = cfg.server.trim();
    let wants_login = !cfg.username.is_empty();
    if cfg.security == SmtpSecurity::Plain && wants_login && !is_loopback(host) {
        return Err("refusing to send the password without encryption: choose TLS or STARTTLS".into());
    }
    if !valid_email(&mail.from) || mail.to.is_empty() || mail.to.iter().any(|a| !valid_email(a)) {
        return Err("the sender or a recipient is not a valid address".into());
    }
    let tcp = match tokio::time::timeout(CONNECT, TcpStream::connect((host.trim_matches(['[', ']']), cfg.port))).await {
        Err(_) => return Err(format!("{host}:{} did not answer in time", cfg.port)),
        Ok(Err(e)) => return Err(crate::neterr::connect_text(&format!("{host}:{}", cfg.port), &e)),
        Ok(Ok(s)) => s,
    };
    let _ = tcp.set_nodelay(true);
    let mut io: Box<dyn Wire> = Box::new(tcp);
    if cfg.security == SmtpSecurity::Tls {
        io = tls_upgrade(io, host).await?;
    }
    let mut c = Conn::new(io);
    let greeting = c.read_reply().await?;
    if greeting.code != 220 {
        return Err(format!("the server did not accept the connection ({} {})", greeting.code, greeting.lines.first().map(String::as_str).unwrap_or_default()));
    }
    let mut caps = c.ehlo().await?;
    if cfg.security == SmtpSecurity::StartTls {
        if !caps.iter().any(|l| l == "STARTTLS") {
            return Err("the server does not offer STARTTLS; pick TLS, or another server".into());
        }
        c.command("STARTTLS", &[220]).await?;
        let Conn { io, buf } = c;
        // Bytes that arrived before the handshake were not sent over TLS. Do not trust them.
        if !buf.is_empty() {
            return Err("the server sent data before the secure connection started".into());
        }
        c = Conn::new(tls_upgrade(io, host).await?);
        caps = c.ehlo().await?;
    }
    if wants_login {
        let methods = caps.iter().find(|l| l.starts_with("AUTH")).cloned().unwrap_or_default();
        let has = |m: &str| methods.split([' ', '=']).any(|w| w == m);
        if has("PLAIN") {
            let token = B64.encode(format!("\0{}\0{}", cfg.username, cfg.password));
            c.command(&format!("AUTH PLAIN {token}"), &[235]).await?;
        } else if has("LOGIN") {
            c.command("AUTH LOGIN", &[334]).await?;
            c.command(&B64.encode(cfg.username.as_bytes()), &[334]).await?;
            c.command(&B64.encode(cfg.password.as_bytes()), &[235]).await?;
        } else {
            return Err("the server offers no sign-in method this app can use (it needs PLAIN or LOGIN)".into());
        }
    }
    c.command(&format!("MAIL FROM:<{}>", mail.from), &[250]).await?;
    for to in &mail.to {
        c.command(&format!("RCPT TO:<{to}>"), &[250, 251]).await?;
    }
    c.command("DATA", &[354]).await?;
    c.write(dot_stuff(&mail.message).as_bytes()).await?;
    let sent = c.read_reply().await?;
    if sent.code != 250 {
        return Err(format!("the server did not accept the message ({} {})", sent.code, sent.lines.first().map(String::as_str).unwrap_or_default()));
    }
    // Politeness only: the message is already accepted.
    let _ = c.write(b"QUIT\r\n").await;
    Ok(())
}

/// The message as sent after DATA: a line that starts with a dot gets another, and a lone dot ends it.
pub fn dot_stuff(message: &str) -> String {
    let mut out = String::with_capacity(message.len() + 8);
    for line in message.trim_end_matches("\r\n").split("\r\n") {
        if line.starts_with('.') {
            out.push('.');
        }
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(".\r\n");
    out
}

/// A header value on one line.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect::<String>().trim().to_string()
}

/// A subject as RFC 2047 words when it is not plain ASCII (or is long), each word short enough to be a legal line.
fn encode_subject(subject: &str) -> String {
    let s = one_line(subject);
    if s.is_ascii() && s.len() <= 70 {
        return s;
    }
    let mut words: Vec<String> = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if cur.len() + c.len_utf8() > 45 {
            words.push(std::mem::take(&mut cur));
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words.iter().map(|w| format!("=?UTF-8?B?{}?=", B64.encode(w))).collect::<Vec<_>>().join("\r\n ")
}

/// `Wed, 28 Oct 2026 14:03:07 +0000`.
fn rfc2822(secs: u64) -> String {
    const DAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let days = (secs / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    let rem = secs % 86_400;
    // 1970-01-01 was a Thursday.
    format!("{}, {d:02} {} {y} {:02}:{:02}:{:02} +0000", DAYS[((days + 4).rem_euclid(7)) as usize], MONTHS[(m - 1) as usize], rem / 3600, rem % 3600 / 60, rem % 60)
}

pub fn build_message(from: &str, to: &[String], subject: &str, body: &str, at: u64, id: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("From: {}\r\n", one_line(from)));
    out.push_str(&format!("To: {}\r\n", to.iter().map(|a| one_line(a)).collect::<Vec<_>>().join(", ")));
    out.push_str(&format!("Subject: {}\r\n", encode_subject(subject)));
    out.push_str(&format!("Date: {}\r\n", rfc2822(at)));
    out.push_str(&format!("Message-ID: <{id}@sshvault.local>\r\n"));
    out.push_str("MIME-Version: 1.0\r\nAuto-Submitted: auto-generated\r\nContent-Type: text/plain; charset=UTF-8\r\nContent-Transfer-Encoding: base64\r\n\r\n");
    let encoded = B64.encode(body.as_bytes());
    for chunk in encoded.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\r\n");
    }
    out
}

pub struct SmtpChannel {
    cfg: EmailConfig,
    from: String,
    to: Vec<String>,
}

impl SmtpChannel {
    pub fn new(cfg: &EmailConfig) -> Result<Self, NotifyError> {
        let from = cfg.sender().filter(|f| valid_email(f)).ok_or_else(|| NotifyError("Email: the sender address is missing or not valid.".into()))?;
        let to = cfg.recipients();
        if to.is_empty() || to.iter().any(|a| !valid_email(a)) {
            return Err(NotifyError("Email: the address to send alerts to is missing or not valid.".into()));
        }
        Ok(Self { cfg: cfg.clone(), from, to })
    }
}

impl Channel for SmtpChannel {
    fn kind(&self) -> ChannelKind {
        ChannelKind::Email
    }

    fn send<'a>(&'a self, n: &'a Notification) -> BoxFuture<'a, Result<(), NotifyError>> {
        Box::pin(async move {
            let id = uuid::Uuid::new_v4().simple().to_string();
            let message = build_message(&self.from, &self.to, &n.subject(), &n.text(), n.at, &id);
            deliver(&self.cfg, &Mail { from: self.from.clone(), to: self.to.clone(), message }).await.map_err(|e| NotifyError(format!("Email: {e}")))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::rules::Status;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::TcpListener;

    #[test]
    fn dots_are_stuffed_and_the_end_is_marked() {
        assert_eq!(dot_stuff("a\r\n.b\r\n..c\r\n"), "a\r\n..b\r\n...c\r\n.\r\n");
        assert_eq!(dot_stuff("one"), "one\r\n.\r\n");
    }

    #[test]
    fn subjects_are_one_line_and_encoded_when_they_need_to_be() {
        assert_eq!(encode_subject("[CRITICAL] web-01: Ping loss"), "[CRITICAL] web-01: Ping loss");
        assert_eq!(encode_subject("a\r\nBcc: evil@example.com"), "a  Bcc: evil@example.com", "no header can be smuggled in");
        let fa = encode_subject("\u{647}\u{634}\u{62F}\u{627}\u{631}: \u{633}\u{631}\u{648}\u{631} \u{62E}\u{627}\u{645}\u{648}\u{634} \u{627}\u{633}\u{62A} \u{648} \u{62F}\u{633}\u{62A}\u{631}\u{633}\u{6CC} \u{646}\u{62F}\u{627}\u{631}\u{62F}");
        assert!(fa.starts_with("=?UTF-8?B?") && fa.contains("\r\n =?UTF-8?B?"), "{fa}");
        for word in fa.split("\r\n ") {
            assert!(word.len() <= 75, "{word}");
            let inner = word.strip_prefix("=?UTF-8?B?").unwrap().strip_suffix("?=").unwrap();
            assert!(String::from_utf8(B64.decode(inner).unwrap()).is_ok());
        }
    }

    #[test]
    fn dates_in_mail_format() {
        assert_eq!(rfc2822(0), "Thu, 01 Jan 1970 00:00:00 +0000");
        assert_eq!(rfc2822(1_793_196_187), "Wed, 28 Oct 2026 14:03:07 +0000");
    }

    #[test]
    fn the_message_has_the_headers_a_mail_client_needs_and_a_wrapped_body() {
        let body = "x".repeat(200);
        let m = build_message("a@example.com", &["b@example.com".to_string(), "c@example.com".to_string()], "Hi", &body, 0, "abc");
        assert!(m.starts_with("From: a@example.com\r\nTo: b@example.com, c@example.com\r\nSubject: Hi\r\n"), "{m}");
        assert!(m.contains("Message-ID: <abc@sshvault.local>\r\n") && m.contains("Content-Transfer-Encoding: base64\r\n\r\n"));
        let (_, encoded) = m.split_once("\r\n\r\n").unwrap();
        assert!(encoded.lines().all(|l| l.len() <= 76));
        let joined: String = encoded.lines().collect();
        assert_eq!(B64.decode(joined).unwrap(), body.as_bytes());
    }

    // -- a scripted server on loopback -------------------------------------------------------------------------

    struct Script {
        starttls: bool,
        reject_auth: bool,
    }

    async fn serve(script: Script) -> (u16, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let log: Arc<Mutex<Vec<String>>> = Arc::default();
        let seen = Arc::clone(&log);
        tokio::spawn(async move {
            let Ok((s, _)) = listener.accept().await else { return };
            let (r, mut w) = s.into_split();
            let mut r = BufReader::new(r);
            let _ = w.write_all(b"220 test ESMTP\r\n").await;
            let mut in_data = false;
            let mut line = String::new();
            loop {
                line.clear();
                if r.read_line(&mut line).await.unwrap_or(0) == 0 {
                    break;
                }
                let l = line.trim_end().to_string();
                seen.lock().unwrap().push(l.clone());
                if in_data {
                    if l == "." {
                        in_data = false;
                        let _ = w.write_all(b"250 queued\r\n").await;
                    }
                    continue;
                }
                let up = l.to_ascii_uppercase();
                let reply: &[u8] = if up.starts_with("EHLO") {
                    if script.starttls {
                        b"250-test\r\n250-STARTTLS\r\n250-AUTH PLAIN LOGIN\r\n250 SIZE 1000\r\n"
                    } else {
                        b"250-test\r\n250-AUTH PLAIN LOGIN\r\n250 SIZE 1000\r\n"
                    }
                } else if up.starts_with("AUTH PLAIN") {
                    if script.reject_auth {
                        b"535 5.7.8 bad credentials\r\n"
                    } else {
                        b"235 ok\r\n"
                    }
                } else if up.starts_with("MAIL FROM") || up.starts_with("RCPT TO") {
                    b"250 ok\r\n"
                } else if up == "DATA" {
                    in_data = true;
                    b"354 go ahead\r\n"
                } else if up == "QUIT" {
                    let _ = w.write_all(b"221 bye\r\n").await;
                    break;
                } else {
                    b"500 what?\r\n"
                };
                let _ = w.write_all(reply).await;
            }
        });
        (port, log)
    }

    fn cfg(port: u16) -> EmailConfig {
        EmailConfig {
            enabled: true,
            server: "127.0.0.1".into(),
            port,
            security: SmtpSecurity::Plain,
            username: "alerts@example.com".into(),
            password: "s3cret-pass".into(),
            from: String::new(),
            to: "ops@example.com, oncall@example.com".into(),
            password_set: false,
        }
    }

    fn note() -> Notification {
        Notification {
            status: Status::Critical,
            previous: Status::Ok,
            target: "web-01".into(),
            address: None,
            rule: "Ping loss".into(),
            summary: "Ping packet loss is 100% (limit 0%)".into(),
            at: 1_793_196_187,
            note: None,
        }
    }

    #[tokio::test]
    async fn sends_a_message_after_signing_in() {
        let (port, log) = serve(Script { starttls: false, reject_auth: false }).await;
        let ch = SmtpChannel::new(&cfg(port)).unwrap();
        ch.send(&note()).await.unwrap();
        let lines = log.lock().unwrap().clone();
        let plain = B64.encode("\0alerts@example.com\0s3cret-pass");
        assert!(lines.contains(&format!("AUTH PLAIN {plain}")), "{lines:?}");
        assert!(lines.contains(&"MAIL FROM:<alerts@example.com>".to_string()));
        assert!(lines.contains(&"RCPT TO:<ops@example.com>".to_string()) && lines.contains(&"RCPT TO:<oncall@example.com>".to_string()));
        let start = lines.iter().position(|l| l == "DATA").unwrap() + 1;
        let end = lines.iter().rposition(|l| l == ".").unwrap();
        let data = lines[start..end].join("\r\n");
        assert!(data.contains("Subject: [CRITICAL] web-01: Ping loss"), "{data}");
        let (_, body) = data.split_once("\r\n\r\n").unwrap();
        let text = String::from_utf8(B64.decode(body.replace("\r\n", "")).unwrap()).unwrap();
        assert!(text.contains("CRITICAL: web-01") && text.contains("Ping packet loss is 100%"), "{text}");
    }

    #[tokio::test]
    async fn a_refused_password_is_reported_without_repeating_it() {
        let (port, _log) = serve(Script { starttls: false, reject_auth: true }).await;
        let e = SmtpChannel::new(&cfg(port)).unwrap().send(&note()).await.unwrap_err().0;
        assert!(e.contains("535"), "{e}");
        assert!(!e.contains("s3cret-pass") && !e.contains(&B64.encode("\0alerts@example.com\0s3cret-pass")), "{e}");
    }

    #[tokio::test]
    async fn starttls_is_required_not_optional() {
        // The server does not offer it: the password must not be sent.
        let (port, log) = serve(Script { starttls: false, reject_auth: false }).await;
        let mut c = cfg(port);
        c.security = SmtpSecurity::StartTls;
        let e = SmtpChannel::new(&c).unwrap().send(&note()).await.unwrap_err().0;
        assert!(e.contains("STARTTLS"), "{e}");
        assert!(log.lock().unwrap().iter().all(|l| !l.to_ascii_uppercase().starts_with("AUTH")), "{:?}", log.lock().unwrap());
    }

    #[tokio::test]
    async fn a_password_never_crosses_the_network_unencrypted() {
        let mut c = cfg(1);
        c.server = "smtp.example.com".into();
        let e = deliver(&c, &Mail { from: "a@example.com".into(), to: vec!["b@example.com".into()], message: "x".into() }).await.unwrap_err();
        assert!(e.contains("without encryption"), "{e}");
    }

    #[test]
    fn loopback_is_recognised() {
        for h in ["localhost", "127.0.0.1", "::1", "[::1]"] {
            assert!(is_loopback(h), "{h}");
        }
        for h in ["smtp.example.com", "10.0.0.1", "localhost.evil.example"] {
            assert!(!is_loopback(h), "{h}");
        }
    }

    #[test]
    fn bad_addresses_are_refused_before_anything_is_sent() {
        let mut c = cfg(25);
        c.to = "ops@example.com\r\nRCPT TO:<x@y.z>".into();
        assert!(SmtpChannel::new(&c).is_err());
        c.to = "ops@example.com".into();
        c.from = "nobody".into();
        assert!(SmtpChannel::new(&c).is_err());
    }
}
