//! Opening the byte stream to a host's first hop: plain TCP, a SOCKS5 or
//! HTTP CONNECT proxy, or an OpenSSH-style `ProxyCommand`.
//!
//! Proxies resolve the destination name themselves (SOCKS5 "domain name"
//! addressing, HTTP `CONNECT host:port`), so no local DNS lookup reveals
//! which hosts are being reached.

use std::pin::Pin;
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;

use crate::models::ProxySpec;
use crate::ssh::{SshError, Target};

pub trait Stream: AsyncRead + AsyncWrite + Unpin + Send + 'static {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send + 'static> Stream for T {}
pub type BoxStream = Box<dyn Stream>;

/// Upper bound on an HTTP proxy's response headers.
const MAX_HTTP_HEADER: usize = 16 * 1024;

pub async fn dial(target: &Target) -> Result<BoxStream, SshError> {
    let (host, port) = (target.hostname.as_str(), target.port);
    match &target.proxy {
        None => Ok(Box::new(tcp(host, port).await?)),
        Some(ProxySpec::Socks5 {
            host: ph,
            port: pp,
            username,
            password,
        }) => {
            let name = format!("socks5://{ph}:{pp}");
            let mut s = tcp(ph, *pp).await?;
            let creds = username.as_deref().map(|u| (u, password.as_deref().unwrap_or("")));
            socks5_connect(&mut s, host, port, creds)
                .await
                .map_err(|reason| SshError::Proxy { proxy: name, reason })?;
            Ok(Box::new(s))
        }
        Some(ProxySpec::Http {
            host: ph,
            port: pp,
            username,
            password,
        }) => {
            let name = format!("http://{ph}:{pp}");
            let mut s = tcp(ph, *pp).await?;
            let creds = username.as_deref().map(|u| (u, password.as_deref().unwrap_or("")));
            http_connect(&mut s, host, port, creds)
                .await
                .map_err(|reason| SshError::Proxy { proxy: name, reason })?;
            Ok(Box::new(s))
        }
        Some(ProxySpec::Command { command, approved }) => {
            if !approved {
                return Err(SshError::Proxy {
                    proxy: "ProxyCommand".into(),
                    reason: "this command hasn't been approved to run on this computer; review it in Proxies".into(),
                });
            }
            let cmd = expand_proxy_command(command, host, port, &target.username);
            spawn_command(&cmd).map_err(|e| SshError::Proxy {
                proxy: "ProxyCommand".into(),
                reason: e.to_string(),
            })
        }
    }
}

async fn tcp(host: &str, port: u16) -> Result<TcpStream, SshError> {
    let s = TcpStream::connect((host, port))
        .await
        .map_err(|source| SshError::Connect {
            addr: format!("{host}:{port}"),
            source,
        })?;
    let _ = s.set_nodelay(true);
    Ok(s)
}

fn io(e: std::io::Error) -> String {
    e.to_string()
}

// ---------------------------------------------------------------------------
// SOCKS5 (RFC 1928, username/password auth RFC 1929)
// ---------------------------------------------------------------------------

pub async fn socks5_connect<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
    host: &str,
    port: u16,
    creds: Option<(&str, &str)>,
) -> Result<(), String> {
    let methods: &[u8] = if creds.is_some() { &[0x00, 0x02] } else { &[0x00] };
    let mut hello = vec![5, methods.len() as u8];
    hello.extend_from_slice(methods);
    s.write_all(&hello).await.map_err(io)?;
    let mut choice = [0u8; 2];
    s.read_exact(&mut choice).await.map_err(io)?;
    if choice[0] != 5 {
        return Err("not a SOCKS5 proxy".into());
    }
    match (choice[1], creds) {
        (0x00, _) => {}
        (0x02, Some((user, pass))) => {
            if user.len() > 255 || pass.len() > 255 {
                return Err("proxy username or password is too long".into());
            }
            let mut auth = vec![1, user.len() as u8];
            auth.extend_from_slice(user.as_bytes());
            auth.push(pass.len() as u8);
            auth.extend_from_slice(pass.as_bytes());
            s.write_all(&auth).await.map_err(io)?;
            let mut status = [0u8; 2];
            s.read_exact(&mut status).await.map_err(io)?;
            if status[1] != 0 {
                return Err("the proxy rejected the username or password".into());
            }
        }
        (0x02, None) => return Err("the proxy requires a username and password".into()),
        _ => return Err("the proxy accepts none of our authentication methods".into()),
    }

    let mut req = vec![5, 1, 0];
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => {
            req.push(1);
            req.extend_from_slice(&ip.octets());
        }
        Ok(std::net::IpAddr::V6(ip)) => {
            req.push(4);
            req.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            if host.is_empty() || host.len() > 255 {
                return Err("host name is too long for SOCKS5".into());
            }
            req.push(3);
            req.push(host.len() as u8);
            req.extend_from_slice(host.as_bytes());
        }
    }
    req.extend_from_slice(&port.to_be_bytes());
    s.write_all(&req).await.map_err(io)?;

    let mut head = [0u8; 4];
    s.read_exact(&mut head).await.map_err(io)?;
    if head[1] != 0 {
        return Err(match head[1] {
            1 => "general proxy failure",
            2 => "connection not allowed by the proxy's rules",
            3 => "network unreachable",
            4 => "host unreachable",
            5 => "connection refused",
            6 => "TTL expired",
            7 => "command not supported",
            8 => "address type not supported",
            _ => "unknown proxy error",
        }
        .to_string());
    }
    // Skip the bound address the proxy reports.
    let skip = match head[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut len = [0u8; 1];
            s.read_exact(&mut len).await.map_err(io)?;
            usize::from(len[0])
        }
        _ => return Err("malformed proxy reply".into()),
    };
    let mut rest = vec![0u8; skip + 2];
    s.read_exact(&mut rest).await.map_err(io)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// HTTP CONNECT
// ---------------------------------------------------------------------------

pub async fn http_connect<S: AsyncRead + AsyncWrite + Unpin>(
    s: &mut S,
    host: &str,
    port: u16,
    creds: Option<(&str, &str)>,
) -> Result<(), String> {
    if host.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("invalid host name".into());
    }
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut req = format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n");
    if let Some((user, pass)) = creds {
        use base64::Engine;
        let token = zeroize::Zeroizing::new(
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}")),
        );
        req.push_str(&format!("Proxy-Authorization: Basic {}\r\n", token.as_str()));
    }
    req.push_str("\r\n");
    let req = zeroize::Zeroizing::new(req);
    s.write_all(req.as_bytes()).await.map_err(io)?;

    // Read one byte at a time so nothing past the headers (the SSH banner)
    // is consumed.
    let mut head = Vec::with_capacity(256);
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= MAX_HTTP_HEADER {
            return Err("proxy response headers are too long".into());
        }
        s.read_exact(&mut b).await.map_err(io)?;
        head.push(b[0]);
    }
    let text = String::from_utf8_lossy(&head);
    let status_line = text.lines().next().unwrap_or_default();
    let code = status_line.split_whitespace().nth(1).unwrap_or_default();
    if !status_line.starts_with("HTTP/1.") {
        return Err("not an HTTP proxy".into());
    }
    match code {
        "200" => Ok(()),
        "407" => Err("the proxy requires authentication (407)".into()),
        _ => Err(format!("the proxy refused the connection: {}", status_line.trim())),
    }
}

// ---------------------------------------------------------------------------
// ProxyCommand
// ---------------------------------------------------------------------------

/// OpenSSH tokens: `%h` host, `%p` port, `%r` user, `%%` a literal `%`.
pub fn expand_proxy_command(cmd: &str, host: &str, port: u16, user: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut chars = cmd.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(host),
            Some('p') => out.push_str(&port.to_string()),
            Some('r') => out.push_str(user),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// The command's stdin/stdout as one stream. The child is killed when the
/// stream is dropped.
struct CommandStream {
    _child: tokio::process::Child,
    io: tokio::io::Join<tokio::process::ChildStdout, tokio::process::ChildStdin>,
}

impl AsyncRead for CommandStream {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_read(cx, buf)
    }
}

impl AsyncWrite for CommandStream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().io).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().io).poll_shutdown(cx)
    }
}

fn spawn_command(cmd: &str) -> std::io::Result<BoxStream> {
    use std::process::Stdio;
    #[cfg(unix)]
    let mut c = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(cmd);
        c
    };
    #[cfg(windows)]
    let mut c = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(cmd);
        // CREATE_NO_WINDOW: no console window flashing up.
        c.creation_flags(0x0800_0000);
        c
    };
    c.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = c.spawn()?;
    let stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("no stdout"))?;
    let stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("no stdin"))?;
    Ok(Box::new(CommandStream {
        _child: child,
        io: tokio::io::join(stdout, stdin),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Echo server: returns every byte it receives.
    async fn echo() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let (mut r, mut w) = s.split();
                    let _ = tokio::io::copy(&mut r, &mut w).await;
                });
            }
        });
        port
    }

    /// Minimal SOCKS5 server (optionally requiring alice/secret) that only
    /// allows connections to 127.0.0.1 by name "localhost-echo".
    async fn socks_server(require_auth: bool, echo_port: u16) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let mut h = [0u8; 2];
                    s.read_exact(&mut h).await.unwrap();
                    let mut m = vec![0u8; h[1] as usize];
                    s.read_exact(&mut m).await.unwrap();
                    if require_auth {
                        if !m.contains(&2) {
                            s.write_all(&[5, 0xFF]).await.unwrap();
                            return;
                        }
                        s.write_all(&[5, 2]).await.unwrap();
                        let mut v = [0u8; 2];
                        s.read_exact(&mut v).await.unwrap();
                        let mut u = vec![0u8; v[1] as usize];
                        s.read_exact(&mut u).await.unwrap();
                        let mut pl = [0u8; 1];
                        s.read_exact(&mut pl).await.unwrap();
                        let mut p = vec![0u8; pl[0] as usize];
                        s.read_exact(&mut p).await.unwrap();
                        let ok = u == b"alice" && p == b"secret";
                        s.write_all(&[1, if ok { 0 } else { 1 }]).await.unwrap();
                        if !ok {
                            return;
                        }
                    } else {
                        s.write_all(&[5, 0]).await.unwrap();
                    }
                    let mut r = [0u8; 4];
                    s.read_exact(&mut r).await.unwrap();
                    assert_eq!(r[3], 3, "names are resolved by the proxy");
                    let mut l = [0u8; 1];
                    s.read_exact(&mut l).await.unwrap();
                    let mut name = vec![0u8; l[0] as usize + 2];
                    s.read_exact(&mut name).await.unwrap();
                    if &name[..name.len() - 2] != b"localhost-echo" {
                        s.write_all(&[5, 4, 0, 1, 0, 0, 0, 0, 0, 0]).await.unwrap();
                        return;
                    }
                    let mut up = TcpStream::connect(("127.0.0.1", echo_port)).await.unwrap();
                    s.write_all(&[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await.unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut s, &mut up).await;
                });
            }
        });
        port
    }

    async fn http_server(echo_port: u16) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = l.accept().await {
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut b = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        s.read_exact(&mut b).await.unwrap();
                        head.push(b[0]);
                    }
                    let text = String::from_utf8(head).unwrap();
                    // "bob:pw" in base64.
                    if !text.contains("Proxy-Authorization: Basic Ym9iOnB3\r\n") {
                        s.write_all(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n").await.unwrap();
                        return;
                    }
                    assert!(text.starts_with("CONNECT localhost-echo:22 HTTP/1.1\r\n"));
                    let mut up = TcpStream::connect(("127.0.0.1", echo_port)).await.unwrap();
                    // Data right after the headers must not be lost.
                    s.write_all(b"HTTP/1.1 200 Connection established\r\nX: y\r\n\r\n").await.unwrap();
                    let _ = tokio::io::copy_bidirectional(&mut s, &mut up).await;
                });
            }
        });
        port
    }

    fn target(proxy: Option<ProxySpec>) -> Target {
        let mut t = crate::ssh::testutil_target();
        t.hostname = "localhost-echo".into();
        t.port = 22;
        t.proxy = proxy;
        t
    }

    async fn roundtrip(mut s: BoxStream) {
        s.write_all(b"ping").await.unwrap();
        let mut buf = [0u8; 4];
        s.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
    }

    #[tokio::test]
    async fn socks5_with_and_without_auth() {
        let e = echo().await;
        let open = socks_server(false, e).await;
        let spec = |port, user: Option<&str>, pass: Option<&str>| ProxySpec::Socks5 {
            host: "127.0.0.1".into(),
            port,
            username: user.map(Into::into),
            password: pass.map(Into::into),
        };
        roundtrip(dial(&target(Some(spec(open, None, None)))).await.unwrap()).await;

        let authed = socks_server(true, e).await;
        roundtrip(dial(&target(Some(spec(authed, Some("alice"), Some("secret"))))).await.unwrap()).await;
        let err = dial(&target(Some(spec(authed, Some("alice"), Some("nope"))))).await.err().unwrap();
        assert!(err.to_string().contains("rejected the username"), "{err}");
        let err = dial(&target(Some(spec(authed, None, None)))).await.err().unwrap();
        assert!(err.to_string().contains("authentication methods"), "{err}");

        let mut t = target(Some(spec(open, None, None)));
        t.hostname = "elsewhere".into();
        let err = dial(&t).await.err().unwrap();
        assert!(err.to_string().contains("host unreachable"), "{err}");
    }

    #[tokio::test]
    async fn http_connect_with_auth() {
        let e = echo().await;
        let p = http_server(e).await;
        let spec = |user: Option<&str>| ProxySpec::Http {
            host: "127.0.0.1".into(),
            port: p,
            username: user.map(Into::into),
            password: Some("pw".into()),
        };
        roundtrip(dial(&target(Some(spec(Some("bob"))))).await.unwrap()).await;
        let err = dial(&target(Some(spec(None)))).await.err().unwrap();
        assert!(err.to_string().contains("407"), "{err}");
    }

    #[test]
    fn proxy_command_tokens() {
        assert_eq!(
            expand_proxy_command("ssh -W %h:%p -l %r bastion 100%%", "db", 2222, "ops"),
            "ssh -W db:2222 -l ops bastion 100%"
        );
    }

    #[tokio::test]
    async fn proxy_command_needs_approval() {
        let e = echo().await;
        let cmd = |approved| ProxySpec::Command {
            command: format!("nc 127.0.0.1 {e}"),
            approved,
        };
        let err = dial(&target(Some(cmd(false)))).await.err().unwrap();
        assert!(err.to_string().contains("approved"), "{err}");
        #[cfg(unix)]
        if std::process::Command::new("nc").arg("-h").output().is_ok() {
            roundtrip(dial(&target(Some(cmd(true)))).await.unwrap()).await;
        }
    }
}
