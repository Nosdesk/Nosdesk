//! The migration that adds per-workspace ticket numbers gives every existing
//! ticket its id as its number, so a number already quoted still finds its
//! ticket, and moves each workspace's number sequence past its highest. Runs
//! the full migration set against a database seeded just before it.

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Insert a row the way it would already exist, with the table's user
/// triggers off, and return its id.
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

fn one(conn: &mut PgConnection, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

#[test]
fn existing_tickets_keep_their_id_as_their_number() {
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
        if name.contains("_per_workspace_ticket_numbers") && seeded.is_none() {
            let workspace = |conn: &mut PgConnection, slug: &str| {
                existing(
                    conn,
                    "workspaces",
                    &format!(
                        "INSERT INTO workspaces (slug, name) VALUES ('{slug}', '{slug}') RETURNING id"
                    ),
                )
            };
            let (a, b) = (
                workspace(&mut conn, "numbers-a"),
                workspace(&mut conn, "numbers-b"),
            );
            let backlog = |conn: &mut PgConnection, ws: i32| {
                existing(
                    conn,
                    "workflow_states",
                    &format!(
                        "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
                         VALUES ({ws}, 'Backlog', 'backlog', 'gray', 0, true) RETURNING id"
                    ),
                )
            };
            let (a_state, b_state) = (backlog(&mut conn, a), backlog(&mut conn, b));
            let ticket = |conn: &mut PgConnection, ws: i32, state: i32| {
                existing(
                    conn,
                    "tickets",
                    &format!(
                        "INSERT INTO tickets (workspace_id, title, workflow_state_id) \
                         VALUES ({ws}, 'Printer jammed', {state}) RETURNING id"
                    ),
                )
            };
            let a_first = ticket(&mut conn, a, a_state);
            let b_only = ticket(&mut conn, b, b_state);
            let a_last = ticket(&mut conn, a, a_state);
            seeded = Some((a, b, a_first, b_only, a_last));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let (a, b, a_first, b_only, a_last) = seeded.expect("the migration must be in the set");

    assert_eq!(
        one(
            &mut conn,
            "SELECT count(*) AS n FROM tickets WHERE number <> id"
        ),
        0,
        "every existing ticket's number is its id"
    );
    for (id, ws) in [(a_first, a), (b_only, b), (a_last, a)] {
        assert_eq!(
            one(
                &mut conn,
                &format!("SELECT count(*) AS n FROM tickets WHERE id = {id} AND workspace_id = {ws} AND number = {id}")
            ),
            1
        );
    }
    // The next number each workspace hands out follows its highest.
    let next = |conn: &mut PgConnection, ws: i32| {
        one(
            conn,
            &format!("SELECT nextval('ticket_numbers.workspace_{ws}') AS n"),
        )
    };
    assert_eq!(next(&mut conn, a), i64::from(a_last) + 1);
    assert_eq!(next(&mut conn, b), i64::from(b_only) + 1);
}
