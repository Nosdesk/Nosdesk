//! People removed from a workspace before membership rows outlived the
//! membership are still that workspace's people.
//!
//! Before 2026-09-09 removing a member deleted their `workspace_members` row,
//! so someone removed under 1.0.x is named by nothing the people definition
//! reads (membership, profiles, tickets, watchers, comments) unless they
//! touched a ticket. Their documentation revisions, activity and assets would
//! then name an unknown user. The migration gives them a removed membership
//! row back. Runs the migration set against a 1.0-style database seeded just
//! before it.

use diesel::prelude::*;
use diesel::sql_types::Integer;
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run `sql` the way the row would already exist: the table's user triggers
/// (audit, seeding) off.
fn existing(conn: &mut PgConnection, table: &str, sql: &str) {
    diesel::sql_query(format!("ALTER TABLE {table} DISABLE TRIGGER USER"))
        .execute(conn)
        .expect("disable triggers");
    diesel::sql_query(sql)
        .execute(conn)
        .unwrap_or_else(|e| panic!("seed {table}: {e}"));
    diesel::sql_query(format!("ALTER TABLE {table} ENABLE TRIGGER USER"))
        .execute(conn)
        .expect("enable triggers");
}

struct Seeded {
    workspace: i32,
    colleague: Uuid,
    former: Uuid,
}

/// A workspace with one current member, and a former one whose membership
/// row is gone but whose documentation revision remains.
fn seed(conn: &mut PgConnection) -> Seeded {
    existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('former-members', 'Former Members')",
    );
    let workspace = diesel::sql_query("SELECT id FROM workspaces WHERE slug = 'former-members'")
        .get_result::<Id>(conn)
        .expect("workspace id")
        .id;
    let (colleague, former) = (Uuid::new_v4(), Uuid::new_v4());
    for (uuid, name) in [
        (colleague, "Current Colleague"),
        (former, "Former Colleague"),
    ] {
        existing(
            conn,
            "users",
            &format!("INSERT INTO users (uuid, name) VALUES ('{uuid}', '{name}')"),
        );
    }
    existing(
        conn,
        "workspace_members",
        &format!(
            "INSERT INTO workspace_members (workspace_id, user_uuid, role) \
             VALUES ({workspace}, '{colleague}', 'admin')"
        ),
    );
    existing(
        conn,
        "documentation_pages",
        &format!(
            "INSERT INTO documentation_pages (workspace_id, title, slug, created_by, last_edited_by) \
             VALUES ({workspace}, 'Printers', 'printers', '{colleague}', '{colleague}')"
        ),
    );
    existing(
        conn,
        "documentation_revisions",
        &format!(
            "INSERT INTO documentation_revisions \
               (workspace_id, page_id, revision_number, title, yjs_document_snapshot, \
                yjs_state_vector, created_by) \
             SELECT {workspace}, id, 1, 'Printers', '\\x00', '\\x00', '{former}' \
             FROM documentation_pages WHERE slug = 'printers'"
        ),
    );
    Seeded {
        workspace,
        colleague,
        former,
    }
}

#[test]
fn a_member_removed_under_1_0_is_still_one_of_the_workspaces_people() {
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

    // Seed just before the backfill, or after everything when there is none.
    let mut seeded = None;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_former_members") && seeded.is_none() {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let seeded = seeded.unwrap_or_else(|| seed(&mut conn));
    drop(conn);

    let pool = diesel::r2d2::Pool::builder()
        .max_size(1)
        .build(backend::db::ResettingManager::new(db.url.clone()))
        .expect("pool on the fresh db");
    let people =
        backend::repository::directory::people(&mut pool.get().expect("conn"), seeded.workspace)
            .expect("the workspace's people");
    assert!(
        people.contains(&seeded.colleague),
        "a current member is one of the workspace's people"
    );
    assert!(
        people.contains(&seeded.former),
        "someone removed under 1.0 who wrote a revision here is too"
    );
}
