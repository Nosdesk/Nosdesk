//! Background workers: the periodic-task scheduler.
//!
//! Extracted from main() so the composition root stays thin. The four event listeners
//! (sync_outbox / email_queue / search_replicator / channel supervisor) remain
//! in main() for now: they're interleaved with the state they capture and move
//! out with Phase 4 (state).

use std::sync::Arc;

use actix_web::web;
use tracing::info;

use crate::db::Pool;
use crate::services::notifications::NotificationService;
use crate::services::scheduler::StatusRegistry;
use crate::services::search::SearchService;

/// Boot the periodic-task scheduler: create the shutdown token + status
/// registry, spawn every periodic job on the shared `scheduler_shutdown` token,
/// and return the status registry (published as app data). The token is created
/// by the caller and shared with the state-bound background listeners so one
/// cancel stops them all.
pub fn spawn_scheduled_jobs(
    pool: Pool,
    search_service: web::Data<Arc<SearchService>>,
    notification_service: web::Data<NotificationService>,
    scheduler_shutdown: tokio_util::sync::CancellationToken,
) -> StatusRegistry {
    let scheduler_status = crate::services::scheduler::status_registry();
    {
        use crate::services::scheduled_jobs as jobs;
        use crate::services::scheduler::spawn_periodic;
        use std::time::Duration;

        // Hourly: prune expired auth sessions + refresh tokens so the
        // tables don't accrete dead rows indefinitely.
        let p = pool.clone();
        spawn_periodic(
            "active_sessions.cleanup",
            Duration::from_secs(60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::cleanup_expired_sessions(p.clone()),
        );
        let p = pool.clone();
        spawn_periodic(
            "refresh_tokens.cleanup",
            Duration::from_secs(60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::cleanup_expired_refresh_tokens(p.clone()),
        );

        // Every 30 min: Microsoft Graph delta sync (skipped at runtime
        // when the provider isn't configured).
        let p = pool.clone();
        spawn_periodic(
            "msgraph.delta_sync",
            Duration::from_secs(30 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::msgraph_delta_sync(p.clone()),
        );

        // Daily: LDAP full reconcile (resets the DirSync cursor + re-snapshots
        // the directory to catch drift the incremental stream missed). Skipped
        // at runtime when LDAP isn't enabled.
        let p = pool.clone();
        spawn_periodic(
            "ldap.nightly_reconcile",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::ldap_nightly_reconcile(p.clone()),
        );

        // Daily: roll the sync_actions / audit_log monthly partitions
        // forward. Inserts after the last provisioned month would
        // otherwise fail; the substrate migration provides the first
        // four months and this job extends the window.
        let p = pool.clone();
        spawn_periodic(
            "sync.partition_provisioner",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::ensure_sync_partitions(p.clone()),
        );

        // Daily: prune CSP violation reports past the retention
        // window so a noisy reporter (browser extension etc.) can't
        // grow the table unbounded. Retention defaults to 30 days.
        let p = pool.clone();
        spawn_periodic(
            "csp_reports.prune",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_csp_reports(p.clone()),
        );

        // Hourly: prune Idempotency-Key cache rows past the retention
        // horizon (default 24h). M5 provisioning retries either
        // succeed in minutes or escalate to ops; old keys serve no
        // purpose and shouldn't accumulate. Hourly instead of daily
        // because the table is small and the sweep is cheap.
        let p = pool.clone();
        spawn_periodic(
            "idempotency_keys.prune",
            Duration::from_secs(60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_idempotency_keys(p.clone()),
        );

        // Every 60s: sweep expired leases on the outbound email queue.
        // A worker that crashed mid-send leaves a row in `sending` with
        // a 5-minute lease; the sweep moves expired-lease rows back to
        // `failed` so the next claim cycle picks them up. Cheap (the
        // partial outbound_emails_lease_idx keeps the scan tiny).
        let p = pool.clone();
        spawn_periodic(
            "outbound_emails.sweep_leases",
            Duration::from_secs(60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::sweep_outbound_email_leases(p.clone()),
        );

        // Hourly: re-verify workspace DKIM sending domains. A `verified`
        // domain whose published record disappears flips back to `pending`
        // so sends fall back to the platform identity instead of shipping
        // mail that fails DKIM/DMARC at the receiver.
        let p = pool.clone();
        spawn_periodic(
            "dkim.reverify_domains",
            Duration::from_secs(60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::reverify_dkim_domains(p.clone()),
        );

        // Daily: row-level retention for security_events and
        // webhook_deliveries; partition-level retention for audit_log
        // and sync_actions. Partition drops use DETACH CONCURRENTLY so
        // the parent's lock window stays at SHARE UPDATE EXCLUSIVE
        // (W6a's lock-friendly attach in reverse).
        let p = pool.clone();
        spawn_periodic(
            "security_events.prune",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_security_events(p.clone()),
        );
        let p = pool.clone();
        spawn_periodic(
            "webhook_deliveries.prune",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_webhook_deliveries(p.clone()),
        );
        let p = pool.clone();
        spawn_periodic(
            "audit_log.drop_old_partitions",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_audit_log_partitions(p.clone()),
        );
        let p = pool.clone();
        spawn_periodic(
            "sync_actions.drop_old_partitions",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::prune_sync_actions_partitions(p.clone()),
        );

        // Daily: hard-delete soft-deleted users past the retention
        // window. The cascade in repository::users::purge_user is
        // destructive (comments / tickets get NULLed or reassigned)
        // so the grace window (default 30 days, set via
        // NOSDESK_USER_PURGE_GRACE_DAYS) is the operator-facing
        // safety net. The worker re-tries failed rows on the next
        // tick rather than aborting the sweep.
        let p = pool.clone();
        let s = search_service.get_ref().clone();
        spawn_periodic(
            "users.purge_soft_deleted",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::purge_soft_deleted_users(p.clone(), s.clone()),
        );

        // Daily: hard-delete workspaces whose archive grace window
        // (default 30 days, `WORKSPACE_HARD_DELETE_GRACE_DAYS` to
        // override) has elapsed. Mirrors purge_soft_deleted_users;
        // BYPASSRLS role for the cross-tenant cascade.
        let p = pool.clone();
        spawn_periodic(
            "workspaces.purge_archived",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::purge_archived_workspaces(p.clone()),
        );

        // Daily: backfill avatar thumbnails missing on disk or unset in
        // the DB. Restores rebuild thumbnails eagerly (they're not in the
        // backup payload); this is the idempotent safety net that heals
        // any later drift and does no work in steady state.
        let p = pool.clone();
        spawn_periodic(
            "users.backfill_thumbnails",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::backfill_user_thumbnails(p.clone()),
        );

        // Every 60s: detect SLA breaches and flip the pill live. Scans
        // the materialised `sla_response_target_at` /
        // `sla_resolution_target_at` columns (cheap partial indexes),
        // atomically stamps `*_breached_at`, emits a ticket.sla_updated
        // sync_action (pill repaint) plus a ticket.sla_breached
        // sync_action (webhook delivery via the outbox), and notifies the
        // assignee + watchers via NotificationService.
        let p = pool.clone();
        let ns = notification_service.clone().into_inner();
        spawn_periodic(
            "sla.detect_breaches",
            Duration::from_secs(60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::detect_sla_breaches(p.clone(), ns.clone()),
        );

        // Daily: remind borrowers about device loans due back soon or
        // overdue, via NotificationService. Advisory-locked; scans all
        // workspaces under BYPASSRLS and stamps each loan so a reminder
        // fires once.
        let p = pool.clone();
        let ns = notification_service.clone().into_inner();
        spawn_periodic(
            "asset_loans.due_reminders",
            Duration::from_secs(24 * 60 * 60),
            scheduler_shutdown.clone(),
            scheduler_status.clone(),
            move || jobs::loan_due_reminders(p.clone(), ns.clone()),
        );

        info!("scheduler: periodic jobs spawned");
    }
    scheduler_status
}
