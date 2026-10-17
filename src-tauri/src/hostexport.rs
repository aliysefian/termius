//! Moving hosts between SSHVault vaults in one encrypted file.
//!
//! An export holds the hosts with their group paths and group defaults, the identities (user names and, if asked,
//! passwords), the keys and proxies they use, and the jump-host links between them. The whole bundle is encrypted
//! with a passphrase chosen for the file (Argon2id, then XChaCha20-Poly1305, like the vault itself), so the file can
//! travel by mail or chat without exposing any of it.
//!
//! Importing never overwrites anything. Ids are made new and every link between records is rewritten to point at
//! the new ids. A host whose name is already used in the same group is a conflict, and the person decides per host:
//! keep both under another name, or skip it (other hosts that jump through a skipped one use the one already there).

use std::collections::{HashMap, HashSet};

use base64::{engine::general_purpose::STANDARD as B64, Engine};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::crypto::{self, KdfParams};
use crate::models::{jump_chain, AuthMethod, Host, HostGroup, Identity, Proxy, ProxySpec, SshKey};
use crate::vault::{Base, Collection, Vault, VaultError};

pub const FORMAT: &str = "sshvault-hosts";
pub const VERSION: u32 = 1;
/// Authenticated with the data, so a host export can't be passed off as anything else.
const AAD: &[u8] = b"sshvault-hosts/v1";
/// The biggest file read for an import.
pub const MAX_FILE_BYTES: u64 = 64 << 20;
pub const MIN_PASSPHRASE_CHARS: usize = 8;
/// The extension an export is saved with.
pub const EXTENSION: &str = "svhosts";

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("This file isn't an SSHVault host export.")]
    NotAnExport,
    #[error("This export was made by a newer SSHVault (file version {0}). Update the app to open it.")]
    Newer(u32),
    #[error("Wrong passphrase, or the file is damaged.")]
    WrongPassphrase,
    #[error("Use a passphrase of at least {MIN_PASSPHRASE_CHARS} characters.")]
    WeakPassphrase,
    #[error("{0}")]
    Name(String),
    #[error("{0}")]
    Other(String),
    #[error(transparent)]
    Vault(#[from] VaultError),
}

// -- the bundle ------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item<T> {
    /// The id in the vault it came from: only there to say which records link to which.
    pub id: Uuid,
    pub data: T,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bundle {
    #[serde(default)]
    pub exported_at: u64,
    /// Whether passwords, private keys and passphrases are in it.
    #[serde(default)]
    pub secrets: bool,
    #[serde(default)]
    pub hosts: Vec<Item<Host>>,
    #[serde(default)]
    pub identities: Vec<Item<Identity>>,
    #[serde(default)]
    pub keys: Vec<Item<SshKey>>,
    #[serde(default)]
    pub proxies: Vec<Item<Proxy>>,
    #[serde(default)]
    pub groups: Vec<HostGroup>,
}

#[derive(Serialize, Deserialize)]
struct Sealed {
    format: String,
    version: u32,
    kdf: KdfParams,
    salt: String,
    data: String,
}

fn check_passphrase(p: &str) -> Result<(), ExportError> {
    if p.chars().count() < MIN_PASSPHRASE_CHARS {
        return Err(ExportError::WeakPassphrase);
    }
    Ok(())
}

/// The bundle as the text of an export file, encrypted under `passphrase`.
pub fn seal(bundle: &Bundle, passphrase: &str, kdf: KdfParams) -> Result<String, ExportError> {
    check_passphrase(passphrase)?;
    let plain = Zeroizing::new(serde_json::to_vec(bundle).map_err(|e| ExportError::Other(e.to_string()))?);
    let salt = crypto::generate_salt();
    let key = crypto::derive_key(passphrase.as_bytes(), &salt, kdf).map_err(|e| ExportError::Other(e.to_string()))?;
    let data = crypto::encrypt(&key, &plain, AAD).map_err(|e| ExportError::Other(e.to_string()))?;
    let sealed = Sealed { format: FORMAT.into(), version: VERSION, kdf, salt: B64.encode(salt), data: B64.encode(data) };
    serde_json::to_string_pretty(&sealed).map_err(|e| ExportError::Other(e.to_string()))
}

/// What is in an export file, if `passphrase` is the one it was made with.
pub fn open(text: &str, passphrase: &str) -> Result<Bundle, ExportError> {
    let sealed: Sealed = serde_json::from_str(text).map_err(|_| ExportError::NotAnExport)?;
    if sealed.format != FORMAT {
        return Err(ExportError::NotAnExport);
    }
    if sealed.version > VERSION {
        return Err(ExportError::Newer(sealed.version));
    }
    let salt = B64.decode(&sealed.salt).map_err(|_| ExportError::NotAnExport)?;
    let data = B64.decode(&sealed.data).map_err(|_| ExportError::NotAnExport)?;
    // The cost parameters come from the file; refuse ones that would hang or exhaust this computer.
    if sealed.kdf.m_cost_kib > 4 * 1024 * 1024 || sealed.kdf.t_cost > 64 || sealed.kdf.p_cost > 64 || sealed.kdf.m_cost_kib == 0 {
        return Err(ExportError::NotAnExport);
    }
    let key = crypto::derive_key(passphrase.as_bytes(), &salt, sealed.kdf).map_err(|_| ExportError::NotAnExport)?;
    let plain = Zeroizing::new(crypto::decrypt(&key, &data, AAD).map_err(|_| ExportError::WrongPassphrase)?);
    serde_json::from_slice(&plain).map_err(|_| ExportError::NotAnExport)
}

// -- exporting -----------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExportOptions {
    /// Passwords, private keys and their passphrases go in the file. Without it only user names, key public
    /// parts and structure do.
    #[serde(default)]
    pub include_secrets: bool,
    /// Only this group and the ones under it, with the jump hosts they need. Everything when `None`.
    #[serde(default)]
    pub group: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ExportSummary {
    pub hosts: usize,
    pub groups: usize,
    pub identities: usize,
    pub keys: usize,
    pub proxies: usize,
    pub secrets: bool,
}

fn in_group(path: &str, group: &str) -> bool {
    let (path, group) = (path.trim_matches('/'), group.trim_matches('/'));
    path == group || path.starts_with(&format!("{group}/"))
}

/// `a/b/c` → `a`, `a/b`, `a/b/c`.
fn with_ancestors(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for part in path.split('/').filter(|p| !p.is_empty()) {
        if !cur.is_empty() {
            cur.push('/');
        }
        cur.push_str(part);
        out.push(cur.clone());
    }
    out
}

fn strip_identity(i: &mut Identity) {
    match &mut i.auth {
        AuthMethod::Password { password } => password.clear(),
        AuthMethod::PrivateKey { private_key, passphrase, .. } => {
            private_key.clear();
            *passphrase = None;
        }
        AuthMethod::KeyFile { passphrase, .. } => *passphrase = None,
        AuthMethod::Key { .. } | AuthMethod::Agent => {}
    }
}

fn strip_key(k: &mut SshKey) {
    k.private_key = None;
    k.passphrase = None;
}

fn strip_proxy(p: &mut Proxy) {
    if let ProxySpec::Socks5 { password, .. } | ProxySpec::Http { password, .. } = &mut p.spec {
        *password = None;
    }
}

pub fn build(vault: &Vault, opts: &ExportOptions) -> Result<Bundle, ExportError> {
    let all_hosts: Vec<(Uuid, Host)> = vault.list::<Host>(Collection::Hosts)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
    let all_groups: Vec<HostGroup> = vault.list::<HostGroup>(Collection::Groups)?.records.into_iter().filter_map(|r| r.data).collect();
    let by_id: HashMap<Uuid, &Host> = all_hosts.iter().map(|(i, h)| (*i, h)).collect();

    let mut chosen: Vec<Uuid> = all_hosts.iter().filter(|(_, h)| opts.group.as_deref().is_none_or(|g| in_group(&h.group, g))).map(|(i, _)| *i).collect();
    let mut group_paths: HashSet<String> = HashSet::new();
    match opts.group.as_deref() {
        // Everything: every group, even an empty one, is part of the structure.
        None => group_paths.extend(all_groups.iter().map(|g| g.path.clone())),
        Some(g) => {
            group_paths.extend(all_groups.iter().filter(|x| in_group(&x.path, g)).map(|x| x.path.clone()));
            group_paths.extend(with_ancestors(g));
        }
    }
    for id in &chosen {
        group_paths.extend(with_ancestors(&by_id[id].group));
    }
    let groups: Vec<HostGroup> = all_groups.into_iter().filter(|g| group_paths.contains(&g.path)).collect();

    // Jump hosts the chosen hosts (and the groups' defaults) go through come along, whatever group they are in.
    let mut seen: HashSet<Uuid> = chosen.iter().copied().collect();
    let mut queue: Vec<Uuid> = chosen.iter().filter_map(|id| by_id[id].jump_host_id).chain(groups.iter().filter_map(|g| g.default_jump_host_id)).collect();
    while let Some(id) = queue.pop() {
        if !seen.insert(id) {
            continue;
        }
        if let Some(h) = by_id.get(&id) {
            chosen.push(id);
            queue.extend(h.jump_host_id);
        }
    }
    let hosts: Vec<Item<Host>> = chosen.iter().map(|id| Item { id: *id, data: by_id[id].clone() }).collect();

    let all_identities: HashMap<Uuid, Identity> = vault.list::<Identity>(Collection::Identities)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
    let all_keys: HashMap<Uuid, SshKey> = vault.list::<SshKey>(Collection::Keys)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
    let all_proxies: HashMap<Uuid, Proxy> = vault.list::<Proxy>(Collection::Proxies)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();

    let mut identity_ids: Vec<Uuid> = Vec::new();
    let mut proxy_ids: Vec<Uuid> = Vec::new();
    let note = |ids: &mut Vec<Uuid>, id: Option<Uuid>| {
        if let Some(id) = id {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    };
    for h in &hosts {
        note(&mut identity_ids, h.data.identity_id);
        note(&mut proxy_ids, h.data.proxy_id);
    }
    for g in &groups {
        note(&mut identity_ids, g.default_identity_id);
        note(&mut proxy_ids, g.proxy_id);
    }
    let mut identities = Vec::new();
    let mut key_ids: Vec<Uuid> = Vec::new();
    for id in identity_ids {
        let Some(mut data) = all_identities.get(&id).cloned() else { continue };
        if let AuthMethod::Key { key_id } = data.auth {
            note(&mut key_ids, Some(key_id));
        }
        if !opts.include_secrets {
            strip_identity(&mut data);
        }
        identities.push(Item { id, data });
    }
    let mut keys: Vec<Item<SshKey>> = key_ids.into_iter().filter_map(|id| all_keys.get(&id).cloned().map(|data| Item { id, data })).collect();
    let mut proxies: Vec<Item<Proxy>> = proxy_ids.into_iter().filter_map(|id| all_proxies.get(&id).cloned().map(|data| Item { id, data })).collect();
    if !opts.include_secrets {
        keys.iter_mut().for_each(|k| strip_key(&mut k.data));
        proxies.iter_mut().for_each(|p| strip_proxy(&mut p.data));
    }

    Ok(Bundle { exported_at: now(), secrets: opts.include_secrets, hosts, identities, keys, proxies, groups })
}

pub fn summary(b: &Bundle) -> ExportSummary {
    ExportSummary { hosts: b.hosts.len(), groups: b.groups.len(), identities: b.identities.len(), keys: b.keys.len(), proxies: b.proxies.len(), secrets: b.secrets }
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// -- importing -----------------------------------------------------------------------------------------------

/// A host in the file whose name is already used in the same group here.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Conflict {
    /// The host's id in the file; resolutions name it.
    pub id: Uuid,
    pub label: String,
    pub group: String,
    /// A free name to keep it under.
    pub suggested: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportPreview {
    pub exported_at: u64,
    pub hosts: usize,
    pub groups: usize,
    pub identities: usize,
    pub keys: usize,
    pub proxies: usize,
    pub secrets: bool,
    pub conflicts: Vec<Conflict>,
    /// Things to know before importing, such as hosts that run a command on this computer.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    /// Import it under this name.
    Rename { label: String },
    /// Leave it out; hosts that jump through it use the one already here.
    Skip,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Resolution {
    pub id: Uuid,
    #[serde(flatten)]
    pub action: Action,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct ImportResult {
    pub hosts_added: usize,
    pub hosts_renamed: usize,
    pub hosts_skipped: usize,
    pub groups_added: usize,
    pub identities_added: usize,
    pub keys_added: usize,
    pub proxies_added: usize,
    pub warnings: Vec<String>,
}

/// Case-insensitive, so `Web` and `web` in one group count as the same name.
fn name_key(label: &str, group: &str) -> (String, String) {
    (label.trim().to_lowercase(), group.trim_matches('/').to_lowercase())
}

/// `name (2)`, `name (3)`, …: the first that is free in the group.
fn free_name(label: &str, group: &str, taken: &HashSet<(String, String)>) -> String {
    (2..10_000).map(|n| format!("{label} ({n})")).find(|c| !taken.contains(&name_key(c, group))).unwrap_or_else(|| format!("{label} ({})", Uuid::new_v4()))
}

fn existing_hosts(vault: &Vault) -> Result<Vec<(Uuid, Host)>, ExportError> {
    Ok(vault.list::<Host>(Collection::Hosts)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect())
}

fn runs_here(h: &Host) -> bool {
    !h.hook_before.trim().is_empty() || !h.hook_after.trim().is_empty() || (h.protocol == "command" && !h.connect_command.trim().is_empty())
}

pub fn preview(vault: &Vault, b: &Bundle) -> Result<ImportPreview, ExportError> {
    let mut taken: HashSet<(String, String)> = existing_hosts(vault)?.iter().map(|(_, h)| name_key(&h.label, &h.group)).collect();
    let mut conflicts = Vec::new();
    for h in &b.hosts {
        let key = name_key(&h.data.label, &h.data.group);
        if taken.contains(&key) {
            conflicts.push(Conflict { id: h.id, label: h.data.label.clone(), group: h.data.group.clone(), suggested: String::new() });
        } else {
            taken.insert(key);
        }
    }
    // Suggestions are free against the vault and everything else in the file, and against each other.
    for c in &mut conflicts {
        c.suggested = free_name(&c.label, &c.group, &taken);
        taken.insert(name_key(&c.suggested, &c.group));
    }
    let mut notes = Vec::new();
    let runs = b.hosts.iter().filter(|h| runs_here(&h.data)).count();
    if runs > 0 {
        notes.push(format!("{runs} host{} run a command on this computer when you connect. You are asked to approve each one the first time.", if runs == 1 { "" } else { "s" }));
    }
    let proxy_cmds = b.proxies.iter().filter(|p| matches!(p.data.spec, ProxySpec::Command { .. })).count();
    if proxy_cmds > 0 {
        notes.push(format!("{proxy_cmds} proxy command{} come in switched off until you approve {}.", if proxy_cmds == 1 { "" } else { "s" }, if proxy_cmds == 1 { "it" } else { "them" }));
    }
    if !b.secrets && (!b.identities.is_empty() || !b.keys.is_empty()) {
        notes.push("This export has no saved passwords or private keys, so those hosts ask for them when you connect.".into());
    }
    Ok(ImportPreview {
        exported_at: b.exported_at,
        hosts: b.hosts.len(),
        groups: b.groups.len(),
        identities: b.identities.len(),
        keys: b.keys.len(),
        proxies: b.proxies.len(),
        secrets: b.secrets,
        conflicts,
        notes,
    })
}

fn same_auth(a: &AuthMethod, b: &AuthMethod) -> bool {
    a == b
}

/// Add the bundle to the vault. Nothing existing is changed or removed.
pub fn apply(vault: &Vault, b: &Bundle, resolutions: &[Resolution]) -> Result<ImportResult, ExportError> {
    let mut result = ImportResult::default();
    let existing = existing_hosts(vault)?;
    let mut taken: HashSet<(String, String)> = existing.iter().map(|(_, h)| name_key(&h.label, &h.group)).collect();
    let existing_by_name: HashMap<(String, String), Uuid> = existing.iter().map(|(i, h)| (name_key(&h.label, &h.group), *i)).collect();
    let decided: HashMap<Uuid, &Action> = resolutions.iter().map(|r| (r.id, &r.action)).collect();

    // 1. Decide every host: its new id and name, or the existing host it stands in for.
    struct Plan<'a> {
        id: Uuid,
        host: &'a Host,
        label: String,
    }
    let mut host_map: HashMap<Uuid, Uuid> = HashMap::new();
    let mut adding: Vec<Plan> = Vec::new();
    for item in &b.hosts {
        let h = &item.data;
        if h.label.trim().is_empty() || h.hostname.trim().is_empty() {
            result.warnings.push("A host with no name or address was left out.".into());
            result.hosts_skipped += 1;
            continue;
        }
        let key = name_key(&h.label, &h.group);
        let label = if taken.contains(&key) {
            match decided.get(&item.id) {
                Some(Action::Skip) => {
                    if let Some(existing_id) = existing_by_name.get(&key) {
                        host_map.insert(item.id, *existing_id);
                    }
                    result.hosts_skipped += 1;
                    continue;
                }
                Some(Action::Rename { label }) => {
                    let label = label.trim().to_string();
                    if label.is_empty() {
                        return Err(ExportError::Name(format!("Give “{}” a name.", h.label)));
                    }
                    if taken.contains(&name_key(&label, &h.group)) {
                        return Err(ExportError::Name(format!("“{label}” is already used in {}. Pick another name.", if h.group.is_empty() { "the top level".to_string() } else { format!("“{}”", h.group) })));
                    }
                    result.hosts_renamed += 1;
                    label
                }
                // Not asked about (the file changed, or the caller skipped a step): never overwrite, keep both.
                None => {
                    result.hosts_renamed += 1;
                    free_name(&h.label, &h.group, &taken)
                }
            }
        } else {
            h.label.clone()
        };
        taken.insert(name_key(&label, &h.group));
        let id = Uuid::new_v4();
        host_map.insert(item.id, id);
        adding.push(Plan { id, host: h, label });
    }
    let added_ids: HashSet<Uuid> = adding.iter().map(|p| p.id).collect();

    // 2. Groups: a group that exists here keeps its own defaults; a missing one comes with the file's.
    let live_groups: HashSet<String> = vault.list::<HostGroup>(Collection::Groups)?.records.into_iter().filter_map(|r| r.data.map(|g| g.path.trim_matches('/').to_lowercase())).collect();
    let groups_to_add: Vec<&HostGroup> = b.groups.iter().filter(|g| !live_groups.contains(&g.path.trim_matches('/').to_lowercase())).collect();

    // 3. What the new hosts and groups use.
    let mut want_identities: Vec<Uuid> = Vec::new();
    let mut want_proxies: Vec<Uuid> = Vec::new();
    let want = |ids: &mut Vec<Uuid>, id: Option<Uuid>| {
        if let Some(id) = id {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
    };
    for p in &adding {
        want(&mut want_identities, p.host.identity_id);
        want(&mut want_proxies, p.host.proxy_id);
    }
    for g in &groups_to_add {
        want(&mut want_identities, g.default_identity_id);
        want(&mut want_proxies, g.proxy_id);
    }

    // Keys: reuse one with the same fingerprint, else add.
    let existing_keys: Vec<(Uuid, SshKey)> = vault.list::<SshKey>(Collection::Keys)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
    let key_by_id: HashMap<Uuid, &SshKey> = b.keys.iter().map(|k| (k.id, &k.data)).collect();
    let identity_by_id: HashMap<Uuid, &Identity> = b.identities.iter().map(|i| (i.id, &i.data)).collect();
    let mut key_map: HashMap<Uuid, Uuid> = HashMap::new();
    let mut key_writes: Vec<(Uuid, SshKey)> = Vec::new();
    for id in &want_identities {
        if let Some(Identity { auth: AuthMethod::Key { key_id }, .. }) = identity_by_id.get(id).copied() {
            if key_map.contains_key(key_id) {
                continue;
            }
            let Some(k) = key_by_id.get(key_id) else { continue };
            if let Some((eid, _)) = existing_keys.iter().find(|(_, e)| !k.fingerprint.is_empty() && e.fingerprint == k.fingerprint) {
                key_map.insert(*key_id, *eid);
            } else {
                let nid = Uuid::new_v4();
                let mut k = (*k).clone();
                // A key made in a host's own form belongs to that host; it follows the host, or becomes the person's.
                k.for_host = k.for_host.and_then(|h| host_map.get(&h).copied()).filter(|h| added_ids.contains(h));
                key_map.insert(*key_id, nid);
                key_writes.push((nid, k));
            }
        }
    }

    // Identities: a shared one identical to one here is reused; one a host owns goes with that host.
    let existing_identities: Vec<(Uuid, Identity)> = vault.list::<Identity>(Collection::Identities)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
    let mut identity_map: HashMap<Uuid, Uuid> = HashMap::new();
    let mut identity_writes: Vec<(Uuid, Identity)> = Vec::new();
    for id in &want_identities {
        let Some(src) = identity_by_id.get(id) else {
            result.warnings.push("A host pointed at a login that isn't in the file; it will ask for one.".into());
            continue;
        };
        let mut data = (*src).clone();
        if let AuthMethod::Key { key_id } = &mut data.auth {
            match key_map.get(key_id) {
                Some(n) => *key_id = *n,
                None => {
                    result.warnings.push(format!("The login “{}” used a key that isn't in the file; it will ask for one.", data.label));
                    data.auth = AuthMethod::Agent;
                }
            }
        }
        match data.for_host {
            Some(owner) => match host_map.get(&owner).copied().filter(|h| added_ids.contains(h)) {
                Some(new_owner) => data.for_host = Some(new_owner),
                None => continue, // its host wasn't imported
            },
            None => {
                if let Some((eid, _)) = existing_identities.iter().find(|(_, e)| e.for_host.is_none() && e.label == data.label && e.username == data.username && same_auth(&e.auth, &data.auth)) {
                    identity_map.insert(*id, *eid);
                    continue;
                }
            }
        }
        let nid = Uuid::new_v4();
        identity_map.insert(*id, nid);
        identity_writes.push((nid, data));
    }

    // Proxies: identical ones are reused. A command never arrives approved.
    let existing_proxies: Vec<(Uuid, Proxy)> = vault.list::<Proxy>(Collection::Proxies)?.records.into_iter().filter_map(|r| r.data.map(|d| (r.id, d))).collect();
    let proxy_by_id: HashMap<Uuid, &Proxy> = b.proxies.iter().map(|p| (p.id, &p.data)).collect();
    let mut proxy_map: HashMap<Uuid, Uuid> = HashMap::new();
    let mut proxy_writes: Vec<(Uuid, Proxy)> = Vec::new();
    for id in &want_proxies {
        let Some(src) = proxy_by_id.get(id) else { continue };
        let mut p = (*src).clone();
        if let ProxySpec::Command { approved, .. } = &mut p.spec {
            *approved = false;
        }
        if let Some((eid, _)) = existing_proxies.iter().find(|(_, e)| e.name == p.name && e.spec == p.spec) {
            proxy_map.insert(*id, *eid);
        } else {
            let nid = Uuid::new_v4();
            proxy_map.insert(*id, nid);
            proxy_writes.push((nid, p));
        }
    }

    // 4. Rewrite the links of hosts and groups to the new ids, then check the jump chains.
    let known_hosts: HashMap<Uuid, Option<Uuid>> = existing.iter().map(|(i, h)| (*i, h.jump_host_id)).collect();
    let mut jumps = known_hosts.clone();
    let mut built: Vec<(Uuid, Host)> = Vec::new();
    for p in &adding {
        let mut h = p.host.clone();
        h.label = p.label.clone();
        h.identity_id = h.identity_id.and_then(|i| identity_map.get(&i).copied());
        h.proxy_id = h.proxy_id.and_then(|i| proxy_map.get(&i).copied());
        h.jump_host_id = match h.jump_host_id {
            None => None,
            Some(j) => match host_map.get(&j) {
                Some(n) => Some(*n),
                None => {
                    result.warnings.push(format!("“{}” went through a jump host that isn't in the file; it connects directly.", h.label));
                    None
                }
            },
        };
        jumps.insert(p.id, h.jump_host_id);
        built.push((p.id, h));
    }
    for (id, h) in &mut built {
        if h.jump_host_id.is_some() && jump_chain(Some(*id), h.jump_host_id, crate::ssh::MAX_JUMPS, |x| jumps.get(&x).copied()).is_err() {
            result.warnings.push(format!("“{}” would loop through its jump hosts here, so it connects directly.", h.label));
            h.jump_host_id = None;
            jumps.insert(*id, None);
        }
    }
    let mut group_writes: Vec<HostGroup> = Vec::new();
    for g in groups_to_add {
        let mut g = g.clone();
        g.default_identity_id = g.default_identity_id.and_then(|i| identity_map.get(&i).copied());
        g.proxy_id = g.proxy_id.and_then(|i| proxy_map.get(&i).copied());
        g.default_jump_host_id = g.default_jump_host_id.and_then(|j| host_map.get(&j).copied());
        group_writes.push(g);
    }

    // 5. Write, the things hosts point at first, so a failure part-way never leaves a host with a dangling link.
    for (id, k) in &key_writes {
        vault.put(Collection::Keys, *id, k, Base::New)?;
    }
    for (id, i) in &identity_writes {
        vault.put(Collection::Identities, *id, i, Base::New)?;
    }
    for (id, p) in &proxy_writes {
        vault.put(Collection::Proxies, *id, p, Base::New)?;
    }
    for g in &group_writes {
        vault.put(Collection::Groups, Uuid::new_v4(), g, Base::New)?;
    }
    for (id, h) in &built {
        vault.put(Collection::Hosts, *id, h, Base::New)?;
    }
    result.hosts_added = built.len();
    result.keys_added = key_writes.len();
    result.identities_added = identity_writes.len();
    result.proxies_added = proxy_writes.len();
    result.groups_added = group_writes.len();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::TempDir::new().unwrap();
        let v = Vault::create(dir.path().join("v"), b"pw", crate::vault::testutil::opts(false), crate::vault::DeviceInfo::new("t")).unwrap().0;
        (dir, v)
    }

    fn host(label: &str, group: &str) -> Host {
        Host { label: label.into(), hostname: format!("{label}.example"), port: 22, group: group.into(), ..Default::default() }
    }

    fn put<T: Serialize + serde::de::DeserializeOwned>(v: &Vault, c: Collection, data: &T) -> Uuid {
        let id = Uuid::new_v4();
        v.put(c, id, data, Base::New).unwrap();
        id
    }

    fn hosts(v: &Vault) -> Vec<(Uuid, Host)> {
        existing_hosts(v).unwrap()
    }

    fn by_label(v: &Vault, label: &str) -> (Uuid, Host) {
        hosts(v).into_iter().find(|(_, h)| h.label == label).unwrap_or_else(|| panic!("no host {label}"))
    }

    fn fast() -> KdfParams {
        KdfParams::insecure_for_tests()
    }

    /// A small estate: nested groups with defaults, a jump host, shared and host-owned logins, a key and a proxy.
    fn estate(v: &Vault) {
        let key = put(v, Collection::Keys, &SshKey { name: "ops key".into(), algorithm: "ssh-ed25519".into(), public_key: "ssh-ed25519 AAAA ops".into(), fingerprint: "SHA256:abc".into(), private_key: Some("PRIVATE-KEY-TEXT".into()), passphrase: Some("key-pass".into()), ..Default::default() });
        let shared = put(v, Collection::Identities, &Identity { label: "deploy".into(), username: "deploy".into(), auth: AuthMethod::Password { password: "s3cret-pw".into() }, notes: String::new(), for_host: None });
        let keyed = put(v, Collection::Identities, &Identity { label: "ops".into(), username: "ops".into(), auth: AuthMethod::Key { key_id: key }, notes: String::new(), for_host: None });
        let proxy = put(v, Collection::Proxies, &Proxy { name: "corp".into(), spec: ProxySpec::Socks5 { host: "p".into(), port: 1080, username: Some("u".into()), password: Some("proxy-pw".into()) } });
        let cmd_proxy = put(v, Collection::Proxies, &Proxy { name: "cmd".into(), spec: ProxySpec::Command { command: "nc %h %p".into(), approved: true } });
        put(v, Collection::Groups, &HostGroup { path: "Prod".into(), default_identity_id: Some(shared), proxy_id: Some(proxy), notes: "production".into(), ..Default::default() });
        put(v, Collection::Groups, &HostGroup { path: "Prod/DB".into(), environment: "production".into(), ..Default::default() });
        put(v, Collection::Groups, &HostGroup { path: "Empty".into(), ..Default::default() });
        let bastion = put(v, Collection::Hosts, &Host { identity_id: Some(keyed), ..host("bastion", "Infra") });
        let web = Host { identity_id: Some(shared), jump_host_id: Some(bastion), proxy_id: Some(cmd_proxy), tags: vec!["web".into()], notes: "n".into(), favorite: true, ..host("web-01", "Prod") };
        let web_id = put(v, Collection::Hosts, &web);
        let own = put(v, Collection::Identities, &Identity { label: "web-01".into(), username: "root".into(), auth: AuthMethod::Password { password: "own-pw".into() }, notes: String::new(), for_host: Some(web_id) });
        put(v, Collection::Hosts, &Host { identity_id: Some(own), jump_host_id: Some(web_id), ..host("db-01", "Prod/DB") });
        put(v, Collection::Hosts, &host("loose", ""));
    }

    // -- the file ------------------------------------------------------------------------------------------

    #[test]
    fn a_file_opens_only_with_its_passphrase() {
        let b = Bundle { hosts: vec![Item { id: Uuid::new_v4(), data: host("a", "G") }], ..Default::default() };
        let text = seal(&b, "correct horse", fast()).unwrap();
        assert!(!text.contains("example") && !text.contains("\"a\""), "nothing readable in the file");
        assert_eq!(open(&text, "correct horse").unwrap().hosts[0].data.label, "a");
        assert!(matches!(open(&text, "wrong horse"), Err(ExportError::WrongPassphrase)));
        assert!(matches!(open(&text, ""), Err(ExportError::WrongPassphrase)));
        assert!(matches!(seal(&b, "short", fast()), Err(ExportError::WeakPassphrase)));
        // Two exports of the same thing differ (fresh salt and nonce).
        assert_ne!(text, seal(&b, "correct horse", fast()).unwrap());
    }

    #[test]
    fn other_files_and_damaged_ones_are_refused() {
        assert!(matches!(open("hello", "x"), Err(ExportError::NotAnExport)));
        assert!(matches!(open("{}", "x"), Err(ExportError::NotAnExport)));
        assert!(matches!(open(r#"{"format":"something-else","version":1,"kdf":{"m_cost_kib":8,"t_cost":1,"p_cost":1},"salt":"","data":""}"#, "x"), Err(ExportError::NotAnExport)));
        let text = seal(&Bundle::default(), "correct horse", fast()).unwrap();
        let newer = text.replace("\"version\": 1", "\"version\": 2");
        assert!(matches!(open(&newer, "correct horse"), Err(ExportError::Newer(2))));
        // A flipped bit in the data is caught, not read as something else.
        let mut v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut data = B64.decode(v["data"].as_str().unwrap()).unwrap();
        let last = data.len() - 1;
        data[last] ^= 1;
        v["data"] = B64.encode(data).into();
        assert!(matches!(open(&v.to_string(), "correct horse"), Err(ExportError::WrongPassphrase)));
        // Absurd cost parameters from a hostile file aren't run.
        let hostile = text.replace("\"t_cost\": 1", "\"t_cost\": 4000000");
        assert!(matches!(open(&hostile, "correct horse"), Err(ExportError::NotAnExport)));
    }

    // -- a round trip --------------------------------------------------------------------------------------

    #[test]
    fn hosts_groups_logins_and_links_arrive_in_another_vault() {
        let (_a, src) = vault();
        estate(&src);
        let text = seal(&build(&src, &ExportOptions { include_secrets: true, group: None }).unwrap(), "correct horse", fast()).unwrap();

        let (_b, dst) = vault();
        let bundle = open(&text, "correct horse").unwrap();
        let p = preview(&dst, &bundle).unwrap();
        assert_eq!((p.hosts, p.groups, p.identities, p.keys, p.proxies), (4, 3, 3, 1, 2));
        assert!(p.conflicts.is_empty() && p.secrets);
        let r = apply(&dst, &bundle, &[]).unwrap();
        assert_eq!((r.hosts_added, r.groups_added, r.identities_added, r.keys_added, r.proxies_added), (4, 3, 3, 1, 2), "{r:?}");
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);

        // The structure is the same.
        let mut groups: Vec<String> = hosts(&dst).into_iter().map(|(_, h)| h.group).collect();
        groups.sort();
        assert_eq!(groups, ["", "Infra", "Prod", "Prod/DB"]);
        let g: Vec<HostGroup> = dst.list::<HostGroup>(Collection::Groups).unwrap().records.into_iter().filter_map(|r| r.data).collect();
        assert!(g.iter().any(|x| x.path == "Empty"), "an empty group is kept");
        let prod = g.iter().find(|x| x.path == "Prod").unwrap();
        assert_eq!(prod.notes, "production");
        let (web_id, web) = by_label(&dst, "web-01");
        assert_eq!((web.tags.clone(), web.favorite, web.notes.as_str()), (vec!["web".to_string()], true, "n"));
        // Links point at the new records: the jump host is the new bastion, the login is the new deploy login.
        let (bastion_id, bastion) = by_label(&dst, "bastion");
        assert_eq!(web.jump_host_id, Some(bastion_id));
        let (db_id, db) = by_label(&dst, "db-01");
        assert_eq!(db.jump_host_id, Some(web_id));
        let deploy = dst.get::<Identity>(Collection::Identities, web.identity_id.unwrap()).unwrap().data.unwrap();
        assert_eq!((deploy.username.as_str(), &deploy.auth), ("deploy", &AuthMethod::Password { password: "s3cret-pw".into() }));
        // The group default points at that same login, and the host-owned login follows its host.
        assert_eq!(prod.default_identity_id, web.identity_id);
        let own = dst.get::<Identity>(Collection::Identities, db.identity_id.unwrap()).unwrap().data.unwrap();
        assert_eq!((own.username.as_str(), own.for_host), ("root", Some(web_id)));
        // The key came along, and the login that used it points at the new key.
        let kid = match dst.get::<Identity>(Collection::Identities, bastion.identity_id.unwrap()).unwrap().data.unwrap().auth {
            AuthMethod::Key { key_id } => key_id,
            other => panic!("{other:?}"),
        };
        let key = dst.get::<SshKey>(Collection::Keys, kid).unwrap().data.unwrap();
        assert_eq!((key.private_key.as_deref(), key.passphrase.as_deref()), (Some("PRIVATE-KEY-TEXT"), Some("key-pass")));
        // New ids everywhere: nothing shares an id with the source.
        let src_ids: HashSet<Uuid> = hosts(&src).into_iter().map(|(i, _)| i).collect();
        assert!(hosts(&dst).iter().all(|(i, _)| !src_ids.contains(i)));
        let _ = db_id;
        // A command proxy never arrives approved; a password proxy keeps its password.
        let proxies: Vec<Proxy> = dst.list::<Proxy>(Collection::Proxies).unwrap().records.into_iter().filter_map(|r| r.data).collect();
        assert!(proxies.iter().any(|p| matches!(&p.spec, ProxySpec::Command { approved: false, .. })));
        assert!(proxies.iter().any(|p| matches!(&p.spec, ProxySpec::Socks5 { password: Some(pw), .. } if pw == "proxy-pw")));
        assert!(p.notes.iter().any(|n| n.contains("proxy command")), "{:?}", p.notes);
    }

    #[test]
    fn without_secrets_no_password_or_key_is_in_the_file() {
        let (_a, src) = vault();
        estate(&src);
        let b = build(&src, &ExportOptions { include_secrets: false, group: None }).unwrap();
        let json = serde_json::to_string(&b).unwrap();
        for secret in ["s3cret-pw", "own-pw", "PRIVATE-KEY-TEXT", "key-pass", "proxy-pw"] {
            assert!(!json.contains(secret), "{secret} leaked");
        }
        // User names, public keys and structure are still there.
        assert!(json.contains("\"deploy\"") && json.contains("ssh-ed25519 AAAA ops") && json.contains("Prod/DB"));
        let (_b, dst) = vault();
        let p = preview(&dst, &b).unwrap();
        assert!(!p.secrets && p.notes.iter().any(|n| n.contains("no saved passwords")));
    }

    #[test]
    fn one_group_comes_with_what_it_needs() {
        let (_a, src) = vault();
        estate(&src);
        let b = build(&src, &ExportOptions { include_secrets: true, group: Some("Prod/DB".into()) }).unwrap();
        let mut labels: Vec<&str> = b.hosts.iter().map(|h| h.data.label.as_str()).collect();
        labels.sort();
        // db-01, and the jump hosts it goes through (web-01, then bastion) wherever they live.
        assert_eq!(labels, ["bastion", "db-01", "web-01"]);
        // The group and its parents (their defaults are inherited), not unrelated groups.
        let mut paths: Vec<&str> = b.groups.iter().map(|g| g.path.as_str()).collect();
        paths.sort();
        assert_eq!(paths, ["Prod", "Prod/DB"]);
        assert!(!labels.contains(&"loose"));
    }

    // -- the same name -------------------------------------------------------------------------------------

    fn seed_conflicts(dst: &Vault) {
        put(dst, Collection::Hosts, &host("web-01", "Prod"));
        put(dst, Collection::Hosts, &host("web-01 (2)", "Prod"));
        put(dst, Collection::Hosts, &host("bastion", "Elsewhere"));
    }

    #[test]
    fn the_same_name_in_the_same_group_is_asked_about_and_other_places_are_not() {
        let (_a, src) = vault();
        estate(&src);
        let bundle = build(&src, &ExportOptions { include_secrets: true, group: None }).unwrap();
        let (_b, dst) = vault();
        seed_conflicts(&dst);
        // `bastion` here is in another group, so it is no clash; `web-01` in Prod is.
        let p = preview(&dst, &bundle).unwrap();
        assert_eq!(p.conflicts.len(), 1);
        let c = &p.conflicts[0];
        assert_eq!((c.label.as_str(), c.group.as_str()), ("web-01", "Prod"));
        assert_eq!(c.suggested, "web-01 (3)", "the suggestion skips names that are taken");
        // Capital letters don't make a different name.
        put(&dst, Collection::Hosts, &host("LOOSE", ""));
        assert_eq!(preview(&dst, &bundle).unwrap().conflicts.len(), 2);
    }

    #[test]
    fn a_clashing_host_can_be_kept_under_another_name_and_nothing_is_overwritten() {
        let (_a, src) = vault();
        estate(&src);
        let bundle = build(&src, &ExportOptions { include_secrets: true, group: None }).unwrap();
        let (_b, dst) = vault();
        let mine = put(&dst, Collection::Hosts, &Host { notes: "mine".into(), ..host("web-01", "Prod") });
        let id = preview(&dst, &bundle).unwrap().conflicts[0].id;
        let r = apply(&dst, &bundle, &[Resolution { id, action: Action::Rename { label: "  web-01 imported ".into() } }]).unwrap();
        assert_eq!((r.hosts_added, r.hosts_renamed, r.hosts_skipped), (4, 1, 0));
        assert_eq!(dst.get::<Host>(Collection::Hosts, mine).unwrap().data.unwrap().notes, "mine", "the existing host is untouched");
        let (new_web, web) = by_label(&dst, "web-01 imported");
        assert_ne!(new_web, mine);
        // Hosts that jump through it follow the renamed one.
        assert_eq!(by_label(&dst, "db-01").1.jump_host_id, Some(new_web));
        assert_eq!(web.group, "Prod");
        // Importing the same file again asks about every host and, left alone, keeps both: still no overwrite.
        let again = apply(&dst, &bundle, &[]).unwrap();
        assert_eq!((again.hosts_added, again.hosts_renamed, again.hosts_skipped), (4, 4, 0));
        assert_eq!(hosts(&dst).len(), 1 + 4 + 4);
    }

    #[test]
    fn a_name_that_is_taken_too_is_refused_and_empty_is_refused() {
        let (_a, src) = vault();
        estate(&src);
        let bundle = build(&src, &ExportOptions { include_secrets: true, group: None }).unwrap();
        let (_b, dst) = vault();
        seed_conflicts(&dst);
        let id = preview(&dst, &bundle).unwrap().conflicts[0].id;
        for label in ["web-01 (2)", "WEB-01 (2)", "web-01", "", "   "] {
            let r = apply(&dst, &bundle, &[Resolution { id, action: Action::Rename { label: label.into() } }]);
            assert!(matches!(r, Err(ExportError::Name(_))), "{label:?}: {r:?}");
        }
        assert_eq!(hosts(&dst).len(), 3, "a refused import writes nothing");
    }

    #[test]
    fn skipping_a_host_points_its_dependants_at_the_one_already_here() {
        let (_a, src) = vault();
        estate(&src);
        let bundle = build(&src, &ExportOptions { include_secrets: true, group: None }).unwrap();
        let (_b, dst) = vault();
        let mine = put(&dst, Collection::Hosts, &host("web-01", "Prod"));
        let id = preview(&dst, &bundle).unwrap().conflicts[0].id;
        let r = apply(&dst, &bundle, &[Resolution { id, action: Action::Skip }]).unwrap();
        assert_eq!((r.hosts_added, r.hosts_skipped), (3, 1));
        assert_eq!(by_label(&dst, "db-01").1.jump_host_id, Some(mine), "db-01 now jumps through the web-01 that was already here");
        assert_eq!(hosts(&dst).iter().filter(|(_, h)| h.label == "web-01").count(), 1);
        // The login that belonged to the skipped host isn't imported for nothing.
        assert!(dst.list::<Identity>(Collection::Identities).unwrap().records.iter().all(|r| r.data.as_ref().is_none_or(|i| i.label != "web-01")));
    }

    // -- sharing what is already there ---------------------------------------------------------------------

    #[test]
    fn identical_logins_keys_and_proxies_are_reused_and_existing_groups_keep_their_settings() {
        let (_a, src) = vault();
        estate(&src);
        let bundle = build(&src, &ExportOptions { include_secrets: true, group: None }).unwrap();
        let (_b, dst) = vault();
        let first = apply(&dst, &bundle, &[]).unwrap();
        assert_eq!(first.identities_added, 3);
        // A second import adds hosts (renamed) but not another copy of the shared login, key, proxies or groups.
        let second = apply(&dst, &bundle, &[]).unwrap();
        assert_eq!((second.keys_added, second.proxies_added, second.groups_added), (0, 0, 0), "{second:?}");
        assert_eq!(second.identities_added, 1, "only the login a host owns is new: it goes with its host");
        // A group that already exists is left as it is.
        let (_c, other) = vault();
        put(&other, Collection::Groups, &HostGroup { path: "Prod".into(), notes: "my own notes".into(), ..Default::default() });
        apply(&other, &bundle, &[]).unwrap();
        let g: Vec<HostGroup> = other.list::<HostGroup>(Collection::Groups).unwrap().records.into_iter().filter_map(|r| r.data).collect();
        assert_eq!(g.iter().filter(|x| x.path == "Prod").count(), 1);
        assert_eq!(g.iter().find(|x| x.path == "Prod").unwrap().notes, "my own notes");
    }

    #[test]
    fn a_crafted_file_cannot_make_a_jump_loop_or_dangle() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let bundle = Bundle {
            hosts: vec![
                Item { id: a, data: Host { jump_host_id: Some(b), ..host("a", "") } },
                Item { id: b, data: Host { jump_host_id: Some(a), identity_id: Some(Uuid::new_v4()), proxy_id: Some(Uuid::new_v4()), ..host("b", "") } },
                Item { id: Uuid::new_v4(), data: Host { jump_host_id: Some(Uuid::new_v4()), ..host("c", "") } },
                Item { id: Uuid::new_v4(), data: Host { label: " ".into(), ..host("d", "") } },
            ],
            ..Default::default()
        };
        let (_d, dst) = vault();
        let r = apply(&dst, &bundle, &[]).unwrap();
        assert_eq!((r.hosts_added, r.hosts_skipped), (3, 1));
        let all = hosts(&dst);
        assert!(all.iter().all(|(id, h)| jump_chain(Some(*id), h.jump_host_id, 8, |x| all.iter().find(|(i, _)| *i == x).map(|(_, h)| h.jump_host_id)).is_ok()), "no loops, no dangling links");
        assert!(all.iter().all(|(_, h)| h.identity_id.is_none() && h.proxy_id.is_none()));
        assert!(r.warnings.len() >= 3, "{:?}", r.warnings);
    }

    #[test]
    fn answers_arrive_from_the_window_in_this_shape() {
        let id = Uuid::new_v4();
        let r: Vec<Resolution> = serde_json::from_str(&format!(r#"[{{"id":"{id}","action":"rename","label":"web (2)"}},{{"id":"{id}","action":"skip"}}]"#)).unwrap();
        assert!(matches!(&r[0].action, Action::Rename { label } if label == "web (2)") && matches!(r[1].action, Action::Skip));
        let o: ExportOptions = serde_json::from_str(r#"{"include_secrets":true,"group":"Prod"}"#).unwrap();
        assert!(o.include_secrets && o.group.as_deref() == Some("Prod"));
        let d: ExportOptions = serde_json::from_str("{}").unwrap();
        assert!(!d.include_secrets && d.group.is_none(), "secrets are never in by default");
    }

    #[test]
    fn hosts_that_run_a_command_here_are_pointed_out() {
        let bundle = Bundle { hosts: vec![Item { id: Uuid::new_v4(), data: Host { hook_before: "vpn up".into(), ..host("a", "") } }, Item { id: Uuid::new_v4(), data: Host { protocol: "command".into(), connect_command: "aws ssm".into(), ..host("b", "") } }, Item { id: Uuid::new_v4(), data: host("c", "") }], ..Default::default() };
        let (_d, dst) = vault();
        let p = preview(&dst, &bundle).unwrap();
        assert!(p.notes.iter().any(|n| n.starts_with("2 hosts run a command")), "{:?}", p.notes);
    }
}
