//! Operations → Logs: following the journal on several hosts at once. The work is in `crate::opslogs`.

use super::*;

struct ChannelOpsSink(Channel<crate::opslogs::OpsEvent>);
impl crate::opslogs::OpsSink for ChannelOpsSink {
    fn event(&self, e: crate::opslogs::OpsEvent) {
        let _ = self.0.send(e);
    }
}

fn jobs(state: &AppState, host_ids: Vec<Uuid>) -> ApiResult<Vec<crate::opslogs::HostJob>> {
    let mut out = Vec::new();
    for id in host_ids {
        let host: Option<Host> = state.session.with_vault(|v| Ok(v.get::<Host>(Collection::Hosts, id).ok().map(|r| r.data)))?.flatten();
        let Some(host) = host else {
            out.push(crate::opslogs::HostJob { host_id: id, label: id.to_string(), target: Err("That host no longer exists.".into()) });
            continue;
        };
        // Logged in to without asking: a host that needs a password typed can't be followed.
        let target = resolve_target(state, id, None).map_err(|e| e.message);
        out.push(crate::opslogs::HostJob { host_id: id, label: host.label, target });
    }
    Ok(out)
}

/// Follow a log on hosts. Events arrive on `on_event` until `ops_logs_stop`.
#[tauri::command]
pub fn ops_logs_start(
    state: State<'_, AppState>,
    stream_id: String,
    spec: crate::opslogs::Spec,
    host_ids: Vec<Uuid>,
    on_event: Channel<crate::opslogs::OpsEvent>,
) -> ApiResult<()> {
    if Uuid::parse_str(&stream_id).is_err() {
        return Err(ApiError::new("validation", "a stream id is a UUID"));
    }
    let jobs = jobs(&state, host_ids)?;
    let sink: Arc<dyn crate::opslogs::OpsSink> = Arc::new(ChannelOpsSink(on_event));
    let manager = Arc::clone(&state.ops_logs);
    manager.start(stream_id, spec, jobs, sink).map_err(|e| ApiError::new("validation", e))
}

/// Change which hosts a running stream follows: follow `add`, and stop following `remove`.
#[tauri::command]
pub fn ops_logs_hosts(state: State<'_, AppState>, stream_id: String, add: Vec<Uuid>, remove: Vec<Uuid>) -> ApiResult<()> {
    state.ops_logs.remove_hosts(&stream_id, &remove);
    if !add.is_empty() {
        let jobs = jobs(&state, add)?;
        state.ops_logs.add_hosts(&stream_id, jobs).map_err(|e| ApiError::new("validation", e))?;
    }
    Ok(())
}

/// End a stream. Each host reports `stopped` when its remote command has been ended and its connection closed.
#[tauri::command]
pub fn ops_logs_stop(state: State<'_, AppState>, stream_id: String) -> bool {
    state.ops_logs.stop(&stream_id)
}

/// Turn retrying of lost connections on or off for a running stream.
#[tauri::command]
pub fn ops_logs_auto_reconnect(state: State<'_, AppState>, stream_id: String, on: bool) -> bool {
    state.ops_logs.set_auto_reconnect(&stream_id, on)
}

/// Try a failed host again now.
#[tauri::command]
pub fn ops_logs_retry(state: State<'_, AppState>, stream_id: String, host_id: Uuid) -> bool {
    state.ops_logs.retry(&stream_id, host_id)
}

/// The entries just before and after one, from the host's own journal.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn ops_logs_context(
    state: State<'_, AppState>,
    stream_id: String,
    host_id: Uuid,
    cursor: String,
    before: u32,
    after: u32,
    unit: Option<String>,
    redact: Option<bool>,
) -> ApiResult<crate::opslogs::Context> {
    let target = resolve_target(&state, host_id, None).ok();
    state
        .ops_logs
        .context(&stream_id, host_id, target, crate::opslogs::Around { cursor: &cursor, before, after, unit: unit.as_deref(), redact: redact.unwrap_or(true) })
        .await
        .map_err(|e| ApiError::new("logs", e))
}

/// The systemd units on a host, for the unit picker.
#[tauri::command]
pub async fn ops_units(state: State<'_, AppState>, host_id: Uuid) -> ApiResult<Vec<String>> {
    let target = resolve_target(&state, host_id, None)?;
    state.ops_logs.units(host_id, target).await.map_err(|e| ApiError::new("logs", e))
}
