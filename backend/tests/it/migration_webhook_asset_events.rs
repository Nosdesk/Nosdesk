//! Webhooks subscribed to `device.*` move to the `asset.*` names delivery
//! uses, and the audit trigger on `webhooks` is back on afterwards. Runs the
//! migration set against a database seeded just before the rename.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{Array, Text};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

const RENAME: &str = "2026-10-14-010000_webhook_asset_event_names";

#[derive(QueryableByName)]
struct Events {
    #[diesel(sql_type = Array<Text>)]
    events: Vec<String>,
}

#[derive(QueryableByName)]
struct Enabled {
    #[diesel(sql_type = Text)]
    tgenabled: String,
}

fn run(conn: &mut PgConnection, sql: &str) {
    diesel::sql_query(sql)
        .execute(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

fn events_of(conn: &mut PgConnection, name: &str) -> Vec<String> {
    diesel::sql_query("SELECT events FROM webhooks WHERE name = $1")
        .bind::<Text, _>(name)
        .get_result::<Events>(conn)
        .unwrap_or_else(|e| panic!("webhook {name}: {e}"))
        .events
}

#[test]
fn device_subscriptions_become_asset_subscriptions() {
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
        if name == RENAME {
            run(&mut conn, "ALTER TABLE workspaces DISABLE TRIGGER USER");
            run(
                &mut conn,
                "INSERT INTO workspaces (id, slug, name) VALUES (900, 'hooks', 'Hooks')",
            );
            run(&mut conn, "ALTER TABLE workspaces ENABLE TRIGGER USER");
            run(&mut conn, "ALTER TABLE webhooks DISABLE TRIGGER USER");
            for (hook, events) in [
                ("devices", "{ticket.created,device.created,device.linked}"),
                ("both", "{device.updated,asset.updated}"),
                ("tickets", "{ticket.updated}"),
            ] {
                run(
                    &mut conn,
                    &format!(
                        "INSERT INTO webhooks (workspace_id, name, url, secret, events) \
                         VALUES (900, '{hook}', 'https://example.test/{hook}', 's', '{events}')"
                    ),
                );
            }
            run(&mut conn, "ALTER TABLE webhooks ENABLE TRIGGER USER");
            seeded = true;
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    assert!(seeded, "{RENAME} must be in the migration set");

    assert_eq!(
        events_of(&mut conn, "devices"),
        ["asset.created", "asset.linked", "ticket.created"]
    );
    assert_eq!(
        events_of(&mut conn, "both"),
        ["asset.updated"],
        "a name held twice is kept once"
    );
    assert_eq!(events_of(&mut conn, "tickets"), ["ticket.updated"]);

    let enabled = diesel::sql_query(
        "SELECT tgenabled::text AS tgenabled FROM pg_trigger WHERE tgname = 'tr_audit_webhooks'",
    )
    .get_result::<Enabled>(&mut conn)
    .expect("read the audit trigger")
    .tgenabled;
    assert_eq!(enabled, "O", "the audit trigger is on again");
}
