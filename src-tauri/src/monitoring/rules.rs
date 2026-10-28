//! Judging readings against rules, and remembering the state of each so only a change is reported.
//!
//! A reading is classified OK / WARNING / CRITICAL against a rule's limits. A state does not change on one reading:
//! it takes `attempts` readings in a row that agree (Checkmk's "max check attempts"), so a lost packet or a CPU spike
//! does not page anyone. Getting better is not delayed: one OK reading clears a problem. Only a confirmed change is
//! returned, and that is what gets sent, so a host that stays CRITICAL is announced once, not on every check.
//!
//! A check that could not run (the host's SSH credentials are missing, `ping` is not installed) is UNKNOWN. That
//! shows in the window, but it neither changes the confirmed state nor sends anything.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::model::{AlertRule, Metric, Op};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warning,
    Critical,
    Unknown,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Warning => "WARNING",
            Status::Critical => "CRITICAL",
            Status::Unknown => "UNKNOWN",
        }
    }
}

/// OK, WARNING or CRITICAL for `value`. The critical limit is checked first.
pub fn classify(op: Op, warn: Option<f64>, crit: Option<f64>, value: f64) -> Status {
    let past = |limit: f64| match op {
        Op::Gt => value > limit,
        Op::Lt => value < limit,
    };
    if crit.is_some_and(past) {
        Status::Critical
    } else if warn.is_some_and(past) {
        Status::Warning
    } else {
        Status::Ok
    }
}

fn number(v: f64, unit: &str) -> String {
    if (v - v.round()).abs() < 0.05 {
        format!("{}{unit}", v.round() as i64)
    } else {
        format!("{v:.1}{unit}")
    }
}

/// One sentence about a reading, for the alert text: `CPU use is 95% (limit 90%)`.
pub fn describe(rule: &AlertRule, value: f64, status: Status) -> String {
    let m = rule.metric;
    if m == Metric::PortClosed {
        return if value >= 1.0 { "The port is closed: nothing accepted the connection".to_string() } else { "The port accepts connections".to_string() };
    }
    let limit = match status {
        Status::Critical => rule.crit,
        Status::Warning => rule.warn,
        _ => rule.warn.or(rule.crit),
    };
    let word = match rule.op {
        Op::Gt => "limit",
        Op::Lt => "minimum",
    };
    match limit {
        Some(l) => format!("{} is {} ({word} {})", m.label(), number(value, m.unit()), number(l, m.unit())),
        None => format!("{} is {}", m.label(), number(value, m.unit())),
    }
}

/// What one state belongs to: a rule applied to a host, and for some metrics a part of the host (a port).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StateKey {
    pub target: Uuid,
    pub rule: Uuid,
    /// Empty, or for example `tcp/22`.
    pub subject: String,
}

#[derive(Debug, Clone)]
struct Slot {
    confirmed: Status,
    /// A different status being seen, and for how many readings in a row.
    pending: Option<(Status, u8)>,
    unknown: bool,
    value: Option<f64>,
    since: u64,
}

/// A confirmed change of state: the thing worth telling someone about.
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub key: StateKey,
    pub from: Status,
    pub to: Status,
    pub value: Option<f64>,
    pub at: u64,
}

/// What the window shows for one state.
#[derive(Debug, Clone, PartialEq)]
pub struct Shown {
    pub status: Status,
    pub value: Option<f64>,
    pub since: u64,
}

#[derive(Debug, Default)]
pub struct Evaluator {
    slots: HashMap<StateKey, Slot>,
}

impl Evaluator {
    /// Record one reading. `raw` is what this reading alone says. Returns a transition only when the confirmed state
    /// changes. A new state starts as OK, so a host that is fine produces nothing.
    pub fn observe(&mut self, key: &StateKey, raw: Status, value: Option<f64>, attempts: u8, now: u64) -> Option<Transition> {
        let slot = self.slots.entry(key.clone()).or_insert(Slot { confirmed: Status::Ok, pending: None, unknown: false, value: None, since: now });
        if raw == Status::Unknown {
            slot.unknown = true;
            return None;
        }
        slot.unknown = false;
        slot.value = value;
        if raw == slot.confirmed {
            slot.pending = None;
            return None;
        }
        let seen = match slot.pending {
            Some((status, n)) if status == raw => n.saturating_add(1),
            _ => 1,
        };
        // Recovery does not wait; a problem does, so that it has to be real.
        if raw != Status::Ok && seen < attempts.max(1) {
            slot.pending = Some((raw, seen));
            return None;
        }
        let from = slot.confirmed;
        slot.confirmed = raw;
        slot.pending = None;
        slot.since = now;
        Some(Transition { key: key.clone(), from, to: raw, value, at: now })
    }

    pub fn shown(&self, key: &StateKey) -> Option<Shown> {
        self.slots.get(key).map(|s| Shown { status: if s.unknown { Status::Unknown } else { s.confirmed }, value: s.value, since: s.since })
    }

    /// Forget states that no longer belong to anything (a deleted rule or host).
    pub fn retain(&mut self, keep: impl Fn(&StateKey) -> bool) {
        self.slots.retain(|k, _| keep(k));
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitoring::model::Scope;

    fn key() -> StateKey {
        StateKey { target: Uuid::from_u128(1), rule: Uuid::from_u128(2), subject: String::new() }
    }

    fn rule(metric: Metric) -> AlertRule {
        AlertRule { name: "r".into(), enabled: true, metric, op: Op::Gt, warn: Some(80.0), crit: Some(95.0), attempts: 3, scope: Scope::All, channels: vec![] }
    }

    #[test]
    fn classification_checks_critical_first_and_honours_the_direction() {
        let c = |v| classify(Op::Gt, Some(80.0), Some(95.0), v);
        assert_eq!((c(50.0), c(80.0), c(81.0), c(95.0), c(96.0)), (Status::Ok, Status::Ok, Status::Warning, Status::Warning, Status::Critical));
        assert_eq!(classify(Op::Gt, None, Some(0.0), 1.0), Status::Critical);
        assert_eq!(classify(Op::Gt, Some(0.0), None, 0.0), Status::Ok, "loss > 0% needs some loss");
        let below = |v| classify(Op::Lt, Some(20.0), Some(10.0), v);
        assert_eq!((below(30.0), below(15.0), below(5.0)), (Status::Ok, Status::Warning, Status::Critical));
    }

    #[test]
    fn a_problem_needs_several_readings_in_a_row_and_is_announced_once() {
        let mut e = Evaluator::default();
        assert_eq!(e.observe(&key(), Status::Ok, Some(1.0), 3, 1), None, "a healthy host says nothing");
        assert_eq!(e.observe(&key(), Status::Critical, Some(99.0), 3, 2), None);
        assert_eq!(e.observe(&key(), Status::Critical, Some(99.0), 3, 3), None);
        let t = e.observe(&key(), Status::Critical, Some(98.0), 3, 4).expect("third in a row");
        assert_eq!((t.from, t.to, t.value, t.at), (Status::Ok, Status::Critical, Some(98.0), 4));
        for now in 5..20 {
            assert_eq!(e.observe(&key(), Status::Critical, Some(99.0), 3, now), None, "no repeat while it stays critical");
        }
    }

    #[test]
    fn one_good_reading_resets_the_count_and_a_blip_never_alerts() {
        let mut e = Evaluator::default();
        for now in 0..30 {
            // Bad, bad, good, bad, bad, good...: never three in a row.
            let raw = if now % 3 == 2 { Status::Ok } else { Status::Critical };
            assert_eq!(e.observe(&key(), raw, None, 3, now), None, "reading {now}");
        }
    }

    #[test]
    fn recovery_is_immediate() {
        let mut e = Evaluator::default();
        for now in 0..3 {
            e.observe(&key(), Status::Warning, Some(85.0), 3, now);
        }
        let t = e.observe(&key(), Status::Ok, Some(10.0), 3, 10).expect("recovered");
        assert_eq!((t.from, t.to), (Status::Warning, Status::Ok));
        assert_eq!(e.observe(&key(), Status::Ok, Some(10.0), 3, 11), None);
    }

    #[test]
    fn escalating_and_easing_also_need_the_readings_in_a_row() {
        let mut e = Evaluator::default();
        for now in 0..2 {
            e.observe(&key(), Status::Warning, None, 2, now);
        }
        assert_eq!(e.shown(&key()).unwrap().status, Status::Warning);
        assert_eq!(e.observe(&key(), Status::Critical, None, 2, 10), None);
        let up = e.observe(&key(), Status::Critical, None, 2, 11).expect("critical");
        assert_eq!((up.from, up.to), (Status::Warning, Status::Critical));
        assert_eq!(e.observe(&key(), Status::Warning, None, 2, 12), None);
        let down = e.observe(&key(), Status::Warning, None, 2, 13).expect("eased");
        assert_eq!((down.from, down.to), (Status::Critical, Status::Warning));
    }

    #[test]
    fn a_pending_count_for_one_status_does_not_carry_over_to_another() {
        let mut e = Evaluator::default();
        assert_eq!(e.observe(&key(), Status::Warning, None, 2, 1), None);
        assert_eq!(e.observe(&key(), Status::Critical, None, 2, 2), None, "different status starts again");
        assert!(e.observe(&key(), Status::Critical, None, 2, 3).is_some());
    }

    #[test]
    fn unknown_shows_but_changes_and_sends_nothing() {
        let mut e = Evaluator::default();
        for now in 0..3 {
            e.observe(&key(), Status::Critical, None, 3, now);
        }
        assert_eq!(e.observe(&key(), Status::Unknown, None, 3, 10), None);
        assert_eq!(e.shown(&key()).unwrap().status, Status::Unknown);
        // The confirmed state was kept, so coming back still critical is not a new alert.
        assert_eq!(e.observe(&key(), Status::Critical, Some(99.0), 3, 11), None);
        assert_eq!(e.shown(&key()).unwrap().status, Status::Critical);
        assert_eq!(e.shown(&StateKey { subject: "x".into(), ..key() }), None);
    }

    #[test]
    fn one_attempt_alerts_at_once_and_zero_is_treated_as_one() {
        let mut e = Evaluator::default();
        assert!(e.observe(&key(), Status::Critical, None, 1, 1).is_some());
        let mut e = Evaluator::default();
        assert!(e.observe(&key(), Status::Critical, None, 0, 1).is_some());
    }

    #[test]
    fn states_are_kept_apart_by_rule_host_and_subject() {
        let mut e = Evaluator::default();
        let other = StateKey { subject: "tcp/22".into(), ..key() };
        assert!(e.observe(&key(), Status::Critical, None, 1, 1).is_some());
        assert!(e.observe(&other, Status::Critical, None, 1, 1).is_some());
        assert_eq!(e.len(), 2);
        e.retain(|k| k.subject.is_empty());
        assert_eq!(e.len(), 1);
        assert!(!e.is_empty());
    }

    #[test]
    fn alert_sentences() {
        let r = rule(Metric::Cpu);
        assert_eq!(describe(&r, 97.0, Status::Critical), "CPU use is 97% (limit 95%)");
        assert_eq!(describe(&r, 85.5, Status::Warning), "CPU use is 85.5% (limit 80%)");
        assert_eq!(describe(&r, 12.0, Status::Ok), "CPU use is 12% (limit 80%)");
        let lat = AlertRule { warn: Some(200.0), crit: None, ..rule(Metric::PingLatency) };
        assert_eq!(describe(&lat, 250.4, Status::Warning), "Ping latency is 250.4 ms (limit 200 ms)");
        assert_eq!(describe(&rule(Metric::PortClosed), 1.0, Status::Critical), "The port is closed: nothing accepted the connection");
        let minimum = AlertRule { op: Op::Lt, warn: Some(20.0), crit: None, ..rule(Metric::Disk) };
        assert_eq!(describe(&minimum, 12.0, Status::Warning), "Disk use is 12% (minimum 20%)");
    }
}
