//! Runbooks: the saved documents, the editor's live check, running them on hosts, and the history of runs.

use super::*;

#[tauri::command]
pub fn list_runbooks(state: State<'_, AppState>) -> ApiResult<Vec<Record<crate::models::SavedRunbook>>> {
    list_records(&state, Collection::Runbooks)
}

#[tauri::command]
pub fn save_runbook(state: State<'_, AppState>, id: Option<Uuid>, base_rev: Option<u64>, runbook: crate::models::SavedRunbook) -> ApiResult<Record<crate::models::SavedRunbook>> {
    if runbook.name.trim().is_empty() {
        return Err(ApiError::new("validation", "a runbook needs a name"));
    }
    if runbook.body.len() > 256 * 1024 {
        return Err(ApiError::new("validation", "that runbook is too large"));
    }
    save_record(&state, Collection::Runbooks, id, base_rev, runbook)
}

#[tauri::command]
pub fn delete_runbook(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    delete_record(&state, Collection::Runbooks, id, base_rev)
}

/// What a runbook's text says: its name, parameters and the problems with it, for the editor to show as you type.
#[derive(Debug, Serialize)]
pub struct RunbookCheck {
    pub name: Option<String>,
    pub description: String,
    pub params: Vec<crate::runbook::Param>,
    pub steps: usize,
    /// How many rollback steps it has (they run only when a run is started with rollback on).
    pub rollback_steps: usize,
    pub problems: Vec<crate::runbook::Problem>,
}

#[tauri::command]
pub fn runbook_check(body: String) -> RunbookCheck {
    match crate::runbook::parse(&body) {
        Ok(rb) => RunbookCheck { name: Some(rb.name), description: rb.description, params: rb.params, steps: rb.steps.len(), rollback_steps: rb.rollback.len(), problems: Vec::new() },
        Err(problems) => {
            // A document that doesn't fully check still shows what it can.
            let loose = serde_json::from_str::<crate::runbook::Runbook>(&body).ok();
            RunbookCheck {
                name: loose.as_ref().map(|r| r.name.clone()),
                description: loose.as_ref().map(|r| r.description.clone()).unwrap_or_default(),
                params: loose.as_ref().map(|r| r.params.clone()).unwrap_or_default(),
                steps: loose.as_ref().map(|r| r.steps.len()).unwrap_or(0),
                rollback_steps: loose.as_ref().map(|r| r.rollback.len()).unwrap_or(0),
                problems,
            }
        }
    }
}

fn runbook_for_run(body: &str, params: &HashMap<String, String>) -> ApiResult<(crate::runbook::Runbook, HashMap<String, String>)> {
    let rb = crate::runbook::parse(body).map_err(|p| ApiError::new("validation", p.iter().map(|p| p.message.clone()).collect::<Vec<_>>().join("; ")))?;
    let text_params: HashMap<String, String> = params.iter().filter(|(k, _)| !rb.params.iter().any(|p| &p.name == *k && p.kind == crate::runbook::ParamKind::File)).map(|(k, v)| (k.clone(), v.clone())).collect();
    let values = crate::runbook::resolve_params(&rb, &text_params).map_err(|e| ApiError::new("validation", e.join("; ")))?;
    Ok((rb, values))
}

/// The steps with everything filled in for one host, without running anything (a dry run).
#[tauri::command]
pub fn runbook_plan(body: String, params: HashMap<String, String>, host: String, label: String) -> ApiResult<Vec<crate::runbook::PlannedStep>> {
    let (rb, values) = runbook_for_run(&body, &params)?;
    crate::runbook::plan(&rb, &values, &HashMap::from([("host".to_string(), host), ("label".to_string(), label)])).map_err(|e| ApiError::new("validation", e))
}

struct ChannelRunbookSink(Channel<crate::runbookrun::RunbookEvent>);
impl crate::runbookrun::RunbookSink for ChannelRunbookSink {
    fn event(&self, e: crate::runbookrun::RunbookEvent) {
        let _ = self.0.send(e);
    }
}

/// Run a runbook on hosts in the background. `files` maps a file parameter to the path the person chose.
/// Hosts that can't connect unattended (no saved credentials) fail at once, without stopping the others.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn runbook_start(
    state: State<'_, AppState>,
    run_id: String,
    body: String,
    params: HashMap<String, String>,
    files: HashMap<String, String>,
    host_ids: Vec<Uuid>,
    scheduled: bool,
    on_event: Channel<crate::runbookrun::RunbookEvent>,
    // Run the runbook's rollback steps on a host where the run fails. Off unless asked for.
    rollback: Option<bool>,
    order: Option<crate::runner::Order>,
) -> ApiResult<()> {
    use crate::runbookrun::{HostJob, MAX_FILE, MAX_FILES_TOTAL};
    if Uuid::parse_str(&run_id).is_err() {
        return Err(ApiError::new("validation", "a run id is a UUID"));
    }
    if host_ids.is_empty() {
        return Err(ApiError::new("validation", "choose at least one host"));
    }
    let (rb, values) = runbook_for_run(&body, &params)?;
    // Every file parameter that is used needs a file; read now so a missing one stops the run before it starts.
    let mut blobs: HashMap<String, Vec<u8>> = HashMap::new();
    let mut total = 0usize;
    for p in rb.params.iter().filter(|p| p.kind == crate::runbook::ParamKind::File) {
        let Some(path) = files.get(&p.name).filter(|f| !f.is_empty()) else {
            if p.optional {
                continue;
            }
            return Err(ApiError::new("validation", format!("choose a file for {}", if p.label.is_empty() { &p.name } else { &p.label })));
        };
        let meta = std::fs::metadata(path).map_err(|e| ApiError::new("io", format!("{path}: {e}")))?;
        if !meta.is_file() || meta.len() as usize > MAX_FILE {
            return Err(ApiError::new("validation", format!("{path} isn't a file of at most {} MB", MAX_FILE / (1024 * 1024))));
        }
        total += meta.len() as usize;
        if total > MAX_FILES_TOTAL {
            return Err(ApiError::new("validation", "the files are too large together"));
        }
        blobs.insert(p.name.clone(), std::fs::read(path).map_err(|e| ApiError::new("io", format!("{path}: {e}")))?);
    }
    let mut jobs = Vec::new();
    for id in host_ids {
        let host: Option<Host> = state.session.with_vault(|v| Ok(v.get::<Host>(Collection::Hosts, id).ok().map(|r| r.data)))?.flatten();
        let Some(host) = host else {
            jobs.push(HostJob { host_id: id, label: id.to_string(), hostname: String::new(), target: Err("that host no longer exists".into()) });
            continue;
        };
        let target = resolve_target(&state, id, None).map_err(|e| e.message);
        jobs.push(HostJob { host_id: id, label: host.label.clone(), hostname: host.hostname.clone(), target });
    }
    let manager = Arc::clone(&state.runbooks);
    let history = state.runbook_history.clone();
    let sink: Arc<dyn crate::runbookrun::RunbookSink> = Arc::new(ChannelRunbookSink(on_event));
    // Spawned inside Tauri's runtime; the manager needs a tokio context.
    tauri::async_runtime::spawn(async move {
        manager.start(run_id, rb, values, blobs, jobs, scheduled, rollback.unwrap_or(false), order.unwrap_or_default(), Some(history), sink);
    });
    Ok(())
}

#[tauri::command]
pub fn runbook_cancel(state: State<'_, AppState>, run_id: String) -> bool {
    state.runbooks.cancel(&run_id)
}

#[tauri::command]
pub fn runbook_history_list(state: State<'_, AppState>) -> Vec<crate::runbookhistory::RunSummary> {
    state.runbook_history.list()
}

#[tauri::command]
pub fn runbook_history_get(state: State<'_, AppState>, id: String) -> Option<crate::runbookhistory::RunRecord> {
    state.runbook_history.get(&id)
}

#[tauri::command]
pub fn runbook_history_delete(state: State<'_, AppState>, id: String) -> bool {
    state.runbook_history.delete(&id)
}

#[tauri::command]
pub fn runbook_history_clear(state: State<'_, AppState>) -> usize {
    state.runbook_history.clear()
}
