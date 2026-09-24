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

/// Which transport a breaker guards.
///
/// The instance relay (`workspace_id: None`) carries platform mail, the
/// fallback and verified-domain mail, so every workspace shares its breaker:
/// if it is down, they are all affected. A workspace's own SMTP server is
/// keyed by workspace as well as host, so one workspace's wrong password on
/// `smtp.gmail.com` never pauses another workspace that uses Gmail too, nor
/// the instance relay if it happens to be the same host.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BreakerKey {
    pub workspace_id: Option<i32>,
    pub host: String,
}

impl BreakerKey {
    pub fn instance(host: &str) -> Self {
        Self {
            workspace_id: None,
            host: host.to_string(),
        }
    }

    pub fn tenant(workspace_id: i32, host: &str) -> Self {
        Self {
            workspace_id: Some(workspace_id),
            host: host.to_string(),
        }
    }
}

/// Circuit breakers per transport, created lazily on first use and kept for
/// the process. The set is small: the instance relay plus one per workspace
/// that sends through its own server.
#[derive(Debug, Default)]
pub struct CircuitBreakerRegistry {
    breakers: Mutex<HashMap<BreakerKey, Arc<CircuitBreaker>>>,
}

impl CircuitBreakerRegistry {
    pub fn new() -> Self {
        Self {
            breakers: Mutex::new(HashMap::new()),
        }
    }

    /// The breaker for `key`, creating it on first use.
    pub async fn get(&self, key: BreakerKey) -> Arc<CircuitBreaker> {
        let mut map = self.breakers.lock().await;
        map.entry(key)
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

    async fn trip(cb: &CircuitBreaker) {
        for _ in 0..FAILURE_THRESHOLD {
            cb.record_failure().await;
        }
    }

    #[tokio::test]
    async fn registry_isolates_breakers_by_host() {
        let reg = CircuitBreakerRegistry::new();
        trip(&*reg.get(BreakerKey::instance("relay-a")).await).await;
        assert!(!reg.get(BreakerKey::instance("relay-a")).await.allow().await);
        assert!(reg.get(BreakerKey::instance("relay-b")).await.allow().await);
    }

    #[tokio::test]
    async fn a_workspace_server_trips_only_its_own_breaker() {
        let reg = CircuitBreakerRegistry::new();
        // Workspace 1 has a wrong password for a shared provider.
        trip(&*reg.get(BreakerKey::tenant(1, "smtp.gmail.com")).await).await;
        assert!(
            !reg.get(BreakerKey::tenant(1, "smtp.gmail.com"))
                .await
                .allow()
                .await
        );
        // Workspace 2 on the same provider, and the instance relay on the
        // same host, keep sending.
        assert!(
            reg.get(BreakerKey::tenant(2, "smtp.gmail.com"))
                .await
                .allow()
                .await
        );
        assert!(
            reg.get(BreakerKey::instance("smtp.gmail.com"))
                .await
                .allow()
                .await
        );
    }
}
