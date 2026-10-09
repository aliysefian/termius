//! Remote desktop (RDP).

use super::*;

/// Pictures and notices for a remote desktop pane, as one byte stream: a tag
/// byte, then 16-bit little-endian numbers and bytes, so a frame costs no JSON.
struct RdpPaneSink(Channel<InvokeResponseBody>);

impl RdpPaneSink {
    fn send(&self, tag: u8, numbers: &[u16], bytes: &[u8]) {
        let mut out = Vec::with_capacity(1 + numbers.len() * 2 + bytes.len());
        out.push(tag);
        for n in numbers {
            out.extend_from_slice(&n.to_le_bytes());
        }
        out.extend_from_slice(bytes);
        let _ = self.0.send(InvokeResponseBody::Raw(out));
    }
}

impl crate::rdp::RdpSink for RdpPaneSink {
    fn size(&self, width: u16, height: u16) {
        self.send(1, &[width, height], &[]);
    }
    fn frame(&self, x: u16, y: u16, width: u16, height: u16, rgba: &[u8]) {
        self.send(0, &[x, y, width, height], rgba);
    }
    fn cursor(&self, cursor: crate::rdp::Cursor) {
        match cursor {
            crate::rdp::Cursor::Default => self.send(2, &[], &[]),
            crate::rdp::Cursor::Hidden => self.send(3, &[], &[]),
            crate::rdp::Cursor::Bitmap { hot_x, hot_y, width, height, rgba } => self.send(4, &[hot_x, hot_y, width, height], &rgba),
        }
    }
    fn clipboard(&self, text: String) {
        self.send(5, &[], text.as_bytes());
    }
    fn ended(&self, error: Option<String>) {
        self.send(6, &[], error.unwrap_or_default().as_bytes());
    }
}

/// What the window learns when the desktop is up.
#[derive(Debug, Clone, Serialize)]
pub struct RdpConnected {
    pub fingerprint: String,
    /// The certificate was pinned by this connection (first use, or a change the person accepted).
    pub pinned_now: bool,
}

/// Open a remote desktop in a pane. The server's certificate is checked before
/// any credentials are sent: an unknown or changed one comes back as an error
/// (`rdp_certificate_unknown`, `rdp_certificate_changed`, with the details) and
/// the window asks; asking again with `accept` set to its fingerprint goes ahead
/// and pins it to the host.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn rdp_connect(
    state: State<'_, AppState>,
    pane_id: String,
    host_id: Uuid,
    width: u16,
    height: u16,
    credentials: Option<Credentials>,
    accept: Option<String>,
    on_event: Channel<InvokeResponseBody>,
) -> ApiResult<RdpConnected> {
    let host: Host = state
        .session
        .with_vault(|v| Ok(v.get::<Host>(Collection::Hosts, host_id)?.data))?
        .ok_or_else(|| ApiError::new("not_found", "no such host"))?;
    let options = host.rdp.clone().unwrap_or_default();
    // The sign-in is a user name and password; keys, jump hosts and proxies are SSH's.
    let target = resolve_target(&state, host_id, credentials)?;
    let AuthMethod::Password { password } = target.auth.clone() else {
        return Err(ApiError::new("validation", "Remote Desktop signs in with a password; attach a credential that has one"));
    };
    if target.jump.is_some() || target.proxy.is_some() {
        return Err(ApiError::new("validation", "Remote Desktop connects directly; it doesn't use jump hosts or proxies yet"));
    }
    let (username, domain) = crate::rdp::split_account(&target.username, &options.domain);
    let settings = crate::rdp::Settings {
        host: host.hostname.clone(),
        port: host.port,
        username,
        password,
        domain,
        width: if options.width == 0 { width } else { options.width },
        height: if options.height == 0 { height } else { options.height },
        color_depth: options.color_depth,
        security: options.security,
    };
    let pinned = Some(options.cert_sha256.clone()).filter(|p| !p.is_empty());
    let check = crate::rdp::CertCheck { pinned: pinned.clone(), accept };
    let info = state.rdp.connect(pane_id, settings, check, Arc::new(RdpPaneSink(on_event))).await?;
    let pinned_now = pinned.as_deref() != Some(info.fingerprint.as_str());
    if pinned_now {
        // Only reached when the person accepted this very certificate.
        let fingerprint = info.fingerprint.clone();
        state.session.with_vault(|v| crate::keymanager::pin_rdp_certificate(v, host_id, &fingerprint))?;
    }
    Ok(RdpConnected { fingerprint: info.fingerprint, pinned_now })
}

#[tauri::command]
pub fn rdp_input(state: State<'_, AppState>, pane_id: String, input: crate::rdp::Input) -> ApiResult<()> {
    Ok(state.rdp.send(&pane_id, input)?)
}

#[tauri::command]
pub fn rdp_close(state: State<'_, AppState>, pane_id: String) {
    state.rdp.close(&pane_id);
}

/// Open a VNC desktop in a pane: through an SSH connection to the same machine by default (so the picture
/// and the password don't cross the network in the clear), or straight to the VNC port. A server that wants a
/// password comes back as `vnc_password_required` (or `vnc_bad_password`) and the window asks.
#[tauri::command]
pub async fn vnc_connect(
    state: State<'_, AppState>,
    pane_id: String,
    host_id: Uuid,
    password: Option<String>,
    credentials: Option<Credentials>,
    on_event: Channel<InvokeResponseBody>,
) -> ApiResult<()> {
    let host: Host = state
        .session
        .with_vault(|v| Ok(v.get::<Host>(Collection::Hosts, host_id)?.data))?
        .ok_or_else(|| ApiError::new("not_found", "no such host"))?;
    let options = host.vnc.clone().unwrap_or_default();
    let password = password.filter(|p| !p.is_empty());
    let sink = Arc::new(RdpPaneSink(on_event));
    if options.ssh_tunnel {
        let mut target = resolve_target(&state, host_id, credentials)?;
        // A VNC host's own port is the VNC port; the SSH one is in its VNC settings.
        target.port = options.ssh_port;
        let (client, _) = crate::ssh::open_client(&target, None).await?;
        let channel = client
            .channel_open_direct_tcpip("localhost".to_string(), u32::from(host.port), "127.0.0.1".to_string(), 0)
            .await
            .map_err(|e| ApiError::new("vnc_unreachable", format!("the SSH server wouldn't open a connection to the VNC server on port {}: {e}", host.port)))?;
        state.vnc.connect(pane_id, channel.into_stream(), password, sink, Some(Box::new(client))).await?;
    } else {
        let address = format!("{}:{}", host.hostname, host.port);
        let stream = tokio::time::timeout(std::time::Duration::from_secs(15), tokio::net::TcpStream::connect((host.hostname.as_str(), host.port)))
            .await
            .map_err(|_| crate::vnc::VncError::Unreachable(address.clone()))?
            .map_err(|_| crate::vnc::VncError::Unreachable(address))?;
        let _ = stream.set_nodelay(true);
        state.vnc.connect(pane_id, stream, password, sink, None).await?;
    }
    Ok(())
}

#[tauri::command]
pub fn vnc_input(state: State<'_, AppState>, pane_id: String, input: crate::display::Input) -> ApiResult<()> {
    Ok(state.vnc.send(&pane_id, input)?)
}

#[tauri::command]
pub fn vnc_close(state: State<'_, AppState>, pane_id: String) {
    state.vnc.close(&pane_id);
}

/// What a cloud or tool's own command-line program lists (JSON), for the window to turn into hosts. Only the
/// programs in `inventory.rs` are run, with values it has checked.
#[tauri::command]
pub async fn inventory_run(source: crate::inventory::Source, option: Option<String>) -> ApiResult<String> {
    Ok(crate::inventory::run(source, option.as_deref()).await?)
}

/// SSH servers on a private network: addresses that answer on `port` with an SSH banner.
#[tauri::command]
pub async fn inventory_scan(range: String, port: u16) -> ApiResult<Vec<crate::inventory::ScanHit>> {
    crate::inventory::scan_ssh(&range, port, std::time::Duration::from_millis(800)).await.map_err(|e| ApiError::new("scan", e.to_string()))
}

/// Run a hook command on this computer. The window asks first, but the window is not who decides: a command that
/// this computer hasn't approved is put to the person in a native dialog the page can't click, and the answer is
/// remembered for that exact text. Without it, anything running in the page could run commands as the user.
#[tauri::command]
pub async fn run_hook(app: AppHandle, state: State<'_, AppState>, command: String, timeout_secs: u64) -> ApiResult<crate::hooks::HookResult> {
    crate::hooks::check(&command).map_err(|e| ApiError::new("hook", e))?;
    // A separate namespace from ProxyCommands: approving one text as a proxy doesn't approve it as a hook.
    let key = format!("hook:{}", command.trim());
    if !crate::proxyapproval::is_approved(&key) {
        let text = format!("SSHVault wants to run this command on this computer:\n\n{}\n\nOnly allow commands you wrote or have read.", command.trim());
        let asker = app.clone();
        let allowed = tauri::async_runtime::spawn_blocking(move || {
            use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
            asker
                .dialog()
                .message(text)
                .title("Allow this command?")
                .buttons(MessageDialogButtons::OkCancelCustom("Allow".to_string(), "Don't allow".to_string()))
                .blocking_show()
        })
        .await
        .unwrap_or(false);
        if !allowed {
            return Err(ApiError::new("hook", "the command was not allowed to run on this computer"));
        }
        let all = crate::proxyapproval::set(&key, true);
        let mut cfg = AppConfig::load(&state.config_dir)?;
        cfg.approved_commands = all;
        cfg.save(&state.config_dir)?;
    }
    crate::hooks::run(&command, std::time::Duration::from_secs(timeout_secs.clamp(1, crate::hooks::MAX_TIMEOUT_SECS))).await.map_err(|e| ApiError::new("hook", e))
}

/// Send a Wake-on-LAN packet for `mac` (to `broadcast`, or the whole local network when empty).
#[tauri::command]
pub fn wake_on_lan(mac: String, broadcast: String) -> ApiResult<()> {
    let to = crate::wol::destination(&broadcast)?;
    Ok(crate::wol::wake(&mac, to)?)
}
