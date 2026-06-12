//! Short-TTL Redis cache for dashboard analytics payloads.
//!
//! Dashboard aggregations tolerate a few seconds of staleness, and the
//! same window is requested repeatedly (every viewer of a workspace, on
//! every refresh). Caching the computed payload keeps the hot path off
//! Postgres entirely on a hit, so a burst of dashboard loads stops
//! translating into a burst of pooled-connection checkouts.
//!
//! The cache is best-effort: any Redis error (down, unreachable, decode
//! mismatch) resolves to a miss, and the caller falls through to the
//! live query. It never fails a request.
//!
//! Keys MUST be workspace-scoped by the caller. Serving one workspace's
//! aggregate to another would be a tenancy violation, so the workspace
//! id is part of every key (see `kpi_summary_key`).

use redis::AsyncCommands;
use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, warn};

/// Default TTL for cached analytics payloads. Short enough that a
/// dashboard never shows materially stale numbers, long enough to
/// absorb the fan-out of a single page-load and rapid refreshes.
pub const DEFAULT_TTL_SECS: u64 = 30;

pub struct AnalyticsCache {
    client: redis::Client,
}

impl AnalyticsCache {
    pub fn new(redis_url: &str) -> Result<Self, redis::RedisError> {
        Ok(Self {
            client: redis::Client::open(redis_url)?,
        })
    }

    /// Fetch and decode a cached value. Returns `None` on a miss or any
    /// Redis / decode error (the caller recomputes).
    pub async fn get_json<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        let mut conn = self.client.get_multiplexed_async_connection().await.ok()?;
        let raw: Option<String> = conn.get(key).await.ok()?;
        let raw = raw?;
        match serde_json::from_str(&raw) {
            Ok(value) => {
                debug!(key = %key, "analytics cache HIT");
                Some(value)
            }
            // A decode failure means a stale schema in the cache; treat
            // it as a miss so the fresh value overwrites it on set.
            Err(e) => {
                warn!(key = %key, error = ?e, "analytics cache decode failed; treating as miss");
                None
            }
        }
    }

    /// Encode and store a value with a TTL. Best-effort: logs and
    /// returns on any error rather than surfacing it.
    pub async fn set_json<T: Serialize>(&self, key: &str, value: &T, ttl_secs: u64) {
        let payload = match serde_json::to_string(value) {
            Ok(p) => p,
            Err(e) => {
                warn!(key = %key, error = ?e, "analytics cache encode failed");
                return;
            }
        };
        match self.client.get_multiplexed_async_connection().await {
            Ok(mut conn) => {
                if let Err(e) = conn.set_ex::<_, _, ()>(key, payload, ttl_secs).await {
                    warn!(key = %key, error = ?e, "analytics cache write failed");
                }
            }
            Err(e) => warn!(key = %key, error = ?e, "analytics cache connection failed"),
        }
    }
}

/// Build the workspace-scoped cache key for a KPI-summary request. The
/// `v1` segment lets a payload-shape change invalidate the namespace
/// without a flush. Every parameter that changes the result is in the
/// key; absent prior bounds collapse to `none`.
#[allow(clippy::too_many_arguments)]
pub fn kpi_summary_key(
    workspace_id: i32,
    from: &str,
    to: &str,
    prior_from: Option<&str>,
    prior_to: Option<&str>,
    sparkline: bool,
    tz: &str,
) -> String {
    format!(
        "analytics:kpi_summary:v1:{workspace_id}:{from}:{to}:{}:{}:{sparkline}:{tz}",
        prior_from.unwrap_or("none"),
        prior_to.unwrap_or("none"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_workspace_scoped() {
        // The leading workspace id must differ so two tenants never
        // collide on the same window — a cross-tenant cache read would
        // leak one workspace's aggregates into another.
        let a = kpi_summary_key(1, "f", "t", None, None, true, "UTC");
        let b = kpi_summary_key(2, "f", "t", None, None, true, "UTC");
        assert_ne!(a, b);
        assert!(a.contains(":1:"));
        assert!(b.contains(":2:"));
    }

    #[test]
    fn key_varies_with_every_result_affecting_param() {
        let base = kpi_summary_key(1, "f", "t", None, None, true, "UTC");
        // Each of these changes the payload, so each must change the key.
        assert_ne!(base, kpi_summary_key(1, "f2", "t", None, None, true, "UTC"));
        assert_ne!(base, kpi_summary_key(1, "f", "t2", None, None, true, "UTC"));
        assert_ne!(
            base,
            kpi_summary_key(1, "f", "t", Some("pf"), Some("pt"), true, "UTC")
        );
        assert_ne!(base, kpi_summary_key(1, "f", "t", None, None, false, "UTC"));
        assert_ne!(
            base,
            kpi_summary_key(1, "f", "t", None, None, true, "Australia/Melbourne")
        );
    }

    #[test]
    fn absent_prior_is_stable() {
        // Same request twice yields the same key (deterministic), and the
        // absent prior collapses to a fixed sentinel rather than varying.
        let a = kpi_summary_key(1, "f", "t", None, None, true, "UTC");
        let b = kpi_summary_key(1, "f", "t", None, None, true, "UTC");
        assert_eq!(a, b);
        assert!(a.contains(":none:none:"));
    }
}
