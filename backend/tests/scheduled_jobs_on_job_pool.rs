//! Every scheduled job runs cleanly on the pool production runs it on.
//!
//! The scheduler's registry (`backend::workers::scheduled_jobs`) is the job
//! list, so a new job is run here as soon as it is registered, and fails the
//! test until it is listed below as seeded or not exercised. Each job runs
//! once on `TestDb::job_pool` (the `nosdesk_app` role, no workspace pinned,
//! every checkout scrubbed), against data that gives it real work. A job must
//! neither return an error nor log a warning or error: several jobs catch a
//! failed row and only log it, so the log is where a silent failure shows.
//!
//! Its own binary: the partition jobs run their DDL over
//! `MIGRATION_DATABASE_URL`, as production's do, and setting that for one
//! test's database is only safe in a process of its own.

#![allow(clippy::expect_used)]

mod common;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use diesel::prelude::*;
use diesel::sql_types::{Integer, Uuid as SqlUuid};
use tokio::sync::RwLock;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;
use uuid::Uuid;

use backend::models::{NewNotification, NewTicket, NewWebhookDelivery};
use backend::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use backend::services::notifications::NotificationService;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

type Conn = backend::db::DbConnection;

/// Jobs this test seeds work for.
const SEEDED: &[&str] = &[
    "active_sessions.cleanup",
    "refresh_tokens.cleanup",
    "workspace_exports.cleanup",
    "notifications.digest",
    "notifications.auto_archive",
    "sync.partition_provisioner",
    "csp_reports.prune",
    "idempotency_keys.prune",
    "outbound_emails.sweep_leases",
    "security_events.prune",
    "webhook_deliveries.prune",
    "audit_log.drop_old_partitions",
    "users.purge_soft_deleted",
    "workspaces.purge_archived",
    "workspaces.purge_files",
    "sla.detect_breaches",
    "asset_loans.due_reminders",
    "guest_residue.cleanup",
];

/// Jobs that still run here, but with nothing to do, and why.
const NOT_EXERCISED: &[(&str, &str)] = &[
    ("msgraph.delta_sync", "needs a Microsoft Graph tenant"),
    ("ldap.nightly_reconcile", "needs an LDAP directory"),
    ("dkim.reverify_domains", "needs DNS for a sending domain"),
    (
        "sync_actions.drop_old_partitions",
        "keeps everything unless SYNC_ACTIONS_RETENTION_DAYS is set; tests/it/partition_prune.rs covers the drop",
    ),
    (
        "users.backfill_thumbnails",
        "needs avatar image files in storage",
    ),
    (
        "branding.email_logo_copies",
        "needs logo image files in storage",
    ),
    (
        "knowledge_gaps.detect",
        "needs ticket clusters and search history to find a gap",
    ),
    (
        "approvals.timeouts",
        "needs a workspace approval period and a pending approval; tests/it covers the approval flow",
    ),
];

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<String>>>);

struct Render<'a>(&'a mut String);
impl Visit for Render<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!(" {}={value:?}", field.name()));
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for Capture {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() <= Level::WARN {
            let mut line = format!(
                "{} {}:",
                event.metadata().level(),
                event.metadata().target()
            );
            event.record(&mut Render(&mut line));
            self.0.lock().expect("capture").push(line);
        }
    }
}

fn pinned<T>(conn: &mut Conn, ws: i32, f: impl FnOnce(&mut Conn) -> QueryResult<T>) -> T {
    let actor = ActorContext::system("test:job_pool_seed").with_workspace(ws);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, f).expect("seed")
}

fn exec(conn: &mut Conn, sql: &str) {
    diesel::sql_query(sql)
        .execute(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn count(conn: &mut Conn, sql: &str) -> i64 {
    #[derive(QueryableByName)]
    struct N {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    diesel::sql_query(sql)
        .get_result::<N>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// What the seed made, for checking the jobs did their work.
struct Seeded {
    guest: Uuid,
    guest_request: i32,
    soft_deleted: Uuid,
    old_delivery: i32,
    archived_workspace: i32,
    overdue_ticket: i32,
    loan: i32,
}

fn seed(conn: &mut Conn) -> Seeded {
    let ws = common::seed_two_workspaces(conn);
    let (a, b) = (ws.a.workspace_id, ws.b.workspace_id);

    // Sessions, refresh tokens, idempotency keys and security events past
    // their windows.
    exec(
        conn,
        &format!(
            "INSERT INTO active_sessions (user_uuid, created_at, expires_at) \
             VALUES ('{}', now() - interval '2 days', now() - interval '1 day')",
            ws.a.admin_uuid
        ),
    );
    exec(
        conn,
        &format!(
            "INSERT INTO refresh_tokens (token_hash, user_uuid, expires_at) \
             VALUES ('expired-token-hash', '{}', now() - interval '1 day')",
            ws.a.admin_uuid
        ),
    );
    exec(
        conn,
        "INSERT INTO idempotency_keys (key, response_body, response_status, created_at) \
         VALUES ('old-key', '{}'::jsonb, 200, now() - interval '3 days')",
    );
    exec(
        conn,
        "INSERT INTO security_events (event_type, created_at) \
         VALUES ('login', now() - interval '400 days')",
    );

    // Workspace A: a stale export, an old CSP report, an expired send lease,
    // an old read notification, a digest-due notification, an old webhook
    // delivery, an overdue SLA ticket and an overdue loan.
    pinned(conn, a, |c| {
        diesel::sql_query(
            "INSERT INTO workspace_export_jobs (workspace_id, status, created_at) \
             VALUES ($1, 'processing', now() - interval '2 hours')",
        )
        .bind::<Integer, _>(a)
        .execute(c)?;
        diesel::sql_query(
            "INSERT INTO csp_reports (dedup_hash, effective_directive, document_uri, disposition, \
                                      first_seen_at, last_seen_at) \
             VALUES (repeat('a', 64), 'script-src', 'https://desk.test/', 'enforce', \
                     now() - interval '60 days', now() - interval '60 days')",
        )
        .execute(c)?;
        diesel::sql_query(
            "INSERT INTO outbound_emails (recipient, subject, body_text, message_id, status, \
                                          lease_token, lease_expires_at) \
             VALUES ('someone@example.test', 'Hi', 'Hi', '<lease@desk.test>', 'sending', \
                     gen_random_uuid(), now() - interval '1 hour')",
        )
        .execute(c)
    });
    let type_id: i32 = {
        use backend::schema::notification_types;
        notification_types::table
            .select(notification_types::id)
            .order(notification_types::id)
            .first(conn)
            .expect("a notification type")
    };
    let notification = |title: &str| NewNotification {
        uuid: Uuid::now_v7(),
        user_uuid: ws.a.member_uuid,
        notification_type_id: type_id,
        entity_type: "ticket".to_string(),
        entity_id: 1,
        title: title.to_string(),
        body: None,
        metadata: None,
        channels_delivered: serde_json::json!([]),
        interrupts: true,
        source_sync_id: None,
    };
    {
        use backend::schema::{notification_preferences, notifications, user_emails};
        diesel::insert_into(user_emails::table)
            .values((
                user_emails::user_uuid.eq(ws.a.member_uuid),
                user_emails::email.eq("member.a@example.test"),
                user_emails::email_type.eq("personal"),
                user_emails::is_primary.eq(true),
                user_emails::is_verified.eq(true),
            ))
            .execute(conn)
            .expect("member email");
        diesel::insert_into(notification_preferences::table)
            .values((
                notification_preferences::user_uuid.eq(ws.a.member_uuid),
                notification_preferences::notification_type_id.eq(type_id),
                notification_preferences::channel.eq("email"),
                notification_preferences::enabled.eq(true),
                notification_preferences::frequency.eq(Some("digest")),
                notification_preferences::workspace_id.eq(a),
            ))
            .execute(conn)
            .expect("digest preference");
        let old = notification("Old and read");
        let due = notification("Waiting for the digest");
        pinned(conn, a, |c| {
            let old_id: i32 = diesel::insert_into(notifications::table)
                .values(old)
                .returning(notifications::id)
                .get_result(c)?;
            diesel::sql_query(
                "UPDATE notifications SET is_read = true, created_at = now() - interval '60 days' \
                 WHERE id = $1",
            )
            .bind::<Integer, _>(old_id)
            .execute(c)?;
            diesel::insert_into(notifications::table)
                .values(due)
                .execute(c)
        });
    }
    let old_delivery = pinned(conn, a, |c| {
        let d = backend::repository::webhooks::create_delivery(
            c,
            NewWebhookDelivery {
                webhook_id: ws.a.webhook_id,
                event_type: common::FIXTURE_WEBHOOK_EVENT.to_string(),
                payload: serde_json::json!({ "id": 1 }),
                request_headers: None,
                attempt_number: 1,
                next_retry_at: None,
            },
        )?;
        diesel::sql_query(
            "UPDATE webhook_deliveries SET created_at = now() - interval '45 days' WHERE id = $1",
        )
        .bind::<Integer, _>(d.id)
        .execute(c)?;
        Ok(d.id)
    });
    let overdue_ticket = pinned(conn, a, |c| {
        use backend::schema::tickets;
        let state = backend::repository::workflow_states::default_state(c)?;
        let id: i32 = diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer on fire".into(),
                workflow_state_id: state.id,
                assignee_uuid: Some(ws.a.admin_uuid),
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)?;
        diesel::update(tickets::table.find(id))
            .set(tickets::sla_response_target_at.eq(Some(
                chrono::Utc::now().naive_utc() - chrono::Duration::hours(1),
            )))
            .execute(c)?;
        Ok(id)
    });
    let loan = pinned(conn, a, |c| {
        let asset = diesel::sql_query(
            "INSERT INTO assets (name, kind) VALUES ('Loaner laptop', 'generic') RETURNING id",
        )
        .get_result::<Id>(c)?
        .id;
        Ok(diesel::sql_query(
            "INSERT INTO asset_loans (asset_id, borrower_user_uuid, due_back, status_before) \
             VALUES ($1, $2, current_date - 1, 'available') RETURNING id",
        )
        .bind::<Integer, _>(asset)
        .bind::<SqlUuid, _>(ws.a.member_uuid)
        .get_result::<Id>(c)?
        .id)
    });

    // Workspace B: an unconfirmed request, its unused guest account and an
    // abandoned upload, all past their windows; and a member soft-deleted
    // long ago.
    let guest = pinned(conn, b, |c| {
        match find_or_create_guest_user("abandoned@jobpool.test", "Guest", c, None)? {
            GuestUserResult::Created(u) => Ok(u.uuid),
            _ => panic!("expected a new guest"),
        }
    });
    let guest_request = pinned(conn, b, |c| {
        use backend::schema::tickets;
        let state = backend::repository::workflow_states::default_state(c)?;
        let id: i32 = diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer".into(),
                workflow_state_id: state.id,
                requester_uuid: Some(guest),
                verification_state: Some("pending".to_string()),
                ..Default::default()
            })
            .returning(tickets::id)
            .get_result(c)?;
        diesel::sql_query(
            "UPDATE tickets SET created_at = now() - interval '15 days' WHERE id = $1",
        )
        .bind::<Integer, _>(id)
        .execute(c)?;
        diesel::sql_query(
            "UPDATE users SET created_at = now() - interval '15 days' WHERE uuid = $1",
        )
        .bind::<SqlUuid, _>(guest)
        .execute(c)?;
        diesel::sql_query(
            "INSERT INTO attachments (url, name, created_at) \
             VALUES ('/uploads/tickets/abandoned.png', 'abandoned.png', now() - interval '2 days')",
        )
        .execute(c)?;
        Ok(id)
    });
    let soft_deleted = ws.b.member_uuid;
    diesel::sql_query("UPDATE users SET deleted_at = now() - interval '400 days' WHERE uuid = $1")
        .bind::<SqlUuid, _>(soft_deleted)
        .execute(conn)
        .expect("soft-delete");

    // A workspace archived past its grace window, and an audit_log month
    // long past retention.
    let archived_workspace = common::mint_workspace(conn, "archived-long-ago", "Archived");
    exec(
        conn,
        &format!(
            "UPDATE workspaces SET archived_at = now() - interval '60 days' WHERE id = {archived_workspace}"
        ),
    );
    exec(
        conn,
        "CREATE TABLE audit_log_2020_01 PARTITION OF audit_log \
         FOR VALUES FROM ('2020-01-01') TO ('2020-02-01')",
    );

    Seeded {
        guest,
        guest_request,
        soft_deleted,
        old_delivery,
        archived_workspace,
        overdue_ticket,
        loan,
    }
}

#[actix_web::test]
async fn every_scheduled_job_runs_cleanly_on_the_job_pool() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    // Boot provisions the current months' partitions before anything writes,
    // so rows never land in the default partition; do the same here.
    backend::sync::partitions::ensure_partitions(&mut conn, 60).expect("partitions");
    let seeded = seed(&mut conn);
    drop(conn);
    // Production runs partition DDL over the schema-owning migration role.
    std::env::set_var("MIGRATION_DATABASE_URL", db.url());

    let job_pool = db.job_pool();
    let tmp = tempfile::tempdir().expect("temp search dir");
    let search =
        Arc::new(backend::services::search::SearchService::new(tmp.path(), &pool).expect("search"));
    let notifications = Arc::new(NotificationService::new(
        job_pool.clone(),
        Arc::new(RwLock::new(HashMap::new())),
    ));
    let jobs = backend::workers::scheduled_jobs(
        job_pool,
        search,
        notifications,
        "http://desk.test".to_string(),
    );

    let names: Vec<&str> = jobs.iter().map(|j| j.name).collect();
    let unlisted: Vec<&str> = names
        .iter()
        .copied()
        .filter(|n| !SEEDED.contains(n) && !NOT_EXERCISED.iter().any(|(m, _)| m == n))
        .collect();
    assert!(
        unlisted.is_empty(),
        "new scheduled jobs: seed work for them here, or list why not: {unlisted:?}"
    );
    let stale: Vec<&str> = SEEDED
        .iter()
        .copied()
        .chain(NOT_EXERCISED.iter().map(|(n, _)| *n))
        .filter(|n| !names.contains(n))
        .collect();
    assert!(
        stale.is_empty(),
        "listed jobs no longer registered: {stale:?}"
    );

    let capture = Capture::default();
    let logged = capture.0.clone();
    let _guard = tracing::subscriber::set_default(tracing_subscriber::registry().with(capture));
    let mut failures = Vec::new();
    for mut job in jobs {
        logged.lock().expect("capture").clear();
        let result = (job.run)().await;
        let lines = std::mem::take(&mut *logged.lock().expect("capture"));
        if let Err(e) = result {
            failures.push(format!("{}: returned {e:#}", job.name));
        }
        for line in lines {
            failures.push(format!("{}: logged {line}", job.name));
        }
    }
    drop(_guard);
    assert!(
        failures.is_empty(),
        "jobs failed on the job pool:\n{}",
        failures.join("\n")
    );

    // And they did the work they were given.
    let mut conn = pool.get().expect("conn");
    let gone = |conn: &mut Conn, what: &str, sql: String| {
        assert_eq!(count(conn, &sql), 0, "{what}");
    };
    gone(
        &mut conn,
        "expired session pruned",
        "SELECT count(*) AS n FROM active_sessions WHERE expires_at < now()".into(),
    );
    gone(
        &mut conn,
        "expired refresh token pruned",
        "SELECT count(*) AS n FROM refresh_tokens WHERE expires_at < now()".into(),
    );
    gone(
        &mut conn,
        "old idempotency key pruned",
        "SELECT count(*) AS n FROM idempotency_keys WHERE key = 'old-key'".into(),
    );
    gone(
        &mut conn,
        "old security event pruned",
        "SELECT count(*) AS n FROM security_events WHERE created_at < now() - interval '365 days'"
            .into(),
    );
    gone(
        &mut conn,
        "stale export failed",
        "SELECT count(*) AS n FROM workspace_export_jobs WHERE status = 'processing'".into(),
    );
    gone(
        &mut conn,
        "old CSP report pruned",
        "SELECT count(*) AS n FROM csp_reports".into(),
    );
    gone(
        &mut conn,
        "expired send lease swept",
        "SELECT count(*) AS n FROM outbound_emails WHERE status = 'sending'".into(),
    );
    gone(
        &mut conn,
        "old read notification archived",
        "SELECT count(*) AS n FROM notifications WHERE is_read AND archived_at IS NULL".into(),
    );
    gone(&mut conn, "digest sent", "SELECT count(*) AS n FROM notifications WHERE title = 'Waiting for the digest' AND NOT (channels_delivered @> '[\"email\"]'::jsonb)".into());
    gone(
        &mut conn,
        "old webhook delivery pruned",
        format!(
            "SELECT count(*) AS n FROM webhook_deliveries WHERE id = {}",
            seeded.old_delivery
        ),
    );
    gone(
        &mut conn,
        "expired audit_log month dropped",
        "SELECT count(*) AS n FROM pg_class WHERE relname = 'audit_log_2020_01'".into(),
    );
    gone(
        &mut conn,
        "soft-deleted user purged",
        format!(
            "SELECT count(*) AS n FROM users WHERE uuid = '{}'",
            seeded.soft_deleted
        ),
    );
    gone(
        &mut conn,
        "archived workspace purged",
        format!(
            "SELECT count(*) AS n FROM workspaces WHERE id = {}",
            seeded.archived_workspace
        ),
    );
    gone(
        &mut conn,
        "deleted workspace's files purged",
        "SELECT count(*) AS n FROM workspace_file_purges WHERE completed_at IS NULL".into(),
    );
    gone(
        &mut conn,
        "SLA breach stamped",
        format!(
            "SELECT count(*) AS n FROM tickets WHERE id = {} AND sla_response_breached_at IS NULL",
            seeded.overdue_ticket
        ),
    );
    gone(
        &mut conn,
        "overdue loan reminded",
        format!(
            "SELECT count(*) AS n FROM asset_loans WHERE id = {} AND overdue_notified_at IS NULL",
            seeded.loan
        ),
    );
    gone(
        &mut conn,
        "unconfirmed request removed",
        format!(
            "SELECT count(*) AS n FROM tickets WHERE id = {}",
            seeded.guest_request
        ),
    );
    gone(
        &mut conn,
        "abandoned upload removed",
        "SELECT count(*) AS n FROM attachments WHERE name = 'abandoned.png'".into(),
    );
    gone(
        &mut conn,
        "unused guest account removed",
        format!(
            "SELECT count(*) AS n FROM users WHERE uuid = '{}'",
            seeded.guest
        ),
    );
}
