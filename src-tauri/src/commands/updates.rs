//! Updates.

use super::*;

pub(super) fn updater_configured(config: &tauri::Config) -> bool {
    config
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|p| p.as_str())
        .is_some_and(|p| !p.trim().is_empty())
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdaterInfo {
    /// This build can check for and verify updates.
    pub enabled: bool,
    /// It can also install them itself. On Linux only the AppImage can;
    /// .deb and .rpm installs are updated by downloading the new package.
    pub can_install: bool,
    pub version: String,
}

#[tauri::command]
pub fn updater_info(app: AppHandle) -> UpdaterInfo {
    let enabled = updater_configured(app.config());
    let can_install = enabled && (!cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some());
    UpdaterInfo {
        enabled,
        can_install,
        version: app.package_info().version.to_string(),
    }
}

/// Put back a record deleted in the last few minutes, exactly as it was.
#[tauri::command]
pub fn undelete_record(state: State<'_, AppState>, collection: Collection, id: Uuid) -> ApiResult<()> {
    let t = {
        let mut trash = state.trash.lock().unwrap_or_else(|p| p.into_inner());
        let idx = trash
            .iter()
            .position(|x| x.collection == collection && x.id == id && x.at.elapsed() < UNDO_WINDOW)
            .ok_or_else(|| ApiError::new("expired", "that can no longer be undone"))?;
        trash.remove(idx)
    };
    state.session.with_vault(|v| {
        for (c, i, d) in &t.along {
            v.put(*c, *i, d, crate::vault::Base::Latest)?;
        }
        v.put(t.collection, t.id, &t.data, crate::vault::Base::Latest)?;
        for hid in &t.detached_from {
            if let Ok(Record { data: Some(mut h), rev, .. }) = v.get::<Host>(Collection::Hosts, *hid) {
                if h.identity_id.is_none() {
                    h.identity_id = Some(t.id);
                    v.put(Collection::Hosts, *hid, &h, crate::vault::Base::Rev(rev))?;
                }
            }
        }
        Ok(())
    })?;
    Ok(())
}
