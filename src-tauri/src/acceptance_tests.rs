//! The portable-vault acceptance scenario, end to end against a real
//! (throw-away, user-level) `sshd`. Skipped when no sshd is available.
//!
//! PC A creates ~/Dropbox/MySSHVault, adds Production/web-01, web-02 and
//! db-01, imports an Ed25519 key, assigns it through the group and connects.
//! PC B opens the same folder and connects without being asked about the
//! host key again. PC A adds redis-01 and PC B sees it live. Concurrent
//! edits never silently destroy a change, and a copy of the folder reveals
//! nothing to an attacker.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

use uuid::Uuid;

use crate::keymanager::{new_key, resolve_target};
use crate::keys::inspect_private;
use crate::models::{AuthMethod, Host, HostGroup, Identity, SshKey};
use crate::session::Session;
use crate::ssh::testutil::spawn_sshd;
use crate::ssh::{
    BoxFuture, HostKeyPolicy, HostKeyPrompter, HostKeyQuestion, HostKeyStatus, HostKeyStore, PresentedKey,
};
use crate::vault::testutil::{kdf, opts};
use crate::vault::{Base, Collection, DeviceInfo, Unlock, Vault, VaultError};

const PASSWORD: &[u8] = b"correct horse battery staple";

/// Host keys trusted in a vault this "PC" holds directly.
struct DirectVault(Arc<Vault>);

impl HostKeyStore for DirectVault {
    fn check(&self, key: &PresentedKey) -> Result<HostKeyStatus, String> {
        crate::hostkeys::check(&self.0, key).map_err(|e| e.to_string())
    }
    fn trust(&self, key: &PresentedKey) -> Result<(), String> {
        crate::hostkeys::trust(&self.0, key).map_err(|e| e.to_string())
    }
}

/// Host keys trusted in a vault unlocked through a `Session`, as the app does.
struct SessionVault(Arc<Session>);

impl HostKeyStore for SessionVault {
    fn check(&self, key: &PresentedKey) -> Result<HostKeyStatus, String> {
        self.0.with_vault(|v| crate::hostkeys::check(v, key)).map_err(|e| e.to_string())
    }
    fn trust(&self, key: &PresentedKey) -> Result<(), String> {
        self.0.with_vault(|v| crate::hostkeys::trust(v, key)).map_err(|e| e.to_string())
    }
}

/// Answers host-key questions from a script and counts them.
struct Asker {
    asked: AtomicUsize,
    trust: bool,
}

impl HostKeyPrompter for Asker {
    fn ask(&self, _q: HostKeyQuestion) -> BoxFuture<bool> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let yes = self.trust;
        Box::pin(async move { yes })
    }
}

fn policy_for_vault(v: Arc<Vault>, asker: Arc<Asker>) -> HostKeyPolicy {
    HostKeyPolicy {
        store: Arc::new(DirectVault(v)),
        prompter: Some(asker),
    }
}

fn policy_for_session(s: Arc<Session>, asker: Arc<Asker>) -> HostKeyPolicy {
    HostKeyPolicy {
        store: Arc::new(SessionVault(s)),
        prompter: Some(asker),
    }
}

fn host(label: &str, port: u16) -> Host {
    Host {
        label: label.into(),
        hostname: "127.0.0.1".into(),
        port,
        group: "Production".into(),
        ..Default::default()
    }
}

fn label_of(v: &Vault, id: Uuid) -> String {
    v.get::<Host>(Collection::Hosts, id).unwrap().data.unwrap().label
}

async fn run(target: &crate::ssh::Target, cmd: &str) -> String {
    let out = crate::runner::exec(target, cmd, Duration::from_secs(20)).await.expect("ssh exec");
    assert_eq!(out.exit_code, Some(0), "{}", out.stderr);
    out.stdout
}

/// Every file under `dir`, recursively, as raw bytes.
fn all_bytes(dir: &std::path::Path) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }
    out
}

fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let dest = to.join(e.file_name());
        if e.path().is_dir() {
            copy_tree(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_pcs_share_a_vault_through_a_synced_folder() {
    let tmp = tempfile::TempDir::new().unwrap();
    let Some(sshd) = spawn_sshd(&tmp.path().join("sshd").tap_mkdir()) else {
        eprintln!("skipping: no usable sshd on this machine");
        return;
    };
    let root = tmp.path().join("Dropbox").join("MySSHVault");

    // ---- PC A: create, import a key, add Production hosts, connect ------
    let (a, recovery) = Vault::create(&root, PASSWORD, opts(true), DeviceInfo::new("PC-A")).unwrap();
    let recovery = recovery.expect("recovery key requested");
    let a = Arc::new(a);

    let material = inspect_private(&sshd.client_key, None).unwrap();
    let fingerprint = material.fingerprint.clone();
    let key_id = a.insert(Collection::Keys, &new_key("laptop ed25519", material, None)).unwrap().id;
    let ident = a
        .insert(
            Collection::Identities,
            &Identity {
                label: "deploy".into(),
                username: sshd.user.clone(),
                auth: AuthMethod::Key { key_id },
                notes: String::new(),
                for_host: None,
            },
        )
        .unwrap()
        .id;
    // The key is assigned once, through the group, not per host.
    a.insert(
        Collection::Groups,
        &HostGroup {
            path: "Production".into(),
            default_identity_id: Some(ident),
            environment: "production".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let web1 = a.insert(Collection::Hosts, &host("web-01", sshd.port)).unwrap().id;
    let _web2 = a.insert(Collection::Hosts, &host("web-02", sshd.port)).unwrap().id;
    let db1 = a.insert(Collection::Hosts, &host("db-01", sshd.port)).unwrap().id;

    let asker_a = Arc::new(Asker { asked: AtomicUsize::new(0), trust: true });
    let target = resolve_target(&a, web1, None, policy_for_vault(a.clone(), asker_a.clone())).unwrap();
    assert!(run(&target, "echo hello-from-web-01").await.contains("hello-from-web-01"));
    assert_eq!(asker_a.asked.load(Ordering::SeqCst), 1, "unknown host key needs explicit trust once");
    // Second connection: already trusted, nobody is asked.
    run(&target, "true").await;
    assert_eq!(asker_a.asked.load(Ordering::SeqCst), 1);

    // ---- PC B: open the same folder, connect, never asked about the key --
    let b = Arc::new(Session::new());
    let (tx, rx) = mpsc::channel();
    b.unlock(root.clone(), Unlock::Password(PASSWORD), DeviceInfo::new("PC-B"), kdf(), move |c| {
        let _ = tx.send(c);
    })
    .unwrap();
    let labels: Vec<String> = b
        .with_vault(|v| Ok(v.list::<Host>(Collection::Hosts)?.records))
        .unwrap()
        .into_iter()
        .map(|r| r.data.unwrap().label)
        .collect();
    assert_eq!(labels.len(), 3, "{labels:?}");
    let refuse_all = Arc::new(Asker { asked: AtomicUsize::new(0), trust: false });
    let policy_b = policy_for_session(b.clone(), refuse_all.clone());
    let target_b = b
        .with_vault(|v| Ok(resolve_target(v, db1, None, policy_b.clone())))
        .unwrap()
        .unwrap();
    assert!(run(&target_b, "echo hello-from-db-01").await.contains("hello-from-db-01"));
    assert_eq!(refuse_all.asked.load(Ordering::SeqCst), 0, "trust synced through the vault");
    let key_b = b
        .with_vault(|v| v.get::<SshKey>(Collection::Keys, key_id))
        .unwrap()
        .data
        .unwrap();
    assert_eq!(key_b.fingerprint, fingerprint);

    // ---- PC A adds redis-01; PC B sees it arrive -----------------------
    let redis = a.insert(Collection::Hosts, &host("redis-01", sshd.port)).unwrap().id;
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let change = rx.recv_timeout(left).expect("PC B never saw redis-01");
        if change.collection == Collection::Hosts && change.id == redis {
            let label = change.record.and_then(|r| r.data).map(|d| d["label"].clone());
            assert_eq!(label, Some(serde_json::json!("redis-01")));
            break;
        }
    }
    let target_b = b
        .with_vault(|v| Ok(resolve_target(v, redis, None, policy_b.clone())))
        .unwrap()
        .unwrap();
    run(&target_b, "true").await;

    // ---- Concurrent edits never silently destroy a change --------------
    let base = a.get::<Host>(Collection::Hosts, web1).unwrap().rev;
    let mut from_a = a.get::<Host>(Collection::Hosts, web1).unwrap().data.unwrap();
    let mut from_b = from_a.clone();
    from_a.notes = "patched kernel".into();
    from_b.tags = vec!["frontend".into()];
    a.put(Collection::Hosts, web1, &from_a, Base::Rev(base)).unwrap();
    b.with_vault(|v| v.put(Collection::Hosts, web1, &from_b, Base::Rev(base))).unwrap();
    let merged = a.get::<Host>(Collection::Hosts, web1).unwrap().data.unwrap();
    assert_eq!(merged.notes, "patched kernel", "A's edit kept");
    assert_eq!(merged.tags, ["frontend"], "B's edit kept");

    // Same field, same starting point: the second save is refused.
    let base = a.get::<Host>(Collection::Hosts, web1).unwrap().rev;
    let mut a_label = merged.clone();
    a_label.label = "web-01-a".into();
    let mut b_label = merged.clone();
    b_label.label = "web-01-b".into();
    a.put(Collection::Hosts, web1, &a_label, Base::Rev(base)).unwrap();
    let err = b.with_vault(|v| v.put(Collection::Hosts, web1, &b_label, Base::Rev(base))).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("conflict") || matches!(err, crate::session::SessionError::Vault(VaultError::Conflict(_))), "{err}");
    assert_eq!(label_of(&a, web1), "web-01-a");

    // A credential changed on both sides is never overwritten either.
    let ibase = a.get::<Identity>(Collection::Identities, ident).unwrap().rev;
    let mut ia = a.get::<Identity>(Collection::Identities, ident).unwrap().data.unwrap();
    let mut ib = ia.clone();
    ia.auth = AuthMethod::Password { password: "from-a".into() };
    ib.auth = AuthMethod::Password { password: "from-b".into() };
    a.put(Collection::Identities, ident, &ia, Base::Rev(ibase)).unwrap();
    assert!(b.with_vault(|v| v.put(Collection::Identities, ident, &ib, Base::Rev(ibase))).is_err());
    let now = a.get::<Identity>(Collection::Identities, ident).unwrap().data.unwrap();
    assert_eq!(now.auth, AuthMethod::Password { password: "from-a".into() });
    b.lock();

    // ---- An attacker's copy of the folder reveals nothing -----------------
    let stolen = tmp.path().join("attacker");
    copy_tree(&root, &stolen);
    let secret_body: String = sshd
        .client_key
        .lines()
        .filter(|l| !l.starts_with("-----"))
        .collect::<String>()
        .chars()
        .take(40)
        .collect();
    let needles: Vec<Vec<u8>> = [
        "web-01",
        "web-02",
        "db-01",
        "redis-01",
        "Production",
        "production",
        "127.0.0.1",
        "deploy",
        "from-a",
        "patched kernel",
        "OPENSSH PRIVATE KEY",
        secret_body.as_str(),
        &sshd.user,
        std::str::from_utf8(PASSWORD).unwrap(),
        recovery.display().as_str(),
    ]
    .iter()
    .map(|s| s.as_bytes().to_vec())
    .collect();
    for (path, bytes) in all_bytes(&stolen) {
        for n in &needles {
            assert!(
                !bytes.windows(n.len()).any(|w| w == n.as_slice()),
                "{} leaks {:?}",
                path.display(),
                String::from_utf8_lossy(n)
            );
        }
    }
    assert!(matches!(
        Vault::open(&stolen, Unlock::Password(b"guess"), DeviceInfo::new("attacker")),
        Err(VaultError::WrongPassword)
    ));
}

trait TapMkdir {
    fn tap_mkdir(self) -> Self;
}

impl TapMkdir for std::path::PathBuf {
    fn tap_mkdir(self) -> Self {
        std::fs::create_dir_all(&self).unwrap();
        self
    }
}
