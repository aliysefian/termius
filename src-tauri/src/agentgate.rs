//! Session-level control of agent forwarding.
//!
//! A host that forwards the agent (`ssh -A`) hands every program on that host, and anyone who can act as the same
//! user or as root there, the ability to ask this computer's agent for signatures for as long as the connection
//! lives. An [`AgentGate`] puts that under the person's control while the session runs: forwarding can be switched
//! off (which also cuts the agent connections already open), or on for a limited time after which it turns itself
//! off. A [`SignObserver`] sees every signature the remote host asks for, so it can be shown, and refused.
//!
//! The gate cannot turn forwarding *on* for a session that never asked for it: the server only creates its agent
//! socket if the shell channel requested forwarding. So a gate exists only for sessions that did ask, and starts
//! off or on as the host says.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use russh::keys::ssh_key::{HashAlg, PublicKey};
use serde::Serialize;
use tokio::sync::watch;
use tokio::time::Instant;

use crate::agent::{Backend, BoxFuture, Offered};

/// What the person asked of the gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Off,
    /// Until switched off.
    On,
    /// On, then off by itself after this long.
    Timed(Duration),
}

/// The gate's state, for showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ForwardState {
    Off,
    On,
    Timed { expires_in_secs: u64 },
}

/// A signature the remote host asked this computer's agent for.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SignRequest {
    /// The host that asked (`host` or `host:port`).
    pub origin: String,
    /// SHA-256 fingerprint of the key it asked to sign with.
    pub fingerprint: String,
    /// Unix seconds.
    pub at: u64,
}

/// Told of each signature request before it is answered. Returning false refuses it.
pub trait SignObserver: Send + Sync + 'static {
    fn signing(&self, request: SignRequest) -> BoxFuture<bool>;
}

#[derive(Debug, Clone, Copy)]
enum Mode {
    Off,
    On,
    Until(Instant),
}

struct Inner {
    mode: Mode,
    /// Bumped on every change, so a timer from an earlier choice can tell it has been overtaken.
    generation: u64,
}

pub struct AgentGate {
    inner: Mutex<Inner>,
    /// Counts the times open agent connections must be cut; each connection watches it.
    cut: watch::Sender<u64>,
    observer: Option<Arc<dyn SignObserver>>,
}

impl AgentGate {
    pub fn new(initially_on: bool, observer: Option<Arc<dyn SignObserver>>) -> Arc<Self> {
        Arc::new(AgentGate {
            inner: Mutex::new(Inner { mode: if initially_on { Mode::On } else { Mode::Off }, generation: 0 }),
            cut: watch::channel(0).0,
            observer,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Whether a new agent connection from the remote host may be accepted now.
    pub fn allows(&self) -> bool {
        match self.lock().mode {
            Mode::Off => false,
            Mode::On => true,
            Mode::Until(t) => Instant::now() < t,
        }
    }

    pub fn status(&self) -> ForwardState {
        match self.lock().mode {
            Mode::On => ForwardState::On,
            Mode::Until(t) => match t.checked_duration_since(Instant::now()) {
                Some(left) if !left.is_zero() => ForwardState::Timed { expires_in_secs: left.as_secs().max(1) },
                _ => ForwardState::Off,
            },
            Mode::Off => ForwardState::Off,
        }
    }

    /// Change the gate. Switching off, or letting a timed grant run out, cuts the agent connections already open.
    /// A timed grant needs a running tokio runtime for its timer.
    pub fn set(self: &Arc<Self>, request: Request) {
        let generation = {
            let mut g = self.lock();
            g.generation += 1;
            g.mode = match request {
                Request::Off => Mode::Off,
                Request::On => Mode::On,
                Request::Timed(d) => Mode::Until(Instant::now() + d),
            };
            g.generation
        };
        match request {
            Request::Off => self.cut_open(),
            Request::On => {}
            Request::Timed(d) => {
                let gate = Arc::clone(self);
                // spawn-ok: only called from async commands, on the tokio runtime
                tokio::spawn(async move {
                    tokio::time::sleep(d).await;
                    let expired = {
                        let mut g = gate.lock();
                        if g.generation == generation {
                            g.mode = Mode::Off;
                            true
                        } else {
                            false
                        }
                    };
                    if expired {
                        gate.cut_open();
                    }
                });
            }
        }
    }

    fn cut_open(&self) {
        self.cut.send_modify(|n| *n += 1);
    }

    /// A receiver that changes when the connections open at this moment must be dropped.
    pub fn watch_cut(&self) -> watch::Receiver<u64> {
        self.cut.subscribe()
    }

    pub fn observer(&self) -> Option<Arc<dyn SignObserver>> {
        self.observer.clone()
    }
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The fingerprint a [`SignRequest`] carries.
pub fn fingerprint_of(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// A backend that tells an observer about each signature before the inner backend decides it.
pub struct Observed {
    pub inner: Arc<dyn Backend>,
    pub observer: Arc<dyn SignObserver>,
}

impl Backend for Observed {
    fn identities(&self) -> Vec<Offered> {
        self.inner.identities()
    }

    fn private(&self, public: &PublicKey) -> Option<russh::keys::PrivateKey> {
        self.inner.private(public)
    }

    fn approve(&self, public: &PublicKey, comment: &str, origin: Option<&str>) -> BoxFuture<bool> {
        let request = SignRequest { origin: origin.unwrap_or("").to_string(), fingerprint: fingerprint_of(public), at: now_secs() };
        let (inner, observer) = (Arc::clone(&self.inner), Arc::clone(&self.observer));
        let (public, comment, origin) = (public.clone(), comment.to_string(), origin.map(str::to_string));
        Box::pin(async move {
            if !observer.signing(request).await {
                return false;
            }
            inner.approve(&public, &comment, origin.as_deref()).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Count(Mutex<Vec<SignRequest>>, bool);
    impl SignObserver for Count {
        fn signing(&self, r: SignRequest) -> BoxFuture<bool> {
            self.0.lock().unwrap().push(r);
            let allow = self.1;
            Box::pin(async move { allow })
        }
    }

    #[tokio::test]
    async fn the_gate_follows_what_was_asked() {
        let g = AgentGate::new(true, None);
        assert!(g.allows());
        assert_eq!(g.status(), ForwardState::On);
        g.set(Request::Off);
        assert!(!g.allows());
        assert_eq!(g.status(), ForwardState::Off);
        g.set(Request::On);
        assert!(g.allows());
        let paused = AgentGate::new(false, None);
        assert!(!paused.allows());
    }

    #[tokio::test(start_paused = true)]
    async fn a_timed_grant_turns_itself_off_and_cuts_open_connections() {
        let g = AgentGate::new(false, None);
        let mut cut = g.watch_cut();
        g.set(Request::Timed(Duration::from_secs(300)));
        assert!(g.allows());
        assert!(matches!(g.status(), ForwardState::Timed { expires_in_secs } if (299..=300).contains(&expires_in_secs)));
        tokio::time::sleep(Duration::from_secs(299)).await;
        assert!(g.allows(), "still inside the five minutes");
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(!g.allows());
        assert_eq!(g.status(), ForwardState::Off);
        assert!(cut.has_changed().unwrap(), "open connections were told to go");
        cut.changed().await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn an_old_timer_does_not_switch_off_a_newer_choice() {
        let g = AgentGate::new(false, None);
        g.set(Request::Timed(Duration::from_secs(60)));
        g.set(Request::On);
        tokio::time::sleep(Duration::from_secs(120)).await;
        assert!(g.allows(), "switching to 'on' replaced the timer");
        g.set(Request::Timed(Duration::from_secs(10)));
        g.set(Request::Timed(Duration::from_secs(100)));
        tokio::time::sleep(Duration::from_secs(50)).await;
        assert!(g.allows(), "the first ten seconds' timer was overtaken");
    }

    #[tokio::test]
    async fn switching_off_cuts_connections_but_switching_on_does_not() {
        let g = AgentGate::new(true, None);
        let rx = g.watch_cut();
        g.set(Request::On);
        assert!(!rx.has_changed().unwrap());
        g.set(Request::Off);
        assert!(rx.has_changed().unwrap());
    }

    struct One(russh::keys::PrivateKey, Mutex<u32>);
    impl Backend for One {
        fn identities(&self) -> Vec<Offered> {
            vec![Offered { public: self.0.public_key().clone(), comment: "k".into() }]
        }
        fn private(&self, _: &PublicKey) -> Option<russh::keys::PrivateKey> {
            Some(self.0.clone())
        }
        fn approve(&self, _: &PublicKey, _: &str, _: Option<&str>) -> BoxFuture<bool> {
            *self.1.lock().unwrap() += 1;
            Box::pin(async { true })
        }
    }

    fn a_key() -> russh::keys::PrivateKey {
        let m = crate::keys::generate(crate::keys::KeyAlgorithm::Ed25519, "t", None).unwrap();
        russh::keys::PrivateKey::from_openssh(m.private_key.unwrap().as_str()).unwrap()
    }

    #[tokio::test]
    async fn an_observed_backend_reports_first_and_a_refusal_never_reaches_the_inner_backend() {
        let key = a_key();
        let inner = Arc::new(One(key.clone(), Mutex::new(0)));
        let allow = Arc::new(Count(Mutex::new(vec![]), true));
        let b = Observed { inner: inner.clone(), observer: allow.clone() };
        assert!(b.approve(key.public_key(), "k", Some("bastion:2222")).await);
        let seen = allow.0.lock().unwrap().clone();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].origin, "bastion:2222");
        assert_eq!(seen[0].fingerprint, fingerprint_of(key.public_key()));
        assert_eq!(*inner.1.lock().unwrap(), 1);

        let refuse = Arc::new(Count(Mutex::new(vec![]), false));
        let b = Observed { inner: inner.clone(), observer: refuse };
        assert!(!b.approve(key.public_key(), "k", None).await);
        assert_eq!(*inner.1.lock().unwrap(), 1, "the vault's own approval was not even asked");
    }
}
