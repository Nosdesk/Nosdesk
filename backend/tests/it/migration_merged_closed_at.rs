//! Tickets merged while open are closed when they were merged.
//!
//! The closed_at trigger used to leave Merged alone, and the merge never
//! stamped it, so a ticket merged while open stayed open on every count. The
//! migration stamps those tickets from `ticket_merges`: closed when merged
//! (never before the ticket was created), by whoever merged it when that user
//! still exists. A ticket closed before its merge keeps its stamp. Runs the
//! migration set against a database seeded just before it.

#![allow(clippy::expect_used)]

use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run `sql` the way the rows would already exist: the table's user triggers
/// off. Returns the id it hands back, if any.
fn existing(conn: &mut PgConnection, table: &str, sql: &str) -> Option<i32> {
    diesel::sql_query(format!("ALTER TABLE {table} DISABLE TRIGGER USER"))
        .execute(conn)
        .expect("disable triggers");
    let id = if sql.contains("RETURNING id") {
        Some(
            diesel::sql_query(sql)
                .get_result::<Id>(conn)
                .unwrap_or_else(|e| panic!("seed {table}: {e}"))
                .id,
        )
    } else {
        diesel::sql_query(sql)
            .execute(conn)
            .unwrap_or_else(|e| panic!("seed {table}: {e}"));
        None
    };
    diesel::sql_query(format!("ALTER TABLE {table} ENABLE TRIGGER USER"))
        .execute(conn)
        .expect("enable triggers");
    id
}

fn at(t: DateTime<Utc>) -> String {
    format!("'{}'::timestamptz", t.to_rfc3339())
}

struct Seeded {
    now: DateTime<Utc>,
    agent: Uuid,
    closer: Uuid,
    merged_while_open: i32,
    merged_before_created: i32,
    merged_by_someone_gone: i32,
    closed_then_merged: i32,
    still_open: i32,
}

/// Tickets as merges left them before the change: closed_at only on the one
/// closed before its merge.
fn seed(conn: &mut PgConnection) -> Seeded {
    let now = Utc::now();
    let ws = existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('merged-closed', 'Merged Closed') RETURNING id",
    )
    .expect("workspace");
    let state = |conn: &mut PgConnection, name: &str, category: &str, position: i32| {
        existing(
            conn,
            "workflow_states",
            &format!(
                "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
                 VALUES ({ws}, '{name}', '{category}', 'gray', {position}, {}) RETURNING id",
                position == 0
            ),
        )
        .expect("state")
    };
    let open = state(conn, "Open", "backlog", 0);
    let merged = state(conn, "Merged", "merged", 1);

    let agent = Uuid::new_v4();
    let closer = Uuid::new_v4();
    existing(
        conn,
        "users",
        &format!(
            "INSERT INTO users (uuid, name) VALUES ('{agent}', 'Agent'), ('{closer}', 'Closer')"
        ),
    );

    let mut number = 0;
    let mut ticket = |conn: &mut PgConnection,
                      state: i32,
                      created: DateTime<Utc>,
                      closed: Option<(DateTime<Utc>, Uuid)>| {
        number += 1;
        let (closed_at, closed_by) = match closed {
            Some((t, by)) => (at(t), format!("'{by}'")),
            None => ("NULL".to_string(), "NULL".to_string()),
        };
        existing(
            conn,
            "tickets",
            &format!(
                "INSERT INTO tickets (workspace_id, title, workflow_state_id, number, created_at, \
                                      updated_at, closed_at, closed_by) \
                 VALUES ({ws}, 'Ticket {number}', {state}, {number}, {created}, {created}, \
                         {closed_at}, {closed_by}) RETURNING id",
                created = at(created),
            ),
        )
        .expect("ticket")
    };
    let destination = ticket(conn, open, now - Duration::days(10), None);
    let merged_while_open = ticket(conn, merged, now - Duration::days(5), None);
    let merged_before_created = ticket(conn, merged, now - Duration::days(1), None);
    let merged_by_someone_gone = ticket(conn, merged, now - Duration::days(5), None);
    let closed_then_merged = ticket(
        conn,
        merged,
        now - Duration::days(5),
        Some((now - Duration::days(4), closer)),
    );
    let still_open = ticket(conn, open, now - Duration::days(5), None);

    for (source, merged_at, by) in [
        (merged_while_open, now - Duration::days(2), agent),
        // A merge clock behind the ticket's own: closed when it was created.
        (merged_before_created, now - Duration::days(2), agent),
        (
            merged_by_someone_gone,
            now - Duration::days(2),
            Uuid::new_v4(),
        ),
        (closed_then_merged, now - Duration::days(2), agent),
    ] {
        existing(
            conn,
            "ticket_merges",
            &format!(
                "INSERT INTO ticket_merges \
                   (ticket_id, merged_into_ticket_id, merged_at, merged_by_user_uuid, workspace_id) \
                 VALUES ({source}, {destination}, {}, '{by}', {ws})",
                at(merged_at)
            ),
        );
    }

    Seeded {
        now,
        agent,
        closer,
        merged_while_open,
        merged_before_created,
        merged_by_someone_gone,
        closed_then_merged,
        still_open,
    }
}

/// Migrate a fresh database, seeding just before the migration.
fn upgrade() -> (FreshDb, PgConnection, Seeded) {
    let db = FreshDb::new();
    let mut conn = PgConnection::establish(&db.url).expect("connect fresh db");
    diesel::sql_query(
        "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (\
         version VARCHAR(50) PRIMARY KEY NOT NULL, \
         run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&mut conn)
    .expect("create migrations table");

    let mut pending = conn
        .pending_migrations(MIGRATIONS)
        .expect("list pending migrations");
    pending.sort_by_key(|m| m.name().to_string());
    let mut seeded = None;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_merged_closed_at") {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let seeded = seeded.expect("the migration ran");
    (db, conn, seeded)
}

#[derive(QueryableByName)]
struct Closed {
    #[diesel(sql_type = Nullable<Timestamptz>)]
    closed_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    closed_by: Option<Uuid>,
    #[diesel(sql_type = Timestamptz)]
    created_at: DateTime<Utc>,
    #[diesel(sql_type = Timestamptz)]
    updated_at: DateTime<Utc>,
}

fn closed(conn: &mut PgConnection, id: i32) -> Closed {
    diesel::sql_query(
        "SELECT closed_at, closed_by, created_at, updated_at FROM tickets WHERE id = $1",
    )
    .bind::<Integer, _>(id)
    .get_result(conn)
    .expect("ticket")
}

fn micros(t: Option<DateTime<Utc>>) -> Option<i64> {
    t.map(|t| t.timestamp_micros())
}

#[test]
fn tickets_merged_while_open_are_closed_when_merged() {
    let (_db, mut conn, s) = upgrade();
    let merged_at = s.now - Duration::days(2);

    let t = closed(&mut conn, s.merged_while_open);
    assert_eq!(micros(t.closed_at), micros(Some(merged_at)));
    assert_eq!(t.closed_by, Some(s.agent));
    assert_eq!(t.updated_at, t.created_at, "the repair isn't an edit");

    let t = closed(&mut conn, s.merged_before_created);
    assert_eq!(
        t.closed_at,
        Some(t.created_at),
        "never before it was created"
    );
    assert_eq!(t.closed_by, Some(s.agent));

    let t = closed(&mut conn, s.merged_by_someone_gone);
    assert_eq!(micros(t.closed_at), micros(Some(merged_at)));
    assert_eq!(t.closed_by, None, "the merging user no longer exists");

    let t = closed(&mut conn, s.closed_then_merged);
    assert_eq!(
        micros(t.closed_at),
        micros(Some(s.now - Duration::days(4))),
        "a ticket closed before its merge keeps its stamp"
    );
    assert_eq!(t.closed_by, Some(s.closer));

    let t = closed(&mut conn, s.still_open);
    assert_eq!((t.closed_at, t.closed_by), (None, None));
}

#[derive(QueryableByName)]
struct Enabled {
    #[diesel(sql_type = Text)]
    tgname: String,
    #[diesel(sql_type = Text)]
    tgenabled: String,
}

#[test]
fn the_backfill_leaves_the_tickets_triggers_on() {
    let (_db, mut conn, _) = upgrade();
    let triggers: Vec<Enabled> = diesel::sql_query(
        "SELECT tgname::text AS tgname, tgenabled::text AS tgenabled FROM pg_trigger \
         WHERE tgrelid = 'public.tickets'::regclass \
           AND tgname IN ('tr_audit_tickets', 'set_updated_at', 'ticket_closed_follows_state') \
         ORDER BY tgname",
    )
    .load(&mut conn)
    .expect("triggers");
    assert_eq!(triggers.len(), 3);
    for t in triggers {
        assert_eq!(t.tgenabled, "O", "{} is enabled", t.tgname);
    }
}
