//! A time limit that stands still while a person is being asked something.
//!
//! The SSH handshake is limited to a few seconds so a server that accepts the connection and then says nothing doesn't
//! hold a tab forever. But the handshake is also where an unknown host key is shown for the person to trust or refuse,
//! and a person can take longer than that. While `paused` is set the clock does not run; the limit counts only the time
//! spent waiting on the server.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

const TICK: Duration = Duration::from_millis(250);

/// `Some(output)` if `fut` finished, `None` if it used up `limit` of un-paused time first.
pub async fn within<F: Future>(limit: Duration, paused: &AtomicBool, fut: F) -> Option<F::Output> {
    within_tick(limit, paused, fut, TICK).await
}

async fn within_tick<F: Future>(limit: Duration, paused: &AtomicBool, fut: F, tick: Duration) -> Option<F::Output> {
    tokio::pin!(fut);
    let mut left = limit;
    loop {
        tokio::select! {
            out = &mut fut => return Some(out),
            _ = tokio::time::sleep(tick) => {
                if !paused.load(Ordering::Relaxed) {
                    left = left.saturating_sub(tick);
                    if left.is_zero() {
                        return None;
                    }
                }
            }
        }
    }
}

/// Sets a flag for as long as it lives, even if the code holding it is dropped mid-way.
pub struct Pause<'a>(&'a AtomicBool);

impl<'a> Pause<'a> {
    pub fn new(flag: &'a AtomicBool) -> Self {
        flag.store(true, Ordering::Relaxed);
        Self(flag)
    }
}

impl Drop for Pause<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    const T: Duration = Duration::from_millis(20);
    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[tokio::test]
    async fn finishing_in_time_returns_the_output() {
        let p = AtomicBool::new(false);
        assert_eq!(within_tick(ms(400), &p, async { 7 }, T).await, Some(7));
    }

    #[tokio::test]
    async fn a_silent_server_times_out() {
        let p = AtomicBool::new(false);
        let start = Instant::now();
        let r = within_tick(ms(100), &p, std::future::pending::<()>(), T).await;
        assert!(r.is_none());
        assert!(start.elapsed() < ms(600), "{:?}", start.elapsed());
    }

    #[tokio::test]
    async fn time_spent_asking_a_person_does_not_count() {
        let p = Arc::new(AtomicBool::new(false));
        let p2 = Arc::clone(&p);
        // The "person" takes 500 ms, five times the limit, then the server answers.
        let slow = async move {
            {
                let _ask = Pause::new(&p2);
                tokio::time::sleep(ms(500)).await;
            }
            tokio::time::sleep(ms(30)).await;
            "trusted"
        };
        assert_eq!(within_tick(ms(100), &p, slow, T).await, Some("trusted"));
        assert!(!p.load(Ordering::Relaxed), "the pause ends with the question");
    }

    #[tokio::test]
    async fn the_clock_resumes_after_the_question() {
        let p = Arc::new(AtomicBool::new(false));
        let p2 = Arc::clone(&p);
        let then_silence = async move {
            {
                let _ask = Pause::new(&p2);
                tokio::time::sleep(ms(200)).await;
            }
            std::future::pending::<()>().await
        };
        let start = Instant::now();
        assert!(within_tick(ms(100), &p, then_silence, T).await.is_none());
        assert!(start.elapsed() >= ms(280), "ran the limit after the pause: {:?}", start.elapsed());
    }

    #[tokio::test]
    async fn a_dropped_question_unpauses() {
        let p = AtomicBool::new(false);
        {
            let _ask = Pause::new(&p);
            assert!(p.load(Ordering::Relaxed));
        }
        assert!(!p.load(Ordering::Relaxed));
    }
}
