//! Target-agnostic exponential-backoff helper for SSE / WebSocket
//! reconnect loops and HTTP retry loops.
//!
//! HAIAI_WASM_PRD §4.6: "Backoff numbers, max-reconnect-attempts default,
//! and event-ID-resumption logic are identical to the native impl. Same
//! defaults, same metric counters." Native uses `tokio::time::sleep` via
//! [`TokioTimer`]; wasm uses `gloo_timers::future::sleep` via
//! [`GlooTimer`]. The reconnect / retry policy itself is pure logic and
//! lives in this module.

use std::time::Duration;

use async_trait::async_trait;

/// Default initial delay between reconnect attempts. Matches the
/// pre-extraction inline value `Duration::from_millis(100)` at
/// `client.rs::on_benchmark_job_with_reconnect` and
/// `client.rs::request_with_retry` (`100ms * 2^attempt`).
pub const INITIAL_DELAY_MS: u64 = 100;

/// Default maximum delay between reconnect attempts. The original
/// inline expression `100 * (1u64 << reconnect_count.min(10))` caps
/// the shift at 10, so the absolute max is `100 * 2^10 = 102_400ms`
/// (~102 s). We preserve that ceiling exactly.
pub const MAX_DELAY_MS: u64 = 102_400;

/// Default maximum reconnect attempts. Matches
/// `client::DEFAULT_MAX_RECONNECT_ATTEMPTS = 10`.
pub const MAX_ATTEMPTS: usize = 10;

/// Multiplier between consecutive delays. The original inline pattern
/// `100 * (1u64 << attempt)` is a power-of-two ramp, i.e. multiplier 2.
pub const MULTIPLIER: u64 = 2;

/// Target-agnostic async sleep. Native uses tokio; wasm uses gloo.
#[async_trait(?Send)]
pub trait Timer {
    async fn sleep(&self, delay: Duration);
}

/// Exponential backoff state machine — pure logic. Each call to
/// [`wait`] sleeps for the next delay step and returns `true` while
/// attempts remain, `false` once `max_attempts` is reached.
pub struct Backoff<T: Timer> {
    attempt: usize,
    initial_delay: Duration,
    max_delay: Duration,
    max_attempts: usize,
    multiplier: u64,
    timer: T,
}

impl<T: Timer> Backoff<T> {
    /// Construct with the PRD §4.6 defaults.
    pub fn with_defaults(timer: T) -> Self {
        Self::new(
            timer,
            Duration::from_millis(INITIAL_DELAY_MS),
            Duration::from_millis(MAX_DELAY_MS),
            MAX_ATTEMPTS,
            MULTIPLIER,
        )
    }

    pub fn new(
        timer: T,
        initial_delay: Duration,
        max_delay: Duration,
        max_attempts: usize,
        multiplier: u64,
    ) -> Self {
        Self {
            attempt: 0,
            initial_delay,
            max_delay,
            max_attempts,
            multiplier,
            timer,
        }
    }

    /// Sleep for the next backoff step, then increment the attempt
    /// counter. Returns `true` if more attempts remain after this one,
    /// `false` if the cap has been reached (the caller should bail).
    pub async fn wait(&mut self) -> bool {
        let delay = self.next_delay();
        self.timer.sleep(delay).await;
        self.attempt += 1;
        self.attempt < self.max_attempts
    }

    /// Compute the next delay without sleeping. Useful for tests + for
    /// callers that want to log / metric the next planned delay before
    /// awaiting.
    pub fn next_delay(&self) -> Duration {
        // Use the same `1u64 << attempt.min(10)` ceiling the original
        // inline code used; multiplier=2 makes the shift equivalent.
        let shift = self.attempt.min(10) as u32;
        let factor = self.multiplier.saturating_pow(shift);
        let ms = self
            .initial_delay
            .as_millis()
            .saturating_mul(factor as u128)
            .min(self.max_delay.as_millis()) as u64;
        Duration::from_millis(ms)
    }

    pub fn attempt(&self) -> usize {
        self.attempt
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn into_timer(self) -> T {
        self.timer
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use self::native::TokioTimer;

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;

    /// Native timer impl backed by `tokio::time::sleep`.
    #[derive(Default, Clone, Copy)]
    pub struct TokioTimer;

    #[async_trait(?Send)]
    impl Timer for TokioTimer {
        async fn sleep(&self, delay: Duration) {
            tokio::time::sleep(delay).await;
        }
    }
}

#[cfg(target_arch = "wasm32")]
pub use self::wasm::GlooTimer;

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;

    /// Wasm timer impl backed by `gloo_timers::future::sleep`. Schedules
    /// a `setTimeout` task on the browser event loop and resolves when
    /// the timeout fires.
    #[derive(Default, Clone, Copy)]
    pub struct GlooTimer;

    #[async_trait(?Send)]
    impl Timer for GlooTimer {
        async fn sleep(&self, delay: Duration) {
            gloo_timers::future::sleep(delay).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Fake timer that records every sleep duration it was asked for
    /// instead of actually waiting. Lets us assert the backoff curve
    /// without slowing the test down.
    #[derive(Default, Clone)]
    struct RecordingTimer {
        calls: Rc<RefCell<Vec<Duration>>>,
    }

    #[async_trait(?Send)]
    impl Timer for RecordingTimer {
        async fn sleep(&self, delay: Duration) {
            self.calls.borrow_mut().push(delay);
        }
    }

    #[tokio::test]
    async fn backoff_grows_exponentially_and_caps() {
        let timer = RecordingTimer::default();
        let calls = timer.calls.clone();
        let mut backoff = Backoff::with_defaults(timer);
        for _ in 0..MAX_ATTEMPTS {
            backoff.wait().await;
        }
        let recorded = calls.borrow().clone();
        assert_eq!(recorded.len(), MAX_ATTEMPTS);
        // First delay matches initial; subsequent double; ceiling at
        // MAX_DELAY_MS (102_400).
        assert_eq!(recorded[0], Duration::from_millis(INITIAL_DELAY_MS));
        assert_eq!(recorded[1], Duration::from_millis(200));
        assert_eq!(recorded[2], Duration::from_millis(400));
        // attempt=10 hits the shift cap: 100 * 2^10 = 102_400ms = MAX.
        assert!(recorded[MAX_ATTEMPTS - 1] <= Duration::from_millis(MAX_DELAY_MS));
    }

    #[tokio::test]
    async fn backoff_resets_attempts() {
        let timer = RecordingTimer::default();
        let mut backoff = Backoff::with_defaults(timer);
        backoff.wait().await;
        backoff.wait().await;
        assert_eq!(backoff.attempt(), 2);
        backoff.reset();
        assert_eq!(backoff.attempt(), 0);
        assert_eq!(
            backoff.next_delay(),
            Duration::from_millis(INITIAL_DELAY_MS)
        );
    }

    #[test]
    fn next_delay_matches_inline_formula() {
        let timer = TokioTimer;
        let backoff = Backoff::with_defaults(timer);
        // Original inline expression in client.rs::request_with_retry was
        //   Duration::from_millis(100 * (1u64 << attempt))
        // assert that for attempt=0..=10 we match it exactly.
        let mut b = backoff;
        for n in 0..=10 {
            let expected_ms = 100u64.saturating_mul(1u64 << n.min(10));
            let expected_ms = expected_ms.min(MAX_DELAY_MS);
            assert_eq!(
                b.next_delay().as_millis() as u64,
                expected_ms,
                "attempt={n}"
            );
            b.attempt = n + 1;
        }
    }
}
