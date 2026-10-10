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
use diesel::sql_types::{Array, BigInt, Integer, Text, Uuid as SqlUuid};
use diesel::{sql_query, Connection, OptionalExtension, QueryableByName, RunQueryDsl};
use std::collections::HashMap;
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
const LOAN_REMINDER_LOCK: i64 = 0x004e_6f73_4c6f_616e;
const NOTIFICATION_DIGEST_LOCK: i64 = 0x004e_6f73_4e44_4947;
const LDAP_RECONCILE_LOCK: i64 = 0x004e_6f73_4c44_5243;
const KNOWLEDGE_GAP_DETECT_LOCK: i64 = 0x004e_6f73_4b47_4450;
const APPROVAL_TIMEOUT_LOCK: i64 = 0x004e_6f73_4150_544f;
const GUEST_RESIDUE_LOCK: i64 = 0x004e_6f73_4752_4553;
const EMAIL_LOGO_COPIES_LOCK: i64 = 0x004e_6f73_454d_4c47;
// Partition drops take a session try-lock that skips the tick when a peer
// machine is already pruning. Per-parent keys so audit_log and sync_actions
// prune independently.
const AUDIT_LOG_PARTITION_PRUNE_LOCK: i64 = 0x004e_6f73_414c_4450;
const SYNC_ACTIONS_PARTITION_PRUNE_LOCK: i64 = 0x004e_6f73_5341_4450;

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

/// Send email digests: batch the notifications a user set to `email` = `digest`
/// into one summary email per user and workspace. Runs daily, single-machine
/// via the advisory lock. Idempotent: it marks the source rows
/// `email`-delivered, so a re-run never re-sends. v1 covers explicit per-user
/// `email=digest` prefs; a workspace-default of `digest` (rare) is a follow-up.
/// `base_url` is the instance's configured `FRONTEND_URL`, the fallback for
/// the digest's link and for the domain its security note names.
pub async fn send_notification_digests(pool: Pool, base_url: String) -> Result<()> {
    let _lock = match try_job_lock(&pool, NOTIFICATION_DIGEST_LOCK, "notifications.digest")? {
        Some(lock) => lock,
        None => {
            info!("scheduler: notifications.digest skipped — another machine holds the lock");
            return Ok(());
        }
    };

    // Cross-workspace scan (bypass): notifications of a type the user set to
    // email=digest, not yet email-delivered, within the lookback window.
    #[derive(QueryableByName)]
    struct PendingRow {
        #[diesel(sql_type = Integer)]
        id: i32,
        #[diesel(sql_type = SqlUuid)]
        user_uuid: uuid::Uuid,
        #[diesel(sql_type = Integer)]
        workspace_id: i32,
        #[diesel(sql_type = Text)]
        title: String,
    }
    // cross-tenant: cross-workspace scan builds the digest work-list; each item is handled per-workspace below.
    let pending: Vec<PendingRow> = crate::sync::session::background_run(
        &pool,
        "background:notification_digest_scan",
        |conn| {
            sql_query(
                "SELECT n.id, n.user_uuid, n.workspace_id, n.title \
                 FROM notifications n \
                 JOIN notification_preferences np \
                   ON np.user_uuid = n.user_uuid \
                  AND np.notification_type_id = n.notification_type_id \
                  AND np.channel = 'email' AND np.frequency = 'digest' \
                 WHERE n.created_at > now() - interval '7 days' \
                   AND NOT (n.channels_delivered @> '[\"email\"]'::jsonb) \
                 ORDER BY n.user_uuid, n.created_at",
            )
            .load(conn)
        },
    )
    .map_err(|e| anyhow::anyhow!("digest scan: {e}"))?;

    if pending.is_empty() {
        return Ok(());
    }

    // One digest per user and workspace: each goes out under its own
    // workspace's name, sending identity and security note, and lists only
    // that workspace's notifications.
    let mut batches: std::collections::BTreeMap<(uuid::Uuid, i32), (Vec<String>, Vec<i32>)> =
        std::collections::BTreeMap::new();
    for r in pending {
        let batch = batches.entry((r.user_uuid, r.workspace_id)).or_default();
        batch.0.push(r.title);
        batch.1.push(r.id);
    }

    let mut recipients: HashMap<uuid::Uuid, Option<String>> = HashMap::new();
    let mut sent = 0usize;

    for ((user, workspace_id), (titles, ids)) in batches {
        #[derive(QueryableByName)]
        struct EmailRow {
            #[diesel(sql_type = Text)]
            email: String,
        }
        let recipient = match recipients.get(&user) {
            Some(known) => known.clone(),
            None => {
                // cross-tenant: user_emails is a global identity table (no workspace to pin to).
                let found: Option<String> = crate::sync::session::background_run(
                    &pool,
                    "background:notification_digest_email",
                    |conn| {
                        let row: Option<EmailRow> = sql_query(
                            "SELECT email FROM user_emails \
                             WHERE user_uuid = $1 AND is_primary = true LIMIT 1",
                        )
                        .bind::<SqlUuid, _>(user)
                        .get_result(conn)
                        .optional()?;
                        Ok(row.map(|e| e.email))
                    },
                )
                .map_err(|e| anyhow::anyhow!("digest email lookup: {e}"))?;
                recipients.insert(user, found.clone());
                found
            }
        };
        let Some(recipient) = recipient else {
            continue;
        };

        // Built, enqueued and marked delivered pinned to the workspace, under
        // row security: the settings read is this workspace's row and no
        // other, and outbound_emails + notifications are RLS + audited.
        let result = crate::sync::session::run_in_workspace(
            &pool,
            "scheduler:notification_digest",
            workspace_id,
            |conn| {
                enqueue_workspace_digest(
                    conn,
                    workspace_id,
                    user,
                    &recipient,
                    &base_url,
                    &titles,
                    &ids,
                )
            },
        );
        match result {
            Ok(_) => sent += 1,
            Err(e) => warn!(
                error = %e,
                workspace_id,
                user_uuid = %user,
                "digest: failed to send"
            ),
        }
    }

    if sent > 0 {
        info!(count = sent, "scheduler: notification digests sent");
    }
    Ok(())
}

/// Queue one workspace's digest for `user` and mark its notifications
/// email-delivered. Runs pinned to that workspace (`run_in_workspace`), never
/// under the bypass role: `get_site_settings` takes the first row it can see,
/// which under bypass is any workspace's.
///
/// The digest links where the workspace's other email to `user` does: the
/// agent app for an agent, the portal for anyone else, `base_url` (the
/// instance's `FRONTEND_URL`) when the workspace has neither.
fn enqueue_workspace_digest(
    conn: &mut crate::db::DbConnection,
    workspace_id: i32,
    user: uuid::Uuid,
    recipient: &str,
    base_url: &str,
    titles: &[String],
    ids: &[i32],
) -> diesel::QueryResult<()> {
    let link_base = crate::services::notifications::channels::email::recipient_link_base(
        conn,
        workspace_id,
        user,
        base_url,
    )
    .map_or_else(|| base_url.to_string(), |(base, _)| base);
    let settings = crate::repository::site_settings::get_site_settings(conn)?;
    let note = crate::utils::email_branding::security_note(
        conn,
        &settings,
        &link_base,
        crate::utils::email_branding::SentFrom::Workspace,
    );
    let locale = crate::utils::locale::effective_locale(
        crate::repository::user_locale::user_locale_preference(conn, user).as_deref(),
        &settings.default_locale,
    );
    let row = crate::services::transactional_email::prepare_notification_digest(
        recipient,
        &settings.app_name,
        &locale,
        &link_base,
        titles,
        note.as_deref(),
    );
    crate::repository::outbound_emails::enqueue_or_suppress(conn, row)?;
    sql_query(
        "UPDATE notifications \
         SET channels_delivered = channels_delivered || '[\"email\"]'::jsonb \
         WHERE id = ANY($1)",
    )
    .bind::<Array<Integer>, _>(ids)
    .execute(conn)?;
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

/// Make the email copies of logos that have none: logos uploaded before
/// email copies existed, and ones a restore or import brought in without.
/// Does no work once every logo has its copy.
pub async fn make_email_logo_copies(pool: Pool) -> Result<()> {
    // One machine is enough: the copies land in shared storage.
    let _lock = match try_job_lock(&pool, EMAIL_LOGO_COPIES_LOCK, "branding.email_logo_copies")? {
        Some(lock) => lock,
        None => return Ok(()),
    };
    crate::services::email_logos::make_missing(&pool, crate::utils::storage::process_storage())
        .await;
    Ok(())
}

/// Provision sync_actions and audit_log partitions out to the
/// configured lookahead. Called daily so an INSERT after the last
/// provisioned month never fails. Idempotent — uses
/// CREATE TABLE IF NOT EXISTS internally, so running it multiple
/// times a day (e.g. across a deploy + scheduled tick collision)
/// is safe.
pub async fn ensure_sync_partitions(pool: Pool) -> Result<()> {
    // Partition CREATE / ATTACH need schema-owner privileges the runtime
    // `nosdesk_app` role lacks; run the DDL over the privileged
    // MIGRATION_DATABASE_URL role when set (same role that applies
    // migrations). See `ddl_conn`.
    let mut conn = ddl_conn(&pool)?;
    // 60-day lookahead matches the architecture doc's recommendation
    // and gives us nearly two months of headroom against any single
    // missed run.
    crate::sync::partitions::ensure_partitions(&mut conn, 60).context("ensure sync partitions")?;
    Ok(())
}

/// Acquire a connection privileged enough for partition DDL
/// (`CREATE`/`ATTACH`/`DETACH`/`DROP` on the `sync_actions` / `audit_log`
/// parents). Prefers the `MIGRATION_DATABASE_URL` schema-owner role; falls
/// back to `pool` for single-role dev / self-host where `DATABASE_URL`
/// already owns the schema.
///
/// The returned connection keeps its pool alive via r2d2's internal `Arc`,
/// so callers don't hold the (possibly short-lived privileged) pool handle
/// themselves.
fn ddl_conn(pool: &Pool) -> Result<crate::db::DbConnection> {
    match crate::db::privileged_ddl_pool() {
        Some(priv_pool) => priv_pool.get().context("privileged ddl pool"),
        None => pool.get().context("db pool"),
    }
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

/// Nightly LDAP full reconcile. Resets the DirSync cursor and runs a full sync
/// for the LDAP-enabled bootstrap workspace, catching drift the incremental
/// DirSync stream missed (and priming the complete current-directory set for the
/// future deprovision pass). No-op when LDAP isn't configured — see
/// [`crate::services::ldap::reconcile::run_scheduled_reconcile`].
pub async fn ldap_nightly_reconcile(pool: Pool) -> Result<()> {
    // Single-machine guard: concurrent runs would race the cursor cookie (last
    // writer wins, silently dropping the other run's progress).
    let _lock = match try_job_lock(&pool, LDAP_RECONCILE_LOCK, "ldap.nightly_reconcile")? {
        Some(lock) => lock,
        None => {
            info!("scheduler: ldap.nightly_reconcile skipped — another machine holds the lock");
            return Ok(());
        }
    };
    crate::services::ldap::reconcile::run_scheduled_reconcile(&pool).await
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
        // cross-tenant: cross-workspace retention prune of csp_reports.
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

/// Retention for self-serve workspace exports: fail jobs stuck in
/// pending/processing (a crashed background task, since the spawn is
/// fire-and-forget with no lease), then delete completed artifacts whose download
/// window has passed (storage file + row). Cross-tenant, so it runs under
/// background_run (BYPASSRLS) like the other sweeps.
pub async fn cleanup_expired_workspace_exports(pool: Pool) -> Result<()> {
    let now = chrono::Utc::now().naive_utc();
    let stale_cutoff = (chrono::Utc::now() - chrono::Duration::hours(1)).naive_utc();

    let failed =
        // cross-tenant: recover exports stranded by a crashed background task.
        crate::sync::session::background_run(&pool, "scheduler:workspace_export_stale", |conn| {
            crate::repository::workspace_export_jobs::fail_stale(conn, stale_cutoff)
        })
        .map_err(|e| anyhow::anyhow!("fail stale workspace exports: {e}"))?;
    if failed > 0 {
        info!(count = failed, "scheduler: stale workspace exports failed");
    }

    let expired =
        // cross-tenant: expired-artifact cleanup across every workspace.
        crate::sync::session::background_run(&pool, "scheduler:workspace_export_expired", |conn| {
            crate::repository::workspace_export_jobs::list_expired(conn, now)
        })
        .map_err(|e| anyhow::anyhow!("list expired workspace exports: {e}"))?;

    let mut purged = 0usize;
    for job in expired {
        if let Some(key) = job.file_path.as_deref() {
            let scoped = crate::utils::storage::WorkspaceScopedStorage::arc(
                crate::utils::storage::process_storage(),
                job.workspace_id,
            );
            if let Err(e) = scoped.delete_file(key).await {
                // Leave the row so we retry next sweep rather than orphaning the file.
                warn!(job_id = %job.id, error = ?e, "scheduler: export artifact delete failed; will retry");
                continue;
            }
        }
        let _ =
            // cross-tenant: delete the expired export's row after its file is gone.
            crate::sync::session::background_run(&pool, "scheduler:workspace_export_delete", |conn| {
                crate::repository::workspace_export_jobs::delete(conn, job.id)
            });
        purged += 1;
    }
    if purged > 0 {
        info!(
            count = purged,
            "scheduler: expired workspace exports purged"
        );
    }
    Ok(())
}

/// Auto-archive stale notifications so the bell/inbox self-prunes instead of
/// growing without bound. Read notifications older than
/// `NOTIFICATION_READ_RETENTION_DAYS` (default 30) are archived; anything older
/// than `NOTIFICATION_MAX_RETENTION_DAYS` (default 90) is archived regardless of
/// read state, so an ignored unread pile still gets a ceiling.
///
/// Archiving sets `archived_at` (the reversible archive axis, not a delete);
/// the notification stays retrievable under the inbox's archived filter. No SSE
/// is emitted — a live client picks up the change on its next bell fetch, same
/// as the other maintenance sweeps.
pub async fn auto_archive_stale_notifications(pool: Pool) -> Result<()> {
    use crate::schema::notifications::dsl as n;
    use diesel::prelude::*;

    let read_days = retention_days("NOTIFICATION_READ_RETENTION_DAYS", 30);
    let max_days = retention_days("NOTIFICATION_MAX_RETENTION_DAYS", 90);
    let now = chrono::Utc::now().naive_utc();
    let read_cutoff = now - chrono::Duration::days(read_days as i64);
    let max_cutoff = now - chrono::Duration::days(max_days as i64);

    // notifications is RLS-enabled and this sweep crosses every workspace
    // (scheduler is platform-level), so elevate via background_run. notifications
    // isn't audited, so a cross-workspace bulk UPDATE is safe (no audit_log
    // workspace_id NOT NULL to satisfy).
    // cross-tenant: cross-workspace retention sweep of the notification inbox.
    let archived = crate::sync::session::background_run(
        &pool,
        "scheduler:auto_archive_notifications",
        move |conn| {
            diesel::update(
                n::notifications.filter(n::archived_at.is_null()).filter(
                    n::is_read
                        .eq(true)
                        .and(n::created_at.lt(read_cutoff))
                        .or(n::created_at.lt(max_cutoff)),
                ),
            )
            .set(n::archived_at.eq(now))
            .execute(conn)
        },
    )
    .map_err(|e| anyhow::anyhow!("auto-archive notifications: {e}"))?;

    if archived > 0 {
        info!(
            archived,
            read_retention_days = read_days,
            max_retention_days = max_days,
            "scheduler: stale notifications auto-archived"
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
    let workspaces =
        // cross-tenant: finds which workspaces hold expired deliveries; each is pruned pinned to its own workspace below.
        crate::sync::session::background_run(&pool, "scheduler:prune_webhook_deliveries", |conn| {
            crate::repository::webhooks::workspaces_with_deliveries_older_than(conn, days)
        })
        .map_err(|e| anyhow::anyhow!("scan webhook deliveries: {e}"))?;
    // webhook_deliveries is audited: each delete is recorded in its own
    // workspace, so prune one workspace at a time, pinned.
    let mut removed = 0usize;
    for workspace_id in workspaces {
        match crate::sync::session::run_in_workspace(
            &pool,
            "scheduler:prune_webhook_deliveries",
            workspace_id,
            |conn| {
                crate::repository::webhooks::prune_deliveries_older_than(conn, workspace_id, days)
            },
        ) {
            Ok(n) => removed += n,
            Err(e) => warn!(workspace_id, error = ?e, "scheduler: webhook delivery prune failed"),
        }
    }
    if removed > 0 {
        info!(
            count = removed,
            retention_days = days,
            "scheduler: webhook deliveries pruned"
        );
    }
    Ok(())
}

/// Drop monthly partitions of `audit_log` that lie entirely before the
/// retention window, each in a short `DROP TABLE` under a lock timeout; see
/// `sync::partitions::drop_partitions_older_than`.
///
/// Retention defaults to 540 days (~18 months) — long enough for typical
/// "what happened a year ago?" investigations, bounded enough that a
/// single audited table doesn't fill disk indefinitely. Override via
/// `AUDIT_LOG_RETENTION_DAYS`.
pub async fn prune_audit_log_partitions(pool: Pool) -> Result<()> {
    let days = retention_days("AUDIT_LOG_RETENTION_DAYS", 540);
    drop_old_event_partitions(
        pool,
        "audit_log",
        AUDIT_LOG_PARTITION_PRUNE_LOCK,
        "audit_log.drop_old_partitions",
        days as i64,
    )
    .await
}

/// Drop monthly partitions of `sync_actions` that lie entirely before the
/// retention window. `sync_actions` is the ticket activity history, so it is
/// kept unless `SYNC_ACTIONS_RETENTION_DAYS` is set.
pub async fn prune_sync_actions_partitions(pool: Pool) -> Result<()> {
    prune_sync_actions_partitions_older_than(
        pool,
        optional_retention_days("SYNC_ACTIONS_RETENTION_DAYS"),
    )
    .await
}

/// [`prune_sync_actions_partitions`] with the retention given: `None` keeps
/// every partition.
pub async fn prune_sync_actions_partitions_older_than(
    pool: Pool,
    retention_days: Option<i32>,
) -> Result<()> {
    let Some(days) = retention_days else {
        return Ok(());
    };
    drop_old_event_partitions(
        pool,
        "sync_actions",
        SYNC_ACTIONS_PARTITION_PRUNE_LOCK,
        "sync_actions.drop_old_partitions",
        days as i64,
    )
    .await
}

/// A positive day-count from `env_var`, or `None` when it's unset. A value
/// that doesn't parse or isn't positive is also `None`, with a warning: for a
/// retention that is off by default, ignoring a bad value keeps the data.
fn optional_retention_days(env_var: &str) -> Option<i32> {
    let raw = std::env::var(env_var).ok()?;
    let days = raw.trim().parse::<i32>().ok().filter(|d| *d > 0);
    if days.is_none() {
        warn!("scheduler: {env_var} isn't a positive number of days; ignoring it");
    }
    days
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
    lock_key: i64,
    lock_name: &'static str,
    retention_days: i64,
) -> Result<()> {
    // Single-machine guard: two machines dropping the same partition race,
    // so skip the tick when a peer is already pruning this parent.
    let _lock = match try_job_lock(&pool, lock_key, lock_name)? {
        Some(lock) => lock,
        None => {
            info!("scheduler: {lock_name} skipped — another machine holds the lock");
            return Ok(());
        }
    };
    // Dropping a partition needs ownership of it, same as the provisioning
    // DDL; run over the privileged role when configured.
    let mut conn = ddl_conn(&pool)?;
    let cutoff = chrono::Utc::now().date_naive() - chrono::Duration::days(retention_days);
    let dropped = crate::sync::partitions::drop_partitions_older_than(&mut conn, parent, cutoff)
        .with_context(|| format!("drop {parent} partitions older than {cutoff}"))?;
    if !dropped.is_empty() {
        info!(
            partitioned_table = parent,
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
    // cross-tenant: operational lease sweep across every tenant's outbound queue.
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
/// Each purge gets its own transaction so an FK violation on one user
/// doesn't abort the whole sweep. The "scheduler:user_purge" system
/// actor lands in the audit_log, in the user's home workspace, for
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
    let mut homeless = 0usize;
    for user in pending {
        let result = purge_account_in_home_workspace(&mut conn, &actor, &user.uuid, Some(&search));
        match result {
            Ok(None) => homeless += 1,
            Ok(Some(_)) => {
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
    warn_homeless_accounts("scheduler:user_purge", homeless);
    info!(
        purged,
        failed,
        grace_days = grace.num_days(),
        "scheduler: soft-deleted users sweep complete"
    );
    Ok(())
}

/// Purge one account as `actor`, recorded in the account's home workspace
/// ([`crate::repository::users::home_workspace_id`]). The purge runs under
/// the bypass role because an account's rows can reach every workspace it
/// belongs to, but the audit trigger still needs a workspace for the rows it
/// writes, and a background connection starts with none. `Ok(None)` when the
/// account is in no workspace; it is left in place (see
/// [`warn_homeless_accounts`]).
///
/// For an account in several workspaces, rows the purge touches in the
/// others are recorded in the home workspace too.
fn purge_account_in_home_workspace(
    conn: &mut DbConnection,
    actor: &crate::sync::actor::ActorContext,
    user_uuid: &uuid::Uuid,
    observer: Option<&dyn crate::repository::users::UserDeletedObserver>,
) -> std::result::Result<Option<usize>, diesel::result::Error> {
    // cross-tenant: purging an account reaches every workspace it belongs to.
    crate::sync::session::with_actor_bypass_context(conn, actor, |conn| {
        let Some(workspace_id) = crate::repository::users::home_workspace_id(conn, user_uuid)?
        else {
            return Ok(None);
        };
        crate::sync::session::pin_workspace(conn, workspace_id)?;
        crate::repository::users::purge_user(user_uuid, conn, observer).map(Some)
    })
}

/// One warning per run for the accounts a purge job left because they are in
/// no workspace: the audit trigger needs one to record the purge in. They
/// stay in the job's scan, so the warning repeats until someone acts.
fn warn_homeless_accounts(job: &str, count: usize) {
    if count > 0 {
        warn!(
            "{job}: {count} account(s) left in place, they are in no workspace to record the purge in"
        );
    }
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
        // cross-tenant: a workspace hard delete cascades through every tenant table.
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

/// Remove the stored files of hard-deleted workspaces, everything under
/// `ws/{id}/`, as queued by `hard_delete_workspace`. A purge that fails stays
/// queued and is retried on the next run.
pub async fn purge_deleted_workspace_files(pool: Pool) -> Result<()> {
    let storage = crate::utils::storage::process_storage();
    purge_deleted_workspace_files_in(&pool, storage.as_ref()).await
}

/// [`purge_deleted_workspace_files`] against the given storage.
pub async fn purge_deleted_workspace_files_in(
    pool: &Pool,
    storage: &dyn crate::utils::storage::Storage,
) -> Result<()> {
    let pending =
        // cross-tenant: the purge queue is platform-level, and the workspaces it names no longer exist.
        crate::sync::session::background_run(pool, "scheduler:workspace_file_purge", |conn| {
            crate::repository::workspace_file_purges::pending(conn)
        })
        .map_err(|e| anyhow::anyhow!("list workspace file purges: {e}"))?;

    for workspace_id in pending {
        let error = match storage.delete_prefix(&format!("ws/{workspace_id}/")).await {
            Ok(removed) => {
                info!(
                    workspace_id,
                    "scheduler:workspace_file_purge: removed {removed} stored files"
                );
                None
            }
            Err(e) => {
                warn!(
                    workspace_id,
                    error = ?e,
                    "scheduler:workspace_file_purge: delete failed (will retry next run)"
                );
                Some("storage delete failed")
            }
        };
        // cross-tenant: the purge queue is platform-level, and the workspaces it names no longer exist.
        crate::sync::session::background_run(pool, "scheduler:workspace_file_purge", |conn| {
            crate::repository::workspace_file_purges::record_attempt(conn, workspace_id, error)
        })
        .map_err(|e| anyhow::anyhow!("record workspace file purge: {e}"))?;
    }
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

    // Do all the DB work first (stamp + sync_action + webhook per ticket,
    // inside process_one_breach), collecting the breach contexts. Notification
    // fanout happens after, coalesced per recipient, so a recipient with many
    // tickets breaching in one sweep gets one summary rather than a storm.
    let mut breaches: Vec<BreachContext> = Vec::new();
    let mut failed = 0usize;
    for (ticket_id, kind, workspace_id) in candidates {
        match process_one_breach(&mut conn, ticket_id, kind, workspace_id) {
            Ok(Some(ctx)) => breaches.push(ctx),
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

    let processed = breaches.len();
    // A breach is told to the ticket's assignee and watchers. One with neither
    // goes to the workspace's admins instead; counted so the sweep line shows
    // how often that happens.
    let no_recipient = breaches
        .iter()
        .filter(|b| b.assignee_uuid.is_none() && b.watcher_uuids.is_empty())
        .count();
    // Async fanout outside the DB workspace context: notify the assignee +
    // watchers, or the admins, via NotificationService (in-app + email). The pill repaint and
    // webhook deliveries already flowed from the `ticket.sla_breached`
    // sync_action emitted inside process_one_breach; no discrete SSE here.
    let fanout = coalesced_fanout(&notification_service, &breaches).await;

    if processed > 0 || failed > 0 {
        // `notified`: people handed a notice (one per person and workspace);
        // `no_recipient`: breaches with no assignee or watcher to tell;
        // `admins_notified`: of `notified`, notices to admins about such a breach.
        info!(
            processed,
            failed,
            notified = fanout.notified,
            notify_failed = fanout.failed,
            no_recipient,
            admins_notified = fanout.admins_notified,
            "scheduler: SLA breach detection swept"
        );
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
    use crate::sync::groups::PENDING_VERIFICATION as PENDING;
    use crate::sync::session::with_actor_bypass_context;
    use diesel::prelude::*;

    let actor = ActorContext::system(SLA_BREACH_ACTOR_REF);
    let now = chrono::Utc::now().naive_utc();
    // cross-tenant: the SLA breach scan covers every workspace's tickets.
    let candidates = with_actor_bypass_context::<_, diesel::result::Error>(conn, &actor, |conn| {
        // Order by the target time so the most-overdue breaches fire first,
        // fairly across tenants (an unordered LIMIT let one workspace's large
        // backlog crowd out an earlier breach in another). SKIP LOCKED avoids
        // two instances re-scanning the same rows; the atomic breached_at stamp
        // in process_one_breach is the real cross-instance dedup.
        let response = tickets::table
            .filter(tickets::sla_response_target_at.is_not_null())
            .filter(tickets::sla_response_target_at.le(now))
            .filter(tickets::sla_response_breached_at.is_null())
            .filter(tickets::verification_state.is_distinct_from(PENDING))
            .select((tickets::id, tickets::workspace_id))
            .order(tickets::sla_response_target_at.asc())
            .limit(SLA_BREACH_SCAN_LIMIT)
            .for_update()
            .skip_locked()
            .load::<(i32, i32)>(conn)?;
        let resolution = tickets::table
            .filter(tickets::sla_resolution_target_at.is_not_null())
            .filter(tickets::sla_resolution_target_at.le(now))
            .filter(tickets::sla_resolution_breached_at.is_null())
            .filter(tickets::verification_state.is_distinct_from(PENDING))
            .select((tickets::id, tickets::workspace_id))
            .order(tickets::sla_resolution_target_at.asc())
            .limit(SLA_BREACH_SCAN_LIMIT)
            .for_update()
            .skip_locked()
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
    ticket_number: i32,
    ticket_title: String,
    workspace_id: i32,
    kind: SlaBreachKind,
    breached_at: chrono::DateTime<chrono::Utc>,
    assignee_uuid: Option<uuid::Uuid>,
    watcher_uuids: Vec<uuid::Uuid>,
    /// The workspace's admins, read only when the ticket has no assignee and
    /// no watchers: they are told instead, so the breach isn't missed.
    admin_uuids: Vec<uuid::Uuid>,
}

/// Atomically stamp the breach + emit a pill-refresh sync_action +
/// gather the bits the orchestrator needs for the async fanout. Runs
/// in the ticket's workspace context so the audited stamp and the
/// emit attribute to the correct workspace. Returns `Ok(None)`, a
/// normal no-op rather than an error, when the idempotency guard caught
/// a duplicate (another tick won the race) or the ticket is finished.
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
        // A finished ticket (Done, Cancelled, Merged) can't breach. A target
        // it still carries is left from before it finished; recomputing
        // clears it, and nothing is stamped, emitted or sent.
        let Some(ticket) = tickets::table
            .find(ticket_id)
            .first::<Ticket>(conn)
            .optional()?
        else {
            return Ok(None);
        };
        // A guest ticket waiting for confirmation has no SLA yet either;
        // recomputing clears any target it was left with.
        if crate::services::sla::is_pending_verification(&ticket)
            || crate::services::sla::StateClock::of_state_id(conn, ticket.workflow_state_id)
                == crate::services::sla::StateClock::Stopped
        {
            crate::services::sla::recompute_and_stamp_sla_for_ticket(conn, &ticket);
            return Ok(None);
        }

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
        // Told once, with the breach: the stamp above makes a later sweep skip it.
        let admin_uuids = if ticket.assignee_uuid.is_none() && watcher_uuids.is_empty() {
            crate::repository::workspaces::admin_uuids(conn, workspace_id).unwrap_or_default()
        } else {
            Vec::new()
        };
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
            ticket_number: ticket.number,
            ticket_title: ticket.title.clone(),
            workspace_id,
            kind,
            breached_at: to_utc(breached_at),
            assignee_uuid: ticket.assignee_uuid,
            watcher_uuids,
            admin_uuids,
        }))
    })
}

/// One coalesced notification to send: the recipient, the workspace it belongs
/// to, the representative ticket to link to, and the (single or summary) body.
struct CoalescedNotice {
    recipient: uuid::Uuid,
    workspace_id: i32,
    ticket_id: i32,
    ticket_number: i32,
    ticket_title: String,
    body: String,
    /// The recipient is told about at least one breach as an admin, because
    /// no one was assigned to or watching that ticket.
    as_admin: bool,
}

/// Group a sweep's breaches into one notification per (recipient, workspace).
/// Keyed by workspace too because one sweep spans every workspace's breaches
/// and a summary must not mix tickets across workspaces (nor notify into the
/// wrong one). A recipient with one breached ticket gets the per-ticket body;
/// one with several gets a single summary linking to the most-overdue ticket.
/// A breach with no assignee and no watchers goes to the workspace's admins.
/// Pure so the grouping + summary logic is unit-tested without the DB.
fn coalesce_breaches(breaches: &[BreachContext]) -> Vec<CoalescedNotice> {
    use std::collections::BTreeMap;

    let mut by_recipient: BTreeMap<(uuid::Uuid, i32), (Vec<&BreachContext>, bool)> =
        BTreeMap::new();
    for b in breaches {
        let mut recipients: Vec<uuid::Uuid> = b
            .assignee_uuid
            .into_iter()
            .chain(b.watcher_uuids.iter().copied())
            .collect();
        let as_admin = recipients.is_empty();
        if as_admin {
            recipients.extend(b.admin_uuids.iter().copied());
        }
        recipients.sort();
        recipients.dedup();
        for recipient in recipients {
            let entry = by_recipient.entry((recipient, b.workspace_id)).or_default();
            entry.0.push(b);
            entry.1 |= as_admin;
        }
    }

    by_recipient
        .into_iter()
        .map(|((recipient, workspace_id), (tickets, as_admin))| {
            if tickets.len() == 1 {
                let b = tickets[0];
                CoalescedNotice {
                    recipient,
                    workspace_id,
                    ticket_id: b.ticket_id,
                    ticket_number: b.ticket_number,
                    ticket_title: b.ticket_title.clone(),
                    body: format!(
                        "{} SLA on #{} \"{}\" breached at {}",
                        b.kind.label(),
                        b.ticket_number,
                        b.ticket_title,
                        b.breached_at.format("%Y-%m-%d %H:%M UTC"),
                    ),
                    as_admin,
                }
            } else {
                // Summarise. Link to the most-overdue ticket as the
                // representative entity; list the first few numbers so the body
                // is actionable.
                let rep = tickets
                    .iter()
                    .min_by_key(|b| b.breached_at)
                    .expect("len > 1");
                let shown: Vec<String> = tickets
                    .iter()
                    .take(3)
                    .map(|b| format!("#{}", b.ticket_number))
                    .collect();
                let more = tickets.len().saturating_sub(shown.len());
                let listing = if more > 0 {
                    format!("{} and {} more", shown.join(", "), more)
                } else {
                    shown.join(", ")
                };
                CoalescedNotice {
                    recipient,
                    workspace_id,
                    ticket_id: rep.ticket_id,
                    ticket_number: rep.ticket_number,
                    ticket_title: rep.ticket_title.clone(),
                    body: format!("{} tickets breached their SLA: {}", tickets.len(), listing),
                    as_admin,
                }
            }
        })
        .collect()
}

/// What [`coalesced_fanout`] did: notices handed to the notification service,
/// notices it refused, and of those handed over, the ones to admins about a
/// breach no one else was told of.
#[derive(Debug, Default, Clone, Copy)]
struct FanoutCounts {
    notified: usize,
    failed: usize,
    admins_notified: usize,
}

/// Coalesced notification fanout for a sweep's detected breaches. The DB work
/// already committed per ticket in `process_one_breach` (incl. the
/// `ticket.sla_breached` sync_action that drives the pool pill repaint + the
/// webhook outbox); this only does the in-app + email surfaces. Grouping is in
/// `coalesce_breaches`; this just turns each notice into a payload + notify.
async fn coalesced_fanout(
    notification_service: &crate::services::notifications::NotificationService,
    breaches: &[BreachContext],
) -> FanoutCounts {
    use crate::services::notifications::types::{
        NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode,
    };

    let mut counts = FanoutCounts::default();
    if breaches.is_empty() {
        return counts;
    }

    // System-triggered: no human actor. `kind: System` marks the origin;
    // the nil uuid + "System" name feed the self-skip and display.
    let actor = NotificationActor {
        uuid: uuid::Uuid::nil(),
        name: "System".to_string(),
        avatar_thumb: None,
        kind: crate::sync::ActorKind::System,
    };

    for notice in coalesce_breaches(breaches) {
        let payload = NotificationPayload::new(
            NotificationTypeCode::SlaBreached,
            notice.recipient,
            actor.clone(),
            NotificationEntity::Ticket {
                id: notice.ticket_id,
                number: Some(notice.ticket_number),
                title: notice.ticket_title,
            },
            notice.workspace_id,
        )
        .with_body(notice.body);
        match notification_service.notify(payload).await {
            Ok(_) => {
                counts.notified += 1;
                if notice.as_admin {
                    counts.admins_notified += 1;
                }
            }
            Err(e) => {
                counts.failed += 1;
                warn!(
                    recipient = %notice.recipient,
                    error = %e,
                    "scheduler:sla_breach: notify failed"
                );
            }
        }
    }
    // The breach webhook is delivered from the webhook_outbox via the
    // ticket.sla_breached sync_action emitted in process_one_breach; no
    // SSE broadcast needed here.
    counts
}

impl SlaBreachKind {
    fn label(&self) -> &'static str {
        match self {
            SlaBreachKind::Response => "Response",
            SlaBreachKind::Resolution => "Resolution",
        }
    }
}

// ---- Device loan due-back reminders --------------------------------

const LOAN_DUE_SOON_DAYS_DEFAULT: i64 = 2;

/// Per-tick cap on each reminder scan, mirroring `SLA_BREACH_SCAN_LIMIT`, so a
/// post-rollout backlog can't fan out unbounded. The remainder is picked up on
/// the next daily tick.
const LOAN_REMINDER_SCAN_LIMIT: i64 = 100;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ReminderKind {
    DueSoon,
    Overdue,
}

/// Daily: remind borrowers about device loans due back soon or overdue.
///
/// Cross-machine guarded by an advisory lock. Scans every workspace's loans
/// under BYPASSRLS, dispatches a notification to the borrower via
/// `NotificationService` (in-app + email per their preferences), then stamps
/// the loan so each reminder fires once. A failed dispatch leaves the stamp
/// unset, so the next tick retries (eventual delivery over double-sending).
/// The due-soon horizon is `NOSDESK_LOAN_DUE_SOON_DAYS` (default 2).
pub async fn loan_due_reminders(
    pool: Pool,
    notification_service: Arc<crate::services::notifications::NotificationService>,
) -> Result<()> {
    use crate::repository::asset_loans as loans;
    use crate::services::notifications::types::{
        NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode,
    };

    let _lock = match try_job_lock(&pool, LOAN_REMINDER_LOCK, "asset_loans.due_reminders")? {
        Some(guard) => guard,
        None => return Ok(()), // another machine holds it this tick
    };

    let due_soon_days = std::env::var("NOSDESK_LOAN_DUE_SOON_DAYS")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .unwrap_or(LOAN_DUE_SOON_DAYS_DEFAULT);
    let today = chrono::Utc::now().date_naive();
    let horizon = today + chrono::Duration::days(due_soon_days);

    // Cross-workspace scan under BYPASSRLS.
    let mut conn = pool.get().context("db pool")?;
    let actor = crate::sync::actor::ActorContext::system("scheduler:loan_reminders");
    // cross-tenant: loan reminders are found across every workspace.
    let (overdue, due_soon) = crate::sync::session::with_actor_bypass_context::<
        _,
        diesel::result::Error,
    >(&mut conn, &actor, |conn| {
        Ok((
            loans::overdue_reminder_candidates(conn, today, LOAN_REMINDER_SCAN_LIMIT)?,
            loans::due_soon_reminder_candidates(conn, today, horizon, LOAN_REMINDER_SCAN_LIMIT)?,
        ))
    })
    .context("scan loan reminder candidates")?;
    drop(conn);

    if overdue.len() as i64 >= LOAN_REMINDER_SCAN_LIMIT {
        info!(
            cap = LOAN_REMINDER_SCAN_LIMIT,
            "scheduler:loan_reminders: overdue scan hit the cap; remainder next tick"
        );
    }
    if due_soon.len() as i64 >= LOAN_REMINDER_SCAN_LIMIT {
        info!(
            cap = LOAN_REMINDER_SCAN_LIMIT,
            "scheduler:loan_reminders: due-soon scan hit the cap; remainder next tick"
        );
    }

    // Loans successfully notified this tick, grouped by (workspace, kind) so we
    // stamp each group in one UPDATE after the loop instead of a fresh pool
    // checkout per loan. Trade-off: a crash between a notify and the batch
    // stamp re-notifies those loans next tick (bounded by the scan cap) — fine
    // for a low-stakes daily reminder.
    let mut to_stamp: std::collections::HashMap<(i32, ReminderKind), Vec<i32>> =
        std::collections::HashMap::new();

    let (mut sent, mut failed) = (0usize, 0usize);
    let work = overdue
        .into_iter()
        .map(|c| (ReminderKind::Overdue, c))
        .chain(due_soon.into_iter().map(|c| (ReminderKind::DueSoon, c)));
    for (kind, c) in work {
        let (type_code, body) = match kind {
            ReminderKind::Overdue => (
                NotificationTypeCode::LoanOverdue,
                format!("{} was due back on {}.", c.asset_name, c.due_back),
            ),
            ReminderKind::DueSoon => (
                NotificationTypeCode::LoanDueSoon,
                format!("{} is due back on {}.", c.asset_name, c.due_back),
            ),
        };
        let payload = NotificationPayload::new(
            type_code,
            c.borrower_user_uuid,
            // System reminder: a sentinel actor (no human triggered it).
            // `kind: System` marks the origin; the uuid is only used for the
            // self-skip + display, not stored.
            NotificationActor {
                uuid: uuid::Uuid::nil(),
                name: "Nosdesk".to_string(),
                avatar_thumb: None,
                kind: crate::sync::ActorKind::System,
            },
            NotificationEntity::Asset {
                id: c.asset_id,
                name: c.asset_name.clone(),
            },
            c.workspace_id,
        )
        .with_body(body);

        if let Err(e) = notification_service.notify(payload).await {
            failed += 1;
            warn!(loan_id = c.loan_id, error = %e, "scheduler:loan_reminders: notify failed; retry next tick");
            continue;
        }
        // Stamp only after a successful dispatch; deferred + batched below.
        to_stamp
            .entry((c.workspace_id, kind))
            .or_default()
            .push(c.loan_id);
        sent += 1;
    }

    // One workspace-pinned UPDATE per (workspace, kind) bucket.
    for ((workspace_id, kind), loan_ids) in to_stamp {
        let stamped = crate::sync::session::run_in_workspace(
            &pool,
            "scheduler:loan_reminders:stamp",
            workspace_id,
            |conn| match kind {
                ReminderKind::Overdue => loans::mark_overdue_notified_batch(conn, &loan_ids),
                ReminderKind::DueSoon => loans::mark_due_soon_notified_batch(conn, &loan_ids),
            },
        );
        if let Err(e) = stamped {
            warn!(workspace_id, count = loan_ids.len(), error = ?e, "scheduler:loan_reminders: failed to stamp batch");
        }
    }

    if sent > 0 || failed > 0 {
        info!(sent, failed, "scheduler: loan due reminders swept");
    }
    Ok(())
}

/// How often knowledge-gap detection runs. `NOSDESK_KNOWLEDGE_GAP_DETECT_SECS`
/// overrides it (for a dev walk, say); default hourly.
/// Approve requests nobody answered within their workspace's automatic-approval
/// period (off unless a workspace sets one). Scans every workspace under
/// BYPASSRLS, then decides each ticket pinned to its own workspace.
pub async fn approval_timeouts(pool: Pool) -> Result<()> {
    let _lock = match try_job_lock(&pool, APPROVAL_TIMEOUT_LOCK, "approvals.timeouts")? {
        Some(guard) => guard,
        None => return Ok(()),
    };
    // cross-tenant: cross-workspace scan builds the work-list; each ticket is decided per-workspace below.
    let due = crate::sync::session::background_run(&pool, "scheduler:approval_timeouts", |conn| {
        crate::repository::ticket_approvals::timed_out(conn, 500)
    })
    .context("scan timed-out approvals")?;
    for (workspace_id, ticket_id) in due {
        let result = crate::sync::session::run_in_workspace(
            &pool,
            "background:approval_timeout",
            workspace_id,
            |conn| {
                if crate::repository::ticket_approvals::auto_approve(conn, ticket_id)? {
                    // Best effort, like a decision's routing: the approval
                    // stands even if the request can't be assigned, and the
                    // savepoint keeps a failed assignment from undoing it.
                    if let Err(e) = conn.transaction(|conn| {
                        crate::services::assignment::AssignmentEngine::assign_after_approval(
                            conn, ticket_id,
                        )
                    }) {
                        warn!(
                            ticket_id,
                            error = ?e,
                            "scheduler:approval_timeouts: assignment after approval failed"
                        );
                    }
                }
                Ok(())
            },
        );
        if let Err(e) = result {
            warn!(ticket_id, error = ?e, "scheduler:approval_timeouts: auto-approve failed");
        }
    }
    Ok(())
}

/// Remove what the public request form leaves when nobody follows through:
/// requests never confirmed within [`guest_residue::UNCONFIRMED_DAYS`], uploads
/// never attached (after a day), and accounts made for an address that never
/// confirmed anything and were never used. Each is found cross-workspace and
/// removed pinned to its own workspace (accounts, which can span workspaces,
/// under the bypass context pinned to their home workspace, like the
/// soft-delete purge).
pub async fn guest_residue_cleanup(pool: Pool, search: Arc<SearchService>) -> Result<()> {
    let _lock = match try_job_lock(&pool, GUEST_RESIDUE_LOCK, "guest_residue.cleanup")? {
        Some(guard) => guard,
        None => return Ok(()),
    };
    guest_residue_sweep(&pool, Some(&search)).await
}

/// One pass of [`guest_residue_cleanup`], without the job lock.
pub async fn guest_residue_sweep(pool: &Pool, search: Option<&Arc<SearchService>>) -> Result<()> {
    use crate::repository::guest_residue;
    let pool = pool.clone();
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::days(guest_residue::UNCONFIRMED_DAYS);
    let delete_files = |workspace_id: i32, paths: Vec<String>| async move {
        if paths.is_empty() {
            return;
        }
        let storage = crate::utils::storage::WorkspaceScopedStorage::arc(
            crate::utils::storage::process_storage(),
            workspace_id,
        );
        for path in paths {
            if let Err(e) = storage.delete_file(&path).await {
                warn!(error = ?e, "scheduler:guest_residue: file delete failed ({path})");
            }
        }
    };

    let stale =
        // cross-tenant: cross-workspace scan builds the work-list; each request is deleted per-workspace below.
        crate::sync::session::background_run(&pool, "scheduler:guest_residue_scan", |conn| {
            guest_residue::stale_pending_tickets(conn, cutoff, 500)
        })
        .context("scan unconfirmed requests")?;
    let mut requests = 0usize;
    for (workspace_id, ticket_id) in stale {
        match crate::sync::session::run_in_workspace(
            &pool,
            "background:guest_residue_request",
            workspace_id,
            |conn| crate::repository::tickets::delete_ticket_with_cleanup(conn, ticket_id),
        ) {
            Ok(deleted) => {
                requests += 1;
                delete_files(workspace_id, deleted.attachment_paths).await;
            }
            Err(e) => {
                warn!(ticket_id, error = ?e, "scheduler:guest_residue: request delete failed")
            }
        }
    }

    let uploads =
        // cross-tenant: cross-workspace scan builds the work-list; each upload is deleted per-workspace below.
        crate::sync::session::background_run(&pool, "scheduler:guest_residue_uploads", |conn| {
            guest_residue::orphan_guest_uploads(conn, now - chrono::Duration::days(1), 500)
        })
        .context("scan abandoned uploads")?;
    let mut files = 0usize;
    for (workspace_id, attachment_id, url) in uploads {
        match crate::sync::session::run_in_workspace(
            &pool,
            "background:guest_residue_upload",
            workspace_id,
            |conn| crate::repository::comments::delete_attachment(conn, attachment_id),
        ) {
            Ok(_) => {
                files += 1;
                let paths = crate::repository::tickets::extract_storage_path_from_url(&url)
                    .into_iter()
                    .collect();
                delete_files(workspace_id, paths).await;
            }
            Err(e) => warn!(error = ?e, "scheduler:guest_residue: upload delete failed"),
        }
    }

    let accounts =
        // cross-tenant: accounts span workspaces; found and purged under the bypass context like the soft-delete purge.
        crate::sync::session::background_run(&pool, "scheduler:guest_residue_accounts", |conn| {
            guest_residue::never_confirmed_guests(conn, cutoff, 200)
        })
        .context("scan unconfirmed guest accounts")?;
    let mut purged = 0usize;
    let mut conn = pool.get().context("db pool")?;
    let actor = crate::sync::actor::ActorContext::system("scheduler:guest_residue_purge");
    let mut homeless = 0usize;
    for uuid in accounts {
        match purge_account_in_home_workspace(
            &mut conn,
            &actor,
            &uuid,
            search.map(|s| s as &dyn crate::repository::users::UserDeletedObserver),
        ) {
            Ok(Some(_)) => purged += 1,
            Ok(None) => homeless += 1,
            Err(e) => warn!(error = ?e, "scheduler:guest_residue: account purge failed"),
        }
    }
    warn_homeless_accounts("scheduler:guest_residue", homeless);
    info!("scheduler:guest_residue: removed {requests} unconfirmed requests, {files} abandoned uploads, {purged} unused guest accounts");
    Ok(())
}

pub fn knowledge_gap_detect_interval() -> std::time::Duration {
    let secs = std::env::var("NOSDESK_KNOWLEDGE_GAP_DETECT_SECS")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|n| *n >= 60)
        .unwrap_or(3600);
    std::time::Duration::from_secs(secs)
}

/// Find knowledge gaps in every workspace: ticket clusters, searches that
/// keep finding nothing, and stale docs that keep resolving tickets. The same
/// detectors and thresholds as the gaps page's Refresh, so the queue fills
/// without anyone pressing it. A gap someone dismissed or resolved is not
/// recreated from the same evidence (see the detectors).
pub async fn knowledge_gap_detection(pool: Pool) -> Result<()> {
    use crate::repository::knowledge_gaps as gaps;

    let _lock = match try_job_lock(&pool, KNOWLEDGE_GAP_DETECT_LOCK, "knowledge_gaps.detect")? {
        Some(guard) => guard,
        None => return Ok(()), // another machine holds it this tick
    };

    let workspaces =
        // cross-tenant: lists workspaces to detect in; detection itself runs pinned per workspace below.
        crate::sync::session::background_run(&pool, "scheduler:knowledge_gaps:list", |conn| {
            crate::repository::workspaces::list_workspaces(conn, false)
        })
        .map_err(|e| anyhow::anyhow!("list workspaces: {e}"))?;
    let workspace_ids: Vec<i32> = workspaces.into_iter().map(|w| w.id).collect();

    let (mut count, mut failed) = (0usize, 0usize);
    for workspace_id in workspace_ids {
        let ran = crate::sync::session::run_in_workspace(
            &pool,
            "scheduler:knowledge_gaps",
            workspace_id,
            |conn| {
                let clusters = gaps::run_cluster_detection(conn, None, 30, 2)?;
                let searches = gaps::run_failed_search_detection(conn, None, 30, 2)?;
                let stale = gaps::run_stale_doc_detection(conn, None, 30, 1)?;
                Ok(clusters.gaps_created + searches.gaps_created + stale.gaps_created)
            },
        );
        match ran {
            Ok(n) => count += n,
            Err(e) => {
                failed += 1;
                warn!(workspace_id, error = %e, "scheduler:knowledge_gaps: detection failed; retry next tick");
            }
        }
    }
    if count > 0 || failed > 0 {
        // `count`: gaps created this run; `failed`: workspaces that errored.
        info!(count, failed, "scheduler: knowledge gaps detected");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use diesel::r2d2;

    /// A ticket that breached, paused and resumed is still breached: the
    /// target moved later by the paused time but the clock is already past
    /// it. The stamp stays and the breach isn't notified a second time.
    #[test]
    fn a_breach_paused_and_resumed_is_notified_once() {
        use crate::models::{TicketUpdate, WorkflowStateCategory};
        use crate::repository::sla_admin::{
            create_calendar, create_policy, SlaPolicyBody, WorkingCalendarBody,
        };
        use crate::schema::{sync_actions, tickets};
        use crate::test_helpers::TestFixtures;
        use chrono::{Duration, Utc};
        use diesel::prelude::*;

        let mut conn = crate::test_helpers::setup_test_connection();
        let day = serde_json::json!([["00:00", "23:59"]]);
        let calendar = create_calendar(
            &mut conn,
            WorkingCalendarBody {
                name: "Always open".into(),
                timezone: None,
                schedule: serde_json::json!({
                    "mon": day, "tue": day, "wed": day, "thu": day,
                    "fri": day, "sat": day, "sun": day,
                }),
                is_default: Some(false),
            },
            None,
        )
        .unwrap();
        create_policy(
            &mut conn,
            SlaPolicyBody {
                name: "Activated clock".into(),
                target_response_minutes: Some(60),
                target_resolution_minutes: None,
                working_calendar_id: Some(calendar.id),
                priority_filter: None,
                category_id_filter: None,
                assignee_group_id_filter: None,
                is_default: Some(false),
                no_sla: Some(false),
                clock_start: Some("activated".into()),
            },
            None,
        )
        .unwrap();
        let state = |conn: &mut crate::db::DbConnection, category| {
            crate::repository::workflow_states::first_in_category(conn, category)
                .unwrap()
                .id
        };
        let active = state(&mut conn, WorkflowStateCategory::Active);
        let backlog = state(&mut conn, WorkflowStateCategory::Backlog);
        let user = TestFixtures::create_user(&mut conn, "sla_pause_resume", "user");
        let ticket =
            TestFixtures::create_ticket(&mut conn, "Pause after breach", Some(user.uuid), None);

        // Running for three hours: the response target passed two hours ago.
        let now = Utc::now().naive_utc();
        let ticket: crate::models::Ticket = diesel::update(tickets::table.find(ticket.id))
            .set((
                tickets::workflow_state_id.eq(active),
                tickets::created_at.eq(now - Duration::hours(3)),
                tickets::sla_clock_started_at.eq(Some(now - Duration::hours(3))),
            ))
            .get_result(&mut conn)
            .unwrap();
        crate::services::sla::recompute_and_stamp_sla_for_ticket(&mut conn, &ticket);
        assert!(
            process_one_breach(&mut conn, ticket.id, SlaBreachKind::Response, 1)
                .unwrap()
                .is_some()
        );
        // The sweep caught it a minute after the target. Postgres keeps
        // microseconds, so compare at that precision.
        let stamp =
            chrono::SubsecRound::trunc_subsecs(now - Duration::hours(2) + Duration::minutes(1), 6);
        diesel::update(tickets::table.find(ticket.id))
            .set(tickets::sla_response_breached_at.eq(Some(stamp)))
            .execute(&mut conn)
            .unwrap();

        // Paused for the last half hour, then resumed now.
        let move_to = |conn: &mut crate::db::DbConnection, state_id| {
            crate::repository::tickets::update_ticket_partial(
                conn,
                ticket.id,
                TicketUpdate {
                    workflow_state_id: Some(state_id),
                    ..Default::default()
                },
                None,
            )
            .unwrap()
        };
        move_to(&mut conn, backlog);
        diesel::update(tickets::table.find(ticket.id))
            .set(tickets::sla_paused_at.eq(Some(now - Duration::minutes(30))))
            .execute(&mut conn)
            .unwrap();
        move_to(&mut conn, active);

        let breached_at: Option<chrono::NaiveDateTime> = tickets::table
            .find(ticket.id)
            .select(tickets::sla_response_breached_at)
            .first(&mut conn)
            .unwrap();
        assert_eq!(breached_at, Some(stamp), "the breach stamp stays");
        assert!(
            process_one_breach(&mut conn, ticket.id, SlaBreachKind::Response, 1)
                .unwrap()
                .is_none(),
            "no second breach"
        );
        let emitted: i64 = sync_actions::table
            .filter(sync_actions::event_type.eq("ticket.sla_breached"))
            .filter(sync_actions::aggregate_id.eq(ticket.id.to_string()))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(emitted, 1, "one breach, one notification");
    }

    #[test]
    fn a_finished_ticket_never_breaches() {
        use crate::models::WorkflowStateCategory;
        use crate::schema::{sync_actions, tickets};
        use crate::test_helpers::TestFixtures;
        use diesel::prelude::*;

        let mut conn = crate::test_helpers::setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sla_finished", "user");
        let ticket = TestFixtures::create_ticket(&mut conn, "Merged away", Some(user.uuid), None);
        let merged = crate::repository::workflow_states::first_in_category(
            &mut conn,
            WorkflowStateCategory::Merged,
        )
        .unwrap();
        // A target left over from before the merge, already past.
        let past = chrono::Utc::now().naive_utc() - chrono::Duration::hours(1);
        diesel::update(tickets::table.find(ticket.id))
            .set((
                tickets::workflow_state_id.eq(merged.id),
                tickets::assignee_uuid.eq(Some(user.uuid)),
                tickets::sla_resolution_target_at.eq(Some(past)),
            ))
            .execute(&mut conn)
            .unwrap();

        let outcome =
            process_one_breach(&mut conn, ticket.id, SlaBreachKind::Resolution, 1).unwrap();
        assert!(outcome.is_none(), "nobody is told a merged ticket breached");

        let (breached, target): (Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>) =
            tickets::table
                .find(ticket.id)
                .select((
                    tickets::sla_resolution_breached_at,
                    tickets::sla_resolution_target_at,
                ))
                .first(&mut conn)
                .unwrap();
        assert_eq!((breached, target), (None, None));
        let emitted: i64 = sync_actions::table
            .filter(sync_actions::event_type.eq("ticket.sla_breached"))
            .filter(sync_actions::aggregate_id.eq(ticket.id.to_string()))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(emitted, 0, "no breach event, so no webhook");
    }

    fn breach(
        ticket_id: i32,
        workspace_id: i32,
        assignee: uuid::Uuid,
        secs_overdue: i64,
    ) -> BreachContext {
        BreachContext {
            ticket_id,
            // Distinct from the id, so a body that quotes the id fails.
            ticket_number: ticket_id + 1000,
            ticket_title: format!("Ticket {ticket_id}"),
            workspace_id,
            kind: SlaBreachKind::Response,
            breached_at: chrono::Utc::now() - chrono::Duration::seconds(secs_overdue),
            assignee_uuid: Some(assignee),
            watcher_uuids: vec![],
            admin_uuids: vec![],
        }
    }

    #[test]
    fn coalesce_tells_the_admins_only_when_no_one_else_is_told() {
        let (agent, admin_a, admin_b) = (
            uuid::Uuid::now_v7(),
            uuid::Uuid::now_v7(),
            uuid::Uuid::now_v7(),
        );
        let owned = BreachContext {
            admin_uuids: vec![admin_a],
            ..breach(1, 1, agent, 60)
        };
        let unowned = BreachContext {
            assignee_uuid: None,
            admin_uuids: vec![admin_a, admin_b],
            ..breach(2, 1, agent, 60)
        };
        let notices = coalesce_breaches(&[owned, unowned]);
        let mut told: Vec<(uuid::Uuid, i32, bool)> = notices
            .iter()
            .map(|n| (n.recipient, n.ticket_id, n.as_admin))
            .collect();
        told.sort();
        let mut expected = vec![(agent, 1, false), (admin_a, 2, true), (admin_b, 2, true)];
        expected.sort();
        assert_eq!(told, expected);
    }

    #[test]
    fn coalesce_single_breach_is_per_ticket_body() {
        let user = uuid::Uuid::now_v7();
        let notices = coalesce_breaches(&[breach(7, 1, user, 60)]);
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].ticket_id, 7);
        assert!(
            notices[0].body.starts_with("Response SLA on #1007"),
            "single breach uses the per-ticket body, got: {}",
            notices[0].body
        );
    }

    #[test]
    fn coalesce_multiple_breaches_summarise_and_link_most_overdue() {
        let user = uuid::Uuid::now_v7();
        // #10 is the most overdue (largest secs_overdue → earliest breached_at).
        let notices = coalesce_breaches(&[
            breach(20, 1, user, 100),
            breach(10, 1, user, 300),
            breach(30, 1, user, 50),
        ]);
        assert_eq!(
            notices.len(),
            1,
            "three breaches for one recipient collapse"
        );
        let n = &notices[0];
        assert!(
            n.body
                .starts_with("3 tickets breached their SLA: #1020, #1010, #1030"),
            "got: {}",
            n.body
        );
        assert_eq!(n.ticket_id, 10, "links to the most-overdue ticket");
    }

    #[test]
    fn coalesce_does_not_merge_across_workspaces() {
        let user = uuid::Uuid::now_v7();
        let notices = coalesce_breaches(&[breach(1, 1, user, 60), breach(2, 2, user, 60)]);
        assert_eq!(
            notices.len(),
            2,
            "the same user's breaches in different workspaces stay separate"
        );
        assert!(notices
            .iter()
            .all(|n| n.body.starts_with("Response SLA on #")));
    }

    #[test]
    fn coalesce_summary_caps_the_id_list_with_more() {
        let user = uuid::Uuid::now_v7();
        let breaches: Vec<BreachContext> =
            (1..=5).map(|i| breach(i, 1, user, 60 + i as i64)).collect();
        let notices = coalesce_breaches(&breaches);
        assert_eq!(notices.len(), 1);
        assert!(
            notices[0].body.contains("and 2 more"),
            "5 tickets shows 3 ids + \"and 2 more\", got: {}",
            notices[0].body
        );
    }

    // A real 2-connection pool (no test-transaction wrapper) so two
    // sessions can be held at once to observe advisory-lock contention.
    fn real_pool() -> Pool {
        let url = std::env::var("TEST_DATABASE_URL")
            .expect("TEST_DATABASE_URL must be set for the advisory-lock test");
        let manager = crate::db::ResettingManager::new(url);
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
