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

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One importable host, fully resolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedHost {
    pub alias: String,
    pub hostname: String,
    pub port: u16,
    pub user: Option<String>,
    /// Absolute path after `~` / `%d` expansion.
    pub identity_file: Option<String>,
    /// Alias of a single-hop ProxyJump, if any.
    pub proxy_jump: Option<String>,
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
fn expand_path(p: &str, home: &Path) -> String {
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
        if get("proxycommand").is_some() {
            result.warnings.push(format!(
                "{alias}: ProxyCommand is not supported and was ignored"
            ));
        }

        result.hosts.push(ImportedHost {
            alias: alias.clone(),
            hostname,
            port,
            user: get("user"),
            identity_file: get("identityfile").map(|p| expand_path(&p, home)),
            proxy_jump,
        });
    }
    result
}

/// What an import did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ImportSummary {
    pub hosts_created: usize,
    pub identities_created: usize,
    pub skipped_existing: Vec<String>,
    pub warnings: Vec<String>,
}

/// Create hosts (and the identities they need) in `vault`.
///
/// * Hosts whose alias matches an existing host label are skipped.
/// * One identity per distinct (user, IdentityFile) pair. The key file's
///   contents are copied into the encrypted vault. Without an IdentityFile
///   the identity uses ssh-agent.
/// * A ProxyJump alias is linked to the imported or already-saved host with
///   that label, unless doing so would create a loop.
pub fn import_into_vault(
    vault: &crate::vault::Vault,
    hosts: &[ImportedHost],
    group: &str,
    local_user: Option<&str>,
) -> Result<ImportSummary, crate::vault::VaultError> {
    use crate::models::{jump_chain, AuthMethod, Host, Identity};
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
    let mut built: Vec<(Uuid, Host)> = Vec::new();
    for (id, h) in &todo {
        let user = h.user.clone().or_else(|| local_user.map(str::to_string));
        // Read the key first: an unreadable file falls back to ssh-agent and
        // shares the agent identity for that user instead of making another.
        let key_file: Option<(String, String)> = match &h.identity_file {
            None => None,
            Some(path) => match std::fs::read_to_string(path) {
                Ok(pem) => Some((path.clone(), pem)),
                Err(e) => {
                    summary.warnings.push(format!(
                        "{}: could not read {path} ({e}); using ssh-agent",
                        h.alias
                    ));
                    None
                }
            },
        };
        let identity_id = match &user {
            None => None,
            Some(user) => {
                let key = (user.clone(), key_file.as_ref().map(|(p, _)| p.clone()));
                if let Some(existing) = identities.get(&key) {
                    Some(*existing)
                } else {
                    let (label, auth) = match key_file {
                        Some((path, pem)) => {
                            let file = Path::new(&path)
                                .file_name()
                                .map(|f| f.to_string_lossy().into_owned())
                                .unwrap_or(path);
                            (
                                format!("{user} · {file}"),
                                AuthMethod::PrivateKey {
                                    private_key: pem,
                                    passphrase: None,
                                },
                            )
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
        built.push((
            *id,
            Host {
                label: h.alias.clone(),
                hostname: h.hostname.clone(),
                port: h.port,
                identity_id,
                jump_host_id: None,
                group: group.to_string(),
                tags: Vec::new(),
                color: None,
                notes: String::new(),
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
        vault.put(Collection::Hosts, *id, host)?;
        summary.hosts_created += 1;
    }
    Ok(summary)
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
        use crate::crypto::KdfParams;
        use crate::models::{AuthMethod, Host, Identity};
        use crate::vault::{Collection, Vault};

        let dir = tempfile::TempDir::new().unwrap();
        let key_path = dir.path().join("id_test");
        std::fs::write(&key_path, "PEM-CONTENTS").unwrap();
        let vault =
            Vault::create(dir.path().join("v"), b"pw", KdfParams::insecure_for_tests()).unwrap();
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
                    group: String::new(),
                    tags: vec![],
                    color: None,
                    notes: String::new(),
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
        let s = import_into_vault(&vault, &parsed.hosts, "Imported", Some("me")).unwrap();

        assert_eq!(s.skipped_existing, ["web"]);
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
        assert!(
            matches!(&ops.data.as_ref().unwrap().auth, AuthMethod::PrivateKey { private_key, .. } if private_key == "PEM-CONTENTS")
        );
        let me = ids
            .iter()
            .find(|i| i.data.as_ref().unwrap().username == "me")
            .unwrap();
        assert_eq!(me.data.as_ref().unwrap().auth, AuthMethod::Agent);

        // Importing again creates nothing new.
        let again = import_into_vault(&vault, &parsed.hosts, "Imported", Some("me")).unwrap();
        assert_eq!(again.hosts_created, 0);
    }

    #[test]
    fn import_refuses_jump_loops() {
        use crate::crypto::KdfParams;
        use crate::models::Host;
        use crate::vault::{Collection, Vault};
        let dir = tempfile::TempDir::new().unwrap();
        let vault =
            Vault::create(dir.path().join("v"), b"pw", KdfParams::insecure_for_tests()).unwrap();
        let parsed = parse("Host a\n ProxyJump b\nHost b\n ProxyJump a\n", dir.path());
        let s = import_into_vault(&vault, &parsed.hosts, "", None).unwrap();
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
