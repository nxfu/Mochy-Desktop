//! The retry policy: upstream `InnerTube.withRetry`
//! (providers/innertube/.../InnerTube.kt:204-223):
//!
//! ```kotlin
//! if (offline) throw OfflineModeException()
//! var currentDelay = initialDelay      // 500 ms
//! while (true) {
//!     try { return block() }
//!     catch (e: IOException) {
//!         attempt++
//!         if (attempt >= maxAttempts) throw e   // 3 attempts total
//!         delay(currentDelay)
//!         currentDelay = (currentDelay * factor).toLong()  // x2
//!     }
//! }
//! ```
//!
//! IO errors only; 429 and every other HTTP error surface at once.

use std::future::Future;
use std::time::Duration;

use crate::error::NetError;
use crate::offline;

/// The exponential-backoff retry policy (3 attempts, 500 ms initial, x2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RetryPolicy {
    /// Total attempts including the first (upstream `maxAttempts = 3`).
    pub max_attempts: u32,
    /// Delay before the second attempt (upstream `initialDelay = 500L`).
    pub initial_delay: Duration,
    /// Multiplier between delays (upstream `factor = 2.0`).
    pub factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        RetryPolicy {
            max_attempts: 3,
            initial_delay: Duration::from_millis(500),
            factor: 2.0,
        }
    }
}

impl RetryPolicy {
    /// Upstream defaults (InnerTube.kt:205-208).
    pub const fn upstream() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            initial_delay: Duration::from_millis(500),
            factor: 2.0,
        }
    }

    /// The delay after attempt `attempt` (0-based): 500 ms, 1000 ms, ...
    /// There are at most `max_attempts - 1` waits.
    pub fn delay_after(&self, attempt: u32) -> Option<Duration> {
        if attempt + 1 >= self.max_attempts {
            return None; // last attempt already used
        }
        let millis = self.initial_delay.as_millis() as f64 * self.factor.powi(attempt as i32);
        Some(Duration::from_millis(millis as u64))
    }

    /// The full backoff schedule between attempts: for upstream defaults,
    /// `[500ms, 1000ms]` — two waits around three attempts.
    pub fn backoff_sequence(&self) -> Vec<Duration> {
        (0..self.max_attempts.saturating_sub(1))
            .filter_map(|attempt| self.delay_after(attempt))
            .collect()
    }

    /// What to do with an error after `attempt` (0-based) attempts.
    /// 429 and offline short-circuit immediately — matching the upstream
    /// caller contract (YTPlayerUtils.kt:280: `if (attempt.exceptionOrNull()
    /// is YouTubeRateLimitedException) return attempt`).
    pub fn decision(&self, error: &NetError, attempt: u32) -> RetryDecision {
        match error {
            NetError::YouTubeRateLimited => RetryDecision::Abort,
            NetError::Offline => RetryDecision::Abort,
            NetError::Io(_) => match self.delay_after(attempt) {
                Some(delay) => RetryDecision::RetryAfter(delay),
                None => RetryDecision::Abort,
            },
            _ => RetryDecision::Abort,
        }
    }
}

/// What the policy decided for a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryDecision {
    /// Wait this long, then try again.
    RetryAfter(Duration),
    /// Give up (error not retryable, or attempts exhausted).
    Abort,
}

/// Run `block` under the offline gate and the retry policy — the port of
/// `withRetry` for every request that goes through the shared layer.
///
/// The offline flag is checked before EVERY attempt (upstream checks at
/// entry; the desktop port re-checks because the flag can flip while a
/// backoff sleep is running).
pub async fn retry_on_io_errors<T, F, Fut>(policy: &RetryPolicy, block: F) -> Result<T, NetError>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, NetError>>,
{
    let mut attempt = 0u32;
    loop {
        if offline::is_on() {
            return Err(NetError::Offline);
        }
        match block().await {
            Ok(value) => return Ok(value),
            Err(error) => match policy.decision(&error, attempt) {
                RetryDecision::RetryAfter(delay) => {
                    tracing::debug!(attempt, ?delay, "io error; retrying");
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
                RetryDecision::Abort => return Err(error),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_sequence_matches_upstream() {
        // InnerTube.kt:205-208: maxAttempts 3, initialDelay 500ms, factor 2
        let policy = RetryPolicy::upstream();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(
            policy.backoff_sequence(),
            vec![Duration::from_millis(500), Duration::from_millis(1000)]
        );
        // delay_after: after attempt 0 -> 500ms, after 1 -> 1000ms, after 2 -> none
        assert_eq!(policy.delay_after(0), Some(Duration::from_millis(500)));
        assert_eq!(policy.delay_after(1), Some(Duration::from_millis(1000)));
        assert_eq!(policy.delay_after(2), None);
    }

    #[test]
    fn io_retries_up_to_three_attempts() {
        let policy = RetryPolicy::upstream();
        // attempts 0 and 1 retry, attempt 2 aborts
        let io = NetError::Io(Box::new(std::io::Error::other("reset")));
        assert_eq!(
            policy.decision(&io, 0),
            RetryDecision::RetryAfter(Duration::from_millis(500))
        );
        assert_eq!(
            policy.decision(&io, 1),
            RetryDecision::RetryAfter(Duration::from_millis(1000))
        );
        assert_eq!(policy.decision(&io, 2), RetryDecision::Abort);
    }

    #[test]
    fn rate_limit_short_circuits_never_retries() {
        let policy = RetryPolicy::upstream();
        for attempt in 0..5 {
            assert_eq!(
                policy.decision(&NetError::YouTubeRateLimited, attempt),
                RetryDecision::Abort
            );
        }
    }

    #[test]
    fn offline_short_circuits() {
        let policy = RetryPolicy::upstream();
        assert_eq!(policy.decision(&NetError::Offline, 0), RetryDecision::Abort);
    }

    // the guard below is HELD ACROSS AWAITS on purpose: it serializes
    // the whole test body against the tests that flip the global
    // offline flag (see `test_support`)
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn retry_execution_gives_up_after_three_io_failures() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        // retry_on_io_errors reads the process-global offline flag at
        // entry: serialize against the tests that flip it
        let _guard = crate::test_support::OFFLINE_FLAG_LOCK.lock().unwrap();
        let policy = RetryPolicy::upstream();
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);
        let result: Result<(), NetError> = retry_on_io_errors(&policy, move || {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Err(NetError::Io(Box::new(std::io::Error::other("refused"))))
            }
        })
        .await;
        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 3, "exactly three attempts");
    }

    // see retry_execution_gives_up_after_three_io_failures: the guard
    // is held across awaits on purpose
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn retry_execution_succeeds_on_second_attempt() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        let _guard = crate::test_support::OFFLINE_FLAG_LOCK.lock().unwrap();
        let policy = RetryPolicy::upstream();
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);
        let result: Result<u32, NetError> = retry_on_io_errors(&policy, move || {
            let counter = Arc::clone(&counter);
            async move {
                let seen = counter.fetch_add(1, Ordering::SeqCst) + 1;
                if seen == 1 {
                    Err(NetError::Io(Box::new(std::io::Error::other("flaky"))))
                } else {
                    Ok(seen)
                }
            }
        })
        .await;
        assert_eq!(result.unwrap(), 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    // see retry_execution_gives_up_after_three_io_failures: the guard
    // is held across awaits on purpose
    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn offline_flag_gates_before_first_attempt() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicU32, Ordering};
        let _guard = crate::test_support::OFFLINE_FLAG_LOCK.lock().unwrap();
        offline::set_offline(true);
        let policy = RetryPolicy::upstream();
        let calls = Arc::new(AtomicU32::new(0));
        let counter = Arc::clone(&calls);
        let result: Result<(), NetError> = retry_on_io_errors(&policy, move || {
            let counter = Arc::clone(&counter);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .await;
        assert!(matches!(result, Err(NetError::Offline)));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no request may go out while offline"
        );
        offline::set_offline(false);
    }
}
