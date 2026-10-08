//! Runbooks: a saved sequence of steps with parameters, run on one host or many, with a record of what happened.
//!
//! A runbook is a JSON document you can read and diff, not a program: steps run a command, wait until a command
//! succeeds (or prints something), or upload a file the person picked. Whether a step runs can depend on an
//! earlier step's exit code or on a parameter, and nothing else. Everything here is pure or goes through the
//! [`Executor`] trait, so the logic is tested without a network, and one test uses a real SSH server.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};

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
    #[serde(default)]
    pub params: Vec<Param>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Param {
    pub name: String,
    #[serde(default)]
    pub label: String,
    /// Without a default (and not `optional`), a value must be given.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(default)]
    pub optional: bool,
    /// If present, the value must be one of these.
    #[serde(default)]
    pub choices: Vec<String>,
    /// "text" (default) or "file": a file chosen on this computer, usable only as an upload's source.
    #[serde(default)]
    pub kind: ParamKind,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ParamKind {
    #[default]
    Text,
    File,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Step {
    #[serde(default)]
    pub name: String,
    /// Lets later steps and conditions refer to this one.
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub run: Option<String>,
    #[serde(default)]
    pub wait: Option<Wait>,
    #[serde(default)]
    pub upload: Option<Upload>,
    #[serde(default)]
    pub when: Option<When>,
    /// What a failed step does: stop this host (default) or carry on.
    #[serde(default)]
    pub on_error: OnError,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

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
    pub message: String,
}

fn problem(step: Option<usize>, message: impl Into<String>) -> Problem {
    Problem { step, message: message.into() }
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

/// Parse a runbook's text and check everything that can be checked before it runs.
pub fn parse(text: &str) -> Result<Runbook, Vec<Problem>> {
    let rb: Runbook = serde_json::from_str(text).map_err(|e| vec![problem(None, format!("not a valid runbook: {e}"))])?;
    let problems = check(&rb);
    if problems.is_empty() {
        Ok(rb)
    } else {
        Err(problems)
    }
}

pub fn check(rb: &Runbook) -> Vec<Problem> {
    let mut out = Vec::new();
    if rb.name.trim().is_empty() || rb.name.len() > 100 {
        out.push(problem(None, "the runbook needs a name of up to 100 characters"));
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
            if p.kind == ParamKind::File {
                out.push(problem(None, format!("a file parameter (\"{}\") can't have a default: the file is chosen when it runs", p.name)));
            }
        }
        if p.kind == ParamKind::File && !p.choices.is_empty() {
            out.push(problem(None, format!("a file parameter (\"{}\") can't have choices", p.name)));
        }
    }
    let known = |name: &str| BUILTINS.contains(&name) || params.contains_key(name);
    let mut ids: HashSet<&str> = HashSet::new();
    let mut earlier: HashSet<&str> = HashSet::new();
    for (i, s) in rb.steps.iter().enumerate() {
        let at = Some(i);
        let kinds = [s.run.is_some(), s.wait.is_some(), s.upload.is_some()].iter().filter(|b| **b).count();
        if kinds != 1 {
            out.push(problem(at, "a step has exactly one of run, wait or upload"));
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
                        } else if params.get(name.as_str()).is_some_and(|p| p.kind == ParamKind::File) {
                            out.push(problem(at, format!("{{{{{name}}}}} is a file parameter; only an upload can use it")));
                        }
                    }
                }
                Err(e) => out.push(problem(at, e)),
            }
        }
        if s.timeout_secs.is_some_and(|t| !(1..=MAX_STEP_TIMEOUT).contains(&t)) {
            out.push(problem(at, format!("timeout_secs must be between 1 and {MAX_STEP_TIMEOUT}")));
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
                    Some(p) if p.kind == ParamKind::Text => {}
                    _ => out.push(problem(at, "when's param must be a text parameter of this runbook")),
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

/// A step as it will run for one host, for a dry run.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PlannedStep {
    pub index: usize,
    pub name: String,
    pub kind: &'static str,
    /// The command, the wait's command, or "upload <file> → <remote>".
    pub text: String,
    /// What it depends on, in words.
    pub condition: Option<String>,
    pub on_error: &'static str,
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

/// The steps with everything filled in, without running any of them.
pub fn plan(rb: &Runbook, values: &HashMap<String, String>, host: &HashMap<String, String>) -> Result<Vec<PlannedStep>, String> {
    let mut all = values.clone();
    all.extend(host.clone());
    // A file parameter has no text value here; the planner shows its name.
    for p in &rb.params {
        if p.kind == ParamKind::File {
            all.entry(p.name.clone()).or_insert_with(|| format!("<{}>", p.name));
        }
    }
    rb.steps
        .iter()
        .enumerate()
        .map(|(index, s)| {
            let (kind, text) = if let Some(r) = &s.run {
                ("run", render(r, &all)?)
            } else if let Some(w) = &s.wait {
                let until = match (w.until_exit, &w.contains) {
                    (e, Some(c)) => format!("exits {} and prints {c:?}", e.unwrap_or(0)),
                    (e, None) => format!("exits {}", e.unwrap_or(0)),
                };
                ("wait", format!("{} (until it {until}, every {} s, up to {} s)", render(&w.run, &all)?, w.every_secs.unwrap_or(5), w.timeout_secs.unwrap_or(60)))
            } else if let Some(u) = &s.upload {
                ("upload", format!("upload {} → {}", render(&u.local, &all)?, render(&u.remote, &all)?))
            } else {
                ("", String::new())
            };
            let name = if s.name.is_empty() { format!("Step {}", index + 1) } else { s.name.clone() };
            Ok(PlannedStep { index, name, kind, text, condition: s.when.as_ref().map(describe_when), on_error: if s.on_error == OnError::Continue { "continue" } else { "stop" } })
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

pub trait Executor: Send + Sync {
    /// Run a command and wait for it, up to `timeout`.
    fn exec(&self, command: &str, timeout: Duration) -> impl Future<Output = Result<Output, String>> + Send;
    /// Wait between two tries of a wait step (a test doesn't).
    fn pause(&self, d: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(d)
    }
    /// Put `data` at `remote` (written beside it and moved into place).
    fn upload(&self, data: &[u8], remote: &str, mode: Option<&str>) -> impl Future<Output = Result<Output, String>> + Send;
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Ok,
    Failed,
    Skipped,
    TimedOut,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepResult {
    pub index: usize,
    pub name: String,
    pub status: StepStatus,
    pub output: Output,
    pub note: String,
}

pub trait Progress: Send + Sync {
    fn started(&self, index: usize);
    fn finished(&self, result: &StepResult);
    fn cancelled(&self) -> bool {
        false
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

/// Run every step on one host, in order. `files` has the bytes of each file parameter. Returns whether the host
/// got through without a step that stopped it.
pub async fn run_host<E: Executor>(rb: &Runbook, values: &HashMap<String, String>, files: &HashMap<String, Vec<u8>>, host: &HashMap<String, String>, exec: &E, progress: &dyn Progress) -> (bool, Vec<StepResult>) {
    let mut all = values.clone();
    all.extend(host.clone());
    let mut exits: HashMap<String, Option<u32>> = HashMap::new();
    let mut results = Vec::new();
    let mut ok = true;
    for (index, s) in rb.steps.iter().enumerate() {
        if progress.cancelled() {
            ok = false;
            break;
        }
        let name = if s.name.is_empty() { format!("Step {}", index + 1) } else { s.name.clone() };
        let finish = |status: StepStatus, output: Output, note: String| StepResult { index, name: name.clone(), status, output, note };
        if let Some(w) = &s.when {
            if !holds(w, &exits, values) {
                let r = finish(StepStatus::Skipped, Output::default(), describe_when(w).replace("only if", "skipped: it runs only if"));
                progress.finished(&r);
                results.push(r);
                continue;
            }
        }
        progress.started(index);
        let timeout = Duration::from_secs(s.timeout_secs.unwrap_or(DEFAULT_STEP_TIMEOUT).min(MAX_STEP_TIMEOUT));
        let outcome: Result<(StepStatus, Output, String), String> = async {
            if let Some(r) = &s.run {
                let out = exec.exec(&render(r, &all)?, timeout).await?;
                let good = out.exit_code == Some(0);
                let note = if good { String::new() } else { format!("exited with {}", out.exit_code.map_or("no code".to_string(), |c| c.to_string())) };
                Ok((if good { StepStatus::Ok } else { StepStatus::Failed }, out, note))
            } else if let Some(w) = &s.wait {
                let command = render(&w.run, &all)?;
                let every = Duration::from_secs(w.every_secs.unwrap_or(5).clamp(1, 3600));
                let limit = Duration::from_secs(w.timeout_secs.unwrap_or(60).min(MAX_STEP_TIMEOUT));
                let want = w.until_exit.unwrap_or(0);
                // How many tries fit in the time allowed, so the count, not the clock, decides when to give up.
                let max_tries = (limit.as_secs() / every.as_secs()).max(1) as u32 + 1;
                let mut tries = 0u32;
                loop {
                    tries += 1;
                    let out = exec.exec(&command, timeout.min(limit.max(Duration::from_secs(1)))).await?;
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
                let remote = render(&u.remote, &all)?;
                let out = exec.upload(data, &remote, u.mode.as_deref()).await?;
                let good = out.exit_code == Some(0);
                Ok((if good { StepStatus::Ok } else { StepStatus::Failed }, out, if good { format!("{} bytes", data.len()) } else { "the upload failed".into() }))
            } else {
                Err("an empty step".into())
            }
        }
        .await;
        let r = match outcome {
            Ok((status, output, note)) => finish(status, clip(output), note),
            Err(e) => finish(StepStatus::Error, Output::default(), e),
        };
        if let Some(id) = &s.id {
            exits.insert(id.clone(), r.output.exit_code);
        }
        let stop = matches!(r.status, StepStatus::Failed | StepStatus::TimedOut | StepStatus::Error) && s.on_error == OnError::Stop;
        progress.finished(&r);
        results.push(r);
        if stop {
            ok = false;
            break;
        }
    }
    (ok, results)
}

#[cfg(test)]
#[path = "runbook/tests.rs"]
mod tests;
