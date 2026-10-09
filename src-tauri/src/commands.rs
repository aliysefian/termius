//! Tauri IPC surface. Every command is a thin wrapper: parse arguments, call
//! into [`Session`] / [`Vault`], and map errors to a serializable [`ApiError`].
//!
//! Records cross the IPC boundary as `Record<T>` (`{ id, updated_at, deleted,
//! data }`) so the frontend can use `updated_at` for last-writer-wins when a
//! `vault:changed` event arrives from the file watcher.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::config::{AppConfig, ConfigError};
use crate::crypto::KdfParams;
use crate::forward::{ForwardManager, ForwardStatus, StatusSink};
use crate::models::{jump_chain, AuthMethod, ForwardRule, Host, Identity, Snippet};
use crate::session::{Session, SessionError, VaultStatus};
use crate::sftp::{
    self, FileEntry, ProgressSink, SftpError, SftpManager, TransferProgress,
};
use crate::ssh::{
    ConnectParams, LearnedKey, SessionStatus, SshError, SshManager, Target, TermSink, MAX_JUMPS,
};
use crate::sync::RecordChange;
use crate::vault::{Collection, Record, VaultError};

pub mod containers;
pub use containers::*;
pub mod databases;
pub use databases::*;
pub mod forwarding;
pub use forwarding::*;
pub mod keys;
pub use keys::*;
pub mod knownhosts;
pub use knownhosts::*;
pub mod localterm;
pub use localterm::*;
pub mod monitoring;
pub use monitoring::*;
pub mod rawconsole;
pub use rawconsole::*;
pub mod rdp;
pub use rdp::*;
pub mod runhosts;
pub use runhosts::*;
pub mod sshconfig;
pub use sshconfig::*;
pub mod updates;
pub use updates::*;
pub mod tray;

pub mod runbooks;

/// Event name for record changes detected on disk. Payload: [`RecordChange`].
pub const EVENT_VAULT_CHANGED: &str = "vault:changed";

/// Event name for SSH session lifecycle notices. Payload: [`SshStatusEvent`].
pub const EVENT_SSH_STATUS: &str = "ssh:status";

/// Event name for port-forward status changes. Payload: [`ForwardStatusEvent`].
pub const EVENT_FORWARD_STATUS: &str = "forward:status";

/// Event name for remote-edit uploads. Payload: [`EditEventPayload`].
pub const EVENT_SFTP_EDIT: &str = "sftp:edit";

/// A pane's optional session log, shared between its sink and the commands
/// that start and stop recording.
type LogSlot = Arc<std::sync::Mutex<Option<crate::sessionlog::SessionLog>>>;

pub struct AppState {
    pub session: Session,
    pub config_dir: PathBuf,
    pub ssh: Arc<SshManager>,
    pub sftp: Arc<SftpManager>,
    /// Every file pane (this computer, SFTP and the other file protocols) behind one interface.
    pub files: Arc<crate::files::FileManager>,
    pub forwards: Arc<ForwardManager>,
    pub runs: Arc<crate::runner::RunManager>,
    pub local: Arc<crate::localpty::LocalManager>,
    /// Remote desktop (RDP) sessions.
    pub rdp: Arc<crate::rdp::RdpManager>,
    pub vnc: Arc<crate::vnc::VncManager>,
    pub runbooks: Arc<crate::runbookrun::RunbookManager>,
    pub runbook_history: crate::runbookhistory::History,
    /// Telnet and serial-console sessions.
    pub raw: Arc<crate::rawterm::RawManager>,
    /// Open database connections (the Databases view).
    pub dbs: Arc<crate::db::DbManager>,
    /// Open container sessions (the Containers view).
    pub containers: Arc<crate::containers::ContainerManager>,
    pub kube: Arc<crate::kube::KubeManager>,
    /// Open detail-monitoring connections (one per host being watched).
    pub monitors: Arc<crate::monitor::MonitorManager>,
    pub edits: Arc<crate::remoteedit::EditManager>,
    /// Where remote files are downloaded for editing.
    pub edit_dir: PathBuf,
    /// Master-password gate for revealing stored secrets.
    pub reveal: std::sync::Mutex<crate::reveal::RevealGate>,
    /// OS keychain for "remember on this device".
    pub keystore: Arc<dyn crate::keychain::KeyStore>,
    /// Host keys trusted in the vault; unknown or changed keys ask the UI.
    pub host_keys: crate::ssh::HostKeyPolicy,
    host_key_prompts: PromptMap,
    /// Recently deleted records, kept in memory for a short undo window.
    trash: std::sync::Mutex<Vec<Trashed>>,
    /// The vault-backed SSH agent, while running.
    agent: std::sync::Mutex<Option<crate::agent::AgentHandle>>,
    /// Answers agent requests from the vault, for the local socket and for
    /// forwarded agent channels.
    vault_agent: Arc<dyn crate::agent::Backend>,
    agent_prompts: PromptMap,
    /// The command-line control socket, while running.
    control: std::sync::Mutex<Option<crate::control::ControlHandle>>,
    cli_prompts: PromptMap,
    /// `run` from the CLI is pre-approved until this time.
    cli_trusted_until: std::sync::Mutex<Option<std::time::Instant>>,
    logs: std::sync::Mutex<std::collections::HashMap<String, LogSlot>>,
    /// When the window last reported activity, for locking an idle vault even if the window can't.
    idle: crate::idle::IdleClock,
    /// Slows repeated wrong master passwords at the unlock command.
    unlock_guard: crate::unlockguard::UnlockGuard,
}

impl AppState {
    fn log_slot(&self, pane_id: &str) -> LogSlot {
        let slot: LogSlot = Arc::new(std::sync::Mutex::new(None));
        self.logs
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(pane_id.to_string(), Arc::clone(&slot));
        slot
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    /// Stable machine-readable code the UI can branch on.
    pub code: &'static str,
    pub message: String,
    /// Structured context, e.g. which fields conflicted.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ApiError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            details: None,
        }
    }
}

impl From<VaultError> for ApiError {
    fn from(e: VaultError) -> Self {
        let code = match &e {
            VaultError::WrongPassword => "wrong_password",
            VaultError::AlreadyExists(_) => "already_exists",
            VaultError::NotInitialized(_) => "not_initialized",
            VaultError::NotFound { .. } => "not_found",
            VaultError::Deleted { .. } => "deleted",
            VaultError::Io { .. } => "io",
            VaultError::Crypto(_) => "crypto",
            VaultError::WrongRecoveryKey => "wrong_recovery_key",
            VaultError::NoRecoverySlot => "no_recovery",
            VaultError::KeyMismatch => "not_remembered",
            VaultError::UnsupportedManifestVersion(_) => "unsupported_version",
            VaultError::MalformedManifest(..) => "corrupted",
            VaultError::MigrationInProgress { .. } => "migration_in_progress",
            VaultError::Backup(_) => "backup",
            VaultError::Conflict(c) => {
                let what = match c.reason {
                    crate::vault::ConflictReason::DeletedElsewhere => {
                        "was deleted on another device"
                    }
                    crate::vault::ConflictReason::AlreadyExists => "already exists",
                    crate::vault::ConflictReason::ChangedElsewhere => {
                        "was changed on another device"
                    }
                };
                let fields = if c.fields.is_empty() {
                    String::new()
                } else {
                    format!(" (both changed: {})", c.fields.join(", "))
                };
                return Self {
                    code: "conflict",
                    message: format!("This item {what} since you opened it{fields}. Your changes were not saved."),
                    details: serde_json::to_value(c.as_ref()).ok(),
                };
            }
            _ => "vault",
        };
        Self::new(code, e.to_string())
    }
}

impl From<crate::keymanager::KeyManagerError> for ApiError {
    fn from(e: crate::keymanager::KeyManagerError) -> Self {
        use crate::keymanager::KeyManagerError as E;
        match e {
            E::Vault(v) => v.into(),
            E::InUse(_) => Self::new("in_use", e.to_string()),
            E::MissingKey(_) => Self::new("not_found", e.to_string()),
            E::PublicOnly(_) | E::Key(_) => Self::new("key", e.to_string()),
        }
    }
}

impl From<SessionError> for ApiError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::Locked => Self::new("locked", "vault is locked"),
            SessionError::Vault(v) => v.into(),
            SessionError::Watch(w) => Self::new("watch", w.to_string()),
        }
    }
}

impl From<SshError> for ApiError {
    fn from(e: SshError) -> Self {
        let code = match &e {
            SshError::AuthFailed { .. } => "auth_failed",
            SshError::HostKeyChanged { .. } => "host_key_changed",
            SshError::HostKeyUnknown { .. } => "host_key_unknown",
            SshError::JumpUnreachable { .. } => "unreachable",
            SshError::Jump { .. } | SshError::JumpChainTooLong => "jump",
            SshError::AlreadyConnected => "already_connected",
            SshError::NotConnected => "not_connected",
            SshError::Connect { source, .. } => crate::neterr::kind(source).code(),
            SshError::Timeout(_) => "timeout",
            SshError::Proxy { .. } => "proxy",
            _ => "ssh",
        };
        Self::new(code, e.to_string())
    }
}

impl From<SftpError> for ApiError {
    fn from(e: SftpError) -> Self {
        match e {
            SftpError::Ssh(s) => s.into(),
            SftpError::NoSession => Self::new("not_connected", "SFTP session is not open"),
            SftpError::NoSubsystem => Self::new("sftp_unavailable", e.to_string()),
            SftpError::UntrustedCertificate(ref info) => Self { code: "ftp_certificate_unknown", message: e.to_string(), details: serde_json::to_value(&**info).ok() },
            SftpError::CertificateChanged { ref expected, ref found } => Self {
                code: "ftp_certificate_changed",
                message: e.to_string(),
                details: Some(serde_json::json!({ "expected": expected, "found": found })),
            },
            SftpError::Unsupported(_) => Self::new("unsupported", e.to_string()),
            SftpError::InvalidPath(_) => Self::new("invalid_path", e.to_string()),
            other => Self::new("sftp", other.to_string()),
        }
    }
}

impl From<crate::forward::ForwardError> for ApiError {
    fn from(e: crate::forward::ForwardError) -> Self {
        Self::new("forward", e.to_string())
    }
}

impl From<crate::rdp::RdpError> for ApiError {
    fn from(e: crate::rdp::RdpError) -> Self {
        use crate::rdp::RdpError as E;
        let message = e.to_string();
        match e {
            E::CertificateUnknown(info) => Self { code: "rdp_certificate_unknown", message, details: serde_json::to_value(&*info).ok() },
            E::CertificateChanged { expected, found } => Self {
                code: "rdp_certificate_changed",
                message,
                details: Some(serde_json::json!({ "expected": expected, "found": found })),
            },
            E::Credentials => Self::new("rdp_credentials", message),
            E::Unreachable(_) => Self::new("rdp_unreachable", message),
            E::NoSession => Self::new("no_session", message),
            E::Connect(_) => Self::new("rdp", message),
        }
    }
}

impl From<crate::vnc::VncError> for ApiError {
    fn from(e: crate::vnc::VncError) -> Self {
        use crate::vnc::VncError as E;
        let message = e.to_string();
        match e {
            E::PasswordRequired => Self::new("vnc_password_required", message),
            E::BadPassword => Self::new("vnc_bad_password", message),
            E::Unreachable(_) => Self::new("vnc_unreachable", message),
            E::NoSession => Self::new("no_session", message),
            E::Refused(_) | E::Protocol(_) | E::Io(_) => Self::new("vnc", message),
        }
    }
}

impl From<crate::inventory::InventoryError> for ApiError {
    fn from(e: crate::inventory::InventoryError) -> Self {
        use crate::inventory::InventoryError as E;
        match e {
            E::NotInstalled(_) => Self::new("inventory_missing", e.to_string()),
            _ => Self::new("inventory", e.to_string()),
        }
    }
}

impl From<crate::wol::WolError> for ApiError {
    fn from(e: crate::wol::WolError) -> Self {
        Self::new("wol", e.to_string())
    }
}

impl From<crate::mosh::MoshError> for ApiError {
    fn from(e: crate::mosh::MoshError) -> Self {
        use crate::mosh::MoshError as E;
        match e {
            E::ServerMissing(_) => Self::new("mosh_server_missing", e.to_string()),
            E::ClientMissing => Self::new("mosh_client_missing", e.to_string()),
            E::Ssh(s) => s.into(),
            other => Self::new("mosh", other.to_string()),
        }
    }
}

impl From<crate::containers::ContainerError> for ApiError {
    fn from(e: crate::containers::ContainerError) -> Self {
        use crate::containers::ContainerError as E;
        let code = match &e {
            E::Ssh(_) => "ssh",
            E::NotInstalled(_) => "not_installed",
            E::Invalid(_) => "validation",
            E::NoSession => "no_session",
            E::Timeout(_) => "timeout",
            E::Io(_) | E::Command(_) | E::Parse(_) => "container_error",
        };
        Self::new(code, e.to_string())
    }
}

impl From<crate::db::DbError> for ApiError {
    fn from(e: crate::db::DbError) -> Self {
        use crate::db::DbError;
        let code = match &e {
            DbError::NeedsConfirmation(_) => "needs_confirmation",
            DbError::Cancelled => "cancelled",
            DbError::NoSession => "no_session",
            DbError::Invalid(_) => "validation",
            DbError::Tunnel(_) => "forward",
            DbError::Connect { .. } | DbError::Timeout(_) => "db_connect",
            DbError::Server(_) => "db_error",
        };
        Self::new(code, e.to_string())
    }
}

impl From<ConfigError> for ApiError {
    fn from(e: ConfigError) -> Self {
        Self::new("config", e.to_string())
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn on_change_emitter(app: AppHandle) -> impl Fn(RecordChange) + Send + 'static {
    move |mut change| {
        if let Some(rec) = change.record.as_mut() {
            if let Some(data) = rec.data.take() {
                rec.data = crate::models::redact_record(change.collection, data);
                // Unreadable secret-bearing data is dropped, never sent as-is.
                if rec.data.is_none() && !rec.deleted {
                    return;
                }
            }
        }
        let _ = app.emit(EVENT_VAULT_CHANGED, &change);
    }
}


/// Tombstones older than this are physically removed on unlock. A computer
/// offline for longer could resurrect a record deleted elsewhere.
const TOMBSTONE_MAX_AGE_MS: u64 = 90 * 24 * 60 * 60 * 1000;

/// After an unlock: start the agent if this device has it switched on.
fn autostart_agent(app: &AppHandle, state: &AppState) {
    let enabled = AppConfig::load(&state.config_dir).map(|c| c.agent_enabled).unwrap_or(false);
    if enabled && state.agent.lock().unwrap_or_else(|p| p.into_inner()).is_none() {
        let _ = start_agent(app, state);
    }
}

fn purge_old_tombstones(state: &AppState) {
    let _ = state.session.with_vault(|v| {
        for c in Collection::ALL {
            v.purge_tombstones(c, TOMBSTONE_MAX_AGE_MS)?;
        }
        Ok(())
    });
    import_legacy_known_hosts(state);
}

/// Before v2, trusted host keys lived in a per-device file. Move them into
/// the vault once, then delete the file, so a key the user later replaces
/// or forgets isn't brought back. On any failure the file stays for the
/// next unlock.
fn import_legacy_known_hosts(state: &AppState) {
    let path = state.config_dir.join("known_hosts");
    if !path.is_file() {
        return;
    }
    let imported = state
        .session
        .with_vault(|v| Ok(crate::hostkeys::import_file(v, &path)));
    if let Ok(Ok(Ok(_))) = imported {
        let _ = std::fs::remove_file(&path);
    }
}

// ---------------------------------------------------------------------------
// Vault lifecycle
// ---------------------------------------------------------------------------

const MIN_PASSWORD: usize = 8;

fn check_new_password(p: &str) -> ApiResult<()> {
    if p.chars().count() < MIN_PASSWORD {
        return Err(ApiError::new(
            "weak_password",
            format!("master password must be at least {MIN_PASSWORD} characters"),
        ));
    }
    Ok(())
}

fn configured_root(cfg: &AppConfig) -> ApiResult<PathBuf> {
    cfg.vault_path
        .clone()
        .ok_or_else(|| ApiError::new("not_configured", "choose a vault folder first"))
}

fn status_of(state: &AppState, cfg: &AppConfig) -> VaultStatus {
    state
        .session
        .status(cfg.vault_path.as_deref(), cfg.remember_vault.is_some())
}

/// Store (or forget) the unlocked vault's key in the OS keychain. A keychain
/// failure never fails the unlock; it's reported so the UI can say so.
fn apply_remember(state: &AppState, remember: bool) -> Option<String> {
    let mut cfg = AppConfig::load(&state.config_dir).ok()?;
    let (vault_id, key) = state
        .session
        .with_vault(|v| Ok((v.vault_id(), v.master_key().clone())))
        .ok()?;
    let result = if remember {
        state.keystore.store(vault_id, &key).map(|_| Some(vault_id))
    } else if cfg.remember_vault.is_some() {
        state.keystore.forget(vault_id).map(|_| None)
    } else {
        Ok(None)
    };
    match result {
        Ok(v) => {
            cfg.remember_vault = v;
            let _ = cfg.save(&state.config_dir);
            None
        }
        Err(e) => Some(e.to_string()),
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct UnlockResult {
    pub status: VaultStatus,
    pub report: crate::session::UnlockReport,
    /// Set if "remember on this device" was requested but the keychain
    /// refused; the vault is unlocked regardless.
    pub keychain_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateResult {
    pub status: VaultStatus,
    /// Shown once, never stored. `None` unless requested.
    pub recovery_key: Option<String>,
    pub keychain_error: Option<String>,
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> ApiResult<VaultStatus> {
    let cfg = AppConfig::load(&state.config_dir)?;
    Ok(status_of(&state, &cfg))
}

#[tauri::command]
pub fn set_vault_path(state: State<'_, AppState>, path: String) -> ApiResult<VaultStatus> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(ApiError::new("invalid_path", "vault path must be absolute"));
    }
    state.session.lock();
    let mut cfg = AppConfig::load(&state.config_dir)?;
    if cfg.vault_path.as_ref() != Some(&path) {
        cfg.remember_vault = None; // a different vault isn't remembered
    }
    cfg.vault_path = Some(path);
    cfg.save(&state.config_dir)?;
    Ok(status_of(&state, &cfg))
}

#[tauri::command]
pub fn create_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    password: String,
    with_recovery: bool,
    remember: bool,
) -> ApiResult<CreateResult> {
    let password = Zeroizing::new(password);
    check_new_password(&password)?;
    let mut cfg = AppConfig::load(&state.config_dir)?;
    let root = configured_root(&cfg)?;
    let device = cfg.device(&state.config_dir)?;
    let rk = state.session.create(
        root,
        password.as_bytes(),
        crate::vault::CreateOptions {
            kdf: KdfParams::default(),
            with_recovery,
        },
        device,
        on_change_emitter(app),
    )?;
    let keychain_error = if remember {
        apply_remember(&state, true)
    } else {
        None
    };
    let cfg = AppConfig::load(&state.config_dir)?;
    Ok(CreateResult {
        status: status_of(&state, &cfg),
        recovery_key: rk.map(|k| k.display().to_string()),
        keychain_error,
    })
}

#[tauri::command]
pub async fn unlock_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    password: String,
    remember: bool,
) -> ApiResult<UnlockResult> {
    let app_for_agent = app.clone();
    let password = Zeroizing::new(password);
    let mut cfg = AppConfig::load(&state.config_dir)?;
    let root = configured_root(&cfg)?;
    let device = cfg.device(&state.config_dir)?;
    if let Err(wait) = state.unlock_guard.check(std::time::Instant::now()) {
        return Err(ApiError::new("locked_out", format!("Too many wrong passwords. Try again in {} seconds.", wait.as_secs() + 1)));
    }
    let report = match state.session.unlock(
        root,
        crate::vault::Unlock::Password(password.as_bytes()),
        device,
        KdfParams::default(),
        on_change_emitter(app),
    ) {
        Ok(report) => {
            state.unlock_guard.succeeded();
            report
        }
        Err(e) => {
            if matches!(e, crate::session::SessionError::Vault(VaultError::WrongPassword)) {
                state.unlock_guard.failed(std::time::Instant::now());
            }
            return Err(e.into());
        }
    };
    purge_old_tombstones(&state);
    let keychain_error = apply_remember(&state, remember);
    let cfg = AppConfig::load(&state.config_dir)?;
    autostart_agent(&app_for_agent, &state);
    autostart_cli(&app_for_agent, &state);
    Ok(UnlockResult {
        status: status_of(&state, &cfg),
        report,
        keychain_error,
    })
}

/// Unlock with the key stored in the OS keychain. On any failure the stored
/// key is left alone and the UI asks for the password instead.
#[tauri::command]
pub async fn unlock_with_device(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ApiResult<UnlockResult> {
    let app_for_agent = app.clone();
    let mut cfg = AppConfig::load(&state.config_dir)?;
    let root = configured_root(&cfg)?;
    let vault_id = cfg
        .remember_vault
        .ok_or_else(|| ApiError::new("not_remembered", "this device doesn't remember the vault"))?;
    let device = cfg.device(&state.config_dir)?;
    let store = Arc::clone(&state.keystore);
    let key = tauri::async_runtime::spawn_blocking(move || store.load(vault_id))
        .await
        .map_err(|e| ApiError::new("keychain", e.to_string()))?
        .map_err(|e| ApiError::new("keychain", e.to_string()))?
        .ok_or_else(|| ApiError::new("not_remembered", "no key for this vault in the keychain"))?;
    let report = match state.session.unlock(
        root,
        crate::vault::Unlock::Key(key),
        device,
        KdfParams::default(),
        on_change_emitter(app),
    ) {
        Ok(r) => r,
        Err(SessionError::Vault(VaultError::KeyMismatch)) => {
            // The folder now holds a different vault: stop offering the key.
            cfg.remember_vault = None;
            let _ = cfg.save(&state.config_dir);
            return Err(ApiError::new(
                "not_remembered",
                "the stored key doesn't match this vault; enter the master password",
            ));
        }
        Err(e) => return Err(e.into()),
    };
    purge_old_tombstones(&state);
    autostart_agent(&app_for_agent, &state);
    autostart_cli(&app_for_agent, &state);
    Ok(UnlockResult {
        status: status_of(&state, &cfg),
        report,
        keychain_error: None,
    })
}

/// "Forgot master password": unlock with the recovery key and set a new one.
#[tauri::command]
pub async fn unlock_with_recovery(
    app: AppHandle,
    state: State<'_, AppState>,
    recovery_key: String,
    new_password: String,
    remember: bool,
) -> ApiResult<UnlockResult> {
    let app_for_agent = app.clone();
    let recovery_key = Zeroizing::new(recovery_key);
    let new_password = Zeroizing::new(new_password);
    check_new_password(&new_password)?;
    let mut cfg = AppConfig::load(&state.config_dir)?;
    let root = configured_root(&cfg)?;
    let device = cfg.device(&state.config_dir)?;
    let report = state.session.reset_with_recovery(
        root,
        &recovery_key,
        new_password.as_bytes(),
        device,
        KdfParams::default(),
        on_change_emitter(app),
    )?;
    let keychain_error = apply_remember(&state, remember);
    let cfg = AppConfig::load(&state.config_dir)?;
    autostart_agent(&app_for_agent, &state);
    autostart_cli(&app_for_agent, &state);
    Ok(UnlockResult {
        status: status_of(&state, &cfg),
        report,
        keychain_error,
    })
}

/// Start remembering the unlocked vault on this device. Returns the
/// keychain's error if it refused; nothing else is stored in that case.
#[tauri::command]
pub fn remember_device(state: State<'_, AppState>) -> ApiResult<VaultStatus> {
    if let Some(err) = apply_remember(&state, true) {
        return Err(ApiError::new("keychain", err));
    }
    let cfg = AppConfig::load(&state.config_dir)?;
    Ok(status_of(&state, &cfg))
}

/// Remove this device's stored key.
#[tauri::command]
pub fn forget_device(state: State<'_, AppState>) -> ApiResult<VaultStatus> {
    let mut cfg = AppConfig::load(&state.config_dir)?;
    if let Some(id) = cfg.remember_vault.take() {
        state
            .keystore
            .forget(id)
            .map_err(|e| ApiError::new("keychain", e.to_string()))?;
        cfg.save(&state.config_dir)?;
    }
    Ok(status_of(&state, &cfg))
}

#[tauri::command]
pub async fn lock_vault(state: State<'_, AppState>) -> ApiResult<VaultStatus> {
    lock_everything(&state).await
}

/// The window reports that someone is there (and how long an idle vault may wait, in minutes; 0 for never). If these
/// reports stop for that long the backend locks the vault itself.
#[tauri::command]
pub fn vault_activity(state: State<'_, AppState>, minutes: u32) {
    state.idle.touch(minutes);
}

/// End every session, transfer, tunnel, run and child program, and drop the agent and the command-line socket. What
/// locking and quitting have in common; neither leaves anything running behind the vault's back.
pub async fn close_everything(state: &AppState) {
    state.ssh.disconnect_all().await;
    state.sftp.close_all().await;
    state.files.close_all().await;
    state.forwards.stop_all();
    state.runs.cancel_all();
    state.local.close_all();
    state.rdp.close_all();
    state.vnc.close_all();
    state.runbooks.cancel_all();
    state.raw.close_all();
    state.dbs.close_all().await;
    state.containers.close_all().await;
    state.kube.close_all().await;
    state.monitors.close_all().await;
    state.edits.stop_all();
    // The agent serves vault keys, so it goes when the vault locks, and
    // the command-line socket with it.
    *state.agent.lock().unwrap_or_else(|p| p.into_inner()) = None;
    *state.control.lock().unwrap_or_else(|p| p.into_inner()) = None;
    state
        .reveal
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .close();
    state.logs.lock().unwrap_or_else(|p| p.into_inner()).clear();
}

/// Everything a lock does: close what the vault's keys opened, then lock.
pub async fn lock_everything(state: &AppState) -> ApiResult<VaultStatus> {
    state.idle.reset();
    close_everything(state).await;
    state.session.lock();
    let mut cfg = AppConfig::load(&state.config_dir)?;
    // A locked vault that opens itself from the device key isn't locked: forget the key too, unless the person chose
    // to keep it. The lock has happened either way, so a keychain that refuses is not an error here.
    if !cfg.keep_key_on_lock {
        if let Some(id) = cfg.remember_vault {
            if state.keystore.forget(id).is_ok() {
                cfg.remember_vault = None;
                let _ = cfg.save(&state.config_dir);
            }
        }
    }
    Ok(status_of(state, &cfg))
}

/// What the crash log holds, for the Diagnostics section and the "closed unexpectedly" notice. Needs no unlocked vault.
#[derive(Debug, Clone, Serialize)]
pub struct CrashLogView {
    pub path: PathBuf,
    /// Newest first.
    pub entries: Vec<crate::crashlog::Entry>,
}

#[tauri::command]
pub fn crash_log(state: State<'_, AppState>) -> CrashLogView {
    CrashLogView { path: crate::crashlog::path(&state.config_dir), entries: crate::crashlog::recent(&state.config_dir, 20) }
}

#[tauri::command]
pub fn clear_crash_log(state: State<'_, AppState>) -> ApiResult<()> {
    crate::crashlog::clear(&state.config_dir).map_err(|e| ApiError::new("io", e.to_string()))
}

/// Whether locking keeps this device's stored key (see `AppConfig::keep_key_on_lock`).
#[tauri::command]
pub fn set_keep_key_on_lock(state: State<'_, AppState>, keep: bool) -> ApiResult<VaultStatus> {
    let mut cfg = AppConfig::load(&state.config_dir)?;
    cfg.keep_key_on_lock = keep;
    cfg.save(&state.config_dir)?;
    Ok(status_of(&state, &cfg))
}

/// Check a password against the unlocked vault without holding it during
/// the (deliberately slow) key derivation.
async fn verify_master_password(state: &AppState, password: &str) -> ApiResult<()> {
    let (vault_id, slot) = state
        .session
        .with_vault(|v| Ok((v.vault_id(), v.password_slot())))?;
    let pw = Zeroizing::new(password.to_string());
    let key = tauri::async_runtime::spawn_blocking(move || {
        crate::vault::format::open_slot(vault_id, &slot, pw.as_bytes())
    })
    .await
    .map_err(|e| ApiError::new("vault", e.to_string()))??;
    let ok = match key {
        Some(k) => state.session.with_vault(|v| Ok(v.master_key().ct_eq(&k)))?,
        None => false,
    };
    if ok {
        Ok(())
    } else {
        Err(VaultError::WrongPassword.into())
    }
}

/// Change the master password. Only the key slot is rewritten; the records
/// and any "remember on this device" key stay valid.
#[tauri::command]
pub async fn change_master_password(
    state: State<'_, AppState>,
    current_password: String,
    new_password: String,
) -> ApiResult<()> {
    let current = Zeroizing::new(current_password);
    let new = Zeroizing::new(new_password);
    check_new_password(&new)?;
    verify_master_password(&state, &current).await?;
    state.session.with_vault_mut(|v| {
        v.change_password(current.as_bytes(), new.as_bytes(), KdfParams::default())
    })?;
    Ok(())
}

/// Create or replace the recovery key. Returned once for the user to write
/// down; the previous one stops working.
#[tauri::command]
pub async fn set_recovery_key(
    state: State<'_, AppState>,
    master_password: String,
) -> ApiResult<String> {
    verify_master_password(&state, &Zeroizing::new(master_password)).await?;
    let rk = state
        .session
        .with_vault_mut(|v| v.set_recovery(KdfParams::default()))?;
    Ok(rk.display().to_string())
}

#[tauri::command]
pub async fn remove_recovery_key(
    state: State<'_, AppState>,
    master_password: String,
) -> ApiResult<()> {
    verify_master_password(&state, &Zeroizing::new(master_password)).await?;
    Ok(state.session.with_vault_mut(|v| v.remove_recovery())?)
}

#[derive(Debug, Clone, Serialize)]
pub struct VaultInfo {
    pub vault_id: Uuid,
    pub path: PathBuf,
    pub format_version: u32,
    pub cipher: String,
    pub kdf: String,
    pub has_recovery: bool,
    pub device_id: Uuid,
    pub device_name: String,
    pub remembered: bool,
    /// Locking keeps the remembered key (the setting is off by default).
    pub keep_key_on_lock: bool,
    /// Short hash of every record's ID and revision; equal on two devices
    /// means they have the same data.
    pub state_hash: String,
    pub records: usize,
    pub last_change: u64,
    pub last_backup: Option<u64>,
    pub active_sessions: Vec<crate::vault::devices::ActiveSession>,
    pub devices: Vec<Record<crate::vault::devices::DeviceRecord>>,
    pub open_conflicts: usize,
    /// Records that went backwards or vanished since this device last saw them.
    pub rollbacks: Vec<crate::vault::highwater::Anomaly>,
}

/// What the status bar needs to know about the synced folder, cheaply (no decrypting): how many conflict copies wait for a
/// decision, and how many records went backwards or vanished since this computer last saw them.
#[derive(Debug, Clone, Serialize)]
pub struct VaultHealth {
    pub open_conflicts: usize,
    pub rollbacks: usize,
}

#[tauri::command]
pub fn vault_health(state: State<'_, AppState>) -> ApiResult<VaultHealth> {
    Ok(state.session.with_vault(|v| Ok(VaultHealth { open_conflicts: v.conflict_copies()?.len(), rollbacks: v.rollbacks().len() }))?)
}

/// The person has looked at the rollback warning: take the folder as it is now as the baseline.
#[tauri::command]
pub fn accept_rollbacks(state: State<'_, AppState>) -> ApiResult<()> {
    Ok(state.session.with_vault(|v| v.accept_rollbacks())?)
}

#[tauri::command]
pub fn vault_info(state: State<'_, AppState>) -> ApiResult<VaultInfo> {
    let cfg = AppConfig::load(&state.config_dir)?;
    Ok(state.session.with_vault(|v| {
        let slot = v.password_slot();
        let (state_hash, records, last_change) = v.state_hash()?;
        Ok(VaultInfo {
            vault_id: v.vault_id(),
            path: v.root().to_path_buf(),
            format_version: v.manifest().version,
            cipher: v.manifest().cipher.clone(),
            kdf: format!(
                "{} (m={} MiB, t={}, p={})",
                slot.kdf.algorithm,
                slot.kdf.m_cost_kib / 1024,
                slot.kdf.t_cost,
                slot.kdf.p_cost
            ),
            has_recovery: v.has_recovery(),
            device_id: v.device().id,
            device_name: v.device().name.clone(),
            remembered: cfg.remember_vault == Some(v.vault_id()),
            keep_key_on_lock: cfg.keep_key_on_lock,
            state_hash,
            records,
            last_change,
            last_backup: v.last_backup_at(),
            active_sessions: v.active_sessions()?,
            devices: v.devices()?,
            open_conflicts: v.conflict_copies()?.len(),
            rollbacks: v.check_rollbacks()?,
        })
    })?)
}

#[tauri::command]
pub fn list_backups(
    state: State<'_, AppState>,
) -> ApiResult<Vec<crate::vault::backup::BackupInfo>> {
    Ok(state.session.with_vault(|v| Ok(v.list_backups()))?)
}

#[tauri::command]
pub fn create_backup(state: State<'_, AppState>) -> ApiResult<crate::vault::backup::BackupInfo> {
    let keep = state
        .session
        .backup_retention
        .load(std::sync::atomic::Ordering::Relaxed);
    Ok(state
        .session
        .with_vault(|v| v.create_backup("manual", keep))?)
}

#[tauri::command]
pub fn restore_backup(
    state: State<'_, AppState>,
    file_name: String,
) -> ApiResult<crate::vault::backup::RestoreReport> {
    let keep = state
        .session
        .backup_retention
        .load(std::sync::atomic::Ordering::Relaxed);
    Ok(state
        .session
        .with_vault(|v| v.restore_backup(&file_name, keep))?)
}

#[tauri::command]
pub fn verify_integrity(
    state: State<'_, AppState>,
) -> ApiResult<crate::vault::integrity::IntegrityReport> {
    Ok(state.session.with_vault(|v| Ok(v.verify_integrity()))?)
}

/// Unresolved sync conflicts. Credential secrets are redacted: the UI shows
/// that they differ, not what they are.
#[tauri::command]
pub fn list_conflicts(
    state: State<'_, AppState>,
) -> ApiResult<Vec<crate::vault::conflicts::ConflictInfo>> {
    let mut list = state.session.with_vault(|v| {
        v.reconcile()?;
        v.conflicts()
    })?;
    for c in &mut list {
        if c.collection == Collection::Identities || c.collection == Collection::Keys {
            for r in [&mut c.current, &mut c.other] {
                r.data = r.data.take().map(redact_secrets);
            }
        }
    }
    Ok(list)
}

/// Blank every field that can hold a secret, keeping the shape.
fn redact_secrets(mut v: serde_json::Value) -> serde_json::Value {
    const SECRET: &[&str] = &[
        "password",
        "private_key",
        "passphrase",
        "certificate_private",
    ];
    fn walk(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, x) in m.iter_mut() {
                    if SECRET.contains(&k.as_str()) && !x.is_null() {
                        *x = serde_json::Value::String("••••••".into());
                    } else {
                        walk(x);
                    }
                }
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(walk),
            _ => {}
        }
    }
    walk(&mut v);
    v
}

#[tauri::command]
pub fn resolve_conflict(
    state: State<'_, AppState>,
    collection: Collection,
    id: Uuid,
    file_name: String,
    keep: String,
) -> ApiResult<()> {
    use crate::vault::conflicts::Resolution;
    let choice = match keep.as_str() {
        "current" => Resolution::KeepCurrent,
        "other" => Resolution::KeepOther,
        _ => {
            return Err(ApiError::new(
                "validation",
                "keep must be \"current\" or \"other\"",
            ))
        }
    };
    Ok(state
        .session
        .with_vault(|v| v.resolve_conflict(collection, id, &file_name, choice))?)
}

/// Copy the vault to `destination` (a new or empty folder), verify the copy,
/// then switch this device to it. The old folder is left for the user to
/// delete once every device has switched.
#[tauri::command]
pub fn move_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    destination: String,
) -> ApiResult<VaultStatus> {
    let dest = PathBuf::from(destination);
    if !dest.is_absolute() {
        return Err(ApiError::new("invalid_path", "choose an absolute folder"));
    }
    if dest.exists()
        && std::fs::read_dir(&dest)
            .map(|mut d| d.next().is_some())
            .unwrap_or(true)
    {
        return Err(ApiError::new(
            "invalid_path",
            "the destination folder must be empty",
        ));
    }
    let (root, key) = state
        .session
        .with_vault(|v| Ok((v.root().to_path_buf(), v.master_key().clone())))?;
    if dest.starts_with(&root) {
        return Err(ApiError::new(
            "invalid_path",
            "the destination can't be inside the vault",
        ));
    }
    copy_tree(&root, &dest).map_err(|e| ApiError::new("io", e.to_string()))?;

    let mut cfg = AppConfig::load(&state.config_dir)?;
    let device = cfg.device(&state.config_dir)?;
    // Verify the copy opens with the same key and is intact before switching.
    let copy = crate::vault::Vault::open(
        &dest,
        crate::vault::Unlock::Key(key.clone()),
        device.clone(),
    )?;
    let report = copy.verify_integrity();
    drop(copy);
    if !report.ok {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(ApiError::new(
            "integrity",
            format!("the copy failed verification: {}", report.errors.join("; ")),
        ));
    }
    state.session.lock();
    cfg.vault_path = Some(dest.clone());
    cfg.save(&state.config_dir)?;
    state.session.unlock(
        dest,
        crate::vault::Unlock::Key(key),
        device,
        KdfParams::default(),
        on_change_emitter(app),
    )?;
    Ok(status_of(&state, &cfg))
}

/// Copy a vault folder, skipping locks and our own temp files. Refuses to
/// follow symlinks, so a planted link can't pull outside files into the copy.
fn copy_tree(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    crate::vault::atomic::create_private_dir(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let name = e.file_name();
        let name_s = name.to_string_lossy();
        if name_s == "locks" || crate::vault::atomic::is_temp_name(&name_s) {
            continue;
        }
        let ft = e.file_type()?;
        let dest = to.join(&name);
        if ft.is_symlink() {
            continue;
        } else if ft.is_dir() {
            copy_tree(&e.path(), &dest)?;
        } else if ft.is_file() {
            std::fs::copy(e.path(), &dest)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Generic record helpers
// ---------------------------------------------------------------------------

fn list_records<T>(state: &AppState, collection: Collection) -> ApiResult<Vec<Record<T>>>
where
    T: serde::de::DeserializeOwned,
{
    Ok(state
        .session
        .with_vault(|v| v.list::<T>(collection))?
        .records)
}

/// Run Key Manager logic against the unlocked vault.
fn with_keys<R>(
    state: &AppState,
    f: impl FnOnce(&crate::vault::Vault) -> crate::keymanager::Result<R>,
) -> ApiResult<R> {
    Ok(state.session.with_vault(|v| Ok(f(v)))??)
}

/// `base_rev` is the revision the UI was editing; `None` for a new record.
fn base_for(id: Option<Uuid>, base_rev: Option<u64>) -> crate::vault::Base {
    match (id, base_rev) {
        (None, _) => crate::vault::Base::New,
        (Some(_), Some(r)) => crate::vault::Base::Rev(r),
        (Some(_), None) => crate::vault::Base::Latest,
    }
}

fn save_record<T>(
    state: &AppState,
    collection: Collection,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    data: T,
) -> ApiResult<Record<T>>
where
    T: Serialize + serde::de::DeserializeOwned,
{
    let base = base_for(id, base_rev);
    let id = id.unwrap_or_else(Uuid::new_v4);
    Ok(state
        .session
        .with_vault(|v| v.put(collection, id, &data, base))?)
}

/// A deleted record, held in memory (never written anywhere) so "Undo" can
/// put it back exactly, secrets included, without those secrets ever
/// reaching the webview.
struct Trashed {
    collection: Collection,
    id: Uuid,
    data: serde_json::Value,
    at: std::time::Instant,
    /// Records removed along with it (a host's own credential), restored first.
    along: Vec<(Collection, Uuid, serde_json::Value)>,
    /// Hosts this credential was detached from, re-attached on undo.
    detached_from: Vec<Uuid>,
}

const UNDO_WINDOW: std::time::Duration = std::time::Duration::from_secs(10 * 60);
const TRASH_MAX: usize = 50;

fn stash(state: &AppState, t: Trashed) {
    let mut trash = state.trash.lock().unwrap_or_else(|p| p.into_inner());
    trash.retain(|x| x.at.elapsed() < UNDO_WINDOW && !(x.collection == t.collection && x.id == t.id));
    trash.push(t);
    if trash.len() > TRASH_MAX {
        trash.remove(0);
    }
}

fn delete_record(
    state: &AppState,
    collection: Collection,
    id: Uuid,
    base_rev: Option<u64>,
) -> ApiResult<()> {
    delete_record_with(state, collection, id, base_rev, Vec::new(), Vec::new())
}

fn delete_record_with(
    state: &AppState,
    collection: Collection,
    id: Uuid,
    base_rev: Option<u64>,
    along: Vec<(Collection, Uuid, serde_json::Value)>,
    detached_from: Vec<Uuid>,
) -> ApiResult<()> {
    let base = base_for(Some(id), base_rev);
    let data = state.session.with_vault(|v| {
        let data = v.get::<serde_json::Value>(collection, id)?.data;
        v.delete(collection, id, base)?;
        Ok(data)
    })?;
    if let Some(data) = data {
        stash(
            state,
            Trashed {
                collection,
                id,
                data,
                at: std::time::Instant::now(),
                along,
                detached_from,
            },
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Vault-backed SSH agent
// ---------------------------------------------------------------------------

/// Event asking the UI to approve one agent signature. Payload: [`AgentPrompt`].
pub const EVENT_AGENT_PROMPT: &str = "agent:prompt";
const AGENT_PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Debug, Clone, Serialize)]
pub struct AgentPrompt {
    pub request_id: Uuid,
    pub key_name: String,
    pub fingerprint: String,
    /// The server asking, for forwarded agent requests; absent for local programs.
    pub origin: Option<String>,
}

/// Offers Key Manager keys marked for the agent, from the unlocked vault.
struct VaultAgent(AppHandle);

impl VaultAgent {
    /// (record, key, decrypted private key) for every offered key.
    fn offered(&self) -> Vec<(crate::models::SshKey, russh::keys::PrivateKey)> {
        let state = self.0.state::<AppState>();
        let keys = state
            .session
            .with_vault(|v| Ok(v.list::<crate::models::SshKey>(Collection::Keys)?.records))
            .unwrap_or_default();
        keys.into_iter()
            .filter_map(|r| r.data)
            .filter(|k| !k.agent.is_off())
            .filter_map(|k| {
                let text = zeroize::Zeroizing::new(k.private_key.clone()?);
                // Encrypted keys are offered only when their passphrase is saved.
                let pk = russh::keys::decode_secret_key(&text, k.passphrase.as_deref()).ok()?;
                Some((k, pk))
            })
            .collect()
    }
}

impl crate::agent::Backend for VaultAgent {
    fn identities(&self) -> Vec<crate::agent::Offered> {
        self.offered()
            .into_iter()
            .map(|(k, pk)| crate::agent::Offered {
                public: pk.public_key().clone(),
                comment: format!("sshvault:{}", k.name),
            })
            .collect()
    }

    fn private(&self, public: &russh::keys::PublicKey) -> Option<russh::keys::PrivateKey> {
        self.offered()
            .into_iter()
            .find(|(_, pk)| pk.public_key().key_data() == public.key_data())
            .map(|(_, pk)| pk)
    }

    fn approve(&self, public: &russh::keys::PublicKey, _comment: &str, origin: Option<&str>) -> crate::agent::BoxFuture<bool> {
        let origin = origin.map(str::to_string);
        let found = self
            .offered()
            .into_iter()
            .find(|(_, pk)| pk.public_key().key_data() == public.key_data())
            .map(|(k, _)| k);
        let app = self.0.clone();
        Box::pin(async move {
            let Some(k) = found else { return false };
            match k.agent {
                crate::models::AgentUse::Allow => true,
                crate::models::AgentUse::Off => false,
                crate::models::AgentUse::Ask => {
                    let state = app.state::<AppState>();
                    let request_id = Uuid::new_v4();
                    let (tx, rx) = tokio::sync::oneshot::channel();
                    state
                        .agent_prompts
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .insert(request_id, tx);
                    let prompt = AgentPrompt {
                        request_id,
                        key_name: k.name.clone(),
                        fingerprint: k.fingerprint.clone(),
                        origin,
                    };
                    let ok = match app.emit(EVENT_AGENT_PROMPT, prompt) {
                        Ok(()) => matches!(tokio::time::timeout(AGENT_PROMPT_TIMEOUT, rx).await, Ok(Ok(true))),
                        Err(_) => false,
                    };
                    state
                        .agent_prompts
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .remove(&request_id);
                    ok
                }
            }
        })
    }
}

fn agent_dir(app: &AppHandle) -> ApiResult<PathBuf> {
    // XDG_RUNTIME_DIR is per-user, memory-backed and cleared at logout.
    if let Some(run) = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(run).join("sshvault"));
    }
    app.path()
        .app_cache_dir()
        .map(|d| d.join("agent"))
        .map_err(|e| ApiError::new("agent", e.to_string()))
}

/// Start listening. Must be called from within the async runtime (the async
/// unlock commands and `agent_set_enabled` are).
fn start_agent(app: &AppHandle, state: &AppState) -> ApiResult<String> {
    let dir = agent_dir(app)?;
    let handle = crate::agent::start(&dir, Arc::clone(&state.vault_agent))
        .map_err(|e| ApiError::new("agent", format!("could not start the SSH agent: {e}")))?;
    let path = handle.path.clone();
    *state.agent.lock().unwrap_or_else(|p| p.into_inner()) = Some(handle);
    Ok(path)
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStatus {
    pub running: bool,
    /// Socket path, or the named pipe on Windows.
    pub path: Option<String>,
    /// Start automatically when the vault unlocks on this computer.
    pub enabled: bool,
}

fn agent_status_of(state: &AppState) -> AgentStatus {
    let path = state
        .agent
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .as_ref()
        .map(|h| h.path.clone());
    AgentStatus {
        running: path.is_some(),
        path,
        enabled: AppConfig::load(&state.config_dir).map(|c| c.agent_enabled).unwrap_or(false),
    }
}

#[tauri::command]
pub fn agent_status(state: State<'_, AppState>) -> AgentStatus {
    agent_status_of(&state)
}

/// Turn the agent on or off for this computer (and start or stop it now).
#[tauri::command]
pub async fn agent_set_enabled(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> ApiResult<AgentStatus> {
    let mut cfg = AppConfig::load(&state.config_dir)?;
    cfg.agent_enabled = enabled;
    cfg.save(&state.config_dir)?;
    if enabled {
        state.session.with_vault(|_| Ok(()))?;
        if state.agent.lock().unwrap_or_else(|p| p.into_inner()).is_none() {
            start_agent(&app, &state)?;
        }
    } else {
        *state.agent.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
    Ok(agent_status_of(&state))
}

#[tauri::command]
pub fn answer_agent_request(state: State<'_, AppState>, request_id: Uuid, allow: bool) -> ApiResult<()> {
    let tx = state
        .agent_prompts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&request_id)
        .ok_or_else(|| ApiError::new("expired", "that request has expired"))?;
    let _ = tx.send(allow);
    Ok(())
}

/// Choose whether the agent offers a key: off, ask each time, or allow.
#[tauri::command]
pub fn set_key_agent(
    state: State<'_, AppState>,
    id: Uuid,
    base_rev: Option<u64>,
    mode: crate::models::AgentUse,
) -> ApiResult<Record<crate::models::SshKey>> {
    let mut key = load_key(&state, id)?.data.expect("checked");
    if !mode.is_off() && key.private_key.is_none() {
        return Err(ApiError::new("key", "a public key can't be used by the agent"));
    }
    if !mode.is_off() && key.encrypted && key.passphrase.is_none() {
        return Err(ApiError::new(
            "key",
            "save this key's passphrase in the vault first; the agent can't ask for it",
        ));
    }
    key.agent = mode;
    save_record(&state, Collection::Keys, Some(id), base_rev, key).map(redacted_key)
}

// ---------------------------------------------------------------------------
// Command-line control (`sshvault list/connect/run/status`)
// ---------------------------------------------------------------------------

/// Asks the UI to open a tab. Payload: `{ host_id, label }`.
pub const EVENT_CLI_OPEN: &str = "cli:open";
/// Asks the UI to approve a `run`. Payload: [`CliPrompt`].
pub const EVENT_CLI_PROMPT: &str = "cli:prompt";
const CLI_PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

#[derive(Debug, Clone, Serialize)]
pub struct CliPrompt {
    pub request_id: Uuid,
    pub command: String,
    pub hosts: Vec<String>,
    pub production: usize,
}

struct AppControl(AppHandle);

impl AppControl {
    fn hosts(&self) -> Result<Vec<(Uuid, Host)>, String> {
        let state = self.0.state::<AppState>();
        state
            .session
            .with_vault(|v| Ok(v.list::<Host>(Collection::Hosts)?.records))
            .map(|rs| rs.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect())
            .map_err(|_| "the vault is locked".to_string())
    }

    async fn approve(&self, command: &str, hosts: &[(Uuid, Host)]) -> bool {
        let state = self.0.state::<AppState>();
        let trusted = state
            .cli_trusted_until
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .is_some_and(|t| std::time::Instant::now() < t);
        // Production hosts are always confirmed, even inside a trust window.
        let groups = state.session.with_vault(|v| Ok(crate::keymanager::load_groups(v).unwrap_or_default())).unwrap_or_default();
        let production = hosts
            .iter()
            .filter(|(_, h)| crate::keymanager::effective(h, &groups).environment == "production")
            .count();
        if trusted && production == 0 {
            return true;
        }
        let request_id = Uuid::new_v4();
        let (tx, rx) = tokio::sync::oneshot::channel();
        state.cli_prompts.lock().unwrap_or_else(|p| p.into_inner()).insert(request_id, tx);
        let prompt = CliPrompt {
            request_id,
            command: command.to_string(),
            hosts: hosts.iter().map(|(_, h)| h.label.clone()).collect(),
            production,
        };
        if let Some(w) = self.0.get_webview_window("main") {
            let _ = w.request_user_attention(Some(tauri::UserAttentionType::Informational));
        }
        let ok = match self.0.emit(EVENT_CLI_PROMPT, prompt) {
            Ok(()) => matches!(tokio::time::timeout(CLI_PROMPT_TIMEOUT, rx).await, Ok(Ok(true))),
            Err(_) => false,
        };
        state.cli_prompts.lock().unwrap_or_else(|p| p.into_inner()).remove(&request_id);
        ok
    }
}

struct CliRunSink(tokio::sync::mpsc::UnboundedSender<crate::control::Response>, std::collections::HashMap<Uuid, String>);
impl crate::runner::RunSink for CliRunSink {
    fn event(&self, e: crate::runner::RunEvent) {
        use crate::control::Response as R;
        use crate::runner::RunEvent as E;
        let name = |id: &Uuid| self.1.get(id).cloned().unwrap_or_default();
        let resp = match e {
            E::Started { host_id } => R::Started { host: name(&host_id) },
            E::Finished { host_id, output } => R::Output {
                host: name(&host_id),
                stdout: output.stdout,
                stderr: output.stderr,
                exit_code: output.exit_code,
                duration_ms: output.duration_ms,
            },
            E::Failed { host_id, message } => R::Failed { host: name(&host_id), message },
            E::Skipped { host_id } => R::Failed { host: name(&host_id), message: "skipped because an earlier host failed".into() },
            E::Done => R::Done { ok: true },
        };
        let _ = self.0.send(resp);
    }
}

impl crate::control::Handler for AppControl {
    fn handle(
        &self,
        req: crate::control::Request,
        out: tokio::sync::mpsc::UnboundedSender<crate::control::Response>,
    ) -> crate::control::BoxFuture<()> {
        use crate::control::{Request, Response};
        let me = AppControl(self.0.clone());
        Box::pin(async move {
            let fail = |m: String| {
                let _ = out.send(Response::Error { message: m });
            };
            match req {
                Request::Status => {
                    let state = me.0.state::<AppState>();
                    let hosts = me.hosts();
                    let agent = state.agent.lock().unwrap_or_else(|p| p.into_inner()).as_ref().map(|h| h.path.clone());
                    let _ = out.send(Response::Status {
                        unlocked: hosts.is_ok(),
                        hosts: hosts.map(|h| h.len()).unwrap_or(0),
                        agent,
                    });
                    let _ = out.send(Response::Done { ok: true });
                }
                Request::List { select } => match me.hosts().and_then(|hs| {
                    crate::control::select(&hs, &select).map(|v| v.into_iter().cloned().collect::<Vec<_>>())
                }) {
                    Ok(list) => {
                        for (_, h) in list {
                            let _ = out.send(Response::Host {
                                label: h.label,
                                hostname: h.hostname,
                                port: h.port,
                                group: h.group,
                                environment: h.environment,
                                tags: h.tags,
                            });
                        }
                        let _ = out.send(Response::Done { ok: true });
                    }
                    Err(e) => fail(e),
                },
                Request::Connect { host } => {
                    let sel = crate::control::Selector { hosts: vec![host], ..Default::default() };
                    match me.hosts().and_then(|hs| crate::control::select(&hs, &sel).map(|v| v[0].clone())) {
                        Ok((id, h)) => {
                            let _ = me.0.emit(EVENT_CLI_OPEN, serde_json::json!({ "host_id": id, "label": h.label }));
                            if let Some(w) = me.0.get_webview_window("main") {
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                            let _ = out.send(Response::Opened { label: h.label });
                            let _ = out.send(Response::Done { ok: true });
                        }
                        Err(e) => fail(e),
                    }
                }
                Request::Run { select, command, timeout_secs } => {
                    let hosts = match me.hosts().and_then(|hs| {
                        crate::control::select(&hs, &select).map(|v| v.into_iter().cloned().collect::<Vec<_>>())
                    }) {
                        Ok(h) if h.is_empty() => return fail("no hosts matched".into()),
                        Ok(h) => h,
                        Err(e) => return fail(e),
                    };
                    let _ = out.send(Response::Waiting {
                        message: format!("Approve running this on {} host(s) in the SSHVault window…", hosts.len()),
                    });
                    if !me.approve(&command, &hosts).await {
                        return fail("not approved in the app".into());
                    }
                    let state = me.0.state::<AppState>();
                    let names: std::collections::HashMap<Uuid, String> = hosts.iter().map(|(id, h)| (*id, h.label.clone())).collect();
                    let sink: Arc<dyn crate::runner::RunSink> = Arc::new(CliRunSink(out.clone(), names));
                    let mut ready = Vec::new();
                    for (id, h) in &hosts {
                        match resolve_target(&state, *id, None) {
                            Ok(t) => ready.push((*id, t, command.clone())),
                            Err(e) => {
                                let _ = out.send(Response::Failed { host: h.label.clone(), message: e.message });
                            }
                        }
                    }
                    if ready.is_empty() {
                        let _ = out.send(Response::Done { ok: false });
                        return;
                    }
                    state.runs.start(
                        format!("cli-{}", Uuid::new_v4()),
                        ready,
                        std::time::Duration::from_secs(timeout_secs.clamp(1, 3600)),
                        sink,
                    );
                }
            }
        })
    }
}

fn autostart_cli(app: &AppHandle, state: &AppState) {
    let enabled = AppConfig::load(&state.config_dir).map(|c| c.cli_enabled).unwrap_or(false);
    if enabled && state.control.lock().unwrap_or_else(|p| p.into_inner()).is_none() {
        let _ = start_cli(app, state);
    }
}

/// Must run within the async runtime (async commands do).
fn start_cli(app: &AppHandle, state: &AppState) -> ApiResult<()> {
    let handle = crate::control::start(Arc::new(AppControl(app.clone())))
        .map_err(|e| ApiError::new("cli", format!("could not start command-line access: {e}")))?;
    *state.control.lock().unwrap_or_else(|p| p.into_inner()) = Some(handle);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct CliStatus {
    pub enabled: bool,
    pub running: bool,
    /// This program, to put on PATH (or alias) as `sshvault`.
    pub executable: Option<String>,
}

fn cli_status_of(state: &AppState) -> CliStatus {
    CliStatus {
        enabled: AppConfig::load(&state.config_dir).map(|c| c.cli_enabled).unwrap_or(false),
        running: state.control.lock().unwrap_or_else(|p| p.into_inner()).is_some(),
        executable: std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned()),
    }
}

#[tauri::command]
pub fn cli_status(state: State<'_, AppState>) -> CliStatus {
    cli_status_of(&state)
}

#[tauri::command]
pub async fn cli_set_enabled(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> ApiResult<CliStatus> {
    let mut cfg = AppConfig::load(&state.config_dir)?;
    cfg.cli_enabled = enabled;
    cfg.save(&state.config_dir)?;
    if enabled {
        state.session.with_vault(|_| Ok(()))?;
        if state.control.lock().unwrap_or_else(|p| p.into_inner()).is_none() {
            start_cli(&app, &state)?;
        }
    } else {
        *state.control.lock().unwrap_or_else(|p| p.into_inner()) = None;
        *state.cli_trusted_until.lock().unwrap_or_else(|p| p.into_inner()) = None;
    }
    Ok(cli_status_of(&state))
}

/// Approve or refuse a `run` from the command line. `trust_minutes` skips
/// the question for further non-production runs for that long.
#[tauri::command]
pub fn answer_cli_request(
    state: State<'_, AppState>,
    request_id: Uuid,
    allow: bool,
    trust_minutes: Option<u32>,
) -> ApiResult<()> {
    let tx = state
        .cli_prompts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&request_id)
        .ok_or_else(|| ApiError::new("expired", "that request has expired"))?;
    if allow {
        if let Some(m) = trust_minutes.filter(|m| *m > 0) {
            *state.cli_trusted_until.lock().unwrap_or_else(|p| p.into_inner()) =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(u64::from(m.min(60)) * 60));
        }
    }
    let _ = tx.send(allow);
    Ok(())
}

// ---------------------------------------------------------------------------
// Hosts / identities / snippets
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_hosts(state: State<'_, AppState>) -> ApiResult<Vec<Record<Host>>> {
    list_records(&state, Collection::Hosts)
}

#[tauri::command]
pub fn save_host(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    host: Host,
) -> ApiResult<Record<Host>> {
    if host.label.trim().is_empty() || host.hostname.trim().is_empty() {
        return Err(ApiError::new(
            "validation",
            "label and hostname are required",
        ));
    }
    // Reject loops and dangling references before they reach the vault.
    let hosts = host_jump_map(&state)?;
    jump_chain(id, host.jump_host_id, MAX_JUMPS, |h| hosts.get(&h).copied())
        .map_err(|e| ApiError::new("validation", e.to_string()))?;
    save_record(&state, Collection::Hosts, id, base_rev, host)
}

/// Save a host together with the credentials entered in its form. See
/// `hostcreds.rs` for the modes and how stored secrets are kept.
#[tauri::command]
pub fn save_host_with_credentials(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    host: Host,
    credentials: crate::hostcreds::HostCredentials,
) -> ApiResult<crate::hostcreds::SaveOutcome> {
    if host.label.trim().is_empty() || host.hostname.trim().is_empty() {
        return Err(ApiError::new(
            "validation",
            "label and hostname are required",
        ));
    }
    let hosts = host_jump_map(&state)?;
    jump_chain(id, host.jump_host_id, MAX_JUMPS, |h| hosts.get(&h).copied())
        .map_err(|e| ApiError::new("validation", e.to_string()))?;

    let mut error = None;
    let outcome =
        state.session.with_vault(|v| {
            match crate::hostcreds::save_host_with_credentials(
                v,
                id,
                base_for(id, base_rev),
                host,
                credentials,
            ) {
                Ok(o) => Ok(Some(o)),
                Err(crate::hostcreds::HostCredError::Vault(e)) => Err(e),
                Err(crate::hostcreds::HostCredError::Validation(msg)) => {
                    error = Some(msg);
                    Ok(None)
                }
            }
        })?;
    outcome.ok_or_else(|| ApiError::new("validation", error.unwrap_or_default()))
}

/// host id -> that host's jump host id, for chain validation.
/// Each host's jump host, after group defaults.
fn host_jump_map(state: &AppState) -> ApiResult<std::collections::HashMap<Uuid, Option<Uuid>>> {
    with_keys(state, crate::keymanager::jump_map)
}

#[tauri::command]
pub fn delete_host(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    // Refuse before touching anything if another device changed it meanwhile.
    check_unchanged(&state, Collection::Hosts, id, base_rev)?;
    // Credentials entered in this host's own form go with it (and come back
    // with it on undo).
    let along = state
        .session
        .with_vault(|v| match v.get::<Host>(Collection::Hosts, id) {
            Ok(Record { data: Some(h), .. }) => {
                let owned = h
                    .identity_id
                    .and_then(|iid| v.get::<serde_json::Value>(Collection::Identities, iid).ok().map(|r| (iid, r.data)))
                    .and_then(|(iid, d)| d.map(|d| (iid, d)))
                    .filter(|(_, d)| d.get("for_host").and_then(|f| f.as_str()) == Some(&id.to_string()))
                    .map(|(iid, d)| (Collection::Identities, iid, d));
                crate::hostcreds::release_for_deleted_host(v, &h, id)?;
                Ok(owned.into_iter().collect::<Vec<_>>())
            }
            _ => Ok(Vec::new()),
        })?;
    // Hosts that tunnelled through this one fall back to direct connections.
    let hosts: Vec<Record<Host>> = list_records(&state, Collection::Hosts)?;
    for h in hosts {
        if let Some(mut data) = h.data {
            if data.jump_host_id == Some(id) {
                data.jump_host_id = None;
                save_record(&state, Collection::Hosts, Some(h.id), Some(h.rev), data)?;
            }
        }
    }
    delete_record_with(&state, Collection::Hosts, id, None, along, Vec::new())
}

/// Fail with a conflict if `id` moved past `base_rev` since the UI loaded it.
fn check_unchanged(
    state: &AppState,
    c: Collection,
    id: Uuid,
    base_rev: Option<u64>,
) -> ApiResult<()> {
    let Some(b) = base_rev else { return Ok(()) };
    let cur = state
        .session
        .with_vault(|v| v.get_raw::<serde_json::Value>(c, id))?;
    if cur.rev != b {
        return Err(VaultError::Conflict(Box::new(crate::vault::ConflictError {
            collection: c,
            id,
            reason: if cur.deleted {
                crate::vault::ConflictReason::DeletedElsewhere
            } else {
                crate::vault::ConflictReason::ChangedElsewhere
            },
            fields: vec![],
            their_device: cur.device_id,
            their_rev: cur.rev,
            their_updated_at: cur.updated_at,
        }))
        .into());
    }
    Ok(())
}

#[tauri::command]
pub fn list_identities(state: State<'_, AppState>) -> ApiResult<Vec<Record<Identity>>> {
    let mut list: Vec<Record<Identity>> = list_records(&state, Collection::Identities)?;
    for r in &mut list {
        r.data = r.data.as_ref().map(Identity::redacted);
    }
    Ok(list)
}

#[derive(Debug, Clone, Serialize)]
pub struct RevealResponse {
    pub secret: crate::reveal::Revealed,
    /// Seconds left before the master password is asked again.
    pub grace_secs: u64,
}

/// Show an identity's password, key or passphrase. Requires the master
/// password unless it was entered within the last couple of minutes. See
/// `reveal.rs` for the grace window and lockout rules.
#[tauri::command]
pub async fn reveal_identity(
    state: State<'_, AppState>,
    id: Uuid,
    master_password: Option<String>,
) -> ApiResult<RevealResponse> {
    use crate::reveal::GateError;
    use std::time::Instant;
    let master_password = master_password.map(Zeroizing::new);

    let gate_err = |e: GateError| match e {
        GateError::ReauthRequired => ApiError::new(
            "reauth_required",
            "Enter your master password to reveal this",
        ),
        GateError::WrongPassword { attempts_left } => ApiError::new(
            "wrong_password",
            format!("Wrong master password. {attempts_left} attempt(s) left before a lockout."),
        ),
        GateError::LockedOut { retry_in } => ApiError::new(
            "locked_out",
            format!(
                "Too many wrong attempts. Try again in {} seconds.",
                retry_in.as_secs().max(1)
            ),
        ),
    };

    let ok = match &master_password {
        None => None,
        Some(pw) => {
            if let Some(wait) = state
                .reveal
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .locked_out(Instant::now())
            {
                return Err(gate_err(GateError::LockedOut { retry_in: wait }));
            }
            // Argon2id is slow on purpose: derive off the async executor and
            // without holding the vault lock.
            let (vault_id, slot) = state
                .session
                .with_vault(|v| Ok((v.vault_id(), v.password_slot())))?;
            let pw = pw.clone();
            let key = tauri::async_runtime::spawn_blocking(move || {
                crate::vault::format::open_slot(vault_id, &slot, pw.as_bytes())
            })
            .await
            .map_err(|e| ApiError::new("reveal", e.to_string()))??;
            Some(match key {
                Some(k) => state.session.with_vault(|v| Ok(v.master_key().ct_eq(&k)))?,
                None => false,
            })
        }
    };

    {
        let mut gate = state.reveal.lock().unwrap_or_else(|p| p.into_inner());
        let pw = master_password.as_ref().map(|p| p.as_bytes());
        gate.check(Instant::now(), pw, |_| ok.unwrap_or(false))
            .map_err(gate_err)?;
    }

    let identity = state
        .session
        .with_vault(|v| v.get::<Identity>(Collection::Identities, id))?
        .data
        .ok_or_else(|| ApiError::new("not_found", "identity has no data"))?;
    let grace_secs = state
        .reveal
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remaining(Instant::now())
        .as_secs();
    Ok(RevealResponse {
        secret: crate::reveal::Revealed::from(&identity.auth),
        grace_secs,
    })
}

/// End the reveal grace window early ("Hide" in the UI).
#[tauri::command]
pub fn reveal_close(state: State<'_, AppState>) {
    state
        .reveal
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .close();
}

/// Public half of a key-based identity, for `authorized_keys`.
#[tauri::command]
pub fn identity_public_key(
    state: State<'_, AppState>,
    id: Uuid,
) -> ApiResult<crate::keys::PublicKeyInfo> {
    let identity = state
        .session
        .with_vault(|v| v.get::<Identity>(Collection::Identities, id))?
        .data
        .ok_or_else(|| ApiError::new("not_found", "identity has no data"))?;
    match identity.auth {
        AuthMethod::PrivateKey {
            private_key,
            passphrase,
            ..
        } => {
            let pem = Zeroizing::new(private_key);
            crate::keys::public_key_of(&pem, passphrase.as_deref())
                .map_err(|e| ApiError::new("key", e.to_string()))
        }
        _ => Err(ApiError::new(
            "validation",
            "only private-key identities have a public key",
        )),
    }
}

#[tauri::command]
pub fn generate_key(comment: String) -> ApiResult<crate::keys::GeneratedKey> {
    crate::keys::generate_ed25519(&comment).map_err(|e| ApiError::new("key", e.to_string()))
}

// ---------------------------------------------------------------------------
// SSH terminal sessions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SshStatusEvent {
    pub pane_id: String,
    pub status: SessionStatus,
}

/// One-time credentials for hosts that have no identity attached.
#[derive(Debug, Clone, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Build a login [`Target`] for a host, including its jump chain. One-time
/// credentials apply to the final host only; every jump host must have an
/// identity. Secrets never round-trip through the webview.
fn resolve_target(
    state: &AppState,
    host_id: Uuid,
    credentials: Option<Credentials>,
) -> ApiResult<Target> {
    use crate::keymanager::ResolveError;
    let creds = credentials.map(|c| (c.username, c.password));
    let policy = state.host_keys.clone();
    let resolved = state
        .session
        .with_vault(|v| Ok(crate::keymanager::resolve_target(v, host_id, creds, policy)))?;
    // With the vault agent on, forwarded agents offer vault keys (with the
    // same per-key approval) instead of the system agent.
    let agent_on = state.agent.lock().unwrap_or_else(|p| p.into_inner()).is_some();
    let resolved = resolved.map(|mut t| {
        if agent_on {
            t.agent_backend = Some(Arc::clone(&state.vault_agent));
        }
        t
    });
    resolved.map_err(|e| match e {
        ResolveError::Keys(k) => k.into(),
        ResolveError::CredentialsRequired(m) => ApiError::new("credentials_required", m),
        ResolveError::JumpChain(m) => ApiError::new("jump_chain", m),
        ResolveError::NotFound(m) => ApiError::new("not_found", m),
    })
}

/// Bridges the engine to the webview: raw PTY bytes over an IPC channel,
/// lifecycle notices over a Tauri event.
struct PaneSink {
    app: AppHandle,
    pane_id: String,
    data: Channel<InvokeResponseBody>,
    log: LogSlot,
}

impl TermSink for PaneSink {
    fn data(&self, bytes: &[u8]) {
        if let Some(log) = self.log.lock().unwrap_or_else(|p| p.into_inner()).as_mut() {
            // A failing log (disk full, file removed) must not break the session.
            let _ = log.write(bytes);
        }
        let _ = self.data.send(InvokeResponseBody::Raw(bytes.to_vec()));
    }
    fn status(&self, status: SessionStatus) {
        let _ = self.app.emit(
            EVENT_SSH_STATUS,
            SshStatusEvent {
                pane_id: self.pane_id.clone(),
                status,
            },
        );
    }
}

// IPC commands take their arguments flat from the frontend call.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ssh_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    host_id: Uuid,
    cols: u32,
    rows: u32,
    credentials: Option<Credentials>,
    on_data: Channel<InvokeResponseBody>,
) -> ApiResult<()> {
    let params = ConnectParams {
        target: resolve_target(&state, host_id, credentials)?,
        cols: cols.max(2),
        rows: rows.max(1),
    };
    let sink = Arc::new(PaneSink {
        app,
        pane_id: pane_id.clone(),
        data: on_data,
        log: state.log_slot(&pane_id),
    });
    state.ssh.connect(pane_id, params, sink)?;
    Ok(())
}

#[tauri::command]
pub async fn ssh_write(
    state: State<'_, AppState>,
    pane_id: String,
    data: Vec<u8>,
) -> ApiResult<()> {
    Ok(state.ssh.write(&pane_id, data).await?)
}

/// Smart completion: a folder listing or a fixed, read-only lookup over an extra channel of this pane's
/// connection. The frontend calls it only while "look up remote paths" is on for the pane.
#[tauri::command]
pub async fn completion_lookup(
    state: State<'_, AppState>,
    pane_id: String,
    request: crate::completion::Request,
) -> ApiResult<crate::completion::Reply> {
    use crate::completion::LookupError;
    state.ssh.lookup(&pane_id, &request).await.map_err(|e| {
        let code = match &e {
            LookupError::Refused => "completion_refused",
            LookupError::Timeout => "completion_timeout",
            LookupError::Invalid(_) => "invalid",
        };
        ApiError::new(code, e.to_string())
    })
}

/// Smart completion in a tab running a shell on this computer: a folder listing, nothing else.
#[tauri::command]
pub async fn completion_lookup_local(request: crate::completion::Request) -> ApiResult<crate::completion::Reply> {
    use crate::completion::{LookupError, Request};
    match request {
        Request::Dir { dir, prefix, limit } => tokio::task::spawn_blocking(move || crate::completion::list_local(&dir, &prefix, limit))
            .await
            .map_err(|e| ApiError::new("invalid", e.to_string()))?
            .map_err(|e: LookupError| ApiError::new("invalid", e.to_string())),
        Request::Generator { .. } => Err(ApiError::new("invalid", "only folder listings are available in a local tab")),
    }
}

#[tauri::command]
pub async fn ssh_resize(
    state: State<'_, AppState>,
    pane_id: String,
    cols: u32,
    rows: u32,
) -> ApiResult<()> {
    Ok(state.ssh.resize(&pane_id, cols.max(2), rows.max(1)).await?)
}

#[tauri::command]
pub async fn ssh_disconnect(state: State<'_, AppState>, pane_id: String) -> ApiResult<()> {
    state.ssh.disconnect(&pane_id).await;
    state
        .logs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&pane_id);
    Ok(())
}

// ---------------------------------------------------------------------------
// Reachability, Ansible import, ssh config export
// ---------------------------------------------------------------------------

/// Probe the SSH port of the given hosts (all hosts if `None`). Hosts behind
/// a jump host are reported as such rather than probed.
#[tauri::command]
pub async fn check_hosts(
    state: State<'_, AppState>,
    host_ids: Option<Vec<Uuid>>,
) -> ApiResult<Vec<crate::health::HealthResult>> {
    let targets = with_keys(&state, |v| crate::keymanager::probe_targets(v, host_ids.as_deref()))?;
    Ok(crate::health::probe_all(targets).await)
}

/// Try a proxy from its form, saved or not: reach `target` (default
/// github.com:22) through it and read the SSH greeting. An empty password
/// uses the saved one, as saving would.
#[tauri::command]
pub async fn test_proxy(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    spec: crate::models::ProxySpec,
    target: Option<String>,
) -> ApiResult<crate::health::Health> {
    use crate::models::ProxySpec;
    let mut spec = spec;
    if let ProxySpec::Socks5 { password, .. } | ProxySpec::Http { password, .. } = &mut spec {
        if password.as_deref() == Some("") {
            *password = match id {
                Some(id) => state
                    .session
                    .with_vault(|v| Ok(v.get::<crate::models::Proxy>(Collection::Proxies, id).ok().and_then(|r| r.data)))?
                    .and_then(|p| match p.spec {
                        ProxySpec::Socks5 { password, .. } | ProxySpec::Http { password, .. } => password,
                        ProxySpec::Command { .. } => None,
                    }),
                None => None,
            };
        }
    }
    match &spec {
        ProxySpec::Socks5 { host, .. } | ProxySpec::Http { host, .. } if host.trim().is_empty() => {
            return Err(ApiError::new("validation", "enter the proxy's address"));
        }
        ProxySpec::Command { command, .. } if command.trim().is_empty() => {
            return Err(ApiError::new("validation", "enter the command"));
        }
        _ => {}
    }
    let (host, port) = crate::health::parse_target(target.as_deref().unwrap_or("").trim())
        .ok_or_else(|| ApiError::new("validation", "test destination must look like host or host:port"))?;
    // Testing a command from the form with "approved" ticked runs it for this test, in memory only: nothing is
    // saved, and a command that was not approved before is not approved afterwards.
    let mut undo_approval = None;
    if let ProxySpec::Command { command, approved: true } = &spec {
        if !crate::proxyapproval::is_approved(command) {
            crate::proxyapproval::set(command, true);
            undo_approval = Some(command.clone());
        }
    }
    let health = crate::health::probe(&host, port, Some(&spec)).await;
    if let Some(command) = undo_approval {
        crate::proxyapproval::set(&command, false);
    }
    Ok(health)
}

/// Read an Ansible INI inventory for the import preview.
#[tauri::command]
pub fn ansible_preview(state: State<'_, AppState>, path: String) -> ApiResult<SshConfigPreview> {
    let text =
        std::fs::read_to_string(&path).map_err(|e| ApiError::new("io", format!("{path}: {e}")))?;
    let parsed = crate::ansible::parse(&text, &sftp::local::home());
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
        path,
        hosts: parsed.hosts,
        warnings: parsed.warnings,
        existing,
    })
}

/// Render all hosts as an OpenSSH config. Writes it to `path` if given and
/// returns the text either way.
#[tauri::command]
pub fn export_ssh_config(state: State<'_, AppState>, path: Option<String>) -> ApiResult<String> {
    let hosts: Vec<(Uuid, Host)> = list_records::<Host>(&state, Collection::Hosts)?
        .into_iter()
        .filter_map(|r| r.data.map(|d| (r.id, d)))
        .collect();
    let identities: std::collections::HashMap<Uuid, Identity> =
        list_records::<Identity>(&state, Collection::Identities)?
            .into_iter()
            .filter_map(|r| r.data.map(|d| (r.id, d.redacted())))
            .collect();
    let text = crate::sshconfig::export(&hosts, &identities);
    if let Some(path) = path {
        std::fs::write(&path, &text).map_err(|e| ApiError::new("io", format!("{path}: {e}")))?;
    }
    Ok(text)
}

/// Start recording a pane's output to `path` (appending). `plain` strips
/// colours and control sequences; otherwise the raw stream is kept.
#[tauri::command]
pub fn ssh_log_start(
    state: State<'_, AppState>,
    pane_id: String,
    path: String,
    plain: bool,
    header: String,
) -> ApiResult<()> {
    let slot = state
        .logs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&pane_id)
        .cloned()
        .ok_or_else(|| ApiError::new("not_connected", "this pane has no session"))?;
    let log = crate::sessionlog::SessionLog::open(std::path::Path::new(&path), plain, &header)
        .map_err(|e| ApiError::new("io", format!("{path}: {e}")))?;
    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(log);
    Ok(())
}

#[tauri::command]
pub fn ssh_log_stop(state: State<'_, AppState>, pane_id: String) {
    if let Some(slot) = state
        .logs
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .get(&pane_id)
    {
        slot.lock().unwrap_or_else(|p| p.into_inner()).take();
    }
}

// ---------------------------------------------------------------------------
// Groups, proxies, workspaces, shared settings
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> ApiResult<Vec<Record<crate::models::HostGroup>>> {
    list_records(&state, Collection::Groups)
}

/// Save a group's defaults. One record per path.
#[tauri::command]
pub fn save_group(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    group: crate::models::HostGroup,
) -> ApiResult<Record<crate::models::HostGroup>> {
    let mut group = group;
    group.path = group
        .path
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if group.path.is_empty() {
        return Err(ApiError::new("validation", "a group needs a name"));
    }
    let clash = list_records::<crate::models::HostGroup>(&state, Collection::Groups)?
        .into_iter()
        .any(|r| Some(r.id) != id && r.data.is_some_and(|g| g.path == group.path));
    if clash {
        return Err(ApiError::new("validation", format!("group \"{}\" already has settings", group.path)));
    }
    save_record(&state, Collection::Groups, id, base_rev, group)
}

#[tauri::command]
pub fn delete_group(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    delete_record(&state, Collection::Groups, id, base_rev)
}

#[tauri::command]
pub fn list_proxies(state: State<'_, AppState>) -> ApiResult<Vec<Record<crate::models::Proxy>>> {
    let mut list: Vec<Record<crate::models::Proxy>> = list_records(&state, Collection::Proxies)?;
    for r in &mut list {
        r.data = r.data.as_ref().map(crate::models::Proxy::redacted);
        // Whether a command is approved is this computer's own answer, whatever the synced record says.
        if let Some(crate::models::Proxy { spec: crate::models::ProxySpec::Command { command, approved }, .. }) = r.data.as_mut() {
            *approved = crate::proxyapproval::is_approved(command);
        }
    }
    Ok(list)
}

/// Save a proxy. An empty password keeps the stored one. A ProxyCommand
/// runs a program on this computer, so it's only approved through this
/// explicit save, never by an import.
#[tauri::command]
pub fn save_proxy(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    base_rev: Option<u64>,
    proxy: crate::models::Proxy,
) -> ApiResult<Record<crate::models::Proxy>> {
    use crate::models::ProxySpec;
    let mut proxy = proxy;
    if proxy.name.trim().is_empty() {
        return Err(ApiError::new("validation", "a proxy needs a name"));
    }
    let stored = match id {
        Some(id) => state
            .session
            .with_vault(|v| match v.get::<crate::models::Proxy>(Collection::Proxies, id) {
                Ok(r) => Ok(r.data),
                Err(VaultError::NotFound { .. } | VaultError::Deleted { .. }) => Ok(None),
                Err(e) => Err(e),
            })?,
        None => None,
    };
    match (&mut proxy.spec, stored.map(|p| p.spec)) {
        (
            ProxySpec::Socks5 { host, password, .. } | ProxySpec::Http { host, password, .. },
            old,
        ) => {
            if host.trim().is_empty() {
                return Err(ApiError::new("validation", "enter the proxy's address"));
            }
            if password.as_deref() == Some("") {
                *password = match old {
                    Some(ProxySpec::Socks5 { password: p, .. } | ProxySpec::Http { password: p, .. }) => p,
                    _ => None,
                };
            }
        }
        (ProxySpec::Command { command, .. }, _) => {
            if command.trim().is_empty() {
                return Err(ApiError::new("validation", "enter the command"));
            }
        }
    }
    // Approving a command is something this computer does, so it is recorded here, and the synced record never says
    // "approved": a copy of it on another device starts out unapproved.
    let mut approval = None;
    if let ProxySpec::Command { command, approved } = &mut proxy.spec {
        approval = Some((command.clone(), *approved));
        *approved = false;
    }
    let mut rec = save_record(&state, Collection::Proxies, id, base_rev, proxy)?;
    if let Some((command, allow)) = approval {
        let all = crate::proxyapproval::set(&command, allow);
        let mut cfg = AppConfig::load(&state.config_dir)?;
        cfg.approved_commands = all;
        cfg.save(&state.config_dir)?;
    }
    rec.data = rec.data.as_ref().map(crate::models::Proxy::redacted);
    if let Some(crate::models::Proxy { spec: ProxySpec::Command { command, approved }, .. }) = rec.data.as_mut() {
        *approved = crate::proxyapproval::is_approved(command);
    }
    Ok(rec)
}

#[tauri::command]
pub fn delete_proxy(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    delete_record(&state, Collection::Proxies, id, base_rev)
}

// ---------------------------------------------------------------------------
// SFTP (remote) and local file system
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SftpOpened {
    pub home: String,
    /// Host keys (target and jumps) seen for the first time.
    pub new_host_keys: Vec<LearnedKey>,
}

/// What the window learns when an FTP pane is open.
#[derive(Debug, Clone, Serialize)]
pub struct FtpOpened {
    pub home: String,
    /// The certificate was pinned by this connection (first use, or a change the person accepted).
    pub pinned_now: bool,
}

/// Open a file pane over FTP or FTPS. An unknown or changed certificate comes back as an error
/// (`ftp_certificate_unknown`, `ftp_certificate_changed`, with the details) before any password
/// is sent; asking again with `accept` set to its fingerprint goes ahead and pins it to the host.
#[tauri::command]
pub async fn ftp_open(
    state: State<'_, AppState>,
    session_id: String,
    host_id: Uuid,
    credentials: Option<Credentials>,
    accept: Option<String>,
) -> ApiResult<FtpOpened> {
    let host: Host = state
        .session
        .with_vault(|v| Ok(v.get::<Host>(Collection::Hosts, host_id)?.data))?
        .ok_or_else(|| ApiError::new("not_found", "no such host"))?;
    let options = host.ftp.clone().unwrap_or_default();
    let (user, password) = if options.anonymous {
        ("anonymous".to_string(), "anonymous@".to_string())
    } else {
        // The sign-in is a user name and password; keys, jump hosts and proxies are SSH's.
        let target = resolve_target(&state, host_id, credentials)?;
        let AuthMethod::Password { password } = target.auth.clone() else {
            return Err(ApiError::new("validation", "FTP signs in with a password; attach a credential that has one, or tick anonymous"));
        };
        if target.jump.is_some() || target.proxy.is_some() {
            return Err(ApiError::new("validation", "FTP connects directly; it doesn't use jump hosts or proxies"));
        }
        (target.username, password)
    };
    let pinned = Some(options.cert_sha256.clone()).filter(|p| !p.is_empty());
    let settings = crate::files::ftp::FtpSettings {
        host: host.hostname.clone(),
        port: host.port,
        user,
        password,
        tls: options.tls,
        cert: crate::certs::CertCheck { pinned: pinned.clone(), accept },
    };
    let (conn, seen) = crate::files::ftp::FtpConn::open(settings).await?;
    let home = crate::files::FileBackend::home(&conn).await?;
    state.files.insert(session_id, Arc::new(conn)).await;
    // Only reached when the certificate was trusted: pinned already, or just accepted.
    let pinned_now = seen.as_ref().is_some_and(|c| pinned.as_deref() != Some(c.fingerprint.as_str()));
    if let (true, Some(c)) = (pinned_now, seen) {
        state.session.with_vault(|v| crate::keymanager::pin_ftp_certificate(v, host_id, &c.fingerprint))?;
    }
    Ok(FtpOpened { home, pinned_now })
}

/// Open a file pane over SCP, for a server with no SFTP. Same answer as [`sftp_open`].
#[tauri::command]
pub async fn scp_open(
    state: State<'_, AppState>,
    session_id: String,
    host_id: Uuid,
    credentials: Option<Credentials>,
) -> ApiResult<SftpOpened> {
    let target = resolve_target(&state, host_id, credentials)?;
    let (conn, new_host_keys) = crate::files::scp::ScpConn::open(&target).await?;
    let home = crate::files::FileBackend::home(&conn).await?;
    state.files.insert(session_id, Arc::new(conn)).await;
    Ok(SftpOpened { home, new_host_keys })
}

#[tauri::command]
pub async fn sftp_open(
    state: State<'_, AppState>,
    session_id: String,
    host_id: Uuid,
    credentials: Option<Credentials>,
) -> ApiResult<SftpOpened> {
    let target = resolve_target(&state, host_id, credentials)?;
    let (home, new_host_keys) = state.sftp.open(session_id, &target).await?;
    Ok(SftpOpened {
        home,
        new_host_keys,
    })
}

// -- any file pane, by the id its window gave it ("local" is this computer) ------------------

fn supported(allowed: bool, what: &'static str) -> ApiResult<()> {
    if allowed {
        Ok(())
    } else {
        Err(ApiError::new("unsupported", format!("this place doesn't support {what}")))
    }
}

#[tauri::command]
pub fn files_caps(state: State<'_, AppState>, session_id: String) -> ApiResult<crate::files::Caps> {
    Ok(state.files.get(&session_id)?.caps())
}

#[tauri::command]
pub async fn files_home(state: State<'_, AppState>, session_id: String) -> ApiResult<String> {
    Ok(state.files.get(&session_id)?.home().await?)
}

#[tauri::command]
pub async fn files_list(state: State<'_, AppState>, session_id: String, path: String) -> ApiResult<Vec<FileEntry>> {
    Ok(state.files.get(&session_id)?.list(&path).await?)
}

#[tauri::command]
pub async fn files_mkdir(state: State<'_, AppState>, session_id: String, dir: String, name: String) -> ApiResult<()> {
    let b = state.files.get(&session_id)?;
    supported(b.caps().mkdir, "making folders")?;
    Ok(b.mkdir(&dir, &name).await?)
}

#[tauri::command]
pub async fn files_rename(state: State<'_, AppState>, session_id: String, path: String, new_name: String) -> ApiResult<()> {
    let b = state.files.get(&session_id)?;
    supported(b.caps().rename, "renaming")?;
    Ok(b.rename(&path, &new_name).await?)
}

#[tauri::command]
pub async fn files_remove(state: State<'_, AppState>, session_id: String, paths: Vec<String>) -> ApiResult<()> {
    let b = state.files.get(&session_id)?;
    supported(b.caps().delete, "deleting")?;
    for p in paths {
        b.remove(&p).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn files_chmod(state: State<'_, AppState>, session_id: String, path: String, mode: u32) -> ApiResult<()> {
    let b = state.files.get(&session_id)?;
    supported(b.caps().chmod, "changing permissions")?;
    Ok(b.chmod(&path, mode).await?)
}

#[tauri::command]
pub async fn files_preview(state: State<'_, AppState>, session_id: String, path: String) -> ApiResult<Preview> {
    let b = state.files.get(&session_id)?;
    supported(b.caps().preview, "previews")?;
    let (bytes, truncated) = b.read_head(&path, PREVIEW_MAX).await?;
    Ok(preview_of(bytes, truncated))
}

#[tauri::command]
pub async fn files_close(state: State<'_, AppState>, session_id: String) -> ApiResult<()> {
    state.files.close(&session_id).await;
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct EditEventPayload {
    pub edit_id: String,
    #[serde(flatten)]
    pub event: crate::remoteedit::EditEvent,
}

struct EventEditSink(AppHandle);
impl crate::remoteedit::EditSink for EventEditSink {
    fn event(&self, edit_id: &str, event: crate::remoteedit::EditEvent) {
        let _ = self.0.emit(
            EVENT_SFTP_EDIT,
            EditEventPayload {
                edit_id: edit_id.to_string(),
                event,
            },
        );
    }
}

/// Download a remote file, open it in this computer's default editor, and
/// upload it again on every save until [`sftp_edit_stop`].
#[tauri::command]
pub async fn sftp_edit_start(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    remote_path: String,
) -> ApiResult<crate::remoteedit::EditStarted> {
    use tauri_plugin_opener::OpenerExt;
    let conn = state.sftp.get(&session_id)?;
    let started = state
        .edits
        .start(
            conn,
            remote_path,
            &state.edit_dir,
            Arc::new(EventEditSink(app.clone())),
        )
        .await?;
    if let Err(e) = app
        .opener()
        .open_path(started.local_path.clone(), None::<String>)
    {
        state.edits.stop(&started.edit_id);
        return Err(ApiError::new(
            "open",
            format!("could not open an editor: {e}"),
        ));
    }
    Ok(started)
}

#[tauri::command]
pub fn sftp_edit_stop(state: State<'_, AppState>, edit_id: String) {
    state.edits.stop(&edit_id);
}


/// A file's beginning, for a quick look: text when it decodes as UTF-8,
/// otherwise base64 (images). Capped so huge files stay cheap.
#[derive(Debug, Clone, Serialize)]
pub struct Preview {
    pub kind: &'static str,
    pub text: Option<String>,
    pub base64: Option<String>,
    pub truncated: bool,
}

const PREVIEW_MAX: usize = 512 * 1024;

fn preview_of(bytes: Vec<u8>, truncated: bool) -> Preview {
    use base64::Engine;
    match String::from_utf8(bytes) {
        Ok(text) if !text.contains('\0') => Preview {
            kind: "text",
            text: Some(text),
            base64: None,
            truncated,
        },
        Ok(text) => Preview {
            kind: "binary",
            text: None,
            base64: Some(base64::engine::general_purpose::STANDARD.encode(text.as_bytes())),
            truncated,
        },
        Err(e) => Preview {
            kind: "binary",
            text: None,
            base64: Some(base64::engine::general_purpose::STANDARD.encode(e.into_bytes())),
            truncated,
        },
    }
}

/// A small local text file the user picked (a theme, a CSV), at most 1 MiB.
#[tauri::command]
pub fn read_text_file(path: String) -> ApiResult<String> {
    let p = PathBuf::from(crate::sshconfig::expand_path(&path, &sftp::local::home()));
    let bytes = crate::vault::read_regular_file(&p, 1024 * 1024)
        .map_err(|e| ApiError::new("io", format!("{}: {e}", p.display())))?;
    String::from_utf8(bytes).map_err(|_| ApiError::new("io", "the file is not UTF-8 text"))
}

/// Write a text file the person chose in the Save dialog, for exports such as a snippet pack or a log. Only `.json`
/// or `.log`, and at most 2 MB, so it can't be used to put anything else anywhere; written beside the target and renamed into
/// place, so a failed write never leaves half a file.
#[tauri::command]
pub fn export_text_file(path: String, contents: String) -> ApiResult<()> {
    crate::exportfile::write_json(&PathBuf::from(&path), &contents).map_err(|e| ApiError::new("io", e))
}

#[tauri::command]
pub fn csv_preview(path: String) -> ApiResult<crate::csvimport::CsvTable> {
    Ok(crate::csvimport::parse(&read_text_file(path)?))
}

/// SSH bookmarks from a MobaXterm `.ini` or `.mxtsessions` file.
#[tauri::command]
pub fn mobaxterm_preview(state: State<'_, AppState>, path: String) -> ApiResult<SshConfigPreview> {
    let text = read_text_file(path.clone())?;
    // `_ProfileDir_` is the Windows user folder.
    let profile = std::env::var("USERPROFILE").ok();
    let parsed = crate::mobaxterm::parse(&text, profile.as_deref());
    let labels: std::collections::HashSet<String> = list_records::<Host>(&state, Collection::Hosts)?
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
        path,
        hosts: parsed.hosts,
        warnings: parsed.warnings,
        existing,
    })
}

/// Saved PuTTY sessions on this computer, as importable hosts.
#[tauri::command]
pub fn putty_sessions(state: State<'_, AppState>) -> ApiResult<SshConfigPreview> {
    let hosts = crate::putty::saved_sessions(&sftp::local::home());
    let labels: std::collections::HashSet<String> = list_records::<Host>(&state, Collection::Hosts)?
        .into_iter()
        .filter_map(|r| r.data.map(|d| d.label.to_lowercase()))
        .collect();
    let existing = hosts.iter().filter(|h| labels.contains(&h.alias.to_lowercase())).map(|h| h.alias.clone()).collect();
    Ok(SshConfigPreview {
        path: if cfg!(windows) { "PuTTY sessions (registry)".into() } else { "~/.putty/sessions".into() },
        hosts,
        warnings: Vec::new(),
        existing,
    })
}


#[tauri::command]
pub fn local_home() -> String {
    sftp::local::home().to_string_lossy().into_owned()
}

struct ChannelProgress(Channel<TransferProgress>);
impl ProgressSink for ChannelProgress {
    fn report(&self, p: TransferProgress) {
        let _ = self.0.send(p);
    }
}

/// Start a transfer in the background between any two open panes (`local` is
/// this computer's disk). Returns immediately; progress and the final state
/// stream over `on_progress`. With `remove_source` the originals are deleted
/// once everything has been copied (a move).
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn files_transfer_start(
    state: State<'_, AppState>,
    source_id: String,
    dest_id: String,
    transfer_id: String,
    sources: Vec<String>,
    dest_dir: String,
    resume: Option<bool>,
    conflict: Option<crate::files::Conflict>,
    remove_source: Option<bool>,
    on_progress: Channel<TransferProgress>,
) -> ApiResult<()> {
    let files = Arc::clone(&state.files);
    tauri::async_runtime::spawn(async move {
        let sink = ChannelProgress(on_progress);
        let conflict = conflict.unwrap_or_default();
        if remove_source.unwrap_or(false) {
            files.move_items(&source_id, &dest_id, transfer_id, sources, dest_dir, conflict, &sink).await;
        } else {
            files.transfer(&source_id, &dest_id, transfer_id, sources, dest_dir, resume.unwrap_or(false), conflict, &sink).await;
        }
    });
    Ok(())
}

#[tauri::command]
pub fn transfer_cancel(state: State<'_, AppState>, transfer_id: String) {
    state.files.registry().cancel(&transfer_id);
}

#[tauri::command]
pub fn transfer_pause(state: State<'_, AppState>, transfer_id: String) {
    state.files.registry().pause(&transfer_id);
}

#[tauri::command]
pub fn transfer_resume(state: State<'_, AppState>, transfer_id: String) {
    state.files.registry().resume(&transfer_id);
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Resolve the per-machine config dir and register [`AppState`].
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = app.path().app_config_dir()?;
    // Before anything else can fail: a release build aborts on a panic, so record why in `crash.log` first.
    crate::crashlog::install(config_dir.clone());
    // The ProxyCommands this computer has approved to run (kept here, not in the synced vault).
    crate::proxyapproval::load(AppConfig::load(&config_dir).map(|c| c.approved_commands).unwrap_or_default());
    // Leftovers from a run that didn't exit cleanly are plaintext copies of
    // remote files; remove them before anything else.
    // Per-user cache folder (not the shared system temp directory, where
    // another local user could pre-create or swap the path).
    let edit_dir = app.path().app_cache_dir()?.join("remote-edit");
    crate::remoteedit::clean_base(&edit_dir);
    crate::remoteedit::clean_base(&std::env::temp_dir().join("sshvault-edit"));
    // Self-update only in builds that embed an update-signing public key
    // (CI adds it when the signing secrets exist). Without one the plugin
    // would fail to start, and there'd be nothing to verify updates with.
    if updater_configured(app.config()) {
        app.handle().plugin(tauri_plugin_updater::Builder::new().build())?;
    }
    let prompts: PromptMap = Default::default();
    let sftp = Arc::new(SftpManager::new());
    app.manage(AppState {
        session: {
            let s = Session::new();
            s.set_highwater_dir(config_dir.join("highwater"));
            s
        },
        config_dir: config_dir.clone(),
        ssh: Arc::new(SshManager::new()),
        sftp: Arc::clone(&sftp),
        files: Arc::new(crate::files::FileManager::new(sftp)),
        forwards: Arc::new(ForwardManager::new()),
        runs: Arc::new(crate::runner::RunManager::new()),
        local: Arc::new(crate::localpty::LocalManager::new()),
        rdp: Arc::new(crate::rdp::RdpManager::new()),
        vnc: Arc::new(crate::vnc::VncManager::new()),
        runbooks: Arc::new(crate::runbookrun::RunbookManager::new()),
        runbook_history: crate::runbookhistory::History::new(config_dir.join("runbook-history")),
        raw: Arc::new(crate::rawterm::RawManager::new()),
        dbs: Arc::new(crate::db::DbManager::new()),
        containers: Arc::new(crate::containers::ContainerManager::new()),
        kube: Arc::new(crate::kube::KubeManager::new()),
        monitors: Arc::new(crate::monitor::MonitorManager::new()),
        edits: Arc::new(crate::remoteedit::EditManager::new()),
        edit_dir,
        reveal: Default::default(),
        keystore: Arc::new(crate::keychain::OsKeyStore),
        host_keys: crate::ssh::HostKeyPolicy {
            store: Arc::new(VaultHostKeys(app.handle().clone())),
            prompter: Some(Arc::new(UiPrompter {
                app: app.handle().clone(),
                pending: Arc::clone(&prompts),
            })),
        },
        host_key_prompts: prompts,
        trash: Default::default(),
        agent: Default::default(),
        vault_agent: Arc::new(VaultAgent(app.handle().clone())),
        agent_prompts: Default::default(),
        control: Default::default(),
        cli_prompts: Default::default(),
        cli_trusted_until: Default::default(),
        logs: Default::default(),
        idle: Default::default(),
        unlock_guard: Default::default(),
    });
    // Lock an idle vault even when the window can't (frozen, throttled, or its timer lost). Checks every 15 seconds;
    // started with `tauri::async_runtime` because setup runs on a thread with no async runtime of its own.
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(15));
        loop {
            tick.tick().await;
            let state = handle.state::<AppState>();
            if state.session.is_unlocked() && state.idle.due(std::time::Instant::now()) && lock_everything(&state).await.is_ok() {
                let _ = handle.emit("vault:idle-locked", ());
            }
        }
    });
    // The tray icon that keeps the app running when its window is closed (none where the system has no tray).
    tray::install(app);
    Ok(())
}
