//! Telnet and serial consoles.

use super::*;

fn raw_err(e: crate::rawterm::RawError) -> ApiError {
    ApiError::new("raw", e.to_string())
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn raw_telnet(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    host: String,
    port: u16,
    cols: u32,
    rows: u32,
    on_data: Channel<InvokeResponseBody>,
) -> ApiResult<()> {
    let sink = Arc::new(PaneSink {
        app,
        pane_id: pane_id.clone(),
        data: on_data,
        log: state.log_slot(&pane_id),
    });
    state.raw.telnet(pane_id, host.trim().to_string(), port, cols, rows, sink).map_err(raw_err)
}

#[tauri::command]
pub fn raw_serial(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    config: crate::rawterm::SerialConfig,
    on_data: Channel<InvokeResponseBody>,
) -> ApiResult<()> {
    let sink = Arc::new(PaneSink {
        app,
        pane_id: pane_id.clone(),
        data: on_data,
        log: state.log_slot(&pane_id),
    });
    state.raw.serial(pane_id, config, sink).map_err(raw_err)
}

#[tauri::command]
pub fn raw_write(state: State<'_, AppState>, pane_id: String, data: Vec<u8>) -> ApiResult<()> {
    state.raw.write(&pane_id, data).map_err(raw_err)
}

#[tauri::command]
pub fn raw_resize(state: State<'_, AppState>, pane_id: String, cols: u32, rows: u32) -> ApiResult<()> {
    state.raw.resize(&pane_id, cols, rows).map_err(raw_err)
}

#[tauri::command]
pub fn raw_close(state: State<'_, AppState>, pane_id: String) {
    state.raw.close(&pane_id);
}

#[tauri::command]
pub fn serial_ports() -> Vec<String> {
    crate::rawterm::serial_ports()
}

#[tauri::command]
pub fn local_write(state: State<'_, AppState>, pane_id: String, data: Vec<u8>) -> ApiResult<()> {
    state
        .local
        .write(&pane_id, &data)
        .map_err(|e| ApiError::new("not_connected", e.to_string()))
}

#[tauri::command]
pub fn local_resize(
    state: State<'_, AppState>,
    pane_id: String,
    cols: u32,
    rows: u32,
) -> ApiResult<()> {
    state
        .local
        .resize(&pane_id, cols, rows)
        .map_err(|e| ApiError::new("not_connected", e.to_string()))
}

#[tauri::command]
pub fn local_close(state: State<'_, AppState>, pane_id: String) {
    state.local.close(&pane_id);
    state
        .logs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&pane_id);
}
