//! Upgrading from 1.0.12 renames a workspace whose slug 1.1 reserves, instead
//! of failing the migration at boot. Runs every migration against a database
//! seeded at the 1.0.12 boundary.

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Text};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

/// The last migration 1.0.12 shipped.
const LAST_IN_1_0_12: &str = "2026-06-18-100000_asset_serial_blank_is_null";

#[derive(QueryableByName)]
struct Slug {
    #[diesel(sql_type = Text)]
    slug: String,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Validated {
    #[diesel(sql_type = Bool)]
    convalidated: bool,
}

fn run(conn: &mut PgConnection, sql: &str) {
    diesel::sql_query(sql)
        .execute(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

/// The slug of the workspace named `name`.
fn slug_of(conn: &mut PgConnection, name: &str) -> String {
    diesel::sql_query("SELECT slug::text AS slug FROM workspaces WHERE name = $1")
        .bind::<Text, _>(name)
        .get_result::<Slug>(conn)
        .unwrap_or_else(|e| panic!("workspace {name}: {e}"))
        .slug
}

#[test]
fn a_workspace_on_a_newly_reserved_slug_is_renamed_on_upgrade() {
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

    let mut seeded = false;
    for m in &pending {
        let name = m.name().to_string();
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
        if name == LAST_IN_1_0_12 {
            // Slugs 1.0.12 allowed and 1.1 reserves, one that keeps its slug,
            // and the names a rename must step past: a taken slug and a
            // retired one.
            for (slug, name) in [
                ("teams", "Teams"),
                ("inbound", "Inbound"),
                ("widget", "Widget"),
                ("widget-workspace", "Widget workspace"),
                ("feedback", "Feedback"),
                ("acme", "Acme"),
            ] {
                run(
                    &mut conn,
                    &format!("INSERT INTO workspaces (slug, name) VALUES ('{slug}', '{name}')"),
                );
            }
            run(
                &mut conn,
                "INSERT INTO retired_slugs (slug, workspace_uuid) \
                 VALUES ('feedback-workspace', gen_random_uuid())",
            );
            seeded = true;
        }
    }
    assert!(seeded, "{LAST_IN_1_0_12} must be in the migration set");

    let renamed: Vec<(&str, String)> = ["Teams", "Inbound", "Widget", "Feedback", "Acme"]
        .into_iter()
        .map(|name| (name, slug_of(&mut conn, name)))
        .collect();
    assert_eq!(
        renamed,
        vec![
            ("Teams", "teams-workspace".to_string()),
            ("Inbound", "inbound-workspace".to_string()),
            ("Widget", "widget-2".to_string()),
            ("Feedback", "feedback-2".to_string()),
            ("Acme", "acme".to_string()),
        ],
        "a newly reserved slug gets -workspace, or the next free -N"
    );
    assert_eq!(slug_of(&mut conn, "Widget workspace"), "widget-workspace");

    let count = diesel::sql_query(
        "SELECT count(*) AS n FROM workspaces \
         WHERE name IN ('Teams', 'Inbound', 'Widget', 'Widget workspace', 'Feedback', 'Acme')",
    )
    .get_result::<Count>(&mut conn)
    .expect("count workspaces")
    .n;
    assert_eq!(count, 6, "renaming keeps every workspace");

    let validated = diesel::sql_query(
        "SELECT convalidated FROM pg_constraint WHERE conname = 'workspaces_slug_not_reserved'",
    )
    .get_result::<Validated>(&mut conn)
    .expect("read the reserved-slug constraint")
    .convalidated;
    assert!(validated, "the reserved-slug constraint ends up validated");
}
