//! The backend's own clock for locking an idle vault.
//!
//! The window locks the vault after a while without input, but that timer lives in the webview: if the webview is frozen,
//! throttled or stalled, it never fires. So the window also tells the backend, now and then, that someone is there, and
//! the backend locks by itself when those reports stop. Terminal output is not activity, as in the window's own timer.

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The longest idle time a report may ask for (one day), whatever the window says.
const MAX_MINUTES: u32 = 24 * 60;

#[derive(Default)]
pub struct IdleClock(Mutex<State>);

#[derive(Default)]
struct State {
    last: Option<Instant>,
    minutes: u32,
}

impl IdleClock {
    /// Someone is there. `minutes` is the idle time the person chose; 0 turns the backend lock off.
    pub fn touch(&self, minutes: u32) {
        self.touch_at(Instant::now(), minutes);
    }

    fn touch_at(&self, now: Instant, minutes: u32) {
        let mut s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        s.last = Some(now);
        s.minutes = minutes.min(MAX_MINUTES);
    }

    /// Whether the vault has been idle for longer than chosen. Never before the first report.
    pub fn due(&self, now: Instant) -> bool {
        let s = self.0.lock().unwrap_or_else(|p| p.into_inner());
        match s.last {
            Some(last) if s.minutes > 0 => now.saturating_duration_since(last) >= Duration::from_secs(u64::from(s.minutes) * 60),
            _ => false,
        }
    }

    /// Forget the reports, as after a lock: the next unlock starts counting again from its first report.
    pub fn reset(&self) {
        *self.0.lock().unwrap_or_else(|p| p.into_inner()) = State::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_only_after_the_chosen_idle_time() {
        let c = IdleClock::default();
        let t0 = Instant::now();
        assert!(!c.due(t0 + Duration::from_secs(86_400)), "nothing is due before the first report");
        c.touch_at(t0, 5);
        assert!(!c.due(t0 + Duration::from_secs(299)));
        assert!(c.due(t0 + Duration::from_secs(300)));
        c.touch_at(t0 + Duration::from_secs(290), 5);
        assert!(!c.due(t0 + Duration::from_secs(300)), "activity restarts the count");
        assert!(c.due(t0 + Duration::from_secs(590)));
    }

    #[test]
    fn zero_minutes_means_off_and_reset_forgets() {
        let c = IdleClock::default();
        let t0 = Instant::now();
        c.touch_at(t0, 0);
        assert!(!c.due(t0 + Duration::from_secs(10 * 86_400)));
        c.touch_at(t0, 1);
        assert!(c.due(t0 + Duration::from_secs(60)));
        c.reset();
        assert!(!c.due(t0 + Duration::from_secs(86_400)));
    }

    #[test]
    fn the_idle_time_is_capped_and_a_clock_going_backwards_is_harmless() {
        let c = IdleClock::default();
        let t0 = Instant::now();
        c.touch_at(t0 + Duration::from_secs(100), u32::MAX);
        assert!(!c.due(t0), "an earlier instant than the last report is not idle");
        assert!(!c.due(t0 + Duration::from_secs(100 + 86_399)));
        assert!(c.due(t0 + Duration::from_secs(100 + 86_400)), "capped at a day");
    }
}
