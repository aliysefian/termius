//! What the monitoring engine stores in the vault: the hosts to watch, the rules that judge their readings, and the
//! channels alerts go out on.
//!
//! A [`MonitorTarget`] is independent of SSH: an address and some checks. It can point at a saved host
//! (`ssh_host_id`) only to borrow that host's credentials for the CPU / memory / disk check. The channel secrets
//! (bot tokens, the SMTP password) are in [`NotifyChannels`], which the window only ever sees through
//! [`NotifyChannels::redacted`].

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The channels record's fixed ID: there is one per vault.
pub const NOTIFY_ID: Uuid = Uuid::from_u128(2);
/// The shortest time between two rounds of checks on a host.
pub const MIN_INTERVAL_SECS: u32 = 10;
/// CPU / memory / disk sampling waits a second on the host, so it runs less often.
pub const MIN_SSH_INTERVAL_SECS: u32 = 30;

fn yes() -> bool {
    true
}
fn default_interval() -> u32 {
    60
}
fn default_ping_count() -> u8 {
    4
}
fn default_ping_timeout_ms() -> u32 {
    1000
}
fn default_attempts() -> u8 {
    3
}
fn default_smtp_port() -> u16 {
    587
}

// -- hosts ------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckSpec {
    /// ICMP echo with the system `ping`: reachability, packet loss and latency.
    Ping {
        #[serde(default = "default_ping_count")]
        count: u8,
        #[serde(default = "default_ping_timeout_ms")]
        timeout_ms: u32,
    },
    /// Is something listening on this TCP port?
    Tcp { port: u16 },
    /// CPU, memory and disk use read over SSH. Needs `ssh_host_id`.
    SshMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorTarget {
    pub label: String,
    /// Host name or IP address the ping and port checks go to.
    pub address: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "default_interval")]
    pub interval_secs: u32,
    #[serde(default)]
    pub checks: Vec<CheckSpec>,
    /// A saved host whose credentials the SSH check uses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ssh_host_id: Option<Uuid>,
    #[serde(default)]
    pub notes: String,
}

impl MonitorTarget {
    pub fn effective_interval(&self) -> u32 {
        self.interval_secs.max(MIN_INTERVAL_SECS)
    }

    pub fn has_ssh_check(&self) -> bool {
        self.checks.iter().any(|c| matches!(c, CheckSpec::SshMetrics))
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.label.trim().is_empty() || self.label.chars().count() > 100 {
            return Err("Give the host a name of up to 100 characters.".into());
        }
        if !super::ping::valid_host(self.address.trim()) {
            return Err("The address must be a host name or an IP address.".into());
        }
        if self.interval_secs < MIN_INTERVAL_SECS {
            return Err(format!("Check at most every {MIN_INTERVAL_SECS} seconds."));
        }
        if self.checks.is_empty() {
            return Err("Add at least one check.".into());
        }
        if self.checks.len() > 20 || self.tags.len() > 20 || self.notes.chars().count() > 2000 {
            return Err("That is too many checks or tags, or too long a note.".into());
        }
        if self.has_ssh_check() && self.interval_secs < MIN_SSH_INTERVAL_SECS {
            return Err(format!("CPU, memory and disk are read at most every {MIN_SSH_INTERVAL_SECS} seconds."));
        }
        for c in &self.checks {
            match c {
                CheckSpec::Ping { count, timeout_ms } => {
                    if !(1..=10).contains(count) || !(200..=10_000).contains(timeout_ms) {
                        return Err("Ping: 1 to 10 packets, and a wait of 200 to 10000 ms.".into());
                    }
                }
                CheckSpec::Tcp { port } => {
                    if *port == 0 {
                        return Err("Port checks need a port from 1 to 65535.".into());
                    }
                }
                CheckSpec::SshMetrics => {
                    if self.ssh_host_id.is_none() {
                        return Err("CPU, memory and disk checks need a saved SSH host to log in with.".into());
                    }
                }
            }
        }
        Ok(())
    }
}

// -- rules ------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Percent of ping packets lost (0 to 100).
    PingLoss,
    /// Average ping round trip, in milliseconds.
    PingLatency,
    /// 1 when a TCP port check could not connect, 0 when it could.
    PortClosed,
    Cpu,
    Mem,
    Disk,
}

impl Metric {
    pub fn label(self) -> &'static str {
        match self {
            Metric::PingLoss => "Ping packet loss",
            Metric::PingLatency => "Ping latency",
            Metric::PortClosed => "Port closed",
            Metric::Cpu => "CPU use",
            Metric::Mem => "Memory use",
            Metric::Disk => "Disk use",
        }
    }

    pub fn unit(self) -> &'static str {
        match self {
            Metric::PingLoss | Metric::Cpu | Metric::Mem | Metric::Disk => "%",
            Metric::PingLatency => " ms",
            Metric::PortClosed => "",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// The reading is above the limit.
    #[default]
    Gt,
    /// The reading is below the limit.
    Lt,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Scope {
    #[default]
    All,
    /// Hosts that carry at least one of these tags.
    Tags { tags: Vec<String> },
    Target { id: Uuid },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelKind {
    Telegram,
    Bale,
    Email,
}

impl ChannelKind {
    pub fn label(self) -> &'static str {
        match self {
            ChannelKind::Telegram => "Telegram",
            ChannelKind::Bale => "Bale",
            ChannelKind::Email => "Email",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub metric: Metric,
    #[serde(default)]
    pub op: Op,
    /// A reading past this is WARNING.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warn: Option<f64>,
    /// A reading past this is CRITICAL.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crit: Option<f64>,
    /// How many checks in a row must agree before the state changes, so one lost packet does not page anyone.
    #[serde(default = "default_attempts")]
    pub attempts: u8,
    #[serde(default)]
    pub scope: Scope,
    /// Only these channels. Empty means every enabled channel.
    #[serde(default)]
    pub channels: Vec<ChannelKind>,
}

impl AlertRule {
    pub fn validate(&self) -> Result<(), String> {
        if self.name.trim().is_empty() || self.name.chars().count() > 100 {
            return Err("Give the rule a name of up to 100 characters.".into());
        }
        if self.warn.is_none() && self.crit.is_none() {
            return Err("Set a warning limit, a critical limit, or both.".into());
        }
        for v in [self.warn, self.crit].into_iter().flatten() {
            if !v.is_finite() {
                return Err("The limits must be numbers.".into());
            }
        }
        if let (Some(w), Some(c)) = (self.warn, self.crit) {
            let ordered = match self.op {
                Op::Gt => w <= c,
                Op::Lt => w >= c,
            };
            if !ordered {
                return Err("The warning limit must be reached before the critical one.".into());
            }
        }
        if !(1..=20).contains(&self.attempts) {
            return Err("Use 1 to 20 checks in a row.".into());
        }
        if let Scope::Tags { tags } = &self.scope {
            if tags.iter().all(|t| t.trim().is_empty()) {
                return Err("Name at least one tag, or apply the rule to all hosts.".into());
            }
        }
        Ok(())
    }

    pub fn applies_to(&self, target_id: Uuid, tags: &[String]) -> bool {
        match &self.scope {
            Scope::All => true,
            Scope::Target { id } => *id == target_id,
            Scope::Tags { tags: wanted } => wanted.iter().any(|w| tags.iter().any(|t| t.eq_ignore_ascii_case(w.trim()))),
        }
    }
}

// -- channels ---------------------------------------------------------------------------------------------------

/// A Telegram or Bale bot. Both speak the Telegram Bot API; only the address differs.
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BotConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token: String,
    /// A number such as `-1001234567890`, or `@channelname`.
    #[serde(default)]
    pub chat_id: String,
    /// Another Bot API address (a mirror or a proxy). `https://` only. Empty uses the service's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Set only on what the window receives: a token is saved but not shown.
    #[serde(default)]
    pub token_set: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SmtpSecurity {
    /// Plain connection upgraded with STARTTLS (usually port 587).
    #[default]
    StartTls,
    /// TLS from the first byte (usually port 465).
    Tls,
    /// No encryption. A password is refused unless the server is this computer, because it would cross the network
    /// in the clear. (Without a login this still sends the alert text unencrypted, for a trusted internal relay.)
    Plain,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmailConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub server: String,
    #[serde(default = "default_smtp_port")]
    pub port: u16,
    #[serde(default)]
    pub security: SmtpSecurity,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    /// The sender address. Empty uses the user name when it is an address.
    #[serde(default)]
    pub from: String,
    /// Where alerts are sent. Several addresses can be separated by commas.
    #[serde(default)]
    pub to: String,
    /// Set only on what the window receives: a password is saved but not shown.
    #[serde(default)]
    pub password_set: bool,
}

/// Printing a config (in a log, a panic, a failed test) must not print its secret.
impl std::fmt::Debug for BotConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BotConfig")
            .field("enabled", &self.enabled)
            .field("bot_token", &if self.bot_token.is_empty() { "" } else { "***" })
            .field("chat_id", &self.chat_id)
            .field("base_url", &self.base_url)
            .finish()
    }
}

impl std::fmt::Debug for EmailConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmailConfig")
            .field("enabled", &self.enabled)
            .field("server", &self.server)
            .field("port", &self.port)
            .field("security", &self.security)
            .field("username", &self.username)
            .field("password", &if self.password.is_empty() { "" } else { "***" })
            .field("from", &self.from)
            .field("to", &self.to)
            .finish()
    }
}

impl Default for EmailConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            server: String::new(),
            port: default_smtp_port(),
            security: SmtpSecurity::StartTls,
            username: String::new(),
            password: String::new(),
            from: String::new(),
            to: String::new(),
            password_set: false,
        }
    }
}

impl EmailConfig {
    /// The sender: `from`, else the user name if that is an address.
    pub fn sender(&self) -> Option<String> {
        let from = self.from.trim();
        if !from.is_empty() {
            return Some(from.to_string());
        }
        let user = self.username.trim();
        user.contains('@').then(|| user.to_string())
    }

    pub fn recipients(&self) -> Vec<String> {
        self.to.split([',', ';']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotifyChannels {
    #[serde(default)]
    pub telegram: BotConfig,
    #[serde(default)]
    pub bale: BotConfig,
    #[serde(default)]
    pub email: EmailConfig,
    /// Put the host's address in messages. Off: only its name, so an address never leaves this computer.
    #[serde(default)]
    pub include_address: bool,
}

/// An address that can go in an SMTP envelope or a header: one `@`, no spaces, brackets or line breaks.
pub fn valid_email(a: &str) -> bool {
    let a = a.trim();
    let Some((local, domain)) = a.split_once('@') else { return false };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && a.len() <= 254
        && a.chars().all(|c| c.is_ascii_graphic() && !matches!(c, '<' | '>' | '(' | ')' | '\\' | '"' | ','))
}

impl NotifyChannels {
    /// What the window receives: the secrets blanked, with a flag saying whether one is saved.
    pub fn redacted(&self) -> Self {
        let mut c = self.clone();
        c.telegram.token_set = !c.telegram.bot_token.is_empty();
        c.telegram.bot_token.clear();
        c.bale.token_set = !c.bale.bot_token.is_empty();
        c.bale.bot_token.clear();
        c.email.password_set = !c.email.password.is_empty();
        c.email.password.clear();
        c
    }

    /// An empty secret from the window means "keep the one stored". The `*_set` flags are never stored.
    pub fn merge_stored_secrets(&mut self, stored: &NotifyChannels) {
        if self.telegram.bot_token.trim().is_empty() {
            self.telegram.bot_token = stored.telegram.bot_token.clone();
        }
        if self.bale.bot_token.trim().is_empty() {
            self.bale.bot_token = stored.bale.bot_token.clone();
        }
        if self.email.password.trim().is_empty() {
            self.email.password = stored.email.password.clone();
        }
        self.telegram.token_set = false;
        self.bale.token_set = false;
        self.email.password_set = false;
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, bot) in [("Telegram", &self.telegram), ("Bale", &self.bale)] {
            if let Some(url) = bot.base_url.as_deref().map(str::trim).filter(|u| !u.is_empty()) {
                if !url.starts_with("https://") || url.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return Err(format!("{name}: the bot address must start with https://"));
                }
            }
            if bot.enabled && (bot.bot_token.trim().is_empty() || bot.chat_id.trim().is_empty()) {
                return Err(format!("{name}: enter the bot token and the chat ID, or turn it off."));
            }
            if bot.chat_id.chars().any(|c| c.is_control()) || bot.bot_token.chars().any(|c| c.is_whitespace() || c.is_control()) {
                return Err(format!("{name}: the token and chat ID can't contain spaces or line breaks."));
            }
        }
        let e = &self.email;
        if e.enabled {
            if !super::ping::valid_host(e.server.trim()) {
                return Err("Email: enter the SMTP server's name.".into());
            }
            if e.port == 0 {
                return Err("Email: enter the SMTP port.".into());
            }
            if e.sender().is_none_or(|s| !valid_email(&s)) {
                return Err("Email: enter a sender address (or a user name that is one).".into());
            }
            let to = e.recipients();
            if to.is_empty() || to.iter().any(|a| !valid_email(a)) {
                return Err("Email: enter a valid address to send alerts to.".into());
            }
        }
        if e.username.chars().any(|c| c.is_control()) || e.password.chars().any(|c| c.is_control()) {
            return Err("Email: the user name and password can't contain line breaks.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> MonitorTarget {
        MonitorTarget {
            label: "web".into(),
            address: "10.0.0.5".into(),
            tags: vec!["Prod".into()],
            enabled: true,
            interval_secs: 60,
            checks: vec![CheckSpec::Ping { count: 4, timeout_ms: 1000 }, CheckSpec::Tcp { port: 22 }],
            ssh_host_id: None,
            notes: String::new(),
        }
    }

    fn rule() -> AlertRule {
        AlertRule {
            name: "loss".into(),
            enabled: true,
            metric: Metric::PingLoss,
            op: Op::Gt,
            warn: Some(0.0),
            crit: Some(50.0),
            attempts: 3,
            scope: Scope::All,
            channels: vec![],
        }
    }

    #[test]
    fn a_stored_target_with_missing_fields_still_loads_with_defaults() {
        let t: MonitorTarget = serde_json::from_str(r#"{"label":"a","address":"h","checks":[{"kind":"ping"},{"kind":"tcp","port":22},{"kind":"ssh_metrics"}]}"#).unwrap();
        assert!(t.enabled);
        assert_eq!(t.interval_secs, 60);
        assert_eq!(t.checks[0], CheckSpec::Ping { count: 4, timeout_ms: 1000 });
        assert_eq!(t.checks[2], CheckSpec::SshMetrics);
    }

    #[test]
    fn target_validation_names_the_problem() {
        assert!(target().validate().is_ok());
        let mut t = target();
        t.address = "-oProxyCommand=x".into();
        assert!(t.validate().unwrap_err().contains("address"));
        let mut t = target();
        t.interval_secs = 5;
        assert!(t.validate().is_err());
        let mut t = target();
        t.checks = vec![CheckSpec::SshMetrics];
        assert!(t.validate().unwrap_err().contains("SSH host"));
        t.ssh_host_id = Some(Uuid::new_v4());
        assert!(t.validate().is_ok());
        let mut t = target();
        t.checks = vec![CheckSpec::Tcp { port: 0 }];
        assert!(t.validate().is_err());
        t.checks.clear();
        assert!(t.validate().is_err());
    }

    #[test]
    fn rule_validation_checks_the_order_of_the_limits() {
        assert!(rule().validate().is_ok());
        let mut r = rule();
        r.warn = Some(80.0);
        assert!(r.validate().is_err(), "warning above critical");
        r.op = Op::Lt;
        r.warn = Some(20.0);
        r.crit = Some(10.0);
        assert!(r.validate().is_ok(), "below-limits count down");
        r.warn = None;
        r.crit = None;
        assert!(r.validate().is_err());
        let mut r = rule();
        r.attempts = 0;
        assert!(r.validate().is_err());
        r.attempts = 3;
        r.warn = Some(f64::NAN);
        assert!(r.validate().is_err());
    }

    #[test]
    fn rules_apply_by_scope_and_tag_case_is_ignored() {
        let id = Uuid::new_v4();
        let tags = vec!["Prod".to_string()];
        let mut r = rule();
        assert!(r.applies_to(id, &tags));
        r.scope = Scope::Tags { tags: vec!["prod".into()] };
        assert!(r.applies_to(id, &tags));
        assert!(!r.applies_to(id, &["dev".to_string()]));
        r.scope = Scope::Target { id };
        assert!(r.applies_to(id, &[]));
        assert!(!r.applies_to(Uuid::new_v4(), &[]));
    }

    #[test]
    fn redaction_hides_every_secret_and_says_which_are_saved() {
        let mut c = NotifyChannels::default();
        c.telegram.bot_token = "123:ABC".into();
        c.email.password = "hunter2".into();
        let r = c.redacted();
        let json = serde_json::to_string(&r).unwrap();
        assert!(!json.contains("123:ABC") && !json.contains("hunter2"), "{json}");
        assert!(r.telegram.token_set && !r.bale.token_set && r.email.password_set);
    }

    #[test]
    fn saving_with_an_empty_secret_keeps_the_stored_one() {
        let mut stored = NotifyChannels::default();
        stored.telegram.bot_token = "123:ABC".into();
        stored.email.password = "hunter2".into();
        let mut incoming = stored.redacted();
        incoming.telegram.chat_id = "42".into();
        incoming.merge_stored_secrets(&stored);
        assert_eq!(incoming.telegram.bot_token, "123:ABC");
        assert_eq!(incoming.email.password, "hunter2");
        assert!(!incoming.telegram.token_set && !incoming.email.password_set);
        incoming.telegram.bot_token = "999:NEW".into();
        incoming.merge_stored_secrets(&stored);
        assert_eq!(incoming.telegram.bot_token, "999:NEW", "a new secret replaces the old");
    }

    #[test]
    fn a_login_check_needs_an_interval_it_can_keep_and_there_are_limits() {
        let mut t = target();
        t.checks = vec![CheckSpec::SshMetrics];
        t.ssh_host_id = Some(Uuid::new_v4());
        t.interval_secs = 20;
        assert!(t.validate().unwrap_err().contains("30 seconds"));
        t.interval_secs = 30;
        assert!(t.validate().is_ok());
        let mut many = target();
        many.checks = (1..=21).map(|port| CheckSpec::Tcp { port }).collect();
        assert!(many.validate().is_err());
        let mut long = target();
        long.notes = "x".repeat(2001);
        assert!(long.validate().is_err());
    }

    #[test]
    fn a_blank_secret_keeps_the_stored_one_even_if_it_is_only_spaces() {
        let mut stored = NotifyChannels::default();
        stored.telegram.bot_token = "123:ABC".into();
        let mut incoming = NotifyChannels::default();
        incoming.telegram.bot_token = "   ".into();
        incoming.merge_stored_secrets(&stored);
        assert_eq!(incoming.telegram.bot_token, "123:ABC");
    }

    #[test]
    fn printing_a_config_never_shows_a_secret() {
        let mut c = NotifyChannels::default();
        c.telegram.bot_token = "123:TOKEN-marker".into();
        c.bale.bot_token = "456:OTHER-marker".into();
        c.email.password = "pw-marker".into();
        let text = format!("{c:?}");
        assert!(!text.contains("marker"), "{text}");
        assert!(text.contains("***"));
    }

    #[test]
    fn channel_validation() {
        let mut c = NotifyChannels::default();
        assert!(c.validate().is_ok(), "everything off is fine");
        c.telegram.enabled = true;
        assert!(c.validate().is_err(), "enabled without a token");
        c.telegram.bot_token = "1:a".into();
        c.telegram.chat_id = "42".into();
        assert!(c.validate().is_ok());
        c.telegram.base_url = Some("http://insecure.example".into());
        assert!(c.validate().is_err());
        c.telegram.base_url = Some("https://tg.example.org".into());
        assert!(c.validate().is_ok());
        c.email.enabled = true;
        c.email.server = "smtp.example.com".into();
        c.email.username = "alerts@example.com".into();
        c.email.to = "ops@example.com, oncall@example.com".into();
        assert!(c.validate().is_ok());
        assert_eq!(c.email.recipients().len(), 2);
        assert_eq!(c.email.sender().as_deref(), Some("alerts@example.com"));
        c.email.to = "ops@example.com\r\nBcc: x@y.z".into();
        assert!(c.validate().is_err(), "header injection");
        c.email.to = "ops@example.com".into();
        c.email.from = "not an address".into();
        assert!(c.validate().is_err());
    }

    #[test]
    fn email_addresses() {
        assert!(valid_email("a.b+c@example.com"));
        for bad in ["", "a", "@x.com", "a@", "a b@x.com", "a@b@c", "a\r\n@x.com", "<a@x.com>"] {
            assert!(!valid_email(bad), "{bad:?}");
        }
    }
}
