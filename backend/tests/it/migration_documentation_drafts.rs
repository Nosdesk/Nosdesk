//! Upgrading to staff-only drafts hides nothing someone outside staff reads
//! today: the migration publishes every draft a requester or a guest can open,
//! and leaves the drafts open to staff only as drafts. Runs the migration set
//! against a database seeded just before it.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{Integer, Text};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run `sql` with the table's user triggers off, as if the rows already
/// existed. Returns the id it hands back, if any.
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

/// Page ids by what the migration should do with them.
struct Seeded {
    published: Vec<(&'static str, i32)>,
    kept: Vec<(&'static str, i32)>,
}

fn seed(conn: &mut PgConnection) -> Seeded {
    let ws = existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('drafts', 'Drafts') RETURNING id",
    )
    .expect("workspace");
    let (admin, agent, requester) = (
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    );
    existing(
        conn,
        "users",
        &format!(
            "INSERT INTO users (uuid, name) VALUES ('{admin}', 'Admin'), \
             ('{agent}', 'Agent'), ('{requester}', 'Requester')"
        ),
    );
    existing(
        conn,
        "workspace_members",
        &format!(
            "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at) \
             VALUES ({ws}, '{admin}', 'admin', now()), ({ws}, '{agent}', 'agent', now()), \
                    ({ws}, '{requester}', 'member', now())"
        ),
    );
    let group = |conn: &mut PgConnection, name: &str| {
        existing(
            conn,
            "groups",
            &format!(
                "INSERT INTO groups (workspace_id, name) VALUES ({ws}, '{name}') RETURNING id"
            ),
        )
        .expect("group")
    };
    let staff_group = group(conn, "IT");
    let requester_group = group(conn, "Finance");
    let including_group = group(conn, "Everyone");
    existing(
        conn,
        "user_groups",
        &format!(
            "INSERT INTO user_groups (workspace_id, user_uuid, group_id) \
             VALUES ({ws}, '{agent}', {staff_group}), ({ws}, '{requester}', {requester_group})"
        ),
    );
    existing(
        conn,
        "group_includes",
        &format!(
            "INSERT INTO group_includes (workspace_id, parent_group_id, child_group_id) \
             VALUES ({ws}, {including_group}, {requester_group})"
        ),
    );
    let collection = |conn: &mut PgConnection, slug: &str, restricted: bool| {
        existing(
            conn,
            "documentation_collections",
            &format!(
                "INSERT INTO documentation_collections (workspace_id, name, slug, restricted) \
                 VALUES ({ws}, '{slug}', '{slug}', {restricted}) RETURNING id"
            ),
        )
        .expect("collection")
    };
    let open_collection = collection(conn, "handbook", false);
    let staff_collection = collection(conn, "runbooks", true);
    let requester_collection = collection(conn, "finance", true);
    existing(
        conn,
        "documentation_collection_visibility",
        &format!(
            "INSERT INTO documentation_collection_visibility (workspace_id, collection_id, group_id) \
             VALUES ({ws}, {staff_collection}, {staff_group}), \
                    ({ws}, {requester_collection}, {including_group})"
        ),
    );
    let page = |conn: &mut PgConnection,
                slug: &str,
                status: &str,
                public: bool,
                restricted: bool| {
        existing(
            conn,
            "documentation_pages",
            &format!(
                "INSERT INTO documentation_pages \
                 (workspace_id, title, slug, status, is_public, restricted, created_by, last_edited_by) \
                 VALUES ({ws}, '{slug}', '{slug}', '{status}', {public}, {restricted}, \
                         '{admin}', '{admin}') RETURNING id"
            ),
        )
        .expect("page")
    };
    let in_collection = |conn: &mut PgConnection, page: i32, collection: i32| {
        existing(
            conn,
            "documentation_collection_pages",
            &format!(
                "INSERT INTO documentation_collection_pages (workspace_id, collection_id, page_id) \
                 VALUES ({ws}, {collection}, {page})"
            ),
        );
    };
    let page_grant = |conn: &mut PgConnection, page: i32, column: &str, value: String| {
        existing(
            conn,
            "documentation_page_visibility",
            &format!(
                "INSERT INTO documentation_page_visibility (workspace_id, page_id, {column}) \
                 VALUES ({ws}, {page}, {value})"
            ),
        );
    };

    let public_staff_only = page(conn, "public-runbook", "draft", true, true);
    let open = page(conn, "open", "draft", false, false);
    let in_open = page(conn, "in-open", "draft", false, false);
    in_collection(conn, in_open, open_collection);
    let in_requester_collection = page(conn, "in-finance", "draft", false, false);
    in_collection(conn, in_requester_collection, requester_collection);
    let granted_requester = page(conn, "to-requester", "draft", false, true);
    page_grant(
        conn,
        granted_requester,
        "user_uuid",
        format!("'{requester}'"),
    );
    let granted_group = page(conn, "to-finance", "draft", false, true);
    page_grant(conn, granted_group, "group_id", requester_group.to_string());

    let in_staff_collection = page(conn, "in-runbooks", "draft", false, false);
    in_collection(conn, in_staff_collection, staff_collection);
    let granted_agent = page(conn, "to-agent", "draft", false, true);
    page_grant(conn, granted_agent, "user_uuid", format!("'{agent}'"));
    let granted_staff_group = page(conn, "to-it", "draft", false, true);
    page_grant(
        conn,
        granted_staff_group,
        "group_id",
        staff_group.to_string(),
    );
    let admins_only = page(conn, "admins-only", "draft", false, true);
    let archived = page(conn, "archived", "archived", false, false);
    let trashed = page(conn, "trashed", "deleted", true, false);

    Seeded {
        published: vec![
            (
                "a public draft, even one only staff can open",
                public_staff_only,
            ),
            ("an open draft in no collection", open),
            ("an open draft in an open collection", in_open),
            (
                "a draft in a collection a requester's group reaches",
                in_requester_collection,
            ),
            ("a draft shared with a requester", granted_requester),
            ("a draft shared with a requester's group", granted_group),
        ],
        kept: vec![
            ("a draft in a staff-only collection", in_staff_collection),
            ("a draft shared with an agent", granted_agent),
            ("a draft shared with a staff group", granted_staff_group),
            ("a restricted draft shared with no one", admins_only),
            ("an archived page", archived),
            ("a page in the trash", trashed),
        ],
    }
}

#[derive(QueryableByName)]
struct Status {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    status: String,
}

#[test]
fn the_upgrade_publishes_every_draft_someone_outside_staff_reads() {
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
    let mut before = Vec::new();
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_publish_reachable_drafts") && seeded.is_none() {
            let s = seed(&mut conn);
            before = statuses(&mut conn);
            seeded = Some(s);
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let s = seeded.expect("the migration ran");
    let after = statuses(&mut conn);
    let status = |rows: &[Status], id: i32| {
        rows.iter()
            .find(|r| r.id == id)
            .map(|r| r.status.clone())
            .expect("page")
    };

    for (what, id) in &s.published {
        assert_eq!(status(&after, *id), "published", "{what}");
    }
    for (what, id) in &s.kept {
        assert_eq!(status(&after, *id), status(&before, *id), "{what}");
    }
}

fn statuses(conn: &mut PgConnection) -> Vec<Status> {
    diesel::sql_query("SELECT id, status::text AS status FROM documentation_pages")
        .load(conn)
        .expect("statuses")
}
