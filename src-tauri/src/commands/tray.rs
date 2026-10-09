//! The tray icon. Closing the window hides it and SSHVault keeps running (sessions, tunnels, the agent and schedules
//! carry on); the icon, or launching the app again, brings the window back, and the icon's Exit really quits.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

use super::*;
use crate::tray::{ExitRequests, EXIT_ACK_TIMEOUT, SHUTDOWN_LIMIT};

/// The app's one tray icon.
pub const TRAY_ID: &str = "sshvault";
/// Sent to the window when Exit is chosen in the tray. It answers with `app_exit_ack` at once, then saves the open
/// tabs, asks about live sessions if it should, and calls `app_exit` (or does nothing, if the person cancels).
pub const EVENT_EXIT_REQUESTED: &str = "app:exit-requested";
const MENU_OPEN: &str = "tray-open";
const MENU_EXIT: &str = "tray-exit";

#[derive(Default)]
pub struct TrayState {
    available: AtomicBool,
    exits: ExitRequests,
}

/// Put the icon in the tray, once. Returns whether there is one. Without one (Linux with no AppIndicator library, or
/// a failure creating it) closing the window quits as it always did.
pub fn install(app: &tauri::App) -> bool {
    if app.try_state::<TrayState>().is_none() {
        app.manage(TrayState::default());
    }
    // Never a second icon, whatever calls this again.
    let ok = app.tray_by_id(TRAY_ID).is_some() || (crate::tray::system_tray_supported() && build(app).is_ok());
    app.state::<TrayState>().available.store(ok, Ordering::Relaxed);
    ok
}

fn build(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, "Open", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let exit = MenuItem::with_id(app, MENU_EXIT, "Exit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &separator, &exit])?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("SSHVault")
        .menu(&menu)
        // Left click opens the window (as in Telegram); the menu is on the right button.
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => show_main(app),
            MENU_EXIT => request_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if opens_window(&event) {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// A left click (on release) or a double click brings the window back. Linux trays send no clicks at all: there the
/// icon always opens its menu, and Open is in it.
fn opens_window(event: &TrayIconEvent) -> bool {
    matches!(
        event,
        TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. }
            | TrayIconEvent::DoubleClick { button: MouseButton::Left, .. }
    )
}

/// Show the main window where it was, restored and in front.
pub fn show_main<R: tauri::Runtime>(app: &AppHandle<R>) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Exit from the tray: let the window wrap up (and ask, with live sessions), but quit regardless if it doesn't answer.
fn request_exit(app: &AppHandle) {
    let id = app.state::<TrayState>().exits.begin();
    let _ = app.emit(EVENT_EXIT_REQUESTED, ());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(EXIT_ACK_TIMEOUT).await;
        if app.state::<TrayState>().exits.unanswered(id) {
            shutdown(app).await;
        }
    });
}

/// Quit for real: end every session, tunnel, transfer and child program (as locking does), then exit. Exiting locks
/// the vault on the way out (see `lib.rs`), which also releases this device's "open here" marker.
pub async fn shutdown(app: AppHandle) {
    {
        let state = app.state::<AppState>();
        let _ = tokio::time::timeout(SHUTDOWN_LIMIT, close_everything(&state)).await;
    }
    app.exit(0);
}

/// Whether closing the window can hide it: there is a tray icon to bring it back.
#[tauri::command]
pub fn tray_available(state: State<'_, TrayState>) -> bool {
    state.available.load(Ordering::Relaxed)
}

/// Hide the window and keep running. Refused without a tray icon, so the window can never be lost.
#[tauri::command]
pub fn window_hide_to_tray(app: AppHandle, state: State<'_, TrayState>) -> ApiResult<()> {
    if !state.available.load(Ordering::Relaxed) {
        return Err(ApiError::new("no_tray", "there is no tray icon to bring the window back"));
    }
    if let Some(w) = app.get_webview_window("main") {
        w.hide().map_err(|e| ApiError::new("window", e.to_string()))?;
    }
    Ok(())
}

/// Bring the window back, e.g. to ask before a tray Exit ends live sessions.
#[tauri::command]
pub fn window_show(app: AppHandle) {
    show_main(&app);
}

/// The window has a tray Exit in hand, so the fallback that quits without it stands down.
#[tauri::command]
pub fn app_exit_ack(state: State<'_, TrayState>) -> bool {
    state.exits.acknowledge()
}

/// Quit the app (tray Exit, after the window has saved what it keeps and the person agreed).
#[tauri::command]
pub async fn app_exit(app: AppHandle) {
    shutdown(app).await;
}
