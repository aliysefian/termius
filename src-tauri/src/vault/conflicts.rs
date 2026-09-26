//! Sync-service conflicted copies.
//!
//! When two computers save the same record while offline, the sync service
//! keeps one file under the real name and saves the other as a "conflicted
//! copy", named differently by each service:
//!
//! | Service | Example |
//! |---|---|
//! | Dropbox | `<uuid> (Ali's conflicted copy 2026-09-26).enc` |
//! | Nextcloud / ownCloud | `<uuid> (conflicted copy 2026-09-26 120000).enc` |
//! | Google Drive | `<uuid> (1).enc` |
//! | OneDrive | `<uuid>-DESKTOP-1234.enc` |
//! | Syncthing | `<uuid>.sync-conflict-20260926-120000-ABCDEFG.enc` |
//!
//! Rather than matching each pattern, any file that starts with a record's
//! UUID, ends in `.enc`, and isn't the canonical name is a candidate. It only
//! counts if it decrypts and authenticates as that record, so random clutter
//! is never mistaken for data.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::{merge, Collection, Envelope, Prev, Record, Result, Vault, VaultError, RECORD_EXT};

/// If `path` looks like a conflicted copy, the record ID it belongs to.
pub fn copy_id(path: &Path) -> Option<Uuid> {
    let name = path.file_name()?.to_str()?;
    if name.starts_with('.') || !name.ends_with(&format!(".{RECORD_EXT}")) || name.len() <= 36 + 4 {
        return None;
    }
    let id = Uuid::parse_str(name.get(..36)?).ok()?;
    (name != format!("{}.{RECORD_EXT}", id.as_hyphenated())).then_some(id)
}

#[derive(Debug, Clone)]
pub struct ConflictCopy {
    pub collection: Collection,
    pub id: Uuid,
    pub path: PathBuf,
}

impl ConflictCopy {
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// An unresolved conflict, for the UI.
#[derive(Debug, Clone, Serialize)]
pub struct ConflictInfo {
    pub collection: Collection,
    pub id: Uuid,
    /// The copy's file name, used to resolve it.
    pub file_name: String,
    pub current: Record<Value>,
    pub other: Record<Value>,
    /// Top-level fields that differ.
    pub fields: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReconcileReport {
    /// Merged automatically (different fields changed).
    pub merged: usize,
    /// Identical or outdated copies removed.
    pub removed: usize,
    /// Left for the user.
    pub unresolved: usize,
}

pub enum Resolution {
    KeepCurrent,
    KeepOther,
    /// A hand-merged version.
    Use(Value),
}

fn same_content(a: &Envelope, b: &Envelope) -> bool {
    a.content_hash == b.content_hash
}

/// `older` is exactly the previous version of `newer`.
fn is_parent(older: &Envelope, newer: &Envelope) -> bool {
    newer.prev.as_ref().is_some_and(|p| {
        p.rev == older.rev && p.deleted == older.deleted && (older.deleted || p.data == older.data)
    })
}

impl Vault {
    /// Every conflicted copy in the synced collections.
    pub fn conflict_copies(&self) -> Result<Vec<ConflictCopy>> {
        let mut out = Vec::new();
        for c in Collection::ALL.into_iter().filter(|c| c.is_synced_data()) {
            let dir = self.collection_dir(c);
            let Ok(entries) = fs::read_dir(&dir) else {
                continue;
            };
            for e in entries.flatten() {
                let path = e.path();
                if let Some(id) = copy_id(&path) {
                    out.push(ConflictCopy {
                        collection: c,
                        id,
                        path,
                    });
                }
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(out)
    }

    fn read_copy(&self, cc: &ConflictCopy) -> Result<Envelope> {
        let bytes = super::read_regular_file(&cc.path, 16 * 1024 * 1024).map_err(|e| VaultError::io(&cc.path, e))?;
        self.decode_envelope(cc.collection, cc.id, &cc.path, &bytes)
    }

    fn remove_copy(&self, cc: &ConflictCopy) -> Result<()> {
        fs::remove_file(&cc.path).map_err(|e| VaultError::io(&cc.path, e))
    }

    fn supersede(
        &self,
        c: Collection,
        main: Option<&Envelope>,
        id: Uuid,
        data: Option<Value>,
        rev_floor: u64,
    ) -> Result<Envelope> {
        let prev = main.filter(|_| data.is_some()).map(|m| Prev {
            rev: m.rev,
            deleted: m.deleted,
            data: m.data.clone(),
        });
        let base = main.map(|m| m.rev).unwrap_or(0);
        let env = self.new_envelope(id, rev_floor.max(base) + 1, base, data, prev);
        self.write_envelope(c, &env)?;
        Ok(env)
    }

    /// Resolve what can be resolved safely: identical copies, outdated
    /// copies, and edits to different fields. Everything else is left for the
    /// user. Unreadable files are never deleted.
    pub fn reconcile(&self) -> Result<ReconcileReport> {
        let mut report = ReconcileReport::default();
        for cc in self.conflict_copies()? {
            let Ok(copy) = self.read_copy(&cc) else {
                continue;
            };
            let main = self.read_envelope(cc.collection, cc.id)?;
            let Some(main) = main else {
                // Only the copy survived: it becomes the record.
                self.write_envelope(cc.collection, &copy)?;
                self.remove_copy(&cc)?;
                report.merged += 1;
                continue;
            };
            if same_content(&main, &copy) || is_parent(&copy, &main) {
                self.remove_copy(&cc)?;
                report.removed += 1;
            } else if is_parent(&main, &copy) {
                self.supersede(
                    cc.collection,
                    Some(&main),
                    cc.id,
                    copy.data.clone(),
                    copy.rev,
                )?;
                self.remove_copy(&cc)?;
                report.merged += 1;
            } else {
                // Siblings of a common parent can be merged field by field.
                let merged = match (&main.prev, &copy.prev) {
                    (Some(a), Some(b))
                        if a.rev == b.rev
                            && a.data == b.data
                            && !a.deleted
                            && !main.deleted
                            && !copy.deleted =>
                    {
                        let base = a.data.clone().unwrap_or(Value::Null);
                        merge::merge3(
                            &base,
                            main.data.as_ref().unwrap_or(&Value::Null),
                            copy.data.as_ref().unwrap_or(&Value::Null),
                        )
                        .ok()
                    }
                    _ => None,
                };
                match merged {
                    Some(v) => {
                        self.supersede(cc.collection, Some(&main), cc.id, Some(v), copy.rev)?;
                        self.remove_copy(&cc)?;
                        report.merged += 1;
                    }
                    None => report.unresolved += 1,
                }
            }
        }
        Ok(report)
    }

    /// Conflicts that need a decision.
    pub fn conflicts(&self) -> Result<Vec<ConflictInfo>> {
        let mut out = Vec::new();
        for cc in self.conflict_copies()? {
            let Ok(copy) = self.read_copy(&cc) else {
                continue;
            };
            let Some(main) = self.read_envelope(cc.collection, cc.id)? else {
                continue;
            };
            let fields = differing_fields(
                main.data.as_ref(),
                copy.data.as_ref(),
                main.deleted,
                copy.deleted,
            );
            out.push(ConflictInfo {
                collection: cc.collection,
                id: cc.id,
                file_name: cc.file_name(),
                current: main_record(&main),
                other: main_record(&copy),
                fields,
            });
        }
        Ok(out)
    }

    /// Apply the user's choice and remove the copy.
    pub fn resolve_conflict(
        &self,
        c: Collection,
        id: Uuid,
        file_name: &str,
        choice: Resolution,
    ) -> Result<()> {
        // Only a bare file name inside this collection, belonging to `id`.
        if file_name.contains(['/', '\\']) || file_name.contains("..") {
            return Err(VaultError::InvalidRecordFile {
                path: file_name.into(),
            });
        }
        let path = self.collection_dir(c).join(file_name);
        if copy_id(&path) != Some(id) {
            return Err(VaultError::InvalidRecordFile { path });
        }
        let cc = ConflictCopy {
            collection: c,
            id,
            path,
        };
        let copy = self.read_copy(&cc)?;
        let main = self.read_envelope(c, id)?;
        let data = match choice {
            Resolution::KeepCurrent => main.as_ref().and_then(|m| m.data.clone()),
            Resolution::KeepOther => copy.data.clone(),
            Resolution::Use(v) => Some(v),
        };
        self.supersede(c, main.as_ref(), id, data, copy.rev)?;
        self.remove_copy(&cc)
    }
}

fn main_record(e: &Envelope) -> Record<Value> {
    Record {
        id: e.id,
        rev: e.rev,
        updated_at: e.updated_at,
        deleted: e.deleted,
        device_id: e.device_id,
        data: e.data.clone(),
    }
}

fn differing_fields(a: Option<&Value>, b: Option<&Value>, a_del: bool, b_del: bool) -> Vec<String> {
    if a_del != b_del {
        return vec!["(deleted on one side)".into()];
    }
    match (a, b) {
        (Some(Value::Object(x)), Some(Value::Object(y))) => {
            let mut keys: Vec<&String> = x.keys().chain(y.keys()).collect();
            keys.sort();
            keys.dedup();
            keys.into_iter()
                .filter(|k| x.get(*k) != y.get(*k))
                .cloned()
                .collect()
        }
        _ if a == b => vec![],
        _ => vec!["(whole record)".into()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_every_services_naming() {
        let id = Uuid::new_v4();
        for name in [
            format!("{id} (Ali's conflicted copy 2026-09-26).enc"),
            format!("{id} (conflicted copy 2026-09-26 120000).enc"),
            format!("{id} (1).enc"),
            format!("{id}(1).enc"),
            format!("{id}-DESKTOP-1234.enc"),
            format!("{id}.sync-conflict-20260926-120000-ABCDEFG.enc"),
        ] {
            assert_eq!(copy_id(Path::new(&name)), Some(id), "{name}");
        }
        for name in [
            format!("{id}.enc"),
            format!(".{id}.enc.1234.tmp"),
            "notes.txt".to_string(),
            format!("{id} (1).txt"),
            "short.enc".to_string(),
        ] {
            assert_eq!(copy_id(Path::new(&name)), None, "{name}");
        }
    }
}
