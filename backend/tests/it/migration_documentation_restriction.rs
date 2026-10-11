//! A page or collection shared with someone before restriction was explicit
//! is restricted after the migration, and stays closed when its grants go.
//!
//! Before, "restricted" was read off the grants: a record with any grant was
//! open only to them, and one with none was open to everyone, so deleting the
//! only group a record was shared with opened it. The migration marks every
//! record that has a grant as restricted. Runs the migration set against a
//! database seeded just before it.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{Bool, Integer, Text};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;
use backend::repository::PageAudience;

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

struct Seeded {
    ws: i32,
    outsider: uuid::Uuid,
    group: i32,
    shared_collection: i32,
    open_collection: i32,
    shared_page: i32,
    open_page: i32,
}

/// A collection shared with a group, a page shared with a person, and one of
/// each shared with no one.
fn seed(conn: &mut PgConnection) -> Seeded {
    let ws = existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('restriction', 'Restriction') RETURNING id",
    )
    .expect("workspace");
    let (author, grantee, outsider) = (
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    );
    existing(
        conn,
        "users",
        &format!(
            "INSERT INTO users (uuid, name) VALUES ('{author}', 'Author'), \
             ('{grantee}', 'Grantee'), ('{outsider}', 'Outsider')"
        ),
    );
    existing(
        conn,
        "workspace_members",
        &format!(
            "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at) \
             VALUES ({ws}, '{author}', 'admin', now()), ({ws}, '{grantee}', 'agent', now()), \
                    ({ws}, '{outsider}', 'agent', now())"
        ),
    );
    let group = existing(
        conn,
        "groups",
        &format!("INSERT INTO groups (workspace_id, name) VALUES ({ws}, 'Payroll') RETURNING id"),
    )
    .expect("group");
    let collection = |conn: &mut PgConnection, slug: &str| {
        existing(
            conn,
            "documentation_collections",
            &format!(
                "INSERT INTO documentation_collections (workspace_id, name, slug) \
                 VALUES ({ws}, '{slug}', '{slug}') RETURNING id"
            ),
        )
        .expect("collection")
    };
    let shared_collection = collection(conn, "payroll");
    let open_collection = collection(conn, "handbook");
    existing(
        conn,
        "documentation_collection_visibility",
        &format!(
            "INSERT INTO documentation_collection_visibility (workspace_id, collection_id, group_id) \
             VALUES ({ws}, {shared_collection}, {group})"
        ),
    );
    let page = |conn: &mut PgConnection, slug: &str| {
        existing(
            conn,
            "documentation_pages",
            &format!(
                "INSERT INTO documentation_pages (workspace_id, title, slug, created_by, last_edited_by) \
                 VALUES ({ws}, '{slug}', '{slug}', '{author}', '{author}') RETURNING id"
            ),
        )
        .expect("page")
    };
    let shared_page = page(conn, "salaries");
    let open_page = page(conn, "holidays");
    existing(
        conn,
        "documentation_page_visibility",
        &format!(
            "INSERT INTO documentation_page_visibility (workspace_id, page_id, user_uuid) \
             VALUES ({ws}, {shared_page}, '{grantee}')"
        ),
    );
    Seeded {
        ws,
        outsider,
        group,
        shared_collection,
        open_collection,
        shared_page,
        open_page,
    }
}

#[derive(QueryableByName)]
struct Flag {
    #[diesel(sql_type = Text)]
    kind: String,
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Bool)]
    restricted: bool,
}

#[test]
fn a_record_shared_before_the_migration_is_restricted_after_it() {
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
        if name.contains("_documentation_explicit_restriction") && seeded.is_none() {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let s = seeded.unwrap_or_else(|| seed(&mut conn));

    let flags: Vec<Flag> = diesel::sql_query(
        "SELECT 'collection' AS kind, id, restricted FROM documentation_collections \
         UNION ALL SELECT 'page' AS kind, id, restricted FROM documentation_pages \
         ORDER BY kind, id",
    )
    .load(&mut conn)
    .expect("restriction flags");
    let restricted = |kind: &str, id: i32| {
        flags
            .iter()
            .find(|f| f.kind == kind && f.id == id)
            .map(|f| f.restricted)
    };
    assert_eq!(restricted("collection", s.shared_collection), Some(true));
    assert_eq!(restricted("collection", s.open_collection), Some(false));
    assert_eq!(restricted("page", s.shared_page), Some(true));
    assert_eq!(restricted("page", s.open_page), Some(false));

    // The group goes, and its grant with it: the collection stays closed.
    diesel::sql_query(format!(
        "SELECT set_config('app.workspace_id', '{}', false)",
        s.ws
    ))
    .execute(&mut conn)
    .expect("pin workspace for the delete");
    diesel::sql_query(format!("DELETE FROM groups WHERE id = {}", s.group))
        .execute(&mut conn)
        .expect("delete the group");
    drop(conn);

    // Read as the app does, pinned to the workspace.
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .test_on_check_out(false)
        .build(backend::db::ResettingManager::new(&db.url))
        .expect("pool");
    let mut conn = pool.get().expect("conn");
    diesel::sql_query(format!(
        "SELECT set_config('app.workspace_id', '{}', false)",
        s.ws
    ))
    .execute(&mut conn)
    .expect("pin workspace");
    let outsider = PageAudience::User {
        user_uuid: s.outsider,
        is_admin: false,
    };
    let (hidden_pages, hidden_collections) = outsider
        .hidden(
            &mut conn,
            &[s.shared_page, s.open_page],
            &[s.shared_collection, s.open_collection],
        )
        .expect("hidden");
    assert!(hidden_collections.contains(&s.shared_collection));
    assert!(!hidden_collections.contains(&s.open_collection));
    assert!(hidden_pages.contains(&s.shared_page));
    assert!(!hidden_pages.contains(&s.open_page));
}
