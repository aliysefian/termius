//! Run a command on many hosts.

use super::*;

#[derive(Debug, Clone, Deserialize)]
pub struct RunJob {
    pub host_id: Uuid,
    pub command: String,
}

struct ChannelRunSink(Channel<crate::runner::RunEvent>);
impl crate::runner::RunSink for ChannelRunSink {
    fn event(&self, e: crate::runner::RunEvent) {
        let _ = self.0.send(e);
    }
}

/// Run each job's command on its host in the background. Hosts that can't
/// connect unattended (no saved credentials) fail immediately.
#[tauri::command]
pub fn run_on_hosts(
    state: State<'_, AppState>,
    run_id: String,
    jobs: Vec<RunJob>,
    timeout_secs: u64,
    on_event: Channel<crate::runner::RunEvent>,
    order: Option<crate::runner::Order>,
) -> ApiResult<()> {
    use crate::runner::{RunEvent, RunSink};
    let sink: Arc<dyn RunSink> = Arc::new(ChannelRunSink(on_event));
    let mut ready = Vec::new();
    for job in jobs {
        match resolve_target(&state, job.host_id, None) {
            Ok(t) => ready.push((job.host_id, t, job.command)),
            Err(e) => sink.event(RunEvent::Failed {
                host_id: job.host_id,
                message: e.message,
            }),
        }
    }
    let timeout = std::time::Duration::from_secs(timeout_secs.clamp(1, 3600));
    // Spawned inside Tauri's runtime; RunManager needs a tokio context.
    let runs = Arc::clone(&state.runs);
    tauri::async_runtime::spawn(async move {
        runs.start_ordered(run_id, ready, timeout, order.unwrap_or_default(), sink);
    });
    Ok(())
}

#[tauri::command]
pub fn run_cancel(state: State<'_, AppState>, run_id: String) -> bool {
    state.runs.cancel(&run_id)
}

/// A jump host on the way to an unsaved host (quick connect's `ssh -J`).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AdhocHop {
    /// A saved host, with its own credentials, proxy and jump chain.
    Saved { host_id: Uuid },
    /// An unsaved hop, authenticated with ssh-agent.
    Unsaved {
        hostname: String,
        port: u16,
        username: String,
    },
}

/// Chain `jumps` (outermost first) into the hop the final host tunnels
/// through. A saved hop keeps its own jump chain only when it is outermost,
/// matching `ssh -J a,b`, where the route is exactly what was typed.
fn adhoc_jump_chain(state: &AppState, jumps: Vec<AdhocHop>) -> ApiResult<Option<Box<Target>>> {
    let mut outer: Option<Box<Target>> = None;
    for hop in jumps {
        let mut t = match hop {
            AdhocHop::Saved { host_id } => resolve_target(state, host_id, None)?,
            AdhocHop::Unsaved {
                hostname,
                port,
                username,
            } => {
                if hostname.trim().is_empty() || username.trim().is_empty() {
                    return Err(ApiError::new("validation", "jump hosts need a user and host"));
                }
                Target {
                    hostname: hostname.trim().to_string(),
                    port,
                    username: username.trim().to_string(),
                    auth: AuthMethod::Agent,
                    host_keys: state.host_keys.clone(),
                    jump: None,
                    forward_agent: false,
                    agent_backend: None,
                    forward_x11: false,
                    proxy: None,
                    keepalive_secs: None,
                    legacy_algorithms: false,
                }
            }
        };
        if outer.is_some() {
            t.jump = outer;
            // Only the outermost hop is dialled directly.
            t.proxy = None;
        }
        outer = Some(Box::new(t));
    }
    Ok(outer)
}

/// Connect a pane to an unsaved `user@host:port`, optionally through jump
/// hosts. Without a password the local ssh-agent is used.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ssh_connect_adhoc(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    hostname: String,
    port: u16,
    username: String,
    password: Option<String>,
    jumps: Option<Vec<AdhocHop>>,
    cols: u32,
    rows: u32,
    on_data: Channel<InvokeResponseBody>,
) -> ApiResult<()> {
    if hostname.trim().is_empty() || username.trim().is_empty() {
        return Err(ApiError::new("validation", "user and host are required"));
    }
    let jump = adhoc_jump_chain(&state, jumps.unwrap_or_default())?;
    let auth = match password {
        Some(p) if !p.is_empty() => AuthMethod::Password { password: p },
        _ => AuthMethod::Agent,
    };
    let params = ConnectParams {
        target: Target {
            hostname: hostname.trim().to_string(),
            port,
            username: username.trim().to_string(),
            auth,
            host_keys: state.host_keys.clone(),
            jump,
            forward_agent: false,
            agent_backend: None,
            forward_x11: false,
            proxy: None,
            keepalive_secs: None,
            legacy_algorithms: false,
        },
        cols: cols.max(2),
        rows: rows.max(1),
    };
    let sink = Arc::new(PaneSink {
        app,
        pane_id: pane_id.clone(),
        data: on_data,
        log: state.log_slot(&pane_id),
    });
    state.ssh.connect(pane_id, params, sink)?;
    Ok(())
}
