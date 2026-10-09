//! Port forwarding.

use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct ForwardStatusEvent {
    pub rule_id: Uuid,
    pub status: ForwardStatus,
}

struct EventStatusSink(AppHandle);
impl StatusSink for EventStatusSink {
    fn status(&self, rule_id: Uuid, status: ForwardStatus) {
        let _ = self
            .0
            .emit(EVENT_FORWARD_STATUS, ForwardStatusEvent { rule_id, status });
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
    if rule.label.trim().is_empty() {
        return Err(ApiError::new("validation", "label is required"));
    }
    save_record(&state, Collection::Forwards, id, base_rev, rule)
}

#[tauri::command]
pub fn delete_forward(
    state: State<'_, AppState>,
    id: Uuid,
    base_rev: Option<u64>,
) -> ApiResult<()> {
    check_unchanged(&state, Collection::Forwards, id, base_rev)?;
    state.forwards.stop(id);
    delete_record(&state, Collection::Forwards, id, None)
}

#[tauri::command]
pub async fn forward_start(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> ApiResult<()> {
    let rule = state
        .session
        .with_vault(|v| v.get::<ForwardRule>(Collection::Forwards, id))?
        .data
        .ok_or_else(|| ApiError::new("not_found", "rule has no data"))?;
    let target = resolve_target(&state, rule.host_id, None)?;
    state
        .forwards
        .start(id, target, rule.kind, Arc::new(EventStatusSink(app)))?;
    Ok(())
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
