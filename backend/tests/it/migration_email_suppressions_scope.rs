//! Scoping email suppressions per workspace keeps a suppression for each
//! workspace that mailed the address, including when more than one did.
//! Runs the full migration set against a database seeded just before it.

use diesel::prelude::*;
use diesel::sql_types::{Integer, Text};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

const SCOPE_MIGRATION: &str = "_email_suppressions_workspace_scope";

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

#[derive(QueryableByName)]
struct Suppression {
    #[diesel(sql_type = Integer)]
    workspace_id: i32,
    #[diesel(sql_type = Text)]
    email: String,
}

fn run(conn: &mut PgConnection, sql: &str) {
    diesel::sql_query(sql)
        .execute(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// Two workspaces both mailed an address that then hard-bounced, and a
/// second suppressed address has no outbound history at all.
fn seed(conn: &mut PgConnection) -> i32 {
    let second = diesel::sql_query(
        "INSERT INTO workspaces (slug, name) VALUES ('second', 'Second') RETURNING id",
    )
    .get_result::<Id>(conn)
    .expect("seed a second workspace")
    .id;
    for workspace in [1, second] {
        run(
            conn,
            &format!(
                "INSERT INTO outbound_emails (workspace_id, recipient, subject, body_text, message_id, status) \
                 VALUES ({workspace}, 'bounced@example.org', 'Your request', 'We are on it.', \
                         '<{workspace}@mail.example.com>', 'sent')"
            ),
        );
    }
    run(
        conn,
        "INSERT INTO email_suppressions (email, reason) VALUES \
         ('bounced@example.org', 'hard_bounce'), ('stale@example.org', 'hard_bounce')",
    );
    second
}

#[test]
fn a_suppression_two_workspaces_mailed_is_kept_for_both() {
    let db = FreshDb::new();
    let mut conn = PgConnection::establish(&db.url).expect("connect fresh db");
    run(
        &mut conn,
        "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (\
         version VARCHAR(50) PRIMARY KEY NOT NULL, \
         run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    );
    let mut pending = conn
        .pending_migrations(MIGRATIONS)
        .expect("list pending migrations");
    pending.sort_by_key(|m| m.name().to_string());

    let mut second = None;
    for m in &pending {
        let name = m.name().to_string();
        if name.ends_with(SCOPE_MIGRATION) {
            second = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let second = second.expect("the scoping migration must be in the set");

    let kept: Vec<(i32, String)> = diesel::sql_query(
        "SELECT workspace_id, email FROM email_suppressions ORDER BY workspace_id, email",
    )
    .load::<Suppression>(&mut conn)
    .expect("read suppressions")
    .into_iter()
    .map(|s| (s.workspace_id, s.email))
    .collect();
    assert_eq!(
        kept,
        vec![
            (1, "bounced@example.org".to_string()),
            (second, "bounced@example.org".to_string()),
        ],
        "each workspace that mailed the address keeps its suppression; \
         the address no workspace mailed is dropped"
    );
}
