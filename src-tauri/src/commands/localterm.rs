//! Local terminals.

use super::*;

/// Whether the system has `mosh-client`, so the host form can say so.
#[tauri::command]
pub fn mosh_available() -> bool {
    crate::mosh::find_client().is_some()
}

/// Connect a pane with Mosh: log in over SSH (vault keys, jump hosts and
/// known-hosts rules apply), start `mosh-server`, then run the system's
/// `mosh-client` in a local terminal. The pane then behaves like a local one.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn mosh_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    host_id: Uuid,
    cols: u32,
    rows: u32,
    credentials: Option<Credentials>,
    on_data: Channel<InvokeResponseBody>,
) -> ApiResult<()> {
    let client = crate::mosh::find_client().ok_or(crate::mosh::MoshError::ClientMissing)?;
    let target = resolve_target(&state, host_id, credentials)?;
    let lang = std::env::var("LANG").ok();
    let launch = crate::mosh::start_server(target, lang.as_deref()).await?;
    let (argv, env) = crate::mosh::client_command(&client, &launch, lang.as_deref());
    let sink = Arc::new(PaneSink {
        app,
        pane_id: pane_id.clone(),
        data: on_data,
        log: state.log_slot(&pane_id),
    });
    state
        .local
        .spawn_env(pane_id, cols, rows, Some(argv), None, env, sink)
        .map_err(|e| ApiError::new("mosh", e.to_string()))
}

/// The shells this computer can start, and its WSL distributions on Windows.
#[tauri::command]
pub async fn local_shells() -> ApiResult<Vec<crate::localshells::ShellInfo>> {
    tauri::async_runtime::spawn_blocking(|| crate::localshells::detect(&crate::localshells::RealEnv))
        .await
        .map_err(|e| ApiError::new("local", e.to_string()))
}

/// Start the user's shell in a local terminal pane.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn local_spawn(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    cols: u32,
    rows: u32,
    shell: Option<String>,
    shell_id: Option<String>,
    cwd: Option<String>,
    on_data: Channel<InvokeResponseBody>,
) -> ApiResult<()> {
    // A detected shell is looked up here by id; the window never supplies its argv.
    let by_id = match shell_id.filter(|i| !i.is_empty()) {
        Some(id) => Some(
            crate::localshells::resolve(&crate::localshells::RealEnv, &id)
                .ok_or_else(|| ApiError::new("local", format!("The shell \"{id}\" isn't available on this computer.")))?
                .argv,
        ),
        None => None,
    };
    let sink = Arc::new(PaneSink {
        app,
        pane_id: pane_id.clone(),
        data: on_data,
        log: state.log_slot(&pane_id),
    });
    // "pwsh -NoLogo" style: the first word is the program.
    let argv: Option<Vec<String>> = by_id.or_else(|| {
        shell
            .map(|s| s.split_whitespace().map(str::to_string).collect::<Vec<_>>())
            .filter(|v| !v.is_empty())
    });
    let home = sftp::local::home();
    let cwd = cwd
        .filter(|c| !c.trim().is_empty())
        .map(|c| PathBuf::from(crate::sshconfig::expand_path(&c, &home)));
    state
        .local
        .spawn(pane_id, cols, rows, argv, cwd, sink)
        .map_err(|e| ApiError::new("local", e.to_string()))
}
