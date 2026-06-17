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

use std::sync::Arc;

use anyhow::{Context, Result};
use diesel::sql_types::BigInt;
use diesel::{sql_query, QueryableByName, RunQueryDsl};
use tracing::{info, warn};

use crate::db::{DbConnection, Pool};
use crate::repository::{active_sessions, refresh_tokens};
use crate::services::search::SearchService;

// Advisory-lock keys for scheduler jobs that must run on a single machine
// per tick. The scheduler has no leader election, so it ticks on every
// machine; jobs with a cross-machine hazard guard their tick via
// `try_job_lock`. Keys are distinct from each other and from
// `PROVISION_LOCK_KEY` in `services::plugins::provisioning`.
const MSGRAPH_DELTA_SYNC_LOCK: i64 = 0x004e_6f73_4d53_4744;
const THUMBNAIL_BACKFILL_LOCK: i64 = 0x004e_6f73_5448_4d42;

/// Holds a per-job Postgres advisory lock for the duration of one
/// scheduler tick, releasing it on drop — including on an unwinding panic,
/// so the lock can never strand a job across the whole fleet. Parks a
/// dedicated pooled connection for the tick; the job pulls its own for
/// work.
pub struct JobLock {
    conn: DbConnection,
    key: i64,
    name: &'static str,
}

impl Drop for JobLock {
    fn drop(&mut self) {
        // Session-scoped lock: explicit unlock here, because the pooled
        // connection is returned to the pool (reused) rather than closed.
        if let Err(e) = sql_query("SELECT pg_advisory_unlock($1)")
            .bind::<BigInt, _>(self.key)
            .execute(&mut self.conn)
        {
            warn!(job = self.name, error = %e, "scheduler: failed to release advisory lock");
        }
    }
}

/// Try to take job `name`'s advisory lock without blocking.
/// `Ok(Some(guard))` — acquired; hold the guard for the tick.
/// `Ok(None)` — another machine holds it; skip this tick.
/// `Err` — couldn't reach Postgres to ask.
pub fn try_job_lock(pool: &Pool, key: i64, name: &'static str) -> Result<Option<JobLock>> {
    #[derive(QueryableByName)]
    struct Acquired {
        #[diesel(sql_type = diesel::sql_types::Bool)]
        pg_try_advisory_lock: bool,
    }
    let mut conn = pool.get().context("advisory-lock conn")?;
    let acquired = sql_query("SELECT pg_try_advisory_lock($1)")
        .bind::<BigInt, _>(key)
        .get_result::<Acquired>(&mut conn)
        .context("pg_try_advisory_lock")?
        .pg_try_advisory_lock;
    Ok(acquired.then_some(JobLock { conn, key, name }))
}

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

/// Backfill avatar thumbnails that are missing on disk or unset in the
/// DB. Thumbnails are derived from the avatar original and are not part
/// of backups (skipped as cheap to regenerate), so a CLI restore or a
/// partial file sync can leave them absent. Restore paths regenerate
/// eagerly; this daily sweep is the idempotent safety net that catches
/// any drift and does no work once everything is in place.
pub async fn backfill_user_thumbnails(pool: Pool) -> Result<()> {
    use crate::services::avatar_thumbnails::{backfill_thumbnails, BackfillMode};
    // Single-machine guard: the sweep re-encodes and re-uploads every
    // avatar to S3, so running it on each machine duplicates the upload
    // traffic for no benefit (the result is identical).
    let _lock = match try_job_lock(&pool, THUMBNAIL_BACKFILL_LOCK, "users.backfill_thumbnails")? {
        Some(lock) => lock,
        None => {
            info!("scheduler: users.backfill_thumbnails skipped — another machine holds the lock");
            return Ok(());
        }
    };
    let mut conn = pool.get().context("db pool")?;
    let stats = backfill_thumbnails(
        &mut conn,
        BackfillMode::MissingOnly,
        "scheduler:thumbnail_backfill",
    )
    .await;
    if stats.regenerated > 0 || stats.failed > 0 {
        info!(
            checked = stats.checked,
            regenerated = stats.regenerated,
            failed = stats.failed,
            "scheduler: avatar thumbnails backfilled"
        );
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
    crate::sync::partitions::ensure_partitions(&mut conn, 60).context("ensure sync partitions")?;
    Ok(())
}

/// Microsoft Graph delta sync. Pulls users/devices/groups from the
/// configured Microsoft provider into the local tables. No-op when
/// MS credentials aren't configured — see
/// [`crate::handlers::msgraph_integration::run_scheduled_delta_sync`]
/// for the details.
pub async fn msgraph_delta_sync(pool: Pool) -> Result<()> {
    // Single-machine guard: concurrent runs race the per-entity delta
    // token in `sync_history` (last writer wins, silently dropping the
    // other machine's progress until those records change again).
    let _lock = match try_job_lock(&pool, MSGRAPH_DELTA_SYNC_LOCK, "msgraph.delta_sync")? {
        Some(lock) => lock,
        None => {
            info!("scheduler: msgraph.delta_sync skipped — another machine holds the lock");
            return Ok(());
        }
    };
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
/// Drop `idempotency_keys` rows past the retention window so the
/// cache table doesn't accumulate stale rows forever. Default 24h
/// horizon; the M5 control-plane retries either succeed within
/// seconds-to-minutes or escalate to operator attention, so a day
/// is plenty. Override via `IDEMPOTENCY_KEY_RETENTION_HOURS`.
pub async fn prune_idempotency_keys(pool: Pool) -> Result<()> {
    let hours = retention_hours("IDEMPOTENCY_KEY_RETENTION_HOURS", 24);
    let horizon = chrono::Utc::now().naive_utc() - chrono::Duration::hours(hours.into());
    let mut conn = pool.get().context("db pool")?;
    let removed = crate::repository::idempotency_keys::prune_older_than(&mut conn, horizon)
        .context("prune idempotency keys")?;
    if removed > 0 {
        info!(
            count = removed,
            retention_hours = hours,
            "scheduler: idempotency keys pruned"
        );
    }
    Ok(())
}

/// Same shape as `retention_days` but the unit is hours; used by the
/// short-horizon caches (idempotency, etc).
fn retention_hours(env_var: &str, default: i32) -> i32 {
    std::env::var(env_var)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|h: &i32| *h > 0)
        .unwrap_or(default)
}

pub async fn prune_csp_reports(pool: Pool) -> Result<()> {
    let days = retention_days("CSP_REPORT_RETENTION_DAYS", 30);
    // csp_reports is RLS-enabled and this prune crosses every
    // workspace (scheduler is platform-level). Elevate via
    // background_run so the DELETE isn't filtered to zero rows
    // post-DSN-flip.
    let removed =
        crate::sync::session::background_run(&pool, "scheduler:prune_csp_reports", |conn| {
            crate::repository::csp_reports::prune_older_than(conn, days)
        })
        .map_err(|e| anyhow::anyhow!("prune CSP reports: {e}"))?;
    if removed > 0 {
        info!(
            count = removed,
            retention_days = days,
            "scheduler: CSP reports pruned"
        );
    }
    Ok(())
}

/// Prune `security_events` rows past the retention window. Long window
/// by default (one year) — login / MFA / password-reset records remain
/// useful for "did anyone touch this account last March?" investigations.
/// Override via `SECURITY_EVENT_RETENTION_DAYS`.
pub async fn prune_security_events(pool: Pool) -> Result<()> {
    let days = retention_days("SECURITY_EVENT_RETENTION_DAYS", 365);
    let mut conn = pool.get().context("db pool")?;
    let removed = crate::utils::security_events::prune_older_than(&mut conn, days)
        .context("prune security events")?;
    if removed > 0 {
        info!(
            count = removed,
            retention_days = days,
            "scheduler: security events pruned"
        );
    }
    Ok(())
}

/// Prune webhook_deliveries past the retention window. The deliveries
/// table fills fast on a busy webhook (one row per subscriber per event)
/// and only the recent rows have diagnostic value. Override via
/// `WEBHOOK_DELIVERY_RETENTION_DAYS`; default 30 days.
pub async fn prune_webhook_deliveries(pool: Pool) -> Result<()> {
    let days = retention_days("WEBHOOK_DELIVERY_RETENTION_DAYS", 30);
    // webhook_deliveries is RLS-enabled; cross-tenant prune.
    let removed =
        crate::sync::session::background_run(&pool, "scheduler:prune_webhook_deliveries", |conn| {
            crate::repository::webhooks::prune_deliveries_older_than(conn, days)
        })
        .map_err(|e| anyhow::anyhow!("prune webhook deliveries: {e}"))?;
    if removed > 0 {
        info!(
            count = removed,
            retention_days = days,
            "scheduler: webhook deliveries pruned"
        );
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
    let days = retention_days("AUDIT_LOG_RETENTION_DAYS", 540);
    drop_old_event_partitions(pool, "audit_log", days as i64).await
}

/// Drop monthly partitions of `sync_actions` whose upper bound is older
/// than the retention window. sync_actions are change events for client
/// cache hydration; a 90-day window is generous since clients re-bootstrap
/// from snapshots when they lag. Override via `SYNC_ACTIONS_RETENTION_DAYS`.
pub async fn prune_sync_actions_partitions(pool: Pool) -> Result<()> {
    let days = retention_days("SYNC_ACTIONS_RETENTION_DAYS", 90);
    drop_old_event_partitions(pool, "sync_actions", days as i64).await
}

/// Read a positive day-count from `env_var`, falling back to `default`.
/// Values that don't parse or aren't positive are treated as unset —
/// the operator's mistyped `RETENTION_DAYS=-1` shouldn't disable
/// pruning entirely, since the failure mode (unbounded growth) is
/// worse than the inconvenience of ignoring a bad value.
fn retention_days(env_var: &str, default: i32) -> i32 {
    std::env::var(env_var)
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|d: &i32| *d > 0)
        .unwrap_or(default)
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

/// Sweep expired leases on `outbound_emails`. A worker that crashes
/// between `claim_batch` and a terminal `mark_*` leaves the row in
/// `sending` with a lease; this job moves expired-lease rows back to
/// `failed` so the next claim cycle picks them up. Cheap (the partial
/// `outbound_emails_lease_idx` keeps the scan tiny). Default cadence:
/// 60s.
pub async fn sweep_outbound_email_leases(pool: Pool) -> Result<()> {
    // outbound_emails is RLS-enabled; cross-workspace sweep.
    let swept = crate::sync::session::background_run(
        &pool,
        "scheduler:sweep_outbound_email_leases",
        crate::repository::outbound_emails::sweep_expired_leases,
    )
    .map_err(|e| anyhow::anyhow!("sweep_expired_leases: {e}"))?;
    if swept > 0 {
        info!(
            count = swept,
            "scheduler: outbound_emails leases swept (worker crash recovery)"
        );
    }
    Ok(())
}

/// Re-verify workspace DKIM sending domains. A domain stays `verified` only
/// while its published record keeps resolving to our key; if a tenant removes
/// the record, this flips it back to `pending` so sends fall back to the
/// platform identity instead of shipping mail that fails DKIM/DMARC. See
/// [`crate::services::dkim_verification::reverify_all`]. Default cadence: hourly.
pub async fn reverify_dkim_domains(pool: Pool) -> Result<()> {
    let stats = crate::services::dkim_verification::reverify_all(&pool)
        .await
        .map_err(|e| anyhow::anyhow!("reverify_dkim_domains: {e}"))?;
    if stats.checked > 0 {
        info!(
            checked = stats.checked,
            still_verified = stats.still_verified,
            reverted = stats.reverted,
            errored = stats.errored,
            "scheduler: DKIM sending domains re-verified"
        );
    }
    Ok(())
}

/// Hard-delete soft-deleted users whose grace window has elapsed.
/// The single + bulk delete handlers stamp `users.deleted_at`; this
/// worker is the only path that runs the destructive cascade for
/// those rows after the configurable retention window
/// (`NOSDESK_USER_PURGE_GRACE_DAYS`, default 30).
///
/// Each purge gets its own savepoint via `with_actor_context` so an
/// FK violation on one user doesn't abort the whole sweep. The
/// "scheduler:user_purge" system actor lands in the audit_log for
/// every purged row so the eventual hard-delete is traceable.
///
/// Search-index removal flows through the same
/// `UserDeletedObserver` the admin-initiated purge uses, so a row
/// purged by the worker disappears from search at the same moment
/// it disappears from the table.
pub async fn purge_soft_deleted_users(pool: Pool, search: Arc<SearchService>) -> Result<()> {
    let mut conn = pool.get().context("db pool")?;
    let grace = crate::repository::users::purge_grace_window();
    let cutoff = chrono::Utc::now().naive_utc() - grace;
    let pending = crate::repository::users::list_users_pending_purge(&mut conn, cutoff)
        .context("list pending purges")?;
    if pending.is_empty() {
        return Ok(());
    }

    // Purge is conceptually cross-tenant: a user typically spans
    // multiple workspaces (workspace_members), and purge_user runs
    // ~30 UPDATE/DELETE statements against tickets / comments /
    // projects / attachments / assets / documentation_pages /
    // article_contents / sync_history / etc., all RLS-enabled.
    // A workspace-pinned actor matches only its workspace's rows
    // and leaves orphans in every other workspace, causing the
    // next purge to fail the FK check. with_actor_bypass_context
    // (nosdesk_admin role, BYPASSRLS) is the correct shape.
    let actor = crate::sync::actor::ActorContext::system("scheduler:user_purge");
    let mut purged = 0usize;
    let mut failed = 0usize;
    for user in pending {
        let result = crate::sync::session::with_actor_bypass_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |conn| crate::repository::users::purge_user(&user.uuid, conn, Some(&search)),
        );
        match result {
            Ok(_) => {
                purged += 1;
                info!(
                    user_uuid = %user.uuid,
                    name = %user.name,
                    deleted_at = ?user.deleted_at,
                    "scheduler:user_purge: purged"
                );
            }
            Err(e) => {
                failed += 1;
                warn!(
                    user_uuid = %user.uuid,
                    error = ?e,
                    "scheduler:user_purge: purge failed (will retry next tick)"
                );
            }
        }
    }
    info!(
        purged,
        failed,
        grace_days = grace.num_days(),
        "scheduler: soft-deleted users sweep complete"
    );
    Ok(())
}

/// Hard-delete archived workspaces whose grace window has elapsed
/// (Phase 4 W1). Mirrors `purge_soft_deleted_users`: BYPASSRLS role
/// elevation for the cross-tenant DELETE, per-row error isolation,
/// system-actor audit attribution. Cascade-deletes every tenant row
/// via the existing ON DELETE CASCADE FKs.
///
/// Grace window default is 30 days; operators override via
/// `WORKSPACE_HARD_DELETE_GRACE_DAYS`. See
/// `repository::workspaces::purge_grace_window` for the precedence.
pub async fn purge_archived_workspaces(pool: Pool) -> Result<()> {
    let mut conn = pool.get().context("db pool")?;
    let grace = crate::repository::workspaces::purge_grace_window();
    let cutoff = chrono::Utc::now()
        - chrono::Duration::from_std(grace).unwrap_or(chrono::Duration::days(30));
    let pending = crate::repository::workspaces::list_workspaces_pending_purge(&mut conn, cutoff)
        .context("list workspaces pending purge")?;
    if pending.is_empty() {
        return Ok(());
    }

    // Workspace hard-delete is intrinsically cross-tenant: the
    // cascading DELETE touches every tenant table at once. We need
    // the nosdesk_admin BYPASSRLS role for the txn so RLS doesn't
    // hide rows from the cascade.
    let actor = crate::sync::actor::ActorContext::system("scheduler:workspace_purge");
    let mut purged = 0usize;
    let mut failed = 0usize;
    for ws in pending {
        let result = crate::sync::session::with_actor_bypass_context::<_, diesel::result::Error>(
            &mut conn,
            &actor,
            |conn| crate::repository::workspaces::hard_delete_workspace(conn, ws.id, cutoff),
        );
        match result {
            Ok(n) if n > 0 => {
                purged += 1;
                info!(
                    workspace_id = ws.id,
                    slug = %ws.slug,
                    archived_at = ?ws.archived_at,
                    "scheduler:workspace_purge: hard-deleted"
                );
            }
            Ok(_) => {
                // Race against a restore that fired between the
                // list and the delete. Not an error.
                info!(
                    workspace_id = ws.id,
                    slug = %ws.slug,
                    "scheduler:workspace_purge: skipped (no longer eligible)"
                );
            }
            Err(e) => {
                failed += 1;
                warn!(
                    workspace_id = ws.id,
                    slug = %ws.slug,
                    error = ?e,
                    "scheduler:workspace_purge: delete failed (will retry next tick)"
                );
            }
        }
    }
    info!(
        purged,
        failed,
        grace_days = grace.as_secs() / 86_400,
        "scheduler: archived workspaces sweep complete"
    );
    Ok(())
}

const SLA_BREACH_ACTOR_REF: &str = "scheduler:sla_breach";
/// Per-tick cap on each timer scan. With both response + resolution
/// scans running, a workspace can process up to 2×LIMIT breaches per
/// minute; a deploy-time backlog of thousands drains over several
/// ticks rather than fan-firing all writes at once.
const SLA_BREACH_SCAN_LIMIT: i64 = 100;

/// Which SLA timer breached on a given ticket. The scan + process
/// helpers thread this through because each timer targets a different
/// pair of columns; Diesel's type-safe DSL forces the per-arm match.
#[derive(Debug, Clone, Copy)]
enum SlaBreachKind {
    Response,
    Resolution,
}

/// Periodic SLA breach-detection sweep. Scans the materialised
/// `sla_response_target_at` / `sla_resolution_target_at` columns
/// (Phase 1c) for tickets whose target has passed without a
/// `*_breached_at` stamp, atomically stamps the breach, and emits a
/// `ticket.sla_updated` sync_action so connected clients flip the
/// pill to "Breached" live — without this, a long-open tab would
/// keep showing the previous on-track/at-risk colour until something
/// else mutated the row. The same sync_action carries the recomputed
/// pill JSON, so the frontend pool's shallow-merge picks it up
/// without needing a dedicated SSE event variant.
///
/// Two scans per tick (response + resolution timers) — each breaches
/// independently. Partial indexes (`tickets_sla_response_scan_idx`,
/// `tickets_sla_resolution_scan_idx`) make each scan cheap even at
/// workspace scale; the LIMIT bounds work per tick so a backlog of
/// thousands of breached tickets after a deploy drains over a few
/// minutes rather than thrashing one tick.
///
/// Cross-workspace: the scan runs under BYPASSRLS so it sees every
/// workspace's breached tickets; per-ticket processing then switches
/// into that ticket's workspace context so the audited
/// `sla_*_breached_at` UPDATE and the emitted sync_action attribute
/// to the correct workspace.
pub async fn detect_sla_breaches(
    pool: Pool,
    notification_service: Arc<crate::services::notifications::NotificationService>,
) -> Result<()> {
    let mut conn = pool.get().context("db pool")?;
    let candidates = scan_breach_candidates(&mut conn).context("scan SLA breach candidates")?;
    if candidates.is_empty() {
        return Ok(());
    }

    let (mut processed, mut failed) = (0usize, 0usize);
    for (ticket_id, kind, workspace_id) in candidates {
        match process_one_breach(&mut conn, ticket_id, kind, workspace_id) {
            Ok(Some(ctx)) => {
                processed += 1;
                // Async fanout outside the DB workspace context: notify
                // the assignee + watchers via NotificationService (in-app
                // + email). The pill repaint and webhook deliveries both
                // flow from the `ticket.sla_breached` sync_action emitted
                // inside process_one_breach (pool + webhook outbox); no
                // discrete SSE is involved.
                fanout_breach(&notification_service, &ctx).await;
            }
            Ok(None) => {} // lost the idempotency race — normal no-op
            Err(e) => {
                failed += 1;
                warn!(
                    ticket_id,
                    kind = ?kind,
                    error = ?e,
                    "scheduler:sla_breach: ticket processing failed"
                );
            }
        }
    }

    if processed > 0 || failed > 0 {
        info!(processed, failed, "scheduler: SLA breach detection swept");
    }
    Ok(())
}

/// Cross-workspace scan under BYPASSRLS for tickets eligible to fire
/// a breach event on either timer. Bounded per-call by
/// `SLA_BREACH_SCAN_LIMIT` to keep one sweep predictable.
fn scan_breach_candidates(
    conn: &mut crate::db::DbConnection,
) -> Result<Vec<(i32, SlaBreachKind, i32)>> {
    use crate::schema::tickets;
    use crate::sync::actor::ActorContext;
    use crate::sync::session::with_actor_bypass_context;
    use diesel::prelude::*;

    let actor = ActorContext::system(SLA_BREACH_ACTOR_REF);
    let now = chrono::Utc::now().naive_utc();
    let candidates = with_actor_bypass_context::<_, diesel::result::Error>(conn, &actor, |conn| {
        let response = tickets::table
            .filter(tickets::sla_response_target_at.is_not_null())
            .filter(tickets::sla_response_target_at.le(now))
            .filter(tickets::sla_response_breached_at.is_null())
            .select((tickets::id, tickets::workspace_id))
            .limit(SLA_BREACH_SCAN_LIMIT)
            .load::<(i32, i32)>(conn)?;
        let resolution = tickets::table
            .filter(tickets::sla_resolution_target_at.is_not_null())
            .filter(tickets::sla_resolution_target_at.le(now))
            .filter(tickets::sla_resolution_breached_at.is_null())
            .select((tickets::id, tickets::workspace_id))
            .limit(SLA_BREACH_SCAN_LIMIT)
            .load::<(i32, i32)>(conn)?;
        Ok(response
            .into_iter()
            .map(|(id, ws)| (id, SlaBreachKind::Response, ws))
            .chain(
                resolution
                    .into_iter()
                    .map(|(id, ws)| (id, SlaBreachKind::Resolution, ws)),
            )
            .collect())
    })?;
    Ok(candidates)
}

/// Everything the orchestrator needs to fan out an SLA breach
/// after `process_one_breach` has done its DB work. Computed inside
/// the workspace context (so the watcher / assignee lookups attribute
/// correctly) and returned out so the async notification + SSE work
/// happens outside any DB transaction.
struct BreachContext {
    ticket_id: i32,
    ticket_title: String,
    workspace_id: i32,
    kind: SlaBreachKind,
    breached_at: chrono::DateTime<chrono::Utc>,
    assignee_uuid: Option<uuid::Uuid>,
    watcher_uuids: Vec<uuid::Uuid>,
}

/// Atomically stamp the breach + emit a pill-refresh sync_action +
/// gather the bits the orchestrator needs for the async fanout. Runs
/// in the ticket's workspace context so the audited stamp and the
/// emit attribute to the correct workspace. Returns `Ok(None)` when
/// the idempotency guard caught a duplicate (another tick won the
/// race) — a normal no-op, not an error.
fn process_one_breach(
    conn: &mut crate::db::DbConnection,
    ticket_id: i32,
    kind: SlaBreachKind,
    workspace_id: i32,
) -> Result<Option<BreachContext>, diesel::result::Error> {
    use crate::models::Ticket;
    use crate::schema::tickets;
    use crate::sync::actor::ActorContext;
    use crate::sync::emit::{self, SyncEmit};
    use crate::sync::groups;
    use crate::sync::session::with_actor_context;
    use chrono::{DateTime, Utc};
    use diesel::prelude::*;
    use serde_json::json;

    let actor = ActorContext::system(SLA_BREACH_ACTOR_REF).with_workspace(workspace_id);
    with_actor_context(conn, &actor, |conn| {
        // Atomic idempotency stamp — the `WHERE breached_at IS NULL`
        // predicate makes a concurrent tick a no-op rather than a
        // duplicate emit.
        let stamped = match kind {
            SlaBreachKind::Response => diesel::update(tickets::table.find(ticket_id))
                .filter(tickets::sla_response_breached_at.is_null())
                .set(tickets::sla_response_breached_at.eq(diesel::dsl::now))
                .execute(conn)?,
            SlaBreachKind::Resolution => diesel::update(tickets::table.find(ticket_id))
                .filter(tickets::sla_resolution_breached_at.is_null())
                .set(tickets::sla_resolution_breached_at.eq(diesel::dsl::now))
                .execute(conn)?,
        };
        if stamped == 0 {
            return Ok(None);
        }
        // Reload to capture the freshly-stamped *_breached_at and
        // compute a pill whose breached flag now reads true.
        let ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;
        let sla = crate::services::sla::recompute_and_stamp_sla_for_ticket(conn, &ticket);
        let groups = groups::for_ticket(conn, &ticket)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: crate::models::SyncAggregate::Ticket,
                aggregate_id: ticket_id.to_string(),
                op: crate::models::SyncOp::Update,
                event_type: "ticket.sla_updated",
                data: json!({ "id": ticket_id, "sla": sla }),
                groups: groups.clone(),
                causation_id: None,
            },
        )?;
        // Dedicated breach event for webhook delivery (only fires on a
        // real breach, unlike sla_updated which also fires on every SLA
        // recompute). op U side event: no row `id` in data, so the
        // object pool skips it; the webhook outbox maps it to
        // `ticket.sla_breached`.
        emit::record(
            conn,
            SyncEmit {
                aggregate: crate::models::SyncAggregate::Ticket,
                aggregate_id: ticket_id.to_string(),
                op: crate::models::SyncOp::Update,
                event_type: "ticket.sla_breached",
                data: json!({ "ticket_id": ticket_id, "timer": kind.label() }),
                groups,
                causation_id: None,
            },
        )?;
        let watcher_uuids =
            crate::repository::ticket_watchers::watcher_uuids(conn, ticket_id).unwrap_or_default();
        let breached_at = match kind {
            SlaBreachKind::Response => ticket.sla_response_breached_at,
            SlaBreachKind::Resolution => ticket.sla_resolution_breached_at,
        };
        // The stamp above guarantees breached_at is now Some; degrade
        // gracefully if not.
        let now = Utc::now();
        let to_utc = |opt: Option<chrono::NaiveDateTime>| -> DateTime<Utc> {
            opt.map(|t| DateTime::<Utc>::from_naive_utc_and_offset(t, Utc))
                .unwrap_or(now)
        };
        Ok(Some(BreachContext {
            ticket_id,
            ticket_title: ticket.title.clone(),
            workspace_id,
            kind,
            breached_at: to_utc(breached_at),
            assignee_uuid: ticket.assignee_uuid,
            watcher_uuids,
        }))
    })
}

/// Async fanout for one detected breach. The DB work already
/// committed in `process_one_breach` (incl. the `ticket.sla_breached`
/// sync_action that drives the pool pill repaint + the webhook outbox);
/// this only does the notification surfaces: in-app + email to the
/// assignee and every watcher (deduped).
async fn fanout_breach(
    notification_service: &crate::services::notifications::NotificationService,
    ctx: &BreachContext,
) {
    use crate::services::notifications::types::{
        NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode,
    };

    // System-triggered: no human actor. nil uuid + "System" name is
    // the convention other system-triggered notifications would use.
    let actor = NotificationActor {
        uuid: uuid::Uuid::nil(),
        name: "System".to_string(),
        avatar_thumb: None,
    };
    let entity = NotificationEntity::Ticket {
        id: ctx.ticket_id,
        title: ctx.ticket_title.clone(),
    };

    let timer_label = ctx.kind.label();
    let body = format!(
        "{} SLA on #{} \"{}\" breached at {}",
        timer_label,
        ctx.ticket_id,
        ctx.ticket_title,
        ctx.breached_at.format("%Y-%m-%d %H:%M UTC"),
    );

    // Recipients: assignee + watchers, deduped. Skip if no recipient
    // (an unassigned ticket with no watchers has nobody to notify —
    // the pill still flips via the sync_action; the webhook still
    // fires below).
    let mut recipients: Vec<uuid::Uuid> = ctx
        .assignee_uuid
        .into_iter()
        .chain(ctx.watcher_uuids.iter().copied())
        .collect();
    recipients.sort();
    recipients.dedup();
    for recipient in recipients {
        let payload = NotificationPayload::new(
            NotificationTypeCode::SlaBreached,
            recipient,
            actor.clone(),
            entity.clone(),
            ctx.workspace_id,
        )
        .with_body(body.clone());
        if let Err(e) = notification_service.notify(payload).await {
            warn!(
                ticket_id = ctx.ticket_id,
                recipient = %recipient,
                error = %e,
                "scheduler:sla_breach: notify failed"
            );
        }
    }
    // The breach webhook is delivered from the webhook_outbox via the
    // ticket.sla_breached sync_action emitted in process_one_breach; no
    // SSE broadcast needed here.
}

impl SlaBreachKind {
    fn label(&self) -> &'static str {
        match self {
            SlaBreachKind::Response => "Response",
            SlaBreachKind::Resolution => "Resolution",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::pg::PgConnection;
    use diesel::r2d2::{self, ConnectionManager};

    // A real 2-connection pool (no test-transaction wrapper) so two
    // sessions can be held at once to observe advisory-lock contention.
    fn real_pool() -> Pool {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for the advisory-lock test");
        let manager = ConnectionManager::<PgConnection>::new(url);
        r2d2::Pool::builder()
            .max_size(2)
            .build(manager)
            .expect("build advisory-lock test pool")
    }

    // A test-only key so a stray lock (app running, or a prior crashed run)
    // can't collide with the assertion.
    const TEST_LOCK: i64 = 0x7465_7374_4c4f_434b; // "testLOCK"

    #[test]
    fn advisory_lock_serialises_then_releases() {
        let pool = real_pool();

        let first = try_job_lock(&pool, TEST_LOCK, "test").expect("acquire");
        assert!(first.is_some(), "first caller takes the lock");

        let contended = try_job_lock(&pool, TEST_LOCK, "test").expect("try while held");
        assert!(
            contended.is_none(),
            "a second caller is locked out while the lock is held"
        );

        drop(first); // releases on drop (Drop runs pg_advisory_unlock)

        let reacquired = try_job_lock(&pool, TEST_LOCK, "test").expect("re-acquire");
        assert!(
            reacquired.is_some(),
            "the lock is free again once the guard drops"
        );
    }
}
