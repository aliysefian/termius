//! Known hosts.

use super::*;

/// Event asking the UI to trust a host key. Payload: [`HostKeyPrompt`].
pub const EVENT_HOSTKEY_PROMPT: &str = "hostkey:prompt";
/// How long a host-key question waits for an answer before refusing.
const HOSTKEY_PROMPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

pub(super) type PromptMap = Arc<std::sync::Mutex<std::collections::HashMap<Uuid, tokio::sync::oneshot::Sender<bool>>>>;

#[derive(Debug, Clone, Serialize)]
pub struct HostKeyPrompt {
    pub request_id: Uuid,
    #[serde(flatten)]
    pub question: crate::ssh::HostKeyQuestion,
}

/// Host keys trusted in the unlocked vault.
pub(super) struct VaultHostKeys(pub(super) AppHandle);

impl VaultHostKeys {
    fn run<R>(
        &self,
        f: impl FnOnce(&crate::vault::Vault) -> Result<R, VaultError>,
    ) -> Result<R, String> {
        let state = self.0.state::<AppState>();
        state.session.with_vault(f).map_err(|e| ApiError::from(e).message)
    }
}

impl crate::ssh::HostKeyStore for VaultHostKeys {
    fn check(&self, key: &crate::ssh::PresentedKey) -> Result<crate::ssh::HostKeyStatus, String> {
        self.run(|v| crate::hostkeys::check(v, key))
    }
    fn trust(&self, key: &crate::ssh::PresentedKey) -> Result<(), String> {
        self.run(|v| crate::hostkeys::trust(v, key))
    }
}

/// Asks the user through the webview; no answer in time means "don't trust".
pub(super) struct UiPrompter {
    pub(super) app: AppHandle,
    pub(super) pending: PromptMap,
}

impl crate::ssh::HostKeyPrompter for UiPrompter {
    fn ask(&self, question: crate::ssh::HostKeyQuestion) -> crate::ssh::BoxFuture<bool> {
        let app = self.app.clone();
        let pending = Arc::clone(&self.pending);
        Box::pin(async move {
            let request_id = Uuid::new_v4();
            let (tx, rx) = tokio::sync::oneshot::channel();
            pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .insert(request_id, tx);
            let asked = app.emit(EVENT_HOSTKEY_PROMPT, HostKeyPrompt { request_id, question });
            let answer = match asked {
                Ok(()) => matches!(tokio::time::timeout(HOSTKEY_PROMPT_TIMEOUT, rx).await, Ok(Ok(true))),
                Err(_) => false,
            };
            pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .remove(&request_id);
            answer
        })
    }
}

/// The user's answer to a [`HostKeyPrompt`]: trust (or replace) the key, or not.
#[tauri::command]
pub fn answer_host_key(state: State<'_, AppState>, request_id: Uuid, trust: bool) -> ApiResult<()> {
    let tx = state
        .host_key_prompts
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .remove(&request_id)
        .ok_or_else(|| ApiError::new("expired", "that question has expired; connect again"))?;
    let _ = tx.send(trust);
    Ok(())
}

#[derive(Debug, Clone, Serialize)]
pub struct KnownHostEntry {
    pub id: Uuid,
    pub rev: u64,
    #[serde(flatten)]
    pub host: crate::models::KnownHost,
}

#[tauri::command]
pub fn known_hosts_list(state: State<'_, AppState>) -> ApiResult<Vec<KnownHostEntry>> {
    let mut list: Vec<KnownHostEntry> = list_records::<crate::models::KnownHost>(&state, Collection::KnownHosts)?
        .into_iter()
        .filter_map(|r| {
            r.data.map(|host| KnownHostEntry {
                id: r.id,
                rev: r.rev,
                host,
            })
        })
        .collect();
    list.sort_by(|a, b| (&a.host.host, a.host.port).cmp(&(&b.host.host, b.host.port)));
    Ok(list)
}

/// Stop trusting a host's key; the next connection asks again.
#[tauri::command]
pub fn known_hosts_forget(state: State<'_, AppState>, host: String, port: u16) -> ApiResult<bool> {
    Ok(state
        .session
        .with_vault(|v| crate::hostkeys::forget(v, &host, port))?)
}

/// Import entries from an OpenSSH `known_hosts` file (default
/// `~/.ssh/known_hosts`). Hosts already in the vault keep their key.
#[tauri::command]
pub fn known_hosts_import(
    state: State<'_, AppState>,
    path: Option<String>,
) -> ApiResult<crate::hostkeys::ImportReport> {
    let home = sftp::local::home();
    let path = match path.filter(|p| !p.trim().is_empty()) {
        Some(p) => PathBuf::from(crate::sshconfig::expand_path(&p, &home)),
        None => home.join(".ssh").join("known_hosts"),
    };
    let result = state
        .session
        .with_vault(|v| {
            Ok(crate::hostkeys::import_file(v, &path))
        })?
        .map_err(|e| ApiError::new("io", format!("cannot read {}: {e}", path.display())))?;
    Ok(result?)
}
