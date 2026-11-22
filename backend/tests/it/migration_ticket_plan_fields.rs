//! A ticket's due and start dates are days.
//!
//! Before the plan fields, a recurring ticket with no due date carried
//! creation's time of day into its successor's due date, and a REST write
//! with an offset stored the instant it named. The migration makes each such
//! value its UTC calendar day without counting it as an edit, then holds
//! every later write to a day, and stops a reorder from counting as one.
//! Runs the migration set against a database seeded just before it.

#![allow(clippy::expect_used)]

use chrono::{DateTime, NaiveDate, NaiveTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Integer, Nullable, Timestamptz};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run `sql` the way the rows would already exist: the table's user triggers
/// off.
fn existing(conn: &mut PgConnection, table: &str, sql: &str) -> i32 {
    diesel::sql_query(format!("ALTER TABLE {table} DISABLE TRIGGER USER"))
        .execute(conn)
        .expect("disable triggers");
    let id = diesel::sql_query(sql)
        .get_result::<Id>(conn)
        .unwrap_or_else(|e| panic!("seed {table}: {e}"))
        .id;
    diesel::sql_query(format!("ALTER TABLE {table} ENABLE TRIGGER USER"))
        .execute(conn)
        .expect("enable triggers");
    id
}

struct Seeded {
    /// The workspace in UTC whose undated ticket the rule tests write to.
    workspace: i32,
    undated: i32,
    stray: i32,
    utc_midnight: i32,
}

/// A workspace whose `site_settings.default_timezone` is `zone` (no settings
/// row for `None`), with an open state. Returns the workspace and the state.
fn workspace(conn: &mut PgConnection, slug: &str, zone: Option<&str>) -> (i32, i32) {
    let workspace = existing(
        conn,
        "workspaces",
        &format!("INSERT INTO workspaces (slug, name) VALUES ('{slug}', '{slug}') RETURNING id"),
    );
    if let Some(zone) = zone {
        existing(
            conn,
            "site_settings",
            &format!(
                "INSERT INTO site_settings (workspace_id, default_timezone) \
                 VALUES ({workspace}, '{zone}') RETURNING id"
            ),
        );
    }
    let state = existing(
        conn,
        "workflow_states",
        &format!(
            "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
             VALUES ({workspace}, 'Open', 'backlog', 'gray', 0, true) RETURNING id"
        ),
    );
    (workspace, state)
}

fn ticket(conn: &mut PgConnection, (workspace, state): (i32, i32), due: &str, start: &str) -> i32 {
    existing(
        conn,
        "tickets",
        &format!(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id, number, created_at, \
                                  updated_at, due_date, start_date) \
             VALUES ({workspace}, 'A job', {state}, \
                     (SELECT coalesce(max(number), 0) + 1 FROM tickets \
                       WHERE workspace_id = {workspace}), \
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z', {due}, {start}) \
             RETURNING id"
        ),
    )
}

/// Tickets as earlier writes left them: some dates at UTC midnight (the web
/// client's), some with a time of day.
fn seed(conn: &mut PgConnection) -> Seeded {
    let utc = workspace(conn, "plan-utc", Some("UTC"));
    let sydney = workspace(conn, "plan-sydney", Some("Australia/Sydney"));
    let new_york = workspace(conn, "plan-new-york", Some("America/New_York"));
    Seeded {
        workspace: utc.0,
        undated: ticket(conn, utc, "NULL", "NULL"),
        // Every screen shows the UTC date: the 17th and the 19th, whatever
        // the workspace's zone.
        stray: ticket(
            conn,
            sydney,
            "'2026-11-17T22:30:00Z'",
            "'2026-11-19T13:00:00Z'",
        ),
        utc_midnight: ticket(conn, new_york, "'2026-11-21T00:00:00Z'", "NULL"),
    }
}

fn upgrade() -> (FreshDb, PgConnection, Seeded) {
    let db = FreshDb::new();
    let mut conn = backend::db::establish(&db.url).expect("connect fresh db");
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
        if name.contains("_ticket_plan_fields") {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    (db, conn, seeded.expect("the migration ran"))
}

#[derive(QueryableByName)]
struct Dates {
    #[diesel(sql_type = Nullable<Timestamptz>)]
    due_date: Option<DateTime<Utc>>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    start_date: Option<DateTime<Utc>>,
    #[diesel(sql_type = Timestamptz)]
    updated_at: DateTime<Utc>,
}

fn dates(conn: &mut PgConnection, id: i32) -> Dates {
    diesel::sql_query("SELECT due_date, start_date, updated_at FROM tickets WHERE id = $1")
        .bind::<Integer, _>(id)
        .get_result(conn)
        .expect("ticket")
}

fn utc_day(y: i32, m: u32, d: u32) -> Option<DateTime<Utc>> {
    Some(
        NaiveDate::from_ymd_opt(y, m, d)
            .expect("date")
            .and_time(NaiveTime::MIN)
            .and_utc(),
    )
}

#[test]
fn a_date_with_a_time_of_day_becomes_its_utc_day() {
    let (_db, mut conn, s) = upgrade();
    let untouched = utc_day(2026, 1, 1).map(|d| d.timestamp());

    let t = dates(&mut conn, s.stray);
    assert_eq!(
        (t.due_date, t.start_date),
        (utc_day(2026, 11, 17), utc_day(2026, 11, 19)),
        "the UTC day, whatever the workspace's zone"
    );
    assert_eq!(
        Some(t.updated_at.timestamp()),
        untouched,
        "the repair isn't an edit"
    );

    let t = dates(&mut conn, s.utc_midnight);
    assert_eq!(
        (t.due_date, t.start_date),
        (utc_day(2026, 11, 21), None),
        "a UTC midnight is left as it is"
    );
    assert_eq!(Some(t.updated_at.timestamp()), untouched);

    let t = dates(&mut conn, s.undated);
    assert_eq!((t.due_date, t.start_date), (None, None));
}

#[test]
fn the_plan_columns_keep_their_rules() {
    let (_db, mut conn, s) = upgrade();
    diesel::sql_query(format!("SET app.workspace_id = '{}'", s.workspace))
        .execute(&mut conn)
        .expect("pin the workspace for the audit trigger");
    let write = |conn: &mut PgConnection, set: &str| {
        diesel::sql_query(format!("UPDATE tickets SET {set} WHERE id = {}", s.undated))
            .execute(conn)
            .map(drop)
    };
    for refused in [
        "due_date = '2026-11-21T09:00:00Z'",
        "start_date = '2026-11-21T00:00:00+11:00'",
        "due_time = '09:00'",
        "start_time = '09:00'",
        "estimate_minutes = 0",
        "rank = ''",
    ] {
        assert!(write(&mut conn, refused).is_err(), "{refused}");
    }
    write(
        &mut conn,
        "due_date = '2026-11-21T00:00:00Z', due_time = '09:00', estimate_minutes = 30",
    )
    .expect("a day with a time and a size");

    // A rank alone leaves updated_at and writes no audit row; anything else
    // stamps it and is audited.
    let audited = |conn: &mut PgConnection| -> i64 {
        #[derive(QueryableByName)]
        struct Count {
            #[diesel(sql_type = diesel::sql_types::BigInt)]
            n: i64,
        }
        diesel::sql_query(format!(
            "SELECT count(*) AS n FROM audit_log \
             WHERE table_name = 'tickets' AND pk_text = '{}' AND op = 'U'",
            s.undated
        ))
        .get_result::<Count>(conn)
        .expect("count audit rows")
        .n
    };
    let before = dates(&mut conn, s.undated).updated_at;
    let audit_before = audited(&mut conn);
    write(&mut conn, "rank = 'a0'").expect("place it");
    assert_eq!(dates(&mut conn, s.undated).updated_at, before);
    assert_eq!(audited(&mut conn), audit_before, "a reorder isn't audited");
    write(&mut conn, "rank = 'a1', estimate_minutes = 60").expect("resize it");
    assert!(dates(&mut conn, s.undated).updated_at > before);
    assert_eq!(audited(&mut conn), audit_before + 1, "a resize is");
}
