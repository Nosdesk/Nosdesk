//! Circuit breaker around the SMTP transport.
//!
//! Without it, an SMTP outage with N pending messages is N attempts ×
//! jittered backoff = days of grinding through retries even after SMTP
//! recovers. The breaker trips after a threshold of consecutive
//! failures within a window; while open, the worker treats SMTP as
//! down and skips its tick rather than burning attempt counters.
//!
//! Implementation is hand-rolled rather than pulling in `failsafe-rs`:
//! the state machine is small (closed / open / half-open), we already
//! have `tokio::sync::Mutex` everywhere, and one less crate
//! dependency. `failsafe-rs` was the alternative considered in the
//! research; the crate is fine but the abstraction wasn't pulling its
//! weight for our tiny surface.
//!
//! Behaviour:
//! - `closed` (default): every call goes through; track consecutive
//!   failures.
//! - On the Nth consecutive failure within a window → trip to `open`.
//! - `open`: every call returns `Err(CircuitOpen)` immediately; the
//!   worker should `release_claim` (don't burn the attempt) and
//!   sleep one tick.
//! - After `cool_down`, transition to `half_open`: the next call
//!   passes through; if it succeeds, back to `closed`; if it fails,
//!   back to `open` and reset the cool-down clock.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const FAILURE_THRESHOLD: u32 = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(60);
const COOL_DOWN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    Closed,
    Open,
    HalfOpen,
}

#[derive(Debug)]
struct Inner {
    state: BreakerState,
    /// Failure timestamps within the current window.
    /// Bounded by `FAILURE_THRESHOLD` so the Vec is tiny.
    failures: Vec<Instant>,
    /// When the breaker tripped open; `None` while closed.
    opened_at: Option<Instant>,
}

#[derive(Debug)]
pub struct CircuitBreaker {
    inner: Mutex<Inner>,
}

impl CircuitBreaker {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                state: BreakerState::Closed,
                failures: Vec::new(),
                opened_at: None,
            }),
        }
    }

    /// Return the current state, taking the cool-down clock into
    /// account. Acquires the lock; cheap enough for per-call use.
    pub async fn state(&self) -> BreakerState {
        let mut inner = self.inner.lock().await;
        if inner.state == BreakerState::Open {
            if let Some(opened) = inner.opened_at {
                if opened.elapsed() >= COOL_DOWN {
                    inner.state = BreakerState::HalfOpen;
                }
            }
        }
        inner.state
    }

    /// Whether the next call should be allowed through. `true` for
    /// closed and half-open; `false` for open (still cooling down).
    pub async fn allow(&self) -> bool {
        !matches!(self.state().await, BreakerState::Open)
    }

    /// Record a successful call. Closes the breaker (or keeps it
    /// closed) and clears accumulated failures.
    pub async fn record_success(&self) {
        let mut inner = self.inner.lock().await;
        inner.state = BreakerState::Closed;
        inner.failures.clear();
        inner.opened_at = None;
    }

    /// Record a failed call. Trips to `open` if N consecutive failures
    /// land within `FAILURE_WINDOW`.
    pub async fn record_failure(&self) {
        let mut inner = self.inner.lock().await;
        let now = Instant::now();
        // Drop failures outside the rolling window before deciding.
        inner
            .failures
            .retain(|t| now.duration_since(*t) < FAILURE_WINDOW);
        inner.failures.push(now);
        if inner.failures.len() as u32 >= FAILURE_THRESHOLD {
            inner.state = BreakerState::Open;
            inner.opened_at = Some(now);
            inner.failures.clear();
        } else if inner.state == BreakerState::HalfOpen {
            // Half-open probe failed: re-open, restart cool-down.
            inner.state = BreakerState::Open;
            inner.opened_at = Some(now);
        }
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-transport circuit breakers, keyed by relay host.
///
/// A single shared breaker would let one tenant's broken SMTP relay
/// (`smtp_relay` mode, its own host) trip the breaker for *every* send,
/// including the platform/auth mail that goes through a different, healthy
/// instance relay. Keying by relay host isolates fate: a broken relay pauses
/// only its own host's sends. Workspaces that share a relay (the instance relay
/// carries platform mail, verified-domain mail, and the fallback) share one
/// breaker, which is correct — if that relay is down, they are all affected.
///
/// Breakers are created lazily on first use and live for the process; the host
/// set is tiny (one per configured relay).
#[derive(Debug, Default)]
pub struct CircuitBreakerRegistry {
    breakers: Mutex<HashMap<String, Arc<CircuitBreaker>>>,
}

impl CircuitBreakerRegistry {
    pub fn new() -> Self {
        Self {
            breakers: Mutex::new(HashMap::new()),
        }
    }

    /// The breaker for `relay_host`, creating it on first use.
    pub async fn for_host(&self, relay_host: &str) -> Arc<CircuitBreaker> {
        let mut map = self.breakers.lock().await;
        map.entry(relay_host.to_string())
            .or_insert_with(|| Arc::new(CircuitBreaker::new()))
            .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn starts_closed_and_allows_calls() {
        let cb = CircuitBreaker::new();
        assert_eq!(cb.state().await, BreakerState::Closed);
        assert!(cb.allow().await);
    }

    #[tokio::test]
    async fn trips_open_after_threshold_failures() {
        let cb = CircuitBreaker::new();
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure().await;
        }
        assert_eq!(cb.state().await, BreakerState::Open);
        assert!(!cb.allow().await);
    }

    #[tokio::test]
    async fn success_resets_failure_window() {
        let cb = CircuitBreaker::new();
        for _ in 0..FAILURE_THRESHOLD - 1 {
            cb.record_failure().await;
        }
        cb.record_success().await;
        assert_eq!(cb.state().await, BreakerState::Closed);
        // Now we'd need `THRESHOLD` more failures to trip.
        for _ in 0..FAILURE_THRESHOLD - 1 {
            cb.record_failure().await;
        }
        assert_eq!(cb.state().await, BreakerState::Closed);
    }

    #[tokio::test]
    async fn registry_isolates_breakers_by_host() {
        let reg = CircuitBreakerRegistry::new();
        let a = reg.for_host("relay-a").await;
        for _ in 0..FAILURE_THRESHOLD {
            a.record_failure().await;
        }
        // relay-a is open; relay-b is a different transport and untouched.
        assert!(!reg.for_host("relay-a").await.allow().await);
        assert!(reg.for_host("relay-b").await.allow().await);
        // The same host returns the same breaker, so state persists.
        assert_eq!(
            reg.for_host("relay-a").await.state().await,
            BreakerState::Open
        );
    }
}
