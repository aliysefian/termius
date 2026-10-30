//! The monitoring engine's commands: the hosts to watch, the alert rules, the notification channels, and the engine's
//! state. See `monitoring/` for how it works.
//!
//! The channel settings hold secrets (bot tokens, the SMTP password). They are read here only to be redacted or to
//! be used by the engine; the window gets them blanked, and saving with a blank secret keeps the stored one.

use std::time::Duration;

use super::*;
use crate::monitoring::engine::{Config, EngineEvent, EngineHost, Snapshot};
use crate::monitoring::model::{AlertRule, ChannelKind, MonitorTarget, NotifyChannels, NOTIFY_ID};
use crate::monitoring::notify::{BoxFuture, Proxies};

// -- the engine's view of the app ------------------------------------------------------------------------------------

/// What the engine asks of the app: the unlocked vault, saved SSH hosts to log in to, and the window to tell.
pub struct AlertHost {
    app: AppHandle,
    /// The SSH sessions this engine opened (in the app's monitor manager, so locking the vault closes them), by host.
    sessions: std::sync::Mutex<HashMap<Uuid, Uuid>>,
}

impl AlertHost {
    fn new(app: AppHandle) -> Self {
        Self { app, sessions: Default::default() }
    }

    fn monitors(&self) -> Arc<crate::monitor::MonitorManager> {
        Arc::clone(&self.app.state::<AppState>().monitors)
    }

    fn cached(&self, host_id: Uuid) -> Option<Uuid> {
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).get(&host_id).copied()
    }

    async fn session_for(&self, host_id: Uuid) -> Result<Uuid, String> {
        if let Some(id) = self.cached(host_id) {
            return Ok(id);
        }
        // Nobody is there to answer a host key dialog or a login prompt: an unknown key or a missing credential is an
        // error for that host, not a question.
        let mut target = {
            let state = self.app.state::<AppState>();
            resolve_target(&state, host_id, None).map_err(|e| e.message)?
        };
        super::network::quiet(&mut target);
        let id = self.monitors().open(target).await.map_err(|e| e.to_string())?;
        self.sessions.lock().unwrap_or_else(|p| p.into_inner()).insert(host_id, id);
        Ok(id)
    }
}

impl EngineHost for AlertHost {
    fn unlocked(&self) -> bool {
        self.app.state::<AppState>().session.is_unlocked()
    }

    fn load(&self) -> Result<Config, String> {
        let state = self.app.state::<AppState>();
        state
            .session
            .with_vault(|v| {
                let targets = v.list::<MonitorTarget>(Collection::Monitors)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
                let rules = v.list::<AlertRule>(Collection::AlertRules)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
                let channels = match v.get::<NotifyChannels>(Collection::NotifyChannels, NOTIFY_ID) {
                    Ok(r) => r.data.unwrap_or_default(),
                    Err(VaultError::NotFound { .. } | VaultError::Deleted { .. }) => NotifyChannels::default(),
                    Err(e) => return Err(e),
                };
                Ok(Config { targets, rules, channels })
            })
            .map_err(|e| e.to_string())
    }

    fn proxies(&self) -> Proxies {
        saved_proxies(&self.app.state::<AppState>())
    }

    fn ssh_exec<'a>(&'a self, host_id: Uuid, script: &'a str, limit: Duration) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let session = self.session_for(host_id).await?;
            match self.monitors().exec(session, script, limit).await {
                Ok(out) if !out.stdout.trim().is_empty() => Ok(out.stdout),
                Ok(out) => {
                    let said = out.stderr.trim();
                    Err(if said.is_empty() { "the host printed nothing".to_string() } else { said.chars().take(200).collect() })
                }
                Err(e) => {
                    // Start from a fresh login next time.
                    let stale = self.sessions.lock().unwrap_or_else(|p| p.into_inner()).remove(&host_id);
                    if let Some(id) = stale {
                        self.monitors().close(id).await;
                    }
                    Err(e.to_string())
                }
            }
        })
    }

    fn emit(&self, event: EngineEvent) {
        match event {
            EngineEvent::Status(s) => {
                let _ = self.app.emit("monitor:status", &s);
            }
            EngineEvent::Alert(a) => {
                let _ = self.app.emit("monitor:alert", &a);
            }
        }
    }
}

/// The saved proxies by id. Empty while the vault is locked, when nothing is sent anyway.
fn saved_proxies(state: &AppState) -> Proxies {
    list_records::<crate::models::Proxy>(state, Collection::Proxies)
        .map(|rs| rs.into_iter().filter_map(|r| r.data.map(|p| (r.id, p))).collect())
        .unwrap_or_default()
}

/// Start the engine for the vault that was just unlocked.
pub async fn start_alerting(app: &AppHandle, state: &AppState) {
    state.alerting.start(Arc::new(AlertHost::new(app.clone()))).await;
}

// -- hosts to watch ----------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn list_monitors(state: State<'_, AppState>) -> ApiResult<Vec<Record<MonitorTarget>>> {
    list_records(&state, Collection::Monitors)
}

#[tauri::command]
pub fn save_monitor(state: State<'_, AppState>, id: Option<Uuid>, base_rev: Option<u64>, monitor: MonitorTarget) -> ApiResult<Record<MonitorTarget>> {
    let mut monitor = monitor;
    monitor.label = monitor.label.trim().to_string();
    monitor.address = monitor.address.trim().to_string();
    monitor.tags = monitor.tags.iter().map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
    monitor.validate().map_err(|m| ApiError::new("validation", m))?;
    let saved = save_record(&state, Collection::Monitors, id, base_rev, monitor)?;
    state.alerting.reload();
    Ok(saved)
}

#[tauri::command]
pub fn delete_monitor(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    delete_record(&state, Collection::Monitors, id, base_rev)?;
    state.alerting.reload();
    Ok(())
}

// -- rules ---------------------------------------------------------------------------------------------------------------

#[tauri::command]
pub fn list_alert_rules(state: State<'_, AppState>) -> ApiResult<Vec<Record<AlertRule>>> {
    list_records(&state, Collection::AlertRules)
}

#[tauri::command]
pub fn save_alert_rule(state: State<'_, AppState>, id: Option<Uuid>, base_rev: Option<u64>, rule: AlertRule) -> ApiResult<Record<AlertRule>> {
    let mut rule = rule;
    rule.name = rule.name.trim().to_string();
    rule.validate().map_err(|m| ApiError::new("validation", m))?;
    let saved = save_record(&state, Collection::AlertRules, id, base_rev, rule)?;
    state.alerting.reload();
    Ok(saved)
}

#[tauri::command]
pub fn delete_alert_rule(state: State<'_, AppState>, id: Uuid, base_rev: Option<u64>) -> ApiResult<()> {
    delete_record(&state, Collection::AlertRules, id, base_rev)?;
    state.alerting.reload();
    Ok(())
}

// -- channels ------------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct ChannelsRecord {
    pub rev: u64,
    /// With every secret blanked; `token_set` / `password_set` say which are saved.
    pub channels: NotifyChannels,
}

fn stored_channels(state: &AppState) -> ApiResult<(u64, NotifyChannels)> {
    let found = state.session.with_vault(|v| match v.get::<NotifyChannels>(Collection::NotifyChannels, NOTIFY_ID) {
        Ok(r) => Ok(Some(r)),
        Err(VaultError::NotFound { .. } | VaultError::Deleted { .. }) => Ok(None),
        Err(e) => Err(e),
    })?;
    Ok(match found {
        Some(r) => (r.rev, r.data.unwrap_or_default()),
        None => (0, NotifyChannels::default()),
    })
}

#[tauri::command]
pub fn get_notify_channels(state: State<'_, AppState>) -> ApiResult<ChannelsRecord> {
    let (rev, channels) = stored_channels(&state)?;
    Ok(ChannelsRecord { rev, channels: channels.redacted() })
}

#[tauri::command]
pub fn save_notify_channels(state: State<'_, AppState>, base_rev: u64, channels: NotifyChannels) -> ApiResult<ChannelsRecord> {
    let (_, stored) = stored_channels(&state)?;
    let mut channels = channels;
    // A blank secret from the window means "keep what is saved".
    channels.merge_stored_secrets(&stored);
    for bot in [&mut channels.telegram, &mut channels.bale] {
        bot.bot_token = bot.bot_token.trim().to_string();
        bot.chat_id = bot.chat_id.trim().to_string();
        bot.base_url = bot.base_url.take().map(|u| u.trim().to_string()).filter(|u| !u.is_empty());
    }
    channels.email.server = channels.email.server.trim().to_string();
    channels.email.username = channels.email.username.trim().to_string();
    channels.validate().map_err(|m| ApiError::new("validation", m))?;
    // A proxy that is chosen has to exist and be one a bot can use, so a typo shows now rather than at the first alert.
    let proxies = saved_proxies(&state);
    for (kind, bot) in [(ChannelKind::Telegram, &channels.telegram), (ChannelKind::Bale, &channels.bale)] {
        crate::monitoring::notify::bot::proxy_for(kind, bot, &proxies).map_err(|e| ApiError::new("validation", e.0))?;
    }
    let base = if base_rev == 0 { crate::vault::Base::New } else { crate::vault::Base::Rev(base_rev) };
    let rec = state.session.with_vault(|v| v.put(Collection::NotifyChannels, NOTIFY_ID, &channels, base))?;
    state.alerting.reload();
    Ok(ChannelsRecord { rev: rec.rev, channels: channels.redacted() })
}

/// Send a test message on one channel with the saved settings.
#[tauri::command]
pub async fn notify_test(state: State<'_, AppState>, channel: ChannelKind) -> ApiResult<()> {
    let (_, cfg) = stored_channels(&state)?;
    let incomplete = match channel {
        ChannelKind::Telegram => cfg.telegram.bot_token.is_empty() || cfg.telegram.chat_id.is_empty(),
        ChannelKind::Bale => cfg.bale.bot_token.is_empty() || cfg.bale.chat_id.is_empty(),
        ChannelKind::Email => cfg.email.server.is_empty() || cfg.email.recipients().is_empty(),
    };
    if incomplete {
        return Err(ApiError::new("validation", format!("Save the {} settings first.", channel.label())));
    }
    let proxies = saved_proxies(&state);
    crate::monitoring::engine::send_test(&cfg, channel, &proxies).await.map_err(|m| ApiError::new("notify", m))
}

// -- the engine ------------------------------------------------------------------------------------------------------------

/// What every watched host last read, the alerts raised since the vault was unlocked, and whether the engine is
/// running.
#[tauri::command]
pub fn monitor_snapshot(state: State<'_, AppState>) -> Snapshot {
    state.alerting.snapshot()
}

/// Which ports of a watched host accept a connection, like `nc -z` for many ports at once. Runs when asked. It takes
/// the host's id and uses the address saved with it, so it can't be aimed at some other machine.
#[tauri::command]
pub async fn monitor_scan(state: State<'_, AppState>, id: Uuid, ports: String) -> ApiResult<crate::monitoring::scan::ScanReport> {
    use crate::monitoring::scan;
    let ports = scan::parse_ports(&ports).map_err(|m| ApiError::new("validation", m))?;
    let address = list_records::<MonitorTarget>(&state, Collection::Monitors)?
        .into_iter()
        .find(|r| r.id == id)
        .and_then(|r| r.data)
        .map(|t| t.address.trim().to_string())
        .ok_or_else(|| ApiError::new("not_found", "That host is not in the list."))?;
    scan::scan(&address, &ports, Duration::from_millis(1500)).await.map_err(|m| ApiError::new("scan", m))
}

/// Check one host now, whatever its schedule.
#[tauri::command]
pub fn monitor_run_now(state: State<'_, AppState>, id: Uuid) {
    state.alerting.run_now(id);
}
