//! Host metrics: the commands over the shared engine in `crate::hostmetrics`.
//!
//! Every view asks for a sample here and gets the engine's: a recent one is shared, a collection already under way is
//! waited for, and the connection is the one the monitoring engine keeps for that host (see `AlertHost`). Nothing here
//! opens a connection of its own, and nothing answers while the vault is locked.

use std::time::Duration;

use super::*;
use crate::hostmetrics::manager::{BoxFuture, EventSink, HostExec, MetricsError};
use crate::hostmetrics::model::{HistoryPoint, LoginReport, PortList, ProcessList, Sample};
use crate::hostmetrics::procs::SignalOutcome;
use crate::monitoring::engine::EngineHost;

use super::alerting::AlertHost;

/// Name of the event pushed for every reading that was actually collected. Payload: [`Sample`].
pub const EVENT_SAMPLE: &str = "hostmetrics:sample";
/// Pushed when a collection failed. Payload: [`FailureEvent`].
pub const EVENT_FAILED: &str = "hostmetrics:failed";

/// The most a caller may ask a reading to be old. Anything bigger is just a stale cache.
const MAX_AGE_LIMIT_MS: u64 = 10 * 60_000;
const DEFAULT_MAX_AGE_MS: u64 = 2_000;

impl HostExec for AlertHost {
    fn exec<'a>(&'a self, host: Uuid, script: &'a str, limit: Duration) -> BoxFuture<'a, Result<String, String>> {
        self.ssh_exec(host, script, limit)
    }
}

/// Pushes each collected reading to the window once, however many views are open.
pub struct AppSink(pub AppHandle);

#[derive(Debug, Clone, Serialize)]
pub struct FailureEvent {
    pub host_id: Uuid,
    pub error: MetricsError,
    pub message: String,
}

impl EventSink for AppSink {
    fn sampled(&self, sample: &std::sync::Arc<Sample>) {
        let _ = self.0.emit(EVENT_SAMPLE, sample.as_ref());
    }

    fn failed(&self, host: Uuid, error: &MetricsError) {
        // A cancel is the caller's own doing; a back-off notice repeats a failure already shown.
        if matches!(error, MetricsError::Cancelled | MetricsError::Cooling { .. }) {
            return;
        }
        let _ = self.0.emit(EVENT_FAILED, FailureEvent { host_id: host, error: error.clone(), message: error.to_string() });
    }
}

fn api_error(e: MetricsError) -> ApiError {
    let code = match &e {
        MetricsError::Exec { .. } => "metrics_exec",
        MetricsError::Unreadable { .. } => "metrics_unreadable",
        MetricsError::Timeout => "metrics_timeout",
        MetricsError::Cancelled => "metrics_cancelled",
        MetricsError::Cooling { .. } => "metrics_cooling",
    };
    ApiError::new(code, e.to_string())
}

/// A view may not read a host's metrics, cached or not, while the vault is locked.
fn unlocked(state: &AppState) -> ApiResult<()> {
    Ok(state.session.with_vault(|_| Ok(()))?)
}

#[derive(Debug, Clone, Serialize)]
pub struct Latest {
    pub sample: Sample,
    /// How long ago it was taken, in milliseconds: a view can show "stale" without a clock of its own.
    pub age_ms: u64,
}

/// A reading of the host no older than `max_age_ms` (default 2 s). Takes one from the host only if the shared one is
/// older, and never more often than the engine's minimum interval.
#[tauri::command]
pub async fn hostmetrics_sample(state: State<'_, AppState>, host_id: Uuid, max_age_ms: Option<u64>) -> ApiResult<Sample> {
    unlocked(&state)?;
    let age = Duration::from_millis(max_age_ms.unwrap_or(DEFAULT_MAX_AGE_MS).min(MAX_AGE_LIMIT_MS));
    let sample = state.hostmetrics.sample(host_id, age).await.map_err(api_error)?;
    Ok(sample.as_ref().clone())
}

/// The last reading of a host, or nothing. Never connects.
#[tauri::command]
pub fn hostmetrics_latest(state: State<'_, AppState>, host_id: Uuid) -> ApiResult<Option<Latest>> {
    unlocked(&state)?;
    Ok(state.hostmetrics.latest(host_id).map(|(s, age)| Latest { sample: s.as_ref().clone(), age_ms: age.as_millis() as u64 }))
}

/// The last reading of every host that has one (for the dashboard's ranking). Never connects.
#[tauri::command]
pub fn hostmetrics_all_latest(state: State<'_, AppState>) -> ApiResult<Vec<Latest>> {
    unlocked(&state)?;
    Ok(state.hostmetrics.all_latest().into_iter().map(|(s, age)| Latest { sample: s.as_ref().clone(), age_ms: age.as_millis() as u64 }).collect())
}

/// What was kept for charts since `since_ms` (milliseconds since 1970). In memory only.
#[tauri::command]
pub fn hostmetrics_history(state: State<'_, AppState>, host_id: Uuid, since_ms: u64) -> ApiResult<Vec<HistoryPoint>> {
    unlocked(&state)?;
    Ok(state.hostmetrics.history(host_id, since_ms))
}

/// Stop the reading in progress on a host.
#[tauri::command]
pub fn hostmetrics_cancel(state: State<'_, AppState>, host_id: Uuid) -> ApiResult<()> {
    unlocked(&state)?;
    state.hostmetrics.cancel(host_id);
    Ok(())
}

/// Forget everything kept about a host (its readings, history and baseline).
#[tauri::command]
pub fn hostmetrics_forget(state: State<'_, AppState>, host_id: Uuid) -> ApiResult<()> {
    unlocked(&state)?;
    state.hostmetrics.forget(host_id);
    Ok(())
}

/// The age a view of processes, ports or logins may have, in milliseconds (default 3 s).
fn view_age(ms: Option<u64>) -> Duration {
    Duration::from_millis(ms.unwrap_or(3_000).min(MAX_AGE_LIMIT_MS))
}

/// The host's processes, busiest first (at most 500), with CPU figures from the second reading on (the first list is read
/// twice, a second apart, so it has them at once). Shared between views: a recent list is not read again.
#[tauri::command]
pub async fn hostmetrics_processes(state: State<'_, AppState>, host_id: Uuid, max_age_ms: Option<u64>) -> ApiResult<ProcessList> {
    unlocked(&state)?;
    Ok(state.hostmetrics.processes(host_id, view_age(max_age_ms)).await.map_err(api_error)?.as_ref().clone())
}

/// What is listening on the host.
#[tauri::command]
pub async fn hostmetrics_ports(state: State<'_, AppState>, host_id: Uuid, max_age_ms: Option<u64>) -> ApiResult<PortList> {
    unlocked(&state)?;
    Ok(state.hostmetrics.ports(host_id, view_age(max_age_ms)).await.map_err(api_error)?.as_ref().clone())
}

/// Logins, failed logins and SSH authentication events, with what could not be read and why.
#[tauri::command]
pub async fn hostmetrics_logins(state: State<'_, AppState>, host_id: Uuid, max_age_ms: Option<u64>) -> ApiResult<LoginReport> {
    unlocked(&state)?;
    Ok(state.hostmetrics.logins(host_id, view_age(max_age_ms)).await.map_err(api_error)?.as_ref().clone())
}

/// Send a signal to one process. The host checks, in the same command, that the pid is still the process that was shown
/// (same name and start time) before sending anything; with the person's own rights, never elevated. One attempt only: if
/// the host does not answer the outcome is `unknown` and nothing is sent again.
#[tauri::command]
pub async fn hostmetrics_signal(state: State<'_, AppState>, host_id: Uuid, pid: u32, start_ticks: u64, name: String, signal: String) -> ApiResult<SignalOutcome> {
    unlocked(&state)?;
    state.hostmetrics.signal(host_id, pid, start_ticks, &name, &signal).await.map_err(|e| ApiError::new("validation", e.to_string()))
}
