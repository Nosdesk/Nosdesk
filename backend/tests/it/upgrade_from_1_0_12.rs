//! An upgrade from 1.0.12 with realistic data: every later migration runs on
//! it as the superuser, the data survives, the backfills land, and the
//! runtime role reads a ticket through the repository afterwards. The boot
//! itself is `tests/upgrade_from_1_0_12_boot.rs` (it sets process env).

use std::collections::{BTreeMap, BTreeSet};

use diesel::migration::MigrationSource;
use diesel::pg::{Pg, PgConnection};
use diesel::prelude::*;
use diesel::r2d2;
use diesel::sql_types::{BigInt, Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use backend::db::MIGRATIONS;
use backend::repository::ticket_visibility::CommentAudience;

use crate::common::upgrade_1_0_12::{self as fixture, Seeded, UpgradeDb, KEPT_TABLES};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Closed {
    #[diesel(sql_type = Nullable<Timestamptz>)]
    closed_at: Option<chrono::DateTime<chrono::Utc>>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    closed_by: Option<Uuid>,
}

#[derive(QueryableByName)]
struct Merge {
    #[diesel(sql_type = Integer)]
    merged_into_ticket_id: i32,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    merged_by_user_uuid: Option<Uuid>,
    #[diesel(sql_type = Integer)]
    workspace_id: i32,
}

#[derive(QueryableByName)]
struct Suppression {
    #[diesel(sql_type = Integer)]
    workspace_id: i32,
    #[diesel(sql_type = Text)]
    email: String,
}

#[derive(QueryableByName)]
struct Slug {
    #[diesel(sql_type = Text)]
    slug: String,
}

#[derive(QueryableByName)]
struct State {
    #[diesel(sql_type = Integer)]
    workflow_state_id: i32,
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

fn closed(conn: &mut PgConnection, ticket: i32) -> Closed {
    diesel::sql_query(format!(
        "SELECT closed_at, closed_by FROM tickets WHERE id = {ticket}"
    ))
    .get_result::<Closed>(conn)
    .unwrap_or_else(|e| panic!("ticket {ticket}: {e}"))
}

fn at(rfc3339: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    Some(
        chrono::DateTime::parse_from_rfc3339(rfc3339)
            .expect("timestamp")
            .with_timezone(&chrono::Utc),
    )
}

/// Seed 1.0.12 data and migrate to head; the seed and the counts before.
fn upgraded() -> (UpgradeDb, PgConnection, Seeded, BTreeMap<String, i64>) {
    let db = UpgradeDb::at_1_0_12();
    let mut conn = db.conn();
    let side_effects = |conn: &mut PgConnection| {
        (
            count(conn, "SELECT count(*) AS n FROM audit_log"),
            count(conn, "SELECT count(*) AS n FROM webhook_outbox"),
        )
    };
    let quiet = side_effects(&mut conn);
    let seeded = fixture::seed(&mut conn);
    // Triggers were off: seeding audited nothing and queued no webhooks.
    assert_eq!(
        side_effects(&mut conn),
        quiet,
        "(audit_log, webhook_outbox)"
    );
    let before = fixture::row_counts(&mut conn, KEPT_TABLES);
    let ran = db.migrate_to_head(&mut conn);
    assert!(!ran.is_empty(), "migrations after 1.0.12 ran");
    (db, conn, seeded, before)
}

#[test]
fn an_upgrade_from_1_0_12_keeps_the_data_and_applies_the_backfills() {
    let (_db, mut conn, seeded, before) = upgraded();
    let (ws, people, tickets) = (&seeded.workspaces, &seeded.people, &seeded.tickets);

    // Every embedded migration is applied and none is unknown: what the boot
    // drift check requires.
    let embedded: BTreeSet<String> = MigrationSource::<Pg>::migrations(&MIGRATIONS)
        .expect("list embedded migrations")
        .iter()
        .map(|m| m.name().version().to_string())
        .collect();
    let applied: BTreeSet<String> = conn
        .applied_migrations()
        .expect("read applied migrations")
        .into_iter()
        .map(|v| v.to_string())
        .collect();
    assert_eq!(applied, embedded, "applied migrations match this build's");

    assert_eq!(
        fixture::row_counts(&mut conn, KEPT_TABLES),
        before,
        "row counts survive the upgrade"
    );

    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS n FROM tickets WHERE number <> id"
        ),
        0,
        "every ticket's number is its id"
    );

    let merges = diesel::sql_query(format!(
        "SELECT merged_into_ticket_id, merged_by_user_uuid, workspace_id \
         FROM ticket_merges WHERE ticket_id = {}",
        tickets.merged
    ))
    .load::<Merge>(&mut conn)
    .expect("read ticket_merges");
    assert_eq!(merges.len(), 1, "the merge moved to ticket_merges");
    assert_eq!(merges[0].merged_into_ticket_id, tickets.merge_target);
    assert_eq!(merges[0].merged_by_user_uuid, Some(people.agent));
    assert_eq!(merges[0].workspace_id, ws.default);
    assert_eq!(
        count(&mut conn, "SELECT count(*) AS n FROM ticket_merges"),
        1
    );

    let done = closed(&mut conn, tickets.done);
    assert_eq!(
        done.closed_at,
        at(tickets.done_closed_at),
        "closed when the history says"
    );
    assert_eq!(
        done.closed_by,
        Some(people.agent),
        "closed by who moved it to Done"
    );
    let cancelled = closed(&mut conn, tickets.cancelled);
    assert_eq!(
        cancelled.closed_at,
        at(tickets.cancelled_updated_at),
        "no history: updated_at"
    );
    assert_eq!(cancelled.closed_by, None);
    let open = closed(&mut conn, tickets.open);
    assert_eq!((open.closed_at, open.closed_by), (None, None));

    let moved = diesel::sql_query(format!(
        "SELECT workflow_state_id FROM tickets WHERE id = {}",
        tickets.foreign_state
    ))
    .get_result::<State>(&mut conn)
    .expect("read the moved ticket")
    .workflow_state_id;
    assert_eq!(
        moved, ws.teams_states.backlog,
        "a ticket on another workspace's state moves to its own Backlog"
    );

    assert_eq!(
        count(
            &mut conn,
            "SELECT count(*) AS n FROM refresh_tokens WHERE revoked_at IS NULL"
        ),
        0,
        "refresh tokens are revoked, so every device signs in again"
    );

    let slug = diesel::sql_query(format!(
        "SELECT slug::text AS slug FROM workspaces WHERE id = {}",
        ws.teams
    ))
    .get_result::<Slug>(&mut conn)
    .expect("read the teams workspace")
    .slug;
    assert_eq!(slug, "teams-workspace", "a newly reserved slug is renamed");

    let suppressions: Vec<(i32, String)> = diesel::sql_query(
        "SELECT workspace_id, email FROM email_suppressions ORDER BY workspace_id, email",
    )
    .load::<Suppression>(&mut conn)
    .expect("read suppressions")
    .into_iter()
    .map(|s| (s.workspace_id, s.email))
    .collect();
    assert_eq!(
        suppressions,
        vec![
            (ws.default, seeded.suppressed_email.to_string()),
            (ws.teams, seeded.suppressed_email.to_string()),
        ],
        "a suppression is kept for each workspace that mailed the address; \
         one no workspace mailed is dropped"
    );

    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM documentation_revisions \
                 WHERE page_id = {} AND created_by = '{}'",
                seeded.page, people.former
            )
        ),
        1,
        "a former member's revision keeps its author"
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM workspace_members WHERE user_uuid = '{}'",
                people.former
            )
        ),
        0,
        "a former member doesn't become a member again"
    );
}

#[test]
fn the_runtime_role_reads_an_upgraded_ticket_through_the_repository() {
    let (db, _conn, seeded, _) = upgraded();
    let sep = if db.url.contains('?') { '&' } else { '?' };
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .test_on_check_out(true)
        .build(backend::db::ResettingManager::new(format!(
            "{}{sep}options=-c%20role%3Dnosdesk_app",
            db.url
        )))
        .expect("runtime pool");
    let mut conn = pool.get().expect("runtime connection");
    diesel::sql_query(format!(
        "SELECT set_config('app.workspace_id', '{}', false)",
        seeded.workspaces.default
    ))
    .execute(&mut conn)
    .expect("pin the workspace");

    let ticket = backend::repository::tickets::get_complete_ticket(
        &mut conn,
        seeded.tickets.open,
        CommentAudience::All,
    )
    .expect("read the upgraded ticket");
    assert_eq!(ticket.ticket.title, "Printer on level 3 is jammed");
    assert_eq!(ticket.ticket.number, seeded.tickets.open);
    assert_eq!(ticket.comments.len(), 2, "the reply and the internal note");
    assert_eq!(
        ticket
            .comments
            .iter()
            .map(|c| c.attachments.len())
            .sum::<usize>(),
        1,
        "the reply keeps its attachment"
    );
    assert_eq!(
        ticket.requester_user.map(|u| u.uuid),
        Some(seeded.people.requester)
    );
}
