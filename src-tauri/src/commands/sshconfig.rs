//! Import from ~/.ssh/config.

use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct SshConfigPreview {
    pub path: String,
    pub hosts: Vec<crate::sshconfig::ImportedHost>,
    pub warnings: Vec<String>,
    /// Aliases that match hosts already in the vault (will be skipped).
    pub existing: Vec<String>,
}

#[tauri::command]
pub fn ssh_config_preview(
    state: State<'_, AppState>,
    path: Option<String>,
) -> ApiResult<SshConfigPreview> {
    let home = sftp::local::home();
    let path = path
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::sshconfig::default_path(&home));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| ApiError::new("io", format!("{}: {e}", path.display())))?;
    let parsed = crate::sshconfig::parse(&text, &home);
    let labels: std::collections::HashSet<String> =
        list_records::<Host>(&state, Collection::Hosts)?
            .into_iter()
            .filter_map(|r| r.data.map(|d| d.label.to_lowercase()))
            .collect();
    let existing = parsed
        .hosts
        .iter()
        .filter(|h| labels.contains(&h.alias.to_lowercase()))
        .map(|h| h.alias.clone())
        .collect();
    Ok(SshConfigPreview {
        path: path.to_string_lossy().into_owned(),
        hosts: parsed.hosts,
        warnings: parsed.warnings,
        existing,
    })
}

#[tauri::command]
pub fn ssh_config_import(
    state: State<'_, AppState>,
    hosts: Vec<crate::sshconfig::ImportedHost>,
    group: String,
    key_import: Option<crate::sshconfig::KeyImport>,
) -> ApiResult<crate::sshconfig::ImportSummary> {
    let local_user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok();
    // Keys are copied into the vault only when the user chose to.
    let key_import = key_import.unwrap_or_default();
    Ok(state.session.with_vault(|v| {
        crate::sshconfig::import_into_vault(v, &hosts, group.trim(), local_user.as_deref(), key_import)
    })?)
}
