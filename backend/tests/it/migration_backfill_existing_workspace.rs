//! Regression guard for data-backfill migrations: run the full migration set
//! against a database that already has a workspace, the way production looks.
//!
//! v1.0.7's `2026-06-16_site_settings_per_workspace` backfilled a settings row
//! per existing workspace with a raw INSERT, which fired the site_settings
//! audit trigger. That trigger raises NDX01 ("audit context missing") when
//! `app.workspace_id` is unset, as it is in a migration session. CI only ever
//! migrated an EMPTY database (no workspaces, so the backfill is a no-op and
//! the trigger never fires), so the crash-loop surfaced only in production.
//!
//! This test seeds a pre-existing workspace BEFORE the backfill migration runs,
//! exercising the path CI missed. With the unguarded backfill it fails at the
//! migration (NDX01); with the trigger suppressed around the backfill it passes.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::BigInt;
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use backend::db::MIGRATIONS;

fn base_url() -> String {
    dotenvy::dotenv().ok();
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for tests")
}

/// Swap the database name on a Postgres URL: `…/old[?params]` -> `…/db[?params]`.
fn with_database(url: &str, db: &str) -> String {
    let q = url.find('?').unwrap_or(url.len());
    let path_start = url[..q].rfind('/').expect("URL must have a path");
    format!("{}/{}{}", &url[..path_start], db, &url[q..])
}

fn admin_url() -> String {
    with_database(&base_url(), "postgres")
}

/// A throwaway empty database, dropped on scope exit (even on panic).
struct FreshDb {
    name: String,
    url: String,
}

impl FreshDb {
    fn new() -> Self {
        let suffix = Uuid::new_v4().simple().to_string()[..16].to_string();
        let name = format!("nosdesk_migtest_{suffix}");
        let url = with_database(&base_url(), &name);
        let mut admin = PgConnection::establish(&admin_url()).expect("connect admin db");
        diesel::sql_query(format!("CREATE DATABASE \"{name}\""))
            .execute(&mut admin)
            .expect("create fresh db");
        Self { name, url }
    }
}

impl Drop for FreshDb {
    fn drop(&mut self) {
        if let Ok(mut admin) = PgConnection::establish(&admin_url()) {
            let _ = diesel::sql_query(format!(
                "SELECT pg_terminate_backend(pid) FROM pg_stat_activity \
                 WHERE datname = '{}' AND pid <> pg_backend_pid()",
                self.name
            ))
            .execute(&mut admin);
            let _ = diesel::sql_query(format!("DROP DATABASE IF EXISTS \"{}\"", self.name))
                .execute(&mut admin);
        }
    }
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    c: i64,
}

#[test]
fn backfill_migration_succeeds_when_a_workspace_already_exists() {
    let db = FreshDb::new();
    let mut conn = PgConnection::establish(&db.url).expect("connect fresh db");

    // Diesel's bookkeeping table, created up front so the per-migration
    // harness calls below work on a truly empty database.
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

    let mut seeded = false;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("site_settings_per_workspace") {
            // The workspaces table exists now (initial schema applied). Seed a
            // pre-existing workspace WITHOUT setting app.workspace_id, so the
            // backfill migration runs under the same context production has.
            // The workspaces audit trigger is suppressed for this scaffolding
            // write (it would otherwise need its own workspace context).
            diesel::sql_query("ALTER TABLE workspaces DISABLE TRIGGER USER")
                .execute(&mut conn)
                .expect("disable workspaces trigger");
            diesel::sql_query(
                "INSERT INTO workspaces (slug, name) VALUES ('acme-preexisting', 'Acme Preexisting')",
            )
            .execute(&mut conn)
            .expect("seed pre-existing workspace");
            diesel::sql_query("ALTER TABLE workspaces ENABLE TRIGGER USER")
                .execute(&mut conn)
                .expect("enable workspaces trigger");
            seeded = true;
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    assert!(seeded, "the target backfill migration must be in the set");

    // The backfill must have given the pre-existing workspace its settings row.
    let count = diesel::sql_query(
        "SELECT count(*) AS c FROM site_settings ss \
         JOIN workspaces w ON w.id = ss.workspace_id \
         WHERE w.slug = 'acme-preexisting'",
    )
    .get_result::<Count>(&mut conn)
    .expect("count settings rows");
    assert_eq!(
        count.c, 1,
        "the pre-existing workspace must get exactly one settings row from the backfill"
    );
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    id: i32,
}

/// Insert one row the way it would already exist, with the table's user
/// triggers (audit, seeding) off, and return its id.
fn insert_existing(conn: &mut PgConnection, table: &str, sql: &str) -> i32 {
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

fn workspace(conn: &mut PgConnection, slug: &str) -> i32 {
    insert_existing(
        conn,
        "workspaces",
        &format!("INSERT INTO workspaces (slug, name) VALUES ('{slug}', '{slug}') RETURNING id"),
    )
}

fn state(conn: &mut PgConnection, ws: i32, category: &str, position: i32, default: bool) -> i32 {
    insert_existing(
        conn,
        "workflow_states",
        &format!(
            "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
             VALUES ({ws}, '{category} {position}', '{category}', 'gray', {position}, {default}) \
             RETURNING id"
        ),
    )
}

fn ticket(conn: &mut PgConnection, ws: i32, state: i32) -> i32 {
    insert_existing(
        conn,
        "tickets",
        &format!(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id) \
             VALUES ({ws}, 'Printer jammed', {state}) RETURNING id"
        ),
    )
}

fn state_of(conn: &mut PgConnection, ticket: i32) -> i32 {
    diesel::sql_query("SELECT workflow_state_id AS id FROM tickets WHERE id = $1")
        .bind::<diesel::sql_types::Integer, _>(ticket)
        .get_result::<Id>(conn)
        .expect("read ticket state")
        .id
}

/// A ticket holding another workspace's workflow state moves to the matching
/// state in its own workspace (same category, the default first), and the
/// migration runs with existing tickets, where the audit trigger would raise
/// NDX01 if left on.
#[test]
fn tickets_holding_another_workspaces_state_move_into_their_own() {
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

    // (ticket, the state it should hold after the repair)
    let mut expected: Vec<(i32, i32)> = Vec::new();
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("ticket_states_in_own_workspace") {
            let a = workspace(&mut conn, "repair-a");
            let b = workspace(&mut conn, "repair-b");
            // A non-default Backlog state that sorts first must not win over
            // the default one.
            state(&mut conn, a, "backlog", 0, false);
            let a_backlog = state(&mut conn, a, "backlog", 1, true);
            let a_done = state(&mut conn, a, "done", 2, false);
            let b_backlog = state(&mut conn, b, "backlog", 0, true);
            let b_done = state(&mut conn, b, "done", 1, false);

            expected.push((ticket(&mut conn, a, b_backlog), a_backlog));
            expected.push((ticket(&mut conn, a, b_done), a_done));
            // Already in their own workspace: untouched.
            expected.push((ticket(&mut conn, a, a_done), a_done));
            expected.push((ticket(&mut conn, b, b_backlog), b_backlog));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    assert!(
        !expected.is_empty(),
        "the repair migration must be in the set"
    );

    for (ticket, state) in expected {
        assert_eq!(state_of(&mut conn, ticket), state, "ticket {ticket}");
    }
}
