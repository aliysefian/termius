//! Import hosts from an OpenSSH client config (`~/.ssh/config`).
//!
//! Follows OpenSSH's resolution rules for the options we map: for each
//! concrete alias, every `Host` block whose patterns match contributes, and
//! the first value obtained for an option wins. That makes a trailing
//! `Host *` block act as defaults, exactly as in `ssh`.
//!
//! Only literal aliases become hosts; wildcard-only and negated patterns are
//! used for matching but never imported. `Match` blocks and `Include` are not
//! evaluated and are reported as warnings.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One importable host, fully resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedHost {
    pub alias: String,
    pub hostname: String,
    pub port: u16,
    pub user: Option<String>,
    /// Absolute path after `~` / `%d` expansion.
    pub identity_file: Option<String>,
    /// Alias of a single-hop ProxyJump, if any.
    pub proxy_jump: Option<String>,
    /// `ForwardAgent yes`.
    #[serde(default)]
    pub forward_agent: bool,
    /// `ForwardX11 yes`.
    #[serde(default)]
    pub forward_x11: bool,
    /// Folder inside the import's target group, e.g. an Ansible group path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// `ProxyCommand`, imported unapproved: it never runs until the user
    /// reviews and approves it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_command: Option<String>,
    /// `ServerAliveInterval`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keepalive_secs: Option<u32>,
    /// `LocalForward`, `RemoteForward` and `DynamicForward` lines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forwards: Vec<crate::models::ForwardKind>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParseResult {
    pub hosts: Vec<ImportedHost>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
struct Block {
    /// (pattern, negated)
    patterns: Vec<(String, bool)>,
    options: Vec<(String, String)>,
}

impl Block {
    fn matches(&self, alias: &str) -> bool {
        let mut positive = false;
        for (pat, neg) in &self.patterns {
            if glob_match(pat, alias) {
                if *neg {
                    return false;
                }
                positive = true;
            }
        }
        positive
    }
}

/// OpenSSH-style glob: `*` any run, `?` any single char, case-insensitive.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let (mut star, mut mark) = (None, 0);
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some(pi);
            mark = ti;
            pi += 1;
        } else if let Some(s) = star {
            pi = s + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

fn is_literal(pattern: &str) -> bool {
    !pattern.contains(['*', '?']) && !pattern.starts_with('!')
}

/// Split a config line into keyword and value. Accepts `Key value`,
/// `Key=value` and `Key = value`, strips one layer of double quotes.
fn split_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let idx = line.find(|c: char| c.is_whitespace() || c == '=')?;
    let key = line[..idx].to_ascii_lowercase();
    let rest = line[idx..].trim_start_matches(|c: char| c.is_whitespace() || c == '=');
    let rest = rest.trim();
    let value = rest
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(rest);
    Some((key, value.to_string()))
}

/// Expand a leading `~` or `%d` to the home directory. The remainder is
/// joined component by component, so the result uses the platform's own
/// separator (`C:\Users\me\.ssh\id_ed25519` on Windows) even though ssh
/// configs are usually written with `/`.
pub fn expand_path(p: &str, home: &Path) -> String {
    let rest = if p == "~" || p == "%d" {
        Some("")
    } else {
        ["~/", "~\\", "%d/", "%d\\"]
            .iter()
            .find_map(|prefix| p.strip_prefix(prefix))
    };
    match rest {
        Some(rest) => {
            let mut path = home.to_path_buf();
            for part in rest.split(['/', '\\']).filter(|c| !c.is_empty()) {
                path.push(part);
            }
            path.to_string_lossy().into_owned()
        }
        // `%d` elsewhere in the value is rare; substitute it as-is.
        None => p.replace("%d", &home.to_string_lossy()),
    }
}

/// Parse config text. `home` is used to expand `~` and `%d`.
pub fn parse(text: &str, home: &Path) -> ParseResult {
    let mut result = ParseResult::default();
    // Options before the first Host line apply to every host.
    let mut blocks: Vec<Block> = vec![Block {
        patterns: vec![("*".into(), false)],
        options: Vec::new(),
    }];
    let mut in_match = false;

    for (lineno, raw) in text.lines().enumerate() {
        let Some((key, value)) = split_line(raw) else {
            continue;
        };
        match key.as_str() {
            "host" => {
                in_match = false;
                let patterns = value
                    .split_whitespace()
                    .map(|p| match p.strip_prefix('!') {
                        Some(rest) => (rest.to_string(), true),
                        None => (p.to_string(), false),
                    })
                    .collect();
                blocks.push(Block {
                    patterns,
                    options: Vec::new(),
                });
            }
            "match" => {
                in_match = true;
                result.warnings.push(format!(
                    "line {}: Match blocks are not supported and were skipped",
                    lineno + 1
                ));
            }
            "include" => result.warnings.push(format!(
                "line {}: Include is not followed ({value})",
                lineno + 1
            )),
            _ if in_match => {}
            _ => {
                if let Some(b) = blocks.last_mut() {
                    b.options.push((key, value));
                }
            }
        }
    }

    // Every literal alias, in file order, without duplicates.
    let mut aliases: Vec<String> = Vec::new();
    for b in &blocks[1..] {
        for (p, neg) in &b.patterns {
            if !neg && is_literal(p) && !aliases.iter().any(|a| a.eq_ignore_ascii_case(p)) {
                aliases.push(p.clone());
            }
        }
    }

    for alias in aliases {
        let get = |opt: &str| -> Option<String> {
            blocks
                .iter()
                .filter(|b| b.matches(&alias))
                .flat_map(|b| b.options.iter())
                .find(|(k, _)| k == opt)
                .map(|(_, v)| v.clone())
        };
        // Forwarding options accumulate rather than first-wins.
        let get_all = |opt: &str| -> Vec<String> {
            blocks
                .iter()
                .filter(|b| b.matches(&alias))
                .flat_map(|b| b.options.iter())
                .filter(|(k, _)| k == opt)
                .map(|(_, v)| v.clone())
                .collect()
        };

        let hostname = get("hostname")
            .map(|h| h.replace("%h", &alias))
            .unwrap_or_else(|| alias.clone());
        let port = match get("port") {
            None => 22,
            Some(p) => match p.parse::<u16>() {
                Ok(p) if p > 0 => p,
                _ => {
                    result
                        .warnings
                        .push(format!("{alias}: invalid Port \"{p}\", using 22"));
                    22
                }
            },
        };
        let proxy_jump = match get("proxyjump") {
            Some(j) if j.eq_ignore_ascii_case("none") => None,
            Some(j) if j.contains(',') => {
                result.warnings.push(format!(
                    "{alias}: multi-hop ProxyJump \"{j}\" not imported; set jump hosts manually"
                ));
                None
            }
            Some(j) => {
                // Strip an optional user@ and :port; we map to a saved alias.
                let host = j.rsplit('@').next().unwrap_or(&j);
                let host = host.split(':').next().unwrap_or(host);
                Some(host.to_string())
            }
            None => None,
        };
        let proxy_command = match get("proxycommand") {
            Some(c) if c.eq_ignore_ascii_case("none") => None,
            Some(_) if proxy_jump.is_some() => {
                result.warnings.push(format!(
                    "{alias}: both ProxyJump and ProxyCommand are set; using ProxyJump"
                ));
                None
            }
            Some(c) => {
                result.warnings.push(format!(
                    "{alias}: ProxyCommand \"{c}\" was imported but won't run until you approve it"
                ));
                Some(c)
            }
            None => None,
        };
        let keepalive_secs = get("serveraliveinterval").and_then(|v| match v.parse::<u32>() {
            Ok(n) => Some(n),
            Err(_) => {
                result
                    .warnings
                    .push(format!("{alias}: invalid ServerAliveInterval \"{v}\""));
                None
            }
        });
        let mut forwards = Vec::new();
        for (opt, kind) in [("localforward", 'L'), ("remoteforward", 'R'), ("dynamicforward", 'D')] {
            for v in get_all(opt) {
                match parse_forward(kind, &v) {
                    Ok(f) => forwards.push(f),
                    Err(e) => result.warnings.push(format!("{alias}: {e}")),
                }
            }
        }

        result.hosts.push(ImportedHost {
            alias: alias.clone(),
            hostname,
            port,
            user: get("user"),
            identity_file: get("identityfile").map(|p| expand_path(&p, home)),
            proxy_jump,
            forward_agent: get("forwardagent").is_some_and(|v| v.eq_ignore_ascii_case("yes")),
            forward_x11: get("forwardx11").is_some_and(|v| v.eq_ignore_ascii_case("yes")),
            group: None,
            proxy_command,
            keepalive_secs,
            forwards,
        });
    }
    result
}

/// `[bind_address:]port` → (address, port). No address means loopback,
/// which is also OpenSSH's default without `GatewayPorts`.
fn parse_listen(spec: &str) -> Option<(String, u16)> {
    let (addr, port) = match spec.rsplit_once(':') {
        Some((a, p)) => (a.trim_matches(['[', ']']).to_string(), p),
        None => (String::new(), spec),
    };
    let port = port.parse::<u16>().ok()?;
    let addr = match addr.as_str() {
        "" | "localhost" => "127.0.0.1".to_string(),
        "*" => "0.0.0.0".to_string(),
        _ => addr,
    };
    Some((addr, port))
}

/// `host:port` (or `[v6]:port`) → (host, port).
fn parse_dest(spec: &str) -> Option<(String, u16)> {
    let (host, port) = spec.rsplit_once(':')?;
    Some((host.trim_matches(['[', ']']).to_string(), port.parse().ok()?))
}

fn parse_forward(kind: char, value: &str) -> Result<crate::models::ForwardKind, String> {
    use crate::models::ForwardKind;
    let parts: Vec<&str> = value.split_whitespace().collect();
    let bad = || format!("could not read forward \"{value}\"");
    match (kind, parts.as_slice()) {
        ('D', [listen]) => {
            let (bind_addr, bind_port) = parse_listen(listen).ok_or_else(bad)?;
            Ok(ForwardKind::Dynamic { bind_addr, bind_port })
        }
        ('L' | 'R', [listen, dest]) => {
            let (bind_addr, bind_port) = parse_listen(listen).ok_or_else(bad)?;
            let (dest_host, dest_port) = parse_dest(dest).ok_or_else(bad)?;
            Ok(if kind == 'L' {
                ForwardKind::Local { bind_addr, bind_port, dest_host, dest_port }
            } else {
                ForwardKind::Remote { bind_addr, bind_port, dest_host, dest_port }
            })
        }
        ('R', [_]) => Err(format!("remote dynamic forward \"{value}\" is not supported")),
        _ => Err(bad()),
    }
}

/// How imported `IdentityFile`s are handled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyImport {
    /// Reference the file by path; it stays on this computer.
    #[default]
    Reference,
    /// Copy the key into the encrypted vault (Key Manager), with approval.
    Copy,
}

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImportSummary {
    pub hosts_created: usize,
    pub identities_created: usize,
    #[serde(default)]
    pub keys_imported: usize,
    #[serde(default)]
    pub proxies_created: usize,
    #[serde(default)]
    pub forwards_created: usize,
    pub skipped_existing: Vec<String>,
    pub warnings: Vec<String>,
}

/// Create hosts (and the identities they need) in `vault`.
///
/// * Hosts whose alias matches an existing host label are skipped.
/// * One identity per distinct (user, IdentityFile) pair. With
///   [`KeyImport::Copy`] the key goes into the Key Manager (encrypted in the
///   vault); otherwise the identity references the file by path. Without an
///   IdentityFile the identity uses ssh-agent.
/// * ProxyCommands become unapproved proxies; forwards become rules that
///   don't auto-start.
/// * A ProxyJump alias is linked to the imported or already-saved host with
///   that label, unless doing so would create a loop.
pub fn import_into_vault(
    vault: &crate::vault::Vault,
    hosts: &[ImportedHost],
    group: &str,
    local_user: Option<&str>,
    key_import: KeyImport,
) -> Result<ImportSummary, crate::vault::VaultError> {
    use crate::models::{jump_chain, AuthMethod, ForwardRule, Host, Identity, Proxy, ProxySpec};
    use crate::vault::Collection;
    use std::collections::HashMap;
    use uuid::Uuid;

    let mut summary = ImportSummary::default();
    let existing = vault.list::<Host>(Collection::Hosts)?.records;
    let mut id_by_label: HashMap<String, Uuid> = existing
        .iter()
        .filter_map(|r| r.data.as_ref().map(|d| (d.label.to_lowercase(), r.id)))
        .collect();
    let mut jump_of: HashMap<Uuid, Option<Uuid>> = existing
        .iter()
        .filter_map(|r| r.data.as_ref().map(|d| (r.id, d.jump_host_id)))
        .collect();

    // Assign ids first so ProxyJump can point at hosts imported later.
    let mut todo: Vec<(Uuid, &ImportedHost)> = Vec::new();
    for h in hosts {
        let key = h.alias.to_lowercase();
        if id_by_label.contains_key(&key) {
            summary.skipped_existing.push(h.alias.clone());
            continue;
        }
        let id = Uuid::new_v4();
        id_by_label.insert(key, id);
        jump_of.insert(id, None);
        todo.push((id, h));
    }

    let mut identities: HashMap<(String, Option<String>), Uuid> = HashMap::new();
    let mut keys_by_path: HashMap<String, Option<Uuid>> = HashMap::new();
    let mut proxies: HashMap<String, Uuid> = HashMap::new();
    let mut built: Vec<(Uuid, Host)> = Vec::new();
    for (id, h) in &todo {
        let user = h.user.clone().or_else(|| local_user.map(str::to_string));
        // Resolve the key first: an unusable file falls back to ssh-agent and
        // shares the agent identity for that user instead of making another.
        let key_file: Option<(String, AuthMethod)> = match &h.identity_file {
            None => None,
            Some(path) if key_import == KeyImport::Reference => {
                if !Path::new(path).is_file() {
                    summary.warnings.push(format!(
                        "{}: {path} doesn't exist on this computer; it's referenced anyway",
                        h.alias
                    ));
                }
                Some((
                    path.clone(),
                    AuthMethod::KeyFile {
                        path: path.clone(),
                        passphrase: None,
                    },
                ))
            }
            Some(path) => {
                let key_id = match keys_by_path.get(path) {
                    Some(k) => *k,
                    None => {
                        let k = copy_key(vault, path, &h.alias, &mut summary)?;
                        keys_by_path.insert(path.clone(), k);
                        k
                    }
                };
                key_id.map(|key_id| (path.clone(), AuthMethod::Key { key_id }))
            }
        };
        let identity_id = match &user {
            None => None,
            Some(user) => {
                let key = (user.clone(), key_file.as_ref().map(|(p, _)| p.clone()));
                if let Some(existing) = identities.get(&key) {
                    Some(*existing)
                } else {
                    let (label, auth) = match key_file {
                        Some((path, auth)) => {
                            let file = Path::new(&path)
                                .file_name()
                                .map(|f| f.to_string_lossy().into_owned())
                                .unwrap_or(path);
                            (format!("{user} · {file}"), auth)
                        }
                        None => (format!("{user} (ssh-agent)"), AuthMethod::Agent),
                    };
                    let rec = vault.insert(
                        Collection::Identities,
                        &Identity {
                            label,
                            username: user.clone(),
                            auth,
                            notes: "Imported from ssh config".into(),
                            for_host: None,
                        },
                    )?;
                    summary.identities_created += 1;
                    identities.insert(key, rec.id);
                    Some(rec.id)
                }
            }
        };
        let proxy_id = match &h.proxy_command {
            None => None,
            Some(cmd) => Some(match proxies.get(cmd) {
                Some(p) => *p,
                None => {
                    let rec = vault.insert(
                        Collection::Proxies,
                        &Proxy {
                            name: format!("ProxyCommand ({})", h.alias),
                            spec: ProxySpec::Command {
                                command: cmd.clone(),
                                approved: false,
                            },
                        },
                    )?;
                    summary.proxies_created += 1;
                    proxies.insert(cmd.clone(), rec.id);
                    rec.id
                }
            }),
        };
        for (n, kind) in h.forwards.iter().enumerate() {
            vault.insert(
                Collection::Forwards,
                &ForwardRule {
                    label: format!("{} #{}", h.alias, n + 1),
                    host_id: *id,
                    auto_start: false,
                    kind: kind.clone(),
                },
            )?;
            summary.forwards_created += 1;
        }
        built.push((
            *id,
            Host {
                label: h.alias.clone(),
                hostname: h.hostname.clone(),
                port: h.port,
                identity_id,
                jump_host_id: None,
                forward_agent: h.forward_agent,
                forward_x11: h.forward_x11,
                environment: String::new(),
                startup_command: String::new(),
                group: match (h.group.as_deref(), group.trim()) {
                    (Some(sub), "") => sub.to_string(),
                    (Some(sub), base) => format!("{base}/{sub}"),
                    (None, base) => base.to_string(),
                },
                tags: Vec::new(),
                color: None,
                notes: String::new(),
                proxy_id,
                keepalive_secs: h.keepalive_secs,
                ..Default::default()
            },
        ));
    }

    // Link jumps, refusing any link that would close a loop.
    for ((id, h), (_, host)) in todo.iter().zip(built.iter_mut()) {
        let Some(alias) = &h.proxy_jump else { continue };
        let Some(&jid) = id_by_label.get(&alias.to_lowercase()) else {
            summary.warnings.push(format!(
                "{}: jump host \"{alias}\" is not saved or imported; connect directly",
                h.alias
            ));
            continue;
        };
        match jump_chain(Some(*id), Some(jid), crate::ssh::MAX_JUMPS, |x| {
            jump_of.get(&x).copied()
        }) {
            Ok(_) => {
                host.jump_host_id = Some(jid);
                jump_of.insert(*id, Some(jid));
            }
            Err(e) => summary
                .warnings
                .push(format!("{}: jump via \"{alias}\" skipped ({e})", h.alias)),
        }
    }

    for (id, host) in &built {
        vault.put(Collection::Hosts, *id, host, crate::vault::Base::New)?;
        summary.hosts_created += 1;
    }
    Ok(summary)
}

/// Copy a key file into the Key Manager. Returns `None` (with a warning)
/// when the file can't be used, so the host falls back to ssh-agent.
fn copy_key(
    vault: &crate::vault::Vault,
    path: &str,
    alias: &str,
    summary: &mut ImportSummary,
) -> Result<Option<uuid::Uuid>, crate::vault::VaultError> {
    use crate::models::SshKey;
    use crate::vault::Collection;
    let text = match std::fs::read_to_string(path) {
        Ok(t) => zeroize::Zeroizing::new(t),
        Err(e) => {
            summary
                .warnings
                .push(format!("{alias}: could not read {path} ({e}); using ssh-agent"));
            return Ok(None);
        }
    };
    let material = match crate::keys::inspect_private(&text, None) {
        Ok(m) => m,
        Err(e) => {
            summary.warnings.push(format!(
                "{alias}: {path} could not be imported ({e}); using ssh-agent. Import it in Keys instead."
            ));
            return Ok(None);
        }
    };
    // The same key imported earlier (from another path or run) is reused.
    if let Some(existing) = vault
        .list::<SshKey>(Collection::Keys)?
        .records
        .into_iter()
        .find(|r| r.data.as_ref().is_some_and(|k| k.fingerprint == material.fingerprint && k.private_key.is_some()))
    {
        return Ok(Some(existing.id));
    }
    let name = Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let rec = vault.insert(Collection::Keys, &crate::keymanager::new_key(&name, material, None))?;
    summary.keys_imported += 1;
    Ok(Some(rec.id))
}

/// Render saved hosts as an OpenSSH client config, so `ssh <alias>` works
/// from a shell with the same names. Keys stay in the vault, so no
/// `IdentityFile` lines are written; ssh-agent or the user's own keys apply.
pub fn export(
    hosts: &[(uuid::Uuid, crate::models::Host)],
    identities: &HashMap<uuid::Uuid, crate::models::Identity>,
) -> String {
    use std::fmt::Write as _;
    // Aliases must be single words and unique.
    let mut alias_of: HashMap<uuid::Uuid, String> = HashMap::new();
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (id, h) in hosts {
        let base: String = h
            .label
            .trim()
            .chars()
            .map(|c| {
                if c.is_whitespace() || c == '*' || c == '?' || c == '!' {
                    '-'
                } else {
                    c
                }
            })
            .collect();
        let base = if base.is_empty() {
            h.hostname.clone()
        } else {
            base
        };
        let mut alias = base.clone();
        let mut n = 2;
        while !used.insert(alias.to_lowercase()) {
            alias = format!("{base}-{n}");
            n += 1;
        }
        alias_of.insert(*id, alias);
    }

    let mut out = String::from(
        "# Exported from SSHVault. Private keys stay in the encrypted vault, so\n\
         # there are no IdentityFile lines: ssh uses your agent or default keys.\n",
    );
    for (id, h) in hosts {
        let _ = writeln!(out);
        if !h.group.is_empty() {
            let _ = writeln!(out, "# {}", h.group);
        }
        let _ = writeln!(out, "Host {}", alias_of[id]);
        let _ = writeln!(out, "    HostName {}", h.hostname);
        if h.port != 22 {
            let _ = writeln!(out, "    Port {}", h.port);
        }
        if let Some(user) = h
            .identity_id
            .and_then(|i| identities.get(&i))
            .map(|i| &i.username)
        {
            let _ = writeln!(out, "    User {user}");
        }
        if let Some(j) = h.jump_host_id.and_then(|j| alias_of.get(&j)) {
            let _ = writeln!(out, "    ProxyJump {j}");
        }
        if h.forward_agent {
            let _ = writeln!(out, "    ForwardAgent yes");
        }
        if h.forward_x11 {
            let _ = writeln!(out, "    ForwardX11 yes");
        }
        if let Some(k) = h.keepalive_secs {
            let _ = writeln!(out, "    ServerAliveInterval {k}");
        }
    }
    out
}

/// Default config location for the current user.
pub fn default_path(home: &Path) -> PathBuf {
    home.join(".ssh").join("config")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
# global
ServerAliveInterval 30

Host bastion
    HostName bastion.example.com
    User ops
    IdentityFile ~/.ssh/id_ops
    ForwardAgent yes

Host db1 db2
    HostName %h.internal
    ProxyJump bastion
    Port=2222

Host web
  hostname="10.0.0.5"
  user deploy

Host legacy
    ProxyJump a,b
    ProxyCommand nc %h %p

Host *.internal !db2 dev-*
    User internal-user

Match host foo
    User ignored

Host *
    User fallback
    IdentityFile %d/.ssh/id_ed25519
    Port 22
"#;

    /// `home` joined with `parts` using this platform's separator.
    fn home_path(home: &str, parts: &[&str]) -> String {
        parts
            .iter()
            .fold(std::path::PathBuf::from(home), |p, c| p.join(c))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn expands_home_with_native_separators() {
        let h = Path::new("/home/me");
        assert_eq!(
            expand_path("~/.ssh/id", h),
            home_path("/home/me", &[".ssh", "id"])
        );
        assert_eq!(
            expand_path("~\\.ssh\\id", h),
            home_path("/home/me", &[".ssh", "id"])
        );
        assert_eq!(
            expand_path("%d/.ssh/id", h),
            home_path("/home/me", &[".ssh", "id"])
        );
        assert_eq!(expand_path("~", h), "/home/me");
        assert_eq!(expand_path("/etc/ssh/key", h), "/etc/ssh/key");
    }

    fn get<'a>(r: &'a ParseResult, alias: &str) -> &'a ImportedHost {
        r.hosts.iter().find(|h| h.alias == alias).unwrap()
    }

    #[test]
    fn resolves_like_openssh() {
        let r = parse(SAMPLE, Path::new("/home/me"));
        let aliases: Vec<_> = r.hosts.iter().map(|h| h.alias.as_str()).collect();
        assert_eq!(aliases, ["bastion", "db1", "db2", "web", "legacy"]);

        let b = get(&r, "bastion");
        assert_eq!(b.hostname, "bastion.example.com");
        assert_eq!(b.user.as_deref(), Some("ops"));
        assert_eq!(
            b.identity_file.as_deref(),
            Some(home_path("/home/me", &[".ssh", "id_ops"]).as_str())
        );
        assert_eq!(b.port, 22);
        assert!(b.forward_agent);
        assert!(!get(&r, "web").forward_agent);

        let d = get(&r, "db1");
        assert_eq!(d.hostname, "db1.internal");
        assert_eq!(d.port, 2222);
        assert_eq!(d.proxy_jump.as_deref(), Some("bastion"));
        // `Host *` defaults fill what the specific block didn't set.
        assert_eq!(d.user.as_deref(), Some("fallback"));
        assert_eq!(
            d.identity_file.as_deref(),
            Some(home_path("/home/me", &[".ssh", "id_ed25519"]).as_str())
        );

        let w = get(&r, "web");
        assert_eq!(w.hostname, "10.0.0.5");
        assert_eq!(w.user.as_deref(), Some("deploy"));

        let l = get(&r, "legacy");
        assert_eq!(l.proxy_jump, None);
        assert!(r.warnings.iter().any(|w| w.contains("multi-hop")));
        assert!(r.warnings.iter().any(|w| w.contains("ProxyCommand")));
        assert!(r.warnings.iter().any(|w| w.contains("Match")));
    }

    #[test]
    fn globs_and_negation() {
        assert!(glob_match("*.internal", "db1.internal"));
        assert!(glob_match("dev-?", "dev-1"));
        assert!(!glob_match("dev-?", "dev-10"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("WEB*", "web01"));
        // `!db2` excludes db2 from the *.internal block.
        let r = parse(
            "Host *.internal !db2.internal\n  User x\nHost db1.internal db2.internal\n",
            Path::new("/h"),
        );
        assert_eq!(get(&r, "db1.internal").user.as_deref(), Some("x"));
        assert_eq!(get(&r, "db2.internal").user, None);
    }

    #[test]
    fn import_creates_hosts_identities_and_jumps() {
        use crate::models::{AuthMethod, Host, Identity};
        use crate::vault::{Collection, Vault};

        let dir = tempfile::TempDir::new().unwrap();
        let key_path = dir.path().join("id_test");
        let generated = crate::keys::generate(crate::keys::KeyAlgorithm::Ed25519, "ops@laptop", None).unwrap();
        std::fs::write(&key_path, generated.private_key.as_ref().unwrap().as_bytes()).unwrap();
        let vault = Vault::create(
            dir.path().join("v"),
            b"pw",
            crate::vault::testutil::opts(false),
            crate::vault::DeviceInfo::new("t"),
        )
        .unwrap()
        .0;
        // A pre-existing host with the same label as one alias.
        vault
            .insert(
                Collection::Hosts,
                &Host {
                    label: "web".into(),
                    hostname: "old".into(),
                    port: 22,
                    identity_id: None,
                    jump_host_id: None,
                    forward_agent: false,
                    forward_x11: false,
                    environment: String::new(),
                    startup_command: String::new(),
                    group: String::new(),
                    tags: vec![],
                    color: None,
                    notes: String::new(),
                    ..Default::default()
                },
            )
            .unwrap();

        let config = format!(
            "Host bastion\n  User ops\n  IdentityFile {}\nHost db\n  ProxyJump bastion\n  User ops\n  IdentityFile {}\n\
             Host web\n  ProxyJump db\nHost lonely\n  ProxyJump ghost\nHost missingkey\n  IdentityFile /nope/key\n",
            key_path.display(),
            key_path.display()
        );
        let parsed = parse(&config, dir.path());
        let s = import_into_vault(&vault, &parsed.hosts, "Imported", Some("me"), KeyImport::Copy).unwrap();

        assert_eq!(s.skipped_existing, ["web"]);
        assert_eq!(s.keys_imported, 1, "one key file shared by two hosts");
        assert_eq!(s.hosts_created, 4); // bastion, db, lonely, missingkey
                                        // ops+key shared by bastion and db; "me" via agent for lonely; "me" fallback for missingkey.
        assert_eq!(s.identities_created, 2);
        assert!(s.warnings.iter().any(|w| w.contains("ghost")));
        assert!(s.warnings.iter().any(|w| w.contains("/nope/key")));

        let hosts = vault.list::<Host>(Collection::Hosts).unwrap().records;
        let by = |l: &str| {
            hosts
                .iter()
                .find(|h| h.data.as_ref().unwrap().label == l)
                .unwrap()
        };
        assert_eq!(
            by("db").data.as_ref().unwrap().jump_host_id,
            Some(by("bastion").id)
        );
        assert_eq!(by("bastion").data.as_ref().unwrap().group, "Imported");
        assert_eq!(by("lonely").data.as_ref().unwrap().jump_host_id, None);

        let ids = vault
            .list::<Identity>(Collection::Identities)
            .unwrap()
            .records;
        let ops = ids
            .iter()
            .find(|i| i.data.as_ref().unwrap().username == "ops")
            .unwrap();
        let AuthMethod::Key { key_id } = ops.data.as_ref().unwrap().auth else {
            panic!("expected a Key Manager reference");
        };
        let key = vault.get::<crate::models::SshKey>(Collection::Keys, key_id).unwrap().data.unwrap();
        assert_eq!(key.fingerprint, generated.fingerprint);
        let me = ids
            .iter()
            .find(|i| i.data.as_ref().unwrap().username == "me")
            .unwrap();
        assert_eq!(me.data.as_ref().unwrap().auth, AuthMethod::Agent);

        // Importing again creates nothing new.
        let again = import_into_vault(&vault, &parsed.hosts, "Imported", Some("me"), KeyImport::Copy).unwrap();
        assert_eq!(again.hosts_created, 0);
    }

    #[test]
    fn proxy_commands_forwards_and_key_references() {
        use crate::models::{AuthMethod, ForwardKind, ForwardRule, Host, Identity, Proxy, ProxySpec};
        use crate::vault::Collection;
        let config = "Host app\n  HostName app.internal\n  User deploy\n  IdentityFile ~/.ssh/id_app\n\
            ProxyCommand ssh -W %h:%p gw\n  ServerAliveInterval 15\n\
            LocalForward 5432 db.internal:5432\n  LocalForward 0.0.0.0:8080 [::1]:80\n\
            RemoteForward 9000 localhost:3000\n  DynamicForward 1080\n  RemoteForward 7000\n\
            Host both\n  ProxyJump app\n  ProxyCommand nc %h %p\n";
        let r = parse(config, Path::new("/home/me"));
        let app = get(&r, "app");
        assert_eq!(app.proxy_command.as_deref(), Some("ssh -W %h:%p gw"));
        assert_eq!(app.keepalive_secs, Some(15));
        assert_eq!(
            app.forwards,
            vec![
                ForwardKind::Local { bind_addr: "127.0.0.1".into(), bind_port: 5432, dest_host: "db.internal".into(), dest_port: 5432 },
                ForwardKind::Local { bind_addr: "0.0.0.0".into(), bind_port: 8080, dest_host: "::1".into(), dest_port: 80 },
                ForwardKind::Remote { bind_addr: "127.0.0.1".into(), bind_port: 9000, dest_host: "localhost".into(), dest_port: 3000 },
                ForwardKind::Dynamic { bind_addr: "127.0.0.1".into(), bind_port: 1080 },
            ]
        );
        assert!(r.warnings.iter().any(|w| w.contains("remote dynamic")));
        assert!(get(&r, "both").proxy_command.is_none(), "ProxyJump wins");

        let (_d, vault) = crate::vault::testutil::new_vault();
        let s = import_into_vault(&vault, &r.hosts, "", None, KeyImport::Reference).unwrap();
        assert_eq!((s.proxies_created, s.forwards_created, s.keys_imported), (1, 4, 0));
        // The key stays a path reference; nothing secret was copied.
        let ident = vault.list::<Identity>(Collection::Identities).unwrap().records;
        assert!(matches!(&ident[0].data.as_ref().unwrap().auth, AuthMethod::KeyFile { path, .. } if path.ends_with("id_app")));
        // The ProxyCommand is stored but NOT approved to run.
        let proxies = vault.list::<Proxy>(Collection::Proxies).unwrap().records;
        assert!(matches!(proxies[0].data.as_ref().unwrap().spec, ProxySpec::Command { approved: false, .. }));
        let hosts = vault.list::<Host>(Collection::Hosts).unwrap().records;
        let app = hosts.iter().find(|h| h.data.as_ref().unwrap().label == "app").unwrap();
        assert_eq!(app.data.as_ref().unwrap().proxy_id, Some(proxies[0].id));
        assert_eq!(app.data.as_ref().unwrap().keepalive_secs, Some(15));
        let rules = vault.list::<ForwardRule>(Collection::Forwards).unwrap().records;
        assert!(rules.iter().all(|r| r.data.as_ref().is_some_and(|f| !f.auto_start && f.host_id == app.id)));
    }

    #[test]
    fn import_refuses_jump_loops() {
        use crate::models::Host;
        use crate::vault::{Collection, Vault};
        let dir = tempfile::TempDir::new().unwrap();
        let vault = Vault::create(
            dir.path().join("v"),
            b"pw",
            crate::vault::testutil::opts(false),
            crate::vault::DeviceInfo::new("t"),
        )
        .unwrap()
        .0;
        let parsed = parse("Host a\n ProxyJump b\nHost b\n ProxyJump a\n", dir.path());
        let s = import_into_vault(&vault, &parsed.hosts, "", None, KeyImport::Reference).unwrap();
        assert_eq!(s.hosts_created, 2);
        assert!(
            s.warnings.iter().any(|w| w.contains("loops")),
            "{:?}",
            s.warnings
        );
        let hosts = vault.list::<Host>(Collection::Hosts).unwrap().records;
        assert_eq!(
            hosts
                .iter()
                .filter(|h| h.data.as_ref().unwrap().jump_host_id.is_some())
                .count(),
            1
        );
    }

    #[test]
    fn export_round_trips_through_the_parser() {
        use crate::models::{AuthMethod, Host, Identity};
        let ident = uuid::Uuid::new_v4();
        let (a, b, c) = (
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
            uuid::Uuid::new_v4(),
        );
        let host = |label: &str, hostname: &str, port: u16| Host {
            label: label.into(),
            hostname: hostname.into(),
            port,
            identity_id: Some(ident),
            jump_host_id: None,
            forward_agent: false,
            forward_x11: false,
            environment: String::new(),
            startup_command: String::new(),
            group: "Prod".into(),
            tags: vec![],
            color: None,
            notes: String::new(),
            ..Default::default()
        };
        let mut web = host("web server", "10.0.0.5", 2222);
        web.jump_host_id = Some(a);
        web.forward_agent = true;
        let hosts = vec![
            (a, host("bastion", "bastion.example.com", 22)),
            (b, web),
            (c, host("bastion", "other.example.com", 22)), // duplicate label
        ];
        let ids = HashMap::from([(
            ident,
            Identity {
                label: "ops".into(),
                username: "ops".into(),
                auth: AuthMethod::Agent,
                notes: String::new(),
                for_host: None,
            },
        )]);
        let text = export(&hosts, &ids);
        // Keys never leave the vault: no IdentityFile settings (the header
        // comment mentions the word, so check lines that set it).
        assert!(!text
            .lines()
            .any(|l| l.trim_start().starts_with("IdentityFile")));
        let r = parse(&text, Path::new("/h"));
        assert_eq!(r.hosts.len(), 3);
        let w = get(&r, "web-server");
        assert_eq!((w.hostname.as_str(), w.port), ("10.0.0.5", 2222));
        assert_eq!(w.user.as_deref(), Some("ops"));
        assert_eq!(w.proxy_jump.as_deref(), Some("bastion"));
        assert!(w.forward_agent);
        assert_eq!(get(&r, "bastion-2").hostname, "other.example.com");
    }

    #[test]
    fn junk_is_tolerated() {
        let r = parse(
            "Host a\n  Port nope\n  ProxyJump user@b:2200\n\n   \n",
            Path::new("/h"),
        );
        let a = get(&r, "a");
        assert_eq!(a.port, 22);
        assert_eq!(a.proxy_jump.as_deref(), Some("b"));
        assert!(r.warnings.iter().any(|w| w.contains("invalid Port")));
        assert_eq!(parse("", Path::new("/h")), ParseResult::default());
    }
}
