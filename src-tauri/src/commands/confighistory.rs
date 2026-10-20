//! The Config Time-Machine: keep a copy of a remote file from before it is edited, and read the copies back.

use super::*;
use crate::confighistory::{now_ms, read_remote, valid_path, Meta, RemoteRead, Snapshot, Stored, TrackedFile, Usage};

/// What happened to a request to keep a file.
#[derive(Debug, Clone, Serialize)]
pub struct CaptureOutcome {
    /// `saved`, `unchanged` (the newest version already has this content), `skipped` or `failed`.
    pub status: &'static str,
    pub reason: Option<String>,
    pub snapshot: Option<Snapshot>,
}

impl CaptureOutcome {
    fn skipped(reason: impl Into<String>) -> Self {
        Self { status: "skipped", reason: Some(reason.into()), snapshot: None }
    }
    fn failed(reason: impl Into<String>) -> Self {
        Self { status: "failed", reason: Some(reason.into()), snapshot: None }
    }
}

fn key(state: &AppState) -> ApiResult<crate::crypto::MasterKey> {
    Ok(state.session.with_vault(|v| Ok(v.master_key().clone()))?)
}

fn check_path(path: &str) -> ApiResult<()> {
    if valid_path(path) {
        Ok(())
    } else {
        Err(ApiError::new("validation", "a remote path is absolute or starts with ~/"))
    }
}

fn keep(state: &AppState, host_id: Uuid, path: &str, content: &[u8], meta: Meta) -> CaptureOutcome {
    let k = match key(state) {
        Ok(k) => k,
        Err(e) => return CaptureOutcome::failed(e.message),
    };
    match state.config_history.store(&k, host_id, path, content, meta, now_ms()) {
        Ok(Stored::Saved(s)) => CaptureOutcome { status: "saved", reason: None, snapshot: Some(s) },
        Ok(Stored::Unchanged(s)) => CaptureOutcome { status: "unchanged", reason: None, snapshot: Some(s) },
        Err(e) => CaptureOutcome::failed(e.to_string()),
    }
}

/// Read a file over an extra channel of a pane's connection (the shell is not touched) and keep it as a version.
/// A file that does not exist yet, is too big, binary or unreadable is skipped, with the reason.
#[tauri::command]
pub async fn config_capture(
    state: State<'_, AppState>,
    pane_id: String,
    host_id: Uuid,
    path: String,
    trigger: String,
    ssh_user: String,
    max_bytes: u64,
) -> ApiResult<CaptureOutcome> {
    check_path(&path)?;
    let Some(client) = state.ssh.client_of(&pane_id) else {
        return Ok(CaptureOutcome::skipped("this pane is not a connected SSH session"));
    };
    let max = max_bytes.clamp(1, 64 * 1024 * 1024);
    Ok(match read_remote(&client, &path, max).await {
        RemoteRead::Missing => CaptureOutcome::skipped("the file does not exist yet"),
        RemoteRead::TooBig(n) => CaptureOutcome::skipped(format!("the file is {n} bytes, over the limit")),
        RemoteRead::Binary => CaptureOutcome::skipped("the file is not text"),
        RemoteRead::Denied => CaptureOutcome::skipped("the file can't be read, even with sudo -n"),
        RemoteRead::Unavailable => CaptureOutcome::skipped("the host would not run the read"),
        RemoteRead::File { content, permissions, owner } => keep(&state, host_id, &path, &content, Meta { permissions, owner, trigger_source: trigger, ssh_user }),
    })
}

/// Keep text the app already has (the editor's copy from before a save, or the live file before a rollback).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn config_store(
    state: State<'_, AppState>,
    host_id: Uuid,
    path: String,
    content: String,
    permissions: String,
    owner: String,
    trigger: String,
    ssh_user: String,
    max_bytes: u64,
) -> ApiResult<CaptureOutcome> {
    check_path(&path)?;
    if content.len() as u64 > max_bytes {
        return Ok(CaptureOutcome::skipped(format!("the file is {} bytes, over the limit", content.len())));
    }
    Ok(keep(&state, host_id, &path, content.as_bytes(), Meta { permissions, owner, trigger_source: trigger, ssh_user }))
}

#[tauri::command]
pub fn config_files(state: State<'_, AppState>, host_id: Option<Uuid>) -> ApiResult<Vec<TrackedFile>> {
    Ok(state.config_history.files(&key(&state)?, host_id))
}

#[tauri::command]
pub fn config_versions(state: State<'_, AppState>, host_id: Uuid, path: String) -> ApiResult<Vec<Snapshot>> {
    check_path(&path)?;
    Ok(state.config_history.versions(&key(&state)?, host_id, &path))
}

/// One version's text.
#[tauri::command]
pub fn config_get(state: State<'_, AppState>, host_id: Uuid, path: String, id: String) -> ApiResult<String> {
    check_path(&path)?;
    let bytes = state
        .config_history
        .read(&key(&state)?, host_id, &path, &id)
        .ok_or_else(|| ApiError::new("not_found", "that version is gone"))?;
    String::from_utf8(bytes).map_err(|_| ApiError::new("not_text", "that version is not text"))
}

#[tauri::command]
pub fn config_delete_file(state: State<'_, AppState>, host_id: Uuid, path: String) -> bool {
    state.config_history.delete_file(host_id, &path)
}

#[tauri::command]
pub fn config_clear(state: State<'_, AppState>) -> usize {
    state.config_history.clear()
}

#[tauri::command]
pub fn config_usage(state: State<'_, AppState>) -> Usage {
    state.config_history.usage()
}

/// Drop versions older than `days` (0 keeps all). The newest version of each file always stays.
#[tauri::command]
pub fn config_prune(state: State<'_, AppState>, days: u32) -> usize {
    state.config_history.prune(days, now_ms())
}
