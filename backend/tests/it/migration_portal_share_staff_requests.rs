//! Tickets raised before `tickets.raised_by_staff` existed get it from their
//! requester's role at migration time: an agent, admin or owner of the
//! ticket's workspace (removed ones included), or a platform admin. Staff of
//! another workspace don't count, and the backfill isn't an edit. Runs the
//! migration set against a database seeded just before it.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{Bool, Integer, Timestamptz};
use diesel_migrations::MigrationHarness;

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

/// Ticket ids by who raised them, and whether each should count as staff.
fn seed(conn: &mut PgConnection) -> Vec<(&'static str, i32, bool)> {
    let ws = existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('raised-a', 'A') RETURNING id",
    )
    .expect("workspace");
    let other = existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('raised-b', 'B') RETURNING id",
    )
    .expect("other workspace");
    let state = existing(
        conn,
        "workflow_states",
        &format!(
            "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
             VALUES ({ws}, 'Open', 'backlog', 'gray', 0, true) RETURNING id"
        ),
    )
    .expect("state");

    // (who, platform role, role here, removed, role in the other workspace, staff?)
    type Person = (
        &'static str,
        &'static str,
        &'static str,
        bool,
        Option<&'static str>,
        bool,
    );
    let people: [Person; 7] = [
        ("member", "user", "member", false, None, false),
        ("agent", "user", "agent", false, None, true),
        ("admin", "user", "admin", false, None, true),
        ("owner", "user", "owner", false, None, true),
        ("removed agent", "user", "agent", true, None, true),
        (
            "platform admin",
            "platform_admin",
            "member",
            false,
            None,
            true,
        ),
        (
            "staff elsewhere",
            "user",
            "member",
            false,
            Some("agent"),
            false,
        ),
    ];
    let mut out = Vec::new();
    for (i, (who, platform, role, removed, elsewhere, staff)) in people.into_iter().enumerate() {
        let user = uuid::Uuid::new_v4();
        existing(
            conn,
            "users",
            &format!("INSERT INTO users (uuid, name, platform_role) VALUES ('{user}', '{who}', '{platform}')"),
        );
        let removed_at = if removed { "now()" } else { "NULL" };
        existing(
            conn,
            "workspace_members",
            &format!(
                "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at, removed_at) \
                 VALUES ({ws}, '{user}', '{role}', now(), {removed_at})"
            ),
        );
        if let Some(role) = elsewhere {
            existing(
                conn,
                "workspace_members",
                &format!(
                    "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at) \
                     VALUES ({other}, '{user}', '{role}', now())"
                ),
            );
        }
        let ticket = existing(
            conn,
            "tickets",
            &format!(
                "INSERT INTO tickets (workspace_id, title, workflow_state_id, requester_uuid, number, updated_at) \
                 VALUES ({ws}, '{who}', {state}, '{user}', {n}, '2026-01-01T00:00:00Z') RETURNING id",
                n = i + 1
            ),
        )
        .expect("ticket");
        out.push((who, ticket, staff));
    }
    let nobody = existing(
        conn,
        "tickets",
        &format!(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id, number, updated_at) \
             VALUES ({ws}, 'no requester', {state}, 99, '2026-01-01T00:00:00Z') RETURNING id"
        ),
    )
    .expect("ticket");
    out.push(("no requester", nobody, false));
    out
}

#[derive(QueryableByName)]
struct Marked {
    #[diesel(sql_type = Bool)]
    raised_by_staff: bool,
    #[diesel(sql_type = Timestamptz)]
    updated_at: chrono::DateTime<chrono::Utc>,
}

#[test]
fn earlier_tickets_are_marked_by_their_requesters_role() {
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

    // Seed just before the migration, or after everything when there is none.
    let mut seeded = None;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_portal_share_staff_requests") && seeded.is_none() {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let tickets = seeded.unwrap_or_else(|| seed(&mut conn));

    let before = chrono::DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .expect("time")
        .with_timezone(&chrono::Utc);
    for (who, id, staff) in tickets {
        let row: Marked = diesel::sql_query(format!(
            "SELECT raised_by_staff, updated_at FROM tickets WHERE id = {id}"
        ))
        .get_result(&mut conn)
        .unwrap_or_else(|e| panic!("{who}: {e}"));
        assert_eq!(row.raised_by_staff, staff, "{who}");
        assert_eq!(row.updated_at, before, "{who}: the backfill is not an edit");
    }
}
