//! A remote desktop (RDP) client on IronRDP.
//!
//! Tauri-agnostic: a session reports pictures, the pointer, clipboard text and
//! its end through [`RdpSink`], and takes keyboard, mouse and clipboard input
//! through [`Input`]. TLS is not checked against a certificate authority (RDP
//! servers mostly have self-signed certificates); the server's certificate is
//! pinned instead, by SHA-256 fingerprint, like an SSH host key, and a session
//! doesn't start until the certificate is known. That check happens before any
//! credentials are sent.

pub mod clipboard;
pub mod keys;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ironrdp_connector::connection_activation::ConnectionActivationState;
use ironrdp_connector::{ClientConnector, ConnectionResult, Credentials, DesktopSize, ServerName};
use ironrdp_core::WriteBuf;
use ironrdp_graphics::image_processing::PixelFormat;
use ironrdp_input::{Database, MouseButton, MousePosition, Operation, WheelRotations};
use ironrdp_pdu::rdp::capability_sets::{client_codecs_capabilities, MajorPlatformType};
use ironrdp_session::image::DecodedImage;
use ironrdp_session::{fast_path, ActiveStage, ActiveStageBuilder, ActiveStageOutput};
use ironrdp_tokio::{single_sequence_step_read, split_tokio_framed, FramedWrite};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use x509_cert::der::Encode;

use self::clipboard::TextClipboard;

pub const MIN_SIZE: u16 = 200;
pub const MAX_SIZE: u16 = 8192;

#[derive(Debug, thiserror::Error)]
pub enum RdpError {
    #[error("{0}")]
    Connect(String),
    #[error("couldn't reach {0}")]
    Unreachable(String),
    #[error("wrong user name or password, or this account may not sign in over Remote Desktop")]
    Credentials,
    #[error("the server's certificate isn't trusted yet")]
    CertificateUnknown(Box<CertInfo>),
    #[error("the server's certificate has changed")]
    CertificateChanged { expected: String, found: Box<CertInfo> },
    #[error("no such remote desktop session")]
    NoSession,
}

/// What the server's certificate looks like, for the person to decide on.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct CertInfo {
    /// SHA-256 of the certificate, as 64 lowercase hex digits.
    pub fingerprint: String,
    pub subject: String,
    pub issuer: String,
    pub not_before: String,
    pub not_after: String,
}

#[derive(Debug, Clone)]
pub struct Settings {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub domain: Option<String>,
    pub width: u16,
    pub height: u16,
    /// 16 or 32.
    pub color_depth: u8,
    pub security: Security,
}

/// How to secure the sign-in.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Security {
    /// Network Level Authentication (sign in before the desktop starts) when the
    /// server offers it, else TLS. What Windows servers choose themselves.
    #[default]
    Auto,
    /// Only NLA; a server that doesn't offer it is refused rather than downgraded to.
    Nla,
    /// TLS only: the server's own sign-in screen.
    Tls,
}

/// What is already known about the server's certificate.
#[derive(Debug, Clone, Default)]
pub struct CertCheck {
    /// The fingerprint trusted before, if any.
    pub pinned: Option<String>,
    /// A fingerprint the person has just accepted, for a new or changed certificate.
    pub accept: Option<String>,
}

impl CertCheck {
    /// Whether to go ahead with a server presenting `found`.
    fn check(&self, found: &CertInfo) -> Result<(), RdpError> {
        let accepted = self.accept.as_deref() == Some(found.fingerprint.as_str());
        match &self.pinned {
            Some(p) if *p == found.fingerprint => Ok(()),
            Some(p) if !accepted => Err(RdpError::CertificateChanged { expected: p.clone(), found: Box::new(found.clone()) }),
            None if !accepted => Err(RdpError::CertificateUnknown(Box::new(found.clone()))),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Cursor {
    Default,
    Hidden,
    Bitmap { hot_x: u16, hot_y: u16, width: u16, height: u16, rgba: Vec<u8> },
}

pub trait RdpSink: Send + Sync + 'static {
    fn size(&self, width: u16, height: u16);
    /// A changed rectangle, as RGBA rows.
    fn frame(&self, x: u16, y: u16, width: u16, height: u16, rgba: &[u8]);
    fn cursor(&self, cursor: Cursor);
    fn clipboard(&self, text: String);
    /// The session is over; `error` says why when it wasn't the person's doing.
    fn ended(&self, error: Option<String>);
}

#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Input {
    Key { code: String, down: bool },
    MouseMove { x: u16, y: u16 },
    Button { button: u8, down: bool },
    Wheel { vertical: bool, units: i16 },
    /// Ctrl+Alt+Del, which the window can't send as keys because the computer would act on it first.
    CtrlAltDel,
    /// Let go of every key and button (the window lost focus).
    ReleaseAll,
    /// Offer this text to the remote's clipboard.
    ClipboardText { text: String },
    Close,
}

enum Event {
    Input(Input),
    Clipboard(ironrdp_cliprdr::backend::ClipboardMessage),
}

// -- the manager ---------------------------------------------------------------------

pub struct RdpManager {
    sessions: Mutex<HashMap<String, mpsc::UnboundedSender<Event>>>,
}

impl Default for RdpManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RdpManager {
    pub fn new() -> Self {
        Self { sessions: Mutex::new(HashMap::new()) }
    }

    /// Connect and start a session for `pane_id`. Returns the server's
    /// certificate fingerprint once the desktop is up, so the caller can pin it.
    pub async fn connect(self: &Arc<Self>, pane_id: String, settings: Settings, cert: CertCheck, sink: Arc<dyn RdpSink>) -> Result<CertInfo, RdpError> {
        let (tx, rx) = mpsc::unbounded_channel::<Event>();
        let (clip_tx, mut clip_rx) = mpsc::unbounded_channel();
        let local_text: Arc<Mutex<Option<String>>> = Arc::default();
        let remote_sink = Arc::clone(&sink);
        let backend = TextClipboard::new(Arc::clone(&local_text), clip_tx, Box::new(move |t| remote_sink.clipboard(t)));
        // The clipboard backend runs inside the session, so its messages join the same queue.
        let forward = tx.clone();
        tokio::spawn(async move {
            while let Some(m) = clip_rx.recv().await {
                if forward.send(Event::Clipboard(m)).is_err() {
                    break;
                }
            }
        });

        let connected = tokio::time::timeout(Duration::from_secs(30), establish(&settings, &cert, backend)).await.map_err(|_| RdpError::Unreachable(settings.host.clone()))??;
        let Established { framed, result, info } = connected;
        sink.size(result.desktop_size.width, result.desktop_size.height);

        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).insert(pane_id.clone(), tx);
        let me = Arc::clone(self);
        tokio::spawn(async move {
            let outcome = run_session(framed, result, rx, &sink, local_text).await;
            me.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&pane_id);
            sink.ended(outcome.err().map(|e| e.to_string()));
        });
        Ok(info)
    }

    pub fn send(&self, pane_id: &str, input: Input) -> Result<(), RdpError> {
        let sessions = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
        sessions.get(pane_id).ok_or(RdpError::NoSession)?.send(Event::Input(input)).map_err(|_| RdpError::NoSession)
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

// -- connecting ------------------------------------------------------------------------

type Stream = ironrdp_tokio::TokioFramed<ironrdp_tls::TlsStream<TcpStream>>;

struct Established {
    framed: Stream,
    result: ConnectionResult,
    info: CertInfo,
}

/// There's no network to reach a Kerberos server over; NTLM needs none.
struct NoNetwork;

impl ironrdp_tokio::NetworkClient for NoNetwork {
    async fn send(&mut self, _request: &ironrdp_connector::sspi::generator::NetworkRequest) -> ironrdp_connector::ConnectorResult<Vec<u8>> {
        Err(ironrdp_connector::general_err!("Kerberos isn't supported; the server must accept NTLM"))
    }
}

pub fn cert_info(cert: &x509_cert::Certificate) -> Result<CertInfo, RdpError> {
    let der = cert.to_der().map_err(|e| RdpError::Connect(format!("unreadable server certificate: {e}")))?;
    let fingerprint: String = Sha256::digest(&der).iter().map(|b| format!("{b:02x}")).collect();
    let validity = &cert.tbs_certificate.validity;
    Ok(CertInfo {
        fingerprint,
        subject: cert.tbs_certificate.subject.to_string(),
        issuer: cert.tbs_certificate.issuer.to_string(),
        not_before: validity.not_before.to_date_time().to_string(),
        not_after: validity.not_after.to_date_time().to_string(),
    })
}

/// `DOMAIN\user` carries the domain with the name; an explicit domain wins.
/// `user@domain` is passed whole: the server reads it as a principal name.
pub fn split_account(username: &str, domain: &str) -> (String, Option<String>) {
    if !domain.trim().is_empty() {
        return (username.to_string(), Some(domain.trim().to_string()));
    }
    match username.split_once('\\') {
        Some((d, u)) if !d.is_empty() && !u.is_empty() => (u.to_string(), Some(d.to_string())),
        _ => (username.to_string(), None),
    }
}

fn clamp_size(v: u16) -> u16 {
    // RDP wants an even width; keep the height even too.
    v.clamp(MIN_SIZE, MAX_SIZE) & !1
}

fn connector_config(s: &Settings) -> ironrdp_connector::Config {
    ironrdp_connector::Config {
        desktop_size: DesktopSize { width: clamp_size(s.width), height: clamp_size(s.height) },
        desktop_scale_factor: 0,
        enable_tls: s.security != Security::Nla,
        enable_credssp: s.security != Security::Tls,
        credentials: Credentials::UsernamePassword { username: s.username.clone(), password: s.password.clone() },
        domain: s.domain.clone().filter(|d| !d.is_empty()),
        client_build: 0,
        client_name: "SSHVault".into(),
        keyboard_type: ironrdp_pdu::gcc::KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_functional_keys_count: 12,
        keyboard_layout: 0,
        ime_file_name: String::new(),
        bitmap: Some(ironrdp_connector::BitmapConfig {
            lossy_compression: true,
            color_depth: if s.color_depth == 16 { 16 } else { 32 },
            codecs: client_codecs_capabilities(&[]).unwrap_or_default(),
        }),
        dig_product_id: String::new(),
        client_dir: String::new(),
        alternate_shell: String::new(),
        work_dir: String::new(),
        platform: MajorPlatformType::UNSPECIFIED,
        hardware_id: None,
        request_data: None,
        // Sends the credentials with the client info, so servers like xrdp skip their own sign-in screen.
        autologon: true,
        enable_audio_playback: false,
        performance_flags: Default::default(),
        license_cache: None,
        timezone_info: Default::default(),
        compression_type: None,
        enable_server_pointer: true,
        pointer_software_rendering: false,
        multitransport_flags: None,
    }
}

fn explain(e: ironrdp_connector::ConnectorError, host: &str) -> RdpError {
    use std::error::Error as _;
    let mut text = e.to_string();
    let mut src = e.source();
    while let Some(s) = src {
        text.push_str(": ");
        text.push_str(&s.to_string());
        src = s.source();
    }
    let lower = text.to_ascii_lowercase();
    if lower.contains("server selected") {
        RdpError::Connect("the server doesn't offer the kind of sign-in this host is set to require. Set the host's security to Automatic".into())
    } else if lower.contains("logon_failure") || lower.contains("0xc000006d") || lower.contains("0xc000006a") || lower.contains("access denied") || lower.contains("wrong password") {
        RdpError::Credentials
    } else if lower.contains("connection refused") || lower.contains("tcp connect") {
        RdpError::Unreachable(host.to_string())
    } else {
        RdpError::Connect(text)
    }
}

async fn establish(s: &Settings, cert: &CertCheck, clipboard: TextClipboard) -> Result<Established, RdpError> {
    let stream = TcpStream::connect((s.host.as_str(), s.port)).await.map_err(|_| RdpError::Unreachable(format!("{}:{}", s.host, s.port)))?;
    let _ = stream.set_nodelay(true);
    let client_addr = stream.local_addr().map_err(|e| RdpError::Connect(e.to_string()))?;
    let mut framed = ironrdp_tokio::TokioFramed::new(stream);
    let mut connector = ClientConnector::new(connector_config(s), client_addr);
    connector.attach_static_channel(ironrdp_cliprdr::Cliprdr::new(Box::new(clipboard)));

    let should_upgrade = ironrdp_tokio::connect_begin(&mut framed, &mut connector).await.map_err(|e| explain(e, &s.host))?;
    let (initial, leftover) = framed.into_inner();
    let (tls, server_cert) = ironrdp_tls::upgrade(initial, &s.host).await.map_err(|e| RdpError::Connect(format!("secure connection failed: {e}")))?;

    // The certificate is checked before anything secret is sent.
    let info = cert_info(&server_cert)?;
    cert.check(&info)?;

    let upgraded = ironrdp_tokio::mark_as_upgraded(should_upgrade, &mut connector);
    let mut framed = ironrdp_tokio::TokioFramed::new_with_leftover(tls, leftover);
    let public_key = ironrdp_tls::extract_tls_server_public_key(&server_cert).ok_or_else(|| RdpError::Connect("the server certificate has no usable public key".into()))?.to_owned();
    let result = ironrdp_tokio::connect_finalize(upgraded, connector, &mut framed, &mut NoNetwork, ServerName::new(s.host.clone()), public_key, None)
        .await
        .map_err(|e| explain(e, &s.host))?;
    Ok(Established { framed, result, info })
}

// -- the session ---------------------------------------------------------------------------

fn button(n: u8) -> Option<MouseButton> {
    MouseButton::from_web_button(n)
}

fn operations(input: &Input) -> Vec<Operation> {
    match input {
        Input::Key { code, down } => match keys::scancode(code) {
            Some(sc) => vec![if *down { Operation::KeyPressed(sc) } else { Operation::KeyReleased(sc) }],
            None => vec![],
        },
        Input::MouseMove { x, y } => vec![Operation::MouseMove(MousePosition { x: *x, y: *y })],
        Input::Button { button: b, down } => button(*b).map(|b| if *down { Operation::MouseButtonPressed(b) } else { Operation::MouseButtonReleased(b) }).into_iter().collect(),
        Input::Wheel { vertical, units } => vec![Operation::WheelRotations(WheelRotations { is_vertical: *vertical, rotation_units: *units })],
        Input::CtrlAltDel => vec![
            Operation::KeyPressed(keys::CONTROL_LEFT),
            Operation::KeyPressed(keys::ALT_LEFT),
            Operation::KeyPressed(keys::DELETE),
            Operation::KeyReleased(keys::DELETE),
            Operation::KeyReleased(keys::ALT_LEFT),
            Operation::KeyReleased(keys::CONTROL_LEFT),
        ],
        Input::ReleaseAll | Input::ClipboardText { .. } | Input::Close => vec![],
    }
}

fn send_region(sink: &dyn RdpSink, image: &DecodedImage, r: &ironrdp_pdu::geometry::InclusiveRectangle) {
    let (iw, ih) = (usize::from(image.width()), usize::from(image.height()));
    let (left, top) = (usize::from(r.left), usize::from(r.top));
    if left >= iw || top >= ih {
        return;
    }
    let right = usize::from(r.right).min(iw - 1);
    let bottom = usize::from(r.bottom).min(ih - 1);
    let (w, h) = (right - left + 1, bottom - top + 1);
    let data = image.data();
    let mut out = Vec::with_capacity(w * h * 4);
    for row in top..=bottom {
        let start = (row * iw + left) * 4;
        out.extend_from_slice(&data[start..start + w * 4]);
    }
    // Sizes fit in u16: they are inside an image whose sides do.
    sink.frame(left as u16, top as u16, w as u16, h as u16, &out);
}

async fn run_session(
    framed: Stream,
    result: ConnectionResult,
    mut events: mpsc::UnboundedReceiver<Event>,
    sink: &Arc<dyn RdpSink>,
    local_text: Arc<Mutex<Option<String>>>,
) -> Result<(), ironrdp_session::SessionError> {
    use ironrdp_cliprdr::backend::ClipboardMessage;
    use ironrdp_cliprdr::CliprdrClient;

    let (mut reader, mut writer) = split_tokio_framed(framed);
    let size = result.desktop_size;
    let mut image = DecodedImage::new(PixelFormat::RgbA32, size.width, size.height);
    let activation_factory = result.activation_factory;
    let mut stage: ActiveStage = ActiveStageBuilder {
        static_channels: result.static_channels,
        user_channel_id: result.user_channel_id,
        io_channel_id: result.io_channel_id,
        message_channel_id: result.message_channel_id,
        share_id: result.share_id,
        compression_type: result.compression_type,
        enable_server_pointer: result.enable_server_pointer,
        pointer_software_rendering: result.pointer_software_rendering,
    }
    .build();
    let mut db = Database::new();
    let mut tick = tokio::time::interval(Duration::from_secs(5));

    let reason = 'outer: loop {
        let outputs = tokio::select! {
            frame = reader.read_pdu() => {
                let (action, payload) = frame.map_err(|e| ironrdp_session::custom_err!("read frame", e))?;
                stage.process(&mut image, action, &payload)?
            }
            event = events.recv() => {
                let Some(event) = event else { break 'outer None };
                match event {
                    Event::Input(Input::Close) => stage.graceful_shutdown()?,
                    Event::Input(Input::ReleaseAll) => {
                        let events = db.release_all();
                        if events.is_empty() { Vec::new() } else { stage.process_fastpath_input(&mut image, &events)? }
                    }
                    Event::Input(Input::ClipboardText { text }) => {
                        *local_text.lock().unwrap_or_else(|p| p.into_inner()) = Some(text);
                        match stage.get_svc_processor_mut::<CliprdrClient>() {
                            Some(c) => {
                                let formats = [ironrdp_cliprdr::pdu::ClipboardFormat::new(ironrdp_cliprdr::pdu::ClipboardFormatId::CF_UNICODETEXT)];
                                let msgs = c.initiate_copy(&formats).map_err(|e| ironrdp_session::custom_err!("CLIPRDR", e))?;
                                vec![ActiveStageOutput::ResponseFrame(stage.process_svc_processor_messages(msgs)?)]
                            }
                            None => Vec::new(),
                        }
                    }
                    Event::Input(input) => {
                        let events = db.apply(operations(&input));
                        if events.is_empty() { Vec::new() } else { stage.process_fastpath_input(&mut image, &events)? }
                    }
                    Event::Clipboard(message) => {
                        let Some(c) = stage.get_svc_processor_mut::<CliprdrClient>() else { continue };
                        let msgs = match message {
                            ClipboardMessage::SendInitiateCopy(formats) => Some(c.initiate_copy(&formats)),
                            ClipboardMessage::SendFormatData(r) => Some(c.submit_format_data(r)),
                            ClipboardMessage::SendInitiatePaste(f) => Some(c.initiate_paste(f)),
                            _ => None,
                        };
                        match msgs {
                            Some(m) => {
                                let m = m.map_err(|e| ironrdp_session::custom_err!("CLIPRDR", e))?;
                                vec![ActiveStageOutput::ResponseFrame(stage.process_svc_processor_messages(m)?)]
                            }
                            None => Vec::new(),
                        }
                    }
                }
            }
            _ = tick.tick() => {
                match stage.get_svc_processor_mut::<CliprdrClient>().map(|c| c.drive_timeouts()) {
                    Some(Ok(m)) => {
                        let frame = stage.process_svc_processor_messages(m)?;
                        if frame.is_empty() { Vec::new() } else { vec![ActiveStageOutput::ResponseFrame(frame)] }
                    }
                    _ => Vec::new(),
                }
            }
        };

        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => writer.write_all(&frame).await.map_err(|e| ironrdp_session::custom_err!("write", e))?,
                ActiveStageOutput::GraphicsUpdate(region) => send_region(sink.as_ref(), &image, &region),
                ActiveStageOutput::PointerDefault => sink.cursor(Cursor::Default),
                ActiveStageOutput::PointerHidden => sink.cursor(Cursor::Hidden),
                ActiveStageOutput::PointerPosition { .. } => {}
                ActiveStageOutput::PointerBitmap(p) => sink.cursor(Cursor::Bitmap { hot_x: p.hotspot_x, hot_y: p.hotspot_y, width: p.width, height: p.height, rgba: p.bitmap_data.clone() }),
                ActiveStageOutput::DeactivateAll => {
                    // The server is changing the screen (size or colour depth): redo the setup steps.
                    let mut activation = activation_factory.create();
                    let mut buf = WriteBuf::new();
                    loop {
                        let written = single_sequence_step_read(&mut reader, &mut activation, &mut buf).await.map_err(|e| ironrdp_session::custom_err!("reactivation", e))?;
                        if written.size().is_some() {
                            writer.write_all(buf.filled()).await.map_err(|e| ironrdp_session::custom_err!("reactivation write", e))?;
                        }
                        if let ConnectionActivationState::Finalized { desktop_size, share_id, enable_server_pointer, pointer_software_rendering } = activation.connection_activation_state() {
                            image = DecodedImage::new(PixelFormat::RgbA32, desktop_size.width, desktop_size.height);
                            stage.set_fastpath_processor(
                                fast_path::ProcessorBuilder {
                                    io_channel_id: activation.io_channel_id(),
                                    user_channel_id: activation.user_channel_id(),
                                    share_id,
                                    enable_server_pointer,
                                    pointer_software_rendering,
                                    bulk_decompressor: None,
                                }
                                .build(),
                            );
                            stage.set_share_id(share_id);
                            stage.set_enable_server_pointer(enable_server_pointer);
                            sink.size(desktop_size.width, desktop_size.height);
                            break;
                        }
                    }
                }
                ActiveStageOutput::Terminate(r) => break 'outer Some(r),
                _ => {}
            }
        }
    };
    let _ = reason;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(fp: &str) -> CertInfo {
        CertInfo { fingerprint: fp.into(), subject: "CN=s".into(), issuer: "CN=s".into(), not_before: "a".into(), not_after: "b".into() }
    }

    #[test]
    fn an_unknown_certificate_stops_the_connection_until_accepted() {
        let none = CertCheck::default();
        assert!(matches!(none.check(&info("aa")), Err(RdpError::CertificateUnknown(i)) if i.fingerprint == "aa"));
        // Accepting a different fingerprint doesn't let this one through.
        let other = CertCheck { pinned: None, accept: Some("bb".into()) };
        assert!(matches!(other.check(&info("aa")), Err(RdpError::CertificateUnknown(_))));
        let ok = CertCheck { pinned: None, accept: Some("aa".into()) };
        assert!(ok.check(&info("aa")).is_ok());
    }

    #[test]
    fn a_pinned_certificate_must_match_and_a_change_needs_a_fresh_accept() {
        let pinned = CertCheck { pinned: Some("aa".into()), accept: None };
        assert!(pinned.check(&info("aa")).is_ok());
        assert!(matches!(pinned.check(&info("bb")), Err(RdpError::CertificateChanged { expected, found }) if expected == "aa" && found.fingerprint == "bb"));
        // Accepting the old one doesn't help a changed certificate; accepting the new one does.
        let wrong = CertCheck { pinned: Some("aa".into()), accept: Some("aa".into()) };
        assert!(matches!(wrong.check(&info("bb")), Err(RdpError::CertificateChanged { .. })));
        let replaced = CertCheck { pinned: Some("aa".into()), accept: Some("bb".into()) };
        assert!(replaced.check(&info("bb")).is_ok());
    }

    #[test]
    fn accounts_split_into_name_and_domain() {
        assert_eq!(split_account("CORP\\alice", ""), ("alice".into(), Some("CORP".into())));
        assert_eq!(split_account("alice", "CORP"), ("alice".into(), Some("CORP".into())));
        assert_eq!(split_account("OTHER\\alice", " CORP "), ("OTHER\\alice".into(), Some("CORP".into())), "an explicit domain wins");
        assert_eq!(split_account("alice@corp.example", ""), ("alice@corp.example".into(), None));
        assert_eq!(split_account("alice", ""), ("alice".into(), None));
        assert_eq!(split_account("\\alice", ""), ("\\alice".into(), None), "an empty domain isn't one");
        assert_eq!(split_account("CORP\\", ""), ("CORP\\".into(), None));
    }

    #[test]
    fn sizes_are_kept_sane_and_even() {
        assert_eq!(clamp_size(1), MIN_SIZE);
        assert_eq!(clamp_size(1025), 1024);
        assert_eq!(clamp_size(u16::MAX), MAX_SIZE);
        assert!(clamp_size(701).is_multiple_of(2));
    }

    #[test]
    fn settings_become_a_connector_config() {
        let s = Settings { host: "h".into(), port: 3389, username: "u".into(), password: "p".into(), domain: Some("CORP".into()), width: 1280, height: 721, color_depth: 16, security: Security::Nla };
        let c = connector_config(&s);
        assert!(c.enable_credssp && !c.enable_tls, "NLA required means CredSSP only, no downgrade");
        let auto = connector_config(&Settings { security: Security::Auto, ..s.clone() });
        assert!(auto.enable_credssp && auto.enable_tls, "automatic offers both and lets the server pick");
        assert_eq!((c.desktop_size.width, c.desktop_size.height), (1280, 720));
        assert_eq!(c.domain.as_deref(), Some("CORP"));
        assert_eq!(c.bitmap.as_ref().map(|b| b.color_depth), Some(16));
        let off = connector_config(&Settings { security: Security::Tls, domain: Some(String::new()), color_depth: 24, ..s });
        assert!(off.enable_tls && !off.enable_credssp);
        assert_eq!(off.domain, None, "an empty domain is no domain");
        assert_eq!(off.bitmap.as_ref().map(|b| b.color_depth), Some(32), "only 16 and 32 are offered");
    }

    #[test]
    fn input_becomes_operations() {
        assert_eq!(operations(&Input::Key { code: "KeyA".into(), down: true }).len(), 1);
        assert!(operations(&Input::Key { code: "Nope".into(), down: true }).is_empty());
        assert!(operations(&Input::Button { button: 9, down: true }).is_empty(), "a button RDP doesn't have");
        assert_eq!(operations(&Input::Button { button: 2, down: false }).len(), 1);
        let cad = operations(&Input::CtrlAltDel);
        assert_eq!(cad.len(), 6);
        assert!(matches!(cad[0], Operation::KeyPressed(_)) && matches!(cad[5], Operation::KeyReleased(_)));
    }

    #[test]
    fn the_input_database_turns_a_click_into_wire_events() {
        let mut db = Database::new();
        let events = db.apply(operations(&Input::MouseMove { x: 10, y: 20 }).into_iter().chain(operations(&Input::Button { button: 0, down: true })));
        assert_eq!(events.len(), 2);
        // Letting go of everything releases what is held.
        assert!(!db.release_all().is_empty());
        assert!(db.release_all().is_empty());
    }

    #[test]
    fn errors_are_explained() {
        let refused = explain(ironrdp_connector::general_err!("TCP connect: connection refused"), "h");
        assert!(matches!(refused, RdpError::Unreachable(_)));
        let bad = explain(ironrdp_connector::general_err!("CredSSP failed: STATUS_LOGON_FAILURE"), "h");
        assert!(matches!(bad, RdpError::Credentials));
        let other = explain(ironrdp_connector::general_err!("something odd"), "h");
        assert!(matches!(other, RdpError::Connect(m) if m.contains("something odd")));
    }

    // -- against a real RDP server ----------------------------------------------------------------
    //
    // Needs SSHVAULT_RDP_ADDR (host:port), SSHVAULT_RDP_USER, SSHVAULT_RDP_PASS and, for the checks
    // that look inside the server, SSHVAULT_RDP_CONTAINER (a Docker container running xrdp + XFCE,
    // for example scottyhardy/docker-remote-desktop). Skipped without them.
    mod live {
        use super::*;
        use std::process::Command;

        /// One user can only have one connection to a session, so the tests take turns.
        static TURN: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

        struct Live {
            host: String,
            port: u16,
            user: String,
            pass: String,
            container: Option<String>,
        }

        fn live() -> Option<Live> {
            let addr = std::env::var("SSHVAULT_RDP_ADDR").ok()?;
            let (host, port) = addr.rsplit_once(':')?;
            Some(Live {
                host: host.into(),
                port: port.parse().ok()?,
                user: std::env::var("SSHVAULT_RDP_USER").ok()?,
                pass: std::env::var("SSHVAULT_RDP_PASS").ok()?,
                container: std::env::var("SSHVAULT_RDP_CONTAINER").ok(),
            })
        }

        impl Live {
            fn settings(&self) -> Settings {
                Settings { host: self.host.clone(), port: self.port, username: self.user.clone(), password: self.pass.clone(), domain: None, width: 1024, height: 700, color_depth: 32, security: Security::Auto }
            }
            /// Start a program inside the server and leave it running (a clipboard owner has to stay alive).
            fn start(&self, script: &str) {
                let c = self.container.as_deref().expect("SSHVAULT_RDP_CONTAINER");
                let full = format!("cd ~ && export DISPLAY=$(ps -eo args | grep '[X]org :' | sed 's/.*Xorg \\(:[0-9]*\\).*/\\1/' | tail -1) XAUTHORITY=$HOME/.Xauthority && {script}");
                let status = Command::new("docker").args(["exec", "-d", "-u", &self.user, c, "sh", "-c", &full]).status().expect("docker");
                assert!(status.success());
                std::thread::sleep(Duration::from_secs(1));
            }
            /// Run a shell command inside the server as its user, with the desktop's display.
            fn exec(&self, script: &str) -> String {
                let c = self.container.as_deref().expect("SSHVAULT_RDP_CONTAINER");
                let full = format!("cd ~ && export DISPLAY=$(ps -eo args | grep '[X]org :' | sed 's/.*Xorg \\(:[0-9]*\\).*/\\1/' | tail -1) XAUTHORITY=$HOME/.Xauthority && {script}");
                let out = Command::new("docker").args(["exec", "-u", &self.user, c, "sh", "-c", &full]).output().expect("docker");
                String::from_utf8_lossy(&out.stdout).trim().to_string()
            }
        }

        /// Everything a session reports, kept for the test to look at.
        #[derive(Default)]
        struct Seen {
            size: Option<(u16, u16)>,
            canvas: Vec<u8>,
            frames: usize,
            cursors: usize,
            clipboard: Vec<String>,
            ended: Option<Option<String>>,
        }

        #[derive(Clone, Default)]
        struct Collect(Arc<Mutex<Seen>>);

        impl RdpSink for Collect {
            fn size(&self, w: u16, h: u16) {
                let mut s = self.0.lock().unwrap();
                s.size = Some((w, h));
                s.canvas = vec![0; usize::from(w) * usize::from(h) * 4];
            }
            fn frame(&self, x: u16, y: u16, w: u16, h: u16, rgba: &[u8]) {
                let mut s = self.0.lock().unwrap();
                let (cw, _) = s.size.unwrap();
                s.frames += 1;
                for row in 0..usize::from(h) {
                    let dst = ((usize::from(y) + row) * usize::from(cw) + usize::from(x)) * 4;
                    let src = row * usize::from(w) * 4;
                    s.canvas[dst..dst + usize::from(w) * 4].copy_from_slice(&rgba[src..src + usize::from(w) * 4]);
                }
            }
            fn cursor(&self, _c: Cursor) {
                self.0.lock().unwrap().cursors += 1;
            }
            fn clipboard(&self, text: String) {
                self.0.lock().unwrap().clipboard.push(text);
            }
            fn ended(&self, error: Option<String>) {
                self.0.lock().unwrap().ended = Some(error);
            }
        }

        impl Collect {
            fn wait(&self, what: &str, secs: u64, f: impl Fn(&Seen) -> bool) {
                let end = std::time::Instant::now() + Duration::from_secs(secs);
                while std::time::Instant::now() < end {
                    if f(&self.0.lock().unwrap()) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                let s = self.0.lock().unwrap();
                panic!("gave up waiting for {what}; session ended: {:?}, clipboard seen: {:?}", s.ended, s.clipboard);
            }
            fn distinct_colours(&self) -> usize {
                colours(&self.0.lock().unwrap().canvas)
            }
        }

        /// How many different colours a picture has, counted up to 65.
        fn colours(canvas: &[u8]) -> usize {
            let mut seen = std::collections::HashSet::new();
            for px in canvas.as_chunks::<4>().0 {
                seen.insert([px[0], px[1], px[2]]);
                if seen.len() > 64 {
                    break;
                }
            }
            seen.len()
        }

        async fn connect(l: &Live, cert: CertCheck) -> (Arc<RdpManager>, Collect, Result<CertInfo, RdpError>) {
            let manager = Arc::new(RdpManager::new());
            let sink = Collect::default();
            let r = manager.connect("pane".into(), l.settings(), cert, Arc::new(sink.clone())).await;
            (manager, sink, r)
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn the_certificate_is_shown_first_pinned_and_checked_again() {
            let _turn = TURN.lock().await;
            let Some(l) = live() else {
                eprintln!("skipping: SSHVAULT_RDP_ADDR isn't set");
                return;
            };
            // First time: stopped, with the fingerprint to show. Nothing was signed in.
            let (_m, sink, r) = connect(&l, CertCheck::default()).await;
            let e = r.unwrap_err(); let RdpError::CertificateUnknown(info) = e else { panic!("expected an unknown certificate, got: {e:?}") };
            assert_eq!(info.fingerprint.len(), 64);
            assert!(info.fingerprint.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(info.subject.contains("CN="), "{}", info.subject);
            assert!(sink.0.lock().unwrap().size.is_none(), "no session started");

            // Accepted: connects, and says which certificate it was.
            let (m, _sink, r) = connect(&l, CertCheck { pinned: None, accept: Some(info.fingerprint.clone()) }).await;
            assert_eq!(r.unwrap().fingerprint, info.fingerprint);
            m.close("pane");

            // Pinned: connects without asking. A different pin is refused, naming both.
            let (m, _sink, r) = connect(&l, CertCheck { pinned: Some(info.fingerprint.clone()), accept: None }).await;
            assert!(r.is_ok());
            m.close("pane");
            let (_m, _sink, r) = connect(&l, CertCheck { pinned: Some("0".repeat(64)), accept: None }).await;
            match r.unwrap_err() {
                RdpError::CertificateChanged { expected, found } => assert_eq!((expected, found.fingerprint), ("0".repeat(64), info.fingerprint)),
                other => panic!("{other}"),
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn requiring_nla_refuses_a_server_that_only_offers_tls() {
            let _turn = TURN.lock().await;
            let Some(l) = live() else { return };
            // xrdp offers TLS but not NLA: a host set to require NLA must not be quietly downgraded.
            let manager = Arc::new(RdpManager::new());
            let mut s = l.settings();
            s.security = Security::Nla;
            let r = manager.connect("p".into(), s, CertCheck::default(), Arc::new(Collect::default())).await;
            let err = r.unwrap_err();
            assert!(matches!(&err, RdpError::Connect(m) if m.contains("Automatic")), "{err}");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn text_copied_in_the_server_reaches_the_window() {
            let _turn = TURN.lock().await;
            let Some(l) = live() else { return };
            if l.container.is_none() {
                return;
            }
            let (_m0, _s0, r) = connect(&l, CertCheck::default()).await;
            let RdpError::CertificateUnknown(info) = r.unwrap_err() else { panic!() };
            let (m, sink, r) = connect(&l, CertCheck { pinned: Some(info.fingerprint), accept: None }).await;
            r.unwrap();
            sink.wait("the desktop to be drawn", 40, |s| colours(&s.canvas) > 10);
            std::thread::sleep(Duration::from_secs(3));
            l.start("echo -n 'copied in the server' | xclip -i -selection clipboard");
            assert_eq!(l.exec("xclip -o -selection clipboard 2>&1"), "copied in the server", "the text is on the server's clipboard");
            sink.wait("the server's clipboard text", 20, |s| s.clipboard.iter().any(|t| t == "copied in the server"));
            m.close("pane");
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
        async fn a_real_desktop_arrives_and_takes_the_mouse_and_keyboard_and_clipboard() {
            let _turn = TURN.lock().await;
            let Some(l) = live() else { return };
            let (_m0, _s0, r) = connect(&l, CertCheck::default()).await;
            let RdpError::CertificateUnknown(info) = r.unwrap_err() else { panic!() };
            let (m, sink, r) = connect(&l, CertCheck { pinned: Some(info.fingerprint), accept: None }).await;
            r.unwrap();
            assert_eq!(sink.0.lock().unwrap().size, Some((1024, 700)));

            // The desktop is drawn: many colours, not one flat fill.
            sink.wait("the desktop to be drawn", 40, |s| colours(&s.canvas) > 10);
            std::thread::sleep(Duration::from_secs(3));
            assert!(sink.distinct_colours() > 10, "the picture is blank or garbled");

            let Some(_) = l.container else { return m.close("pane") };
            // Mouse and keyboard: open the terminal from the dock, type a command, and see its effect inside the server.
            l.exec("rm -f /tmp/typed-over-rdp");
            let click = |x, y| {
                m.send("pane", Input::MouseMove { x, y }).unwrap();
                m.send("pane", Input::Button { button: 0, down: true }).unwrap();
                m.send("pane", Input::Button { button: 0, down: false }).unwrap();
            };
            click(440, 677);
            std::thread::sleep(Duration::from_secs(5));
            let keys = |text: &str| {
                for ch in text.chars() {
                    let (code, shift) = match ch {
                        'a'..='z' => (format!("Key{}", ch.to_ascii_uppercase()), false),
                        '0'..='9' => (format!("Digit{ch}"), false),
                        ' ' => ("Space".to_string(), false),
                        '/' => ("Slash".to_string(), false),
                        '-' => ("Minus".to_string(), false),
                        '>' => ("Period".to_string(), true),
                        '\n' => ("Enter".to_string(), false),
                        other => panic!("no key for {other:?}"),
                    };
                    if shift {
                        m.send("pane", Input::Key { code: "ShiftLeft".into(), down: true }).unwrap();
                    }
                    m.send("pane", Input::Key { code: code.clone(), down: true }).unwrap();
                    m.send("pane", Input::Key { code, down: false }).unwrap();
                    if shift {
                        m.send("pane", Input::Key { code: "ShiftLeft".into(), down: false }).unwrap();
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
            };
            keys("echo typed over rdp > /tmp/typed-over-rdp\n");
            let end = std::time::Instant::now() + Duration::from_secs(20);
            let mut got = String::new();
            while std::time::Instant::now() < end && got.is_empty() {
                std::thread::sleep(Duration::from_millis(500));
                got = l.exec("cat /tmp/typed-over-rdp 2>/dev/null");
            }
            assert_eq!(got, "typed over rdp", "what was typed reached the server's terminal");

            // Clipboard, window to server: offer text, then read the server's clipboard.
            m.send("pane", Input::ClipboardText { text: "from the window\nsecond line".into() }).unwrap();
            let end = std::time::Instant::now() + Duration::from_secs(15);
            let mut remote = String::new();
            while std::time::Instant::now() < end && !remote.contains("second line") {
                std::thread::sleep(Duration::from_millis(500));
                remote = l.exec("xclip -o -selection clipboard 2>/dev/null");
            }
            assert_eq!(remote.replace('\r', ""), "from the window\nsecond line");

            // Clipboard, server to window.
            l.start("echo -n 'from the server' | xclip -i -selection clipboard");
            sink.wait("the server's clipboard text", 15, |s| s.clipboard.iter().any(|t| t == "from the server"));

            m.close("pane");
            sink.wait("the session to end", 10, |s| s.ended.is_some());
            assert_eq!(sink.0.lock().unwrap().ended, Some(None), "a clean close is not an error");
        }
    }
}
