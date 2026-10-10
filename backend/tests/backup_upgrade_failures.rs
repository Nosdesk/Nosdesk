//! Restoring a backup from an earlier version fails safely: a role that can't
//! create the scratch database is refused before anything changes, and a
//! failed upgrade leaves the live database as it was and drops its scratch
//! database.
//!
//! A standalone binary rather than part of `tests/it`: it asserts that no
//! scratch database is left on the server, which a concurrent restore in the
//! same process would make racy.

#![allow(clippy::expect_used)]

mod common;

use std::io::{Read, Write};

use diesel::prelude::*;
use diesel::sql_types::BigInt;

use backend::services::backup as backup_service;

use common::{admin_url, fixture_path, hash_table, with_upload_dir, TestDb};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Scratch databases on the test server.
fn scratch_databases() -> i64 {
    let mut admin = PgConnection::establish(&admin_url()).expect("connect to admin DB");
    diesel::sql_query(
        "SELECT count(*) AS n FROM pg_database WHERE datname LIKE 'nosdesk\\_restore\\_%'",
    )
    .get_result::<Count>(&mut admin)
    .expect("count scratch databases")
    .n
}

/// A copy of the 1.0.12 plaintext fixture with one byte of `data/tickets.json`
/// changed, so its upgrade fails at the scratch load on the table's checksum.
fn tampered_fixture(dir: &std::path::Path) -> std::path::PathBuf {
    let original = std::fs::read(fixture_path("backups/1.0.12/plain.zip")).expect("fixture");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original)).expect("open fixture");
    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).expect("entry");
            let name = entry.name().to_string();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).expect("read entry");
            if name == "data/tickets.json" {
                let pos = content
                    .iter()
                    .position(|&b| b == b'P')
                    .expect("a ticket title");
                content[pos] = b'Q';
            }
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .expect("start entry");
            writer.write_all(&content).expect("write entry");
        }
        writer.finish().expect("finish zip");
    }
    let path = dir.join("tampered.zip");
    std::fs::write(&path, out).expect("write tampered fixture");
    path
}

/// One test, so nothing else in this process creates a scratch database
/// while it counts them.
#[test]
fn an_upgrade_that_cant_finish_changes_nothing() {
    // Counted rather than assumed zero: a test process killed mid-restore
    // elsewhere on the cluster may have left one behind.
    let before = scratch_databases();
    a_role_without_createdb_is_refused_before_anything_changes(before);
    a_failed_upgrade_drops_its_scratch_database_and_leaves_the_live_one(before);
}

fn a_role_without_createdb_is_refused_before_anything_changes(scratch_before: i64) {
    with_upload_dir();
    let db = TestDb::new();
    let before = hash_table(&mut db.conn(), "site_settings");
    // `nosdesk_app` can't create databases.
    let mut conn = db.runtime_pool(1).get().expect("app-role connection");
    let refused = backup_service::restore_database(
        &mut conn,
        &fixture_path("backups/1.0.12/plain.zip"),
        None,
        backup_service::RestoreOptions {
            force_non_empty: true,
            server_url: Some(db.url().to_string()),
        },
    )
    .expect_err("refused without CREATEDB");
    assert!(
        matches!(refused, backup_service::BackupError::CannotCreateDatabase),
        "{refused}"
    );
    assert!(refused.to_string().contains("CREATEDB"), "{refused}");
    assert_eq!(hash_table(&mut db.conn(), "site_settings"), before);
    assert_eq!(scratch_databases(), scratch_before);
}

fn a_failed_upgrade_drops_its_scratch_database_and_leaves_the_live_one(scratch_before: i64) {
    with_upload_dir();
    let dir = tempfile::tempdir().expect("tempdir");
    let tampered = tampered_fixture(dir.path());
    let db = TestDb::new();
    let before = hash_table(&mut db.conn(), "site_settings");
    let failed = backup_service::restore_database(
        &mut db.conn(),
        &tampered,
        None,
        backup_service::RestoreOptions {
            force_non_empty: true,
            server_url: Some(db.url().to_string()),
        },
    )
    .expect_err("a tampered table fails the upgrade");
    assert!(failed.to_string().contains("sha256 mismatch"), "{failed}");
    assert_eq!(hash_table(&mut db.conn(), "site_settings"), before);
    assert_eq!(
        scratch_databases(),
        scratch_before,
        "the scratch database is dropped"
    );
}
