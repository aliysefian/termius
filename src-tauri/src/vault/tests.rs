//! Vault v2 tests: cryptography, storage failures, concurrency between two
//! devices, backups, integrity, migration.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::testutil::*;
use super::*;
use crate::crypto::MasterKey;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Host {
    label: String,
    hostname: String,
    port: u16,
    #[serde(default)]
    tags: Vec<String>,
}

fn host(label: &str) -> Host {
    Host {
        label: label.into(),
        hostname: format!("{label}.internal.example"),
        port: 22,
        tags: vec![],
    }
}

/// Every file under `root` with its bytes, to prove something didn't change.
fn snapshot(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(
                    p.strip_prefix(root).unwrap().display().to_string(),
                    fs::read(&p).unwrap(),
                );
            }
        }
    }
    out
}

fn conflict(e: VaultError) -> ConflictError {
    match e {
        VaultError::Conflict(c) => *c,
        other => panic!("expected a conflict, got {other}"),
    }
}

// ---------------------------------------------------------------------------
// Unlocking and key management
// ---------------------------------------------------------------------------

#[test]
fn correct_password_unlocks_and_wrong_fails_without_writing() {
    let (_d, v) = new_vault();
    v.insert(Collection::Hosts, &host("web-01")).unwrap();
    let root = v.root().to_path_buf();
    drop(v);
    let before = snapshot(&root);
    assert!(matches!(
        Vault::open(&root, Unlock::Password(b"wrong"), DeviceInfo::new("PC-B")).unwrap_err(),
        VaultError::WrongPassword
    ));
    assert_eq!(
        snapshot(&root),
        before,
        "a failed unlock must not modify the vault"
    );
    let v = Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("PC-B")).unwrap();
    assert_eq!(v.list::<Host>(Collection::Hosts).unwrap().records.len(), 1);
}

#[test]
fn every_vault_gets_its_own_salt_and_key() {
    let (_a, a) = new_vault();
    let (_b, b) = new_vault();
    let sa = &a.manifest().slots[0];
    let sb = &b.manifest().slots[0];
    assert_ne!(sa.kdf.salt, sb.kdf.salt);
    assert_ne!(sa.wrapped_key, sb.wrapped_key);
    assert!(!a.master_key().ct_eq(b.master_key()));
}

#[test]
fn tampered_metadata_is_detected() {
    let (_d, v) = new_vault();
    let root = v.root().to_path_buf();
    drop(v);
    let path = root.join(MANIFEST_FILE);
    let original: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    let open = || Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("x"));

    // Weaker KDF parameters: the slot AAD no longer matches, unlock fails.
    let mut m = original.clone();
    m["slots"][0]["kdf"]["t_cost"] = json!(1);
    m["slots"][0]["kdf"]["m_cost_kib"] = json!(9);
    fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(open().is_err());

    // Different vault ID.
    let mut m = original.clone();
    m["vault_id"] = json!(uuid::Uuid::new_v4());
    fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(matches!(open().unwrap_err(), VaultError::WrongPassword));

    // Garbage, missing, and a newer format.
    fs::write(&path, b"{not json").unwrap();
    assert!(matches!(
        open().unwrap_err(),
        VaultError::MalformedManifest(..)
    ));
    let mut m = original.clone();
    m["version"] = json!(3);
    fs::write(&path, serde_json::to_vec(&m).unwrap()).unwrap();
    assert!(matches!(
        open().unwrap_err(),
        VaultError::UnsupportedManifestVersion(3)
    ));
    fs::remove_file(&path).unwrap();
    assert!(matches!(open().unwrap_err(), VaultError::NotInitialized(_)));

    fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
    assert!(open().is_ok());
}

#[test]
fn missing_folder_fails_cleanly() {
    let d = tempfile::TempDir::new().unwrap();
    // Like Dropbox not having synced (or the drive being unmounted).
    let err = Vault::open(
        d.path().join("not-there"),
        Unlock::Password(b"x"),
        DeviceInfo::new("x"),
    )
    .unwrap_err();
    assert!(matches!(err, VaultError::NotInitialized(_)));
    assert!(!d.path().join("not-there").exists());
}

#[test]
fn change_password_rewraps_without_touching_records() {
    let (_d, mut v) = new_vault();
    v.insert(Collection::Hosts, &host("web-01")).unwrap();
    let records_before: BTreeMap<_, _> = snapshot(v.root())
        .into_iter()
        .filter(|(k, _)| k.starts_with("hosts"))
        .collect();
    let vmk = v.master_key().clone();

    assert!(matches!(
        v.change_password(b"nope", b"new-pass", kdf()).unwrap_err(),
        VaultError::WrongPassword
    ));
    v.change_password(b"hunter2", b"new-pass", kdf()).unwrap();

    let records_after: BTreeMap<_, _> = snapshot(v.root())
        .into_iter()
        .filter(|(k, _)| k.starts_with("hosts"))
        .collect();
    assert_eq!(
        records_before, records_after,
        "records must not be re-encrypted"
    );
    let root = v.root().to_path_buf();
    drop(v);
    assert!(Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("x")).is_err());
    let v = Vault::open(&root, Unlock::Password(b"new-pass"), DeviceInfo::new("x")).unwrap();
    assert!(v.master_key().ct_eq(&vmk));
}

#[test]
fn interrupted_password_change_keeps_the_old_password() {
    let (_d, mut v) = new_vault();
    atomic::fail_next(atomic::FailAt::BeforeRename);
    assert!(v.change_password(b"hunter2", b"new-pass", kdf()).is_err());
    let root = v.root().to_path_buf();
    drop(v);
    assert!(Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("x")).is_ok());
    assert!(Vault::open(&root, Unlock::Password(b"new-pass"), DeviceInfo::new("x")).is_err());
}

#[test]
fn recovery_key_unlocks_resets_rotates_and_removes() {
    let d = tempfile::TempDir::new().unwrap();
    let root = d.path().join("v");
    let (v, rk) = Vault::create(&root, b"hunter2", opts(true), DeviceInfo::new("A")).unwrap();
    let rk = rk.expect("recovery key requested");
    let shown = rk.display().to_string();
    v.insert(Collection::Hosts, &host("web-01")).unwrap();
    drop(v);

    // The recovery key never appears in the vault folder.
    for bytes in snapshot(&root).values() {
        assert!(!bytes.windows(20).any(|w| w == &shown.as_bytes()[..20]));
    }

    let v = Vault::open(
        &root,
        Unlock::Recovery(&shown.to_lowercase()),
        DeviceInfo::new("B"),
    )
    .unwrap();
    assert_eq!(v.list::<Host>(Collection::Hosts).unwrap().records.len(), 1);
    drop(v);
    assert!(matches!(
        Vault::open(
            &root,
            Unlock::Recovery("0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000-0000"),
            DeviceInfo::new("B")
        )
        .unwrap_err(),
        VaultError::WrongRecoveryKey
    ));
    assert!(matches!(
        Vault::open(&root, Unlock::Recovery("not a key"), DeviceInfo::new("B")).unwrap_err(),
        VaultError::WrongRecoveryKey
    ));

    // Forgot the password: reset through the recovery key.
    let mut v = Vault::reset_password_with_recovery(
        &root,
        &shown,
        b"brand-new",
        kdf(),
        DeviceInfo::new("B"),
    )
    .unwrap();
    assert!(v.check_password(b"brand-new").unwrap());
    assert!(!v.check_password(b"hunter2").unwrap());

    // Rotating makes the old recovery key useless; removing leaves none.
    let rk2 = v.set_recovery(kdf()).unwrap().display().to_string();
    drop(v);
    assert!(Vault::open(&root, Unlock::Recovery(&shown), DeviceInfo::new("B")).is_err());
    let mut v = Vault::open(&root, Unlock::Recovery(&rk2), DeviceInfo::new("B")).unwrap();
    v.remove_recovery().unwrap();
    assert!(!v.has_recovery());
    drop(v);
    assert!(matches!(
        Vault::open(&root, Unlock::Recovery(&rk2), DeviceInfo::new("B")).unwrap_err(),
        VaultError::NoRecoverySlot
    ));
}

#[test]
fn device_key_from_keychain_is_checked() {
    let (_d, v) = new_vault();
    let root = v.root().to_path_buf();
    let k = decode_key(&encode_key(v.master_key())).unwrap();
    drop(v);
    assert!(Vault::open(&root, Unlock::Key(k), DeviceInfo::new("x")).is_ok());
    assert!(matches!(
        Vault::open(
            &root,
            Unlock::Key(MasterKey::generate()),
            DeviceInfo::new("x")
        )
        .unwrap_err(),
        VaultError::KeyMismatch
    ));
}

// ---------------------------------------------------------------------------
// Records, tampering, storage failures
// ---------------------------------------------------------------------------

#[test]
fn tampered_or_transplanted_records_fail_authentication() {
    let (_d, v) = new_vault();
    let r = v.insert(Collection::Hosts, &host("web-01")).unwrap();
    let path = v.record_path(Collection::Hosts, r.id);

    let mut bytes = fs::read(&path).unwrap();
    let n = bytes.len();
    bytes[n - 5] ^= 0x40;
    fs::write(&path, &bytes).unwrap();
    assert!(matches!(
        v.get::<Host>(Collection::Hosts, r.id).unwrap_err(),
        VaultError::Crypto(_)
    ));
    let listing = v.list::<Host>(Collection::Hosts).unwrap();
    assert!(listing.records.is_empty() && listing.skipped.len() == 1);

    // A valid record from another vault with the same collection and ID.
    let (_d2, other) = new_vault();
    other
        .put(Collection::Hosts, r.id, &host("evil"), Base::New)
        .unwrap();
    fs::copy(other.record_path(Collection::Hosts, r.id), &path).unwrap();
    assert!(v.get::<Host>(Collection::Hosts, r.id).is_err());

    // Moved between collections.
    let r2 = v.insert(Collection::Hosts, &host("web-02")).unwrap();
    fs::copy(
        v.record_path(Collection::Hosts, r2.id),
        v.record_path(Collection::Snippets, r2.id),
    )
    .unwrap();
    assert!(v.get::<Value>(Collection::Snippets, r2.id).is_err());
}

#[test]
fn disk_full_and_crash_during_save_leave_the_record_intact() {
    let (_d, v) = new_vault();
    let r = v.insert(Collection::Hosts, &host("web-01")).unwrap();
    for stage in [
        atomic::FailAt::Write,
        atomic::FailAt::Sync,
        atomic::FailAt::BeforeRename,
    ] {
        atomic::fail_next(stage);
        assert!(v
            .put(Collection::Hosts, r.id, &host("changed"), Base::Rev(1))
            .is_err());
        let now = v.get::<Host>(Collection::Hosts, r.id).unwrap();
        assert_eq!(now.data.unwrap().label, "web-01", "{stage:?}");
        assert_eq!(now.rev, 1);
    }
}

#[test]
fn revisions_and_basic_crud() {
    let (_d, v) = new_vault();
    let r = v.insert(Collection::Hosts, &host("web-01")).unwrap();
    assert_eq!(r.rev, 1);
    assert_eq!(r.device_id, v.device().id);
    let r2 = v
        .put(Collection::Hosts, r.id, &host("web-01b"), Base::Rev(1))
        .unwrap();
    assert_eq!(r2.rev, 2);
    assert!(matches!(
        conflict(
            v.put(Collection::Hosts, r.id, &host("dup"), Base::New)
                .unwrap_err()
        )
        .reason,
        ConflictReason::AlreadyExists
    ));
    v.delete(Collection::Hosts, r.id, Base::Rev(2)).unwrap();
    assert!(matches!(
        v.get::<Host>(Collection::Hosts, r.id).unwrap_err(),
        VaultError::Deleted { .. }
    ));
    // Tombstones keep no data.
    let raw = v.read_envelope(Collection::Hosts, r.id).unwrap().unwrap();
    assert!(raw.data.is_none() && raw.prev.as_ref().unwrap().data.is_none());
}

// ---------------------------------------------------------------------------
// Two devices
// ---------------------------------------------------------------------------

#[test]
fn two_devices_adding_different_records_both_survive() {
    let (_d, a) = new_vault();
    let b = second_device(&a, "PC-B");
    a.insert(Collection::Hosts, &host("server-A")).unwrap();
    b.insert(Collection::Hosts, &host("server-B")).unwrap();
    for v in [&a, &b] {
        let mut labels: Vec<_> = v
            .list::<Host>(Collection::Hosts)
            .unwrap()
            .records
            .into_iter()
            .map(|r| r.data.unwrap().label)
            .collect();
        labels.sort();
        assert_eq!(labels, ["server-A", "server-B"]);
    }
    assert_eq!(a.state_hash().unwrap().0, b.state_hash().unwrap().0);
}

#[test]
fn concurrent_edits_merge_or_conflict_never_silently_overwrite() {
    let (_d, a) = new_vault();
    let b = second_device(&a, "PC-B");
    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct Cred {
        label: String,
        username: String,
        auth: Value,
    }
    let cred = |label: &str, pw: &str| Cred {
        label: label.into(),
        username: "root".into(),
        auth: json!({"type": "password", "password": pw}),
    };
    let r = a
        .insert(Collection::Identities, &cred("server-X", "original"))
        .unwrap();

    // Both devices loaded rev 1. A changes the password first.
    a.put(
        Collection::Identities,
        r.id,
        &cred("server-X", "from-A"),
        Base::Rev(1),
    )
    .unwrap();
    // B changes the password too, from rev 1: must be refused, not merged.
    let c = conflict(
        b.put(
            Collection::Identities,
            r.id,
            &cred("server-X", "from-B"),
            Base::Rev(1),
        )
        .unwrap_err(),
    );
    assert_eq!(c.fields, ["auth"]);
    assert_eq!(c.their_device, a.device().id);
    let now = a.get::<Cred>(Collection::Identities, r.id).unwrap();
    assert_eq!(
        now.data.unwrap().auth["password"],
        "from-A",
        "A's credential must survive"
    );

    // B renames only (different field): merges with A's password change.
    let merged = b
        .put(
            Collection::Identities,
            r.id,
            &cred("renamed-by-B", "original"),
            Base::Rev(1),
        )
        .unwrap();
    let data = merged.data.unwrap();
    assert_eq!(data.label, "renamed-by-B");
    assert_eq!(data.auth["password"], "from-A");

    // A deletes from a stale revision: refused.
    assert!(matches!(
        a.delete(Collection::Identities, r.id, Base::Rev(2))
            .unwrap_err(),
        VaultError::Conflict(_)
    ));
    // B deletes; A then edits from its old revision: told it was deleted.
    b.delete(Collection::Identities, r.id, Base::Rev(merged.rev))
        .unwrap();
    let c = conflict(
        a.put(
            Collection::Identities,
            r.id,
            &cred("x", "y"),
            Base::Rev(merged.rev),
        )
        .unwrap_err(),
    );
    assert_eq!(c.reason, ConflictReason::DeletedElsewhere);
}

/// Simulate a sync service keeping B's version as a conflicted copy.
fn make_sync_conflict(
    a: &Vault,
    b: &Vault,
    id: uuid::Uuid,
    a_data: &Host,
    b_data: &Host,
    copy_name: &str,
) {
    let path = a.record_path(Collection::Hosts, id);
    let original = fs::read(&path).unwrap();
    // B saves while offline...
    b.put(Collection::Hosts, id, b_data, Base::Rev(1)).unwrap();
    let b_bytes = fs::read(&path).unwrap();
    // ...A saves offline from the same base...
    fs::write(&path, &original).unwrap();
    a.put(Collection::Hosts, id, a_data, Base::Rev(1)).unwrap();
    // ...and the sync service keeps A's file, saving B's alongside.
    fs::write(a.collection_dir(Collection::Hosts).join(copy_name), b_bytes).unwrap();
}

#[test]
fn sync_conflicted_copies_merge_when_possible_and_ask_when_not() {
    let (_d, a) = new_vault();
    let b = second_device(&a, "PC-B");
    let r = a.insert(Collection::Hosts, &host("web")).unwrap();

    // Different fields: merged automatically, copy removed.
    let mut a_side = host("web");
    a_side.port = 2222;
    let mut b_side = host("web");
    b_side.tags = vec!["prod".into()];
    make_sync_conflict(
        &a,
        &b,
        r.id,
        &a_side,
        &b_side,
        &format!("{} (PC-B's conflicted copy 2026-09-26).enc", r.id),
    );
    let rep = a.reconcile().unwrap();
    assert_eq!((rep.merged, rep.unresolved), (1, 0));
    let now = a
        .get::<Host>(Collection::Hosts, r.id)
        .unwrap()
        .data
        .unwrap();
    assert_eq!(
        (now.port, now.tags.as_slice()),
        (2222, &["prod".to_string()][..])
    );
    assert!(a.conflict_copies().unwrap().is_empty());

    // Same field: left for the user, with both versions shown.
    let r = a.insert(Collection::Hosts, &host("db")).unwrap();
    let mut a_side = host("db");
    a_side.hostname = "10.0.0.1".into();
    let mut b_side = host("db");
    b_side.hostname = "10.0.0.2".into();
    let name = format!("{}.sync-conflict-20260926-120000-ABCDEFG.enc", r.id);
    make_sync_conflict(&a, &b, r.id, &a_side, &b_side, &name);
    assert_eq!(a.reconcile().unwrap().unresolved, 1);
    let cs = a.conflicts().unwrap();
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].fields, ["hostname"]);
    assert_eq!(cs[0].other.data.as_ref().unwrap()["hostname"], "10.0.0.2");

    a.resolve_conflict(
        Collection::Hosts,
        r.id,
        &name,
        conflicts::Resolution::KeepOther,
    )
    .unwrap();
    assert_eq!(
        a.get::<Host>(Collection::Hosts, r.id)
            .unwrap()
            .data
            .unwrap()
            .hostname,
        "10.0.0.2"
    );
    assert!(a.conflicts().unwrap().is_empty());

    // Path tricks are refused.
    assert!(a
        .resolve_conflict(
            Collection::Hosts,
            r.id,
            "../vault.json",
            conflicts::Resolution::KeepCurrent
        )
        .is_err());

    // Identical copy: just removed. Garbage that isn't a real record: left alone.
    let r = a.insert(Collection::Hosts, &host("same")).unwrap();
    let dir = a.collection_dir(Collection::Hosts);
    fs::copy(
        a.record_path(Collection::Hosts, r.id),
        dir.join(format!("{} (1).enc", r.id)),
    )
    .unwrap();
    fs::write(
        dir.join(format!("{}-JUNK.enc", uuid::Uuid::new_v4())),
        b"not a record",
    )
    .unwrap();
    let rep = a.reconcile().unwrap();
    assert_eq!(rep.removed, 1);
    assert!(dir
        .read_dir()
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().ends_with("-JUNK.enc")));
}

#[test]
fn advisory_locks_and_devices() {
    let (_d, a) = new_vault();
    let b = second_device(&a, "PC-B");
    a.register_device("linux", "0.5.0").unwrap();
    b.register_device("windows", "0.5.0").unwrap();
    assert_eq!(a.devices().unwrap().len(), 2);
    a.touch_lock().unwrap();
    let seen = b.active_sessions().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].device_name, "PC-A");
    assert!(
        a.active_sessions().unwrap().is_empty(),
        "own lock isn't reported"
    );
    a.release_lock().unwrap();
    assert!(b.active_sessions().unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Backups
// ---------------------------------------------------------------------------

#[test]
fn backup_restore_verifies_first_and_keeps_a_safety_copy() {
    let (_d, v) = new_vault();
    let keep = v.insert(Collection::Hosts, &host("web-01")).unwrap();
    let b = v.create_backup("manual", 10).unwrap();
    assert_eq!(b.records, 1);

    // Change things after the backup.
    v.put(Collection::Hosts, keep.id, &host("renamed"), Base::Rev(1))
        .unwrap();
    let extra = v.insert(Collection::Hosts, &host("added-later")).unwrap();

    let rep = v.restore_backup(&b.file_name, 10).unwrap();
    assert_eq!((rep.restored, rep.removed), (1, 1));
    assert_eq!(
        v.get::<Host>(Collection::Hosts, keep.id)
            .unwrap()
            .data
            .unwrap()
            .label,
        "web-01"
    );
    assert!(v.get::<Host>(Collection::Hosts, extra.id).is_err());
    // The restore is a new revision, so other devices take it as a change.
    assert!(v.get::<Host>(Collection::Hosts, keep.id).unwrap().rev > 2);
    // And it can be undone.
    assert!(v
        .list_backups()
        .iter()
        .any(|x| x.file_name == rep.safety_backup && x.error.is_none()));
}

#[test]
fn database_connections_are_encrypted_sync_back_up_and_old_vaults_still_open() {
    use crate::models::DbConnection;
    let conn = DbConnection {
        name: "Orders".into(),
        engine: "mysql".into(),
        host: "db-secret-host.internal".into(),
        port: 3306,
        username: "app".into(),
        password: Some("p4ssw0rd-marker".into()),
        database: String::new(),
        tls: Default::default(),
        ssh_host_id: None,
        group: String::new(),
        environment: "production".into(),
        notes: String::new(),
        options: Default::default(),
    };
    let (_d, a) = new_vault();
    let b = second_device(&a, "PC-B");
    let rec = a.insert(Collection::Databases, &conn).unwrap();

    // Nothing readable on disk, not the password and not even the host name.
    let root = a.root().to_path_buf();
    for (name, bytes) in snapshot(&root) {
        let text = String::from_utf8_lossy(&bytes);
        assert!(
            !text.contains("p4ssw0rd-marker") && !text.contains("db-secret-host"),
            "{name} holds the connection in plain text"
        );
    }

    // The other device reads it back whole, password included.
    let got = b.get::<DbConnection>(Collection::Databases, rec.id).unwrap().data.unwrap();
    assert_eq!(got, conn);

    // It is part of backups, and a restore brings it back.
    let backup = a.create_backup("manual", 10).unwrap();
    assert_eq!(backup.records, 1);
    a.delete(Collection::Databases, rec.id, Base::Rev(1)).unwrap();
    a.restore_backup(&backup.file_name, 10).unwrap();
    assert_eq!(a.get::<DbConnection>(Collection::Databases, rec.id).unwrap().data.unwrap(), conn);

    // A vault made before this collection existed has no folder for it.
    // Opening it must work, create the folder, and leave other records alone.
    a.insert(Collection::Hosts, &host("web-01")).unwrap();
    drop(a);
    drop(b);
    std::fs::remove_dir_all(root.join("databases")).unwrap();
    let v = Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("PC-C")).unwrap();
    assert!(root.join("databases").is_dir());
    assert_eq!(v.list::<Host>(Collection::Hosts).unwrap().records.len(), 1);
    assert!(v.list::<DbConnection>(Collection::Databases).unwrap().records.is_empty());
    assert!(v.verify_integrity().errors.is_empty(), "{:?}", v.verify_integrity().errors);
}

#[test]
fn corrupted_or_foreign_backups_are_never_restored() {
    let (_d, v) = new_vault();
    v.insert(Collection::Hosts, &host("web-01")).unwrap();
    let b = v.create_backup("manual", 10).unwrap();
    let path = v.root().join(backup::BACKUP_DIR).join(&b.file_name);
    let mut bytes = fs::read(&path).unwrap();
    bytes[40] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let before = snapshot(v.root());
    assert!(matches!(
        v.restore_backup(&b.file_name, 10).unwrap_err(),
        VaultError::Backup(_)
    ));
    assert_eq!(
        snapshot(v.root()),
        before,
        "a refused restore changes nothing"
    );

    let (_d2, other) = new_vault();
    let ob = other.create_backup("manual", 10).unwrap();
    fs::copy(
        other.root().join(backup::BACKUP_DIR).join(&ob.file_name),
        v.root().join(backup::BACKUP_DIR).join(&ob.file_name),
    )
    .unwrap();
    assert!(v.restore_backup(&ob.file_name, 10).is_err());
    assert!(v.restore_backup("../vault.json", 10).is_err());
}

#[test]
fn backup_retention_prunes_oldest() {
    let (_d, v) = new_vault();
    for _ in 0..5 {
        v.create_backup("manual", 3).unwrap();
    }
    assert_eq!(v.list_backups().len(), 3);
    assert!(
        v.auto_backup(60_000, 3).unwrap().is_none(),
        "recent backup exists"
    );
    assert!(v.auto_backup(0, 3).unwrap().is_some());
}

// ---------------------------------------------------------------------------
// Integrity
// ---------------------------------------------------------------------------

#[test]
fn integrity_report_finds_problems_without_changing_anything() {
    let (_d, v) = new_vault();
    let h = v
        .insert(
            Collection::Hosts,
            &json!({"label": "web", "identity_id": uuid::Uuid::new_v4()}),
        )
        .unwrap();
    v.create_backup("manual", 10).unwrap();
    let rep = v.verify_integrity();
    assert!(rep.ok, "{:?}", rep.errors);
    assert!(
        rep.warnings.iter().any(|w| w.contains("identity_id")),
        "dangling reference is reported"
    );

    let path = v.record_path(Collection::Hosts, h.id);
    let mut bytes = fs::read(&path).unwrap();
    let n = bytes.len();
    bytes[n - 1] ^= 1;
    fs::write(&path, &bytes).unwrap();
    let before = snapshot(v.root());
    let rep = v.verify_integrity();
    assert!(!rep.ok);
    assert!(rep.errors.iter().any(|e| e.contains("hosts/")));
    assert_eq!(snapshot(v.root()), before, "verification must be read-only");
}

// ---------------------------------------------------------------------------
// Migration from v1
// ---------------------------------------------------------------------------

fn v1_vault(root: &Path) -> Vec<uuid::Uuid> {
    let v1 = legacy_v1::Vault::create(root, b"hunter2", kdf()).unwrap();
    let a = v1
        .insert(legacy_v1::Collection::Hosts, &host("web-01"))
        .unwrap();
    let b = v1.insert(legacy_v1::Collection::Identities, &json!({"label": "root", "username": "root", "auth": {"type": "password", "password": "s3cret"}})).unwrap();
    let gone = v1
        .insert(
            legacy_v1::Collection::Snippets,
            &json!({"label": "x", "command": "ls"}),
        )
        .unwrap();
    v1.delete(legacy_v1::Collection::Snippets, gone.id).unwrap();
    vec![a.id, b.id, gone.id]
}

#[test]
fn v1_vaults_migrate_with_a_raw_backup() {
    let d = tempfile::TempDir::new().unwrap();
    let root = d.path().join("v");
    let ids = v1_vault(&root);
    assert!(matches!(
        Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("A")).unwrap_err(),
        VaultError::NeedsMigration
    ));
    let before = snapshot(&root);
    assert!(matches!(
        migrate::migrate_v1(&root, b"wrong", DeviceInfo::new("A"), kdf()).unwrap_err(),
        VaultError::WrongPassword
    ));
    assert_eq!(
        snapshot(&root),
        before,
        "wrong password must not start a migration"
    );

    let v = migrate::migrate_v1(&root, b"hunter2", DeviceInfo::new("A"), kdf()).unwrap();
    assert_eq!(v.manifest().version, 2);
    assert_eq!(
        v.get::<Host>(Collection::Hosts, ids[0])
            .unwrap()
            .data
            .unwrap()
            .label,
        "web-01"
    );
    assert_eq!(
        v.get::<Value>(Collection::Identities, ids[1])
            .unwrap()
            .data
            .unwrap()["auth"]["password"],
        "s3cret"
    );
    assert!(
        v.get_raw::<Value>(Collection::Snippets, ids[2])
            .unwrap()
            .deleted
    );
    assert!(v.verify_integrity().ok);
    // The raw v1 backup is there, and the temporary folders are gone.
    let backups: Vec<_> = fs::read_dir(root.join("backups"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(backups.iter().any(|n| n.starts_with("v1-")));
    for tmp in [migrate::STAGING, migrate::OLD, migrate::JOURNAL] {
        assert!(!root.join(tmp).exists(), "{tmp} left behind");
    }
    drop(v);
    // Reopening with the same password just works.
    assert!(Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("B")).is_ok());
}

#[test]
fn interrupted_migration_rolls_back_and_can_be_retried() {
    let d = tempfile::TempDir::new().unwrap();
    let root = d.path().join("v");
    let ids = v1_vault(&root);
    let dev = DeviceInfo::new("A");
    migrate::CRASH_AFTER_SWAPS.with(|s| s.set(Some(2)));
    assert!(migrate::migrate_v1(&root, b"hunter2", dev.clone(), kdf()).is_err());
    migrate::CRASH_AFTER_SWAPS.with(|s| s.set(None));
    assert!(root.join(migrate::JOURNAL).exists());

    // Another device sees a fresh journal and waits.
    assert!(matches!(
        Vault::open(&root, Unlock::Password(b"hunter2"), DeviceInfo::new("B")).unwrap_err(),
        VaultError::MigrationInProgress { .. }
    ));

    // The same device reopening rolls back to intact v1, then migrates.
    assert!(matches!(
        Vault::open(&root, Unlock::Password(b"hunter2"), dev.clone()).unwrap_err(),
        VaultError::NeedsMigration
    ));
    let v1 = legacy_v1::Vault::open(&root, b"hunter2").unwrap();
    assert_eq!(
        v1.list::<Host>(legacy_v1::Collection::Hosts)
            .unwrap()
            .records
            .len(),
        1
    );
    drop(v1);
    let v = migrate::migrate_v1(&root, b"hunter2", dev, kdf()).unwrap();
    assert_eq!(
        v.get::<Host>(Collection::Hosts, ids[0])
            .unwrap()
            .data
            .unwrap()
            .label,
        "web-01"
    );
}

// ---------------------------------------------------------------------------
// What an attacker with the folder sees
// ---------------------------------------------------------------------------

#[test]
fn the_folder_reveals_no_inventory_or_secrets() {
    let d = tempfile::TempDir::new().unwrap();
    let root = d.path().join("v");
    let (v, rk) = Vault::create(
        &root,
        b"correct horse battery staple",
        opts(true),
        DeviceInfo::new("Ali's laptop"),
    )
    .unwrap();
    v.insert(
        Collection::Hosts,
        &json!({"label": "payment-db-01", "hostname": "db.payments.corp"}),
    )
    .unwrap();
    v.insert(Collection::Identities, &json!({"label": "root", "username": "deploy", "auth": {"type": "password", "password": "Sup3r-S3cret-Pw"}})).unwrap();
    v.insert(
        Collection::Keys,
        &json!({"private_key": "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAA\n"}),
    )
    .unwrap();
    v.insert(
        Collection::Snippets,
        &json!({"command": "mysql -p'DbPassw0rd!'"}),
    )
    .unwrap();
    v.register_device("linux", "0.5").unwrap();
    v.touch_lock().unwrap();
    v.create_backup("manual", 10).unwrap();
    let rk = rk.unwrap().display().to_string();
    drop(v);

    let needles: [&[u8]; 10] = [
        b"payment-db-01",
        b"db.payments.corp",
        b"deploy",
        b"Sup3r-S3cret-Pw",
        b"OPENSSH PRIVATE KEY",
        b"b3BlbnNzaC1rZXktdjEAAAAA",
        b"DbPassw0rd",
        b"correct horse",
        b"Ali's laptop",
        &rk.as_bytes()[..24],
    ];
    for (file, bytes) in snapshot(&root) {
        for n in needles {
            assert!(
                !bytes.windows(n.len()).any(|w| w == n),
                "{file} contains {:?}",
                String::from_utf8_lossy(n)
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn symlinks_in_the_folder_are_never_followed() {
    let (_d, v) = new_vault();
    let id = v.insert(Collection::Hosts, &host("real")).unwrap().id;
    // A planted link in place of a record, and one as a new "record".
    let victim = v.root().join("..").join("outside.txt");
    fs::write(&victim, b"not yours").unwrap();
    let path = v.record_path(Collection::Hosts, id);
    fs::remove_file(&path).unwrap();
    std::os::unix::fs::symlink(&victim, &path).unwrap();
    let planted = v.record_path(Collection::Hosts, uuid::Uuid::new_v4());
    std::os::unix::fs::symlink("/dev/zero", &planted).unwrap();

    assert!(v.get::<Host>(Collection::Hosts, id).is_err());
    let listing = v.list::<Host>(Collection::Hosts).unwrap();
    assert!(listing.records.is_empty());
    assert_eq!(listing.skipped.len(), 2, "both links reported, neither read");
}
