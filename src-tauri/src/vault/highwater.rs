//! Noticing that the synced folder went backwards.
//!
//! Every record carries a revision that only goes up. A device that remembers the newest revision it has seen of each
//! record can tell when the folder later shows an older one, or loses a live record altogether: a sync service
//! restoring an old copy, a bad merge by the sync tool, or someone with access to the folder replaying an older (still
//! validly encrypted) file. The memory is kept on this device, outside the synced folder, so the folder can't rewrite it.
//!
//! This only reports. Nothing is blocked or repaired automatically: an old copy restored on purpose looks the same,
//! so the person decides, and can accept the current state as the new baseline.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct Seen {
    rev: u64,
    deleted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AnomalyKind {
    /// The record shows revision `now`, but revision `seen` was seen before.
    Older { seen: u64, now: u64 },
    /// A live record last seen at revision `seen` is gone from the folder (deleting leaves a marker, so this isn't one).
    Missing { seen: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Anomaly {
    pub collection: String,
    pub id: Uuid,
    #[serde(flatten)]
    pub kind: AnomalyKind,
}

#[derive(Debug)]
pub struct HighWater {
    path: PathBuf,
    seen: BTreeMap<String, Seen>,
    found: Vec<Anomaly>,
}

fn key(collection: &str, id: Uuid) -> String {
    format!("{collection}/{id}")
}

impl HighWater {
    /// Load from `path`. A missing or unreadable file is an empty memory: the next reads become the baseline.
    pub fn load(path: PathBuf) -> Self {
        let seen = std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self { path, seen, found: Vec::new() }
    }

    /// A record was read from the folder at `rev`.
    pub fn observe(&mut self, collection: &str, id: Uuid, rev: u64, deleted: bool) {
        let k = key(collection, id);
        match self.seen.get(&k) {
            Some(prev) if rev < prev.rev => {
                let anomaly = Anomaly { collection: collection.into(), id, kind: AnomalyKind::Older { seen: prev.rev, now: rev } };
                if !self.found.contains(&anomaly) {
                    self.found.retain(|a| !(a.collection == collection && a.id == id));
                    self.found.push(anomaly);
                }
            }
            Some(prev) if rev == prev.rev => self.found.retain(|a| !(a.collection == collection && a.id == id)),
            _ => {
                self.seen.insert(k, Seen { rev, deleted });
                // A record that is back at (or past) what was seen is no longer a problem.
                self.found.retain(|a| !(a.collection == collection && a.id == id));
            }
        }
    }

    /// After a full listing: live records seen before that are not in the folder any more.
    pub fn note_missing(&mut self, present: &BTreeSet<String>) {
        self.found.retain(|a| !matches!(a.kind, AnomalyKind::Missing { .. }));
        let gone: Vec<Anomaly> = self
            .seen
            .iter()
            .filter(|(k, s)| !s.deleted && !present.contains(*k))
            .filter_map(|(k, s)| {
                let (collection, id) = k.split_once('/')?;
                Some(Anomaly { collection: collection.into(), id: id.parse().ok()?, kind: AnomalyKind::Missing { seen: s.rev } })
            })
            .collect();
        self.found.extend(gone);
    }

    pub fn anomalies(&self) -> Vec<Anomaly> {
        self.found.clone()
    }

    /// Take the folder as it is now as the baseline, forgetting what was seen before.
    pub fn accept(&mut self, current: impl IntoIterator<Item = (String, Uuid, u64, bool)>) {
        self.seen = current.into_iter().map(|(c, id, rev, deleted)| (key(&c, id), Seen { rev, deleted })).collect();
        self.found.clear();
    }

    /// Write the memory next to the app's config. Best effort: a failure only means less memory next time.
    pub fn save(&self) -> std::io::Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec(&self.seen).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, &self.path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hw() -> (tempfile::TempDir, HighWater) {
        let d = tempfile::tempdir().unwrap();
        let h = HighWater::load(d.path().join("hw.json"));
        (d, h)
    }

    #[test]
    fn going_up_is_fine_and_going_down_is_reported_once() {
        let (_d, mut h) = hw();
        let id = Uuid::new_v4();
        h.observe("hosts", id, 1, false);
        h.observe("hosts", id, 3, false);
        assert!(h.anomalies().is_empty());
        h.observe("hosts", id, 2, false);
        h.observe("hosts", id, 2, false);
        assert_eq!(h.anomalies(), vec![Anomaly { collection: "hosts".into(), id, kind: AnomalyKind::Older { seen: 3, now: 2 } }]);
        h.observe("hosts", id, 3, false);
        assert!(h.anomalies().is_empty(), "back at what was seen: no longer a problem");
    }

    #[test]
    fn a_vanished_live_record_is_reported_but_a_deleted_one_is_not() {
        let (_d, mut h) = hw();
        let (live, dead, kept) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        h.observe("hosts", live, 2, false);
        h.observe("hosts", dead, 4, true);
        h.observe("hosts", kept, 1, false);
        h.note_missing(&BTreeSet::from([key("hosts", kept)]));
        assert_eq!(h.anomalies(), vec![Anomaly { collection: "hosts".into(), id: live, kind: AnomalyKind::Missing { seen: 2 } }]);
        h.note_missing(&BTreeSet::from([key("hosts", kept), key("hosts", live)]));
        assert!(h.anomalies().is_empty(), "it came back");
    }

    #[test]
    fn the_memory_survives_a_restart_and_accepting_resets_it() {
        let (d, mut h) = hw();
        let id = Uuid::new_v4();
        h.observe("snippets", id, 5, false);
        h.save().unwrap();
        let mut again = HighWater::load(d.path().join("hw.json"));
        again.observe("snippets", id, 4, false);
        assert_eq!(again.anomalies().len(), 1, "the rollback is noticed after a restart");
        again.accept([("snippets".to_string(), id, 4, false)]);
        assert!(again.anomalies().is_empty());
        again.observe("snippets", id, 4, false);
        assert!(again.anomalies().is_empty(), "revision 4 is the baseline now");
    }

    #[test]
    fn a_damaged_file_means_an_empty_memory_not_a_failure() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("hw.json"), b"{nope").unwrap();
        let mut h = HighWater::load(d.path().join("hw.json"));
        h.observe("hosts", Uuid::new_v4(), 1, false);
        assert!(h.anomalies().is_empty());
    }
}
