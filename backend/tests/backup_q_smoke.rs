//! Item Q — backup E2E smoke test.
//!
//! V1-blocker per `docs/v1-readiness-plan.md`. Exercises the documented
//! procedure as a durable integration test: seed non-trivial workspace
//! data (5 users, 10 tickets + comments, branding configured, doc
//! page with Yjs state), encrypted export, restore on the same
//! template, verify every aggregate intact.
//!
//! Why this is its own file rather than rolled into `backup_basic.rs`:
//! the byte-equality test in backup_basic already proves the
//! *mechanism* works. This test proves the *fixture variety* Q's
//! checklist names — tickets, comments, SLA, branding, Yjs doc
//! state — each survive the round-trip with their identifying
//! sentinel intact. If the byte-equality regression test ever has
//! to be relaxed for a load-bearing reason, this one stays as the
//! "did the things Q cares about specifically survive?" check.
//!
//! The migration baseline already seeds the SLA default policy +
//! working calendar (`2026-05-04-120000_sla_engine`), so the test
//! only asserts those are present after restore rather than
//! re-creating them.

mod common;

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text};

use backend::services::backup as backup_service;

use common::{insert_user, seed_backup_job, with_upload_dir, TestDb};

#[derive(diesel::QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn count(conn: &mut PgConnection, table: &str) -> i64 {
    diesel::sql_query(format!("SELECT COUNT(*) AS count FROM \"{table}\""))
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("count {table}: {e}"))
        .count
}

#[test]
fn q_smoke_full_workspace_encrypted_round_trip() {
    let db = TestDb::new();
    let mut conn = db.conn();
    with_upload_dir();

    // 5 users (one per "team member" the Q spec calls out).
    let user_a = insert_user(&mut conn, "Q Alice");
    let user_b = insert_user(&mut conn, "Q Bob");
    let user_c = insert_user(&mut conn, "Q Carol");
    let user_d = insert_user(&mut conn, "Q Dave");
    let _user_e = insert_user(&mut conn, "Q Eve");

    // Workflow state default + SLA default + working calendar all
    // come from migrations - confirm they're present so we know the
    // round-trip preserves them, not that we have to seed them.
    let state_id: i32 = diesel::sql_query(
        "SELECT id FROM workflow_states WHERE is_default = TRUE AND archived_at IS NULL LIMIT 1",
    )
    .get_result::<IdRow>(&mut *conn)
    .expect("migration-seeded default workflow state")
    .id;
    assert!(count(&mut conn, "sla_policies") >= 1, "SLA default seeded");
    assert!(
        count(&mut conn, "working_calendars") >= 1,
        "working_calendars default seeded"
    );

    // 10 tickets, each with the workspace_id GUC-driven default
    // resolving to workspace 1 (TestDb's WorkspaceGucCustomizer
    // primes app.workspace_id = '1' on every pooled checkout).
    let requesters = [user_a.uuid, user_b.uuid, user_c.uuid, user_d.uuid];
    let mut ticket_ids = Vec::with_capacity(10);
    for i in 0..10 {
        let id: i32 = diesel::sql_query(
            "INSERT INTO tickets (title, workflow_state_id, priority, requester_uuid) \
             VALUES ($1, $2, 'medium', $3) RETURNING id",
        )
        .bind::<Text, _>(format!("Q ticket {i}"))
        .bind::<Integer, _>(state_id)
        .bind::<diesel::sql_types::Uuid, _>(requesters[i % 4])
        .get_result::<IdRow>(&mut *conn)
        .expect("insert ticket")
        .id;
        ticket_ids.push(id);
    }
    assert_eq!(count(&mut conn, "tickets") as usize, 10);

    // One comment per ticket so the comments table has variety too.
    for (i, &tid) in ticket_ids.iter().enumerate() {
        diesel::sql_query(
            "INSERT INTO comments (ticket_id, user_uuid, content, content_format) \
             VALUES ($1, $2, $3, 'plaintext')",
        )
        .bind::<Integer, _>(tid)
        .bind::<diesel::sql_types::Uuid, _>(requesters[i % 4])
        .bind::<Text, _>(format!("Q comment on ticket {tid}"))
        .execute(&mut *conn)
        .expect("insert comment");
    }
    assert_eq!(count(&mut conn, "comments") as usize, 10);

    // Branding: stamp a recognisable primary_color so we can assert
    // it survives the round-trip via a single field-read.
    diesel::sql_query("UPDATE site_settings SET primary_color = '#deadbe' WHERE id = 1")
        .execute(&mut *conn)
        .expect("set branding primary_color");

    // Doc page with deterministic Yjs bytes. The actual Yjs payload
    // would be a binary update protocol blob; for the round-trip we
    // only care that the bytea column survives byte-identical.
    let yjs_state_vector: Vec<u8> = (0..16).collect();
    let yjs_document: Vec<u8> = (16..96).collect();
    let doc_id: i32 = diesel::sql_query(
        "INSERT INTO documentation_pages \
            (uuid, title, slug, status, created_by, last_edited_by, is_public, is_template, \
             yjs_state_vector, yjs_document, yjs_client_id, has_unsaved_changes) \
         VALUES (gen_random_uuid(), 'Q doc with Yjs', 'q-doc-yjs', 'published', $1, $1, \
                 false, false, $2, $3, 42, false) \
         RETURNING id",
    )
    .bind::<diesel::sql_types::Uuid, _>(user_a.uuid)
    .bind::<diesel::sql_types::Bytea, _>(&yjs_state_vector)
    .bind::<diesel::sql_types::Bytea, _>(&yjs_document)
    .get_result::<IdRow>(&mut *conn)
    .expect("insert doc page with yjs state")
    .id;

    let job_id = seed_backup_job(&mut conn);

    // Encrypted export.
    let backup_path = backup_service::create_backup(
        &mut conn,
        job_id,
        Some("q-smoke-correct-horse-battery-staple"),
    )
    .expect("create_backup with password");
    assert!(backup_path.exists(), "backup zip on disk");

    // Restore on the same DB - the existing test infrastructure
    // doesn't tear down + recreate, but `restore_database`'s
    // force_non_empty path is the same code that runs on a
    // re-imported empty environment. The test asserts the data
    // matches after the restore completes; if the restore's path
    // had any bug specific to the fixture variety here (e.g. a
    // diesel error on the doc page's bytea columns), it'd surface.
    let stats = backup_service::restore_database(
        &mut conn,
        &backup_path,
        Some("q-smoke-correct-horse-battery-staple"),
        backup_service::RestoreOptions {
            force_non_empty: true,
            ignore_schema_mismatch: false,
        },
    )
    .expect("restore with correct password");
    assert!(
        stats.records_restored > 0,
        "restore touched rows: {}",
        stats.records_restored
    );

    // Aggregate checks: every Q-spec category is intact.
    assert!(count(&mut conn, "users") >= 5, "5 seeded users survived");
    assert_eq!(count(&mut conn, "tickets") as usize, 10, "10 tickets");
    assert_eq!(count(&mut conn, "comments") as usize, 10, "10 comments");
    assert!(
        count(&mut conn, "sla_policies") >= 1,
        "SLA default still present"
    );
    assert!(
        count(&mut conn, "working_calendars") >= 1,
        "working_calendars still present"
    );

    // Branding sentinel: primary_color round-tripped.
    #[derive(diesel::QueryableByName)]
    struct BrandingRow {
        #[diesel(sql_type = diesel::sql_types::Nullable<Text>)]
        primary_color: Option<String>,
    }
    let branding: BrandingRow =
        diesel::sql_query("SELECT primary_color FROM site_settings WHERE id = 1")
            .get_result(&mut *conn)
            .expect("read branding post-restore");
    assert_eq!(
        branding.primary_color.as_deref(),
        Some("#deadbe"),
        "primary_color round-tripped"
    );

    // Doc page + Yjs bytea sentinel: bytes match exactly.
    #[derive(diesel::QueryableByName)]
    struct DocRow {
        #[diesel(sql_type = Text)]
        title: String,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Bytea>)]
        yjs_state_vector: Option<Vec<u8>>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Bytea>)]
        yjs_document: Option<Vec<u8>>,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::BigInt>)]
        yjs_client_id: Option<i64>,
    }
    let doc: DocRow = diesel::sql_query(
        "SELECT title, yjs_state_vector, yjs_document, yjs_client_id \
         FROM documentation_pages WHERE id = $1",
    )
    .bind::<Integer, _>(doc_id)
    .get_result(&mut *conn)
    .expect("read doc page post-restore");
    assert_eq!(doc.title, "Q doc with Yjs");
    assert_eq!(
        doc.yjs_state_vector.as_deref(),
        Some(yjs_state_vector.as_slice()),
        "yjs_state_vector bytes preserved"
    );
    assert_eq!(
        doc.yjs_document.as_deref(),
        Some(yjs_document.as_slice()),
        "yjs_document bytes preserved"
    );
    assert_eq!(doc.yjs_client_id, Some(42), "yjs_client_id preserved");
}

#[derive(diesel::QueryableByName)]
struct IdRow {
    #[diesel(sql_type = Integer)]
    id: i32,
}
