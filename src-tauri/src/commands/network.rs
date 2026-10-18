//! Network ergonomics: the route of a connected session, switching its agent forwarding, seeing the signatures a
//! remote host asks for, and diagnosing a connection.

use super::*;

use crate::agentgate::{ForwardState, Request, SignObserver, SignRequest};
use crate::diagnose::{self, Place, Subject};
use crate::ssh::{HopInfo, HopRtt};

/// Event: a host the agent is forwarded to asked this computer for a signature. Payload: [`AgentSignatureEvent`].
pub const EVENT_AGENT_SIGNATURE: &str = "agent:signature";

#[derive(Debug, Clone, Serialize)]
pub struct AgentSignatureEvent {
    pub pane_id: String,
    /// The saved host's name.
    pub host: String,
    /// The server that asked (`host` or `host:port`).
    pub origin: String,
    pub fingerprint: String,
    /// The vault key's name, when the key is one of the vault's.
    pub key_name: Option<String>,
    /// Unix seconds.
    pub at: u64,
    /// The person said no (only when "ask before a forwarded agent signs" is on).
    pub refused: bool,
}

/// Tells the webview of each signature a forwarded agent is asked for, and, when the vault's setting says so, asks
/// the person first.
pub struct SignatureNotifier {
    pub app: AppHandle,
    pub pane_id: String,
    pub host: String,
}

fn vault_key_named(state: &AppState, fingerprint: &str) -> Option<crate::models::SshKey> {
    state
        .session
        .with_vault(|v| Ok(v.list::<crate::models::SshKey>(Collection::Keys)?.records))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|r| r.data)
        .find(|k| k.fingerprint == fingerprint)
}

fn approval_required(state: &AppState) -> bool {
    state
        .session
        .with_vault(|v| Ok(v.get::<crate::models::VaultSettings>(Collection::Settings, crate::models::SETTINGS_ID).ok().and_then(|r| r.data)))
        .ok()
        .flatten()
        .is_some_and(|s| s.forwarded_agent_approval)
}

impl SignObserver for SignatureNotifier {
    fn signing(&self, request: SignRequest) -> crate::agent::BoxFuture<bool> {
        let (app, pane_id, host) = (self.app.clone(), self.pane_id.clone(), self.host.clone());
        Box::pin(async move {
            let state = app.state::<AppState>();
            let key = vault_key_named(&state, &request.fingerprint);
            let key_name = key.as_ref().map(|k| k.name.clone());
            // A key set to "ask" is asked about by the vault agent itself; asking here too would be two prompts for
            // one signature.
            let asked_anyway = key.as_ref().is_some_and(|k| matches!(k.agent, crate::models::AgentUse::Ask));
            let allowed = if approval_required(&state) && !asked_anyway {
                ask_signature(&app, key_name.clone().unwrap_or_else(|| request.fingerprint.clone()), request.fingerprint.clone(), Some(request.origin.clone())).await
            } else {
                true
            };
            let _ = app.emit(
                EVENT_AGENT_SIGNATURE,
                AgentSignatureEvent { pane_id, host, origin: request.origin, fingerprint: request.fingerprint, key_name, at: request.at, refused: !allowed },
            );
            allowed
        })
    }
}

/// The hops of a connected session: its jump hosts outermost first, then the host. Empty for anything else.
#[tauri::command]
pub fn ssh_hops(state: State<'_, AppState>, pane_id: String) -> Vec<HopInfo> {
    state.ssh.hops(&pane_id)
}

/// A fresh round-trip measurement of every hop of a connected session.
#[tauri::command]
pub async fn ssh_hop_rtts(state: State<'_, AppState>, pane_id: String) -> ApiResult<Vec<HopRtt>> {
    Ok(state.ssh.probe_hops(&pane_id).await)
}

/// Whether a session forwards the agent, and if so whether it is on now. `None` for a session that doesn't.
#[tauri::command]
pub fn ssh_agent_forwarding(state: State<'_, AppState>, pane_id: String) -> Option<ForwardState> {
    state.ssh.agent_gate(&pane_id).map(|g| g.status())
}

/// Switch a session's agent forwarding: `"off"`, `"on"`, or `"timed"` (on, then off by itself after `secs`, five
/// minutes by default). Async because a timed grant starts a timer.
#[tauri::command]
pub async fn ssh_agent_forwarding_set(state: State<'_, AppState>, pane_id: String, mode: String, secs: Option<u64>) -> ApiResult<ForwardState> {
    let gate = state
        .ssh
        .agent_gate(&pane_id)
        .ok_or_else(|| ApiError::new("not_found", "this session does not forward the agent"))?;
    let request = match mode.as_str() {
        "off" => Request::Off,
        "on" => Request::On,
        "timed" => Request::Timed(std::time::Duration::from_secs(secs.unwrap_or(300).clamp(10, 3600))),
        other => return Err(ApiError::new("validation", format!("unknown mode \"{other}\""))),
    };
    gate.set(request);
    Ok(gate.status())
}

/// Hosts' diagnoses never ask about host keys (nobody is watching a dialog) and never forward the agent.
fn quiet(target: &mut crate::ssh::Target) {
    target.host_keys.prompter = None;
    target.agent_gate = None;
    target.forward_agent = false;
    if let Some(j) = target.jump.as_deref_mut() {
        quiet(j);
    }
}

/// Diagnose the way to a host: name, TCP, SSH handshake, route, and (with its stored credentials) the login. Each
/// stage arrives on `on_event` as it finishes; the verdict is returned.
#[tauri::command]
pub async fn diagnose_host(state: State<'_, AppState>, host_id: Uuid, on_event: Channel<diagnose::Event>) -> ApiResult<diagnose::Verdict> {
    let route = state
        .session
        .with_vault(|v| Ok(crate::keymanager::route_of(v, host_id)))?
        .map_err(resolve_api_error)?;
    let place = |s: &crate::keymanager::RouteStop| Place { host: s.hostname.clone(), port: s.port, label: s.label.clone() };
    let first = route.jumps.first();
    let subject = Subject {
        target: place(&route.target),
        first_hop: place(first.unwrap_or(&route.target)),
        via_bastion: first.is_some(),
        proxy: route.first_hop_proxy.clone(),
        legacy_algorithms: first.unwrap_or(&route.target).legacy_algorithms,
        trace_route: true,
    };
    // The login is tried only if the host has what it needs stored; a host that wants a password typed is reachable
    // as far as the handshake, and the verdict says the login was not tried.
    let login = resolve_target(&state, host_id, None).ok().map(|mut t| {
        quiet(&mut t);
        t
    });
    Ok(diagnose::run(subject, login, move |e| {
        let _ = on_event.send(e);
    })
    .await)
}
