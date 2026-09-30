//! Tauri commands for updating from GitHub Releases (see `release`).

use crate::release::{self, InstallKind, Outcome, ReleaseCheck};
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager, Runtime};

pub const EVENT_PROGRESS: &str = "update://progress";

static INSTALLING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Serialize)]
struct Progress {
    done: u64,
    total: u64,
}

fn current<R: Runtime>(app: &AppHandle<R>) -> String {
    app.package_info().version.to_string()
}

async fn install_kind() -> InstallKind {
    tauri::async_runtime::spawn_blocking(release::detect_install_kind)
        .await
        .unwrap_or(InstallKind::Manual)
}

/// Is there a newer release, and can this copy install it itself?
#[tauri::command]
pub async fn release_check<R: Runtime>(app: AppHandle<R>) -> Result<ReleaseCheck, String> {
    let cur = current(&app);
    let kind = install_kind().await;
    let rel = release::latest_release(&cur).await?;
    Ok(release::summarize(&rel, &cur, kind))
}

/// Download, verify and install the latest release, then restart (or exit so
/// the Windows installer can take over). Progress arrives as
/// `update://progress` events. Only returns if something went wrong.
#[tauri::command]
pub async fn release_install<R: Runtime>(app: AppHandle<R>) -> Result<(), String> {
    if INSTALLING.swap(true, Ordering::SeqCst) {
        return Err("an update is already being installed".into());
    }
    let result = run(&app).await;
    INSTALLING.store(false, Ordering::SeqCst);
    result
}

async fn run<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let cur = current(app);
    let kind = install_kind().await;
    if kind == InstallKind::Manual {
        return Err("this copy can't install updates itself; download the new version from the releases page".into());
    }
    // Looked up again here rather than taken from the page, so only this
    // project's newest release can ever be installed.
    let rel = release::latest_release(&cur).await?;
    if !release::is_newer(&rel.tag_name, &cur) {
        return Err("you already have the latest version".into());
    }
    let asset = release::pick_asset(kind, &rel.assets)
        .cloned()
        .ok_or("the latest release has no installer for this kind of install")?;
    let cache = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let dest = release::download_path(kind, &asset, &cache)?;

    let mut last_pct = u64::MAX;
    release::download_verified(&cur, &asset, &dest, |done, total| {
        let pct = if total == 0 { 0 } else { done * 100 / total };
        if pct != last_pct {
            last_pct = pct;
            let _ = app.emit(EVENT_PROGRESS, Progress { done, total });
        }
    })
    .await?;

    let (outcome, next) = release::install(kind, &dest).await?;
    if outcome == Outcome::Restart {
        if let Some(program) = next {
            release::relaunch_after_exit(&program, std::env::args_os().skip(1))?;
        }
    }
    app.exit(0);
    Ok(())
}
