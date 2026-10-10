//! A backup made on 1.0.x restores, through the admin restore's path, into
//! the database an in-place upgrade of the same install would have produced.
//!
//! The fixtures are real in-app backups of throwaway installs of 1.0.6,
//! 1.0.10 and 1.0.12, one for each 1.0 schema (`admin@example.com`, no MFA):
//! four tickets, the second merged into the first, the last two closed, a
//! comment with two attachments and a document. `encrypted.zip` is the same
//! install sealed with `fixture-passphrase`. `restored-by-1.0.12.sql` is the
//! 1.0.12 encrypted backup as 1.0.12's own `nosdesk-cli db restore` loaded
//! it, dumped with `pg_dump --data-only --inserts --load-via-partition-root`:
//! the in-place starting point.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use sha2::Digest as _;

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Integer, Nullable, Text};

use backend::services::backup as backup_service;

use crate::common::upgrade_1_0_12::UpgradeDb;
use crate::common::{fixture_path, user_tables, with_upload_dir, TestDb};

const PASSPHRASE: &str = "fixture-passphrase";

/// One backup of each 1.0 schema.
const RELEASES: &[&str] = &["1.0.6", "1.0.10", "1.0.12"];

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

/// Restore `fixture` of `release` the way the admin restore does: over a
/// live database, with nothing but the archive and its password.
fn restore(
    db: &TestDb,
    release: &str,
    fixture: &str,
    password: Option<&str>,
) -> backup_service::RestoreStats {
    let path = fixture_path(&format!("backups/{release}/{fixture}"));
    let mut conn = db.conn();
    let stats = backup_service::restore_database(
        &mut conn,
        &path,
        password,
        backup_service::RestoreOptions {
            force_non_empty: true,
            server_url: Some(db.url().to_string()),
            ..Default::default()
        },
    )
    .unwrap_or_else(|e| panic!("restore {release}/{fixture}: {e}"));
    backup_service::restore_backup_files(&path, password).expect("restore files");
    stats
}

/// What an in-place upgrade of a fixture install leaves behind.
fn assert_upgraded(conn: &mut PgConnection, release: &str) {
    // The merge moved to `ticket_merges` (1.1 squash migration).
    let merges: Vec<Merge> = diesel::sql_query(
        "SELECT ticket_id, merged_into_ticket_id, merge_reason FROM ticket_merges",
    )
    .load(conn)
    .expect("ticket_merges");
    assert_eq!(merges.len(), 1, "{release}: the merge survives the restore");
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
    assert!(!notes.is_empty(), "{release}: the merge note is restored");
    assert!(
        notes.iter().all(|n| n.is_internal),
        "{release}: the merge note is internal"
    );

    // Every notification type a fresh 1.1 database has, not 1.0's eight.
    let fresh = TestDb::new();
    assert_eq!(
        notification_codes(conn),
        notification_codes(&mut fresh.conn()),
        "{release}: notification types match a fresh database"
    );

    // Refresh tokens from before 1.1 don't sign anyone in
    // (refresh_token_audience revokes them).
    assert_eq!(
        count(
            conn,
            "SELECT count(*) AS n FROM refresh_tokens WHERE revoked_at IS NULL"
        ),
        0,
        "{release}: no pre-1.1 refresh token is live"
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
        "{release}: closed tickets carry closed_by"
    );

    // The guest form's default rate rose from 5 to 20.
    assert_eq!(
        count(
            conn,
            "SELECT guest_ticket_rate_limit_per_hour::bigint AS n FROM site_settings"
        ),
        20
    );

    // Tickets keep their ids as numbers.
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
    for release in RELEASES {
        let db = TestDb::new();
        let stats = restore(&db, release, "plain.zip", None);
        assert!(stats.records_restored > 0);
        let mut conn = db.conn();
        assert_upgraded(&mut conn, release);
        assert_files_restored(&mut conn);
    }
}

#[test]
fn an_encrypted_1_0_backup_restores_upgraded() {
    with_upload_dir();
    for release in RELEASES {
        let db = TestDb::new();
        restore(&db, release, "encrypted.zip", Some(PASSPHRASE));
        let mut conn = db.conn();
        assert_upgraded(&mut conn, release);
        assert_files_restored(&mut conn);
        // The encrypted backup carries the sign-in tokens; they come back
        // revoked.
        assert!(
            count(&mut conn, "SELECT count(*) AS n FROM refresh_tokens") > 0,
            "{release}: the encrypted backup's refresh tokens are restored"
        );
    }
}

/// Columns an upgrade fills with the time it runs, so they differ between
/// any two upgrades of the same rows.
const NOW_COLUMNS: &[(&str, &[&str])] = &[
    // merge_notes_internal and guest_rate_default update rows, and the
    // set_updated_at trigger stamps them.
    ("comments", &["updated_at"]),
    ("site_settings", &["updated_at"]),
    // Rows the 1.1 migrations add.
    ("notification_types", &["created_at"]),
    // refresh_token_audience revokes every token as it runs.
    ("refresh_tokens", &["revoked_at"]),
    // Restore stamps this build's schema fingerprint, as boot does.
    ("system_meta", &["updated_at"]),
];

/// Per-table digests of every row, leaving out [`NOW_COLUMNS`].
fn table_digests(conn: &mut PgConnection) -> BTreeMap<String, String> {
    #[derive(QueryableByName)]
    struct Digest {
        #[diesel(sql_type = Text)]
        digest: String,
    }
    user_tables(conn)
        .into_iter()
        .map(|table| {
            let mut row = "to_jsonb(t)".to_string();
            for (t, columns) in NOW_COLUMNS {
                if *t == table {
                    for column in *columns {
                        row.push_str(&format!(" - '{column}'"));
                    }
                }
            }
            let digest = diesel::sql_query(format!(
                "SELECT COALESCE(md5(string_agg(r::text, '|' ORDER BY r::text)), 'empty') AS digest \
                 FROM (SELECT {row} AS r FROM public.\"{table}\" t) rows"
            ))
            .get_result::<Digest>(conn)
            .unwrap_or_else(|e| panic!("digest {table}: {e}"))
            .digest;
            (table, digest)
        })
        .collect()
}

/// The value each sequence hands out next, by `schema.name`.
fn next_values(conn: &mut PgConnection) -> BTreeMap<String, i64> {
    #[derive(QueryableByName)]
    struct Seq {
        #[diesel(sql_type = Text)]
        name: String,
    }
    #[derive(QueryableByName)]
    struct Next {
        #[diesel(sql_type = BigInt)]
        next: i64,
    }
    let sequences: Vec<Seq> = diesel::sql_query(
        "SELECT format('%I.%I', n.nspname, c.relname) AS name FROM pg_class c \
         JOIN pg_namespace n ON n.oid = c.relnamespace \
         WHERE c.relkind = 'S' AND n.nspname IN ('public', 'ticket_numbers')",
    )
    .load(conn)
    .expect("sequences");
    sequences
        .into_iter()
        .map(|Seq { name }| {
            let next = diesel::sql_query(format!(
                "SELECT CASE WHEN is_called THEN last_value + 1 ELSE last_value END AS next \
                 FROM {name}"
            ))
            .get_result::<Next>(conn)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .next;
            (name, next)
        })
        .collect()
}

/// The upgrade restore against the in-place upgrade it stands for: the same
/// 1.0.12 rows, loaded by 1.0.12 itself and migrated forward where they
/// are, give the same rows in every table and the same next value from
/// every sequence. This covers the re-export from the scratch database and
/// the reload here (hop 2) as well as the migrations.
#[test]
fn an_upgrade_restore_matches_an_in_place_upgrade_table_for_table() {
    with_upload_dir();

    // In place: 1.0.12's own restore, then this build's migrations.
    let in_place = UpgradeDb::at_1_0_12();
    {
        let mut load = in_place.conn();
        let tables = user_tables(&mut load);
        load.batch_execute(&format!(
            "TRUNCATE TABLE {} RESTART IDENTITY CASCADE",
            tables
                .iter()
                .map(|t| format!("public.\"{t}\""))
                .collect::<Vec<_>>()
                .join(", ")
        ))
        .expect("clear the seeded rows, as 1.0.12's restore does");
        let dump = std::fs::read_to_string(fixture_path("backups/1.0.12/restored-by-1.0.12.sql"))
            .expect("in-place dump");
        load.batch_execute(&format!("SET session_replication_role = replica;\n{dump}"))
            .expect("load the dump");
    }
    let mut in_place_conn = in_place.conn();
    in_place.migrate_to_head(&mut in_place_conn);
    // What the first boot after the upgrade does.
    diesel::sql_query(
        "UPDATE system_meta SET value = to_jsonb($1::text) WHERE key = 'schema_hash'",
    )
    .bind::<Text, _>(env!("NOSDESK_SCHEMA_HASH"))
    .execute(&mut in_place_conn)
    .expect("stamp the schema fingerprint");

    // Upgrade restore of the backup 1.0.12 restored.
    let db = TestDb::new();
    restore(&db, "1.0.12", "encrypted.zip", Some(PASSPHRASE));
    let mut restored = PgConnection::establish(db.url()).expect("connect");

    let expected = table_digests(&mut in_place_conn);
    let actual = table_digests(&mut restored);
    let differing: Vec<&String> = expected
        .keys()
        .filter(|t| expected.get(*t) != actual.get(*t))
        .collect();
    assert!(differing.is_empty(), "tables differ: {differing:?}");
    assert_eq!(expected.len(), actual.len());
    assert_eq!(
        next_values(&mut in_place_conn),
        next_values(&mut restored),
        "every sequence hands out the same next value"
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

/// A copy of fixture `rel` with `edit` applied to each entry, written to
/// `dir`. Each `data/` table it changes gets its new checksum in the
/// manifest, so the archive is still a valid backup.
pub fn rewrite_fixture(
    rel: &str,
    dir: &std::path::Path,
    edit: impl Fn(&str, Vec<u8>) -> Vec<u8>,
) -> std::path::PathBuf {
    let original = std::fs::read(fixture_path(rel)).expect("fixture");
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(original)).expect("open fixture");
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).expect("entry");
        let name = entry.name().to_string();
        let mut content = Vec::new();
        entry.read_to_end(&mut content).expect("read entry");
        let edited = edit(&name, content.clone());
        entries.push((name, edited));
    }
    let mut manifest: serde_json::Value = serde_json::from_slice(
        &entries
            .iter()
            .find(|(n, _)| n == "manifest.json")
            .expect("manifest")
            .1,
    )
    .expect("manifest json");
    for (name, content) in &entries {
        if let Some(table) = name
            .strip_prefix("data/")
            .and_then(|n| n.strip_suffix(".json"))
        {
            manifest["tables"][table]["sha256"] =
                serde_json::Value::String(hex::encode(sha2::Sha256::digest(content)));
        }
    }
    let mut out = Vec::new();
    {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
        for (name, content) in &entries {
            writer
                .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
                .expect("start entry");
            if name == "manifest.json" {
                writer
                    .write_all(&serde_json::to_vec(&manifest).expect("manifest"))
                    .expect("write manifest");
            } else {
                writer.write_all(content).expect("write entry");
            }
        }
        writer.finish().expect("finish zip");
    }
    let path = dir.join("rewritten.zip");
    std::fs::write(&path, out).expect("write fixture copy");
    path
}

/// A backup whose schema this build can't place is refused at the preview,
/// before the operator confirms anything.
#[test]
fn the_preview_refuses_a_backup_from_an_unknown_schema() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = rewrite_fixture("backups/1.0.12/plain.zip", dir.path(), |name, content| {
        if name == "manifest.json" {
            String::from_utf8(content)
                .expect("utf-8 manifest")
                .replace("0319c33351b4a3c4", "0123456789abcdef")
                .into_bytes()
        } else {
            content
        }
    });

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
