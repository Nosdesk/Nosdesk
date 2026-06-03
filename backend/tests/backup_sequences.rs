//! Sequence drift: after `restore_database`, an INSERT without
//! an explicit id must produce a non-colliding PK on every table
//! that has a serial / identity column. Catches the "next insert
//! after restore PK-collides" class of bug.
//!
//! The restore path's `reset_sequences` walks `pg_depend` to
//! find every column-owned sequence and aligns it to MAX(col)+1.
//! This test exercises that across multiple representative
//! tables (different id widths, different starting max IDs).

mod common;

use diesel::prelude::*;
use diesel::sql_types::BigInt;

use backend::services::backup as backup_service;

use common::{insert_stock_asset, insert_user, seed_backup_job, with_upload_dir, TestDb};

#[derive(diesel::QueryableByName)]
struct MaxIdRow {
    #[diesel(sql_type = BigInt)]
    max_id: i64,
}

fn max_id(conn: &mut PgConnection, table: &str, col: &str) -> i64 {
    let q = format!("SELECT COALESCE(MAX(\"{col}\"), 0)::bigint AS max_id FROM \"{table}\"");
    diesel::sql_query(q)
        .get_result::<MaxIdRow>(conn)
        .expect("max id query")
        .max_id
}

#[test]
fn restore_realigns_sequences_so_new_inserts_dont_collide() {
    let db = TestDb::new();
    let mut conn = db.conn();
    with_upload_dir();

    // Seed a few rows so the sequences advance past their initial
    // values. We want the post-restore sequence reset to land at
    // MAX(id)+1, not at 1.
    for i in 0..3 {
        insert_user(&mut conn, &format!("seq-user-{i}"));
        insert_stock_asset(&mut conn, &format!("seq-asset-{i}"));
    }

    let assets_max_before = max_id(&mut conn, "assets", "id");
    assert!(assets_max_before > 0, "seed should have advanced sequence");

    let job_id = seed_backup_job(&mut conn);
    let backup_path =
        backup_service::create_backup(&mut conn, job_id, None).expect("create_backup succeeded");

    backup_service::restore_database(
        &mut conn,
        &backup_path,
        None,
        backup_service::RestoreOptions {
            force_non_empty: true,
            ignore_schema_mismatch: false,
        },
    )
    .expect("restore succeeded");

    // Same max id post-restore: we just round-tripped the same
    // rows.
    let assets_max_after = max_id(&mut conn, "assets", "id");
    assert_eq!(
        assets_max_after, assets_max_before,
        "assets.id MAX changed across restore"
    );

    // The load-bearing assertion: inserting a new row without an
    // explicit id must succeed. If reset_sequences left the
    // sequence at <= MAX(id), this would raise
    // duplicate_key_value.
    let new_id = insert_stock_asset(&mut conn, "post-restore canary");
    assert!(
        new_id as i64 > assets_max_after,
        "new asset id ({new_id}) must be greater than pre-insert MAX ({assets_max_after})"
    );

    // Same check on a second table with a serial id, to catch a
    // selective-table sequence-reset regression. Comments has a
    // serial id and gets created by user fixtures via FK chains;
    // refresh_tokens is simpler.
    let rt_max_before = max_id(&mut conn, "refresh_tokens", "id");
    // Inserts into refresh_tokens require a user reference, skip
    // the actual insert if the table is empty (no FK to use).
    // Just assert the sequence sits above MAX (would be the
    // pre-condition for any future insert).
    #[derive(diesel::QueryableByName)]
    struct NextValRow {
        #[diesel(sql_type = BigInt)]
        next_val: i64,
    }
    let q = "SELECT last_value AS next_val FROM refresh_tokens_id_seq";
    let rt_seq: NextValRow = diesel::sql_query(q)
        .get_result(&mut *conn)
        .expect("refresh_tokens_id_seq lookup");
    assert!(
        rt_seq.next_val > rt_max_before,
        "refresh_tokens sequence ({}) must be above MAX ({rt_max_before}) after restore",
        rt_seq.next_val
    );
}
