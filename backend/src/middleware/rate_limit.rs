//! Per-scope request rate limiting over `actix_limitation::Limiter`.
//!
//! Replaces the library's middleware, which answers a bare 500 for every
//! request while Redis is unreachable. Here a Redis failure lets the request
//! through and logs one warning per limiter per minute; a spent quota is a
//! 429 with `Retry-After` and the app's usual rate-limit body.
//!
//! The limiter is resolved by type (`web::Data<RequestLimiter>`), so a scope
//! picks its quota by registering its own `app_data`, which shadows the
//! app-level one.

use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use actix_limitation::{Error as LimitationError, Limiter};
use actix_web::body::EitherBody;
use actix_web::dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform};
use actix_web::{web, Error};
use futures::future::LocalBoxFuture;
use std::future::{ready, Ready};
use tracing::{error, warn};

type KeyFn = dyn Fn(&ServiceRequest) -> Option<String> + Send + Sync;

/// Seconds between "Redis unreachable" warnings for one limiter.
const WARN_INTERVAL_SECS: u64 = 60;

/// A count that takes longer than this is treated as Redis being unreachable,
/// so a black-holed host can't stall every request.
const COUNT_TIMEOUT: Duration = Duration::from_secs(1);

/// One quota: a `Limiter` and the key it counts a request under. The key
/// function lives here because `Limiter` keeps its own private.
pub struct RequestLimiter {
    name: &'static str,
    limiter: Limiter,
    key: Arc<KeyFn>,
    /// Unix seconds of the last unreachable warning; 0 before the first.
    last_warned: AtomicU64,
    /// Requests let through unthrottled since the last warning.
    unthrottled: AtomicU64,
}

impl RequestLimiter {
    /// `limit` requests per `period` for each key. A request with no key
    /// (no client address) is not counted. Builds without contacting Redis.
    pub fn build<F>(
        name: &'static str,
        redis_url: &str,
        limit: usize,
        period: Duration,
        key: F,
    ) -> Result<Self, LimitationError>
    where
        F: Fn(&ServiceRequest) -> Option<String> + Send + Sync + 'static,
    {
        let limiter = Limiter::builder(redis_url)
            .key_by(|_: &ServiceRequest| None)
            .limit(limit)
            .period(period)
            .build()?;
        Ok(Self {
            name,
            limiter,
            key: Arc::new(key),
            last_warned: AtomicU64::new(0),
            unthrottled: AtomicU64::new(0),
        })
    }

    /// Whether to log the unreachable warning at `now` (unix seconds). At
    /// most one caller wins per interval.
    fn should_warn(&self, now: u64) -> bool {
        let last = self.last_warned.load(Ordering::Relaxed);
        if last != 0 && now < last.saturating_add(WARN_INTERVAL_SECS) {
            return false;
        }
        self.last_warned
            .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
    }

    fn note_unreachable(&self, err: &dyn std::fmt::Display) {
        self.unthrottled.fetch_add(1, Ordering::Relaxed);
        if self.should_warn(unix_now()) {
            let count = self.unthrottled.swap(0, Ordering::Relaxed);
            warn!(
                error = %err,
                count,
                "Rate limiter {} could not reach Redis; requests pass unthrottled",
                self.name
            );
        }
    }
}

fn unix_now() -> u64 {
    u64::try_from(chrono::Utc::now().timestamp()).unwrap_or(0)
}

/// Rate-limit middleware. Wrap a scope or resource with it; the
/// `web::Data<RequestLimiter>` in scope decides the quota.
#[derive(Clone, Copy, Default)]
pub struct RateLimit;

impl<S, B> Transform<S, ServiceRequest> for RateLimit
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type InitError = ();
    type Transform = RateLimitMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(RateLimitMiddleware {
            service: Rc::new(service),
        }))
    }
}

pub struct RateLimitMiddleware<S> {
    service: Rc<S>,
}

impl<S, B> Service<ServiceRequest> for RateLimitMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = Rc::clone(&self.service);
        let Some(limiter) = req.app_data::<web::Data<RequestLimiter>>().cloned() else {
            // A wiring bug, not a client error: refuse rather than run unlimited.
            error!("Rate-limited route has no RequestLimiter in app data");
            return Box::pin(async move {
                Ok(req.into_response(
                    crate::errors::internal("Internal server error").map_into_right_body(),
                ))
            });
        };
        let Some(key) = (limiter.key)(&req) else {
            return Box::pin(async move {
                service
                    .call(req)
                    .await
                    .map(ServiceResponse::map_into_left_body)
            });
        };

        Box::pin(async move {
            match tokio::time::timeout(COUNT_TIMEOUT, limiter.limiter.count(key.clone())).await {
                Ok(Ok(_)) => {}
                Ok(Err(LimitationError::LimitExceeded(status))) => {
                    warn!("Rate limit exceeded for {key}");
                    let retry_after = u64::try_from(status.reset_epoch_utc())
                        .unwrap_or(0)
                        .saturating_sub(unix_now())
                        .max(1);
                    return Ok(req.into_response(
                        crate::errors::too_many_requests("Too many requests", retry_after)
                            .map_into_right_body(),
                    ));
                }
                Ok(Err(LimitationError::Client(e))) => limiter.note_unreachable(&e),
                Ok(Err(e)) => limiter.note_unreachable(&e),
                Err(elapsed) => limiter.note_unreachable(&elapsed),
            }
            service
                .call(req)
                .await
                .map(ServiceResponse::map_into_left_body)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter() -> RequestLimiter {
        RequestLimiter::build(
            "test",
            "redis://127.0.0.1:1",
            1,
            Duration::from_secs(60),
            |_| None,
        )
        .unwrap()
    }

    #[test]
    fn warns_once_per_interval() {
        let l = limiter();
        let t0 = 1_800_000_000;
        assert!(l.should_warn(t0));
        assert!(!l.should_warn(t0));
        assert!(!l.should_warn(t0 + WARN_INTERVAL_SECS - 1));
        assert!(l.should_warn(t0 + WARN_INTERVAL_SECS));
        assert!(!l.should_warn(t0 + WARN_INTERVAL_SECS + 1));
    }
}
