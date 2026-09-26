//! "Verify vault integrity": read-only checks of everything on disk.

use std::collections::{HashMap, HashSet};
use std::fs;

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::{atomic, backup, conflicts, format, Collection, Vault};

#[derive(Debug, Clone, Default, Serialize)]
pub struct IntegrityReport {
    /// No errors (warnings are allowed).
    pub ok: bool,
    pub format_version: u32,
    pub records_checked: usize,
    pub backups_ok: usize,
    pub backups_bad: usize,
    pub conflict_copies: usize,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

/// Reference fields: (collection holding the field, JSON pointer, target).
const REFERENCES: &[(Collection, &str, Collection)] = &[
    (Collection::Hosts, "/identity_id", Collection::Identities),
    (Collection::Hosts, "/jump_host_id", Collection::Hosts),
    (Collection::Hosts, "/proxy_id", Collection::Proxies),
    (Collection::Identities, "/auth/key_id", Collection::Keys),
    (Collection::Forwards, "/host_id", Collection::Hosts),
    (
        Collection::Groups,
        "/default_identity_id",
        Collection::Identities,
    ),
    (
        Collection::Groups,
        "/default_jump_host_id",
        Collection::Hosts,
    ),
    (Collection::Groups, "/proxy_id", Collection::Proxies),
];

impl Vault {
    /// Check metadata, every record, references and backups. Never writes.
    pub fn verify_integrity(&self) -> IntegrityReport {
        let mut r = IntegrityReport {
            format_version: self.manifest().version,
            ..Default::default()
        };

        // Metadata on disk still matches what we unlocked.
        match format::read(self.root()) {
            Ok(format::OnDisk::V2(m)) => {
                if m.vault_id != self.vault_id() {
                    r.errors
                        .push("vault.json now describes a different vault".into());
                } else if !format::verify_key(&m, self.master_key()) {
                    r.errors
                        .push("vault.json key check fails with the unlocked key".into());
                }
            }
            Ok(format::OnDisk::V1) => r
                .errors
                .push("vault.json was replaced by an old-format manifest".into()),
            Err(e) => r.errors.push(format!("vault.json: {e}")),
        }

        // Every record file.
        let mut live: HashMap<Collection, HashSet<Uuid>> = HashMap::new();
        let mut data: Vec<(Collection, Uuid, Value)> = Vec::new();
        for c in Collection::ALL {
            let dir = self.collection_dir(c);
            let Ok(entries) = fs::read_dir(&dir) else {
                r.warnings.push(format!("{c}/ is missing"));
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some((_, id)) = self.parse_record_path(&path) {
                    r.records_checked += 1;
                    match super::read_regular_file(&path, 16 * 1024 * 1024) {
                        Err(err) => r.errors.push(format!("{c}/{name}: {err}")),
                        Ok(bytes) => match self.decode_envelope(c, id, &path, &bytes) {
                            Err(err) => r.errors.push(format!("{c}/{name}: {err}")),
                            Ok(env) => {
                                if env.rev == 0 || env.base_rev >= env.rev {
                                    r.errors.push(format!(
                                        "{c}/{name}: invalid revision {} (base {})",
                                        env.rev, env.base_rev
                                    ));
                                }
                                if env.deleted != env.data.is_none() {
                                    r.errors.push(format!(
                                        "{c}/{name}: deleted flag doesn't match its data"
                                    ));
                                }
                                if !env.deleted {
                                    live.entry(c).or_default().insert(id);
                                    if let Some(v) = env.data {
                                        data.push((c, id, v));
                                    }
                                }
                            }
                        },
                    }
                } else if conflicts::copy_id(&path).is_some() {
                    r.conflict_copies += 1;
                } else if atomic::is_temp_name(&name) {
                    r.warnings
                        .push(format!("{c}/{name}: leftover temporary file"));
                } else {
                    r.warnings.push(format!("{c}/{name}: unexpected file"));
                }
            }
        }
        if r.conflict_copies > 0 {
            r.warnings.push(format!(
                "{} sync conflict(s) waiting to be resolved",
                r.conflict_copies
            ));
        }

        // References between records.
        for (c, id, v) in &data {
            for (src, pointer, target) in REFERENCES {
                if src != c {
                    continue;
                }
                if let Some(Value::String(s)) = v.pointer(pointer) {
                    let ok = Uuid::parse_str(s)
                        .ok()
                        .is_some_and(|t| live.get(target).is_some_and(|set| set.contains(&t)));
                    if !ok {
                        r.warnings.push(format!(
                            "{c}/{id}: {pointer} points to a missing {target} record"
                        ));
                    }
                }
            }
        }

        // Backups.
        for b in self.list_backups() {
            match b.error {
                None => r.backups_ok += 1,
                Some(e) => {
                    r.backups_bad += 1;
                    r.errors
                        .push(format!("{}/{}: {e}", backup::BACKUP_DIR, b.file_name));
                }
            }
        }

        r.ok = r.errors.is_empty();
        r
    }
}
