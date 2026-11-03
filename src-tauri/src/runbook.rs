//! Runbooks: a saved sequence of steps with parameters, run on one host or many, with a record of what happened.
//!
//! A runbook is a JSON or YAML document you can read and diff, not a program: steps run a command, wait until a command
//! succeeds (or prints something), upload or download a file the person picked, pause, check a web address or a port,
//! or run a Docker operation. Whether a step runs can depend on an earlier step's exit code or on a parameter, and
//! nothing else. Everything here is pure or goes through the [`Executor`] trait, so the logic is tested without a
//! network, and one test uses a real SSH server.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::containers::transport::shell_quote;

pub const MAX_STEPS: usize = 100;
pub const MAX_PARAMS: usize = 20;
pub const MAX_COMMAND: usize = 8192;
pub const MAX_VALUE: usize = 4096;
/// Output kept per step in the record of a run.
pub const KEEP_OUTPUT: usize = 16 * 1024;
pub const DEFAULT_STEP_TIMEOUT: u64 = 120;
pub const MAX_STEP_TIMEOUT: u64 = 3600;

// -- the document -------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Runbook {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Where it sits in the library ("Deployments", "Maintenance", or one of the person's own). Only for sorting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// What a step that doesn't say otherwise does, and how the runbook is usually run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<Defaults>,
    #[serde(default)]
    pub params: Vec<Param>,
    pub steps: Vec<Step>,
    /// Steps to undo what the run did, tried on a host where the run failed, only when the person turned rollback on for that
    /// run. Written like `steps` (and may look at the earlier ids); never run on their own.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rollback: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub name: String,
    #[serde(default)]
    pub label: String,
    /// What it is for, shown under the field when the runbook is run.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Without a default (and not `optional`), a value must be given.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub optional: bool,
    /// If present, the value must be one of these.
    #[serde(default)]
    pub choices: Vec<String>,
    /// "text" (default), "number", "boolean", "file" (a file chosen on this computer, usable only as an upload's source),
    /// "dir" (a folder chosen on this computer, usable only as a download's destination) or "secret".
    #[serde(default)]
    pub kind: ParamKind,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    #[default]
    Text,
    /// A number (any finite decimal), checked when the runbook is run.
    Number,
    /// `true` or `false`.
    Boolean,
    File,
    /// A folder on this computer that a download saves into; chosen when the runbook runs, like a file.
    Dir,
    /// Typed when the runbook runs: shown as dots, never kept in a schedule, hidden in the record of the run and wherever
    /// it turns up in a command's output.
    Secret,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Step {
    #[serde(default)]
    pub name: String,
    /// Lets later steps and conditions refer to this one.
    #[serde(default)]
    pub id: Option<String>,
    /// A step turned off stays in the document but is skipped when it runs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub wait: Option<Wait>,
    #[serde(default)]
    pub upload: Option<Upload>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub download: Option<Download>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay: Option<Delay>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<Http>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tcp: Option<Tcp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker: Option<Docker>,
    #[serde(default)]
    pub when: Option<When>,
    /// What a failed step does: stop this host (default) or carry on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_error: Option<OnError>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Try a failed `run` or `upload` again up to this many more times (at most 5). Only for steps that are safe to
    /// repeat: nothing checks that.
    #[serde(default)]
    pub retries: Option<u32>,
    /// Seconds to wait between tries (default 5, at most 300).
    #[serde(default)]
    pub retry_delay_secs: Option<u64>,
}

impl Step {
    /// The kinds of work this step names (it must name exactly one).
    pub fn kinds(&self) -> Vec<&'static str> {
        let mut k = Vec::new();
        for (present, name) in [
            (self.run.is_some(), "run"),
            (self.wait.is_some(), "wait"),
            (self.upload.is_some(), "upload"),
            (self.download.is_some(), "download"),
            (self.delay.is_some(), "delay"),
            (self.http.is_some(), "http"),
            (self.tcp.is_some(), "tcp"),
            (self.docker.is_some(), "docker"),
        ] {
            if present {
                k.push(name);
            }
        }
        k
    }

    pub fn on_error_or_stop(&self) -> OnError {
        self.on_error.unwrap_or_default()
    }
}

/// What a step that doesn't say otherwise does. Written once, applied to every step that has no value of its own.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Defaults {
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub retries: Option<u32>,
    #[serde(default)]
    pub retry_delay_secs: Option<u64>,
    #[serde(default)]
    pub on_error: Option<OnError>,
    /// How the hosts are taken when the runbook is run (the person can change it for a run).
    #[serde(default)]
    pub strategy: Option<Strategy>,
    /// The hosts it is usually run on.
    #[serde(default)]
    pub hosts: Vec<Uuid>,
    /// Whether to tell the person when a run ends: "always", "failure" (default) or "never".
    #[serde(default)]
    pub notify: Option<Notify>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Notify {
    Always,
    #[default]
    Failure,
    Never,
}

/// How a run takes its hosts. `Parallel`, and `Sequential` with `stop_on_failure`, are what runs have always been; the others
/// add a limit on how many at once, and rolling batches. This is about hosts. The steps of one host always run in order.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Strategy {
    /// All at once, up to eight at a time.
    #[default]
    Parallel,
    /// At most `limit` at once. With `stop_on_failure`, the first host that fails stops the run: hosts not yet started are skipped
    /// and hosts running stop after their current step.
    Concurrent {
        limit: usize,
        #[serde(default)]
        stop_on_failure: bool,
    },
    /// One after another, in the order given.
    Sequential {
        #[serde(default)]
        stop_on_failure: bool,
    },
    /// In batches of `batch_size`, one batch after another, with up to `concurrency` (default the whole batch) at once inside it.
    /// With `stop_on_failure`, a batch with a failed host is the last one.
    Rolling {
        batch_size: usize,
        #[serde(default)]
        concurrency: Option<usize>,
        #[serde(default)]
        stop_on_failure: bool,
    },
}

pub const MAX_CONCURRENCY: usize = 32;

impl Strategy {
    pub fn check(&self, hosts: usize) -> Result<(), String> {
        match *self {
            Strategy::Concurrent { limit, .. } if !(1..=MAX_CONCURRENCY).contains(&limit) => Err(format!("the limit must be between 1 and {MAX_CONCURRENCY}")),
            Strategy::Rolling { batch_size, concurrency, .. } => {
                if !(1..=MAX_CONCURRENCY).contains(&batch_size) {
                    return Err(format!("the batch size must be between 1 and {MAX_CONCURRENCY}"));
                }
                if concurrency.is_some_and(|c| !(1..=batch_size).contains(&c)) {
                    return Err("the concurrency must be between 1 and the batch size".into());
                }
                let _ = hosts;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

impl From<crate::runner::Order> for Strategy {
    fn from(o: crate::runner::Order) -> Self {
        match o {
            crate::runner::Order::Parallel => Strategy::Parallel,
            crate::runner::Order::Sequential { stop_on_failure } => Strategy::Sequential { stop_on_failure },
        }
    }
}

/// Pause for `secs`, a fixed time.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Delay {
    pub secs: u64,
}

pub const MAX_DELAY: u64 = 3600;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    #[default]
    Get,
    Head,
}

/// Ask a web address, from this computer, and expect a status (default any 2xx or 3xx) and, if given, some text in the page.
/// With `wait_secs` it asks again every `every_secs` until it passes or that time is up.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Http {
    pub url: String,
    #[serde(default)]
    pub method: HttpMethod,
    #[serde(default)]
    pub expect_status: Option<u16>,
    #[serde(default)]
    pub contains: Option<String>,
    /// The longest one request may take (default 10, at most 120).
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub every_secs: Option<u64>,
    #[serde(default)]
    pub wait_secs: Option<u64>,
}

/// Open a TCP connection from this computer, then close it. `host` is the host the step runs on when left out.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Tcp {
    #[serde(default)]
    pub host: Option<String>,
    pub port: u16,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    #[serde(default)]
    pub every_secs: Option<u64>,
    #[serde(default)]
    pub wait_secs: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    /// A file with that name is replaced.
    Overwrite,
    /// The file is left as it is and the step says so.
    Skip,
    /// The new file gets a number added to its name.
    #[default]
    Rename,
}

/// Copy a file from the host into the folder chosen for the parameter `to`. It is saved as `<host>__<file name>`, so hosts
/// don't overwrite each other, and never anywhere else.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Download {
    pub remote: String,
    /// `{{name}}` of a parameter whose kind is "dir".
    pub to: String,
    #[serde(default)]
    pub on_conflict: Conflict,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DockerAction {
    Start,
    Stop,
    Restart,
    Pull,
    /// Run `command` inside the container (through `sh -c`).
    Exec,
    /// `docker compose -f <file> up -d`.
    ComposeUp,
    /// `docker compose -f <file> down`.
    ComposeDown,
}

/// A Docker operation on the host. It is turned into one `docker` command with every value quoted, shown in the dry run, and run
/// like any other command. `sudo` is off unless written; it runs `sudo -n`, which fails rather than ask for a password.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Docker {
    pub action: DockerAction,
    /// The container (start, stop, restart, exec).
    #[serde(default)]
    pub container: Option<String>,
    /// The image (pull).
    #[serde(default)]
    pub image: Option<String>,
    /// The compose file on the host (compose_up, compose_down).
    #[serde(default)]
    pub file: Option<String>,
    /// A compose project name.
    #[serde(default)]
    pub project: Option<String>,
    /// What to run inside the container (exec).
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub sudo: bool,
}

/// The most extra tries a step may ask for.
pub const MAX_RETRIES: u32 = 5;
pub const DEFAULT_RETRY_DELAY: u64 = 5;
pub const MAX_RETRY_DELAY: u64 = 300;

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OnError {
    #[default]
    Stop,
    Continue,
}

/// Run `run` every `every_secs` until it exits with `until_exit` (default 0) and, if given, prints `contains`.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Wait {
    pub run: String,
    #[serde(default)]
    pub until_exit: Option<u32>,
    #[serde(default)]
    pub contains: Option<String>,
    #[serde(default)]
    pub every_secs: Option<u64>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// Put the file chosen for the parameter `local` at `remote` (written beside it, then moved into place).
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Upload {
    /// `{{name}}` of a parameter whose kind is "file".
    pub local: String,
    pub remote: String,
    /// Octal, like "0644".
    #[serde(default)]
    pub mode: Option<String>,
}

/// Run only if this is true: an earlier step's exit code, or a parameter's value.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct When {
    pub step: Option<String>,
    pub exit: Option<u32>,
    pub exit_not: Option<u32>,
    pub param: Option<String>,
    pub equals: Option<String>,
    pub not_equals: Option<String>,
}

// -- checking it ----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Problem {
    /// The step it is about (0-based), or none for the runbook as a whole.
    pub step: Option<usize>,
    /// Where in the text the parser gave up, for a document that couldn't be read at all (1-based).
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub message: String,
}

fn problem(step: Option<usize>, message: impl Into<String>) -> Problem {
    Problem { step, line: None, column: None, message: message.into() }
}

/// Names given by the app itself, usable in any template.
pub const BUILTINS: [&str; 2] = ["host", "label"];

fn valid_name(n: &str) -> bool {
    let mut chars = n.chars();
    matches!(chars.next(), Some('a'..='z')) && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && n.len() <= 40
}

/// The `{{name}}` / `{{name|q}}` placeholders in a template, as (name, quoted).
pub fn placeholders(template: &str) -> Result<Vec<(String, bool)>, String> {
    let mut out = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after = &rest[start + 2..];
        let Some(end) = after.find("}}") else {
            return Err("a {{ is never closed".into());
        };
        let inner = after[..end].trim();
        let (name, filter) = match inner.split_once('|') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (inner, None),
        };
        let quoted = match filter {
            None => false,
            Some("q") => true,
            Some("raw") => false,
            Some(other) => return Err(format!("the filter |{other} doesn't exist (|q quotes the value for the shell, |raw puts it in as it is)")),
        };
        if !valid_name(name) {
            return Err(format!("{{{{{inner}}}}} isn't a valid name (lowercase letters, digits and _)"));
        }
        out.push((name.to_string(), quoted));
        rest = &after[end + 2..];
    }
    Ok(out)
}

/// Parse a runbook's text and check everything that can be checked before it runs. The text is JSON or YAML (a document that
/// starts with `{` is JSON).
pub fn parse(text: &str) -> Result<Runbook, Vec<Problem>> {
    let rb = read(text)?;
    let problems = check(&rb);
    if problems.is_empty() {
        Ok(rb)
    } else {
        Err(problems)
    }
}

/// Whether the text is written as JSON (else YAML).
pub fn is_json(text: &str) -> bool {
    text.trim_start().starts_with('{')
}

/// Read the document without checking it any further than its shape.
pub fn read(text: &str) -> Result<Runbook, Vec<Problem>> {
    if is_json(text) {
        serde_json::from_str(text).map_err(|e| {
            vec![Problem { step: None, line: Some(e.line()), column: Some(e.column()), message: format!("not a valid runbook: {}", shape_message(&e.to_string())) }]
        })
    } else {
        serde_yaml_ng::from_str(text).map_err(|e| {
            let loc = e.location();
            vec![Problem { step: None, line: loc.as_ref().map(|l| l.line()), column: loc.as_ref().map(|l| l.column()), message: format!("not a valid runbook: {}", shape_message(&e.to_string())) }]
        })
    }
}

/// The parser's own words, with the position they end in cut off (the position is reported beside the message).
fn shape_message(m: &str) -> String {
    let m = m.trim();
    for marker in [" at line ", " at position "] {
        if let Some(i) = m.find(marker) {
            return m[..i].to_string();
        }
    }
    m.to_string()
}

pub fn check(rb: &Runbook) -> Vec<Problem> {
    if rb.rollback.is_empty() {
        return check_steps(rb);
    }
    // The rollback steps are checked as a continuation of the steps (same parameters, ids unique across both, a `when` may look
    // at an earlier step), and what is found about them is reported against the rollback.
    let n = rb.steps.len();
    let combined = Runbook { steps: [rb.steps.clone(), rb.rollback.clone()].concat(), rollback: Vec::new(), ..rb.clone() };
    let mut out = check_steps(&combined);
    if rb.steps.is_empty() {
        out.push(problem(None, "a runbook needs at least one step before its rollback"));
    }
    for p in &mut out {
        if let Some(i) = p.step.filter(|i| *i >= n) {
            p.message = format!("Rollback step {}: {}", i - n + 1, p.message);
            p.step = None;
        }
    }
    out
}

fn check_steps(rb: &Runbook) -> Vec<Problem> {
    let mut out = Vec::new();
    if rb.name.trim().is_empty() || rb.name.len() > 100 {
        out.push(problem(None, "the runbook needs a name of up to 100 characters"));
    }
    if rb.category.as_deref().is_some_and(|c| c.trim().is_empty() || c.len() > 40 || c.contains('\n')) {
        out.push(problem(None, "the category is up to 40 characters on one line"));
    }
    if let Some(d) = &rb.defaults {
        if d.timeout_secs.is_some_and(|t| !(1..=MAX_STEP_TIMEOUT).contains(&t)) {
            out.push(problem(None, format!("defaults: timeout_secs must be between 1 and {MAX_STEP_TIMEOUT}")));
        }
        if d.retries.is_some_and(|r| r > MAX_RETRIES) {
            out.push(problem(None, format!("defaults: retries can be at most {MAX_RETRIES}")));
        }
        if d.retry_delay_secs.is_some_and(|x| !(1..=MAX_RETRY_DELAY).contains(&x)) {
            out.push(problem(None, format!("defaults: retry_delay_secs must be between 1 and {MAX_RETRY_DELAY}")));
        }
        if let Some(strategy) = &d.strategy {
            if let Err(e) = strategy.check(d.hosts.len()) {
                out.push(problem(None, format!("defaults: {e}")));
            }
        }
        if d.hosts.len() > 500 {
            out.push(problem(None, "defaults: more than 500 hosts"));
        }
    }
    if rb.steps.is_empty() {
        out.push(problem(None, "a runbook needs at least one step"));
    }
    if rb.steps.len() > MAX_STEPS {
        out.push(problem(None, format!("more than {MAX_STEPS} steps")));
    }
    if rb.params.len() > MAX_PARAMS {
        out.push(problem(None, format!("more than {MAX_PARAMS} parameters")));
    }
    let mut params: HashMap<&str, &Param> = HashMap::new();
    for p in &rb.params {
        if !valid_name(&p.name) {
            out.push(problem(None, format!("the parameter name \"{}\" isn't valid (lowercase letters, digits and _, starting with a letter)", p.name)));
        } else if BUILTINS.contains(&p.name.as_str()) {
            out.push(problem(None, format!("\"{}\" is given by the app and can't be a parameter", p.name)));
        } else if params.insert(&p.name, p).is_some() {
            out.push(problem(None, format!("the parameter \"{}\" is listed twice", p.name)));
        }
        if let Some(d) = &p.default {
            if !p.choices.is_empty() && !p.choices.contains(d) {
                out.push(problem(None, format!("the default of \"{}\" isn't one of its choices", p.name)));
            }
            if matches!(p.kind, ParamKind::File | ParamKind::Dir) {
                out.push(problem(None, format!("a {} parameter (\"{}\") can't have a default: it is chosen when the runbook runs", kind_word(p.kind), p.name)));
            }
            if let Err(e) = check_value_kind(p, d) {
                out.push(problem(None, format!("the default of \"{}\" {e}", p.name)));
            }
        }
        if matches!(p.kind, ParamKind::File | ParamKind::Dir | ParamKind::Boolean) && !p.choices.is_empty() {
            out.push(problem(None, format!("a {} parameter (\"{}\") can't have choices", kind_word(p.kind), p.name)));
        }
        if p.kind == ParamKind::Secret && (p.default.is_some() || !p.choices.is_empty()) {
            out.push(problem(None, format!("a secret parameter (\"{}\") can't have a default or choices: it is typed when the runbook runs, and never written down", p.name)));
        }
    }
    let known = |name: &str| BUILTINS.contains(&name) || params.contains_key(name);
    let mut ids: HashSet<&str> = HashSet::new();
    let mut earlier: HashSet<&str> = HashSet::new();
    for (i, s) in rb.steps.iter().enumerate() {
        let at = Some(i);
        if s.kinds().len() != 1 {
            out.push(problem(at, "a step has exactly one of run, wait, upload, download, delay, http, tcp or docker"));
        }
        let mut templates: Vec<(&str, &str)> = Vec::new();
        if let Some(r) = &s.run {
            templates.push(("the command", r));
        }
        if let Some(w) = &s.wait {
            templates.push(("the command to wait on", &w.run));
            if w.every_secs.is_some_and(|e| !(1..=3600).contains(&e)) {
                out.push(problem(at, "every_secs must be between 1 and 3600"));
            }
            if w.timeout_secs.is_some_and(|t| !(1..=MAX_STEP_TIMEOUT).contains(&t)) {
                out.push(problem(at, format!("timeout_secs must be between 1 and {MAX_STEP_TIMEOUT}")));
            }
            if w.contains.as_deref().is_some_and(str::is_empty) {
                out.push(problem(at, "contains can't be empty"));
            }
        }
        if let Some(u) = &s.upload {
            templates.push(("the remote path", &u.remote));
            match placeholders(&u.local) {
                Ok(p) if p.len() == 1 && u.local.trim() == format!("{{{{{}}}}}", p[0].0) => match params.get(p[0].0.as_str()) {
                    Some(param) if param.kind == ParamKind::File => {}
                    _ => out.push(problem(at, format!("upload's local file must be {{{{name}}}} of a parameter with kind \"file\" ({} isn't one)", p[0].0))),
                },
                _ => out.push(problem(at, "upload's local file must be exactly {{name}} of a parameter with kind \"file\", so the file is chosen when the runbook runs and a runbook can't pick files by itself")),
            }
            if u.remote.trim().is_empty() || u.remote.contains('\n') || u.remote.contains('\0') {
                out.push(problem(at, "upload needs a remote path on one line"));
            }
            if let Some(m) = &u.mode {
                if !(3..=4).contains(&m.len()) || !m.chars().all(|c| ('0'..='7').contains(&c)) {
                    out.push(problem(at, "mode is octal, like \"0644\""));
                }
            }
        }
        if let Some(d) = &s.download {
            templates.push(("the remote path", &d.remote));
            match placeholders(&d.to) {
                Ok(p) if p.len() == 1 && d.to.trim() == format!("{{{{{}}}}}", p[0].0) => match params.get(p[0].0.as_str()) {
                    Some(param) if param.kind == ParamKind::Dir => {}
                    _ => out.push(problem(at, format!("download's destination must be {{{{name}}}} of a parameter with kind \"dir\" ({} isn't one)", p[0].0))),
                },
                _ => out.push(problem(at, "download's destination must be exactly {{name}} of a parameter with kind \"dir\", so the folder is chosen when the runbook runs and a runbook can't write anywhere by itself")),
            }
            if d.remote.trim().is_empty() || d.remote.contains('\n') || d.remote.contains('\0') {
                out.push(problem(at, "download needs a remote path on one line"));
            }
        }
        if let Some(d) = &s.delay {
            if !(1..=MAX_DELAY).contains(&d.secs) {
                out.push(problem(at, format!("delay's secs must be between 1 and {MAX_DELAY}")));
            }
        }
        if let Some(h) = &s.http {
            templates.push(("the web address", &h.url));
            if h.expect_status.is_some_and(|c| c != 0 && !(100..=599).contains(&c)) {
                out.push(problem(at, "expect_status is a status code like 200 (0 for any 2xx or 3xx)"));
            }
            if h.contains.as_deref().is_some_and(str::is_empty) {
                out.push(problem(at, "contains can't be empty"));
            }
            check_poll(&mut out, at, h.timeout_secs, h.every_secs, h.wait_secs);
            if !h.url.contains("{{") && !crate::monitoring::http::valid_url(&h.url) {
                out.push(problem(at, "the web address must start with http:// or https://"));
            }
        }
        if let Some(t) = &s.tcp {
            if t.port == 0 {
                out.push(problem(at, "the port must be between 1 and 65535"));
            }
            if let Some(h) = &t.host {
                templates.push(("the host", h));
            }
            check_poll(&mut out, at, t.timeout_secs, t.every_secs, t.wait_secs);
        }
        if let Some(d) = &s.docker {
            for (what, v) in [("container", &d.container), ("image", &d.image), ("file", &d.file), ("project", &d.project), ("command", &d.command)] {
                if let Some(v) = v {
                    templates.push((what, v));
                }
            }
            if let Err(e) = docker_fields(d) {
                out.push(problem(at, e));
            }
        }
        for (what, t) in templates {
            if t.trim().is_empty() {
                out.push(problem(at, format!("{what} is empty")));
            }
            if t.len() > MAX_COMMAND {
                out.push(problem(at, format!("{what} is longer than {MAX_COMMAND} characters")));
            }
            match placeholders(t) {
                Ok(list) => {
                    for (name, _) in list {
                        if !known(&name) {
                            out.push(problem(at, format!("{{{{{name}}}}} isn't a parameter of this runbook")));
                        } else if params.get(name.as_str()).is_some_and(|p| matches!(p.kind, ParamKind::File | ParamKind::Dir)) {
                            let kind = params[name.as_str()].kind;
                            out.push(problem(at, format!("{{{{{name}}}}} is a {} parameter; only {} can use it", kind_word(kind), if kind == ParamKind::File { "an upload" } else { "a download" })));
                        }
                    }
                }
                Err(e) => out.push(problem(at, e)),
            }
        }
        if s.timeout_secs.is_some_and(|t| !(1..=MAX_STEP_TIMEOUT).contains(&t)) {
            out.push(problem(at, format!("timeout_secs must be between 1 and {MAX_STEP_TIMEOUT}")));
        }
        if s.retries.is_some_and(|r| r > MAX_RETRIES) {
            out.push(problem(at, format!("retries can be at most {MAX_RETRIES}")));
        }
        if s.retry_delay_secs.is_some_and(|d| !(1..=MAX_RETRY_DELAY).contains(&d)) {
            out.push(problem(at, format!("retry_delay_secs must be between 1 and {MAX_RETRY_DELAY}")));
        }
        if (s.retries.is_some() || s.retry_delay_secs.is_some()) && (s.wait.is_some() || s.delay.is_some()) {
            out.push(problem(at, "retries don't apply to a wait step or a delay step: a wait already repeats until it is ready or time runs out"));
        }
        if let Some(id) = &s.id {
            if !valid_name(id) {
                out.push(problem(at, format!("the id \"{id}\" isn't valid (lowercase letters, digits and _)")));
            } else if !ids.insert(id) {
                out.push(problem(at, format!("the id \"{id}\" is used twice")));
            }
        }
        if let Some(w) = &s.when {
            let by_step = w.step.is_some() || w.exit.is_some() || w.exit_not.is_some();
            let by_param = w.param.is_some() || w.equals.is_some() || w.not_equals.is_some();
            if by_step == by_param {
                out.push(problem(at, "when is either {step, exit or exit_not} or {param, equals or not_equals}"));
            } else if by_step {
                match &w.step {
                    Some(id) if earlier.contains(id.as_str()) => {}
                    Some(id) => out.push(problem(at, format!("when refers to the step \"{id}\", which isn't an earlier step with that id"))),
                    None => out.push(problem(at, "when needs the step it looks at")),
                }
                if w.exit.is_some() == w.exit_not.is_some() {
                    out.push(problem(at, "when has exactly one of exit and exit_not"));
                }
            } else {
                match w.param.as_deref().and_then(|p| params.get(p)) {
                    Some(p) if matches!(p.kind, ParamKind::Text | ParamKind::Number | ParamKind::Boolean) => {}
                    _ => out.push(problem(at, "when's param must be a text, number or boolean parameter of this runbook")),
                }
                if w.equals.is_some() == w.not_equals.is_some() {
                    out.push(problem(at, "when has exactly one of equals and not_equals"));
                }
            }
        }
        if let Some(id) = &s.id {
            earlier.insert(id);
        }
    }
    out
}

fn kind_word(k: ParamKind) -> &'static str {
    match k {
        ParamKind::Text => "text",
        ParamKind::Number => "number",
        ParamKind::Boolean => "boolean",
        ParamKind::File => "file",
        ParamKind::Dir => "dir",
        ParamKind::Secret => "secret",
    }
}

/// The checks every polling step (http, tcp) shares.
fn check_poll(out: &mut Vec<Problem>, at: Option<usize>, timeout: Option<u64>, every: Option<u64>, wait: Option<u64>) {
    if timeout.is_some_and(|t| !(1..=120).contains(&t)) {
        out.push(problem(at, "timeout_secs (for one try) must be between 1 and 120"));
    }
    if every.is_some_and(|e| !(1..=3600).contains(&e)) {
        out.push(problem(at, "every_secs must be between 1 and 3600"));
    }
    if wait.is_some_and(|w| !(1..=MAX_STEP_TIMEOUT).contains(&w)) {
        out.push(problem(at, format!("wait_secs must be between 1 and {MAX_STEP_TIMEOUT}")));
    }
}

/// Whether `value` suits the parameter's kind (a number is a finite decimal, a boolean is true or false).
fn check_value_kind(p: &Param, value: &str) -> Result<(), &'static str> {
    match p.kind {
        ParamKind::Number if value.trim().parse::<f64>().map_or(true, |n| !n.is_finite()) => Err("isn't a number"),
        ParamKind::Boolean if value != "true" && value != "false" => Err("must be true or false"),
        _ => Ok(()),
    }
}

/// Which fields a Docker action needs, and that no other is given.
fn docker_fields(d: &Docker) -> Result<(), String> {
    use DockerAction::*;
    let (needs, allows): (&[&str], &[&str]) = match d.action {
        Start | Stop | Restart => (&["container"], &["container"]),
        Pull => (&["image"], &["image"]),
        Exec => (&["container", "command"], &["container", "command"]),
        ComposeUp | ComposeDown => (&["file"], &["file", "project"]),
    };
    let given = [("container", d.container.is_some()), ("image", d.image.is_some()), ("file", d.file.is_some()), ("project", d.project.is_some()), ("command", d.command.is_some())];
    for n in needs {
        if !given.iter().any(|(k, g)| k == n && *g) {
            return Err(format!("docker {:?} needs {n}", d.action).to_lowercase());
        }
    }
    for (k, g) in given {
        if g && !allows.contains(&k) {
            return Err(format!("docker {:?} doesn't use {k}", d.action).to_lowercase());
        }
    }
    Ok(())
}

// -- parameters and templates -------------------------------------------------------------

/// The values to run with: given ones, else defaults; refused if a required one is missing or not allowed.
pub fn resolve_params(rb: &Runbook, given: &HashMap<String, String>) -> Result<HashMap<String, String>, Vec<String>> {
    let mut out = HashMap::new();
    let mut errors = Vec::new();
    for p in &rb.params {
        let value = given.get(&p.name).map(|v| v.trim().to_string()).filter(|v| !v.is_empty()).or_else(|| p.default.clone());
        let label = if p.label.is_empty() { &p.name } else { &p.label };
        match value {
            None if p.optional => {
                out.insert(p.name.clone(), String::new());
            }
            None => errors.push(format!("{label} needs a value")),
            Some(v) if v.len() > MAX_VALUE || v.contains('\0') => errors.push(format!("{label} isn't usable")),
            Some(v) if !p.choices.is_empty() && !p.choices.contains(&v) => errors.push(format!("{label} must be one of: {}", p.choices.join(", "))),
            Some(v) if check_value_kind(p, &v).is_err() => errors.push(format!("{label} {}", check_value_kind(p, &v).err().unwrap_or("isn't usable"))),
            Some(v) => {
                out.insert(p.name.clone(), v);
            }
        }
    }
    for k in given.keys() {
        if !rb.params.iter().any(|p| &p.name == k) {
            errors.push(format!("{k} isn't a parameter of this runbook"));
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// Fill in a template. `{{name|q}}` is quoted for the shell and `{{name|raw}}` is put in as it is. A bare `{{name}}` is
/// put in as it is for a parameter (the person who runs the runbook typed it), but the app's own `{{host}}` and
/// `{{label}}` come from host records that an import or a synced vault can fill, so they are quoted unless `|raw`.
pub fn render(template: &str, values: &HashMap<String, String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or("a {{ is never closed")?;
        let inner = after[..end].trim();
        let (name, filter) = match inner.split_once('|') {
            Some((n, f)) => (n.trim(), Some(f.trim())),
            None => (inner, None),
        };
        let quoted = match filter {
            Some("raw") => false,
            Some(_) => true,
            None => BUILTINS.contains(&name),
        };
        let value = values.get(name).ok_or_else(|| format!("no value for {name}"))?;
        out.push_str(&if quoted { shell_quote(value) } else { value.clone() });
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Fill in a template for a place that is not a shell (a web address, a host name): every `{{name}}` is put in as it is.
pub fn render_plain(template: &str, values: &HashMap<String, String>) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("}}").ok_or("a {{ is never closed")?;
        let inner = after[..end].trim();
        let name = inner.split_once('|').map_or(inner, |(n, _)| n.trim());
        out.push_str(values.get(name).ok_or_else(|| format!("no value for {name}"))?);
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    Ok(out)
}

/// A container or project name as Docker takes it.
fn docker_name_ok(n: &str) -> bool {
    let mut c = n.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphanumeric()) && c.all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c)) && n.len() <= 128
}

/// An image reference: registry, name, tag or digest.
fn docker_image_ok(n: &str) -> bool {
    !n.is_empty() && n.len() <= 255 && !n.starts_with(['-', '/']) && n.chars().all(|c| c.is_ascii_alphanumeric() || "._/:@-".contains(c))
}

/// The one command a Docker step runs, with every value checked and quoted. It adds the usual install folders to PATH (a login-less
/// SSH command lacks them) and, only when the step says `sudo`, `sudo -n` in front.
pub fn docker_command(d: &Docker, values: &HashMap<String, String>) -> Result<String, String> {
    use DockerAction::*;
    let get = |v: &Option<String>| -> Result<Option<String>, String> { v.as_deref().map(|t| render_plain(t, values)).transpose() };
    let container = get(&d.container)?;
    let image = get(&d.image)?;
    let file = get(&d.file)?;
    let project = get(&d.project)?;
    let command = get(&d.command)?;
    let need = |v: &Option<String>, what: &str| v.clone().ok_or_else(|| format!("docker {:?} needs {what}", d.action).to_lowercase());
    let name = |v: String, what: &str| if docker_name_ok(&v) { Ok(v) } else { Err(format!("\"{v}\" isn't a valid {what} name")) };
    let mut args: Vec<String> = Vec::new();
    match d.action {
        Start | Stop | Restart => {
            args.push(format!("{:?}", d.action).to_lowercase());
            args.push("--".into());
            args.push(name(need(&container, "container")?, "container")?);
        }
        Pull => {
            let i = need(&image, "image")?;
            if !docker_image_ok(&i) {
                return Err(format!("\"{i}\" isn't a valid image name"));
            }
            args.extend(["pull".into(), "--".into(), i]);
        }
        Exec => {
            let c = name(need(&container, "container")?, "container")?;
            let cmd = need(&command, "command")?;
            if cmd.trim().is_empty() {
                return Err("the command to run in the container is empty".into());
            }
            args.extend(["exec".into(), c, "sh".into(), "-c".into(), cmd]);
        }
        ComposeUp | ComposeDown => {
            let f = need(&file, "file")?;
            if f.trim().is_empty() || f.contains('\n') || f.contains('\0') {
                return Err("the compose file needs a path on one line".into());
            }
            args.extend(["compose".into(), "-f".into(), f]);
            if let Some(p) = project {
                args.extend(["-p".into(), name(p, "project")?]);
            }
            if d.action == ComposeUp {
                args.extend(["up".into(), "-d".into()]);
            } else {
                args.push("down".into());
            }
        }
    }
    let command = crate::containers::transport::remote_command("docker", &args);
    Ok(if d.sudo { format!("sudo -n {command}") } else { command })
}

/// The runbook with its `defaults` written into every step that has no value of its own, so a step is judged the same
/// whether it says a thing or the defaults do.
pub fn with_defaults(rb: &Runbook) -> Runbook {
    let mut rb = rb.clone();
    let Some(d) = rb.defaults.clone() else { return rb };
    for s in rb.steps.iter_mut().chain(rb.rollback.iter_mut()) {
        if s.timeout_secs.is_none() {
            s.timeout_secs = d.timeout_secs;
        }
        // A wait and a delay already time themselves, and retries don't apply to them.
        if s.wait.is_none() && s.delay.is_none() {
            if s.retries.is_none() {
                s.retries = d.retries;
            }
            if s.retry_delay_secs.is_none() {
                s.retry_delay_secs = d.retry_delay_secs;
            }
        }
        if s.on_error.is_none() {
            s.on_error = d.on_error;
        }
    }
    rb
}

/// A step as it will run for one host, for a dry run.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlannedStep {
    /// "run", or "rollback" for the steps that only run on a host where the run failed, when rollback is on.
    pub phase: Phase,
    pub index: usize,
    pub name: String,
    /// run, wait, upload, download, delay, http, tcp or docker.
    pub kind: &'static str,
    /// The command, the wait's command, or a sentence for the other kinds ("upload <file> → <remote>").
    pub text: String,
    /// What it depends on, in words.
    pub condition: Option<String>,
    pub on_error: &'static str,
    /// The step is switched off and will be skipped.
    pub disabled: bool,
    /// It acts on this computer, not on the host: a file upload reads from here, a web or port check is made from here.
    pub local: bool,
}

pub fn describe_when(w: &When) -> String {
    match (&w.step, w.exit, w.exit_not, &w.param, &w.equals, &w.not_equals) {
        (Some(s), Some(e), ..) => format!("only if step {s} exited with {e}"),
        (Some(s), _, Some(e), ..) => format!("only if step {s} did not exit with {e}"),
        (_, _, _, Some(p), Some(v), _) => format!("only if {p} is {v}"),
        (_, _, _, Some(p), _, Some(v)) => format!("only if {p} is not {v}"),
        _ => "conditional".into(),
    }
}

/// How a poll step (http, tcp) reads in the plan.
fn poll_text(every: Option<u64>, wait: Option<u64>) -> String {
    match wait {
        Some(w) => format!(", again every {} s for up to {w} s", every.unwrap_or(5)),
        None => String::new(),
    }
}

/// The steps with everything filled in, without running any of them.
pub fn plan(rb: &Runbook, values: &HashMap<String, String>, host: &HashMap<String, String>) -> Result<Vec<PlannedStep>, String> {
    let rb = &with_defaults(rb);
    let mut all = values.clone();
    all.extend(host.clone());
    // A file parameter has no text value here; the planner shows its name.
    for p in &rb.params {
        if matches!(p.kind, ParamKind::File | ParamKind::Dir) {
            all.entry(p.name.clone()).or_insert_with(|| format!("<{}>", p.name));
        }
        if p.kind == ParamKind::Secret {
            all.insert(p.name.clone(), format!("<{}>", p.name));
        }
    }
    let n = rb.steps.len();
    rb.steps
        .iter()
        .map(|s| (Phase::Run, s))
        .chain(rb.rollback.iter().map(|s| (Phase::Rollback, s)))
        .enumerate()
        .map(|(index, (phase, s))| {
            let (kind, text, local) = if let Some(r) = &s.run {
                ("run", render(r, &all)?, false)
            } else if let Some(w) = &s.wait {
                let until = match (w.until_exit, &w.contains) {
                    (e, Some(c)) => format!("exits {} and prints {c:?}", e.unwrap_or(0)),
                    (e, None) => format!("exits {}", e.unwrap_or(0)),
                };
                ("wait", format!("{} (until it {until}, every {} s, up to {} s)", render(&w.run, &all)?, w.every_secs.unwrap_or(5), w.timeout_secs.unwrap_or(60)), false)
            } else if let Some(u) = &s.upload {
                ("upload", format!("upload {} → {}", render(&u.local, &all)?, render(&u.remote, &all)?), true)
            } else if let Some(d) = &s.download {
                ("download", format!("download {} → {} (if a file is already there: {})", render(&d.remote, &all)?, render(&d.to, &all)?, match d.on_conflict {
                    Conflict::Overwrite => "replace it",
                    Conflict::Skip => "leave it",
                    Conflict::Rename => "keep both",
                }), true)
            } else if let Some(d) = &s.delay {
                ("delay", format!("wait {} s", d.secs), false)
            } else if let Some(h) = &s.http {
                let want = match h.expect_status {
                    None | Some(0) => "a 2xx or 3xx status".to_string(),
                    Some(c) => format!("status {c}"),
                };
                let method = format!("{:?}", h.method).to_uppercase();
                ("http", format!("{method} {} from this computer, expecting {want}{}{}", render_plain(&h.url, &all)?, h.contains.as_ref().map(|c| format!(" and {c:?} in the page")).unwrap_or_default(), poll_text(h.every_secs, h.wait_secs)), true)
            } else if let Some(t) = &s.tcp {
                let host = match &t.host {
                    Some(h) => render_plain(h, &all)?,
                    None => all.get("host").cloned().unwrap_or_default(),
                };
                ("tcp", format!("connect to {host}:{} from this computer{}", t.port, poll_text(t.every_secs, t.wait_secs)), true)
            } else if let Some(d) = &s.docker {
                ("docker", docker_command(d, &all)?, false)
            } else {
                ("", String::new(), false)
            };
            let name = match (s.name.is_empty(), phase) {
                (false, _) => s.name.clone(),
                (true, Phase::Run) => format!("Step {}", index + 1),
                (true, Phase::Rollback) => format!("Rollback step {}", index - n + 1),
            };
            let when = s.when.as_ref().map(describe_when);
            // A rollback step never runs on its own: say so, ahead of whatever else it depends on.
            let condition = match (phase, when) {
                (Phase::Run, w) => w,
                (Phase::Rollback, Some(w)) => Some(format!("only on a host where the run failed, with rollback on; {w}")),
                (Phase::Rollback, None) => Some("only on a host where the run failed, with rollback on".to_string()),
            };
            Ok(PlannedStep { phase, index, name, kind, text, condition, on_error: if s.on_error_or_stop() == OnError::Continue { "continue" } else { "stop" }, disabled: s.disabled, local })
        })
        .collect()
}

// -- running it ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Output {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<u32>,
    pub truncated: bool,
    pub duration_ms: u64,
}

/// Which stream a piece of live output came from.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
}

/// Where a step's output goes as it is produced, whole lines at a time.
pub trait OutputSink: Send + Sync {
    fn chunk(&self, stream: Stream, text: &str);
}

pub trait Executor: Send + Sync {
    /// Run a command and wait for it, up to `timeout`.
    fn exec(&self, command: &str, timeout: Duration) -> impl Future<Output = Result<Output, String>> + Send;
    /// Like [`Executor::exec`], handing the output on as it arrives. The default hands it all over at the end.
    fn exec_stream<'a>(&'a self, command: &'a str, timeout: Duration, out: &'a dyn OutputSink) -> impl Future<Output = Result<Output, String>> + Send + 'a {
        async move {
            let o = self.exec(command, timeout).await?;
            if !o.stdout.is_empty() {
                out.chunk(Stream::Stdout, &o.stdout);
            }
            if !o.stderr.is_empty() {
                out.chunk(Stream::Stderr, &o.stderr);
            }
            Ok(o)
        }
    }
    /// Wait between two tries of a wait step (a test doesn't).
    fn pause(&self, d: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(d)
    }
    /// Put `data` at `remote` (written beside it and moved into place).
    fn upload(&self, data: &[u8], remote: &str, mode: Option<&str>) -> impl Future<Output = Result<Output, String>> + Send;
    /// The contents of `remote`, at most [`MAX_DOWNLOAD`] bytes.
    fn download<'a>(&'a self, _remote: &'a str) -> impl Future<Output = Result<Vec<u8>, String>> + Send + 'a {
        async { Err("this connection can't download files".to_string()) }
    }
    /// One web request, from this computer.
    fn http<'a>(&'a self, method: HttpMethod, url: &'a str, expect: Option<u16>, contains: Option<&'a str>, limit: Duration) -> impl Future<Output = Output> + Send + 'a {
        crate::runbookchecks::http_once(method, url, expect, contains, limit)
    }
    /// One TCP connection, from this computer.
    fn tcp<'a>(&'a self, host: &'a str, port: u16, limit: Duration) -> impl Future<Output = Output> + Send + 'a {
        crate::runbookchecks::tcp_once(host, port, limit)
    }
}

/// The largest file a download takes.
pub const MAX_DOWNLOAD: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Ok,
    Failed,
    Skipped,
    TimedOut,
    Error,
}

/// Whether a result is from the steps of the run or from the rollback after a failure.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Run,
    Rollback,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepResult {
    #[serde(default)]
    pub phase: Phase,
    pub index: usize,
    pub name: String,
    pub status: StepStatus,
    pub output: Output,
    pub note: String,
}

pub trait Progress: Send + Sync {
    fn started(&self, index: usize);
    fn finished(&self, result: &StepResult);
    /// A piece of a step's output, as it is produced. Not every executor produces any before the step ends.
    fn output(&self, _index: usize, _stream: Stream, _text: &str) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// Hands a step's output to the progress report, tagged with the step.
struct StepOut<'a> {
    progress: &'a dyn Progress,
    index: usize,
}

impl OutputSink for StepOut<'_> {
    fn chunk(&self, stream: Stream, text: &str) {
        self.progress.output(self.index, stream, text);
    }
}

fn clip(mut o: Output) -> Output {
    for s in [&mut o.stdout, &mut o.stderr] {
        if s.len() > KEEP_OUTPUT {
            let mut end = KEEP_OUTPUT;
            while !s.is_char_boundary(end) {
                end -= 1;
            }
            s.truncate(end);
            o.truncated = true;
        }
    }
    o
}

fn holds(w: &When, results: &HashMap<String, Option<u32>>, values: &HashMap<String, String>) -> bool {
    if let Some(id) = &w.step {
        let code = results.get(id).copied().flatten();
        return match (w.exit, w.exit_not) {
            (Some(e), _) => code == Some(e),
            (_, Some(e)) => code != Some(e),
            _ => true,
        };
    }
    let value = w.param.as_ref().and_then(|p| values.get(p)).map(String::as_str).unwrap_or("");
    match (&w.equals, &w.not_equals) {
        (Some(v), _) => value == v,
        (_, Some(v)) => value != v,
        _ => true,
    }
}

/// What can be asked of one host's run beyond the document itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct RunOptions {
    /// Give a host whose run failed the runbook's rollback steps.
    pub rollback: bool,
    /// Don't run the steps before this one (0-based): they are reported as skipped. For running again from a failed step.
    pub from_step: Option<usize>,
}

/// Run every step on one host, in order. `files` has the bytes of each file parameter. Returns whether the host
/// got through without a step that stopped it.
pub async fn run_host<E: Executor>(rb: &Runbook, values: &HashMap<String, String>, files: &HashMap<String, Vec<u8>>, host: &HashMap<String, String>, exec: &E, progress: &dyn Progress) -> (bool, Vec<StepResult>) {
    run_host_with(rb, values, files, host, exec, progress, false).await
}

/// Like [`run_host`]. With `rollback` on, a host whose run failed (and was not cancelled) then gets the runbook's rollback
/// steps, after the failure, as best effort: each is tried in order, a step's own `on_error` decides whether to go on, and the
/// host still counts as failed. Without it, or with no rollback steps, nothing more runs.
pub async fn run_host_with<E: Executor>(rb: &Runbook, values: &HashMap<String, String>, files: &HashMap<String, Vec<u8>>, host: &HashMap<String, String>, exec: &E, progress: &dyn Progress, rollback: bool) -> (bool, Vec<StepResult>) {
    run_host_opts(rb, values, files, host, exec, progress, RunOptions { rollback, from_step: None }).await
}

pub async fn run_host_opts<E: Executor>(rb: &Runbook, values: &HashMap<String, String>, files: &HashMap<String, Vec<u8>>, host: &HashMap<String, String>, exec: &E, progress: &dyn Progress, opts: RunOptions) -> (bool, Vec<StepResult>) {
    let rb = &with_defaults(rb);
    let mut all = values.clone();
    all.extend(host.clone());
    let mut exits: HashMap<String, Option<u32>> = HashMap::new();
    let mut results = Vec::new();
    let ok = run_steps(&rb.steps, 0, Phase::Run, opts.from_step, values, files, &all, exec, progress, &mut exits, &mut results).await;
    if !ok && opts.rollback && !rb.rollback.is_empty() && !progress.cancelled() {
        run_steps(&rb.rollback, rb.steps.len(), Phase::Rollback, None, values, files, &all, exec, progress, &mut exits, &mut results).await;
    }
    (ok, results)
}

/// A name that is safe to use as one part of a local file name: letters, digits, `.`, `_` and `-`.
pub fn safe_name(s: &str, fallback: &str) -> String {
    let last = s.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = last.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).collect();
    let cleaned = cleaned.trim_start_matches('.');
    let cleaned: String = cleaned.chars().take(100).collect();
    if cleaned.is_empty() { fallback.to_string() } else { cleaned }
}

/// Save a downloaded file in `dir` as `<host>__<name>` and say where. Never writes anywhere but `dir`.
async fn save_download(dir: &str, host: &str, remote: &str, data: &[u8], on_conflict: Conflict) -> Result<String, String> {
    let dir = std::path::Path::new(dir);
    let meta = tokio::fs::metadata(dir).await.map_err(|e| format!("{}: {e}", dir.display()))?;
    if !meta.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    let name = format!("{}__{}", safe_name(host, "host"), safe_name(remote, "file"));
    let mut path = dir.join(&name);
    let mut n = 0u32;
    loop {
        let mut open = tokio::fs::OpenOptions::new();
        open.write(true);
        match on_conflict {
            Conflict::Overwrite => open.create(true).truncate(true),
            _ => open.create_new(true),
        };
        match open.open(&path).await {
            Ok(mut f) => {
                use tokio::io::AsyncWriteExt;
                f.write_all(data).await.map_err(|e| format!("{}: {e}", path.display()))?;
                f.flush().await.map_err(|e| format!("{}: {e}", path.display()))?;
                return Ok(path.display().to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => match on_conflict {
                Conflict::Skip => return Err(format!("SKIP:{}", path.display())),
                _ => {
                    n += 1;
                    if n > 1000 {
                        return Err("too many files with that name".into());
                    }
                    path = dir.join(format!("{name}.{n}"));
                }
            },
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
    }
}

/// Ask `once` again every `every` until it passes or `wait` is up (a single try when `wait` is none).
async fn poll<E: Executor, F, Fut>(exec: &E, progress: &dyn Progress, every: Option<u64>, wait: Option<u64>, mut once: F) -> (StepStatus, Output, String)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Output>,
{
    let every = Duration::from_secs(every.unwrap_or(5).clamp(1, 3600));
    let max_tries = wait.map_or(1, |w| (w.min(MAX_STEP_TIMEOUT) / every.as_secs()).max(1) as u32 + 1);
    let mut tries = 0u32;
    loop {
        tries += 1;
        let out = once().await;
        if out.exit_code == Some(0) {
            return (StepStatus::Ok, out, if tries > 1 { format!("passed after {tries} tries") } else { String::new() });
        }
        if progress.cancelled() || tries >= max_tries {
            let status = if wait.is_some() { StepStatus::TimedOut } else { StepStatus::Failed };
            let note = if wait.is_some() { format!("still failing after {tries} {}", if tries == 1 { "try" } else { "tries" }) } else { String::new() };
            return (status, out, note);
        }
        exec.pause(every).await;
    }
}

/// Run `steps` in order on one host. `offset` is where the first one sits in the numbering of results; steps before `from`
/// are reported as skipped. Returns whether the host got through without a step that stopped it.
#[allow(clippy::too_many_arguments)]
async fn run_steps<E: Executor>(
    steps: &[Step],
    offset: usize,
    phase: Phase,
    from: Option<usize>,
    values: &HashMap<String, String>,
    files: &HashMap<String, Vec<u8>>,
    all: &HashMap<String, String>,
    exec: &E,
    progress: &dyn Progress,
    exits: &mut HashMap<String, Option<u32>>,
    results: &mut Vec<StepResult>,
) -> bool {
    let mut ok = true;
    for (position, s) in steps.iter().enumerate() {
        let index = offset + position;
        if progress.cancelled() {
            ok = false;
            break;
        }
        let name = match (s.name.is_empty(), phase) {
            (false, _) => s.name.clone(),
            (true, Phase::Run) => format!("Step {}", index + 1),
            (true, Phase::Rollback) => format!("Rollback step {}", position + 1),
        };
        let finish = |status: StepStatus, output: Output, note: String| StepResult { phase, index, name: name.clone(), status, output, note };
        let skip = if from.is_some_and(|f| phase == Phase::Run && index < f) {
            Some(format!("not run again: this run starts at step {}", from.unwrap_or(0) + 1))
        } else if s.disabled {
            Some("switched off".to_string())
        } else if let Some(w) = &s.when {
            (!holds(w, exits, values)).then(|| describe_when(w).replace("only if", "skipped: it runs only if"))
        } else {
            None
        };
        if let Some(note) = skip {
            let r = finish(StepStatus::Skipped, Output::default(), note);
            progress.finished(&r);
            results.push(r);
            continue;
        }
        progress.started(index);
        let sink = StepOut { progress, index };
        let timeout = Duration::from_secs(s.timeout_secs.unwrap_or(DEFAULT_STEP_TIMEOUT).min(MAX_STEP_TIMEOUT));
        let attempts = 1 + s.retries.unwrap_or(0).min(MAX_RETRIES);
        let retry_delay = Duration::from_secs(s.retry_delay_secs.unwrap_or(DEFAULT_RETRY_DELAY).clamp(1, MAX_RETRY_DELAY));
        let mut attempt = 1u32;
        let outcome = loop {
            let outcome: Result<(StepStatus, Output, String), String> = async {
                if let Some(r) = &s.run {
                    let out = exec.exec_stream(&render(r, all)?, timeout, &sink).await?;
                    let good = out.exit_code == Some(0);
                    let note = if good { String::new() } else { format!("exited with {}", out.exit_code.map_or("no code".to_string(), |c| c.to_string())) };
                    Ok((if good { StepStatus::Ok } else { StepStatus::Failed }, out, note))
                } else if let Some(d) = &s.docker {
                    let out = exec.exec_stream(&docker_command(d, all)?, timeout, &sink).await?;
                    let good = out.exit_code == Some(0);
                    let note = if good { String::new() } else { format!("exited with {}", out.exit_code.map_or("no code".to_string(), |c| c.to_string())) };
                    Ok((if good { StepStatus::Ok } else { StepStatus::Failed }, out, note))
                } else if let Some(w) = &s.wait {
                    let command = render(&w.run, all)?;
                    let every = Duration::from_secs(w.every_secs.unwrap_or(5).clamp(1, 3600));
                    let limit = Duration::from_secs(w.timeout_secs.unwrap_or(60).min(MAX_STEP_TIMEOUT));
                    let want = w.until_exit.unwrap_or(0);
                    // How many tries fit in the time allowed, so the count, not the clock, decides when to give up.
                    let max_tries = (limit.as_secs() / every.as_secs()).max(1) as u32 + 1;
                    let mut tries = 0u32;
                    loop {
                        tries += 1;
                        let out = exec.exec_stream(&command, timeout.min(limit.max(Duration::from_secs(1))), &sink).await?;
                        let exit_ok = out.exit_code == Some(want);
                        let text_ok = w.contains.as_ref().is_none_or(|c| out.stdout.contains(c.as_str()));
                        if exit_ok && text_ok {
                            break Ok((StepStatus::Ok, out, format!("ready after {tries} {}", if tries == 1 { "try" } else { "tries" })));
                        }
                        if progress.cancelled() || tries >= max_tries {
                            break Ok((StepStatus::TimedOut, out, format!("not ready after about {} s ({tries} tries)", u64::from(tries - 1) * every.as_secs())));
                        }
                        exec.pause(every).await;
                    }
                } else if let Some(u) = &s.upload {
                    let param = placeholders(&u.local)?.into_iter().next().map(|p| p.0).unwrap_or_default();
                    let data = files.get(&param).ok_or_else(|| format!("no file was chosen for {param}"))?;
                    let remote = render(&u.remote, all)?;
                    let out = exec.upload(data, &remote, u.mode.as_deref()).await?;
                    let good = out.exit_code == Some(0);
                    Ok((if good { StepStatus::Ok } else { StepStatus::Failed }, out, if good { format!("{} bytes", data.len()) } else { "the upload failed".into() }))
                } else if let Some(d) = &s.download {
                    let param = placeholders(&d.to)?.into_iter().next().map(|p| p.0).unwrap_or_default();
                    let dir = values.get(&param).filter(|v| !v.is_empty()).ok_or_else(|| format!("no folder was chosen for {param}"))?;
                    let remote = render_plain(&d.remote, all)?;
                    let started = std::time::Instant::now();
                    let data = exec.download(&remote).await?;
                    let host = all.get("label").map(String::as_str).unwrap_or("host");
                    match save_download(dir, host, &remote, &data, d.on_conflict).await {
                        Ok(path) => {
                            let out = Output { stdout: format!("saved {} bytes to {path}", data.len()), exit_code: Some(0), duration_ms: started.elapsed().as_millis() as u64, ..Default::default() };
                            sink.chunk(Stream::Stdout, &out.stdout);
                            Ok((StepStatus::Ok, out, format!("{} bytes", data.len())))
                        }
                        Err(e) if e.starts_with("SKIP:") => {
                            let out = Output { stdout: format!("{} already exists, so it was left as it is", &e[5..]), exit_code: Some(0), duration_ms: started.elapsed().as_millis() as u64, ..Default::default() };
                            sink.chunk(Stream::Stdout, &out.stdout);
                            Ok((StepStatus::Ok, out, "left the existing file".into()))
                        }
                        Err(e) => Err(e),
                    }
                } else if let Some(d) = &s.delay {
                    let started = std::time::Instant::now();
                    let mut left = d.secs.min(MAX_DELAY);
                    // In slices, so a cancel is noticed within a second.
                    while left > 0 && !progress.cancelled() {
                        let slice = left.min(1);
                        exec.pause(Duration::from_secs(slice)).await;
                        left -= slice;
                    }
                    let out = Output { exit_code: Some(0), duration_ms: started.elapsed().as_millis() as u64, ..Default::default() };
                    Ok((StepStatus::Ok, out, format!("waited {} s", d.secs)))
                } else if let Some(h) = &s.http {
                    let url = render_plain(&h.url, all)?;
                    let limit = Duration::from_secs(h.timeout_secs.unwrap_or(10).clamp(1, 120));
                    let (status, out, note) = poll(exec, progress, h.every_secs, h.wait_secs, || exec.http(h.method, &url, h.expect_status, h.contains.as_deref(), limit)).await;
                    for (stream, text) in [(Stream::Stdout, &out.stdout), (Stream::Stderr, &out.stderr)] {
                        if !text.is_empty() {
                            sink.chunk(stream, text);
                        }
                    }
                    Ok((status, out, note))
                } else if let Some(t) = &s.tcp {
                    let host = match &t.host {
                        Some(h) => render_plain(h, all)?,
                        None => all.get("host").cloned().unwrap_or_default(),
                    };
                    if host.trim().is_empty() {
                        return Err("there is no host to connect to".into());
                    }
                    let limit = Duration::from_secs(t.timeout_secs.unwrap_or(10).clamp(1, 120));
                    let (status, out, note) = poll(exec, progress, t.every_secs, t.wait_secs, || exec.tcp(&host, t.port, limit)).await;
                    for (stream, text) in [(Stream::Stdout, &out.stdout), (Stream::Stderr, &out.stderr)] {
                        if !text.is_empty() {
                            sink.chunk(stream, text);
                        }
                    }
                    Ok((status, out, note))
                } else {
                    Err("an empty step".into())
                }
            }
            .await;
            // Only a command that ran and failed is tried again; a dropped connection or a bad template would fail the same way.
            let failed = matches!(&outcome, Ok((StepStatus::Failed | StepStatus::TimedOut, ..)));
            if !failed || attempt >= attempts || progress.cancelled() {
                break outcome;
            }
            exec.pause(retry_delay).await;
            attempt += 1;
        };
        let r = match outcome {
            Ok((status, output, note)) if attempt > 1 => {
                let note = if note.is_empty() { format!("after {attempt} tries") } else { format!("{note}, after {attempt} tries") };
                finish(status, clip(output), note)
            }
            Ok((status, output, note)) => finish(status, clip(output), note),
            Err(e) => finish(StepStatus::Error, Output::default(), e),
        };
        if let Some(id) = &s.id {
            exits.insert(id.clone(), r.output.exit_code);
        }
        let stop = matches!(r.status, StepStatus::Failed | StepStatus::TimedOut | StepStatus::Error) && s.on_error_or_stop() == OnError::Stop;
        progress.finished(&r);
        results.push(r);
        if stop {
            ok = false;
            break;
        }
    }
    ok
}

#[cfg(test)]
#[path = "runbook/tests.rs"]
mod tests;
