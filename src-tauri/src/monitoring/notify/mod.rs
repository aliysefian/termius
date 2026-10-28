//! Sending an alert out: what a message says, and the dispatcher that hands it to every channel the rule allows.
//!
//! A [`Channel`] is one way out (a Telegram bot, a Bale bot, an email account). The dispatcher does not know how any
//! of them work: it sends to each in parallel, gives each 20 seconds, tries once more if one fails, and stops a
//! flood: a channel is sent at most 20 messages a minute, and the next one that goes through says how many it held
//! back.

pub mod bot;
pub mod smtp;

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::model::ChannelKind;
use super::rules::Status;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Why a message did not go out. Never holds a token or a password.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotifyError(pub String);

impl std::fmt::Display for NotifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

pub trait Channel: Send + Sync {
    fn kind(&self) -> ChannelKind;
    fn send<'a>(&'a self, n: &'a Notification) -> BoxFuture<'a, Result<(), NotifyError>>;
}

/// One alert, ready to be written out.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Notification {
    pub status: Status,
    pub previous: Status,
    /// The host's name as the person gave it.
    pub target: String,
    /// The host's address. Left out of the text unless the person turned that on.
    pub address: Option<String>,
    pub rule: String,
    /// What the reading was, in a sentence.
    pub summary: String,
    /// Unix seconds.
    pub at: u64,
    /// A line added by the dispatcher, such as how many alerts a rate limit held back.
    pub note: Option<String>,
}

impl Notification {
    fn icon(&self) -> &'static str {
        match self.status {
            Status::Critical => "\u{1F534}",
            Status::Warning => "\u{1F7E0}",
            Status::Ok => "\u{1F7E2}",
            Status::Unknown => "\u{26AA}",
        }
    }

    /// `CRITICAL`, or `OK` for a recovery.
    fn headline(&self) -> String {
        match self.status {
            Status::Ok if self.previous != Status::Ok => format!("RECOVERED: {}", self.target),
            Status::Ok => self.target.clone(),
            s => format!("{}: {}", s.label(), self.target),
        }
    }

    /// A one-line subject.
    pub fn subject(&self) -> String {
        match self.status {
            Status::Ok if self.previous != Status::Ok => format!("[OK] {}: {} recovered", self.target, self.rule),
            Status::Ok => format!("[OK] {}: {}", self.target, self.rule),
            s => format!("[{}] {}: {}", s.label(), self.target, self.rule),
        }
    }

    /// The message text, plain so that no chat service needs escaping.
    pub fn text(&self) -> String {
        let mut out = format!("{} {}\n{}\nRule: {}\n", self.icon(), self.headline(), self.summary, self.rule);
        if self.previous != self.status {
            out.push_str(&format!("Was: {}\n", self.previous.label()));
        }
        if let Some(a) = &self.address {
            out.push_str(&format!("Address: {a}\n"));
        }
        out.push_str(&format!("Time: {}", fmt_utc(self.at)));
        if let Some(note) = &self.note {
            out.push_str(&format!("\n{note}"));
        }
        out
    }
}

/// `2026-10-28 14:03:07 UTC`.
pub fn fmt_utc(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    let rem = secs % 86_400;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Year, month and day of a day count from 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// The outcome of one channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Delivery {
    pub channel: ChannelKind,
    /// `None` when it was sent; otherwise why not.
    pub error: Option<String>,
}

const PER_MINUTE: usize = 20;

#[derive(Default)]
struct Bucket {
    sent: Vec<Instant>,
    held_back: usize,
}

pub struct Dispatcher {
    buckets: Mutex<HashMap<ChannelKind, Bucket>>,
    pub timeout: Duration,
    pub retry_after: Duration,
}

impl Default for Dispatcher {
    fn default() -> Self {
        Self { buckets: Mutex::default(), timeout: Duration::from_secs(20), retry_after: Duration::from_secs(2) }
    }
}

impl Dispatcher {
    /// How long to wait before the one retry.
    pub fn with_retry_after(mut self, d: Duration) -> Self {
        self.retry_after = d;
        self
    }

    /// Take a slot for `kind`. `Ok` carries how many earlier messages were held back (to be mentioned now);
    /// `Err` means this one is held back too.
    fn admit(&self, kind: ChannelKind) -> Result<usize, ()> {
        let mut all = self.buckets.lock().unwrap_or_else(|p| p.into_inner());
        let b = all.entry(kind).or_default();
        let now = Instant::now();
        b.sent.retain(|t| now.duration_since(*t) < Duration::from_secs(60));
        if b.sent.len() >= PER_MINUTE {
            b.held_back += 1;
            return Err(());
        }
        b.sent.push(now);
        Ok(std::mem::take(&mut b.held_back))
    }

    /// Send `n` on every channel in `channels` that the rule allows (`only` empty allows all), all at once.
    pub async fn dispatch(&self, n: &Notification, channels: &[Arc<dyn Channel>], only: &[ChannelKind]) -> Vec<Delivery> {
        let mut set = tokio::task::JoinSet::new();
        let mut out: Vec<Delivery> = Vec::new();
        for ch in channels {
            let kind = ch.kind();
            if !only.is_empty() && !only.contains(&kind) {
                continue;
            }
            let held = match self.admit(kind) {
                Ok(h) => h,
                Err(()) => {
                    out.push(Delivery { channel: kind, error: Some("held back: more than 20 alerts in a minute".into()) });
                    continue;
                }
            };
            let mut msg = n.clone();
            if held > 0 {
                msg.note = Some(format!("({held} earlier alert(s) were held back by the limit of {PER_MINUTE} a minute.)"));
            }
            let (ch, timeout, retry_after) = (Arc::clone(ch), self.timeout, self.retry_after);
            set.spawn(async move {
                let mut error = None;
                for attempt in 0..2 {
                    if attempt > 0 {
                        tokio::time::sleep(retry_after).await;
                    }
                    match tokio::time::timeout(timeout, ch.send(&msg)).await {
                        Ok(Ok(())) => {
                            error = None;
                            break;
                        }
                        Ok(Err(e)) => error = Some(e.0),
                        Err(_) => error = Some(format!("no answer within {} s", timeout.as_secs())),
                    }
                }
                Delivery { channel: kind, error }
            });
        }
        while let Some(done) = set.join_next().await {
            if let Ok(d) = done {
                out.push(d);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn note(status: Status) -> Notification {
        Notification {
            status,
            previous: Status::Ok,
            target: "web-01".into(),
            address: None,
            rule: "Ping loss".into(),
            summary: "Ping packet loss is 100% (limit 0%)".into(),
            at: 1_793_196_187,
            note: None,
        }
    }

    #[test]
    fn utc_times() {
        assert_eq!(fmt_utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(fmt_utc(951_782_400 + 86_399), "2000-02-29 23:59:59 UTC", "leap day");
        assert_eq!(fmt_utc(1_793_196_187), "2026-10-28 14:03:07 UTC");
    }

    #[test]
    fn messages_say_what_changed_and_leave_the_address_out_unless_asked() {
        let t = note(Status::Critical).text();
        assert!(t.starts_with("\u{1F534} CRITICAL: web-01\nPing packet loss is 100% (limit 0%)\n"), "{t}");
        assert!(t.contains("Rule: Ping loss") && t.contains("Was: OK") && t.contains("Time: 2026-10-28"));
        assert!(!t.contains("Address"));
        let mut n = note(Status::Warning);
        n.address = Some("10.0.0.5".into());
        assert!(n.text().contains("Address: 10.0.0.5"));
        let mut r = note(Status::Ok);
        r.previous = Status::Critical;
        assert!(r.text().contains("RECOVERED: web-01") && r.text().contains("Was: CRITICAL"));
        assert_eq!(note(Status::Critical).subject(), "[CRITICAL] web-01: Ping loss");
        assert_eq!(r.subject(), "[OK] web-01: Ping loss recovered");
    }

    struct Fake {
        kind: ChannelKind,
        sent: AtomicUsize,
        fail_first: usize,
        seen: Mutex<Vec<String>>,
    }

    impl Fake {
        fn new(kind: ChannelKind, fail_first: usize) -> Arc<Self> {
            Arc::new(Self { kind, sent: AtomicUsize::new(0), fail_first, seen: Mutex::default() })
        }
    }

    impl Channel for Fake {
        fn kind(&self) -> ChannelKind {
            self.kind
        }
        fn send<'a>(&'a self, n: &'a Notification) -> BoxFuture<'a, Result<(), NotifyError>> {
            Box::pin(async move {
                let attempt = self.sent.fetch_add(1, Ordering::SeqCst);
                if attempt < self.fail_first {
                    return Err(NotifyError("boom".into()));
                }
                self.seen.lock().unwrap().push(n.text());
                Ok(())
            })
        }
    }

    fn quick() -> Dispatcher {
        Dispatcher::default().with_retry_after(Duration::ZERO)
    }

    #[tokio::test]
    async fn goes_to_every_channel_the_rule_allows() {
        let (tg, mail) = (Fake::new(ChannelKind::Telegram, 0), Fake::new(ChannelKind::Email, 0));
        let channels: Vec<Arc<dyn Channel>> = vec![tg.clone(), mail.clone()];
        let d = quick();
        let out = d.dispatch(&note(Status::Critical), &channels, &[]).await;
        assert_eq!(out.len(), 2);
        assert!(out.iter().all(|x| x.error.is_none()));
        let out = d.dispatch(&note(Status::Critical), &channels, &[ChannelKind::Email]).await;
        assert_eq!(out, vec![Delivery { channel: ChannelKind::Email, error: None }]);
        assert_eq!((tg.seen.lock().unwrap().len(), mail.seen.lock().unwrap().len()), (1, 2));
    }

    #[tokio::test]
    async fn one_failure_is_retried_and_a_second_is_reported_without_stopping_the_others() {
        let flaky = Fake::new(ChannelKind::Telegram, 1);
        let broken = Fake::new(ChannelKind::Bale, 99);
        let fine = Fake::new(ChannelKind::Email, 0);
        let channels: Vec<Arc<dyn Channel>> = vec![flaky.clone(), broken.clone(), fine.clone()];
        let out = quick().dispatch(&note(Status::Critical), &channels, &[]).await;
        let by = |k: ChannelKind| out.iter().find(|d| d.channel == k).unwrap().clone();
        assert_eq!(by(ChannelKind::Telegram).error, None, "second try worked");
        assert_eq!(by(ChannelKind::Bale).error.as_deref(), Some("boom"));
        assert_eq!(by(ChannelKind::Email).error, None);
        assert_eq!(broken.sent.load(Ordering::SeqCst), 2, "tried twice, no more");
    }

    #[tokio::test]
    async fn a_flood_is_held_back_and_the_next_message_says_so() {
        let tg = Fake::new(ChannelKind::Telegram, 0);
        let channels: Vec<Arc<dyn Channel>> = vec![tg.clone()];
        let d = quick();
        for _ in 0..PER_MINUTE {
            assert!(d.dispatch(&note(Status::Critical), &channels, &[]).await[0].error.is_none());
        }
        let held = d.dispatch(&note(Status::Critical), &channels, &[]).await;
        assert!(held[0].error.as_deref().unwrap().contains("held back"));
        assert_eq!(tg.seen.lock().unwrap().len(), PER_MINUTE);
        // Age the window out by hand; the next one goes and carries the count.
        d.buckets.lock().unwrap().get_mut(&ChannelKind::Telegram).unwrap().sent.clear();
        assert!(d.dispatch(&note(Status::Critical), &channels, &[]).await[0].error.is_none());
        assert!(tg.seen.lock().unwrap().last().unwrap().contains("1 earlier alert(s) were held back"));
    }

    struct Slow;
    impl Channel for Slow {
        fn kind(&self) -> ChannelKind {
            ChannelKind::Bale
        }
        fn send<'a>(&'a self, _n: &'a Notification) -> BoxFuture<'a, Result<(), NotifyError>> {
            Box::pin(async {
                tokio::time::sleep(Duration::from_secs(3600)).await;
                Ok(())
            })
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_channel_that_never_answers_times_out() {
        let channels: Vec<Arc<dyn Channel>> = vec![Arc::new(Slow)];
        let out = Dispatcher::default().dispatch(&note(Status::Critical), &channels, &[]).await;
        assert!(out[0].error.as_deref().unwrap().contains("no answer within 20 s"), "{out:?}");
    }
}
