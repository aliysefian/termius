//! A VNC (RFB 3.3 to 3.8) client over any byte stream: a TCP socket, or a channel of an SSH connection.
//!
//! Tauri-agnostic, like the RDP client: pictures, the pointer, clipboard text and the end come out through
//! [`DisplaySink`], keys, mouse and clipboard text go in as [`Input`]. Supported: no authentication and VNC
//! authentication (a 16-byte challenge answered with DES, the protocol's own weak scheme: use it through
//! an SSH tunnel), the Raw and CopyRect encodings, desktop resizes, the server's cursor shape and
//! Latin-1 clipboard text. Not supported: Hextile, ZRLE and the other compressed encodings (a server
//! falls back to Raw, which costs bandwidth), TLS and the VeNCrypt and Apple/Mac logins.

pub mod keys;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use des::cipher::{Block, BlockCipherEncrypt, KeyInit};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::display::{Cursor, DisplaySink, Input};

pub const MIN_SIZE: u16 = 1;
/// A picture larger than this (in pixels) is refused rather than allocated.
const MAX_PIXELS: usize = 64 * 1024 * 1024;
/// Text the server puts on the clipboard, at most; the rest is read and dropped.
const MAX_CLIPBOARD: usize = 1 << 20;
const MAX_NAME: usize = 4096;
const MAX_REASON: usize = 4096;

#[derive(Debug, thiserror::Error)]
pub enum VncError {
    #[error("couldn't reach {0}")]
    Unreachable(String),
    #[error("this server needs a password")]
    PasswordRequired,
    #[error("wrong password")]
    BadPassword,
    #[error("the server refused the connection: {0}")]
    Refused(String),
    #[error("{0}")]
    Protocol(String),
    #[error("no such remote desktop session")]
    NoSession,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

type Result<T> = std::result::Result<T, VncError>;

fn protocol(msg: impl Into<String>) -> VncError {
    VncError::Protocol(msg.into())
}

/// What the server says about itself once the handshake is done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerInfo {
    pub width: u16,
    pub height: u16,
    pub name: String,
}

// -- the handshake ---------------------------------------------------------------------

/// The key DES wants for a VNC password: the first eight bytes, zero padded, each with its bits reversed.
pub fn auth_key(password: &str) -> [u8; 8] {
    let mut key = [0u8; 8];
    for (k, b) in key.iter_mut().zip(password.bytes()) {
        *k = b.reverse_bits();
    }
    key
}

/// One DES block, encrypted.
pub fn des_block(key: &[u8; 8], block: &[u8]) -> [u8; 8] {
    let cipher = des::Des::new_from_slice(key).expect("a DES key is eight bytes");
    let mut b = Block::<des::Des>::try_from(block).expect("a DES block is eight bytes");
    cipher.encrypt_block(&mut b);
    let mut out = [0u8; 8];
    out.copy_from_slice(&b);
    out
}

/// The answer to a VNC authentication challenge.
pub fn challenge_response(password: &str, challenge: &[u8; 16]) -> [u8; 16] {
    let key = auth_key(password);
    let mut out = [0u8; 16];
    out[..8].copy_from_slice(&des_block(&key, &challenge[..8]));
    out[8..].copy_from_slice(&des_block(&key, &challenge[8..]));
    out
}

async fn read_string<R: AsyncRead + Unpin>(r: &mut R, cap: usize) -> Result<String> {
    let len = r.read_u32().await? as usize;
    if len > cap {
        return Err(protocol("the server sent a message that is too long"));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).await?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Meet the server: versions, the way to sign in, and the shape of the screen.
pub async fn handshake<S: AsyncRead + AsyncWrite + Unpin>(s: &mut S, password: Option<&str>) -> Result<ServerInfo> {
    let mut version = [0u8; 12];
    s.read_exact(&mut version).await?;
    let text = String::from_utf8_lossy(&version);
    let minor = text
        .strip_prefix("RFB 003.")
        .and_then(|rest| rest.trim_end().parse::<u32>().ok())
        .ok_or_else(|| protocol("this isn't a VNC server (it didn't start with a version)"))?;
    // What we speak: 3.8, 3.7, or 3.3 (3.4 to 3.6 are 3.3).
    let ours = if minor >= 8 { 8 } else if minor == 7 { 7 } else { 3 };
    s.write_all(format!("RFB 003.00{ours}\n").as_bytes()).await?;

    let chosen: u8 = if ours == 3 {
        match s.read_u32().await? {
            0 => return Err(VncError::Refused(read_string(s, MAX_REASON).await?)),
            t @ (1 | 2) => t as u8,
            t => return Err(protocol(format!("the server asked for a sign-in this app doesn't have ({t})"))),
        }
    } else {
        let n = s.read_u8().await?;
        if n == 0 {
            return Err(VncError::Refused(read_string(s, MAX_REASON).await?));
        }
        let mut types = vec![0u8; n as usize];
        s.read_exact(&mut types).await?;
        let pick = if types.contains(&2) && (password.is_some() || !types.contains(&1)) {
            2
        } else if types.contains(&1) {
            1
        } else {
            return Err(protocol(format!("the server offers only sign-ins this app doesn't have ({types:?}); VeNCrypt and Apple logins aren't supported")));
        };
        if pick == 2 && password.is_none() {
            return Err(VncError::PasswordRequired);
        }
        s.write_all(&[pick]).await?;
        pick
    };

    if chosen == 2 {
        let Some(password) = password else { return Err(VncError::PasswordRequired) };
        let mut challenge = [0u8; 16];
        s.read_exact(&mut challenge).await?;
        s.write_all(&challenge_response(password, &challenge)).await?;
    }
    // 3.8 always sends a result; before that only VNC authentication does.
    if (ours >= 8 || chosen == 2) && s.read_u32().await? != 0 {
        if ours >= 8 {
            let why = read_string(s, MAX_REASON).await.unwrap_or_default();
            let lower = why.to_lowercase();
            return Err(if why.is_empty() || lower.contains("fail") || lower.contains("password") { VncError::BadPassword } else { VncError::Refused(why) });
        }
        return Err(VncError::BadPassword);
    }

    // ClientInit: share the desktop with whoever else is connected.
    s.write_all(&[1]).await?;
    let width = s.read_u16().await?;
    let height = s.read_u16().await?;
    let mut format = [0u8; 16];
    s.read_exact(&mut format).await?;
    let name = read_string(s, MAX_NAME).await?;
    Ok(ServerInfo { width, height, name })
}

// -- messages we send ----------------------------------------------------------------

/// SetPixelFormat for 32 bits per pixel, true colour, red 16, green 8, blue 0, little endian: bytes B, G, R, X.
fn set_pixel_format() -> Vec<u8> {
    let mut m = vec![0u8, 0, 0, 0];
    m.extend_from_slice(&[32, 24, 0, 1]); // bits, depth, big-endian, true-colour
    m.extend_from_slice(&255u16.to_be_bytes());
    m.extend_from_slice(&255u16.to_be_bytes());
    m.extend_from_slice(&255u16.to_be_bytes());
    m.extend_from_slice(&[16, 8, 0, 0, 0, 0]); // shifts, padding
    m
}

const ENC_RAW: i32 = 0;
const ENC_COPY_RECT: i32 = 1;
const ENC_DESKTOP_SIZE: i32 = -223;
const ENC_CURSOR: i32 = -239;

fn set_encodings() -> Vec<u8> {
    let list = [ENC_COPY_RECT, ENC_RAW, ENC_DESKTOP_SIZE, ENC_CURSOR];
    let mut m = vec![2u8, 0];
    m.extend_from_slice(&(list.len() as u16).to_be_bytes());
    for e in list {
        m.extend_from_slice(&e.to_be_bytes());
    }
    m
}

fn update_request(incremental: bool, w: u16, h: u16) -> Vec<u8> {
    let mut m = vec![3u8, incremental as u8, 0, 0, 0, 0];
    m.extend_from_slice(&w.to_be_bytes());
    m.extend_from_slice(&h.to_be_bytes());
    m
}

/// Where the mouse is and what is held, turning [`Input`] into messages.
#[derive(Default)]
pub struct InputState {
    mask: u8,
    x: u16,
    y: u16,
    keys: HashSet<u32>,
}

fn key_event(down: bool, keysym: u32) -> Vec<u8> {
    let mut m = vec![4u8, down as u8, 0, 0];
    m.extend_from_slice(&keysym.to_be_bytes());
    m
}

impl InputState {
    fn pointer(&self, mask: u8) -> Vec<u8> {
        let mut m = vec![5u8, mask];
        m.extend_from_slice(&self.x.to_be_bytes());
        m.extend_from_slice(&self.y.to_be_bytes());
        m
    }

    /// The bytes to send for one input.
    pub fn encode(&mut self, input: &Input) -> Vec<u8> {
        match input {
            Input::Key { code, down } => match keys::keysym(code) {
                Some(k) => {
                    if *down {
                        self.keys.insert(k);
                    } else {
                        self.keys.remove(&k);
                    }
                    key_event(*down, k)
                }
                None => vec![],
            },
            Input::MouseMove { x, y } => {
                self.x = *x;
                self.y = *y;
                self.pointer(self.mask)
            }
            Input::Button { button, down } => {
                // The web's 0, 1, 2 are left, middle, right; VNC's bits are 1, 2, 4.
                let bit = match button {
                    0 => 1,
                    1 => 2,
                    2 => 4,
                    _ => return vec![],
                };
                if *down {
                    self.mask |= bit;
                } else {
                    self.mask &= !bit;
                }
                self.pointer(self.mask)
            }
            Input::Wheel { vertical, units } => {
                // VNC has the wheel as buttons 4 and 5 (up, down) and 6 and 7 (left, right), each press a notch.
                let bit: u8 = match (*vertical, *units > 0) {
                    (true, true) => 8,
                    (true, false) => 16,
                    (false, false) => 32,
                    (false, true) => 64,
                };
                let notches = (i32::from(*units).unsigned_abs().div_ceil(120)).clamp(1, 10);
                let mut out = Vec::new();
                for _ in 0..notches {
                    out.extend(self.pointer(self.mask | bit));
                    out.extend(self.pointer(self.mask));
                }
                out
            }
            Input::CtrlAltDel => {
                let seq = [keys::CONTROL_LEFT, keys::ALT_LEFT, keys::DELETE];
                let mut out = Vec::new();
                for k in seq {
                    out.extend(key_event(true, k));
                }
                for k in seq.iter().rev() {
                    out.extend(key_event(false, *k));
                }
                out
            }
            Input::ReleaseAll => {
                let mut out = Vec::new();
                let mut held: Vec<u32> = self.keys.drain().collect();
                held.sort_unstable();
                for k in held {
                    out.extend(key_event(false, k));
                }
                if self.mask != 0 {
                    self.mask = 0;
                    out.extend(self.pointer(0));
                }
                out
            }
            Input::ClipboardText { text } => {
                // Latin-1 only, as the protocol says; anything else becomes a question mark.
                let bytes: Vec<u8> = text.chars().take(MAX_CLIPBOARD).map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')).collect();
                let mut m = vec![6u8, 0, 0, 0];
                m.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
                m.extend_from_slice(&bytes);
                m
            }
            Input::Close => vec![],
        }
    }
}

// -- the session -------------------------------------------------------------------------

struct Screen {
    width: usize,
    height: usize,
    rgba: Vec<u8>,
}

impl Screen {
    fn new(width: u16, height: u16) -> Result<Self> {
        let (w, h) = (usize::from(width), usize::from(height));
        if w * h > MAX_PIXELS {
            return Err(protocol("the screen is too large"));
        }
        Ok(Self { width: w, height: h, rgba: vec![0; w * h * 4] })
    }

    fn check(&self, x: u16, y: u16, w: u16, h: u16) -> Result<(usize, usize, usize, usize)> {
        let (x, y, w, h) = (usize::from(x), usize::from(y), usize::from(w), usize::from(h));
        if x + w > self.width || y + h > self.height {
            return Err(protocol("the server drew outside the screen"));
        }
        Ok((x, y, w, h))
    }

    /// Put a rectangle of server pixels (B, G, R, X) on the screen.
    fn put(&mut self, x: usize, y: usize, w: usize, h: usize, bgrx: &[u8]) {
        for row in 0..h {
            let from = row * w * 4;
            let to = ((y + row) * self.width + x) * 4;
            let (src, dst) = (&bgrx[from..from + w * 4], &mut self.rgba[to..to + w * 4]);
            for i in 0..w {
                let (s, d) = (&src[i * 4..i * 4 + 4], &mut dst[i * 4..i * 4 + 4]);
                d.copy_from_slice(&[s[2], s[1], s[0], 255]);
            }
        }
    }

    fn copy(&mut self, from: (usize, usize), to: (usize, usize), w: usize, h: usize) {
        let mut tmp = Vec::with_capacity(w * h * 4);
        for row in 0..h {
            let at = ((from.1 + row) * self.width + from.0) * 4;
            tmp.extend_from_slice(&self.rgba[at..at + w * 4]);
        }
        for row in 0..h {
            let at = ((to.1 + row) * self.width + to.0) * 4;
            self.rgba[at..at + w * 4].copy_from_slice(&tmp[row * w * 4..(row + 1) * w * 4]);
        }
    }

    fn region(&self, x: usize, y: usize, w: usize, h: usize) -> Vec<u8> {
        let mut out = Vec::with_capacity(w * h * 4);
        for row in 0..h {
            let at = ((y + row) * self.width + x) * 4;
            out.extend_from_slice(&self.rgba[at..at + w * 4]);
        }
        out
    }
}

async fn skip<R: AsyncRead + Unpin>(r: &mut R, mut n: usize) -> Result<()> {
    let mut buf = [0u8; 4096];
    while n > 0 {
        let step = n.min(buf.len());
        r.read_exact(&mut buf[..step]).await?;
        n -= step;
    }
    Ok(())
}

/// Read what the server sends, draw it, and ask for more, until it ends.
async fn read_loop<R, W>(mut r: R, w: Arc<tokio::sync::Mutex<W>>, info: &ServerInfo, sink: &Arc<dyn DisplaySink>) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut screen = Screen::new(info.width, info.height)?;
    loop {
        match r.read_u8().await? {
            0 => {
                r.read_u8().await?;
                let rects = r.read_u16().await?;
                let mut resized = false;
                for _ in 0..rects {
                    let (x, y, rw, rh) = (r.read_u16().await?, r.read_u16().await?, r.read_u16().await?, r.read_u16().await?);
                    let encoding = r.read_i32().await?;
                    match encoding {
                        ENC_RAW => {
                            let (x, y, rw, rh) = screen.check(x, y, rw, rh)?;
                            let mut bgrx = vec![0u8; rw * rh * 4];
                            r.read_exact(&mut bgrx).await?;
                            screen.put(x, y, rw, rh, &bgrx);
                            if rw > 0 && rh > 0 {
                                sink.frame(x as u16, y as u16, rw as u16, rh as u16, &screen.region(x, y, rw, rh));
                            }
                        }
                        ENC_COPY_RECT => {
                            let (sx, sy) = (r.read_u16().await?, r.read_u16().await?);
                            let (x, y, rw, rh) = screen.check(x, y, rw, rh)?;
                            let (sx, sy, _, _) = screen.check(sx, sy, rw as u16, rh as u16)?;
                            screen.copy((sx, sy), (x, y), rw, rh);
                            if rw > 0 && rh > 0 {
                                sink.frame(x as u16, y as u16, rw as u16, rh as u16, &screen.region(x, y, rw, rh));
                            }
                        }
                        ENC_DESKTOP_SIZE => {
                            screen = Screen::new(rw, rh)?;
                            sink.size(rw, rh);
                            resized = true;
                        }
                        ENC_CURSOR => {
                            let (cw, ch) = (usize::from(rw), usize::from(rh));
                            if cw * ch > MAX_PIXELS / 16 {
                                return Err(protocol("the cursor is too large"));
                            }
                            let mut pixels = vec![0u8; cw * ch * 4];
                            r.read_exact(&mut pixels).await?;
                            let mut mask = vec![0u8; cw.div_ceil(8) * ch];
                            r.read_exact(&mut mask).await?;
                            if cw == 0 || ch == 0 {
                                sink.cursor(Cursor::Hidden);
                            } else {
                                let mut rgba = Vec::with_capacity(cw * ch * 4);
                                for row in 0..ch {
                                    for col in 0..cw {
                                        let p = &pixels[(row * cw + col) * 4..][..4];
                                        let opaque = mask[row * cw.div_ceil(8) + col / 8] & (0x80 >> (col % 8)) != 0;
                                        rgba.extend_from_slice(&[p[2], p[1], p[0], if opaque { 255 } else { 0 }]);
                                    }
                                }
                                sink.cursor(Cursor::Bitmap { hot_x: x, hot_y: y, width: rw, height: rh, rgba });
                            }
                        }
                        other => return Err(protocol(format!("the server used a picture encoding this app doesn't have ({other})"))),
                    }
                }
                let request = if resized { update_request(false, screen.width as u16, screen.height as u16) } else { update_request(true, screen.width as u16, screen.height as u16) };
                w.lock().await.write_all(&request).await?;
            }
            1 => {
                // Colour map entries: this client asks for true colour, so they are not used.
                r.read_u8().await?;
                r.read_u16().await?;
                let n = r.read_u16().await? as usize;
                skip(&mut r, n * 6).await?;
            }
            2 => {} // the bell
            3 => {
                let mut pad = [0u8; 3];
                r.read_exact(&mut pad).await?;
                let len = r.read_u32().await? as usize;
                let keep = len.min(MAX_CLIPBOARD);
                let mut buf = vec![0u8; keep];
                r.read_exact(&mut buf).await?;
                skip(&mut r, len - keep).await?;
                // Latin-1 to text.
                let text: String = buf.iter().map(|&b| b as char).collect();
                if !text.is_empty() {
                    sink.clipboard(text);
                }
            }
            t => return Err(protocol(format!("the server sent a message this app doesn't know ({t})"))),
        }
    }
}

enum Event {
    Input(Input),
}

pub struct VncManager {
    sessions: Mutex<HashMap<String, mpsc::UnboundedSender<Event>>>,
}

impl Default for VncManager {
    fn default() -> Self {
        Self::new()
    }
}

impl VncManager {
    pub fn new() -> Self {
        Self { sessions: Mutex::new(HashMap::new()) }
    }

    /// Meet the server over `stream` and start a session for `pane_id`. Returns once the desktop is up.
    ///
    /// `keep` is anything that must stay alive as long as the session does, such as the SSH connection the
    /// stream runs through.
    pub async fn connect<S>(self: &Arc<Self>, pane_id: String, mut stream: S, password: Option<String>, sink: Arc<dyn DisplaySink>, keep: Option<Box<dyn std::any::Any + Send>>) -> Result<ServerInfo>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let info = tokio::time::timeout(std::time::Duration::from_secs(20), handshake(&mut stream, password.as_deref()))
            .await
            .map_err(|_| VncError::Unreachable("the server didn't answer".into()))??;
        if usize::from(info.width) * usize::from(info.height) > MAX_PIXELS {
            return Err(protocol("the screen is too large"));
        }
        stream.write_all(&set_pixel_format()).await?;
        stream.write_all(&set_encodings()).await?;
        stream.write_all(&update_request(false, info.width, info.height)).await?;
        sink.size(info.width, info.height);

        let (read, write) = tokio::io::split(stream);
        let write = Arc::new(tokio::sync::Mutex::new(write));
        let (tx, mut rx) = mpsc::unbounded_channel::<Event>();
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).insert(pane_id.clone(), tx);

        let me = Arc::clone(self);
        let shared = Arc::clone(&sink);
        let for_input = Arc::clone(&write);
        let started = info.clone();
        tokio::spawn(async move {
            let _keep = keep;
            let mut input = tokio::spawn(async move {
                let mut state = InputState::default();
                while let Some(Event::Input(i)) = rx.recv().await {
                    if matches!(i, Input::Close) {
                        break;
                    }
                    let bytes = state.encode(&i);
                    if !bytes.is_empty() && for_input.lock().await.write_all(&bytes).await.is_err() {
                        break;
                    }
                }
            });
            let outcome = tokio::select! {
                r = read_loop(read, write, &started, &shared) => r,
                _ = &mut input => Ok(()),
            };
            input.abort();
            me.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&pane_id);
            // A server that closes the connection is the normal way a session ends.
            let error = match outcome {
                Err(VncError::Io(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => None,
                Err(e) => Some(e.to_string()),
                Ok(()) => None,
            };
            shared.ended(error);
        });
        Ok(info)
    }

    pub fn send(&self, pane_id: &str, input: Input) -> Result<()> {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions.get(pane_id).ok_or(VncError::NoSession)?.send(Event::Input(input)).map_err(|_| VncError::NoSession)
    }

    pub fn close(&self, pane_id: &str) {
        let tx = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(pane_id);
        if let Some(tx) = tx {
            let _ = tx.send(Event::Input(Input::Close));
        }
    }

    pub fn close_all(&self) {
        let all: Vec<_> = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).drain().collect();
        for (_, tx) in all {
            let _ = tx.send(Event::Input(Input::Close));
        }
    }
}

#[cfg(test)]
mod tests;
