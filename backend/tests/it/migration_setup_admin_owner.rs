//! First-run setup makes its administrator the bootstrap workspace's owner.
//! An install set up before that has an admin there and no owner, so the
//! upgrade makes its setup administrator the owner: the earliest live
//! platform admin who is a current admin of the workspace. A workspace that
//! already has an owner is left as it is. Runs the migration set against a
//! database seeded just before the migration.

use diesel::prelude::*;
use diesel::sql_types::Text;
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

/// A person in the bootstrap workspace: their platform role, when their
/// account was made (days ago), their workspace role, and whether they were
/// removed from it.
struct Person {
    uuid: Uuid,
    platform: &'static str,
    days_ago: i32,
    role: &'static str,
    removed: bool,
}

fn person(platform: &'static str, days_ago: i32, role: &'static str, removed: bool) -> Person {
    Person {
        uuid: Uuid::new_v4(),
        platform,
        days_ago,
        role,
        removed,
    }
}

fn seed(conn: &mut PgConnection, people: &[Person]) {
    for p in people {
        existing(
            conn,
            "users",
            &format!(
                "INSERT INTO users (uuid, name, platform_role, created_at) \
                 VALUES ('{}', 'Person', '{}', now() - interval '{} days')",
                p.uuid, p.platform, p.days_ago
            ),
        );
        existing(
            conn,
            "workspace_members",
            &format!(
                "INSERT INTO workspace_members (workspace_id, user_uuid, role, removed_at) \
                 VALUES (1, '{}', '{}', {})",
                p.uuid,
                p.role,
                if p.removed { "now()" } else { "NULL" }
            ),
        );
    }
}

/// Migrate a fresh database, seeding `people` just before the migration.
fn upgrade(people: &[Person]) -> (FreshDb, PgConnection) {
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
        if name.contains("_setup_admin_owns_workspace") {
            seed(&mut conn, people);
            seeded = true;
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    assert!(seeded, "the migration ran");
    (db, conn)
}

#[derive(QueryableByName)]
struct Role {
    #[diesel(sql_type = Text)]
    role: String,
}

fn role(conn: &mut PgConnection, p: &Person) -> String {
    diesel::sql_query(format!(
        "SELECT role FROM workspace_members WHERE workspace_id = 1 AND user_uuid = '{}'",
        p.uuid
    ))
    .get_result::<Role>(conn)
    .expect("membership")
    .role
}

#[test]
fn the_setup_administrator_owns_a_workspace_with_no_owner() {
    let removed = person("platform_admin", 40, "admin", true);
    let plain_admin = person("user", 35, "admin", false);
    let setup = person("platform_admin", 30, "admin", false);
    let later = person("platform_admin", 10, "admin", false);
    let agent = person("user", 20, "agent", false);
    let people = [removed, plain_admin, setup, later, agent];
    let (_db, mut conn) = upgrade(&people);

    let [removed, plain_admin, setup, later, agent] = &people;
    assert_eq!(role(&mut conn, setup), "owner", "the setup administrator");
    assert_eq!(
        role(&mut conn, removed),
        "admin",
        "a removed member stays out"
    );
    assert_eq!(
        role(&mut conn, plain_admin),
        "admin",
        "an admin without the platform role"
    );
    assert_eq!(role(&mut conn, later), "admin", "only one owner is made");
    assert_eq!(role(&mut conn, agent), "agent");
}

#[test]
fn a_workspace_with_an_owner_keeps_its_roles() {
    let setup = person("platform_admin", 30, "admin", false);
    let owner = person("user", 20, "owner", false);
    let people = [setup, owner];
    let (_db, mut conn) = upgrade(&people);

    let [setup, owner] = &people;
    assert_eq!(role(&mut conn, setup), "admin");
    assert_eq!(role(&mut conn, owner), "owner");
}
