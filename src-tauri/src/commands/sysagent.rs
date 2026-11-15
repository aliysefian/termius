//! The ssh-agent already running on this computer: what it holds, and which socket to find it on. This is how
//! hardware keys are used; see `crate::sysagent`.

use super::*;

/// The agent a login would use right now and the keys it lists. Nothing is signed.
#[tauri::command]
pub async fn system_agent_status() -> crate::sysagent::Status {
    crate::sysagent::status().await
}

/// Use this agent socket on this computer (empty: go back to looking for one), then report what is there.
#[tauri::command]
pub async fn system_agent_set_socket(state: State<'_, AppState>, path: Option<String>) -> ApiResult<crate::sysagent::Status> {
    let path = path.map(|p| p.trim().to_string()).filter(|p| !p.is_empty());
    let mut cfg = AppConfig::load(&state.config_dir)?;
    cfg.agent_socket = path.clone();
    cfg.save(&state.config_dir)?;
    crate::sysagent::set_socket(path);
    Ok(crate::sysagent::status().await)
}
