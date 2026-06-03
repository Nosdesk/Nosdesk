//! Per-item retry executor.
//!
//! Wraps an async closure that produces a single item with the
//! retry policy derived from [`MsGraphSyncError::classify`]:
//!
//!   * `Transient` — retry with exponential backoff up to
//!     [`RetryConfig::max_attempts`]; the last attempt's error is
//!     returned to the caller, carrying the attempt count.
//!   * `Permanent` / `Conflict` / `Auth` — give up immediately.
//!     There's nothing useful to do by trying again, and a
//!     conflict in particular needs the racing state to settle
//!     between runs (which won't happen if we keep hammering it).
//!
//! Cancellation: the executor checks the supplied
//! [`tokio_util::sync::CancellationToken`] between attempts and
//! short-circuits with `MsGraphSyncError::Cancelled` rather than
//! starting another backoff. The token is the same shape the rest
//! of the codebase already uses for graceful-shutdown plumbing, so
//! the executor composes with the scheduler's shutdown signal
//! without a bespoke channel.
//!
//! The function returns the typed error directly (no anyhow); the
//! pipeline above is what wraps the attempt count into an
//! [`ItemFailure`].

use std::future::Future;
use std::time::Duration;

use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::error::{Classification, MsGraphSyncError};

/// Retry policy. Defaults are tuned for MS Graph: rate-limit retries
/// rarely need more than ~3 tries with low-second-range backoff to
/// clear; longer waits push us past the scheduler's own tick.
#[derive(Debug, Clone, Copy)]
pub struct RetryConfig {
    /// Total attempts including the first try. `max_attempts = 1`
    /// disables retry entirely.
    pub max_attempts: u32,
    /// Initial backoff applied between attempt 1 and attempt 2.
    pub initial_backoff: Duration,
    /// Multiplier applied to the previous backoff to produce the
    /// next one. 2.0 gives doubling: 250ms -> 500ms -> 1s -> ...
    pub backoff_multiplier: f64,
    /// Upper bound on a single backoff. Without this, exponential
    /// growth past 5-6 retries pushes us toward minute-range waits
    /// — wasted time vs. waiting for the next delta tick instead.
    pub max_backoff: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(250),
            backoff_multiplier: 2.0,
            max_backoff: Duration::from_secs(5),
        }
    }
}

impl RetryConfig {
    /// Disable retry. Useful for tests + for permanent-only call
    /// sites (the executor's classify check would skip them anyway,
    /// but a max_attempts=1 config makes intent explicit).
    pub fn no_retry() -> Self {
        Self {
            max_attempts: 1,
            initial_backoff: Duration::ZERO,
            backoff_multiplier: 1.0,
            max_backoff: Duration::ZERO,
        }
    }

    /// Backoff to apply *before* the given attempt number (1-based).
    /// Returns ZERO for attempt 1 since that's the initial try.
    fn backoff_for(&self, attempt: u32) -> Duration {
        if attempt <= 1 {
            return Duration::ZERO;
        }
        let exponent = (attempt - 2) as i32;
        let scale = self.backoff_multiplier.powi(exponent);
        let nanos = (self.initial_backoff.as_nanos() as f64 * scale).min(u64::MAX as f64) as u64;
        Duration::from_nanos(nanos).min(self.max_backoff)
    }
}

/// Execute `f` with retry on transient failures. Returns the value
/// of the first successful attempt, or the last error encountered
/// (with `attempts` reflecting how many tries it took).
///
/// `item_label` is the bounded enum tag (e.g. "users", "devices")
/// used for the structured warn line emitted on retry. The full
/// external id of the item belongs on the caller side; this layer
/// stays generic over the unit of work.
pub async fn with_retry<F, Fut, T>(
    item_label: &'static str,
    config: RetryConfig,
    cancel: &CancellationToken,
    mut f: F,
) -> Result<T, RetryFailure>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, MsGraphSyncError>>,
{
    let mut attempt: u32 = 1;
    loop {
        if cancel.is_cancelled() {
            return Err(RetryFailure {
                attempts: attempt,
                error: MsGraphSyncError::Cancelled,
            });
        }

        let backoff = config.backoff_for(attempt);
        if !backoff.is_zero() {
            tokio::select! {
                _ = tokio::time::sleep(backoff) => {}
                _ = cancel.cancelled() => {
                    return Err(RetryFailure {
                        attempts: attempt,
                        error: MsGraphSyncError::Cancelled,
                    });
                }
            }
        }

        match f().await {
            Ok(value) => return Ok(value),
            Err(err) => {
                let cls = err.classify();
                // Transient failures retry up to the cap; everything
                // else (Permanent / Conflict / Auth) gives up
                // immediately with `attempts` reflecting how many
                // tries we actually made.
                if cls == Classification::Transient && attempt < config.max_attempts {
                    warn!(
                        entity = item_label,
                        attempt,
                        max_attempts = config.max_attempts,
                        error_kind = err.kind_str(),
                        classification = cls.as_str(),
                        "msgraph item failed, retrying"
                    );
                    attempt += 1;
                    continue;
                }
                return Err(RetryFailure {
                    attempts: attempt,
                    error: err,
                });
            }
        }
    }
}

/// What the executor returns on failure: the typed error plus how
/// many attempts it took. The pipeline wraps this into an
/// `ItemFailure` along with the entity + external_id context.
#[derive(Debug)]
pub struct RetryFailure {
    pub attempts: u32,
    pub error: MsGraphSyncError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    #[test]
    fn backoff_doubles_then_caps() {
        let cfg = RetryConfig {
            max_attempts: 6,
            initial_backoff: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            max_backoff: Duration::from_millis(500),
        };
        // Attempt 1 = no backoff (it's the first try).
        assert_eq!(cfg.backoff_for(1), Duration::ZERO);
        assert_eq!(cfg.backoff_for(2), Duration::from_millis(100));
        assert_eq!(cfg.backoff_for(3), Duration::from_millis(200));
        assert_eq!(cfg.backoff_for(4), Duration::from_millis(400));
        // Capped at max_backoff from here on.
        assert_eq!(cfg.backoff_for(5), Duration::from_millis(500));
        assert_eq!(cfg.backoff_for(6), Duration::from_millis(500));
    }

    #[tokio::test]
    async fn transient_retries_then_succeeds() {
        let cfg = RetryConfig {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(1),
            backoff_multiplier: 1.0,
            max_backoff: Duration::from_millis(1),
        };
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_in = attempts.clone();
        let result = with_retry::<_, _, ()>("test", cfg, &CancellationToken::new(), move || {
            let attempts = attempts_in.clone();
            async move {
                let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 {
                    Err(MsGraphSyncError::HttpTransient {
                        status: 429,
                        source: Box::new(std::io::Error::new(std::io::ErrorKind::Other, "rate")),
                    })
                } else {
                    Ok(())
                }
            }
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn permanent_gives_up_after_one_try() {
        let attempts = Arc::new(AtomicU32::new(0));
        let attempts_in = attempts.clone();
        let result = with_retry::<_, _, ()>(
            "test",
            RetryConfig::default(),
            &CancellationToken::new(),
            move || {
                let attempts = attempts_in.clone();
                async move {
                    attempts.fetch_add(1, Ordering::SeqCst);
                    Err(MsGraphSyncError::HttpPermanent {
                        status: 404,
                        source: Box::new(std::io::Error::new(std::io::ErrorKind::Other, "gone")),
                    })
                }
            },
        )
        .await;
        assert!(result.is_err());
        let failure = result.unwrap_err();
        assert_eq!(failure.attempts, 1);
        assert_eq!(attempts.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn cancellation_short_circuits() {
        let token = CancellationToken::new();
        token.cancel();
        let result = with_retry::<_, _, ()>("test", RetryConfig::default(), &token, || async {
            panic!("closure should never run when token is pre-cancelled");
        })
        .await;
        let failure = result.unwrap_err();
        assert!(matches!(failure.error, MsGraphSyncError::Cancelled));
    }
}
