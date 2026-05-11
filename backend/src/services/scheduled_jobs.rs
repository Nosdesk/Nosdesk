//! Concrete periodic-job functions wired by `main.rs` into the
//! [`crate::services::scheduler`] runtime.
//!
//! Each function has the scheduler-compatible signature
//! `async fn(…) -> anyhow::Result<()>` and captures its own
//! dependencies (pool, storage, …) at call time. They're kept here
//! so `main.rs` stays readable — adding a new periodic job is a
//! matter of writing one function here and one line in main.rs.
//!
//! # Error semantics
//!
//! A returned `Err` produces one `error!` log line and leaves the
//! scheduler's status registry flagged `failed`; the next tick runs
//! normally. These jobs are maintenance — transient failures are
//! expected and not fatal.

use anyhow::{Context, Result};
use tracing::info;

use crate::db::Pool;
use crate::repository::{active_sessions, refresh_tokens};

/// Delete rows from `active_sessions` whose `expires_at` is in the
/// past. One-liner today; kept here (rather than being a closure in
/// main.rs) so future additions — e.g. an audit-event write when
/// large batches are pruned — have an obvious home.
pub async fn cleanup_expired_sessions(pool: Pool) -> Result<()> {
    let mut conn = pool.get().context("db pool")?;
    let removed = active_sessions::cleanup_expired(&mut conn).context("delete expired sessions")?;
    if removed > 0 {
        info!(count = removed, "scheduler: expired sessions pruned");
    }
    Ok(())
}

/// Delete rows from `refresh_tokens` whose `expires_at` is in the past.
/// Revoked-but-not-expired rows are kept so audit trails are intact;
/// this only prunes naturally expired tokens.
pub async fn cleanup_expired_refresh_tokens(pool: Pool) -> Result<()> {
    let mut conn = pool.get().context("db pool")?;
    let removed =
        refresh_tokens::cleanup_expired(&mut conn).context("delete expired refresh tokens")?;
    if removed > 0 {
        info!(count = removed, "scheduler: expired refresh tokens pruned");
    }
    Ok(())
}

/// Provision sync_actions and audit_log partitions out to the
/// configured lookahead. Called daily so an INSERT after the last
/// provisioned month never fails. Idempotent — uses
/// CREATE TABLE IF NOT EXISTS internally, so running it multiple
/// times a day (e.g. across a deploy + scheduled tick collision)
/// is safe.
pub async fn ensure_sync_partitions(pool: Pool) -> Result<()> {
    let mut conn = pool.get().context("db pool")?;
    // 60-day lookahead matches the architecture doc's recommendation
    // and gives us nearly two months of headroom against any single
    // missed run.
    crate::sync::partitions::ensure_partitions(&mut conn, 60)
        .context("ensure sync partitions")?;
    Ok(())
}

/// Microsoft Graph delta sync. Pulls users/devices/groups from the
/// configured Microsoft provider into the local tables. No-op when
/// MS credentials aren't configured — see
/// [`crate::handlers::msgraph_integration::run_scheduled_delta_sync`]
/// for the details.
pub async fn msgraph_delta_sync(pool: Pool) -> Result<()> {
    crate::handlers::msgraph_integration::run_scheduled_delta_sync(&pool).await
}

/// Prune CSP violation reports older than the configured retention
/// window. Reports are useful for triaging policy regressions soon
/// after they happen but lose value quickly — month-old reports
/// rarely surface anything actionable. Without pruning the table
/// would grow unbounded under a noisy reporter (eg. a single
/// browser-extension injection that fires on every page load).
///
/// Retention defaults to 30 days; configurable via
/// `CSP_REPORT_RETENTION_DAYS` env var so deployments with stricter
/// audit / compliance requirements can dial it up or down.
pub async fn prune_csp_reports(pool: Pool) -> Result<()> {
    let days: i32 = std::env::var("CSP_REPORT_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|d: &i32| *d > 0)
        .unwrap_or(30);

    let mut conn = pool.get().context("db pool")?;
    let removed = crate::repository::csp_reports::prune_older_than(&mut conn, days)
        .context("prune CSP reports")?;
    if removed > 0 {
        info!(count = removed, retention_days = days, "scheduler: CSP reports pruned");
    }
    Ok(())
}

/// Prune `security_events` rows past the retention window. Long window
/// by default (one year) — login / MFA / password-reset records remain
/// useful for "did anyone touch this account last March?" investigations.
/// Override via `SECURITY_EVENT_RETENTION_DAYS`.
pub async fn prune_security_events(pool: Pool) -> Result<()> {
    let days: i32 = std::env::var("SECURITY_EVENT_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|d: &i32| *d > 0)
        .unwrap_or(365);

    let mut conn = pool.get().context("db pool")?;
    let removed = crate::utils::security_events::prune_older_than(&mut conn, days)
        .context("prune security events")?;
    if removed > 0 {
        info!(count = removed, retention_days = days, "scheduler: security events pruned");
    }
    Ok(())
}

/// Prune webhook_deliveries past the retention window. The deliveries
/// table fills fast on a busy webhook (one row per subscriber per event)
/// and only the recent rows have diagnostic value. Override via
/// `WEBHOOK_DELIVERY_RETENTION_DAYS`; default 30 days.
pub async fn prune_webhook_deliveries(pool: Pool) -> Result<()> {
    let days: i32 = std::env::var("WEBHOOK_DELIVERY_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|d: &i32| *d > 0)
        .unwrap_or(30);

    let mut conn = pool.get().context("db pool")?;
    let removed = crate::repository::webhooks::prune_deliveries_older_than(&mut conn, days)
        .context("prune webhook deliveries")?;
    if removed > 0 {
        info!(count = removed, retention_days = days, "scheduler: webhook deliveries pruned");
    }
    Ok(())
}

/// Drop monthly partitions of `audit_log` whose upper bound is older than
/// the retention window. Lock-friendly via DETACH CONCURRENTLY +
/// DROP TABLE — see `sync::partitions::drop_partitions_older_than`.
///
/// Retention defaults to 540 days (~18 months) — long enough for typical
/// "what happened a year ago?" investigations, bounded enough that a
/// single audited table doesn't fill disk indefinitely. Override via
/// `AUDIT_LOG_RETENTION_DAYS`.
pub async fn prune_audit_log_partitions(pool: Pool) -> Result<()> {
    let days: i64 = std::env::var("AUDIT_LOG_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|d: &i64| *d > 0)
        .unwrap_or(540);
    drop_old_event_partitions(pool, "audit_log", days).await
}

/// Drop monthly partitions of `sync_actions` whose upper bound is older
/// than the retention window. sync_actions are change events for client
/// cache hydration; a 90-day window is generous since clients re-bootstrap
/// from snapshots when they lag. Override via `SYNC_ACTIONS_RETENTION_DAYS`.
pub async fn prune_sync_actions_partitions(pool: Pool) -> Result<()> {
    let days: i64 = std::env::var("SYNC_ACTIONS_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|d: &i64| *d > 0)
        .unwrap_or(90);
    drop_old_event_partitions(pool, "sync_actions", days).await
}

async fn drop_old_event_partitions(
    pool: Pool,
    parent: &'static str,
    retention_days: i64,
) -> Result<()> {
    let mut conn = pool.get().context("db pool")?;
    let cutoff = chrono::Utc::now().date_naive() - chrono::Duration::days(retention_days);
    let dropped = crate::sync::partitions::drop_partitions_older_than(&mut conn, parent, cutoff)
        .with_context(|| format!("drop {parent} partitions older than {cutoff}"))?;
    if !dropped.is_empty() {
        info!(
            parent = parent,
            cutoff = %cutoff,
            count = dropped.len(),
            partitions = ?dropped,
            "scheduler: dropped expired partitions"
        );
    }
    Ok(())
}
