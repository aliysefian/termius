//! Detail monitoring.

use super::*;

/// Keep one SSH connection to a host open for sampling. The host needs saved
/// credentials: the connection is made without asking.
#[tauri::command]
pub async fn monitor_open(state: State<'_, AppState>, host_id: Uuid) -> ApiResult<Uuid> {
    let target = resolve_target(&state, host_id, None)?;
    Ok(state.monitors.open(target).await?)
}

/// Run a script on a watched host, under `sh -c`, and wait for it.
#[tauri::command]
pub async fn monitor_exec(
    state: State<'_, AppState>,
    session_id: Uuid,
    script: String,
    timeout_secs: u64,
) -> ApiResult<crate::monitor::ExecResult> {
    let timeout = std::time::Duration::from_secs(timeout_secs.clamp(1, 120));
    Ok(state.monitors.exec(session_id, &script, timeout).await?)
}

/// Follow a script's output on a watched host (logs). Events arrive on `on_event`; the returned id stops it.
#[tauri::command]
pub fn monitor_stream_start(
    state: State<'_, AppState>,
    session_id: Uuid,
    script: String,
    on_event: Channel<crate::containers::LogEvent>,
) -> ApiResult<Uuid> {
    let sink: Arc<dyn crate::containers::LogSink> = Arc::new(ChannelLogSink(on_event));
    Ok(state.monitors.start_stream(session_id, &script, sink)?)
}

#[tauri::command]
pub fn monitor_stream_stop(state: State<'_, AppState>, stream_id: Uuid) {
    state.monitors.stop_stream(stream_id);
}

#[tauri::command]
pub async fn monitor_close(state: State<'_, AppState>, session_id: Uuid) -> ApiResult<()> {
    state.monitors.close(session_id).await;
    Ok(())
}

#[tauri::command]
pub fn list_workspaces(state: State<'_, AppState>) -> ApiResult<Vec<Record<crate::models::Workspace>>> {
    list_records(&state, Collection::Workspaces)
}

#[tauri::command]
pub fn save_workspace(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    workspace: crate::models::Workspace,
) -> ApiResult<Record<crate::models::Workspace>> {
    if workspace.name.trim().is_empty() {
        return Err(ApiError::new("validation", "a workspace needs a name"));
    }
    save_record(&state, Collection::Workspaces, id, base_rev, workspace)
}

#[tauri::command]
pub fn delete_workspace(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    delete_record(&state, Collection::Workspaces, id, base_rev)
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsRecord {
    pub rev: u64,
    pub settings: crate::models::VaultSettings,
}

/// Settings shared by every device using the vault.
#[tauri::command]
pub fn get_vault_settings(state: State<'_, AppState>) -> ApiResult<SettingsRecord> {
    let rec = state.session.with_vault(|v| {
        match v.get::<crate::models::VaultSettings>(Collection::Settings, crate::models::SETTINGS_ID) {
            Ok(r) => Ok(Some(r)),
            Err(VaultError::NotFound { .. } | VaultError::Deleted { .. }) => Ok(None),
            Err(e) => Err(e),
        }
    })?;
    Ok(match rec {
        Some(r) => SettingsRecord {
            rev: r.rev,
            settings: r.data.unwrap_or_default(),
        },
        None => SettingsRecord {
            rev: 0,
            settings: Default::default(),
        },
    })
}

#[tauri::command]
pub fn save_vault_settings(
    state: State<'_, AppState>,
    base_rev: u64,
    settings: crate::models::VaultSettings,
) -> ApiResult<SettingsRecord> {
    let mut settings = settings;
    settings.backup_retention = settings.backup_retention.clamp(1, 1000);
    for p in &settings.destructive_patterns {
        if p.len() > 500 {
            return Err(ApiError::new("validation", "a warning pattern is too long"));
        }
    }
    let base = if base_rev == 0 {
        crate::vault::Base::New
    } else {
        crate::vault::Base::Rev(base_rev)
    };
    let rec = state
        .session
        .with_vault(|v| v.put(Collection::Settings, crate::models::SETTINGS_ID, &settings, base))?;
    state
        .session
        .backup_retention
        .store(settings.backup_retention, std::sync::atomic::Ordering::Relaxed);
    Ok(SettingsRecord {
        rev: rec.rev,
        settings,
    })
}
