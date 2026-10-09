//! The record of runbook runs, kept on this computer: one file per run, the newest hundred. Outputs can hold
//! secrets, so it is never synced or backed up with the vault.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runbook::StepResult;

/// Runs kept in the record.
pub const KEEP_RUNS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostRecord {
    pub host_id: Uuid,
    pub label: String,
    /// `None` while it runs.
    pub ok: Option<bool>,
    pub error: Option<String>,
    pub steps: Vec<StepResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub runbook: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub scheduled: bool,
    pub cancelled: bool,
    /// The values the run was given (text parameters; a file parameter shows only that one was chosen).
    pub params: HashMap<String, String>,
    pub hosts: Vec<HostRecord>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunSummary {
    pub id: String,
    pub runbook: String,
    pub started_at: u64,
    pub scheduled: bool,
    pub cancelled: bool,
    pub running: bool,
    pub hosts: usize,
    pub ok: usize,
    pub failed: usize,
}

impl RunRecord {
    pub fn summary(&self) -> RunSummary {
        RunSummary {
            id: self.id.clone(),
            runbook: self.runbook.clone(),
            started_at: self.started_at,
            scheduled: self.scheduled,
            cancelled: self.cancelled,
            running: self.finished_at.is_none(),
            hosts: self.hosts.len(),
            ok: self.hosts.iter().filter(|h| h.ok == Some(true)).count(),
            failed: self.hosts.iter().filter(|h| h.ok == Some(false)).count(),
        }
    }
}

/// A copy of the record that is safe to keep: command output, notes, errors and secret-looking parameters are masked.
pub fn masked(r: &RunRecord) -> RunRecord {
    use crate::mask::{is_secret_name, secrets, HIDDEN};
    let mut m = r.clone();
    for (name, value) in m.params.iter_mut() {
        *value = if is_secret_name(name) && !value.is_empty() { HIDDEN.to_string() } else { secrets(value) };
    }
    for h in &mut m.hosts {
        h.error = h.error.as_deref().map(secrets);
        for step in &mut h.steps {
            step.output.stdout = secrets(&step.output.stdout);
            step.output.stderr = secrets(&step.output.stderr);
            step.note = secrets(&step.note);
        }
    }
    m
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The runs kept on this computer, one file each.
#[derive(Clone)]
pub struct History {
    dir: PathBuf,
}

impl History {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, r: &RunRecord) -> PathBuf {
        self.dir.join(format!("{:012}-{}.json", r.started_at, r.id))
    }

    pub fn save(&self, r: &RunRecord) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let path = self.path(r);
        let tmp = path.with_extension("tmp");
        // What is kept on disk has secrets in the output hidden (the run view the person watched is unchanged).
        std::fs::write(&tmp, serde_json::to_vec(&masked(r)).map_err(std::io::Error::other)?)?;
        std::fs::rename(&tmp, &path)?;
        self.prune();
        Ok(())
    }

    fn files(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect())
            .unwrap_or_default();
        files.sort();
        files
    }

    fn prune(&self) {
        let files = self.files();
        for old in files.iter().take(files.len().saturating_sub(KEEP_RUNS)) {
            let _ = std::fs::remove_file(old);
        }
    }

    /// Newest first.
    pub fn list(&self) -> Vec<RunSummary> {
        self.files()
            .iter()
            .rev()
            .filter_map(|p| std::fs::read(p).ok().and_then(|b| serde_json::from_slice::<RunRecord>(&b).ok()))
            .map(|r| r.summary())
            .collect()
    }

    fn find(&self, id: &str) -> Option<PathBuf> {
        // An id is a UUID; anything else can't name a file here.
        Uuid::parse_str(id).ok()?;
        self.files().into_iter().find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().ends_with(&format!("-{id}.json"))))
    }

    pub fn get(&self, id: &str) -> Option<RunRecord> {
        serde_json::from_slice(&std::fs::read(self.find(id)?).ok()?).ok()
    }

    pub fn delete(&self, id: &str) -> bool {
        self.find(id).is_some_and(|p| std::fs::remove_file(p).is_ok())
    }

    pub fn clear(&self) -> usize {
        self.files().iter().filter(|p| std::fs::remove_file(p).is_ok()).count()
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::runbook::{Output, StepStatus};

    fn record(n: u64, ok: &[Option<bool>]) -> RunRecord {
        RunRecord {
            id: Uuid::new_v4().to_string(),
            runbook: format!("run {n}"),
            started_at: n,
            finished_at: Some(n + 1),
            scheduled: false,
            cancelled: false,
            params: HashMap::from([("service".to_string(), "nginx".to_string())]),
            hosts: ok
                .iter()
                .map(|o| HostRecord { host_id: Uuid::new_v4(), label: "h".into(), ok: *o, error: None, steps: vec![StepResult { index: 0, name: "s".into(), status: StepStatus::Ok, output: Output::default(), note: String::new() }] })
                .collect(),
        }
    }

    #[test]
    fn secrets_in_output_are_hidden_in_what_is_kept() {
        let dir = tempfile::TempDir::new().unwrap();
        let h = History::new(dir.path().join("history"));
        let mut r = record(1, &[Some(true)]);
        r.params.insert("db_password".into(), "hunter2".into());
        r.params.insert("note".into(), "uses token=abc123".into());
        r.hosts[0].error = Some("fetch failed: https://bob:pa55@db/x".into());
        let step = &mut r.hosts[0].steps[0];
        step.output.stdout = "DB_PASSWORD=hunter2\nok\n-----BEGIN RSA PRIVATE KEY-----\nAAAA\n-----END RSA PRIVATE KEY-----\n".into();
        step.output.stderr = "curl: Authorization: Bearer abc.def".into();
        step.note = "token=zzz".into();
        h.save(&r).unwrap();

        let kept = h.get(&r.id).unwrap();
        let text = serde_json::to_string(&kept).unwrap();
        for leaked in ["hunter2", "pa55", "abc123", "abc.def", "AAAA", "zzz"] {
            assert!(!text.contains(leaked), "{leaked} was kept: {text}");
        }
        assert!(kept.hosts[0].steps[0].output.stdout.contains("DB_PASSWORD=[hidden]\nok\n"));
        assert_eq!(kept.params["db_password"], "[hidden]");
        assert_eq!(kept.params["service"], "nginx", "ordinary values stay");
        // The record the caller holds, which the live view was drawn from, is not changed.
        assert!(r.hosts[0].steps[0].output.stdout.contains("hunter2"));
    }

    #[test]
    fn a_run_is_saved_listed_read_and_deleted() {
        let dir = tempfile::TempDir::new().unwrap();
        let h = History::new(dir.path().join("history"));
        assert!(h.list().is_empty());
        let r = record(10, &[Some(true), Some(false), Some(true)]);
        h.save(&r).unwrap();
        let list = h.list();
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].hosts, list[0].ok, list[0].failed, list[0].running), (3, 2, 1, false));
        assert_eq!(h.get(&r.id).unwrap().hosts.len(), 3);
        assert!(h.delete(&r.id));
        assert!(h.get(&r.id).is_none() && !h.delete(&r.id));
        // No temporary file is left behind.
        h.save(&r).unwrap();
        assert!(std::fs::read_dir(h.dir()).unwrap().all(|e| e.unwrap().path().extension().is_some_and(|x| x == "json")));
    }

    #[test]
    fn newest_first_and_only_the_newest_hundred_are_kept() {
        let dir = tempfile::TempDir::new().unwrap();
        let h = History::new(dir.path());
        for n in 1..=(KEEP_RUNS as u64 + 5) {
            h.save(&record(n, &[Some(true)])).unwrap();
        }
        let list = h.list();
        assert_eq!(list.len(), KEEP_RUNS);
        assert_eq!(list[0].started_at, KEEP_RUNS as u64 + 5);
        assert_eq!(list.last().unwrap().started_at, 6, "the five oldest went");
        assert_eq!(h.clear(), KEEP_RUNS);
        assert!(h.list().is_empty());
    }

    #[test]
    fn an_id_that_is_not_a_uuid_names_nothing() {
        let dir = tempfile::TempDir::new().unwrap();
        let h = History::new(dir.path());
        let r = record(1, &[Some(true)]);
        h.save(&r).unwrap();
        for bad in ["", "..", "../x", "*", "1", &format!("{}/..", r.id)] {
            assert!(h.get(bad).is_none(), "{bad:?}");
            assert!(!h.delete(bad), "{bad:?}");
        }
        assert!(h.get(&r.id).is_some());
    }

    #[test]
    fn a_run_that_is_still_going_says_so_and_a_damaged_file_is_skipped() {
        let dir = tempfile::TempDir::new().unwrap();
        let h = History::new(dir.path());
        let mut r = record(1, &[None]);
        r.finished_at = None;
        h.save(&r).unwrap();
        std::fs::write(dir.path().join("000000000002-not-a-run.json"), "{ nope").unwrap();
        let list = h.list();
        assert_eq!(list.len(), 1);
        assert!(list[0].running);
    }
}
