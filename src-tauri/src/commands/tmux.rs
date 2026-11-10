//! tmux on hosts: the commands over the shared engine in `crate::tmux`.
//!
//! Every view asks here and gets the engine's answer: a recent snapshot is shared, a reading under way is waited for, and the
//! connection is the one the monitoring engine keeps for that host (see `AlertHost`). Nothing here opens a connection of its
//! own, composes a tmux command line, or answers while the vault is locked. Pane text is returned to the window that asked
//! and is never written anywhere or logged.

use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::tmux::manager::{OpResult, PaneCapture, SearchReport, SearchTarget, TmuxError, TmuxSink};
use crate::tmux::model::{valid_pane_id, valid_session_id, valid_window_id, HostSnapshot, Socket};
use crate::tmux::ops::Op;
use crate::tmux::resources::Resources;
use crate::tmux::script::{self, CaptureOpts};
use crate::tmux::search::Query;

use super::alerting::AlertHost;

/// Pushed when a host's snapshot actually changed (or went stale). Payload: [`HostSnapshot`].
pub const EVENT_SNAPSHOT: &str = "tmux:snapshot";

pub type Manager = crate::tmux::manager::TmuxManager<AlertHost>;

/// Pushes each real change to the window once, however many views are open.
pub struct AppSink(pub AppHandle);

impl TmuxSink for AppSink {
    fn changed(&self, snapshot: &Arc<HostSnapshot>) {
        let _ = self.0.emit(EVENT_SNAPSHOT, snapshot.as_ref());
    }
}

fn api_error(e: TmuxError) -> ApiError {
    let code = match &e {
        TmuxError::Exec { .. } => "tmux_exec",
        TmuxError::Timeout => "tmux_timeout",
        TmuxError::Unreadable { .. } => "tmux_unreadable",
        TmuxError::Invalid { .. } => "validation",
        TmuxError::Rejected { .. } => "tmux_rejected",
        TmuxError::ServerChanged => "tmux_server_changed",
        TmuxError::Cancelled => "tmux_cancelled",
    };
    ApiError::new(code, e.to_string())
}

/// A view may not read a host's tmux, cached or not, while the vault is locked.
fn unlocked(state: &AppState) -> ApiResult<()> {
    Ok(state.session.with_vault(|_| Ok(()))?)
}

fn socket_of(s: Option<Socket>) -> ApiResult<Socket> {
    let s = s.unwrap_or_default();
    s.validate().map_err(|m| ApiError::new("validation", m))?;
    Ok(s)
}

fn age(ms: Option<u64>) -> Duration {
    Duration::from_millis(ms.unwrap_or(2_000).min(10 * 60_000))
}

/// The host's tmux: whether it is installed, which sessions, windows and panes exist. No older than `max_age_ms` (default
/// 2 s). A reading that fails after an earlier good one returns the earlier one marked stale, not an error.
#[tauri::command]
pub async fn tmux_snapshot(state: State<'_, AppState>, host_id: Uuid, socket: Option<Socket>, max_age_ms: Option<u64>) -> ApiResult<HostSnapshot> {
    unlocked(&state)?;
    Ok(state.tmux.snapshot(host_id, &socket_of(socket)?, age(max_age_ms)).await.map_err(api_error)?.as_ref().clone())
}

/// Read the host now (the person pressed refresh).
#[tauri::command]
pub async fn tmux_refresh(state: State<'_, AppState>, host_id: Uuid, socket: Option<Socket>) -> ApiResult<HostSnapshot> {
    unlocked(&state)?;
    Ok(state.tmux.refresh(host_id, &socket_of(socket)?).await.map_err(api_error)?.as_ref().clone())
}

/// Every snapshot taken so far, for drawing the tree at once. Never connects.
#[tauri::command]
pub fn tmux_latest(state: State<'_, AppState>) -> ApiResult<Vec<HostSnapshot>> {
    unlocked(&state)?;
    Ok(state.tmux.all_latest().into_iter().map(|s| s.as_ref().clone()).collect())
}

/// Keep a host's snapshot current while `subscriber` (a view's own name) watches. One loop serves every watcher.
#[tauri::command]
pub async fn tmux_watch(state: State<'_, AppState>, host_id: Uuid, socket: Option<Socket>, subscriber: String, interval_ms: u64) -> ApiResult<()> {
    // Async so that it runs inside the async runtime: the poll loop is spawned from here.
    unlocked(&state)?;
    if subscriber.is_empty() || subscriber.len() > 80 || subscriber.chars().any(|c| c.is_control()) {
        return Err(ApiError::new("validation", "Not a view name."));
    }
    state.tmux.watch(host_id, socket_of(socket)?, &subscriber, Duration::from_millis(interval_ms)).await.map_err(api_error)
}

/// A view closed or was hidden: stop its watching everywhere.
#[tauri::command]
pub fn tmux_unwatch(state: State<'_, AppState>, subscriber: String) -> ApiResult<()> {
    state.tmux.unwatch(&subscriber);
    Ok(())
}

/// A pane's text, without attaching: the visible screen and up to `scrollback` lines above it (at most 5000).
#[tauri::command]
pub async fn tmux_capture(state: State<'_, AppState>, host_id: Uuid, socket: Option<Socket>, pane_id: String, ansi: bool, scrollback: u32) -> ApiResult<PaneCapture> {
    unlocked(&state)?;
    let opts = CaptureOpts { ansi, join: !ansi, scrollback };
    state.tmux.capture(host_id, &socket_of(socket)?, &pane_id, opts, None).await.map_err(api_error)
}

#[derive(Debug, Clone, Deserialize)]
pub struct SearchTargetArg {
    pub session_id: String,
    pub window_id: String,
    pub pane_id: String,
}

/// Search the text of the given panes. `search_id` names the search so [`tmux_search_cancel`] can stop it; a new search under
/// the same name stops the old one.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn tmux_search(
    state: State<'_, AppState>,
    host_id: Uuid,
    socket: Option<Socket>,
    search_id: String,
    targets: Vec<SearchTargetArg>,
    query: Query,
    scrollback: u32,
) -> ApiResult<SearchReport> {
    unlocked(&state)?;
    if search_id.is_empty() || search_id.len() > 80 {
        return Err(ApiError::new("validation", "Not a search name."));
    }
    let socket = socket_of(socket)?;
    for t in &targets {
        if !valid_session_id(&t.session_id) || !valid_window_id(&t.window_id) || !valid_pane_id(&t.pane_id) {
            return Err(ApiError::new("validation", "Not a tmux pane."));
        }
    }
    let (token, serial) = state.tmux.begin_search(&search_id);
    let targets = targets.into_iter().map(|t| SearchTarget { session_id: t.session_id, window_id: t.window_id, pane_id: t.pane_id }).collect();
    let result = state.tmux.search(host_id, &socket, targets, &query, scrollback, &token).await;
    state.tmux.end_search(&search_id, serial);
    result.map_err(api_error)
}

#[tauri::command]
pub fn tmux_search_cancel(state: State<'_, AppState>, search_id: String) -> ApiResult<()> {
    state.tmux.cancel_search(&search_id);
    Ok(())
}

/// Change a tmux server and confirm the change against a fresh reading. The operation is checked before anything is sent,
/// aimed by tmux id at the server of the snapshot the person was looking at, and succeeds only if the reading afterwards
/// shows it.
#[tauri::command]
pub async fn tmux_operate(state: State<'_, AppState>, host_id: Uuid, socket: Option<Socket>, op: Op) -> ApiResult<OpResult> {
    unlocked(&state)?;
    state.tmux.operate(host_id, &socket_of(socket)?, &op).await.map_err(api_error)
}

/// The command to type into a terminal tab's shell to attach to (or, inside that server's tmux, switch to) a session.
/// `tab` marks the tab so a later switch can find its tmux client.
#[tauri::command]
pub fn tmux_attach_command(state: State<'_, AppState>, host_id: Uuid, socket: Option<Socket>, session_id: String, tab: String, detach_others: bool) -> ApiResult<String> {
    unlocked(&state)?;
    let socket = socket_of(socket)?;
    if !valid_session_id(&session_id) {
        return Err(ApiError::new("validation", "That is not a tmux session id."));
    }
    check_tab(&tab)?;
    let snap = state.tmux.latest(host_id, &socket);
    // The session has to be one the last reading showed: an id that was never listed is not something to attach to.
    if !snap.as_ref().is_some_and(|s| s.sessions.iter().any(|x| x.id == session_id)) {
        return Err(ApiError::new("tmux_rejected", "That session is not in the list. Refresh it and choose again."));
    }
    Ok(script::attach_command(&socket, &session_id, snap.as_ref().and_then(|s| s.server.as_ref()), &tab, detach_others))
}

/// Move the tmux client of terminal tab `tab` to a session, without typing anything into the terminal. `false`: that tab has
/// no client on this server (it is not attached there), so attach it instead.
#[tauri::command]
pub async fn tmux_switch_client(state: State<'_, AppState>, host_id: Uuid, socket: Option<Socket>, tab: String, session_id: String) -> ApiResult<bool> {
    unlocked(&state)?;
    state.tmux.switch_client(host_id, &socket_of(socket)?, &tab, &session_id).await.map_err(api_error)
}

/// What Auto Tmux types into a new shell: attach to the preferred session (creating it if asked), never nesting.
#[tauri::command]
pub fn tmux_auto_command(state: State<'_, AppState>, socket: Option<Socket>, session: String, create_if_missing: bool, always_new: bool, mouse: bool, tab: String) -> ApiResult<String> {
    unlocked(&state)?;
    let socket = socket_of(socket)?;
    crate::tmux::ops::valid_session_name(&session).map_err(|m| ApiError::new("validation", m))?;
    check_tab(&tab)?;
    Ok(script::auto_command(&socket, &session, create_if_missing, always_new, mouse, &tab))
}

fn check_tab(tab: &str) -> ApiResult<()> {
    if tab.is_empty() || tab.len() > 64 || !tab.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return Err(ApiError::new("validation", "Not a terminal tab marker."));
    }
    Ok(())
}

/// CPU and memory attributed to the sessions' panes by process tree. Estimates; see `tmux::resources`.
#[tauri::command]
pub async fn tmux_resources(state: State<'_, AppState>, host_id: Uuid, socket: Option<Socket>) -> ApiResult<Resources> {
    unlocked(&state)?;
    state.tmux.resources(host_id, &socket_of(socket)?).await.map_err(api_error)
}

/// Forget everything kept about a host.
#[tauri::command]
pub fn tmux_forget(state: State<'_, AppState>, host_id: Uuid) -> ApiResult<()> {
    unlocked(&state)?;
    state.tmux.forget(host_id);
    Ok(())
}
