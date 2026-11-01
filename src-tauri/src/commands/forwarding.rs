//! Port forwarding.

use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct ForwardStatusEvent {
    pub rule_id: Uuid,
    pub status: ForwardStatus,
}

/// Event name for a line written to a tunnel's log. Payload: [`ForwardLogEvent`].
pub const EVENT_FORWARD_LOG: &str = "forward:log";

#[derive(Debug, Clone, Serialize)]
pub struct ForwardLogEvent {
    pub rule_id: Uuid,
    pub entry: crate::forward::ForwardLog,
}

struct EventStatusSink(AppHandle);
impl StatusSink for EventStatusSink {
    fn status(&self, rule_id: Uuid, status: ForwardStatus) {
        let _ = self
            .0
            .emit(EVENT_FORWARD_STATUS, ForwardStatusEvent { rule_id, status });
    }
    fn log(&self, rule_id: Uuid, entry: &crate::forward::ForwardLog) {
        let _ = self.0.emit(EVENT_FORWARD_LOG, ForwardLogEvent { rule_id, entry: entry.clone() });
    }
}

#[tauri::command]
pub fn list_forwards(state: State<'_, AppState>) -> ApiResult<Vec<Record<ForwardRule>>> {
    list_records(&state, Collection::Forwards)
}

#[tauri::command]
pub fn save_forward(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    rule: ForwardRule,
) -> ApiResult<Record<ForwardRule>> {
    // Checked here as well as in the form: the backend is the one that must refuse a rule that can't work.
    let others: Vec<(Uuid, ForwardRule)> = state.session.with_vault(|v| {
        Ok(v.list::<ForwardRule>(Collection::Forwards)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect())
    })?;
    crate::forward::validate_rule(&rule, id, &others).map_err(|e| ApiError::new("validation", e))?;
    save_record(&state, Collection::Forwards, id, base_rev, rule)
}

#[tauri::command]
pub async fn delete_forward(
    state: State<'_, AppState>,
    id: Uuid,
    base_rev: Option<u64>,
) -> ApiResult<()> {
    check_unchanged(&state, Collection::Forwards, id, base_rev)?;
    state.forwards.stop_and_wait(id, std::time::Duration::from_secs(5)).await;
    state.forwards.forget(id);
    delete_record(&state, Collection::Forwards, id, None)
}

#[tauri::command]
pub async fn forward_start(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> ApiResult<()> {
    start_saved_rule(app, &state, id)
}

/// Stop a rule, wait until its listener is really gone, and start it again: one operation, so the port is free
/// for the new start and the two never overlap.
#[tauri::command]
pub async fn forward_restart(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> ApiResult<()> {
    state.forwards.stop_and_wait(id, std::time::Duration::from_secs(5)).await;
    start_saved_rule(app, &state, id)
}

/// A tunnel's diagnostic log, oldest line first. Secrets are removed before a line is kept.
#[tauri::command]
pub fn forward_logs(state: State<'_, AppState>, id: Uuid) -> Vec<crate::forward::ForwardLog> {
    state.forwards.logs(id)
}

/// What the active tunnels know about themselves (when each became active).
#[tauri::command]
pub fn forward_runtime(state: State<'_, AppState>) -> std::collections::HashMap<Uuid, crate::forward::ForwardRuntime> {
    state.forwards.runtime()
}

fn start_saved_rule(app: AppHandle, state: &AppState, id: Uuid) -> ApiResult<()> {
    let rule = state
        .session
        .with_vault(|v| v.get::<ForwardRule>(Collection::Forwards, id))?
        .data
        .ok_or_else(|| ApiError::new("not_found", "rule has no data"))?;
    let target = resolve_target(state, rule.host_id, None)?;
    state
        .forwards
        .start(id, target, rule.kind, Arc::new(EventStatusSink(app)))?;
    Ok(())
}

/// Start a forward that is not a saved rule (a port spotted in a terminal), through `host_id`. The returned id is
/// what `forward_stop` and the `forward:status` events use; nothing is written to the vault, so it is gone when the
/// app closes or it is stopped.
#[tauri::command]
pub async fn forward_start_adhoc(app: AppHandle, state: State<'_, AppState>, host_id: Uuid, kind: crate::models::ForwardKind) -> ApiResult<Uuid> {
    let target = resolve_target(&state, host_id, None)?;
    let id = Uuid::new_v4();
    state.forwards.start(id, target, kind, Arc::new(EventStatusSink(app)))?;
    Ok(id)
}

#[tauri::command]
pub fn forward_stop(state: State<'_, AppState>, id: Uuid) {
    state.forwards.stop(id);
}

#[tauri::command]
pub fn forward_statuses(
    state: State<'_, AppState>,
) -> std::collections::HashMap<Uuid, ForwardStatus> {
    state.forwards.statuses()
}

/// What an import did, for the person to read: how many tunnels were added, and which were left out and why.
#[derive(Debug, Clone, Serialize, Default)]
pub struct TunnelImportReport {
    pub added: usize,
    pub skipped: Vec<String>,
}

/// Write every tunnel whose host still exists to `path` (see `tunnelfile`). Returns how many were written.
#[tauri::command]
pub fn export_tunnels(state: State<'_, AppState>, path: String) -> ApiResult<usize> {
    let rules: Vec<ForwardRule> = list_records::<ForwardRule>(&state, Collection::Forwards)?.into_iter().filter_map(|r| r.data).collect();
    let hosts: std::collections::HashMap<Uuid, String> = list_records::<Host>(&state, Collection::Hosts)?.into_iter().filter_map(|r| r.data.map(|d| (r.id, d.label))).collect();
    let text = crate::tunnelfile::export(&rules, |r| hosts.get(&r.host_id).cloned());
    let count = text.matches("\"host\":").count();
    std::fs::write(&path, text).map_err(|e| ApiError::new("io", format!("{path}: {e}")))?;
    Ok(count)
}

/// Add the tunnels in a file written by `export_tunnels`. A tunnel whose host isn't here, that is already here, or
/// that wouldn't be valid is skipped and named in the report; the rest are added, stopped.
#[tauri::command]
pub fn import_tunnels(state: State<'_, AppState>, path: String) -> ApiResult<TunnelImportReport> {
    let text = std::fs::read_to_string(&path).map_err(|e| ApiError::new("io", format!("{path}: {e}")))?;
    let entries = crate::tunnelfile::parse(&text).map_err(|e| ApiError::new("validation", e))?;
    let hosts: std::collections::HashMap<String, Uuid> = list_records::<Host>(&state, Collection::Hosts)?.into_iter().filter_map(|r| r.data.map(|d| (d.label, r.id))).collect();
    let mut existing: Vec<(Uuid, ForwardRule)> = list_records::<ForwardRule>(&state, Collection::Forwards)?.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
    let mut report = TunnelImportReport::default();
    for e in entries {
        let Some(&host_id) = hosts.get(&e.host) else {
            report.skipped.push(format!("{}: there is no host called \"{}\" here", e.name, e.host));
            continue;
        };
        let rule = ForwardRule { label: e.name.clone(), host_id, auto_start: e.auto_start, description: e.description, kind: e.kind };
        if existing.iter().any(|(_, o)| o.host_id == rule.host_id && o.kind == rule.kind) {
            report.skipped.push(format!("{}: already here", e.name));
            continue;
        }
        if let Err(why) = crate::forward::validate_rule(&rule, None, &existing) {
            report.skipped.push(format!("{}: {why}", e.name));
            continue;
        }
        let rec = save_record(&state, Collection::Forwards, None, None, rule.clone())?;
        existing.push((rec.id, rule));
        report.added += 1;
    }
    Ok(report)
}
