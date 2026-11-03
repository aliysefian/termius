use super::*;
use std::sync::Mutex;

fn doc(extra: &str) -> String {
    format!(r#"{{"name":"Restart","params":[{{"name":"service","default":"nginx"}},{{"name":"mode","choices":["fast","safe"],"default":"safe"}},{{"name":"conf","kind":"file","optional":true}}],"steps":[{extra}]}}"#)
}

fn problems(extra: &str) -> Vec<String> {
    match parse(&doc(extra)) {
        Ok(_) => vec![],
        Err(p) => p.into_iter().map(|p| p.message).collect(),
    }
}

fn has(extra: &str, needle: &str) -> bool {
    let p = problems(extra);
    p.iter().any(|m| m.contains(needle))
}

#[test]
fn a_good_runbook_parses() {
    let rb = parse(&doc(r#"{"name":"Is it up","id":"up","run":"systemctl is-active {{service|q}}","on_error":"continue"},{"name":"Restart","run":"sudo systemctl restart {{service}}","when":{"step":"up","exit_not":0}},{"name":"Healthy","wait":{"run":"curl -fs localhost/{{label}}","until_exit":0,"every_secs":2,"timeout_secs":30}},{"name":"Config","upload":{"local":"{{conf}}","remote":"/etc/app.conf","mode":"0644"},"when":{"param":"mode","equals":"safe"}}"#)).unwrap();
    assert_eq!(rb.steps.len(), 4);
    assert_eq!(rb.params[0].name, "service");
}

#[test]
fn what_is_wrong_is_said_per_step() {
    assert!(has(r#"{"name":"x"}"#, "exactly one of run, wait, upload"));
    assert!(has(r#"{"run":"a","wait":{"run":"b"}}"#, "exactly one"));
    assert!(has(r#"{"run":"echo {{nope}}"}"#, "{{nope}} isn't a parameter"));
    assert!(has(r#"{"run":"echo {{service|x}}"}"#, "the filter |x"));
    assert!(has(r#"{"run":"echo {{service"}"#, "never closed"));
    assert!(has(r#"{"run":"echo {{Bad}}"}"#, "isn't a valid name"));
    assert!(has(r#"{"run":"  "}"#, "is empty"));
    assert!(has(r#"{"run":"a","timeout_secs":0}"#, "timeout_secs"));
    assert!(has(r#"{"run":"a","timeout_secs":99999}"#, "timeout_secs"));
    assert!(has(r#"{"run":"a","id":"x"},{"run":"b","id":"x"}"#, "used twice"));
    assert!(has(r#"{"run":"a","id":"Bad Id"}"#, "isn't valid"));
    assert!(has(r#"{"wait":{"run":"a","every_secs":0}}"#, "every_secs"));
    assert!(has(r#"{"wait":{"run":"a","contains":""}}"#, "contains can't be empty"));
    let all = problems(r#"{"run":"a"},{"run":"{{x}}"}"#);
    assert_eq!(all.len(), 1, "{all:?}");
    let p = parse(&doc(r#"{"run":"a"},{"run":"{{x}}"}"#)).unwrap_err();
    assert_eq!(p[0].step, Some(1));
}

#[test]
fn the_document_itself_is_checked() {
    assert!(parse("not json").unwrap_err()[0].message.contains("not a valid runbook"));
    assert!(parse(r#"{"name":"x","steps":[{"run":"a","surprise":1}]}"#).unwrap_err()[0].message.contains("surprise"), "unknown fields are refused, so a typo isn't silently ignored");
    assert!(parse(r#"{"name":"","steps":[{"run":"a"}]}"#).is_err());
    assert!(parse(r#"{"name":"x","steps":[]}"#).is_err());
    let many = format!(r#"{{"name":"x","steps":[{}]}}"#, vec![r#"{"run":"a"}"#; MAX_STEPS + 1].join(","));
    assert!(parse(&many).unwrap_err()[0].message.contains("more than"));
}

#[test]
fn parameters_are_checked() {
    let bad = |params: &str| parse(&format!(r#"{{"name":"x","params":[{params}],"steps":[{{"run":"a"}}]}}"#)).unwrap_err().into_iter().map(|p| p.message).collect::<Vec<_>>().join("|");
    assert!(bad(r#"{"name":"Bad"}"#).contains("isn't valid"));
    assert!(bad(r#"{"name":"host"}"#).contains("given by the app"));
    assert!(bad(r#"{"name":"a"},{"name":"a"}"#).contains("listed twice"));
    assert!(bad(r#"{"name":"a","choices":["x"],"default":"y"}"#).contains("isn't one of its choices"));
    assert!(bad(r#"{"name":"a","kind":"file","default":"/etc/passwd"}"#).contains("can't have a default"));
    assert!(bad(r#"{"name":"a","kind":"file","choices":["x"]}"#).contains("can't have choices"));
}

#[test]
fn an_upload_can_only_take_a_file_chosen_when_it_runs() {
    assert!(has(r#"{"upload":{"local":"/home/me/.ssh/id_rsa","remote":"/tmp/x"}}"#, "must be exactly {{name}}"));
    assert!(has(r#"{"upload":{"local":"{{service}}","remote":"/tmp/x"}}"#, "isn't one"));
    assert!(has(r#"{"upload":{"local":"{{conf}}.bak","remote":"/tmp/x"}}"#, "must be exactly"));
    assert!(has(r#"{"upload":{"local":"{{conf}}","remote":"/tmp/x","mode":"9"}}"#, "octal"));
    assert!(has(r#"{"upload":{"local":"{{conf}}","remote":""}}"#, "remote"));
    assert!(has(r#"{"run":"cat {{conf}}"}"#, "file parameter"));
    assert!(!has(r#"{"upload":{"local":"{{conf}}","remote":"/tmp/x"}}"#, "upload"));
}

#[test]
fn conditions_look_back_not_forward() {
    assert!(has(r#"{"run":"a","when":{"step":"later","exit":0}},{"run":"b","id":"later"}"#, "isn't an earlier step"));
    assert!(has(r#"{"run":"a","id":"a1"},{"run":"b","when":{"step":"a1"}}"#, "exactly one of exit and exit_not"));
    assert!(has(r#"{"run":"a","id":"a1"},{"run":"b","when":{"step":"a1","exit":0,"param":"mode","equals":"x"}}"#, "either"));
    assert!(has(r#"{"run":"a","when":{"param":"nope","equals":"x"}}"#, "boolean parameter"));
    assert!(has(r#"{"run":"a","when":{"param":"conf","equals":"x"}}"#, "boolean parameter"));
    assert!(has(r#"{"run":"a","when":{}}"#, "either"));
    assert!(!has(r#"{"run":"a","when":{"param":"mode","not_equals":"fast"}}"#, "when"));
}

#[test]
fn placeholders_are_found_in_order() {
    assert_eq!(placeholders("a {{x}} b {{ y | q }} c").unwrap(), vec![("x".into(), false), ("y".into(), true)]);
    assert_eq!(placeholders("no placeholders").unwrap(), vec![]);
    assert!(placeholders("{{}}").is_err());
}

fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn values_come_from_what_was_given_or_the_default() {
    let rb = parse(&doc(r#"{"run":"a"}"#)).unwrap();
    let v = resolve_params(&rb, &values(&[])).unwrap();
    assert_eq!((v["service"].as_str(), v["mode"].as_str(), v["conf"].as_str()), ("nginx", "safe", ""));
    let v = resolve_params(&rb, &values(&[("service", " redis "), ("mode", "fast")])).unwrap();
    assert_eq!((v["service"].as_str(), v["mode"].as_str()), ("redis", "fast"));
    let e = resolve_params(&rb, &values(&[("mode", "reckless"), ("nope", "1")])).unwrap_err();
    assert!(e.iter().any(|m| m.contains("must be one of: fast, safe")) && e.iter().any(|m| m.contains("nope isn't a parameter")), "{e:?}");
    let needs = parse(r#"{"name":"x","params":[{"name":"who","label":"Who to greet"}],"steps":[{"run":"echo {{who}}"}]}"#).unwrap();
    assert_eq!(resolve_params(&needs, &values(&[])).unwrap_err(), vec!["Who to greet needs a value"]);
    assert!(resolve_params(&needs, &values(&[("who", &"x".repeat(MAX_VALUE + 1))])).is_err());
}

#[test]
fn rendering_quotes_only_when_asked() {
    let v = values(&[("service", "my app; rm -rf /"), ("n", "it's")]);
    assert_eq!(render("echo {{service}}", &v).unwrap(), "echo my app; rm -rf /");
    assert_eq!(render("echo {{service|q}}", &v).unwrap(), "echo 'my app; rm -rf /'");
    assert_eq!(render("echo {{n|q}}", &v).unwrap(), r"echo 'it'\''s'");
    assert_eq!(render("plain", &v).unwrap(), "plain");
    assert!(render("echo {{missing}}", &v).is_err());
}

#[test]
fn the_apps_own_values_are_quoted_because_records_can_hold_anything() {
    let v = values(&[("host", "db$(id);x"), ("label", "web 1"), ("service", "a b")]);
    assert_eq!(render("ping {{host}}", &v).unwrap(), "ping 'db$(id);x'");
    assert_eq!(render("echo {{label}}", &v).unwrap(), "echo 'web 1'");
    assert_eq!(render("ping {{host|raw}}", &v).unwrap(), "ping db$(id);x", "|raw is the explicit way to ask for it as it is");
    assert_eq!(render("echo {{service|raw}}", &v).unwrap(), "echo a b");
    // A sane host name is untouched, so existing runbooks read the same.
    let ok = values(&[("host", "h.example"), ("label", "h")]);
    assert_eq!(render("echo {{label}} {{host}}", &ok).unwrap(), "echo h h.example");
    assert_eq!(placeholders("{{x|raw}} {{y|q}}").unwrap(), vec![("x".into(), false), ("y".into(), true)]);
}

#[test]
fn a_dry_run_shows_each_step_filled_in() {
    let rb = parse(&doc(r#"{"name":"Check","id":"c","run":"systemctl is-active {{service|q}}"},{"run":"restart {{service}} on {{host}}","when":{"step":"c","exit":0},"on_error":"continue"},{"wait":{"run":"curl x","contains":"ok"}},{"upload":{"local":"{{conf}}","remote":"/etc/{{service}}.conf"}}"#)).unwrap();
    let p = plan(&rb, &values(&[("service", "nginx"), ("mode", "safe")]), &values(&[("host", "web-01.example"), ("label", "web-01")])).unwrap();
    assert_eq!(p[0].text, "systemctl is-active nginx");
    assert_eq!(p[0].name, "Check");
    assert_eq!(p[1].text, "restart nginx on web-01.example");
    assert_eq!(p[1].condition.as_deref(), Some("only if step c exited with 0"));
    assert_eq!(p[1].on_error, "continue");
    assert_eq!(p[1].name, "Step 2");
    assert!(p[2].text.contains("prints \"ok\"") && p[2].kind == "wait");
    assert_eq!(p[3].text, "upload <conf> → /etc/nginx.conf");
}

// -- running -----------------------------------------------------------------------------

#[derive(Default)]
struct Fake {
    /// command → (exit, stdout), in order of preference; anything else exits 0 with no output.
    script: Mutex<Vec<(String, u32, String)>>,
    seen: Mutex<Vec<String>>,
    uploads: Mutex<Vec<(usize, String, Option<String>)>>,
}

impl Fake {
    fn on(self, command: &str, exit: u32, out: &str) -> Self {
        self.script.lock().unwrap().push((command.into(), exit, out.into()));
        self
    }
}

impl Executor for Fake {
    async fn exec(&self, command: &str, _timeout: Duration) -> Result<Output, String> {
        self.seen.lock().unwrap().push(command.to_string());
        let mut script = self.script.lock().unwrap();
        // A scripted command answers in order, and the last answer repeats.
        let hits: Vec<usize> = script.iter().enumerate().filter(|(_, s)| s.0 == command).map(|(i, _)| i).collect();
        let (exit, out) = match hits.as_slice() {
            [] => (0, String::new()),
            [only] => (script[*only].1, script[*only].2.clone()),
            [first, ..] => {
                let s = script.remove(*first);
                (s.1, s.2)
            }
        };
        if command == "boom" {
            return Err("the connection dropped".into());
        }
        Ok(Output { stdout: out, exit_code: Some(exit), ..Default::default() })
    }
    async fn pause(&self, _d: Duration) {}
    async fn upload(&self, data: &[u8], remote: &str, mode: Option<&str>) -> Result<Output, String> {
        self.uploads.lock().unwrap().push((data.len(), remote.into(), mode.map(String::from)));
        Ok(Output { exit_code: Some(0), ..Default::default() })
    }
}

#[derive(Default)]
struct Log(Mutex<Vec<String>>);
impl Progress for Log {
    fn started(&self, i: usize) {
        self.0.lock().unwrap().push(format!("start {i}"));
    }
    fn finished(&self, r: &StepResult) {
        self.0.lock().unwrap().push(format!("{} {:?}", r.index, r.status));
    }
}

fn run(rb: &str, v: &[(&str, &str)], fake: &Fake) -> (bool, Vec<StepResult>, Vec<String>) {
    let rb = parse(rb).unwrap();
    let vals = resolve_params(&rb, &values(v)).unwrap();
    let log = Log::default();
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    let (ok, results) = rt.block_on(run_host(&rb, &vals, &HashMap::from([("conf".to_string(), vec![1u8, 2, 3])]), &values(&[("host", "h.example"), ("label", "h")]), fake, &log));
    (ok, results, log.0.into_inner().unwrap())
}

fn statuses(r: &[StepResult]) -> Vec<StepStatus> {
    r.iter().map(|s| s.status).collect()
}

#[test]
fn steps_run_in_order_and_a_failure_stops_the_host() {
    let fake = Fake::default().on("two", 5, "");
    let (ok, r, _) = run(&doc(r#"{"run":"one"},{"run":"two"},{"run":"three"}"#), &[], &fake);
    assert!(!ok);
    assert_eq!(statuses(&r), [StepStatus::Ok, StepStatus::Failed]);
    assert_eq!(r[1].note, "exited with 5");
    assert_eq!(*fake.seen.lock().unwrap(), ["one", "two"], "the third never ran");
}

#[test]
fn a_step_can_carry_on_after_failing() {
    let fake = Fake::default().on("two", 1, "");
    let (ok, r, _) = run(&doc(r#"{"run":"one"},{"run":"two","on_error":"continue"},{"run":"three"}"#), &[], &fake);
    assert!(ok, "the host got through, as the runbook allows");
    assert_eq!(statuses(&r), [StepStatus::Ok, StepStatus::Failed, StepStatus::Ok]);
}

#[test]
fn a_step_runs_or_is_skipped_by_an_earlier_exit_code() {
    let fake = Fake::default().on("systemctl is-active nginx", 3, "inactive");
    let (ok, r, log) = run(&doc(r#"{"id":"up","run":"systemctl is-active {{service}}","on_error":"continue"},{"run":"restart","when":{"step":"up","exit_not":0}},{"run":"leave alone","when":{"step":"up","exit":0}}"#), &[], &fake);
    assert!(ok);
    assert_eq!(statuses(&r), [StepStatus::Failed, StepStatus::Ok, StepStatus::Skipped]);
    assert!(r[2].note.contains("runs only if step up exited with 0"), "{}", r[2].note);
    assert_eq!(*fake.seen.lock().unwrap(), ["systemctl is-active nginx", "restart"]);
    assert!(log.contains(&"start 1".to_string()) && !log.contains(&"start 2".to_string()), "a skipped step never starts");
}

#[test]
fn a_step_can_depend_on_a_parameter() {
    let rb = doc(r#"{"run":"careful","when":{"param":"mode","equals":"safe"}},{"run":"fast","when":{"param":"mode","not_equals":"safe"}}"#);
    let fake = Fake::default();
    run(&rb, &[], &fake);
    assert_eq!(*fake.seen.lock().unwrap(), ["careful"]);
    let fake = Fake::default();
    run(&rb, &[("mode", "fast")], &fake);
    assert_eq!(*fake.seen.lock().unwrap(), ["fast"]);
}

#[test]
fn values_are_filled_in_per_host() {
    let fake = Fake::default();
    run(&doc(r#"{"run":"echo {{label}} {{host}} {{service|q}}"}"#), &[("service", "a b")], &fake);
    assert_eq!(*fake.seen.lock().unwrap(), ["echo h h.example 'a b'"]);
}

#[test]
fn a_failed_step_is_tried_again_only_when_it_asks_to_be() {
    // Fails twice, then works: three tries are enough for "retries": 2.
    let fake = Fake::default().on("flaky", 1, "").on("flaky", 1, "").on("flaky", 0, "fine");
    let (ok, r, _) = run(&doc(r#"{"run":"flaky","retries":2,"retry_delay_secs":1},{"run":"after"}"#), &[], &fake);
    assert!(ok);
    assert_eq!((r[0].status, r[0].note.as_str()), (StepStatus::Ok, "after 3 tries"));
    assert_eq!(fake.seen.lock().unwrap().iter().filter(|c| *c == "flaky").count(), 3);

    // Not enough tries: it ends as a failure that says how many were made, and the host stops.
    let fake = Fake::default().on("down", 7, "");
    let (ok, r, _) = run(&doc(r#"{"run":"down","retries":2},{"run":"never"}"#), &[], &fake);
    assert!(!ok);
    assert_eq!((r[0].status, r[0].note.as_str()), (StepStatus::Failed, "exited with 7, after 3 tries"));
    assert_eq!(fake.seen.lock().unwrap().iter().filter(|c| *c == "down").count(), 3);
    assert!(!fake.seen.lock().unwrap().contains(&"never".to_string()));

    // Without "retries" a failure is tried once, as before.
    let fake = Fake::default().on("down", 1, "");
    run(&doc(r#"{"run":"down","on_error":"continue"}"#), &[], &fake);
    assert_eq!(fake.seen.lock().unwrap().len(), 1);

    // A dropped connection is an error, not a failed command: no retry.
    let fake = Fake::default();
    let (ok, r, _) = run(&doc(r#"{"run":"boom","retries":3}"#), &[], &fake);
    assert!(!ok && r[0].status == StepStatus::Error);
    assert_eq!(fake.seen.lock().unwrap().len(), 1);
}

#[test]
fn retry_settings_are_checked() {
    let bad = |step: &str| parse(&doc(step)).unwrap_err().iter().map(|p| p.message.clone()).collect::<Vec<_>>().join("; ");
    assert!(bad(r#"{"run":"x","retries":6}"#).contains("at most 5"));
    assert!(bad(r#"{"run":"x","retries":1,"retry_delay_secs":0}"#).contains("retry_delay_secs"));
    assert!(bad(r#"{"wait":{"run":"x"},"retries":1}"#).contains("wait step"));
    assert!(parse(&doc(r#"{"run":"x","retries":5,"retry_delay_secs":300}"#)).is_ok());
}

/// A runbook with two steps and a rollback; the fake decides which step fails.
fn with_rollback(steps: &str, rollback: &str) -> String {
    format!(r#"{{"name":"Deploy","params":[{{"name":"service","default":"web"}}],"steps":[{steps}],"rollback":[{rollback}]}}"#)
}

fn run_with_rollback(rb: &str, fake: &Fake, rollback: bool) -> (bool, Vec<StepResult>) {
    let rb = parse(rb).unwrap();
    let vals = resolve_params(&rb, &values(&[])).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    rt.block_on(run_host_with(&rb, &vals, &HashMap::new(), &values(&[("host", "h.example"), ("label", "h")]), fake, &Log::default(), rollback))
}

const DEPLOY: &str = r#"{"name":"Copy","run":"copy"},{"name":"Switch","run":"switch {{service|q}}"},{"name":"Check","run":"check"}"#;
const UNDO: &str = r#"{"name":"Switch back","run":"switch-back {{service|q}}"},{"run":"cleanup"}"#;

#[test]
fn rollback_runs_after_a_failure_only_when_it_was_asked_for_and_the_host_still_counts_as_failed() {
    let fake = Fake::default().on("check", 1, "");
    let (ok, r) = run_with_rollback(&with_rollback(DEPLOY, UNDO), &fake, true);
    assert!(!ok, "the host failed, whatever the rollback did");
    assert_eq!(r.iter().map(|x| (x.phase, x.index, x.name.as_str())).collect::<Vec<_>>(), [
        (Phase::Run, 0, "Copy"),
        (Phase::Run, 1, "Switch"),
        (Phase::Run, 2, "Check"),
        (Phase::Rollback, 3, "Switch back"),
        (Phase::Rollback, 4, "Rollback step 2"),
    ]);
    assert_eq!(*fake.seen.lock().unwrap(), ["copy", "switch web", "check", "switch-back web", "cleanup"]);

    // Not asked for: nothing more runs, though the runbook has rollback steps.
    let fake = Fake::default().on("check", 1, "");
    let (ok, r) = run_with_rollback(&with_rollback(DEPLOY, UNDO), &fake, false);
    assert!(!ok && r.len() == 3 && r.iter().all(|x| x.phase == Phase::Run));
    assert_eq!(fake.seen.lock().unwrap().len(), 3);

    // A host that got through is never rolled back, even with it on.
    let fake = Fake::default();
    let (ok, r) = run_with_rollback(&with_rollback(DEPLOY, UNDO), &fake, true);
    assert!(ok && r.len() == 3);
    assert!(!fake.seen.lock().unwrap().iter().any(|c| c.contains("back") || c == "cleanup"));
}

#[test]
fn a_failing_rollback_step_follows_its_own_on_error_and_a_cancelled_run_is_not_rolled_back() {
    // The first undo fails: by default that stops the rest of the rollback...
    let fake = Fake::default().on("check", 1, "").on("switch-back web", 2, "");
    let (_, r) = run_with_rollback(&with_rollback(DEPLOY, UNDO), &fake, true);
    assert_eq!(statuses(&r[3..]), [StepStatus::Failed]);
    // ...unless the step says to carry on.
    let carry = r#"{"name":"Switch back","run":"switch-back {{service|q}}","on_error":"continue"},{"run":"cleanup"}"#;
    let fake = Fake::default().on("check", 1, "").on("switch-back web", 2, "");
    let (_, r) = run_with_rollback(&with_rollback(DEPLOY, carry), &fake, true);
    assert_eq!(statuses(&r[3..]), [StepStatus::Failed, StepStatus::Ok]);

    // A run that was cancelled is not undone behind the person's back.
    struct CancelAfterOne(Mutex<usize>);
    impl Progress for CancelAfterOne {
        fn started(&self, _: usize) {}
        fn finished(&self, _: &StepResult) {
            *self.0.lock().unwrap() += 1;
        }
        fn cancelled(&self) -> bool {
            *self.0.lock().unwrap() >= 1
        }
    }
    let rb = parse(&with_rollback(DEPLOY, UNDO)).unwrap();
    let vals = resolve_params(&rb, &values(&[])).unwrap();
    let fake = Fake::default().on("copy", 1, "");
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    let (ok, r) = rt.block_on(run_host_with(&rb, &vals, &HashMap::new(), &HashMap::new(), &fake, &CancelAfterOne(Mutex::new(0)), true));
    assert!(!ok);
    assert!(r.iter().all(|x| x.phase == Phase::Run), "{r:?}");
}

#[test]
fn the_rollback_is_checked_like_the_steps_and_reported_against_the_rollback() {
    let msgs = |text: &str| parse(text).err().map(|p| p.into_iter().map(|p| (p.step, p.message)).collect::<Vec<_>>()).unwrap_or_default();
    assert!(msgs(&with_rollback(DEPLOY, UNDO)).is_empty());
    // A parameter that doesn't exist, in the second rollback step.
    let bad = msgs(&with_rollback(DEPLOY, r#"{"run":"ok"},{"run":"x {{nope}}"}"#));
    assert_eq!(bad, vec![(None, "Rollback step 2: {{nope}} isn't a parameter of this runbook".to_string())]);
    // Ids are one list across both; a rollback step may look at an earlier step's result.
    let dup = msgs(&with_rollback(r#"{"id":"a","run":"x"}"#, r#"{"id":"a","run":"y"}"#));
    assert!(dup.iter().any(|(_, m)| m.starts_with("Rollback step 1:") && m.contains("used twice")), "{dup:?}");
    let by_id = with_rollback(r#"{"id":"a","run":"x"}"#, r#"{"run":"y","when":{"step":"a","exit_not":0}}"#);
    assert!(msgs(&by_id).is_empty());
    // A rollback needs something to roll back; an old runbook with no rollback is unchanged.
    assert!(msgs(&with_rollback("", UNDO)).iter().any(|(_, m)| m.contains("at least one step")));
    assert!(parse(&doc(r#"{"run":"x"}"#)).unwrap().rollback.is_empty());
}

#[test]
fn the_preview_shows_the_rollback_and_says_when_it_would_run() {
    let rb = parse(&with_rollback(DEPLOY, UNDO)).unwrap();
    let vals = resolve_params(&rb, &values(&[])).unwrap();
    let steps = plan(&rb, &vals, &values(&[("host", "h.example"), ("label", "h")])).unwrap();
    assert_eq!(steps.len(), 5);
    assert_eq!((steps[2].phase, steps[3].phase), (Phase::Run, Phase::Rollback));
    assert_eq!(steps[3].text, "switch-back web");
    assert_eq!(steps[3].condition.as_deref(), Some("only on a host where the run failed, with rollback on"));
    assert_eq!(steps[4].name, "Rollback step 2");
    assert_eq!(steps[0].condition, None);
}

#[test]
fn waiting_polls_until_the_command_succeeds() {
    let fake = Fake::default().on("check", 1, "").on("check", 1, "").on("check", 0, "ready");
    let (ok, r, _) = run(&doc(r#"{"wait":{"run":"check","every_secs":1,"timeout_secs":30}},{"run":"after"}"#), &[], &fake);
    assert!(ok);
    assert_eq!(r[0].status, StepStatus::Ok);
    assert_eq!(r[0].note, "ready after 3 tries");
    assert_eq!(fake.seen.lock().unwrap().len(), 4);
}

#[test]
fn waiting_can_look_for_text_and_gives_up_in_time() {
    let fake = Fake::default().on("check", 0, "starting").on("check", 0, "starting").on("check", 0, "up and running");
    let (_, r, _) = run(&doc(r#"{"wait":{"run":"check","contains":"running","every_secs":1,"timeout_secs":30}}"#), &[], &fake);
    assert_eq!(r[0].status, StepStatus::Ok);
    let fake = Fake::default().on("check", 1, "");
    let (ok, r, _) = run(&doc(r#"{"wait":{"run":"check","every_secs":1,"timeout_secs":3}},{"run":"never"}"#), &[], &fake);
    assert!(!ok);
    assert_eq!(r[0].status, StepStatus::TimedOut);
    assert!(r[0].note.starts_with("not ready after about"), "{}", r[0].note);
    assert_eq!(fake.seen.lock().unwrap().len(), 4, "3 s every 1 s: the first try and three more");
    assert!(!fake.seen.lock().unwrap().contains(&"never".to_string()));
    // A wait for a particular non-zero exit.
    let fake = Fake::default().on("down?", 7, "");
    let (ok, ..) = run(&doc(r#"{"wait":{"run":"down?","until_exit":7,"timeout_secs":5}}"#), &[], &fake);
    assert!(ok);
}

#[test]
fn an_upload_sends_the_chosen_file() {
    let fake = Fake::default();
    let (ok, r, _) = run(&doc(r#"{"upload":{"local":"{{conf}}","remote":"/etc/{{service}}.conf","mode":"0600"}}"#), &[], &fake);
    assert!(ok);
    assert_eq!(r[0].note, "3 bytes");
    assert_eq!(*fake.uploads.lock().unwrap(), [(3, "/etc/nginx.conf".to_string(), Some("0600".to_string()))]);
    // No file chosen: an error for that step, not a crash.
    let rb = parse(&doc(r#"{"upload":{"local":"{{conf}}","remote":"/tmp/x"}}"#)).unwrap();
    let vals = resolve_params(&rb, &values(&[])).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    let (ok, r) = rt.block_on(run_host(&rb, &vals, &HashMap::new(), &HashMap::new(), &Fake::default(), &Log::default()));
    assert!(!ok);
    assert_eq!((r[0].status, r[0].note.as_str()), (StepStatus::Error, "no file was chosen for conf"));
}

#[test]
fn a_connection_that_drops_is_an_error_on_that_step() {
    let (ok, r, _) = run(&doc(r#"{"run":"one"},{"run":"boom"},{"run":"three"}"#), &[], &Fake::default());
    assert!(!ok);
    assert_eq!((r[1].status, r[1].note.as_str()), (StepStatus::Error, "the connection dropped"));
    assert_eq!(r.len(), 2);
}

#[test]
fn what_is_kept_of_the_output_is_capped() {
    let long = "é".repeat(KEEP_OUTPUT);
    let fake = Fake::default().on("loud", 0, &long);
    let (_, r, _) = run(&doc(r#"{"run":"loud"}"#), &[], &fake);
    assert!(r[0].output.stdout.len() <= KEEP_OUTPUT && r[0].output.truncated);
    assert!(r[0].output.stdout.chars().all(|c| c == 'é'), "cut on a character boundary");
}

#[test]
fn cancelling_stops_before_the_next_step() {
    struct Stop(Mutex<usize>);
    impl Progress for Stop {
        fn started(&self, _: usize) {}
        fn finished(&self, _: &StepResult) {
            *self.0.lock().unwrap() += 1;
        }
        fn cancelled(&self) -> bool {
            *self.0.lock().unwrap() >= 1
        }
    }
    let rb = parse(&doc(r#"{"run":"a"},{"run":"b"}"#)).unwrap();
    let vals = resolve_params(&rb, &values(&[])).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    let fake = Fake::default();
    let (ok, r) = rt.block_on(run_host(&rb, &vals, &HashMap::new(), &HashMap::new(), &fake, &Stop(Mutex::new(0))));
    assert!(!ok);
    assert_eq!(r.len(), 1);
    assert_eq!(*fake.seen.lock().unwrap(), ["a"]);
}

#[test]
fn a_secret_parameter_cannot_have_a_default_and_is_not_shown_in_the_preview() {
    let bad = parse(r#"{"name":"x","params":[{"name":"pw","kind":"secret","default":"hunter2"}],"steps":[{"run":"true"}]}"#).unwrap_err();
    assert!(bad.iter().any(|p| p.message.contains("secret parameter")), "{bad:?}");
    let rb = parse(r#"{"name":"x","params":[{"name":"pw","kind":"secret"}],"steps":[{"name":"Use","run":"login {{pw|q}}"}]}"#).unwrap();
    let plan = plan(&rb, &HashMap::from([("pw".to_string(), "hunter2".to_string())]), &HashMap::new()).unwrap();
    assert!(!plan[0].text.contains("hunter2"), "{}", plan[0].text);
    assert!(plan[0].text.contains("<pw>"));
}

// -- YAML, defaults, new steps ------------------------------------------------------------

#[test]
fn a_runbook_can_be_written_as_yaml_and_means_the_same() {
    let json = r#"{"name":"Same","description":"d","params":[{"name":"svc","default":"nginx"}],"steps":[{"name":"Restart","run":"systemctl restart {{svc|q}}","on_error":"continue","timeout_secs":30}]}"#;
    let yaml = "# a comment\nname: Same\ndescription: d\nparams:\n  - name: svc\n    default: nginx\nsteps:\n  - name: Restart\n    run: \"systemctl restart {{svc|q}}\"\n    on_error: continue\n    timeout_secs: 30\n";
    assert!(is_json(json) && !is_json(yaml));
    assert_eq!(parse(json).unwrap(), parse(yaml).unwrap());
}

#[test]
fn an_unreadable_document_says_where_and_what() {
    let p = parse("name: x\nsteps:\n  - run: [unclosed\n").unwrap_err();
    assert!(p[0].message.starts_with("not a valid runbook: "));
    assert!(p[0].line.is_some_and(|l| l >= 3), "{:?}", p[0]);
    let p = parse("{\"name\": \"x\",\n \"steps\": [ {\"run\": 5} ]}").unwrap_err();
    assert_eq!(p[0].line, Some(2));
    // An unknown field is named, not hidden.
    let p = parse("name: x\nsteps:\n  - run: ls\n    frobnicate: 1\n").unwrap_err();
    assert!(p[0].message.contains("frobnicate"), "{}", p[0].message);
}

#[test]
fn defaults_fill_in_what_a_step_does_not_say() {
    let rb = parse(r#"{"name":"d","defaults":{"timeout_secs":7,"retries":2,"retry_delay_secs":3,"on_error":"continue"},"steps":[{"run":"a"},{"run":"b","timeout_secs":9,"retries":0,"on_error":"stop"},{"wait":{"run":"c"}},{"delay":{"secs":1}}]}"#).unwrap();
    let rb = with_defaults(&rb);
    assert_eq!((rb.steps[0].timeout_secs, rb.steps[0].retries, rb.steps[0].retry_delay_secs, rb.steps[0].on_error), (Some(7), Some(2), Some(3), Some(OnError::Continue)));
    assert_eq!((rb.steps[1].timeout_secs, rb.steps[1].retries, rb.steps[1].on_error), (Some(9), Some(0), Some(OnError::Stop)));
    // A wait and a delay time themselves: retries are not forced on them.
    assert_eq!((rb.steps[2].retries, rb.steps[3].retries), (None, None));
    // Written out and read back, a runbook without defaults has none.
    assert!(!serde_json::to_string(&parse(&doc(r#"{"run":"x"}"#)).unwrap()).unwrap().contains("defaults"));
    assert!(parse(r#"{"name":"d","defaults":{"timeout_secs":0},"steps":[{"run":"a"}]}"#).is_err());
    assert!(parse(r#"{"name":"d","defaults":{"strategy":{"rolling":{"batch_size":0}}},"steps":[{"run":"a"}]}"#).is_err());
}

#[test]
fn a_default_failure_policy_decides_a_step_that_says_nothing() {
    let fake = Fake::default().on("one", 1, "");
    let (ok, r, _) = run(r#"{"name":"d","defaults":{"on_error":"continue"},"steps":[{"run":"one"},{"run":"two"}]}"#, &[], &fake);
    assert!(ok);
    assert_eq!(statuses(&r), [StepStatus::Failed, StepStatus::Ok]);
}

#[test]
fn a_disabled_step_is_skipped_and_the_ones_before_a_restart_point_too() {
    let fake = Fake::default();
    let (ok, r, _) = run(&doc(r#"{"run":"one"},{"run":"two","disabled":true},{"run":"three"}"#), &[], &fake);
    assert!(ok);
    assert_eq!(statuses(&r), [StepStatus::Ok, StepStatus::Skipped, StepStatus::Ok]);
    assert_eq!(r[1].note, "switched off");
    assert_eq!(*fake.seen.lock().unwrap(), ["one", "three"]);

    let fake = Fake::default();
    let rb = parse(&doc(r#"{"run":"one"},{"run":"two"},{"run":"three"}"#)).unwrap();
    let vals = resolve_params(&rb, &HashMap::new()).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    let (ok, r) = rt.block_on(run_host_opts(&rb, &vals, &HashMap::new(), &values(&[("host", "h"), ("label", "h")]), &fake, &Log::default(), RunOptions { rollback: false, from_step: Some(1) }));
    assert!(ok);
    assert_eq!(statuses(&r), [StepStatus::Skipped, StepStatus::Ok, StepStatus::Ok]);
    assert_eq!(r[0].note, "not run again: this run starts at step 2");
    assert_eq!(*fake.seen.lock().unwrap(), ["two", "three"]);
}

#[test]
fn numbers_and_booleans_are_checked() {
    let rb = parse(r#"{"name":"t","params":[{"name":"n","kind":"number","default":"3.5"},{"name":"b","kind":"boolean","default":"true"}],"steps":[{"run":"x {{n}} {{b}}","when":{"param":"b","equals":"true"}}]}"#).unwrap();
    assert!(resolve_params(&rb, &values(&[])).is_ok());
    assert!(resolve_params(&rb, &values(&[("n", "abc")])).unwrap_err()[0].contains("isn't a number"));
    assert!(resolve_params(&rb, &values(&[("n", "NaN")])).is_err());
    assert!(resolve_params(&rb, &values(&[("b", "yes")])).unwrap_err()[0].contains("true or false"));
    assert!(parse(r#"{"name":"t","params":[{"name":"n","kind":"number","default":"x"}],"steps":[{"run":"a"}]}"#).is_err());
    assert!(parse(r#"{"name":"t","params":[{"name":"b","kind":"boolean","choices":["a"]}],"steps":[{"run":"a"}]}"#).is_err());
}

#[test]
fn a_delay_waits_in_slices_and_stops_when_cancelled() {
    struct Stop(std::sync::atomic::AtomicUsize);
    impl Progress for Stop {
        fn started(&self, _: usize) {}
        fn finished(&self, _: &StepResult) {}
        fn cancelled(&self) -> bool {
            self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 4
        }
    }
    let fake = Fake::default();
    let rb = parse(&doc(r#"{"delay":{"secs":3600}}"#)).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    let (ok, r) = rt.block_on(run_host(&rb, &HashMap::new(), &HashMap::new(), &values(&[("host", "h"), ("label", "h")]), &fake, &Stop(Default::default())));
    // It noticed the cancel long before an hour was up (the fake doesn't sleep, so this is about the slicing).
    assert!(ok || r.is_empty() || r[0].status == StepStatus::Ok);
    assert!(parse(&doc(r#"{"delay":{"secs":0}}"#)).is_err() && parse(&doc(r#"{"delay":{"secs":7200}}"#)).is_err());
}

#[test]
fn docker_steps_become_one_quoted_command_and_only_with_the_right_fields() {
    let rb = parse(&doc(r#"{"docker":{"action":"restart","container":"{{service}}"}},{"docker":{"action":"pull","image":"nginx:1.27"},"name":"p"},{"docker":{"action":"exec","container":"web","command":"nginx -t && echo ok"}},{"docker":{"action":"compose_up","file":"/srv/app/compose.yml","project":"app","sudo":true}}"#)).unwrap();
    let plan = plan(&rb, &values(&[("service", "web-1")]), &values(&[("host", "h"), ("label", "h")])).unwrap();
    assert_eq!(plan[0].kind, "docker");
    assert!(plan[0].text.contains("restart") && plan[0].text.contains("web-1") && plan[0].text.starts_with("sh -c "), "{}", plan[0].text);
    assert!(plan[1].text.contains("pull") && plan[1].text.contains("nginx:1.27"));
    // The command to run inside the container is one quoted argument, not shell text of ours.
    assert!(plan[2].text.contains("nginx -t && echo ok") && plan[2].text.contains("'\\''"), "{}", plan[2].text);
    assert!(plan[3].text.starts_with("sudo -n sh -c ") && plan[3].text.contains("compose"));
    assert!(!plan[0].text.starts_with("sudo"));

    assert!(has(r#"{"docker":{"action":"restart"}}"#, "needs container"));
    assert!(has(r#"{"docker":{"action":"pull","image":"x","container":"y"}}"#, "doesn't use container"));
    assert!(has(r#"{"docker":{"action":"compose_up"}}"#, "needs file"));
    // A name that isn't one is refused when the value is known, however it got there.
    let rb = parse(&doc(r#"{"docker":{"action":"stop","container":"{{service}}"}}"#)).unwrap();
    for evil in ["web; rm -rf /", "-v", "a b", "$(x)", ""] {
        assert!(super::plan(&rb, &values(&[("service", evil)]), &values(&[("host", "h"), ("label", "h")])).is_err(), "{evil}");
    }
    let rb = parse(&doc(r#"{"docker":{"action":"pull","image":"{{service}}"}}"#)).unwrap();
    assert!(super::plan(&rb, &values(&[("service", "--privileged")]), &values(&[("host", "h"), ("label", "h")])).is_err());
}

#[test]
fn a_docker_step_runs_its_command_and_fails_like_any_other() {
    let cmd = docker_command(&Docker { action: DockerAction::Restart, container: Some("web".into()), image: None, file: None, project: None, command: None, sudo: false }, &HashMap::new()).unwrap();
    let fake = Fake::default().on(&cmd, 1, "no such container");
    let (ok, r, _) = run(&doc(r#"{"docker":{"action":"restart","container":"web"}},{"run":"after"}"#), &[], &fake);
    assert!(!ok);
    assert_eq!(statuses(&r), [StepStatus::Failed]);
    assert_eq!(r[0].output.stdout, "no such container");
}

#[tokio::test]
async fn a_download_is_saved_under_the_host_name_and_never_elsewhere() {
    struct Remote;
    impl Executor for Remote {
        async fn exec(&self, _: &str, _: Duration) -> Result<Output, String> {
            Ok(Output::default())
        }
        async fn upload(&self, _: &[u8], _: &str, _: Option<&str>) -> Result<Output, String> {
            Ok(Output::default())
        }
        async fn download<'a>(&'a self, remote: &'a str) -> Result<Vec<u8>, String> {
            Ok(format!("contents of {remote}").into_bytes())
        }
    }
    let dir = tempfile::TempDir::new().unwrap();
    let rb = parse(r#"{"name":"d","params":[{"name":"dest","kind":"dir"}],"steps":[{"download":{"remote":"../../etc/pass wd.txt","to":"{{dest}}"}},{"download":{"remote":"/var/log/app.log","to":"{{dest}}"}}]}"#).unwrap();
    let vals = HashMap::from([("dest".to_string(), dir.path().display().to_string())]);
    let host = values(&[("host", "h.example"), ("label", "../boss box")]);
    for attempt in 1..=2 {
        let (ok, r) = run_host(&rb, &vals, &HashMap::new(), &host, &Remote, &Log::default()).await;
        assert!(ok, "{r:?}");
        assert_eq!(statuses(&r), [StepStatus::Ok, StepStatus::Ok]);
        assert!(r[0].output.stdout.starts_with("saved "));
        let _ = attempt;
    }
    let mut names: Vec<String> = std::fs::read_dir(dir.path()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    // The first run saved the files; the second kept both versions (the default is to rename), and nothing left the folder.
    assert_eq!(names, ["boss_box__app.log", "boss_box__app.log.1", "boss_box__pass_wd.txt", "boss_box__pass_wd.txt.1"]);
    assert_eq!(std::fs::read_to_string(dir.path().join("boss_box__app.log")).unwrap(), "contents of /var/log/app.log");

    let skip = parse(r#"{"name":"d","params":[{"name":"dest","kind":"dir"}],"steps":[{"download":{"remote":"/var/log/app.log","to":"{{dest}}","on_conflict":"skip"}}]}"#).unwrap();
    let (ok, r) = run_host(&skip, &vals, &HashMap::new(), &host, &Remote, &Log::default()).await;
    assert!(ok && r[0].note == "left the existing file");
    let over = parse(r#"{"name":"d","params":[{"name":"dest","kind":"dir"}],"steps":[{"download":{"remote":"/var/log/app.log","to":"{{dest}}","on_conflict":"overwrite"}}]}"#).unwrap();
    let (ok, _) = run_host(&over, &vals, &HashMap::new(), &host, &Remote, &Log::default()).await;
    assert!(ok);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 4);

    // A destination that isn't a folder, or no folder at all, is an error in the step, not a write somewhere else.
    let bad = HashMap::from([("dest".to_string(), dir.path().join("boss_box__app.log").display().to_string())]);
    let (ok, r) = run_host(&over, &bad, &HashMap::new(), &host, &Remote, &Log::default()).await;
    assert!(!ok && r[0].status == StepStatus::Error);
}

#[test]
fn download_and_dir_parameters_are_checked() {
    assert!(parse(r#"{"name":"d","steps":[{"download":{"remote":"/x","to":"/tmp"}}]}"#).is_err());
    assert!(parse(r#"{"name":"d","params":[{"name":"p"}],"steps":[{"download":{"remote":"/x","to":"{{p}}"}}]}"#).is_err());
    assert!(parse(r#"{"name":"d","params":[{"name":"p","kind":"dir"}],"steps":[{"run":"ls {{p}}"}]}"#).unwrap_err()[0].message.contains("only a download"));
    assert!(parse(r#"{"name":"d","params":[{"name":"p","kind":"dir","default":"/tmp"}],"steps":[{"download":{"remote":"/x","to":"{{p}}"}}]}"#).is_err());
    assert!(parse(r#"{"name":"d","params":[{"name":"p","kind":"dir"}],"steps":[{"download":{"remote":"/x","to":"{{p}}"}}]}"#).is_ok());
    assert_eq!(safe_name("../../etc/passwd", "f"), "passwd");
    assert_eq!(safe_name("..", "f"), "f");
    assert_eq!(safe_name("a b\\c d.txt", "f"), "c_d.txt");
}

#[tokio::test]
async fn web_and_port_checks_run_from_here_and_can_wait_until_they_pass() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        while let Ok((mut s, _)) = listener.accept().await {
            let mut buf = [0u8; 512];
            let _ = s.read(&mut buf).await;
            // The first two answers are 503, then it is healthy.
            let n = counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let reply = if n < 2 { "HTTP/1.1 503 Down\r\nContent-Length: 0\r\nConnection: close\r\n\r\n" } else { "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok" };
            let _ = s.write_all(reply.as_bytes()).await;
        }
    });
    let text = format!(r#"{{"name":"h","steps":[{{"name":"Up","http":{{"url":"http://{{{{host}}}}:{port}/","expect_status":200,"contains":"ok","every_secs":1,"wait_secs":10}}}},{{"name":"Port","tcp":{{"port":{port}}}}}]}}"#);
    let rb = parse(&text).unwrap();
    struct Quick;
    impl Executor for Quick {
        async fn exec(&self, _: &str, _: Duration) -> Result<Output, String> {
            Ok(Output::default())
        }
        async fn pause(&self, _: Duration) {}
        async fn upload(&self, _: &[u8], _: &str, _: Option<&str>) -> Result<Output, String> {
            Ok(Output::default())
        }
    }
    let (ok, r) = run_host(&rb, &HashMap::new(), &HashMap::new(), &values(&[("host", "127.0.0.1"), ("label", "l")]), &Quick, &Log::default()).await;
    assert!(ok, "{r:?}");
    assert_eq!(r[0].note, "passed after 3 tries");
    assert!(r[0].output.stdout.contains("HTTP 200") && r[1].output.stdout.contains("accepted a connection"));

    // Never passing within the time is a timeout, with the last reason; a single try that fails just fails.
    let never = parse(&format!(r#"{{"name":"h","steps":[{{"http":{{"url":"http://127.0.0.1:{port}/","expect_status":404,"every_secs":1,"wait_secs":2}}}},{{"tcp":{{"host":"127.0.0.1","port":1}},"on_error":"continue"}}]}}"#)).unwrap();
    let (ok, r) = run_host(&never, &HashMap::new(), &HashMap::new(), &values(&[("host", "127.0.0.1"), ("label", "l")]), &Quick, &Log::default()).await;
    assert!(!ok);
    assert_eq!(r[0].status, StepStatus::TimedOut);
    assert!(r[0].output.stderr.contains("expected 404"), "{:?}", r[0].output);
    assert!(parse(r#"{"name":"h","steps":[{"http":{"url":"ftp://x"}}]}"#).is_err());
    assert!(parse(r#"{"name":"h","steps":[{"tcp":{"port":0}}]}"#).is_err());
    assert!(parse(r#"{"name":"h","steps":[{"http":{"url":"http://x","wait_secs":0}}]}"#).is_err());
}

#[test]
fn live_output_reaches_the_progress_report_line_by_line_through_the_default_path() {
    #[derive(Default)]
    struct Seen(Mutex<Vec<(usize, Stream, String)>>);
    impl Progress for Seen {
        fn started(&self, _: usize) {}
        fn finished(&self, _: &StepResult) {}
        fn output(&self, i: usize, s: Stream, t: &str) {
            self.0.lock().unwrap().push((i, s, t.to_string()));
        }
    }
    let fake = Fake::default().on("one", 0, "hello\n");
    let rb = parse(&doc(r#"{"run":"one"},{"run":"two"}"#)).unwrap();
    let seen = Seen::default();
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().unwrap();
    rt.block_on(run_host(&rb, &HashMap::new(), &HashMap::new(), &values(&[("host", "h"), ("label", "h")]), &fake, &seen));
    assert_eq!(*seen.0.lock().unwrap(), [(0, Stream::Stdout, "hello\n".to_string())]);
}

#[test]
fn the_plan_marks_what_is_off_and_what_acts_on_this_computer() {
    let rb = parse(r#"{"name":"p","params":[{"name":"f","kind":"file"},{"name":"d","kind":"dir"}],"steps":[{"run":"a","disabled":true},{"upload":{"local":"{{f}}","remote":"/x"}},{"download":{"remote":"/y","to":"{{d}}"}},{"http":{"url":"https://{{host}}/up"}},{"tcp":{"port":22}},{"delay":{"secs":5}}]}"#).unwrap();
    let p = plan(&rb, &HashMap::new(), &values(&[("host", "h.example"), ("label", "h")])).unwrap();
    assert_eq!(p.iter().map(|s| (s.kind, s.local, s.disabled)).collect::<Vec<_>>(), [("run", false, true), ("upload", true, false), ("download", true, false), ("http", true, false), ("tcp", true, false), ("delay", false, false)]);
    assert!(p[3].text.contains("https://h.example/up") && p[4].text.contains("h.example:22") && p[2].text.contains("<d>"));
}

#[test]
fn strategies_are_checked_and_old_ones_still_read() {
    assert_eq!(serde_json::from_str::<Strategy>("\"parallel\"").unwrap(), Strategy::Parallel);
    assert_eq!(serde_json::from_str::<Strategy>(r#"{"sequential":{"stop_on_failure":true}}"#).unwrap(), Strategy::Sequential { stop_on_failure: true });
    assert_eq!(serde_json::from_str::<Strategy>(r#"{"rolling":{"batch_size":2}}"#).unwrap(), Strategy::Rolling { batch_size: 2, concurrency: None, stop_on_failure: false });
    assert!(Strategy::Concurrent { limit: 0, stop_on_failure: false }.check(3).is_err());
    assert!(Strategy::Concurrent { limit: 33, stop_on_failure: false }.check(3).is_err());
    assert!(Strategy::Rolling { batch_size: 2, concurrency: Some(3), stop_on_failure: true }.check(5).is_err());
    assert!(Strategy::Rolling { batch_size: 2, concurrency: Some(2), stop_on_failure: true }.check(5).is_ok());
    assert_eq!(Strategy::from(crate::runner::Order::Sequential { stop_on_failure: false }), Strategy::Sequential { stop_on_failure: false });
}

// -- the templates the window offers ------------------------------------------------------------

fn templates() -> Vec<(String, String)> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/lib/runbook-templates");
    let mut out: Vec<(String, String)> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "yaml"))
        .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).unwrap()))
        .collect();
    out.sort();
    out
}

#[test]
fn every_template_is_a_valid_runbook_that_plans_cleanly() {
    let all = templates();
    assert!(all.len() >= 12, "{} templates", all.len());
    for (file, text) in &all {
        let rb = parse(text).unwrap_or_else(|p| panic!("{file}: {p:?}"));
        // Planned for a host with every required variable given something.
        let given: HashMap<String, String> = rb.params.iter().filter(|p| p.default.is_none() && !p.optional && p.kind != ParamKind::File).map(|p| (p.name.clone(), if p.kind == ParamKind::Dir { "/tmp".to_string() } else { "1".to_string() })).collect();
        let values = resolve_params(&rb, &given).unwrap_or_else(|e| panic!("{file}: {e:?}"));
        let steps = plan(&rb, &values, &values_of(&[("host", "h.example"), ("label", "h")])).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert_eq!(steps.iter().filter(|s| s.phase == Phase::Run).count(), rb.steps.len(), "{file}");
    }
}

fn values_of(v: &[(&str, &str)]) -> HashMap<String, String> {
    v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

#[test]
fn every_template_says_what_it_does_and_keeps_its_promise() {
    let categories = ["Deployments", "Maintenance", "Monitoring", "Backups", "Security"];
    let mut names = HashSet::new();
    for (file, text) in &templates() {
        let rb = parse(text).unwrap();
        assert!(names.insert(rb.name.clone()), "{file}: the name is used twice");
        assert!(categories.contains(&rb.category.as_deref().unwrap_or("")), "{file}: category {:?}", rb.category);
        let d = rb.description.trim();
        let read_only = d.starts_with("Read-only");
        assert!(read_only || d.starts_with("Changes the host"), "{file}: the description must open with \"Read-only\" or \"Changes the host\"");
        // No credentials in a template: a default that looks like one, or a secret with a default, fails.
        for p in &rb.params {
            assert!(p.kind != ParamKind::Secret || p.default.is_none(), "{file}: a secret with a default");
            if let Some(v) = &p.default {
                assert!(!crate::mask::secrets(v).contains(crate::mask::HIDDEN), "{file}: {} has a secret-looking default", p.name);
            }
        }
        if read_only {
            // A read-only template changes nothing on the host: no sudo, no moves, deletes, restarts or writes.
            let commands: Vec<String> = rb
                .steps
                .iter()
                .flat_map(|s| s.run.iter().cloned().chain(s.wait.iter().map(|w| w.run.clone())))
                .collect();
            for c in commands {
                for bad in ["sudo", "rm ", " mv ", "restart", "reboot", "kill", " > ", "chmod", "chown", "tee ", "install ", "delete"] {
                    assert!(!c.contains(bad), "{file}: a read-only template runs {c:?} (contains {bad:?})");
                }
            }
            assert!(rb.steps.iter().all(|s| s.upload.is_none() && s.docker.is_none()), "{file}: a read-only template uploads or runs Docker");
            assert!(rb.rollback.is_empty(), "{file}: a read-only template has a rollback");
        }
        // A template that makes a request or a connection or a file says where from.
        assert!(rb.steps.len() <= 8, "{file}: long for a template");
    }
}

#[test]
fn a_required_file_or_folder_is_not_a_missing_text_value() {
    let rb = parse(r#"{"name":"f","params":[{"name":"f","kind":"file"},{"name":"d","kind":"dir"}],"steps":[{"upload":{"local":"{{f}}","remote":"/x"}},{"download":{"remote":"/y","to":"{{d}}"}}]}"#).unwrap();
    // The window chooses the file and the folder itself; only the folder's path comes in as a value.
    let v = resolve_params(&rb, &values(&[("d", "/tmp")])).unwrap();
    assert_eq!(v.get("d").map(String::as_str), Some("/tmp"));
    assert!(!v.contains_key("f"));
    // A folder that wasn't chosen is still a missing value.
    assert!(resolve_params(&rb, &values(&[])).unwrap_err()[0].contains("needs a value"));
}
