//! Vault-side Key Manager and connect-time resolution: turning key
//! references and group defaults into what the SSH engine needs.

use std::collections::HashMap;

use serde::Serialize;
use uuid::Uuid;

use crate::keys::{self, KeyMaterial};
use crate::models::{AuthMethod, Host, HostGroup, Identity, SshKey};
use crate::vault::{now_ms, Collection, Vault, VaultError};

#[derive(Debug, thiserror::Error)]
pub enum KeyManagerError {
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("{0}")]
    Key(#[from] keys::KeyError),
    #[error("SSH key {0} no longer exists in the vault")]
    MissingKey(Uuid),
    #[error("SSH key \"{0}\" has no private half; import the private key to log in with it")]
    PublicOnly(String),
    #[error("this key is used by {0}; detach it first")]
    InUse(String),
}

pub type Result<T> = std::result::Result<T, KeyManagerError>;

/// Build a Key Manager entry from inspected key material. The passphrase is
/// stored only if the user asked to save it.
pub fn new_key(name: &str, m: KeyMaterial, passphrase: Option<&str>) -> SshKey {
    let name = if name.trim().is_empty() {
        if m.comment.is_empty() {
            m.algorithm.clone()
        } else {
            m.comment.clone()
        }
    } else {
        name.trim().to_string()
    };
    SshKey {
        name,
        algorithm: m.algorithm,
        public_key: m.public_key,
        fingerprint: m.fingerprint,
        private_key: m.private_key.map(|p| p.to_string()),
        passphrase: passphrase.filter(|p| !p.is_empty()).map(str::to_string),
        encrypted: m.encrypted,
        certificate: None,
        comment: m.comment,
        created_at: now_ms(),
        for_host: None,
        agent: Default::default(),
    }
}

/// Turn a Key Manager reference into the key itself for the SSH engine.
/// Other auth methods pass through unchanged.
pub fn resolve_auth(vault: &Vault, auth: AuthMethod) -> Result<AuthMethod> {
    let AuthMethod::Key { key_id } = auth else {
        return Ok(auth);
    };
    let key = vault
        .get::<SshKey>(Collection::Keys, key_id)
        .ok()
        .and_then(|r| r.data)
        .ok_or(KeyManagerError::MissingKey(key_id))?;
    let private_key = key
        .private_key
        .ok_or_else(|| KeyManagerError::PublicOnly(key.name.clone()))?;
    Ok(AuthMethod::PrivateKey {
        private_key,
        passphrase: key.passphrase,
        certificate: key.certificate,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct KeyUsage {
    pub identities: Vec<UsedBy>,
    pub hosts: Vec<UsedBy>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UsedBy {
    pub id: Uuid,
    pub label: String,
}

/// Credentials that use a key, and the hosts that use those credentials
/// (directly or through their group's default).
pub fn key_usage(vault: &Vault, key_id: Uuid) -> Result<KeyUsage> {
    let identities: Vec<UsedBy> = vault
        .list::<Identity>(Collection::Identities)?
        .records
        .into_iter()
        .filter_map(|r| {
            let d = r.data?;
            matches!(d.auth, AuthMethod::Key { key_id: k } if k == key_id).then_some(UsedBy {
                id: r.id,
                label: d.label,
            })
        })
        .collect();
    let groups = load_groups(vault)?;
    let hosts = vault
        .list::<Host>(Collection::Hosts)?
        .records
        .into_iter()
        .filter_map(|r| {
            let h = r.data?;
            let ident = effective(&h, &groups).identity_id?;
            identities.iter().any(|i| i.id == ident).then_some(UsedBy {
                id: r.id,
                label: h.label,
            })
        })
        .collect();
    Ok(KeyUsage { identities, hosts })
}

/// Refuse to delete a key that credentials still point to.
pub fn check_unused(vault: &Vault, key_id: Uuid) -> Result<()> {
    let usage = key_usage(vault, key_id)?;
    if usage.identities.is_empty() {
        return Ok(());
    }
    let names: Vec<_> = usage.identities.iter().map(|u| format!("\"{}\"", u.label)).collect();
    Err(KeyManagerError::InUse(names.join(", ")))
}

// ---------------------------------------------------------------------------
// Group defaults
// ---------------------------------------------------------------------------

pub fn load_groups(vault: &Vault) -> Result<HashMap<String, HostGroup>> {
    Ok(vault
        .list::<HostGroup>(Collection::Groups)?
        .records
        .into_iter()
        .filter_map(|r| r.data)
        .map(|g| (normalize(&g.path), g))
        .collect())
}

fn normalize(path: &str) -> String {
    path.split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// What a host actually uses once its groups' defaults are applied. A
/// host's own setting wins; otherwise the nearest group that sets it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Effective {
    pub identity_id: Option<Uuid>,
    pub jump_host_id: Option<Uuid>,
    pub proxy_id: Option<Uuid>,
    pub environment: String,
}

pub fn effective(host: &Host, groups: &HashMap<String, HostGroup>) -> Effective {
    let mut e = Effective {
        identity_id: host.identity_id,
        jump_host_id: host.jump_host_id,
        proxy_id: host.proxy_id,
        environment: host.environment.clone(),
    };
    // Nearest group first: "a/b/c", "a/b", "a".
    let path = normalize(&host.group);
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    while !parts.is_empty() {
        if let Some(g) = groups.get(&parts.join("/")) {
            e.identity_id = e.identity_id.or(g.default_identity_id);
            if !host.no_group_jump {
                e.jump_host_id = e.jump_host_id.or(g.default_jump_host_id);
            }
            if !host.no_group_proxy {
                e.proxy_id = e.proxy_id.or(g.proxy_id);
            }
            if e.environment.is_empty() {
                e.environment = g.environment.clone();
            }
        }
        parts.pop();
    }
    e
}

// ---------------------------------------------------------------------------
// Connection targets
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error(transparent)]
    Keys(#[from] KeyManagerError),
    /// The host (or a jump host) has no credential, directly or via a group.
    #[error("{0}")]
    CredentialsRequired(String),
    #[error("{0}")]
    JumpChain(String),
    #[error("{0}")]
    NotFound(String),
}

impl From<VaultError> for ResolveError {
    fn from(e: VaultError) -> Self {
        Self::Keys(KeyManagerError::Vault(e))
    }
}

/// Each host's jump host after group defaults. A host never defaults to
/// jumping through itself (e.g. the bastion in its own group).
pub fn jump_map(vault: &Vault) -> Result<HashMap<Uuid, Option<Uuid>>> {
    let groups = load_groups(vault)?;
    Ok(vault
        .list::<Host>(Collection::Hosts)?
        .records
        .into_iter()
        .filter_map(|r| {
            let d = r.data?;
            let j = effective(&d, &groups).jump_host_id.filter(|j| *j != r.id);
            Some((r.id, j))
        })
        .collect())
}

/// What the reachability check probes for each host: its address and the
/// proxy it is reached through, after group defaults. Hosts behind a jump
/// host get `None`; a probe doesn't open jump sessions.
pub fn probe_targets(vault: &Vault, only: Option<&[Uuid]>) -> Result<Vec<(Uuid, Option<crate::health::ProbeTarget>)>> {
    let groups = load_groups(vault)?;
    let proxies: HashMap<Uuid, crate::models::ProxySpec> = vault
        .list::<crate::models::Proxy>(Collection::Proxies)?
        .records
        .into_iter()
        .filter_map(|r| Some((r.id, r.data?.spec)))
        .collect();
    Ok(vault
        .list::<Host>(Collection::Hosts)?
        .records
        .into_iter()
        .filter(|r| only.is_none_or(|ids| ids.contains(&r.id)))
        .filter_map(|r| {
            let d = r.data?;
            let e = effective(&d, &groups);
            let behind_jump = e.jump_host_id.is_some_and(|j| j != r.id);
            let target = (!behind_jump).then(|| {
                let proxy = e.proxy_id.and_then(|p| proxies.get(&p).cloned());
                (d.hostname, d.port, proxy)
            });
            Some((r.id, target))
        })
        .collect())
}

fn load_host(vault: &Vault, id: Uuid) -> std::result::Result<Host, ResolveError> {
    vault
        .get::<Host>(Collection::Hosts, id)
        .ok()
        .and_then(|r| r.data)
        .ok_or_else(|| ResolveError::NotFound("that host no longer exists".into()))
}

/// Remember the certificate a Remote Desktop host presented, so a different one
/// is refused later. Nothing else about the host changes.
pub fn pin_rdp_certificate(vault: &Vault, host_id: Uuid, fingerprint: &str) -> std::result::Result<(), crate::vault::VaultError> {
    let record = vault.get::<Host>(Collection::Hosts, host_id)?;
    if let Some(mut host) = record.data {
        host.rdp.get_or_insert_with(Default::default).cert_sha256 = fingerprint.to_string();
        vault.put(Collection::Hosts, host_id, &host, crate::vault::Base::Rev(record.rev))?;
    }
    Ok(())
}

fn load_proxy(vault: &Vault, id: Option<Uuid>) -> std::result::Result<Option<crate::models::ProxySpec>, ResolveError> {
    let Some(id) = id else { return Ok(None) };
    vault
        .get::<crate::models::Proxy>(Collection::Proxies, id)
        .ok()
        .and_then(|r| r.data)
        .map(|p| Some(p.spec))
        .ok_or_else(|| ResolveError::NotFound("the proxy this host uses was deleted".into()))
}

fn login(vault: &Vault, e: &Effective) -> std::result::Result<Option<(String, AuthMethod)>, ResolveError> {
    let Some(identity_id) = e.identity_id else {
        return Ok(None);
    };
    let identity = vault
        .get::<Identity>(Collection::Identities, identity_id)
        .ok()
        .and_then(|r| r.data)
        .ok_or_else(|| ResolveError::NotFound("the credential this host uses was deleted".into()))?;
    Ok(Some((identity.username, resolve_auth(vault, identity.auth)?)))
}

/// Everything needed to connect to `host_id`: its address, login (after
/// group defaults and key references), proxy, keep-alive, and jump chain.
/// One-time `credentials` (username, password) apply to the final host
/// only; every jump host must have a credential of its own.
pub fn resolve_target(
    vault: &Vault,
    host_id: Uuid,
    credentials: Option<(String, String)>,
    host_keys: crate::ssh::HostKeyPolicy,
) -> std::result::Result<crate::ssh::Target, ResolveError> {
    use crate::ssh::Target;
    let host = load_host(vault, host_id)?;
    let groups = load_groups(vault)?;
    let jumps = jump_map(vault)?;
    let first_jump = jumps.get(&host_id).copied().flatten();
    let chain = crate::models::jump_chain(Some(host_id), first_jump, crate::ssh::MAX_JUMPS, |h| jumps.get(&h).copied())
        .map_err(|e| ResolveError::JumpChain(e.to_string()))?;

    let eff = effective(&host, &groups);
    // The proxy reaches whichever hop is dialled directly: the outermost
    // jump uses its own proxy, else the target's.
    let target_proxy = load_proxy(vault, eff.proxy_id)?;
    let outermost = chain.last().copied();

    // Build from the outermost hop inwards so each Target owns its jump.
    let mut jump: Option<Box<Target>> = None;
    for jid in chain.into_iter().rev() {
        let jh = load_host(vault, jid)?;
        let je = effective(&jh, &groups);
        let proxy = match load_proxy(vault, je.proxy_id)? {
            Some(p) => Some(p),
            None if Some(jid) == outermost => target_proxy.clone(),
            None => None,
        };
        let (username, auth) = login(vault, &je)?.ok_or_else(|| {
            ResolveError::CredentialsRequired(format!("jump host \"{}\" needs a credential attached", jh.label))
        })?;
        jump = Some(Box::new(Target {
            hostname: jh.hostname,
            port: jh.port,
            username,
            auth,
            host_keys: host_keys.clone(),
            jump,
            forward_agent: false,
            agent_backend: None,
            forward_x11: false,
            proxy,
            keepalive_secs: jh.keepalive_secs,
        }));
    }

    let (username, auth) = match credentials {
        Some((username, password)) => (username, AuthMethod::Password { password }),
        None => login(vault, &eff)?
            .ok_or_else(|| ResolveError::CredentialsRequired("this host has no credential; supply one".into()))?,
    };
    let proxy = if jump.is_none() { target_proxy } else { None };
    Ok(Target {
        hostname: host.hostname,
        port: host.port,
        username,
        auth,
        host_keys,
        jump,
        forward_agent: host.forward_agent,
        agent_backend: None,
        forward_x11: host.forward_x11,
        proxy,
        keepalive_secs: host.keepalive_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyAlgorithm;
    use crate::vault::testutil::new_vault;
    use crate::vault::Base;

    fn identity(key_id: Uuid) -> Identity {
        Identity {
            label: "deploy".into(),
            username: "deploy".into(),
            auth: AuthMethod::Key { key_id },
            notes: String::new(),
            for_host: None,
        }
    }

    #[test]
    fn pinning_a_certificate_changes_only_the_pin() {
        let (_d, v) = new_vault();
        let host = Host {
            label: "winbox".into(),
            hostname: "10.0.0.9".into(),
            port: 3389,
            protocol: "rdp".into(),
            rdp: Some(crate::models::RdpOptions { domain: "CORP".into(), width: 1280, height: 720, color_depth: 16, ..Default::default() }),
            ..Default::default()
        };
        let id = v.insert(Collection::Hosts, &host).unwrap().id;
        pin_rdp_certificate(&v, id, &"ab".repeat(32)).unwrap();
        let got = v.get::<Host>(Collection::Hosts, id).unwrap().data.unwrap();
        let rdp = got.rdp.unwrap();
        assert_eq!(rdp.cert_sha256, "ab".repeat(32));
        assert_eq!((rdp.domain.as_str(), rdp.width, rdp.height, rdp.color_depth), ("CORP", 1280, 720, 16), "the other settings are untouched");
        assert_eq!((got.label.as_str(), got.hostname.as_str(), got.port), ("winbox", "10.0.0.9", 3389));
        // A changed certificate the person accepted replaces the pin.
        pin_rdp_certificate(&v, id, &"cd".repeat(32)).unwrap();
        assert_eq!(v.get::<Host>(Collection::Hosts, id).unwrap().data.unwrap().rdp.unwrap().cert_sha256, "cd".repeat(32));
        // A host with no Remote Desktop settings yet gets them with defaults.
        let plain = v.insert(Collection::Hosts, &Host { label: "x".into(), hostname: "h".into(), ..Default::default() }).unwrap().id;
        pin_rdp_certificate(&v, plain, "ee").unwrap();
        let r = v.get::<Host>(Collection::Hosts, plain).unwrap().data.unwrap().rdp.unwrap();
        assert_eq!((r.color_depth, r.cert_sha256.as_str()), (32, "ee"));
    }

    #[test]
    fn rdp_settings_survive_the_vault_and_old_hosts_load_without_them() {
        let (_d, v) = new_vault();
        let mut host = Host { label: "w".into(), hostname: "h".into(), protocol: "rdp".into(), rdp: Some(Default::default()), ..Default::default() };
        host.rdp.as_mut().unwrap().security = crate::rdp::Security::Nla;
        let id = v.insert(Collection::Hosts, &host).unwrap().id;
        assert_eq!(v.get::<Host>(Collection::Hosts, id).unwrap().data.unwrap(), host);
        // A host saved before Remote Desktop existed has no such field.
        let old: Host = serde_json::from_str(r#"{"label":"a","hostname":"b","port":22,"group":"","tags":[],"notes":""}"#).unwrap();
        assert_eq!(old.rdp, None);
        let json = serde_json::to_string(&old).unwrap();
        assert!(!json.contains("rdp"), "nothing is written for hosts without it: {json}");
    }

    #[test]
    fn key_references_resolve_and_track_usage() {
        let (_d, v) = new_vault();
        let m = keys::generate(KeyAlgorithm::Ed25519, "me@pc", Some("pp")).unwrap();
        let key = new_key("", m, Some("pp"));
        assert_eq!(key.name, "me@pc");
        let kid = v.insert(Collection::Keys, &key).unwrap().id;
        let iid = v.insert(Collection::Identities, &identity(kid)).unwrap().id;
        // One host uses the identity directly, one through its group.
        let direct = Host {
            label: "web-01".into(),
            hostname: "10.0.0.1".into(),
            identity_id: Some(iid),
            ..Default::default()
        };
        let via_group = Host {
            label: "db-01".into(),
            hostname: "10.0.0.2".into(),
            group: "Production/DB".into(),
            ..Default::default()
        };
        v.insert(Collection::Hosts, &direct).unwrap();
        v.insert(Collection::Hosts, &via_group).unwrap();
        v.insert(
            Collection::Groups,
            &HostGroup {
                path: "Production".into(),
                default_identity_id: Some(iid),
                environment: "production".into(),
                ..Default::default()
            },
        )
        .unwrap();

        let usage = key_usage(&v, kid).unwrap();
        assert_eq!(usage.identities.len(), 1);
        let mut hosts: Vec<_> = usage.hosts.iter().map(|h| h.label.as_str()).collect();
        hosts.sort();
        assert_eq!(hosts, ["db-01", "web-01"]);
        assert!(matches!(check_unused(&v, kid), Err(KeyManagerError::InUse(_))));

        match resolve_auth(&v, AuthMethod::Key { key_id: kid }).unwrap() {
            AuthMethod::PrivateKey { private_key, passphrase, .. } => {
                assert!(russh::keys::decode_secret_key(&private_key, passphrase.as_deref()).is_ok());
            }
            other => panic!("unexpected {other:?}"),
        }
        // Deleted key → a clear error, not a silent fallback.
        v.delete(Collection::Keys, kid, Base::Latest).unwrap();
        assert!(matches!(
            resolve_auth(&v, AuthMethod::Key { key_id: kid }),
            Err(KeyManagerError::MissingKey(_))
        ));
    }

    #[test]
    fn public_only_keys_cannot_log_in() {
        let (_d, v) = new_vault();
        let m = keys::generate(KeyAlgorithm::Ed25519, "x", None).unwrap();
        let public = keys::inspect_public(&m.public_key).unwrap();
        let kid = v.insert(Collection::Keys, &new_key("colleague", public, None)).unwrap().id;
        assert!(matches!(
            resolve_auth(&v, AuthMethod::Key { key_id: kid }),
            Err(KeyManagerError::PublicOnly(_))
        ));
    }

    #[test]
    fn nearest_group_default_wins_and_host_overrides() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let mut groups = HashMap::new();
        groups.insert(
            "Prod".to_string(),
            HostGroup {
                path: "Prod".into(),
                default_identity_id: Some(a),
                default_jump_host_id: Some(a),
                environment: "production".into(),
                ..Default::default()
            },
        );
        groups.insert(
            "Prod/EU".to_string(),
            HostGroup {
                path: "Prod/EU".into(),
                default_identity_id: Some(b),
                ..Default::default()
            },
        );
        let mut h = Host {
            group: " Prod / EU ".into(),
            ..Default::default()
        };
        let e = effective(&h, &groups);
        assert_eq!(e.identity_id, Some(b));
        assert_eq!(e.jump_host_id, Some(a));
        assert_eq!(e.environment, "production");
        h.identity_id = Some(a);
        h.environment = "staging".into();
        let e = effective(&h, &groups);
        assert_eq!(e.identity_id, Some(a));
        assert_eq!(e.environment, "staging");
        // A host can opt out of the group's jump host and proxy.
        groups.get_mut("Prod").unwrap().proxy_id = Some(b);
        assert_eq!(effective(&h, &groups).proxy_id, Some(b));
        h.no_group_jump = true;
        h.no_group_proxy = true;
        let e = effective(&h, &groups);
        assert_eq!((e.jump_host_id, e.proxy_id), (None, None));
    }
}
