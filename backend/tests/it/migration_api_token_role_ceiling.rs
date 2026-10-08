//! Existing API tokens made for someone else get the roles they act with
//! today as their ceilings, so the upgrade locks out no one whose role is
//! unchanged: the holder's current workspace role capped at the maker's, and
//! the holder's platform role when the maker holds the same one. A token made
//! for oneself gets none. Runs the migration set against a database seeded just
//! before the migration.

use diesel::prelude::*;
use diesel::sql_types::{Nullable, Text};
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

/// Run `sql` the way the rows would already exist: the table's user triggers
/// off.
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

struct People {
    admin: Uuid,
    agent: Uuid,
    owner: Uuid,
    platform_holder: Uuid,
}

fn seed(conn: &mut PgConnection) -> People {
    existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('role-ceilings', 'Role Ceilings')",
    );
    let p = People {
        admin: Uuid::new_v4(),
        agent: Uuid::new_v4(),
        owner: Uuid::new_v4(),
        platform_holder: Uuid::new_v4(),
    };
    for (uuid, name, platform) in [
        (p.admin, "Admin", "user"),
        (p.agent, "Agent", "user"),
        (p.owner, "Owner", "user"),
        (p.platform_holder, "Platform", "platform_admin"),
    ] {
        existing(
            conn,
            "users",
            &format!("INSERT INTO users (uuid, name, platform_role) VALUES ('{uuid}', '{name}', '{platform}')"),
        );
    }
    for (uuid, role) in [
        (p.admin, "admin"),
        (p.agent, "agent"),
        (p.owner, "owner"),
        (p.platform_holder, "member"),
    ] {
        existing(
            conn,
            "workspace_members",
            &format!(
                "INSERT INTO workspace_members (workspace_id, user_uuid, role) \
                 SELECT id, '{uuid}', '{role}' FROM workspaces WHERE slug = 'role-ceilings'"
            ),
        );
    }
    // Tokens the admin made: for the agent, for the owner (who now outranks
    // the admin), for a member holding a platform role, and for themselves.
    for (holder, name) in [
        (p.agent, "agent"),
        (p.owner, "owner"),
        (p.platform_holder, "platform"),
        (p.admin, "self"),
    ] {
        existing(
            conn,
            "api_tokens",
            &format!(
                "INSERT INTO api_tokens (token_hash, token_prefix, user_uuid, name, created_by, workspace_id) \
                 SELECT md5(random()::text), 'nsk_test', '{holder}', '{name}', '{admin}', id \
                 FROM workspaces WHERE slug = 'role-ceilings'",
                admin = p.admin
            ),
        );
    }
    p
}

#[derive(QueryableByName)]
struct Ceiling {
    #[diesel(sql_type = Nullable<Text>)]
    role_ceiling: Option<String>,
    #[diesel(sql_type = Nullable<Text>)]
    platform_role_ceiling: Option<String>,
}

fn ceiling(conn: &mut PgConnection, name: &str) -> (Option<String>, Option<String>) {
    let c = diesel::sql_query(format!(
        "SELECT role_ceiling, platform_role_ceiling FROM api_tokens WHERE name = '{name}'"
    ))
    .get_result::<Ceiling>(conn)
    .unwrap_or_else(|e| panic!("token {name}: {e}"));
    (c.role_ceiling, c.platform_role_ceiling)
}

#[test]
fn existing_tokens_get_the_roles_they_act_with() {
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
    let mut seeded = false;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_api_token_role_ceiling") {
            seed(&mut conn);
            seeded = true;
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    assert!(seeded, "the migration ran");

    let s = |v: &str| Some(v.to_string());
    // The agent's token keeps working as an agent.
    assert_eq!(ceiling(&mut conn, "agent"), (s("agent"), s("user")));
    // The owner outranks the admin who made it: capped at admin, as minting
    // would have.
    assert_eq!(ceiling(&mut conn, "owner"), (s("admin"), s("user")));
    // A platform role the maker doesn't hold isn't carried.
    assert_eq!(ceiling(&mut conn, "platform"), (s("member"), s("user")));
    // Made for oneself: no ceiling, it follows one's role.
    assert_eq!(ceiling(&mut conn, "self"), (None, None));
}
