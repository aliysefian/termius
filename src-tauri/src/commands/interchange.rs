//! Import hosts from, and export them to, other SSH clients (MobaXterm, Ásbrú). See `interchange.rs`.
//!
//! A preview returns the same `SshConfigPreview` the other importers do, so the frontend's pick-and-import step
//! (`ssh_config_import`, which writes into the encrypted vault) is shared. Exports hold no passwords or keys.

use super::*;
use crate::interchange::{self, FormatInfo, ImportContext};

fn format(id: &str) -> ApiResult<&'static dyn interchange::HostFormat> {
    interchange::find(id).ok_or_else(|| ApiError::new("validation", format!("Unknown file format \"{id}\".")))
}

/// The clients that can be imported from and exported to.
#[tauri::command]
pub fn interchange_formats() -> Vec<FormatInfo> {
    interchange::infos()
}

/// What is in another client's file, and which of its names already exist here. Changes nothing.
#[tauri::command]
pub fn interchange_preview(state: State<'_, AppState>, format_id: String, path: String) -> ApiResult<SshConfigPreview> {
    let f = format(&format_id)?;
    let text = read_text_file(path.clone())?;
    // `_ProfileDir_` in MobaXterm is the Windows user folder.
    let ctx = ImportContext { profile_dir: std::env::var("USERPROFILE").ok() };
    let parsed = interchange::parse(f, &text, &ctx);
    let labels: std::collections::HashSet<String> = list_records::<Host>(&state, Collection::Hosts)?
        .into_iter()
        .filter_map(|r| r.data.map(|d| d.label.to_lowercase()))
        .collect();
    let existing = parsed.hosts.iter().filter(|h| labels.contains(&h.alias.to_lowercase())).map(|h| h.alias.clone()).collect();
    Ok(SshConfigPreview { path, hosts: parsed.hosts, warnings: parsed.warnings, existing })
}

/// Write the hosts (all, or one group and below) to `path` in another client's format. Returns how many were written.
#[tauri::command]
pub fn interchange_export(state: State<'_, AppState>, format_id: String, path: String, group: Option<String>) -> ApiResult<usize> {
    let f = format(&format_id)?;
    let hosts = state.session.with_vault(|v| interchange::collect(v, group.as_deref()))?;
    let text = f.export(&hosts);
    crate::exportfile::write_interchange(&PathBuf::from(&path), &text).map_err(|e| ApiError::new("io", e))?;
    Ok(hosts.len())
}
