//! Encryption round-trip and wrong-password rejection. The
//! backup wrapper seals the inner zip with AES-256-GCM + PBKDF2;
//! these tests pin the success and failure paths so a future
//! crypto refactor can't silently break either.

mod common;

use diesel::prelude::*;

use backend::services::backup as backup_service;

use common::{insert_stock_asset, insert_user, seed_backup_job, with_upload_dir, TestDb};

#[test]
fn encrypted_backup_round_trips_with_correct_password() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let _upload = with_upload_dir();

    insert_user(&mut *conn, "Encrypted Eve");
    let asset_id = insert_stock_asset(&mut *conn, "encrypted-asset");

    let job_id = seed_backup_job(&mut conn);
    let backup_path =
        backup_service::create_backup(&mut conn, job_id, Some("correct-horse-battery-staple"))
            .expect("create_backup with password");
    assert!(backup_path.exists());

    // restore_database with the same password should succeed.
    let stats = backup_service::restore_database(
        &mut conn,
        &backup_path,
        Some("correct-horse-battery-staple"),
        backup_service::RestoreOptions {
            force_non_empty: true,
            ignore_schema_mismatch: false,
        },
    )
    .expect("restore with correct password");
    assert!(stats.records_restored > 0);

    // Sentinel: the seeded asset is still there with its name.
    #[derive(diesel::QueryableByName)]
    struct Row {
        #[diesel(sql_type = diesel::sql_types::Text)]
        name: String,
    }
    let row: Row = diesel::sql_query("SELECT name FROM assets WHERE id = $1")
        .bind::<diesel::sql_types::Integer, _>(asset_id)
        .get_result(&mut *conn)
        .expect("seeded asset survives encryption round-trip");
    assert_eq!(row.name, "encrypted-asset");
}

#[test]
fn restore_rejects_wrong_password() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let _upload = with_upload_dir();

    insert_user(&mut *conn, "Eve");
    let job_id = seed_backup_job(&mut conn);
    let backup_path = backup_service::create_backup(&mut conn, job_id, Some("password-a"))
        .expect("create_backup");

    // Wrong password: must fail. The error variant we care about
    // is `InvalidPassword` (PBKDF2/AES-GCM tag mismatch). Any
    // other error would suggest the encryption envelope isn't
    // being verified before parsing.
    let result = backup_service::restore_database(
        &mut conn,
        &backup_path,
        Some("password-b"),
        backup_service::RestoreOptions {
            force_non_empty: true,
            ignore_schema_mismatch: false,
        },
    );

    match result {
        Err(backup_service::BackupError::InvalidPassword) => {}
        Err(other) => panic!("expected InvalidPassword, got {other:?}"),
        Ok(_) => panic!("restore should have failed on wrong password"),
    }
}
