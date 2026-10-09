//! Slowing down repeated wrong master passwords at the unlock command.
//!
//! The key derivation already makes each guess cost real time, but a program that can call the unlock command in a loop
//! (a script in the window, say) should not get to try forever. After a few free tries each wrong password makes the next
//! attempt wait twice as long, up to five minutes; a right one clears it. This slows guessing through the app only: it does
//! nothing about someone who has a copy of the vault folder, which the key derivation and a strong password are for.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Wrong passwords allowed before waiting starts.
pub const FREE_TRIES: u32 = 4;
/// The longest wait.
pub const MAX_WAIT: Duration = Duration::from_secs(300);

/// How long to wait after `fails` wrong passwords in a row.
pub fn wait_after(fails: u32) -> Duration {
    if fails <= FREE_TRIES {
        return Duration::ZERO;
    }
    let doublings = (fails - FREE_TRIES).min(20);
    Duration::from_secs(1u64 << doublings).min(MAX_WAIT)
}

#[derive(Default)]
pub struct UnlockGuard(Mutex<State>);

#[derive(Default)]
struct State {
    fails: u32,
    until: Option<Instant>,
}

impl UnlockGuard {
    /// Ok to try now, or how much longer to wait.
    pub fn check(&self, now: Instant) -> Result<(), Duration> {
        let s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        match s.until {
            Some(t) if t > now => Err(t - now),
            _ => Ok(()),
        }
    }

    /// A wrong password was given.
    pub fn failed(&self, now: Instant) {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        s.fails = s.fails.saturating_add(1);
        let wait = wait_after(s.fails);
        s.until = (!wait.is_zero()).then_some(now + wait);
    }

    /// The right password was given.
    pub fn succeeded(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = State::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: u64) -> Duration {
        Duration::from_secs(n)
    }

    #[test]
    fn the_wait_doubles_after_a_few_free_tries_and_stops_at_five_minutes() {
        let w: Vec<u64> = (0..=14).map(|n| wait_after(n).as_secs()).collect();
        assert_eq!(w, [0, 0, 0, 0, 0, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300]);
        assert_eq!(wait_after(u32::MAX), MAX_WAIT);
    }

    #[test]
    fn a_few_wrong_passwords_cost_nothing_then_each_one_waits_longer() {
        let g = UnlockGuard::default();
        let t0 = Instant::now();
        for _ in 0..FREE_TRIES {
            assert!(g.check(t0).is_ok());
            g.failed(t0);
        }
        assert!(g.check(t0).is_ok(), "the free tries are used up but nothing waits yet");
        g.failed(t0);
        assert_eq!(g.check(t0), Err(secs(2)));
        assert!(g.check(t0 + secs(1)).is_err());
        assert!(g.check(t0 + secs(2)).is_ok());
        g.failed(t0 + secs(2));
        assert_eq!(g.check(t0 + secs(2)), Err(secs(4)));
    }

    #[test]
    fn the_right_password_clears_it() {
        let g = UnlockGuard::default();
        let t0 = Instant::now();
        for _ in 0..8 {
            g.failed(t0);
        }
        assert!(g.check(t0).is_err());
        g.succeeded();
        assert!(g.check(t0).is_ok());
        // and the free tries start over
        for _ in 0..FREE_TRIES {
            g.failed(t0);
        }
        assert!(g.check(t0).is_ok());
    }
}
