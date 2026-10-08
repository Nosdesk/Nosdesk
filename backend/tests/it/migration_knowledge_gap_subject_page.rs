//! A stale-doc gap names its page by id, not by title.
//!
//! Stale-doc gaps were titled "Doc may be stale: {page title}" and their
//! signals carried the page's title and slug, so the queue named a page to
//! whoever could read gaps, whether or not they could open the page. The
//! migration that adds `knowledge_gaps.subject_page_id` moves existing gaps
//! over: the page id goes in the column, the title loses the page's name and
//! the signal payload its title and slug. Runs the migration set against a
//! database seeded just before it.

use diesel::prelude::*;
use diesel::sql_types::{Integer, Jsonb, Nullable, Text};
use diesel_migrations::MigrationHarness;

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

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// A workspace with one stale page and the gap detection raised about it,
/// as detection used to write them.
fn seed(conn: &mut PgConnection) -> i32 {
    existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('stale-docs', 'Stale Docs')",
    );
    let ws = diesel::sql_query("SELECT id FROM workspaces WHERE slug = 'stale-docs'")
        .get_result::<Id>(conn)
        .expect("workspace")
        .id;
    let author = uuid::Uuid::new_v4();
    existing(
        conn,
        "users",
        &format!("INSERT INTO users (uuid, name) VALUES ('{author}', 'Author')"),
    );
    existing(
        conn,
        "documentation_pages",
        &format!(
            "INSERT INTO documentation_pages (workspace_id, title, slug, created_by, last_edited_by) \
             VALUES ({ws}, 'Payroll Export Runbook', 'payroll-export-runbook', '{author}', '{author}')"
        ),
    );
    existing(
        conn,
        "knowledge_gaps",
        &format!(
            "INSERT INTO knowledge_gaps (workspace_id, title, status) \
             VALUES ({ws}, 'Doc may be stale: Payroll Export Runbook', 'open')"
        ),
    );
    existing(
        conn,
        "knowledge_gap_signals",
        &format!(
            "INSERT INTO knowledge_gap_signals \
               (workspace_id, gap_id, signal_type, source_kind, source_ref, payload, confidence) \
             SELECT {ws}, g.id, 'stale_doc', 'page', p.id::text, \
                    jsonb_build_object('page_title', p.title, 'page_slug', p.slug, \
                                       'page_uuid', p.uuid, 'days_stale', 70), 50 \
             FROM knowledge_gaps g, documentation_pages p \
             WHERE g.workspace_id = {ws} AND p.workspace_id = {ws}"
        ),
    );
    ws
}

#[derive(QueryableByName)]
struct Gap {
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Jsonb)]
    payload: serde_json::Value,
}

#[derive(QueryableByName)]
struct Subject {
    #[diesel(sql_type = Nullable<Integer>)]
    subject_page_id: Option<i32>,
    #[diesel(sql_type = Integer)]
    page_id: i32,
}

#[test]
fn existing_stale_doc_gaps_name_their_page_by_id() {
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

    // Seed just before the migration, or after everything when there is none.
    let mut seeded = None;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_knowledge_gap_subject_page") && seeded.is_none() {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let ws = seeded.unwrap_or_else(|| seed(&mut conn));

    let gap = diesel::sql_query(format!(
        "SELECT g.title, s.payload FROM knowledge_gaps g \
         JOIN knowledge_gap_signals s ON s.gap_id = g.id WHERE g.workspace_id = {ws}"
    ))
    .get_result::<Gap>(&mut conn)
    .expect("the gap and its signal");
    assert_eq!(
        gap.title, "Doc may be stale",
        "the title no longer names the page"
    );
    assert!(
        gap.payload.get("page_title").is_none() && gap.payload.get("page_slug").is_none(),
        "the signal no longer carries the page's title or slug: {}",
        gap.payload
    );
    assert_eq!(
        gap.payload["days_stale"], 70,
        "the rest of the payload stays"
    );

    let subject = diesel::sql_query(format!(
        "SELECT g.subject_page_id, p.id AS page_id FROM knowledge_gaps g, documentation_pages p \
         WHERE g.workspace_id = {ws} AND p.workspace_id = {ws}"
    ))
    .get_result::<Subject>(&mut conn)
    .expect("the gap's subject");
    assert_eq!(
        subject.subject_page_id,
        Some(subject.page_id),
        "the gap names its page by id"
    );
}
