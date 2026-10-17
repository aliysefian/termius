//! Encrypted export and import of hosts. See `hostexport.rs` for what is in a file and how conflicts are handled.
//!
//! Import is two calls that each take the file and passphrase: `hosts_import_preview` says what is in it and which
//! names clash, and `hosts_import_apply` adds it with the person's answers. Nothing decrypted is kept between them.

use super::*;
use crate::hostexport::{self, ExportError, ExportOptions, ExportSummary, ImportPreview, ImportResult, Resolution};

fn api(e: ExportError) -> ApiError {
    let code = match &e {
        ExportError::Vault(_) => "",
        ExportError::WrongPassphrase => "wrong_passphrase",
        ExportError::WeakPassphrase | ExportError::Name(_) => "validation",
        ExportError::NotAnExport | ExportError::Newer(_) => "bad_export",
        ExportError::Other(_) => "io",
    };
    match e {
        ExportError::Vault(v) => v.into(),
        other => ApiError::new(code, other.to_string()),
    }
}

/// Open the file at `path` with `passphrase`. The key derivation takes about a second, so it runs off the UI thread.
async fn read_bundle(path: String, passphrase: String) -> ApiResult<hostexport::Bundle> {
    let p = PathBuf::from(crate::sshconfig::expand_path(&path, &sftp::local::home()));
    let bytes = crate::vault::read_regular_file(&p, hostexport::MAX_FILE_BYTES).map_err(|e| ApiError::new("io", format!("{}: {e}", p.display())))?;
    let text = String::from_utf8(bytes).map_err(|_| api(ExportError::NotAnExport))?;
    let passphrase = Zeroizing::new(passphrase);
    tauri::async_runtime::spawn_blocking(move || hostexport::open(&text, &passphrase))
        .await
        .map_err(|e| ApiError::new("io", e.to_string()))?
        .map_err(api)
}

/// Write the hosts (all, or one group) to `path`, encrypted with `passphrase`.
#[tauri::command]
pub async fn hosts_export(state: State<'_, AppState>, path: String, passphrase: String, options: ExportOptions) -> ApiResult<ExportSummary> {
    let passphrase = Zeroizing::new(passphrase);
    let bundle = state.session.with_vault(|v| Ok(hostexport::build(v, &options)))?.map_err(api)?;
    let summary = hostexport::summary(&bundle);
    let text = tauri::async_runtime::spawn_blocking(move || hostexport::seal(&bundle, &passphrase, crate::crypto::KdfParams::default()))
        .await
        .map_err(|e| ApiError::new("io", e.to_string()))?
        .map_err(api)?;
    crate::exportfile::write_host_export(&PathBuf::from(&path), &text).map_err(|e| ApiError::new("io", e))?;
    Ok(summary)
}

/// What is in an export file and which of its hosts clash with ones here. Changes nothing.
#[tauri::command]
pub async fn hosts_import_preview(state: State<'_, AppState>, path: String, passphrase: String) -> ApiResult<ImportPreview> {
    let bundle = read_bundle(path, passphrase).await?;
    state.session.with_vault(|v| Ok(hostexport::preview(v, &bundle)))?.map_err(api)
}

/// Add an export file's hosts to this vault, with the person's answer for each clashing name.
#[tauri::command]
pub async fn hosts_import_apply(state: State<'_, AppState>, path: String, passphrase: String, resolutions: Vec<Resolution>) -> ApiResult<ImportResult> {
    let bundle = read_bundle(path, passphrase).await?;
    state.session.with_vault(|v| Ok(hostexport::apply(v, &bundle, &resolutions)))?.map_err(api)
}
