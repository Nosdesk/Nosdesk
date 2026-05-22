//! Backup/restore happy-path round-trip.
//!
//! Per-push CI gate: the migration-seeded baseline + a small
//! hand-rolled fixture should survive a full backup → restore
//! round-trip with byte-identical table contents.
//!
//! Runs against a per-test sandbox DB cloned from a once-per-
//! binary template (see `common::TestDb`), so this test plays
//! nicely under `cargo test` parallelism.

mod common;

use std::path::PathBuf;

use bigdecimal::BigDecimal;
use diesel::prelude::*;
use diesel::sql_types::Text;
use std::str::FromStr;
use uuid::Uuid;

use backend::models::{NewAsset, NewBackupJob, NewUser, User, UserRole};
use backend::repository::backup as backup_repo;
use backend::services::backup as backup_service;

use common::{count_table, hash_table, user_tables, TestDb};

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

/// Seed a user. Returns the inserted row so the test can refer
/// to it by UUID later.
fn insert_user(conn: &mut PgConnection, name: &str) -> User {
    use backend::schema::users;
    let new_user = NewUser {
        uuid: Uuid::new_v4(),
        name: name.to_string(),
        role: UserRole::Admin,
        pronouns: None,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        microsoft_uuid: None,
        mfa_secret: None,
        mfa_enabled: false,
        mfa_backup_codes: None,
    };
    diesel::insert_into(users::table)
        .values(&new_user)
        .get_result(conn)
        .expect("insert user")
}

/// Seed a stock-tracked asset with the Phase E columns set
/// (quantity, unit, low_stock_threshold). Exercises numeric
/// round-trip plus the jsonb `attributes` column.
fn insert_stock_asset(conn: &mut diesel::PgConnection, name: &str) -> i32 {
    use backend::schema::assets;
    let new = NewAsset {
        name: name.to_string(),
        serial_number: None,
        manufacturer: Some("Acme".to_string()),
        model: Some("Pipe".to_string()),
        location: Some("Warehouse A".to_string()),
        notes: None,
        primary_user_uuid: None,
        purchase_date: None,
        asset_tag: None,
        kind: "generic".to_string(),
        attributes: serde_json::json!({"warranty_status": "Active", "color": "blue"}),
        quantity: Some(BigDecimal::from_str("123.456").unwrap()),
        unit: Some("m".to_string()),
        external_sync_source: None,
        low_stock_threshold: Some(BigDecimal::from_str("10.000").unwrap()),
    };
    diesel::insert_into(assets::table)
        .values(&new)
        .returning(assets::id)
        .get_result(conn)
        .expect("insert asset")
}

/// Seed a backup_jobs row so `create_backup` has something to
/// update on completion. Mirrors what `handlers::backup` does;
/// `BackupJob.id` is a UUID.
fn seed_backup_job(conn: &mut backend::db::DbConnection) -> Uuid {
    backup_repo::create_backup_job(
        conn,
        NewBackupJob {
            job_type: "export".to_string(),
            status: "processing".to_string(),
            include_sensitive: true,
            created_by: None,
        },
    )
    .expect("seed backup_jobs row")
    .id
}

#[test]
fn round_trip_preserves_every_table_byte_for_byte() {
    let db = TestDb::new();
    let mut conn = db.conn();

    // Per-test UPLOAD_DIR so the backup zip writes somewhere we
    // own. tempdir cleans up automatically on Drop.
    let upload_root = tempfile::tempdir().expect("tempdir");
    std::env::set_var("UPLOAD_DIR", upload_root.path());

    // Seed user-data variety on top of the migration baseline.
    // Numeric (3dp BigDecimal), jsonb attributes, varchar, uuid
    // PK on users, custom enum (UserRole) — each exercised here.
    let _kyle = insert_user(&mut *conn, "Kyle");
    let pipe_id = insert_stock_asset(&mut *conn, "20mm copper pipe");

    assert!(count_table(&mut *conn, "users") >= 1);
    assert!(count_table(&mut *conn, "assets") >= 1);

    // Reference state.
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
