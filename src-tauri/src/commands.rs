//! Tauri IPC surface. Every command is a thin wrapper: parse arguments, call
//! into [`Session`] / [`Vault`], and map errors to a serializable [`ApiError`].
//!
//! Records cross the IPC boundary as `Record<T>` (`{ id, updated_at, deleted,
//! data }`) so the frontend can use `updated_at` for last-writer-wins when a
//! `vault:changed` event arrives from the file watcher.

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
    self, Direction, FileEntry, ProgressSink, SftpError, SftpManager, TransferProgress,
};
use crate::ssh::{
    ConnectParams, LearnedKey, SessionStatus, SshError, SshManager, Target, TermSink, MAX_JUMPS,
};
use crate::sync::RecordChange;
use crate::vault::{Collection, Record, VaultError};

/// Event name for record changes detected on disk. Payload: [`RecordChange`].
pub const EVENT_VAULT_CHANGED: &str = "vault:changed";

/// Event name for SSH session lifecycle notices. Payload: [`SshStatusEvent`].
pub const EVENT_SSH_STATUS: &str = "ssh:status";

/// Event name for port-forward status changes. Payload: [`ForwardStatusEvent`].
pub const EVENT_FORWARD_STATUS: &str = "forward:status";

pub struct AppState {
    pub session: Session,
    pub config_dir: PathBuf,
    pub ssh: Arc<SshManager>,
    pub sftp: Arc<SftpManager>,
    pub forwards: Arc<ForwardManager>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApiError {
    /// Stable machine-readable code the UI can branch on.
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
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
            _ => "vault",
        };
        Self::new(code, e.to_string())
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
            SshError::JumpUnreachable { .. } => "unreachable",
            SshError::Jump { .. } | SshError::JumpChainTooLong => "jump",
            SshError::AlreadyConnected => "already_connected",
            SshError::NotConnected => "not_connected",
            SshError::Timeout(_) | SshError::Connect { .. } => "unreachable",
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

impl From<ConfigError> for ApiError {
    fn from(e: ConfigError) -> Self {
        Self::new("config", e.to_string())
    }
}

type ApiResult<T> = Result<T, ApiError>;

fn on_change_emitter(app: AppHandle) -> impl Fn(RecordChange) + Send + 'static {
    move |mut change| {
        // Identities arriving from other computers are redacted like lists
        // are; secrets stay on the Rust side unless explicitly requested.
        if change.collection == Collection::Identities {
            if let Some(rec) = change.record.as_mut() {
                rec.data = rec
                    .data
                    .take()
                    .and_then(|v| serde_json::from_value::<Identity>(v).ok())
                    .and_then(|i| serde_json::to_value(i.redacted()).ok());
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

fn purge_old_tombstones(state: &AppState) {
    let _ = state.session.with_vault(|v| {
        for c in Collection::ALL {
            v.purge_tombstones(c, TOMBSTONE_MAX_AGE_MS)?;
        }
        Ok(())
    });
}

// ---------------------------------------------------------------------------
// Vault lifecycle
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> ApiResult<VaultStatus> {
    let cfg = AppConfig::load(&state.config_dir)?;
    Ok(state.session.status(cfg.vault_path.as_deref()))
}

#[tauri::command]
pub fn set_vault_path(state: State<'_, AppState>, path: String) -> ApiResult<VaultStatus> {
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(ApiError::new("invalid_path", "vault path must be absolute"));
    }
    state.session.lock();
    let cfg = AppConfig {
        vault_path: Some(path),
    };
    cfg.save(&state.config_dir)?;
    Ok(state.session.status(cfg.vault_path.as_deref()))
}

#[tauri::command]
pub fn create_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    password: String,
) -> ApiResult<VaultStatus> {
    let password = Zeroizing::new(password);
    if password.len() < 8 {
        return Err(ApiError::new(
            "weak_password",
            "master password must be at least 8 characters",
        ));
    }
    let cfg = AppConfig::load(&state.config_dir)?;
    let root = cfg
        .vault_path
        .clone()
        .ok_or_else(|| ApiError::new("not_configured", "choose a vault folder first"))?;
    state.session.create(
        root,
        password.as_bytes(),
        KdfParams::default(),
        on_change_emitter(app),
    )?;
    Ok(state.session.status(cfg.vault_path.as_deref()))
}

#[tauri::command]
pub fn unlock_vault(
    app: AppHandle,
    state: State<'_, AppState>,
    password: String,
) -> ApiResult<VaultStatus> {
    let password = Zeroizing::new(password);
    let cfg = AppConfig::load(&state.config_dir)?;
    let root = cfg
        .vault_path
        .clone()
        .ok_or_else(|| ApiError::new("not_configured", "choose a vault folder first"))?;
    state
        .session
        .unlock(root, password.as_bytes(), on_change_emitter(app))?;
    purge_old_tombstones(&state);
    Ok(state.session.status(cfg.vault_path.as_deref()))
}

#[tauri::command]
pub async fn lock_vault(state: State<'_, AppState>) -> ApiResult<VaultStatus> {
    state.ssh.disconnect_all().await;
    state.sftp.close_all().await;
    state.forwards.stop_all();
    state.session.lock();
    let cfg = AppConfig::load(&state.config_dir)?;
    Ok(state.session.status(cfg.vault_path.as_deref()))
}

#[tauri::command]
pub fn change_master_password(
    state: State<'_, AppState>,
    current_password: String,
    new_password: String,
) -> ApiResult<()> {
    let current = Zeroizing::new(current_password);
    let new = Zeroizing::new(new_password);
    if new.len() < 8 {
        return Err(ApiError::new(
            "weak_password",
            "master password must be at least 8 characters",
        ));
    }
    // Re-verify the current password before rotating anything.
    let root = state.session.with_vault(|v| Ok(v.root().to_path_buf()))?;
    crate::vault::Vault::open(&root, current.as_bytes())?;
    state
        .session
        .with_vault_mut(|v| v.change_password(new.as_bytes(), KdfParams::default()))?;
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

fn save_record<T>(
    state: &AppState,
    collection: Collection,
    id: Option<Uuid>,
    data: T,
) -> ApiResult<Record<T>>
where
    T: Serialize + Clone,
{
    let id = id.unwrap_or_else(Uuid::new_v4);
    Ok(state.session.with_vault(|v| v.put(collection, id, &data))?)
}

fn delete_record(state: &AppState, collection: Collection, id: Uuid) -> ApiResult<()> {
    Ok(state.session.with_vault(|v| v.delete(collection, id))?)
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
    save_record(&state, Collection::Hosts, id, host)
}

/// Save a host together with the credentials entered in its form. See
/// `hostcreds.rs` for the modes and how stored secrets are kept.
#[tauri::command]
pub fn save_host_with_credentials(
    state: State<'_, AppState>,
    id: Option<Uuid>,
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
            match crate::hostcreds::save_host_with_credentials(v, id, host, credentials) {
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
fn host_jump_map(state: &AppState) -> ApiResult<std::collections::HashMap<Uuid, Option<Uuid>>> {
    Ok(list_records::<Host>(state, Collection::Hosts)?
        .into_iter()
        .filter_map(|r| r.data.map(|d| (r.id, d.jump_host_id)))
        .collect())
}

#[tauri::command]
pub fn delete_host(state: State<'_, AppState>, id: Uuid) -> ApiResult<()> {
    // Credentials entered in this host's own form go with it.
    state
        .session
        .with_vault(|v| match v.get::<Host>(Collection::Hosts, id) {
            Ok(Record { data: Some(h), .. }) => {
                crate::hostcreds::release_for_deleted_host(v, &h, id)
            }
            _ => Ok(()),
        })?;
    // Hosts that tunnelled through this one fall back to direct connections.
    let hosts: Vec<Record<Host>> = list_records(&state, Collection::Hosts)?;
    for h in hosts {
        if let Some(mut data) = h.data {
            if data.jump_host_id == Some(id) {
                data.jump_host_id = None;
                save_record(&state, Collection::Hosts, Some(h.id), data)?;
            }
        }
    }
    delete_record(&state, Collection::Hosts, id)
}

#[tauri::command]
pub fn list_identities(state: State<'_, AppState>) -> ApiResult<Vec<Record<Identity>>> {
    let mut list: Vec<Record<Identity>> = list_records(&state, Collection::Identities)?;
    for r in &mut list {
        r.data = r.data.as_ref().map(Identity::redacted);
    }
    Ok(list)
}

/// Full identity including secrets, for the edit form only.
#[tauri::command]
pub fn get_identity(state: State<'_, AppState>, id: Uuid) -> ApiResult<Record<Identity>> {
    Ok(state
        .session
        .with_vault(|v| v.get::<Identity>(Collection::Identities, id))?)
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

#[tauri::command]
pub fn save_identity(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    identity: Identity,
) -> ApiResult<Record<Identity>> {
    if identity.label.trim().is_empty() || identity.username.trim().is_empty() {
        return Err(ApiError::new(
            "validation",
            "label and username are required",
        ));
    }
    // Return the redacted form; the webview only holds secrets while editing.
    let mut rec = save_record(&state, Collection::Identities, id, identity)?;
    rec.data = rec.data.as_ref().map(Identity::redacted);
    Ok(rec)
}

#[tauri::command]
pub fn delete_identity(state: State<'_, AppState>, id: Uuid) -> ApiResult<()> {
    // Detach the identity from any host that references it so the UI never
    // shows a dangling reference.
    let hosts: Vec<Record<Host>> = list_records(&state, Collection::Hosts)?;
    for h in hosts {
        if let Some(mut data) = h.data {
            if data.identity_id == Some(id) {
                data.identity_id = None;
                save_record(&state, Collection::Hosts, Some(h.id), data)?;
            }
        }
    }
    delete_record(&state, Collection::Identities, id)
}

#[tauri::command]
pub fn list_snippets(state: State<'_, AppState>) -> ApiResult<Vec<Record<Snippet>>> {
    list_records(&state, Collection::Snippets)
}

#[tauri::command]
pub fn save_snippet(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    snippet: Snippet,
) -> ApiResult<Record<Snippet>> {
    if snippet.label.trim().is_empty() || snippet.command.trim().is_empty() {
        return Err(ApiError::new(
            "validation",
            "label and command are required",
        ));
    }
    save_record(&state, Collection::Snippets, id, snippet)
}

#[tauri::command]
pub fn delete_snippet(state: State<'_, AppState>, id: Uuid) -> ApiResult<()> {
    delete_record(&state, Collection::Snippets, id)
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
    let host = load_host(state, host_id)?;
    let hosts = host_jump_map(state)?;
    let chain = jump_chain(Some(host_id), host.jump_host_id, MAX_JUMPS, |h| {
        hosts.get(&h).copied()
    })
    .map_err(|e| ApiError::new("jump_chain", e.to_string()))?;

    // Build from the outermost hop inwards so each Target owns its jump.
    let mut jump: Option<Box<Target>> = None;
    for jid in chain.into_iter().rev() {
        let jh = load_host(state, jid)?;
        let (username, auth) = identity_login(state, &jh).map_err(|e| {
            if e.code == "credentials_required" {
                ApiError::new(
                    "credentials_required",
                    format!("jump host \"{}\" needs an identity attached", jh.label),
                )
            } else {
                e
            }
        })?;
        jump = Some(Box::new(Target {
            hostname: jh.hostname,
            port: jh.port,
            username,
            auth,
            known_hosts: state.config_dir.join("known_hosts"),
            jump,
        }));
    }

    let (username, auth) = match credentials {
        Some(c) => (
            c.username,
            AuthMethod::Password {
                password: c.password,
            },
        ),
        None => identity_login(state, &host)?,
    };
    Ok(Target {
        hostname: host.hostname,
        port: host.port,
        username,
        auth,
        known_hosts: state.config_dir.join("known_hosts"),
        jump,
    })
}

fn load_host(state: &AppState, id: Uuid) -> ApiResult<Host> {
    state
        .session
        .with_vault(|v| v.get::<Host>(Collection::Hosts, id))?
        .data
        .ok_or_else(|| ApiError::new("not_found", "host has no data"))
}

fn identity_login(state: &AppState, host: &Host) -> ApiResult<(String, AuthMethod)> {
    let Some(identity_id) = host.identity_id else {
        return Err(ApiError::new(
            "credentials_required",
            "this host has no identity; supply credentials",
        ));
    };
    let identity = state
        .session
        .with_vault(|v| v.get::<Identity>(Collection::Identities, identity_id))?
        .data
        .ok_or_else(|| ApiError::new("not_found", "identity has no data"))?;
    Ok((identity.username, identity.auth))
}

/// Bridges the engine to the webview: raw PTY bytes over an IPC channel,
/// lifecycle notices over a Tauri event.
struct PaneSink {
    app: AppHandle,
    pane_id: String,
    data: Channel<InvokeResponseBody>,
}

impl TermSink for PaneSink {
    fn data(&self, bytes: &[u8]) {
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
    Ok(())
}

/// Connect a pane to an unsaved `user@host:port`. Without a password the
/// local ssh-agent is used.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn ssh_connect_adhoc(
    app: AppHandle,
    state: State<'_, AppState>,
    pane_id: String,
    hostname: String,
    port: u16,
    username: String,
    password: Option<String>,
    cols: u32,
    rows: u32,
    on_data: Channel<InvokeResponseBody>,
) -> ApiResult<()> {
    if hostname.trim().is_empty() || username.trim().is_empty() {
        return Err(ApiError::new("validation", "user and host are required"));
    }
    let auth = match password {
        Some(p) if !p.is_empty() => AuthMethod::Password { password: p },
        _ => AuthMethod::Agent,
    };
    let params = ConnectParams {
        target: Target {
            hostname: hostname.trim().to_string(),
            port,
            username: username.trim().to_string(),
            auth,
            known_hosts: state.config_dir.join("known_hosts"),
            jump: None,
        },
        cols: cols.max(2),
        rows: rows.max(1),
    };
    let sink = Arc::new(PaneSink {
        app,
        pane_id: pane_id.clone(),
        data: on_data,
    });
    state.ssh.connect(pane_id, params, sink)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Known hosts
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn known_hosts_list(
    state: State<'_, AppState>,
) -> ApiResult<Vec<crate::knownhosts::KnownHost>> {
    crate::knownhosts::list(&state.config_dir.join("known_hosts"))
        .map_err(|e| ApiError::new("io", e.to_string()))
}

#[tauri::command]
pub fn known_hosts_remove(state: State<'_, AppState>, line: usize) -> ApiResult<bool> {
    crate::knownhosts::remove_line(&state.config_dir.join("known_hosts"), line)
        .map_err(|e| ApiError::new("io", e.to_string()))
}

/// Forget the pinned key for `host:port` so the next connection re-learns
/// it. Used by the "Trust new key" action after a key change.
#[tauri::command]
pub fn known_hosts_forget(state: State<'_, AppState>, host: String, port: u16) -> ApiResult<usize> {
    crate::knownhosts::forget(&state.config_dir.join("known_hosts"), &host, port)
        .map_err(|e| ApiError::new("io", e.to_string()))
}

// ---------------------------------------------------------------------------
// Import from ~/.ssh/config
// ---------------------------------------------------------------------------

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
) -> ApiResult<crate::sshconfig::ImportSummary> {
    let local_user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok();
    Ok(state.session.with_vault(|v| {
        crate::sshconfig::import_into_vault(v, &hosts, group.trim(), local_user.as_deref())
    })?)
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

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
) -> ApiResult<Vec<FileEntry>> {
    Ok(state.sftp.get(&session_id)?.list(&path).await?)
}

#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, AppState>,
    session_id: String,
    dir: String,
    name: String,
) -> ApiResult<()> {
    Ok(state.sftp.get(&session_id)?.mkdir(&dir, &name).await?)
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    new_name: String,
) -> ApiResult<()> {
    Ok(state
        .sftp
        .get(&session_id)?
        .rename(&path, &new_name)
        .await?)
}

#[tauri::command]
pub async fn sftp_remove(
    state: State<'_, AppState>,
    session_id: String,
    paths: Vec<String>,
) -> ApiResult<()> {
    let conn = state.sftp.get(&session_id)?;
    for p in paths {
        conn.remove(&p).await?;
    }
    Ok(())
}

#[tauri::command]
pub async fn sftp_close(state: State<'_, AppState>, session_id: String) -> ApiResult<()> {
    state.sftp.close(&session_id).await;
    Ok(())
}

#[tauri::command]
pub fn local_home() -> String {
    sftp::local::home().to_string_lossy().into_owned()
}

#[tauri::command]
pub async fn local_list(path: String) -> ApiResult<Vec<FileEntry>> {
    tauri::async_runtime::spawn_blocking(move || sftp::local::list(std::path::Path::new(&path)))
        .await
        .map_err(|e| ApiError::new("local", e.to_string()))?
        .map_err(Into::into)
}

#[tauri::command]
pub fn local_mkdir(dir: String, name: String) -> ApiResult<()> {
    Ok(sftp::local::mkdir(std::path::Path::new(&dir), &name)?)
}

#[tauri::command]
pub fn local_rename(path: String, new_name: String) -> ApiResult<()> {
    Ok(sftp::local::rename(std::path::Path::new(&path), &new_name)?)
}

#[tauri::command]
pub async fn local_remove(paths: Vec<String>) -> ApiResult<()> {
    tauri::async_runtime::spawn_blocking(move || {
        for p in paths {
            sftp::local::remove(std::path::Path::new(&p))?;
        }
        Ok::<_, SftpError>(())
    })
    .await
    .map_err(|e| ApiError::new("local", e.to_string()))?
    .map_err(Into::into)
}

struct ChannelProgress(Channel<TransferProgress>);
impl ProgressSink for ChannelProgress {
    fn report(&self, p: TransferProgress) {
        let _ = self.0.send(p);
    }
}

/// Start a transfer in the background. Returns immediately; progress and the
/// final state stream over `on_progress`.
#[tauri::command]
pub fn transfer_start(
    state: State<'_, AppState>,
    session_id: String,
    transfer_id: String,
    direction: Direction,
    sources: Vec<String>,
    dest_dir: String,
    on_progress: Channel<TransferProgress>,
) -> ApiResult<()> {
    let sftp = Arc::clone(&state.sftp);
    tauri::async_runtime::spawn(async move {
        let sink = ChannelProgress(on_progress);
        sftp.transfer(
            &session_id,
            transfer_id,
            direction,
            sources,
            dest_dir,
            &sink,
        )
        .await;
    });
    Ok(())
}

#[tauri::command]
pub fn transfer_cancel(state: State<'_, AppState>, transfer_id: String) {
    state.sftp.cancel(&transfer_id);
}

// ---------------------------------------------------------------------------
// Port forwarding
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ForwardStatusEvent {
    pub rule_id: Uuid,
    pub status: ForwardStatus,
}

struct EventStatusSink(AppHandle);
impl StatusSink for EventStatusSink {
    fn status(&self, rule_id: Uuid, status: ForwardStatus) {
        let _ = self
            .0
            .emit(EVENT_FORWARD_STATUS, ForwardStatusEvent { rule_id, status });
    }
}

#[tauri::command]
pub fn list_forwards(state: State<'_, AppState>) -> ApiResult<Vec<Record<ForwardRule>>> {
    list_records(&state, Collection::Forwards)
}

#[tauri::command]
pub fn save_forward(
    state: State<'_, AppState>,
    id: Option<Uuid>,
    rule: ForwardRule,
) -> ApiResult<Record<ForwardRule>> {
    if rule.label.trim().is_empty() {
        return Err(ApiError::new("validation", "label is required"));
    }
    save_record(&state, Collection::Forwards, id, rule)
}

#[tauri::command]
pub fn delete_forward(state: State<'_, AppState>, id: Uuid) -> ApiResult<()> {
    state.forwards.stop(id);
    delete_record(&state, Collection::Forwards, id)
}

#[tauri::command]
pub async fn forward_start(app: AppHandle, state: State<'_, AppState>, id: Uuid) -> ApiResult<()> {
    let rule = state
        .session
        .with_vault(|v| v.get::<ForwardRule>(Collection::Forwards, id))?
        .data
        .ok_or_else(|| ApiError::new("not_found", "rule has no data"))?;
    let target = resolve_target(&state, rule.host_id, None)?;
    state
        .forwards
        .start(id, target, rule.kind, Arc::new(EventStatusSink(app)))?;
    Ok(())
}

#[tauri::command]
pub fn forward_stop(state: State<'_, AppState>, id: Uuid) {
    state.forwards.stop(id);
}

#[tauri::command]
pub fn forward_statuses(
    state: State<'_, AppState>,
) -> std::collections::HashMap<Uuid, ForwardStatus> {
    state.forwards.statuses()
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Resolve the per-machine config dir and register [`AppState`].
pub fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let config_dir = app.path().app_config_dir()?;
    app.manage(AppState {
        session: Session::new(),
        config_dir,
        ssh: Arc::new(SshManager::new()),
        sftp: Arc::new(SftpManager::new()),
        forwards: Arc::new(ForwardManager::new()),
    });
    Ok(())
}
