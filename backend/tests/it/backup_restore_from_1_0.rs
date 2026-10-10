//! A backup made on 1.0.12 restores, through the admin restore's path, into
//! the database an in-place upgrade of the same install would have produced.
//!
//! The fixtures are real 1.0.12 in-app backups of a throwaway install
//! (`admin@example.com`, no MFA): four tickets, the second merged into the
//! first, the last two closed, a comment with two attachments and a document.
//! `encrypted.zip` is the same install sealed with `fixture-passphrase`.

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Integer, Nullable, Text};

use backend::services::backup as backup_service;

use crate::common::{fixture_path, with_upload_dir, TestDb};

const PASSPHRASE: &str = "fixture-passphrase";

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Code {
    #[diesel(sql_type = Text)]
    code: String,
}

#[derive(QueryableByName)]
struct Merge {
    #[diesel(sql_type = Integer)]
    ticket_id: i32,
    #[diesel(sql_type = Integer)]
    merged_into_ticket_id: i32,
    #[diesel(sql_type = Nullable<Text>)]
    merge_reason: Option<String>,
}

#[derive(QueryableByName)]
struct Note {
    #[diesel(sql_type = Bool)]
    is_internal: bool,
}

#[derive(QueryableByName)]
struct Url {
    #[diesel(sql_type = Text)]
    url: String,
}

fn count(conn: &mut PgConnection, sql: &str) -> i64 {
    diesel::sql_query(sql)
        .get_result::<Count>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .n
}

fn notification_codes(conn: &mut PgConnection) -> Vec<String> {
    diesel::sql_query("SELECT code FROM notification_types ORDER BY code")
        .load::<Code>(conn)
        .expect("notification types")
        .into_iter()
        .map(|c| c.code)
        .collect()
}

/// Restore `fixture` the way the admin restore does: over a live database,
/// with nothing but the archive and its password.
fn restore(db: &TestDb, fixture: &str, password: Option<&str>) -> backup_service::RestoreStats {
    let path = fixture_path(&format!("backups/1.0.12/{fixture}"));
    let mut conn = db.conn();
    let stats = backup_service::restore_database(
        &mut conn,
        &path,
        password,
        backup_service::RestoreOptions {
            force_non_empty: true,
            server_url: Some(db.url().to_string()),
        },
    )
    .unwrap_or_else(|e| panic!("restore {fixture}: {e}"));
    backup_service::restore_backup_files(&path, password).expect("restore files");
    stats
}

/// What an in-place upgrade of the fixture install leaves behind.
fn assert_upgraded(conn: &mut PgConnection) {
    // The merge moved to `ticket_merges` (1.1 squash migration).
    let merges: Vec<Merge> = diesel::sql_query(
        "SELECT ticket_id, merged_into_ticket_id, merge_reason FROM ticket_merges",
    )
    .load(conn)
    .expect("ticket_merges");
    assert_eq!(merges.len(), 1, "the merge survives the restore");
    assert_eq!(
        (merges[0].ticket_id, merges[0].merged_into_ticket_id),
        (2, 1)
    );
    assert_eq!(merges[0].merge_reason.as_deref(), Some("Duplicate"));

    // The merge note became internal (merge_notes_internal).
    let notes: Vec<Note> =
        diesel::sql_query("SELECT is_internal FROM comments WHERE content ILIKE '%merged%'")
            .load(conn)
            .expect("merge notes");
    assert!(!notes.is_empty(), "the merge note is restored");
    assert!(
        notes.iter().all(|n| n.is_internal),
        "the merge note is internal"
    );

    // Every notification type a fresh 1.1 database has, not 1.0's eight.
    let fresh = TestDb::new();
    assert_eq!(
        notification_codes(conn),
        notification_codes(&mut fresh.conn()),
        "notification types match a fresh database"
    );

    // Refresh tokens from before 1.1 don't sign anyone in
    // (refresh_token_audience revokes them).
    assert_eq!(
        count(
            conn,
            "SELECT count(*) AS n FROM refresh_tokens WHERE revoked_at IS NULL"
        ),
        0,
        "no pre-1.1 refresh token is live"
    );

    // A closed ticket names who closed it (closed_follows_state backfill).
    assert_eq!(
        count(
            conn,
            "SELECT count(*) AS n FROM tickets WHERE closed_at IS NOT NULL"
        ),
        2
    );
    assert_eq!(
        count(
            conn,
            "SELECT count(*) AS n FROM tickets WHERE closed_at IS NOT NULL AND closed_by IS NULL"
        ),
        0,
        "closed tickets carry closed_by"
    );

    // The guest form's default rate rose from 5 to 20.
    assert_eq!(
        count(
            conn,
            "SELECT guest_ticket_rate_limit_per_hour::bigint AS n FROM site_settings"
        ),
        20
    );

    // Tickets keep their ids as numbers, and the next one follows them.
    assert_eq!(
        count(conn, "SELECT count(*) AS n FROM tickets WHERE number = id"),
        4
    );
}

/// The restored attachments open: each file is where its row points.
fn assert_files_restored(conn: &mut PgConnection) {
    let upload_dir = std::path::PathBuf::from(std::env::var("UPLOAD_DIR").expect("UPLOAD_DIR"));
    let urls: Vec<Url> = diesel::sql_query("SELECT url FROM attachments ORDER BY id")
        .load(conn)
        .expect("attachments");
    assert_eq!(urls.len(), 2);
    for Url { url } in urls {
        let rel = url.strip_prefix("/uploads/").expect("an uploads url");
        let path = upload_dir.join("ws/1").join(rel);
        assert!(path.is_file(), "{url} restored at {}", path.display());
    }
}

#[test]
fn a_1_0_backup_without_a_password_restores_upgraded() {
    with_upload_dir();
    let db = TestDb::new();
    let stats = restore(&db, "plain.zip", None);
    assert!(stats.records_restored > 0);
    let mut conn = db.conn();
    assert_upgraded(&mut conn);
    assert_files_restored(&mut conn);
}

#[test]
fn an_encrypted_1_0_backup_restores_upgraded() {
    with_upload_dir();
    let db = TestDb::new();
    restore(&db, "encrypted.zip", Some(PASSPHRASE));
    let mut conn = db.conn();
    assert_upgraded(&mut conn);
    assert_files_restored(&mut conn);
    // The encrypted backup carries the sign-in tokens; they come back revoked.
    assert!(
        count(&mut conn, "SELECT count(*) AS n FROM refresh_tokens") > 0,
        "the encrypted backup's refresh tokens are restored"
    );
}

#[test]
fn the_preview_names_the_version_a_1_0_backup_is_upgraded_from() {
    let path = fixture_path("backups/1.0.12/plain.zip");
    let preview = backup_service::preview_restore(&path, None).expect("preview");
    assert_eq!(preview.manifest.nosdesk_version, "1.0.12");
    let upgrade = preview.upgrade.expect("the preview says it upgrades");
    assert_eq!(upgrade.from_version, "1.0.12");
    assert_eq!(upgrade.to_version, env!("CARGO_PKG_VERSION"));
}

/// A backup whose schema this build can't place is refused at the preview,
/// before the operator confirms anything.
#[test]
fn the_preview_refuses_a_backup_from_an_unknown_schema() {
    use std::io::{Read, Write};

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
            if name == "manifest.json" {
                content = String::from_utf8(content)
                    .expect("utf-8 manifest")
                    .replace("0319c33351b4a3c4", "0123456789abcdef")
                    .into_bytes();
            }
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .expect("start entry");
            writer.write_all(&content).expect("write entry");
        }
        writer.finish().expect("finish zip");
    }
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("unknown.zip");
    std::fs::write(&path, out).expect("write");

    let refused = backup_service::preview_restore(&path, None).expect_err("refused");
    assert!(
        matches!(
            refused,
            backup_service::BackupError::NotRestorable(
                backup_service::NotRestorable::Unknown { .. }
            )
        ),
        "{refused}"
    );
    assert!(
        refused
            .to_string()
            .contains(&format!("Nosdesk 1.0.0 to {}", env!("CARGO_PKG_VERSION"))),
        "{refused}"
    );
}
