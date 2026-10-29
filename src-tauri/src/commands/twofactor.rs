//! Two-factor codes: the per-computer switch, and a live code for checking a secret in the identity form.
//! Off until switched on; see `totp` for what it does when it is.

use super::*;

#[derive(Debug, Clone, Serialize)]
pub struct TotpStatus {
    /// Answer "Verification code" prompts from an identity's saved secret, on this computer.
    pub enabled: bool,
}

fn totp_status_of(state: &AppState) -> TotpStatus {
    TotpStatus { enabled: totp_enabled(state) }
}

/// The switch as connections read it: any trouble reading the settings counts as off.
pub(super) fn totp_enabled(state: &AppState) -> bool {
    AppConfig::load(&state.config_dir).map(|c| c.totp_enabled).unwrap_or(false)
}

#[tauri::command]
pub fn totp_status(state: State<'_, AppState>) -> TotpStatus {
    totp_status_of(&state)
}

/// Turn two-factor codes on or off for this computer. Saved secrets stay in the vault either way.
#[tauri::command]
pub fn totp_set_enabled(state: State<'_, AppState>, enabled: bool) -> ApiResult<TotpStatus> {
    let mut cfg = AppConfig::load(&state.config_dir)?;
    cfg.totp_enabled = enabled;
    cfg.save(&state.config_dir)?;
    Ok(totp_status_of(&state))
}

/// The code a secret gives right now and how long it lasts, so the form can be checked against the authenticator app.
/// Works on what was typed; a saved secret is not read back without the master password (see `reveal_identity`).
#[tauri::command]
pub fn totp_preview(secret: String) -> ApiResult<crate::totp::Preview> {
    crate::totp::Totp::parse(&secret)
        .map(|t| t.preview())
        .map_err(|e| ApiError::new("validation", e.to_string()))
}
