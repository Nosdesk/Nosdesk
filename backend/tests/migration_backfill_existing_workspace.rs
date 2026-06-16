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
