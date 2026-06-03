//! Tamper resistance: modifying a table's JSON inside the
//! backup zip must trip the sha256 check at restore time. The
//! manifest carries a per-table hash; restore_database refuses
//! to apply rows whose recomputed hash disagrees.

mod common;

use std::io::{Read, Write};

use backend::services::backup as backup_service;

use common::{insert_stock_asset, seed_backup_job, with_upload_dir, TestDb};

/// Rewrite the backup zip with the contents of `data/users.json`
/// surgically altered. Re-zips from scratch (the `zip` crate
/// doesn't expose in-place replacement) so the result is a
/// valid archive that fails only on hash verification, not on
/// archive parsing.
fn tamper_backup(path: &std::path::Path) {
    let original = std::fs::read(path).expect("read backup");
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(original.clone())).expect("open archive");

    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).unwrap();

            if name == "data/users.json" {
                // Mutate any payload byte; the SHA-256 doesn't
                // care what changes, only that something does.
                // We swap a quote for a different one in a way
                // that keeps the JSON parseable so the failure
                // is specifically a hash mismatch, not a parse
                // error.
                if let Some(pos) = content.iter().position(|&b| b == b'A') {
                    content[pos] = b'B';
                }
            }

            let options = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            writer.start_file(&name, options).unwrap();
            writer.write_all(&content).unwrap();
        }
        writer.finish().unwrap();
    }

    std::fs::write(path, out).expect("write tampered backup");
}

#[test]
fn restore_rejects_tampered_row_payload() {
    let db = TestDb::new();
    let mut conn = db.conn();
    with_upload_dir();

    // Need at least one user row with an 'A' somewhere so the
    // tamper helper has a byte to flip. Insert a deterministic
    // name that contains an 'A'.
    use backend::models::NewUser;
    use backend::schema::users;
    use diesel::prelude::*;
    use uuid::Uuid;
    diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: "Alice".to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: Some("platform_admin".to_string()),
        })
        .execute(&mut *conn)
        .expect("seed Alice");
    let _ = insert_stock_asset(&mut conn, "T-asset");

    let job_id = seed_backup_job(&mut conn);
    let backup_path =
        backup_service::create_backup(&mut conn, job_id, None).expect("create_backup");

    // Surgically alter the users.json content in-place so the
    // archive's hash no longer matches the manifest entry.
    tamper_backup(&backup_path);

    // Restore must refuse. The error is CorruptedBackup with a
    // sha256 mismatch message.
    let result = backup_service::restore_database(
        &mut conn,
        &backup_path,
        None,
        backup_service::RestoreOptions {
            force_non_empty: true,
            ignore_schema_mismatch: false,
        },
    );

    match result {
        Err(backup_service::BackupError::CorruptedBackup(msg))
            if msg.contains("sha256 mismatch") => {}
        Err(other) => panic!("expected CorruptedBackup(sha256 mismatch), got {other:?}"),
        Ok(_) => panic!("restore should have refused a tampered archive"),
    }
}
