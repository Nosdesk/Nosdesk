//! Asset kinds and the user field schema get a field order from the order
//! clients have always shown their fields in: the schema's keys in byte
//! order, as the API's sorted map writes them.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text};
use diesel_migrations::MigrationHarness;
use serde_json::{json, Value};

use backend::db::MIGRATIONS;
use backend::services::custom_fields::order::normalise_order;

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

/// Keys whose byte order differs from jsonb's (length first) and from a
/// case-blind sort.
fn kind_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "b": { "type": "string" },
            "aa": { "type": "string" },
            "Zed": { "type": "string" },
            "a_b": { "type": "string" },
            "a1": { "type": "string" }
        }
    })
}

fn user_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "year_level": { "type": "string" },
            "house": { "type": "string" },
            "Campus": { "type": "string" }
        }
    })
}

struct Seeded {
    kind: i32,
    empty_kind: i32,
    workspace: i32,
    audited_before: i64,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

fn audited(conn: &mut PgConnection) -> i64 {
    diesel::sql_query(
        "SELECT count(*) AS n FROM audit_log \
         WHERE table_name IN ('asset_kinds', 'user_field_schema')",
    )
    .get_result::<Count>(conn)
    .expect("count audit rows")
    .n
}

fn seed(conn: &mut PgConnection) -> Seeded {
    let workspace = existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('order-ws', 'order-ws') RETURNING id",
    );
    let kind = existing(
        conn,
        "asset_kinds",
        &format!(
            "INSERT INTO asset_kinds (workspace_id, slug, label, attribute_schema, sort_order, \
                 is_builtin, category) \
             VALUES ({workspace}, 'rig', 'Rig', '{}'::jsonb, 10, false, 'physical') RETURNING id",
            kind_schema()
        ),
    );
    let empty_kind = existing(
        conn,
        "asset_kinds",
        &format!(
            "INSERT INTO asset_kinds (workspace_id, slug, label, attribute_schema, sort_order, \
                 is_builtin, category) \
             VALUES ({workspace}, 'bare', 'Bare', '{{\"type\": \"object\"}}'::jsonb, 20, false, \
                 'generic') RETURNING id"
        ),
    );
    existing(
        conn,
        "user_field_schema",
        &format!(
            "INSERT INTO user_field_schema (workspace_id, schema) \
             VALUES ({workspace}, '{}'::jsonb) RETURNING workspace_id AS id",
            user_schema()
        ),
    );
    Seeded {
        kind,
        empty_kind,
        workspace,
        audited_before: audited(conn),
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
        if name.contains("_attribute_field_order") {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    (db, conn, seeded.expect("the migration ran"))
}

fn order_of_kind(conn: &mut PgConnection, id: i32) -> Vec<String> {
    use backend::schema::asset_kinds;
    asset_kinds::table
        .find(id)
        .select(asset_kinds::field_order)
        .first::<Vec<Option<String>>>(conn)
        .expect("kind order")
        .into_iter()
        .flatten()
        .collect()
}

#[test]
fn the_backfill_keeps_the_order_clients_show() {
    let (_db, mut conn, s) = upgrade();

    let order = order_of_kind(&mut conn, s.kind);
    assert_eq!(order, ["Zed", "a1", "a_b", "aa", "b"]);
    assert_eq!(order, normalise_order(&kind_schema(), &[]));
    assert!(order_of_kind(&mut conn, s.empty_kind).is_empty());

    use backend::schema::user_field_schema;
    let user_order: Vec<String> = user_field_schema::table
        .find(s.workspace)
        .select(user_field_schema::field_order)
        .first::<Vec<Option<String>>>(&mut conn)
        .expect("user order")
        .into_iter()
        .flatten()
        .collect();
    assert_eq!(user_order, ["Campus", "house", "year_level"]);
    assert_eq!(user_order, normalise_order(&user_schema(), &[]));

    assert_eq!(audited(&mut conn), s.audited_before, "no audit rows");

    #[derive(QueryableByName)]
    struct Trigger {
        #[diesel(sql_type = Text)]
        tgname: String,
        #[diesel(sql_type = Text)]
        enabled: String,
    }
    let triggers = diesel::sql_query(
        "SELECT tgname::text, tgenabled::text AS enabled FROM pg_trigger \
         WHERE tgname IN ('tr_audit_asset_kinds', 'tr_audit_user_field_schema') \
         ORDER BY tgname",
    )
    .load::<Trigger>(&mut conn)
    .expect("read triggers");
    assert_eq!(triggers.len(), 2);
    for t in triggers {
        assert_eq!(t.enabled, "O", "{} is on", t.tgname);
    }
}
