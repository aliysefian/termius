//! X11 forwarding, done the way OpenSSH does it.
//!
//! The server is given a random *fake* cookie. Each X connection the server
//! forwards back starts with an X11 setup packet carrying that cookie. We
//! check it (so other users on the server can't use your display), replace it
//! with the *real* cookie from the local `xauth`, and connect to the local X
//! server. The real cookie never leaves this computer.

use std::path::PathBuf;

use rand::{rngs::OsRng, RngCore};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const PROTO: &str = "MIT-MAGIC-COOKIE-1";

/// Everything one connection needs to forward X11.
#[derive(Debug)]
pub struct X11Setup {
    pub display: Display,
    /// Given to the server; checked on every forwarded connection.
    pub fake: Vec<u8>,
    /// From the local `xauth`, substituted before reaching the X server.
    pub real: Option<Vec<u8>>,
}

impl X11Setup {
    /// Prepare forwarding to the display named by `$DISPLAY` (or the
    /// platform default). Errors describe why X11 can't be used.
    pub fn from_env() -> Result<Self, String> {
        let value = std::env::var("DISPLAY").ok();
        let display = parse_display(value.as_deref()).ok_or_else(|| {
            format!(
                "DISPLAY={} is not a valid X display",
                value.unwrap_or_default()
            )
        })?;
        let real = real_cookie(&display);
        Ok(Self {
            display,
            fake: fake_cookie(),
            real,
        })
    }
}

/// Where the local X server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Display {
    /// Unix socket, e.g. `/tmp/.X11-unix/X0`, or XQuartz's launchd socket.
    Unix {
        path: PathBuf,
        display: String,
        screen: u32,
    },
    /// TCP, e.g. `localhost:0` → port 6000.
    Tcp {
        host: String,
        port: u16,
        display: String,
        screen: u32,
    },
}

impl Display {
    pub fn screen(&self) -> u32 {
        match self {
            Display::Unix { screen, .. } | Display::Tcp { screen, .. } => *screen,
        }
    }

    /// The display string `xauth` should look up.
    pub fn xauth_name(&self) -> &str {
        match self {
            Display::Unix { display, .. } | Display::Tcp { display, .. } => display,
        }
    }
}

/// Parse a `DISPLAY` value. `None` means use the platform default: `:0` on
/// Unix, and `localhost:0` (VcXsrv, X410, Xming) on Windows.
pub fn parse_display(value: Option<&str>) -> Option<Display> {
    let raw = value.filter(|v| !v.is_empty()).unwrap_or(if cfg!(windows) {
        "localhost:0.0"
    } else {
        ":0"
    });

    // XQuartz: "/private/tmp/com.apple.launchd.x/org.xquartz:0" is itself a socket path.
    if raw.starts_with('/') {
        let (_, num) = raw.rsplit_once(':')?;
        let (_, screen) = split_screen(num)?;
        return Some(Display::Unix {
            path: PathBuf::from(raw),
            display: raw.to_string(),
            screen,
        });
    }

    let (host, num) = raw.rsplit_once(':')?;
    let (n, screen) = split_screen(num)?;
    if host.is_empty() || host == "unix" {
        Some(Display::Unix {
            path: PathBuf::from(format!("/tmp/.X11-unix/X{n}")),
            display: raw.to_string(),
            screen,
        })
    } else {
        Some(Display::Tcp {
            host: host.to_string(),
            port: 6000u16.checked_add(u16::try_from(n).ok()?)?,
            display: raw.to_string(),
            screen,
        })
    }
}

fn split_screen(s: &str) -> Option<(u32, u32)> {
    let (n, screen) = s.split_once('.').unwrap_or((s, "0"));
    Some((n.parse().ok()?, screen.parse().ok()?))
}

/// The real cookie for `display`, from `xauth list`. `None` if xauth isn't
/// installed or has no MIT-MAGIC-COOKIE-1 entry; forwarding then sends no
/// credentials, which works with servers run without access control.
pub fn real_cookie(display: &Display) -> Option<Vec<u8>> {
    let out = std::process::Command::new("xauth")
        .args(["list", display.xauth_name()])
        .output()
        .ok()?;
    parse_xauth_list(&String::from_utf8_lossy(&out.stdout))
}

pub fn parse_xauth_list(text: &str) -> Option<Vec<u8>> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let _display = parts.next()?;
        (parts.next()? == PROTO).then_some(())?;
        hex_decode(parts.next()?)
    })
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

pub fn hex_encode(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A fresh 16-byte fake cookie for one session.
pub fn fake_cookie() -> Vec<u8> {
    let mut c = vec![0u8; 16];
    OsRng.fill_bytes(&mut c);
    c
}

fn pad4(n: usize) -> usize {
    (4 - n % 4) % 4
}

/// Read the client's setup packet from `remote`, check it carries `fake`,
/// and return the same packet rewritten to carry `real` (or no auth).
///
/// Layout: byte order ('B' or 'l'), pad, major u16, minor u16,
/// auth-name length u16, auth-data length u16, pad u16, then the name and
/// data, each padded to 4 bytes.
pub async fn rewrite_setup<R: AsyncRead + Unpin>(
    remote: &mut R,
    fake: &[u8],
    real: Option<&[u8]>,
) -> std::io::Result<Vec<u8>> {
    let bad = |m: &str| std::io::Error::new(std::io::ErrorKind::PermissionDenied, m.to_string());
    let mut head = [0u8; 12];
    remote.read_exact(&mut head).await?;
    let big = match head[0] {
        b'B' => true,
        b'l' => false,
        _ => return Err(bad("not an X11 connection")),
    };
    let u16_at = |i: usize| {
        let b = [head[i], head[i + 1]];
        if big {
            u16::from_be_bytes(b)
        } else {
            u16::from_le_bytes(b)
        }
    };
    let (name_len, data_len) = (u16_at(6) as usize, u16_at(8) as usize);
    if name_len > 256 || data_len > 256 {
        return Err(bad("X11 auth too long"));
    }
    let mut name = vec![0u8; name_len + pad4(name_len)];
    remote.read_exact(&mut name).await?;
    let mut data = vec![0u8; data_len + pad4(data_len)];
    remote.read_exact(&mut data).await?;
    name.truncate(name_len);
    data.truncate(data_len);

    let fake_ok = name == PROTO.as_bytes()
        && data.len() == fake.len()
        && data.iter().zip(fake).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    if !fake_ok {
        return Err(bad("X11 connection with the wrong cookie was refused"));
    }

    let (new_name, new_data): (&[u8], &[u8]) = match real {
        Some(r) => (PROTO.as_bytes(), r),
        None => (b"", b""),
    };
    let to_bytes = |v: u16| {
        if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        }
    };
    let mut out = head.to_vec();
    out[6..8].copy_from_slice(&to_bytes(new_name.len() as u16));
    out[8..10].copy_from_slice(&to_bytes(new_data.len() as u16));
    out.extend_from_slice(new_name);
    out.extend(std::iter::repeat_n(0, pad4(new_name.len())));
    out.extend_from_slice(new_data);
    out.extend(std::iter::repeat_n(0, pad4(new_data.len())));
    Ok(out)
}

/// Bridge one forwarded X11 channel to the local display.
pub async fn forward<S>(
    mut remote: S,
    display: Display,
    fake: Vec<u8>,
    real: Option<Vec<u8>>,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let setup = rewrite_setup(&mut remote, &fake, real.as_deref()).await?;
    match display {
        #[cfg(unix)]
        Display::Unix { path, .. } => {
            let mut local = tokio::net::UnixStream::connect(path).await?;
            local.write_all(&setup).await?;
            tokio::io::copy_bidirectional(&mut local, &mut remote).await?;
        }
        #[cfg(not(unix))]
        Display::Unix { .. } => {
            return Err(std::io::Error::other(
                "Unix-socket X displays aren't available on this OS",
            ));
        }
        Display::Tcp { host, port, .. } => {
            let mut local = tokio::net::TcpStream::connect((host.as_str(), port)).await?;
            local.write_all(&setup).await?;
            tokio::io::copy_bidirectional(&mut local, &mut remote).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_display_values() {
        assert_eq!(
            parse_display(Some(":1")),
            Some(Display::Unix {
                path: "/tmp/.X11-unix/X1".into(),
                display: ":1".into(),
                screen: 0
            })
        );
        assert_eq!(
            parse_display(Some("localhost:10.2")),
            Some(Display::Tcp {
                host: "localhost".into(),
                port: 6010,
                display: "localhost:10.2".into(),
                screen: 2
            })
        );
        let xq = "/private/tmp/com.apple.launchd.abc/org.xquartz:0";
        assert!(
            matches!(parse_display(Some(xq)), Some(Display::Unix { path, .. }) if path == std::path::Path::new(xq))
        );
        assert_eq!(parse_display(Some("nonsense")), None);
        assert_eq!(parse_display(Some("host:abc")), None);
        assert!(parse_display(None).is_some());
    }

    #[test]
    fn reads_xauth_output() {
        let out = "myhost/unix:0  MIT-MAGIC-COOKIE-1  00ff10ab\nother  XDM-AUTHORIZATION-1  1234\n";
        assert_eq!(parse_xauth_list(out), Some(vec![0x00, 0xff, 0x10, 0xab]));
        assert_eq!(parse_xauth_list(""), None);
        assert_eq!(hex_encode(&[0, 255, 16]), "00ff10");
    }

    fn setup_packet(big: bool, name: &[u8], data: &[u8]) -> Vec<u8> {
        let enc = |v: u16| {
            if big {
                v.to_be_bytes()
            } else {
                v.to_le_bytes()
            }
        };
        let mut p = vec![if big { b'B' } else { b'l' }, 0];
        p.extend_from_slice(&enc(11));
        p.extend_from_slice(&enc(0));
        p.extend_from_slice(&enc(name.len() as u16));
        p.extend_from_slice(&enc(data.len() as u16));
        p.extend_from_slice(&[0, 0]);
        p.extend_from_slice(name);
        p.extend(std::iter::repeat_n(0, pad4(name.len())));
        p.extend_from_slice(data);
        p.extend(std::iter::repeat_n(0, pad4(data.len())));
        p
    }

    #[tokio::test]
    async fn swaps_fake_cookie_for_real_in_both_byte_orders() {
        let fake = vec![7u8; 16];
        let real = vec![9u8; 16];
        for big in [false, true] {
            let pkt = setup_packet(big, PROTO.as_bytes(), &fake);
            let out = rewrite_setup(&mut pkt.as_slice(), &fake, Some(&real))
                .await
                .unwrap();
            assert_eq!(out, setup_packet(big, PROTO.as_bytes(), &real));
            // No real cookie: auth is stripped.
            let out = rewrite_setup(&mut pkt.as_slice(), &fake, None)
                .await
                .unwrap();
            assert_eq!(out, setup_packet(big, b"", b""));
        }
    }

    #[tokio::test]
    async fn refuses_wrong_or_malformed_setup() {
        let fake = vec![7u8; 16];
        let wrong = setup_packet(false, PROTO.as_bytes(), &[8u8; 16]);
        assert!(rewrite_setup(&mut wrong.as_slice(), &fake, None)
            .await
            .is_err());
        let no_auth = setup_packet(false, b"", b"");
        assert!(rewrite_setup(&mut no_auth.as_slice(), &fake, None)
            .await
            .is_err());
        let junk = b"GET / HTTP/1.1\r\n".to_vec();
        assert!(rewrite_setup(&mut junk.as_slice(), &fake, None)
            .await
            .is_err());
    }
}
