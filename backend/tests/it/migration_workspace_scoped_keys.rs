//! The migration that adds the workspace to every key between workspace tables
//! cleans up rows that already reference another workspace's row before the
//! keys are validated: a ticket's workflow state moves to the matching state in
//! its own workspace, a nullable reference (a self-reference included) is
//! cleared, and a row whose required reference crosses is deleted. Rows that
//! stay in their own workspace are untouched. Runs the full migration set
//! against a database seeded just before that migration, with audit triggers
//! active as in production.

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Nullable};
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use backend::db::MIGRATIONS;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

#[derive(QueryableByName)]
struct MaybeId {
    #[diesel(sql_type = Nullable<Integer>)]
    id: Option<i32>,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Insert a row the way it would already exist, with the table's user
/// triggers off, and return its id.
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

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

fn maybe_id(conn: &mut PgConnection, sql: &str) -> Option<i32> {
    diesel::sql_query(sql)
        .get_result::<MaybeId>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .id
}

/// Ids of the seeded rows, for the assertions after the migration.
struct Seeded {
    a_backlog: i32,
    b_backlog: i32,
    state_crossed: i32,
    category_crossed: i32,
    b_ticket: i32,
    comment_crossed: i32,
    comment_own: i32,
    a_tag: i32,
    b_tag: i32,
    page_crossed: i32,
}

fn seed(conn: &mut PgConnection) -> Seeded {
    let user = Uuid::new_v4();
    for sql in [
        "ALTER TABLE users DISABLE TRIGGER USER".to_string(),
        format!("INSERT INTO users (uuid, name) VALUES ('{user}', 'Seeder')"),
        "ALTER TABLE users ENABLE TRIGGER USER".to_string(),
    ] {
        diesel::sql_query(sql).execute(conn).expect("seed user");
    }

    let workspace = |conn: &mut PgConnection, slug: &str| {
        existing(
            conn,
            "workspaces",
            &format!(
                "INSERT INTO workspaces (slug, name) VALUES ('{slug}', '{slug}') RETURNING id"
            ),
        )
    };
    let a = workspace(conn, "keys-a");
    let b = workspace(conn, "keys-b");

    let backlog = |conn: &mut PgConnection, ws: i32| {
        existing(
            conn,
            "workflow_states",
            &format!(
                "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
                 VALUES ({ws}, 'Backlog', 'backlog', 'gray', 0, true) RETURNING id"
            ),
        )
    };
    let a_backlog = backlog(conn, a);
    let b_backlog = backlog(conn, b);

    let b_category = existing(
        conn,
        "ticket_categories",
        &format!("INSERT INTO ticket_categories (workspace_id, name) VALUES ({b}, 'Hardware') RETURNING id"),
    );
    let tag = |conn: &mut PgConnection, ws: i32, name: &str| {
        existing(
            conn,
            "tags",
            &format!("INSERT INTO tags (workspace_id, name) VALUES ({ws}, '{name}') RETURNING id"),
        )
    };
    let a_tag = tag(conn, a, "urgent");
    let b_tag = tag(conn, b, "vip");

    let ticket = |conn: &mut PgConnection, ws: i32, state: i32, category: Option<i32>| {
        let category = category.map_or("NULL".to_string(), |c| c.to_string());
        existing(
            conn,
            "tickets",
            &format!(
                "INSERT INTO tickets (workspace_id, title, workflow_state_id, category_id) \
                 VALUES ({ws}, 'Printer jammed', {state}, {category}) RETURNING id"
            ),
        )
    };
    let state_crossed = ticket(conn, a, b_backlog, None);
    let category_crossed = ticket(conn, a, a_backlog, Some(b_category));
    let b_ticket = ticket(conn, b, b_backlog, None);

    let comment = |conn: &mut PgConnection, ws: i32, ticket: i32| {
        existing(
            conn,
            "comments",
            &format!(
                "INSERT INTO comments (workspace_id, content, ticket_id, user_uuid) \
                 VALUES ({ws}, '<p>note</p>', {ticket}, '{user}') RETURNING id"
            ),
        )
    };
    let comment_crossed = comment(conn, a, b_ticket);
    let comment_own = comment(conn, a, state_crossed);

    for tag in [a_tag, b_tag] {
        diesel::sql_query("ALTER TABLE ticket_tags DISABLE TRIGGER USER")
            .execute(conn)
            .expect("disable triggers");
        diesel::sql_query(format!(
            "INSERT INTO ticket_tags (workspace_id, ticket_id, tag_id) VALUES ({a}, {state_crossed}, {tag})"
        ))
        .execute(conn)
        .expect("seed ticket tag");
        diesel::sql_query("ALTER TABLE ticket_tags ENABLE TRIGGER USER")
            .execute(conn)
            .expect("enable triggers");
    }

    let page = |conn: &mut PgConnection, ws: i32, slug: &str, parent: Option<i32>| {
        let parent = parent.map_or("NULL".to_string(), |p| p.to_string());
        existing(
            conn,
            "documentation_pages",
            &format!(
                "INSERT INTO documentation_pages (workspace_id, title, slug, created_by, last_edited_by, parent_id) \
                 VALUES ({ws}, '{slug}', '{slug}', '{user}', '{user}', {parent}) RETURNING id"
            ),
        )
    };
    let b_page = page(conn, b, "b-handbook", None);
    let page_crossed = page(conn, a, "a-child", Some(b_page));

    Seeded {
        a_backlog,
        b_backlog,
        state_crossed,
        category_crossed,
        b_ticket,
        comment_crossed,
        comment_own,
        a_tag,
        b_tag,
        page_crossed,
    }
}

#[test]
fn rows_referencing_another_workspace_are_repaired_before_the_keys_validate() {
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
        if name.contains("_workspace_scoped_foreign_keys") && seeded.is_none() {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let s = seeded.expect("the migration must be in the set");

    let state_of = |conn: &mut PgConnection, ticket: i32| {
        maybe_id(
            conn,
            &format!("SELECT workflow_state_id AS id FROM tickets WHERE id = {ticket}"),
        )
    };
    assert_eq!(
        state_of(&mut conn, s.state_crossed),
        Some(s.a_backlog),
        "moved to A's state"
    );
    assert_eq!(
        state_of(&mut conn, s.b_ticket),
        Some(s.b_backlog),
        "B's own ticket untouched"
    );
    assert_eq!(
        maybe_id(
            &mut conn,
            &format!(
                "SELECT category_id AS id FROM tickets WHERE id = {}",
                s.category_crossed
            )
        ),
        None,
        "the crossed category is cleared, the ticket kept"
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM comments WHERE id = {}",
                s.comment_crossed
            )
        ),
        0,
        "a comment on another workspace's ticket is deleted"
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM comments WHERE id = {}",
                s.comment_own
            )
        ),
        1
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM ticket_tags WHERE ticket_id = {} AND tag_id = {}",
                s.state_crossed, s.b_tag
            )
        ),
        0,
        "a tag from another workspace is removed"
    );
    assert_eq!(
        count(
            &mut conn,
            &format!(
                "SELECT count(*) AS n FROM ticket_tags WHERE ticket_id = {} AND tag_id = {}",
                s.state_crossed, s.a_tag
            )
        ),
        1
    );
    assert_eq!(
        maybe_id(
            &mut conn,
            &format!(
                "SELECT parent_id AS id FROM documentation_pages WHERE id = {}",
                s.page_crossed
            )
        ),
        None,
        "a parent page in another workspace is cleared"
    );

    // Every key between workspace tables now includes the workspace, validated.
    let unvalidated = count(
        &mut conn,
        "SELECT count(*) AS n FROM pg_constraint con \
         WHERE con.contype = 'f' AND con.connamespace = 'public'::regnamespace \
           AND array_length(con.conkey, 1) = 2 AND NOT con.convalidated",
    );
    assert_eq!(unvalidated, 0);
}
