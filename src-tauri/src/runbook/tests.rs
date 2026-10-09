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
    assert!(has(r#"{"name":"x"}"#, "exactly one of run, wait or upload"));
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
    assert!(has(r#"{"run":"a","when":{"param":"nope","equals":"x"}}"#, "text parameter"));
    assert!(has(r#"{"run":"a","when":{"param":"conf","equals":"x"}}"#, "text parameter"));
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
