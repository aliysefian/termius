//! Moving hosts to and from other SSH clients.
//!
//! Each client is a [`HostFormat`]: it turns the client's file into [`ImportedHost`]s (the same type the
//! `~/.ssh/config`, PuTTY and CSV imports produce, so they all end in `sshconfig::import_into_vault`) and turns
//! [`ExportHost`]s back into the client's file. Supporting another client (SecureCRT, …) means writing one
//! `HostFormat` and adding it to [`formats`].
//!
//! Secrets never pass through here. Parsers don't read passwords out of the foreign file (they warn that some were
//! left behind), and exports carry user names and key *paths* only: the collector below never copies a password,
//! private key or passphrase out of the vault.

use std::collections::HashSet;

use serde::Serialize;

use crate::models::{AuthMethod, Host, Identity};
use crate::sshconfig::ImportedHost;
use crate::vault::{Collection, Vault, VaultError};

/// What a parser found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub hosts: Vec<ImportedHost>,
    pub warnings: Vec<String>,
}

/// Facts about this computer that a format may need to read its file.
#[derive(Debug, Default, Clone)]
pub struct ImportContext {
    /// The Windows user folder, for MobaXterm's `_ProfileDir_` placeholder.
    pub profile_dir: Option<String>,
}

/// One host as an exporter sees it: no secrets.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExportHost {
    pub name: String,
    pub hostname: String,
    pub port: u16,
    pub user: Option<String>,
    /// Slash-separated folder path, empty at the top level.
    pub group: String,
    /// Path of a private key file the identity references.
    pub key_file: Option<String>,
}

pub trait HostFormat: Sync {
    /// Stable id the frontend passes back: `"mobaxterm"`, `"asbru"`.
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    /// File extensions (lower case, no dot) the format reads and writes. The first is used for exports.
    fn extensions(&self) -> &'static [&'static str];
    fn parse(&self, text: &str, ctx: &ImportContext) -> Parsed;
    fn export(&self, hosts: &[ExportHost]) -> String;
}

static FORMATS: [&dyn HostFormat; 2] = [&crate::mobaxterm::MobaXterm, &crate::asbru::Asbru];

pub fn formats() -> &'static [&'static dyn HostFormat] {
    &FORMATS
}

pub fn find(id: &str) -> Option<&'static dyn HostFormat> {
    formats().iter().copied().find(|f| f.id() == id)
}

/// Parse with `format`, then make the aliases unique: two hosts with one name in different folders are normal in
/// other clients, but here a label is what an import matches on.
pub fn parse(format: &dyn HostFormat, text: &str, ctx: &ImportContext) -> Parsed {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parsed = format.parse(text, ctx);
    let mut seen: HashSet<String> = HashSet::new();
    let mut renamed = 0usize;
    for h in &mut parsed.hosts {
        let base = h.alias.clone();
        let mut n = 2;
        while !seen.insert(h.alias.to_lowercase()) {
            h.alias = format!("{base} ({n})");
            n += 1;
        }
        if h.alias != base {
            renamed += 1;
        }
    }
    if renamed > 0 {
        parsed.warnings.push(format!("{renamed} host(s) shared a name with another and were renamed, e.g. \"name (2)\""));
    }
    parsed
}

/// What the frontend lists for the person to choose from.
#[derive(Debug, Clone, Serialize)]
pub struct FormatInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub extensions: &'static [&'static str],
}

pub fn infos() -> Vec<FormatInfo> {
    formats().iter().map(|f| FormatInfo { id: f.id(), name: f.name(), extensions: f.extensions() }).collect()
}

fn in_group(path: &str, group: &str) -> bool {
    path == group || path.strip_prefix(group).is_some_and(|rest| rest.starts_with('/'))
}

/// The hosts to export (all, or one group and what is below it), reduced to what a foreign file can hold.
///
/// Identities are read inside the vault call and cut down to a user name and a key-file path before anything is
/// returned, so no password, private key or passphrase leaves it.
pub fn collect(vault: &Vault, group: Option<&str>) -> Result<Vec<ExportHost>, VaultError> {
    let group = group.map(|g| g.trim().trim_matches('/')).filter(|g| !g.is_empty());
    let identities: std::collections::HashMap<uuid::Uuid, (String, Option<String>)> = vault
        .list::<Identity>(Collection::Identities)?
        .records
        .into_iter()
        .filter_map(|r| {
            let i = r.data?;
            let key_file = match &i.auth {
                AuthMethod::KeyFile { path, .. } => Some(path.clone()),
                _ => None,
            };
            Some((r.id, (i.username.clone(), key_file)))
        })
        .collect();
    let mut out: Vec<ExportHost> = vault
        .list::<Host>(Collection::Hosts)?
        .records
        .into_iter()
        .filter_map(|r| r.data)
        .filter(|h| group.is_none() || group.is_some_and(|g| in_group(&h.group, g)))
        .map(|h| {
            let (user, key_file) = h.identity_id.and_then(|i| identities.get(&i)).cloned().unzip();
            ExportHost {
                name: h.label.clone(),
                hostname: h.hostname.clone(),
                port: if h.port == 0 { 22 } else { h.port },
                user: user.filter(|u| !u.is_empty()),
                group: h.group.trim_matches('/').to_string(),
                key_file: key_file.flatten(),
            }
        })
        .collect();
    out.sort_by(|a, b| (a.group.to_lowercase(), a.name.to_lowercase()).cmp(&(b.group.to_lowercase(), b.name.to_lowercase())));
    Ok(out)
}
