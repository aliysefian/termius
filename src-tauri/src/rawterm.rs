//! Telnet and serial-console sessions, for network gear and devices that
//! don't speak SSH. Streamed through the same [`TermSink`] as SSH and local
//! shells, so tabs, logging and find behave the same.
//!
//! Telnet sends everything in clear text, which the UI says whenever one is
//! opened.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

use crate::ssh::{SessionStatus, TermSink};

#[derive(Debug, thiserror::Error)]
pub enum RawError {
    #[error("could not connect to {addr}: {reason}")]
    Connect { addr: String, reason: String },
    #[error("could not open {path}: {reason}")]
    Serial { path: String, reason: String },
    #[error("no telnet or serial session for this pane")]
    NotRunning,
    #[error("a session is already open in this pane")]
    AlreadyOpen,
}

// ---------------------------------------------------------------------------
// Telnet protocol (RFC 854 plus NAWS, TTYPE, ECHO and SGA)
// ---------------------------------------------------------------------------

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;
const OPT_ECHO: u8 = 1;
const OPT_SGA: u8 = 3;
const OPT_TTYPE: u8 = 24;
const OPT_NAWS: u8 = 31;
const TTYPE_SEND: u8 = 1;
const TTYPE_IS: u8 = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    Iac,
    /// After WILL/WONT/DO/DONT, waiting for the option byte.
    Verb(u8),
    Sub,
    SubIac,
}

/// Telnet option negotiation and framing. Pure, so it's unit tested.
pub struct Telnet {
    state: State,
    sub: Vec<u8>,
    /// The server agreed that we send window sizes.
    naws: bool,
    cols: u16,
    rows: u16,
    term: &'static str,
}

impl Telnet {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            state: State::Data,
            sub: Vec::new(),
            naws: false,
            cols,
            rows,
            term: "XTERM-256COLOR",
        }
    }

    fn naws_bytes(&self) -> Vec<u8> {
        let mut out = vec![IAC, SB, OPT_NAWS];
        for b in [self.cols.to_be_bytes(), self.rows.to_be_bytes()].concat() {
            out.push(b);
            if b == IAC {
                out.push(IAC); // escape 255 inside subnegotiation
            }
        }
        out.extend_from_slice(&[IAC, SE]);
        out
    }

    /// Split server bytes into terminal data and our protocol replies.
    pub fn receive(&mut self, input: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let mut data = Vec::with_capacity(input.len());
        let mut reply = Vec::new();
        for &b in input {
            self.state = match (self.state, b) {
                (State::Data, IAC) => State::Iac,
                (State::Data, _) => {
                    data.push(b);
                    State::Data
                }
                (State::Iac, IAC) => {
                    data.push(IAC);
                    State::Data
                }
                (State::Iac, WILL | WONT | DO | DONT) => State::Verb(b),
                (State::Iac, SB) => {
                    self.sub.clear();
                    State::Sub
                }
                // NOP, GA, AYT…: nothing to do.
                (State::Iac, _) => State::Data,
                (State::Verb(verb), opt) => {
                    match (verb, opt) {
                        // We want the server to echo and suppress go-ahead.
                        (WILL, OPT_ECHO | OPT_SGA) => reply.extend_from_slice(&[IAC, DO, opt]),
                        (WILL, _) => reply.extend_from_slice(&[IAC, DONT, opt]),
                        (DO, OPT_NAWS) => {
                            self.naws = true;
                            reply.extend_from_slice(&[IAC, WILL, OPT_NAWS]);
                            reply.extend_from_slice(&self.naws_bytes());
                        }
                        (DO, OPT_TTYPE | OPT_SGA) => reply.extend_from_slice(&[IAC, WILL, opt]),
                        (DO, _) => reply.extend_from_slice(&[IAC, WONT, opt]),
                        (DONT, OPT_NAWS) => self.naws = false,
                        _ => {}
                    }
                    State::Data
                }
                (State::Sub, IAC) => State::SubIac,
                (State::Sub, _) => {
                    if self.sub.len() < 256 {
                        self.sub.push(b);
                    }
                    State::Sub
                }
                (State::SubIac, SE) => {
                    if self.sub.first() == Some(&OPT_TTYPE) && self.sub.get(1) == Some(&TTYPE_SEND) {
                        reply.extend_from_slice(&[IAC, SB, OPT_TTYPE, TTYPE_IS]);
                        reply.extend_from_slice(self.term.as_bytes());
                        reply.extend_from_slice(&[IAC, SE]);
                    }
                    State::Data
                }
                (State::SubIac, _) => {
                    self.sub.push(b);
                    State::Sub
                }
            };
        }
        (data, reply)
    }

    /// Frame keyboard input: double IAC, and CR becomes CR NUL (RFC 854).
    pub fn encode_input(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len() + 4);
        for &b in data {
            match b {
                IAC => out.extend_from_slice(&[IAC, IAC]),
                b'\r' => out.extend_from_slice(b"\r\0"),
                _ => out.push(b),
            }
        }
        out
    }

    /// New window size; bytes to send when the server accepted NAWS.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        self.cols = cols;
        self.rows = rows;
        self.naws.then(|| self.naws_bytes())
    }
}

// ---------------------------------------------------------------------------
// Serial settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Parity {
    #[default]
    None,
    Odd,
    Even,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FlowControl {
    #[default]
    None,
    Software,
    Hardware,
}

/// A serial line, e.g. 9600 8N1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SerialConfig {
    pub path: String,
    pub baud: u32,
    #[serde(default = "default_bits")]
    pub data_bits: u8,
    #[serde(default)]
    pub parity: Parity,
    #[serde(default = "default_stop")]
    pub stop_bits: u8,
    #[serde(default)]
    pub flow: FlowControl,
}

fn default_bits() -> u8 {
    8
}
fn default_stop() -> u8 {
    1
}

impl SerialConfig {
    /// "9600 8N1".
    pub fn describe(&self) -> String {
        let p = match self.parity {
            Parity::None => 'N',
            Parity::Odd => 'O',
            Parity::Even => 'E',
        };
        format!("{} {}{}{}", self.baud, self.data_bits, p, self.stop_bits)
    }

    fn validate(&self) -> Result<(), String> {
        if self.path.trim().is_empty() {
            return Err("choose a serial port".into());
        }
        if !(5..=8).contains(&self.data_bits) {
            return Err("data bits must be 5 to 8".into());
        }
        if !(1..=2).contains(&self.stop_bits) {
            return Err("stop bits must be 1 or 2".into());
        }
        if self.baud == 0 || self.baud > 4_000_000 {
            return Err("unsupported baud rate".into());
        }
        Ok(())
    }
}

/// Serial ports this computer has, by name.
pub fn serial_ports() -> Vec<String> {
    let mut ports: Vec<String> = serialport::available_ports()
        .map(|v| v.into_iter().map(|p| p.port_name).collect())
        .unwrap_or_default();
    // Without udev, Linux enumeration can come back empty; look directly.
    #[cfg(target_os = "linux")]
    if ports.is_empty() {
        if let Ok(entries) = std::fs::read_dir("/dev") {
            for e in entries.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("ttyUSB") || n.starts_with("ttyACM") || n.starts_with("ttyS") && n.len() <= 6 {
                    ports.push(format!("/dev/{n}"));
                }
            }
        }
    }
    ports.sort();
    ports.dedup();
    ports
}

// ---------------------------------------------------------------------------
// Sessions
// ---------------------------------------------------------------------------

enum Input {
    Data(Vec<u8>),
    Resize(u16, u16),
    Close,
}

#[derive(Default)]
pub struct RawManager {
    sessions: Mutex<HashMap<String, mpsc::UnboundedSender<Input>>>,
}

impl RawManager {
    pub fn new() -> Self {
        Self::default()
    }

    fn register(&self, pane_id: &str) -> Result<mpsc::UnboundedReceiver<Input>, RawError> {
        let mut s = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        if s.contains_key(pane_id) {
            return Err(RawError::AlreadyOpen);
        }
        let (tx, rx) = mpsc::unbounded_channel();
        s.insert(pane_id.to_string(), tx);
        Ok(rx)
    }

    fn unregister(&self, pane_id: &str) {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(pane_id);
    }

    fn send(&self, pane_id: &str, input: Input) -> Result<(), RawError> {
        self.sessions
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(pane_id)
            .ok_or(RawError::NotRunning)?
            .send(input)
            .map_err(|_| RawError::NotRunning)
    }

    pub fn write(&self, pane_id: &str, data: Vec<u8>) -> Result<(), RawError> {
        self.send(pane_id, Input::Data(data))
    }

    pub fn resize(&self, pane_id: &str, cols: u32, rows: u32) -> Result<(), RawError> {
        self.send(pane_id, Input::Resize(cols.min(u16::MAX as u32) as u16, rows.min(u16::MAX as u32) as u16))
    }

    pub fn close(&self, pane_id: &str) {
        let _ = self.send(pane_id, Input::Close);
        self.unregister(pane_id);
    }

    pub fn close_all(&self) {
        let ids: Vec<String> = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).keys().cloned().collect();
        for id in ids {
            self.close(&id);
        }
    }

    /// Open a Telnet session. Must be called within a tokio runtime.
    pub fn telnet(
        self: &Arc<Self>,
        pane_id: String,
        host: String,
        port: u16,
        cols: u32,
        rows: u32,
        sink: Arc<dyn TermSink>,
    ) -> Result<(), RawError> {
        let mut rx = self.register(&pane_id)?;
        let me = Arc::clone(self);
        // spawn-ok: only called from the async raw_telnet command
        tokio::spawn(async move {
            sink.status(SessionStatus::Connecting);
            let addr = format!("{host}:{port}");
            let stream = match tokio::time::timeout(Duration::from_secs(20), tokio::net::TcpStream::connect((host.as_str(), port))).await {
                Ok(Ok(s)) => s,
                Ok(Err(e)) => {
                    me.unregister(&pane_id);
                    return sink.status(SessionStatus::Error { message: RawError::Connect { addr, reason: e.to_string() }.to_string() });
                }
                Err(_) => {
                    me.unregister(&pane_id);
                    return sink.status(SessionStatus::Error { message: RawError::Connect { addr, reason: "timed out".into() }.to_string() });
                }
            };
            let _ = stream.set_nodelay(true);
            let (mut r, mut w) = stream.into_split();
            let mut tn = Telnet::new(cols.min(u16::MAX as u32) as u16, rows.min(u16::MAX as u32) as u16);
            sink.status(SessionStatus::Connected);
            let mut buf = vec![0u8; 16 * 1024];
            loop {
                tokio::select! {
                    n = r.read(&mut buf) => match n {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            let (data, reply) = tn.receive(&buf[..n]);
                            if !reply.is_empty() && w.write_all(&reply).await.is_err() {
                                break;
                            }
                            if !data.is_empty() {
                                sink.data(&data);
                            }
                        }
                    },
                    msg = rx.recv() => match msg {
                        Some(Input::Data(d)) => {
                            if w.write_all(&Telnet::encode_input(&d)).await.is_err() {
                                break;
                            }
                        }
                        Some(Input::Resize(c, rw)) => {
                            if let Some(b) = tn.resize(c, rw) {
                                let _ = w.write_all(&b).await;
                            }
                        }
                        Some(Input::Close) | None => break,
                    },
                }
            }
            me.unregister(&pane_id);
            sink.status(SessionStatus::Disconnected { code: None });
        });
        Ok(())
    }

    /// Open a serial console. Reads block, so they get their own thread.
    pub fn serial(self: &Arc<Self>, pane_id: String, cfg: SerialConfig, sink: Arc<dyn TermSink>) -> Result<(), RawError> {
        cfg.validate().map_err(|reason| RawError::Serial { path: cfg.path.clone(), reason })?;
        let err = |reason: String| RawError::Serial { path: cfg.path.clone(), reason };
        let port = serialport::new(&cfg.path, cfg.baud)
            .data_bits(match cfg.data_bits {
                5 => serialport::DataBits::Five,
                6 => serialport::DataBits::Six,
                7 => serialport::DataBits::Seven,
                _ => serialport::DataBits::Eight,
            })
            .parity(match cfg.parity {
                Parity::None => serialport::Parity::None,
                Parity::Odd => serialport::Parity::Odd,
                Parity::Even => serialport::Parity::Even,
            })
            .stop_bits(if cfg.stop_bits == 2 { serialport::StopBits::Two } else { serialport::StopBits::One })
            .flow_control(match cfg.flow {
                FlowControl::None => serialport::FlowControl::None,
                FlowControl::Software => serialport::FlowControl::Software,
                FlowControl::Hardware => serialport::FlowControl::Hardware,
            })
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| err(e.to_string()))?;
        let mut reader = port.try_clone().map_err(|e| err(e.to_string()))?;
        let mut writer = port;
        let mut rx = self.register(&pane_id)?;
        sink.status(SessionStatus::Connected);

        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop_r = Arc::clone(&stop);
        let sink_r = Arc::clone(&sink);
        std::thread::Builder::new()
            .name(format!("serial-{pane_id}"))
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while !stop_r.load(std::sync::atomic::Ordering::Relaxed) {
                    match reader.read(&mut buf) {
                        Ok(0) => {}
                        Ok(n) => sink_r.data(&buf[..n]),
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| err(e.to_string()))?;

        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("serial-w-{pane_id}"))
            .spawn(move || {
                while let Some(msg) = rx.blocking_recv() {
                    match msg {
                        Input::Data(d) => {
                            if writer.write_all(&d).is_err() {
                                break;
                            }
                        }
                        Input::Resize(..) => {}
                        Input::Close => break,
                    }
                }
                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                me.unregister(&pane_id);
                sink.status(SessionStatus::Disconnected { code: None });
            })
            .map_err(|e| err(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negotiates_echo_naws_and_terminal_type() {
        let mut t = Telnet::new(120, 40);
        let (data, reply) = t.receive(&[b'h', IAC, WILL, OPT_ECHO, IAC, DO, OPT_NAWS, b'i', IAC, IAC, IAC, DO, 99]);
        assert_eq!(data, [b'h', b'i', IAC]);
        assert_eq!(
            reply,
            [IAC, DO, OPT_ECHO, IAC, WILL, OPT_NAWS, IAC, SB, OPT_NAWS, 0, 120, 0, 40, IAC, SE, IAC, WONT, 99]
        );
        // Terminal type on request.
        let (_, reply) = t.receive(&[IAC, DO, OPT_TTYPE, IAC, SB, OPT_TTYPE, TTYPE_SEND, IAC, SE]);
        let mut expected = vec![IAC, WILL, OPT_TTYPE, IAC, SB, OPT_TTYPE, TTYPE_IS];
        expected.extend_from_slice(b"XTERM-256COLOR");
        expected.extend_from_slice(&[IAC, SE]);
        assert_eq!(reply, expected);
        // Resizes are sent once NAWS is agreed; a 255 is escaped.
        assert_eq!(t.resize(255, 30).unwrap(), [IAC, SB, OPT_NAWS, 0, IAC, IAC, 0, 30, IAC, SE]);
    }

    #[test]
    fn sequences_split_across_reads_and_unknown_will() {
        let mut t = Telnet::new(80, 24);
        let (d1, r1) = t.receive(&[b'a', IAC]);
        let (d2, r2) = t.receive(&[WILL, 42, b'b']);
        assert_eq!([d1, d2].concat(), b"ab");
        assert!(r1.is_empty());
        assert_eq!(r2, [IAC, DONT, 42]);
        assert!(t.resize(100, 30).is_none(), "no NAWS until the server asks");
    }

    #[test]
    fn input_framing() {
        assert_eq!(Telnet::encode_input(b"ls\r"), b"ls\r\0");
        assert_eq!(Telnet::encode_input(&[1, IAC, 2]), [1, IAC, IAC, 2]);
    }

    #[test]
    fn serial_settings() {
        let c = SerialConfig { path: "/dev/ttyUSB0".into(), baud: 115200, data_bits: 8, parity: Parity::None, stop_bits: 1, flow: FlowControl::None };
        assert_eq!(c.describe(), "115200 8N1");
        assert!(c.validate().is_ok());
        assert!(SerialConfig { data_bits: 9, ..c.clone() }.validate().is_err());
        assert!(SerialConfig { path: " ".into(), ..c }.validate().is_err());
    }

    struct Collect(std::sync::mpsc::Sender<(Option<Vec<u8>>, Option<SessionStatus>)>);
    impl TermSink for Collect {
        fn data(&self, bytes: &[u8]) {
            let _ = self.0.send((Some(bytes.to_vec()), None));
        }
        fn status(&self, s: SessionStatus) {
            let _ = self.0.send((None, Some(s)));
        }
    }

    /// A tiny telnet server: asks for NAWS, then echoes lines in upper case.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn telnet_session_end_to_end() {
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut s, _) = l.accept().await.unwrap();
            s.write_all(&[IAC, DO, OPT_NAWS]).await.unwrap();
            s.write_all(b"login: ").await.unwrap();
            let mut got = Vec::new();
            let mut buf = [0u8; 256];
            while !got.windows(2).any(|w| w == b"\r\0") {
                let n = s.read(&mut buf).await.unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            // Lowercase only: window-size bytes (80 = b'P') must not count as typed text.
            let text: Vec<u8> = got.iter().copied().filter(|b| b.is_ascii_lowercase()).collect();
            s.write_all(format!("\r\nHELLO {}\r\n", String::from_utf8_lossy(&text).to_uppercase()).as_bytes()).await.unwrap();
            got
        });

        let m = Arc::new(RawManager::new());
        let (tx, rx) = std::sync::mpsc::channel();
        m.telnet("p".into(), "127.0.0.1".into(), port, 80, 24, Arc::new(Collect(tx))).unwrap();
        let mut out = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut sent = false;
        while !String::from_utf8_lossy(&out).contains("HELLO OPS") {
            let (data, status) = rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now())).expect("output");
            if let Some(d) = data {
                out.extend(d);
            }
            if matches!(status, Some(SessionStatus::Error { .. })) {
                panic!("{status:?}");
            }
            if !sent && String::from_utf8_lossy(&out).contains("login:") {
                m.write("p", b"ops\r".to_vec()).unwrap();
                sent = true;
            }
        }
        let got = server.await.unwrap();
        // The server received our window size before the input.
        assert!(got.windows(3).any(|w| w == [IAC, SB, OPT_NAWS]), "{got:?}");
        assert!(!String::from_utf8_lossy(&out).contains('\u{fffd}'), "no protocol bytes leak into the terminal");
        m.close("p");
    }
}
