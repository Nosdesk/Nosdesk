//! Background workers: the periodic-task scheduler.
//!
//! Extracted from main() so the composition root stays thin. The four event listeners
//! (sync_outbox / email_queue / search_replicator / channel supervisor) remain
//! in main() for now: they're interleaved with the state they capture and move
//! out with Phase 4 (state).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use actix_web::web;
use tracing::info;

use crate::db::Pool;
use crate::services::notifications::NotificationService;
use crate::services::scheduler::StatusRegistry;
use crate::services::search::SearchService;

/// A periodic job: its name (the status registry key), how often it runs,
/// and one run of it.
pub struct ScheduledJob {
    pub name: &'static str,
    pub every: Duration,
    pub run: JobFn,
}

/// One run of a scheduled job.
pub type JobFn =
    Box<dyn FnMut() -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send>;

impl ScheduledJob {
    fn new<F, Fut>(name: &'static str, every: Duration, mut run: F) -> Self
    where
        F: FnMut() -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        Self {
            name,
            every,
            run: Box::new(move || Box::pin(run())),
        }
    }
}

/// Boot the periodic-task scheduler: create the status registry, spawn every
/// job in [`scheduled_jobs`] on the shared `scheduler_shutdown` token, and
/// return the status registry (published as app data). The token is created
/// by the caller and shared with the state-bound background listeners so one
/// cancel stops them all.
pub fn spawn_scheduled_jobs(
    pool: Pool,
    search_service: web::Data<Arc<SearchService>>,
    notification_service: web::Data<NotificationService>,
    scheduler_shutdown: tokio_util::sync::CancellationToken,
    frontend_url: String,
) -> StatusRegistry {
    let scheduler_status = crate::services::scheduler::status_registry();
    for job in scheduled_jobs(
        pool,
        search_service.get_ref().clone(),
        notification_service.into_inner(),
        frontend_url,
    ) {
        crate::services::scheduler::spawn_periodic(
            job.name,
            job.every,
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            job.run,
        );
    }
    info!("scheduler: periodic jobs spawned");
    scheduler_status
}

/// Every periodic job the scheduler runs. The one list: the scheduler spawns
/// from it and the job-pool guard test runs each entry once.
pub fn scheduled_jobs(
    pool: Pool,
    search_service: Arc<SearchService>,
    notification_service: Arc<NotificationService>,
    frontend_url: String,
) -> Vec<ScheduledJob> {
    use crate::services::scheduled_jobs as jobs;
    let mut registry = Vec::new();
    // Hourly: prune expired auth sessions + refresh tokens so the
    // tables don't accrete dead rows indefinitely.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "active_sessions.cleanup",
        Duration::from_secs(60 * 60),
        move || jobs::cleanup_expired_sessions(p.clone()),
    ));
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "refresh_tokens.cleanup",
        Duration::from_secs(60 * 60),
        move || jobs::cleanup_expired_refresh_tokens(p.clone()),
    ));

    // Hourly: fail stranded self-serve workspace exports and delete expired
    // export artifacts (storage file + row) past their download window.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "workspace_exports.cleanup",
        Duration::from_secs(60 * 60),
        move || jobs::cleanup_expired_workspace_exports(p.clone()),
    ));

    // Daily: send notification email digests (batches the notifications a
    // user set to email=digest into one summary). Single-machine via lock.
    let p = pool.clone();
    let base_url = frontend_url.clone();
    registry.push(ScheduledJob::new(
        "notifications.digest",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::send_notification_digests(p.clone(), base_url.clone()),
    ));

    // Daily: auto-archive stale notifications so the bell/inbox self-prunes
    // (read older than 30d, or anything older than 90d). archived_at is the
    // reversible archive axis, so nothing is lost — the user can still find
    // them under the archived filter.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "notifications.auto_archive",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::auto_archive_stale_notifications(p.clone()),
    ));

    // Every 30 min: Microsoft Graph delta sync (skipped at runtime
    // when the provider isn't configured).
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "msgraph.delta_sync",
        Duration::from_secs(30 * 60),
        move || jobs::msgraph_delta_sync(p.clone()),
    ));

    // Daily: LDAP full reconcile (resets the DirSync cursor + re-snapshots
    // the directory to catch drift the incremental stream missed). Skipped
    // at runtime when LDAP isn't enabled.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "ldap.nightly_reconcile",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::ldap_nightly_reconcile(p.clone()),
    ));

    // Daily: roll the sync_actions / audit_log monthly partitions
    // forward. Inserts after the last provisioned month would
    // otherwise fail; the substrate migration provides the first
    // four months and this job extends the window.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "sync.partition_provisioner",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::ensure_sync_partitions(p.clone()),
    ));

    // Daily: prune CSP violation reports past the retention
    // window so a noisy reporter (browser extension etc.) can't
    // grow the table unbounded. Retention defaults to 30 days.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "csp_reports.prune",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::prune_csp_reports(p.clone()),
    ));

    // Hourly: prune Idempotency-Key cache rows past the retention
    // horizon (default 24h). M5 provisioning retries either
    // succeed in minutes or escalate to ops; old keys serve no
    // purpose and shouldn't accumulate. Hourly instead of daily
    // because the table is small and the sweep is cheap.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "idempotency_keys.prune",
        Duration::from_secs(60 * 60),
        move || jobs::prune_idempotency_keys(p.clone()),
    ));

    // Every 60s: sweep expired leases on the outbound email queue.
    // A worker that crashed mid-send leaves a row in `sending` with
    // a 5-minute lease; the sweep moves expired-lease rows back to
    // `failed` so the next claim cycle picks them up. Cheap (the
    // partial outbound_emails_lease_idx keeps the scan tiny).
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "outbound_emails.sweep_leases",
        Duration::from_secs(60),
        move || jobs::sweep_outbound_email_leases(p.clone()),
    ));

    // Hourly: re-verify workspace DKIM sending domains. A `verified`
    // domain whose published record disappears flips back to `pending`
    // so sends fall back to the platform identity instead of shipping
    // mail that fails DKIM/DMARC at the receiver.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "dkim.reverify_domains",
        Duration::from_secs(60 * 60),
        move || jobs::reverify_dkim_domains(p.clone()),
    ));

    // Daily: row-level retention for security_events and
    // webhook_deliveries; partition-level retention for audit_log
    // and sync_actions. Each expired partition is dropped in a short
    // transaction under a lock timeout (DETACH CONCURRENTLY is refused
    // while the parent has a default partition).
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "security_events.prune",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::prune_security_events(p.clone()),
    ));
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "webhook_deliveries.prune",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::prune_webhook_deliveries(p.clone()),
    ));
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "audit_log.drop_old_partitions",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::prune_audit_log_partitions(p.clone()),
    ));
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "sync_actions.drop_old_partitions",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::prune_sync_actions_partitions(p.clone()),
    ));

    // Daily: hard-delete soft-deleted users past the retention
    // window. The cascade in repository::users::purge_user is
    // destructive (comments / tickets get NULLed or reassigned)
    // so the grace window (default 30 days, set via
    // NOSDESK_USER_PURGE_GRACE_DAYS) is the operator-facing
    // safety net. The worker re-tries failed rows on the next
    // tick rather than aborting the sweep.
    let p = pool.clone();
    let s = search_service.clone();
    registry.push(ScheduledJob::new(
        "users.purge_soft_deleted",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::purge_soft_deleted_users(p.clone(), s.clone()),
    ));

    // Daily: hard-delete workspaces whose archive grace window
    // (default 30 days, `WORKSPACE_HARD_DELETE_GRACE_DAYS` to
    // override) has elapsed. Mirrors purge_soft_deleted_users;
    // BYPASSRLS role for the cross-tenant cascade.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "workspaces.purge_archived",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::purge_archived_workspaces(p.clone()),
    ));

    // Hourly: remove the stored files of hard-deleted workspaces, which
    // the database cascade can't reach. The hard delete queues them; a
    // failed purge is retried on the next run.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "workspaces.purge_files",
        Duration::from_secs(60 * 60),
        move || jobs::purge_deleted_workspace_files(p.clone()),
    ));

    // Daily: backfill avatar thumbnails missing on disk or unset in
    // the DB. Restores rebuild thumbnails eagerly (they're not in the
    // backup payload); this is the idempotent safety net that heals
    // any later drift and does no work in steady state.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "users.backfill_thumbnails",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::backfill_user_thumbnails(p.clone()),
    ));

    // Hourly: make the email copies of logos that have none (uploaded
    // before copies existed, or restored without one). One small query
    // once every logo has its copy.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "branding.email_logo_copies",
        Duration::from_secs(60 * 60),
        move || jobs::make_email_logo_copies(p.clone()),
    ));

    // Every 60s: detect SLA breaches and flip the pill live. Scans
    // the materialised `sla_response_target_at` /
    // `sla_resolution_target_at` columns (cheap partial indexes),
    // atomically stamps `*_breached_at`, emits a ticket.sla_updated
    // sync_action (pill repaint) plus a ticket.sla_breached
    // sync_action (webhook delivery via the outbox), and notifies the
    // assignee + watchers via NotificationService.
    let p = pool.clone();
    let ns = notification_service.clone();
    registry.push(ScheduledJob::new(
        "sla.detect_breaches",
        Duration::from_secs(60),
        move || jobs::detect_sla_breaches(p.clone(), ns.clone()),
    ));

    // Daily: remind borrowers about device loans due back soon or
    // overdue, via NotificationService. Advisory-locked; scans all
    // workspaces under BYPASSRLS and stamps each loan so a reminder
    // fires once.
    let p = pool.clone();
    let ns = notification_service.clone();
    registry.push(ScheduledJob::new(
        "asset_loans.due_reminders",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::loan_due_reminders(p.clone(), ns.clone()),
    ));

    // Hourly: detect knowledge gaps in every workspace (clusters, failed
    // searches, stale docs), so the gaps queue fills by itself.
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "knowledge_gaps.detect",
        jobs::knowledge_gap_detect_interval(),
        move || jobs::knowledge_gap_detection(p.clone()),
    ));

    // Daily: remove never-confirmed requests, abandoned uploads and
    // never-used guest accounts left by the public request form.
    let p = pool.clone();
    let s = search_service.clone();
    registry.push(ScheduledJob::new(
        "guest_residue.cleanup",
        Duration::from_secs(24 * 60 * 60),
        move || jobs::guest_residue_cleanup(p.clone(), s.clone()),
    ));

    // Hourly: approve requests nobody answered within their workspace's
    // automatic-approval period (a no-op unless one is set).
    let p = pool.clone();
    registry.push(ScheduledJob::new(
        "approvals.timeouts",
        Duration::from_secs(60 * 60),
        move || jobs::approval_timeouts(p.clone()),
    ));
    registry
}
