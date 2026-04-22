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

/// Microsoft Graph delta sync. Pulls users/devices/groups from the
/// configured Microsoft provider into the local tables. No-op when
/// MS credentials aren't configured — see
/// [`crate::handlers::msgraph_integration::run_scheduled_delta_sync`]
/// for the details.
pub async fn msgraph_delta_sync(pool: Pool) -> Result<()> {
    crate::handlers::msgraph_integration::run_scheduled_delta_sync(&pool).await
}
