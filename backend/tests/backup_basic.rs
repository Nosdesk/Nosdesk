//! Backup/restore happy-path round-trip.
//!
//! Per-push CI gate: the migration-seeded baseline + a small
//! hand-rolled fixture should survive a full backup → restore
//! round-trip with byte-identical table contents.

mod common;

use std::path::PathBuf;

use diesel::prelude::*;
use diesel::sql_types::Text;

use backend::services::backup as backup_service;

use common::{
    count_table, hash_table, insert_stock_asset, insert_user, seed_backup_job, user_tables,
    with_upload_dir, TestDb,
};

/// Snapshot of every user table's content hash. Equality means
/// every row across the schema matches at the text-cast level.
fn snapshot_all(conn: &mut PgConnection) -> Vec<(String, String)> {
    user_tables(conn)
        .into_iter()
        .map(|t| {
            let h = hash_table(conn, &t);
            (t, h)
        })
        .collect()
}

#[test]
fn round_trip_preserves_every_table_byte_for_byte() {
    let db = TestDb::new();
    let mut conn = db.conn();
    with_upload_dir();

    // Seed user-data variety on top of the migration baseline.
    // Numeric (3dp BigDecimal), jsonb attributes, varchar, uuid
    // PK on users, custom enum (UserRole) — each exercised here.
    let _kyle = insert_user(&mut *conn, "Kyle");
    let pipe_id = insert_stock_asset(&mut *conn, "20mm copper pipe");

    assert!(count_table(&mut *conn, "users") >= 1);
    assert!(count_table(&mut *conn, "assets") >= 1);

    let baseline = snapshot_all(&mut *conn);

    let job_id = seed_backup_job(&mut conn);
    let backup_path: PathBuf = backup_service::create_backup(&mut conn, job_id, None)
        .unwrap_or_else(|e| panic!("create_backup failed: {e}"));
    assert!(backup_path.exists(), "backup zip exists on disk");

    let stats = backup_service::restore_database(
        &mut conn,
        &backup_path,
        None,
        backup_service::RestoreOptions {
            force_non_empty: true,
            ignore_schema_mismatch: false,
        },
    )
    .expect("restore succeeded");
    assert!(stats.tables_restored > 0);
    assert!(
        stats.records_restored >= 14,
        "expected at least the seeded asset + the 13 builtin asset_kinds, got {}",
        stats.records_restored
    );

    // Tables that mutate during the backup itself: `backup_jobs`
    // gets its row inserted between baseline and backup, then
    // its status flipped after the JSON is written. `audit_log`
    // captures every write including ours. The byte-equality
    // assertion would race against both by construction.
    const RACE_PRONE: &[&str] = &["backup_jobs", "audit_log"];

    let after = snapshot_all(&mut *conn);
    for (table, baseline_hash) in &baseline {
        if RACE_PRONE.contains(&table.as_str()) {
            continue;
        }
        let after_hash = after
            .iter()
            .find(|(t, _)| t == table)
            .map(|(_, h)| h.as_str())
            .unwrap_or_else(|| panic!("table '{table}' missing after restore"));
        assert_eq!(
            baseline_hash, after_hash,
            "table '{table}' content drifted across backup round-trip"
        );
    }

    // Sentinel: the asset we seeded is queryable by id and
    // carries its original name + kind.
    #[derive(diesel::QueryableByName)]
    struct AssetRow {
        #[diesel(sql_type = Text)]
        name: String,
        #[diesel(sql_type = Text)]
        kind: String,
    }
    let row: AssetRow = diesel::sql_query("SELECT name, kind FROM assets WHERE id = $1")
        .bind::<diesel::sql_types::Integer, _>(pipe_id)
        .get_result(&mut *conn)
        .expect("re-read seeded asset");
    assert_eq!(row.name, "20mm copper pipe");
    assert_eq!(row.kind, "generic");
}
