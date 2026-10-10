//! A restore that can't finish changes nothing, two never run at once, and
//! what a killed one left behind is cleaned up.
//!
//! Scratch databases are named for the database being restored
//! (`nosdesk_restore_<its oid>_…`), so each test here only sees its own.

use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text};

use backend::services::backup as backup_service;

use super::backup_restore_from_1_0::rewrite_fixture;
use crate::common::{
    fixture_path, hash_table, seed_backup_job, seed_two_workspaces, user_tables, with_upload_dir,
    TestDb,
};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Name {
    #[diesel(sql_type = Text)]
    name: String,
}

/// The scratch-database name prefix for `db`.
fn scratch_prefix(db: &TestDb) -> String {
    let mut conn = PgConnection::establish(db.url()).expect("connect");
    let oid: Name = diesel::sql_query(
        "SELECT oid::text AS name FROM pg_database WHERE datname = current_database()",
    )
    .get_result(&mut conn)
    .expect("oid");
    format!("nosdesk_restore_{}_", oid.name)
}

/// Scratch databases of `db` on the server.
fn scratch_databases(db: &TestDb) -> i64 {
    let mut conn = PgConnection::establish(db.url()).expect("connect");
    diesel::sql_query("SELECT count(*) AS n FROM pg_database WHERE starts_with(datname, $1)")
        .bind::<Text, _>(scratch_prefix(db))
        .get_result::<Count>(&mut conn)
        .expect("count scratch databases")
        .n
}

fn options(db: &TestDb) -> backup_service::RestoreOptions {
    backup_service::RestoreOptions {
        force_non_empty: true,
        server_url: Some(db.url().to_string()),
        ..Default::default()
    }
}

fn plain_1_0_12() -> std::path::PathBuf {
    fixture_path("backups/1.0.12/plain.zip")
}

/// Connect to `db` as `role`, set for the session.
fn as_role(db: &TestDb, role: &str) -> backend::db::DbConnection {
    let sep = if db.url().contains('?') { '&' } else { '?' };
    let url = format!("{}{sep}options=-c%20role%3D{role}", db.url());
    r2d2::Pool::builder()
        .max_size(1)
        .build(backend::db::ResettingManager::new(url))
        .expect("pool")
        .get()
        .expect("connection")
}

#[test]
fn a_role_without_the_upgrade_privileges_is_refused_before_anything_changes() {
    with_upload_dir();
    let db = TestDb::new();
    let before = hash_table(&mut db.conn(), "site_settings");

    // `nosdesk_app` has neither CREATEDB nor BYPASSRLS.
    let refused = backup_service::restore_database(
        &mut as_role(&db, "nosdesk_app"),
        &plain_1_0_12(),
        None,
        options(&db),
    )
    .expect_err("refused");
    match &refused {
        backup_service::BackupError::MissingPrivileges(missing) => {
            assert_eq!(missing, &["CREATEDB", "BYPASSRLS"])
        }
        other => panic!("refused for another reason: {other}"),
    }

    // CREATEDB alone isn't enough: the upgraded rows are read back across
    // every workspace.
    let role = format!(
        "nosdesk_test_createdb_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..12]
    );
    db.conn()
        .batch_execute(&format!("CREATE ROLE {role} NOLOGIN CREATEDB"))
        .expect("create role");
    let refused = backup_service::restore_database(
        &mut as_role(&db, &role),
        &plain_1_0_12(),
        None,
        options(&db),
    )
    .expect_err("refused");
    db.conn()
        .batch_execute(&format!("DROP ROLE {role}"))
        .expect("drop role");
    match &refused {
        backup_service::BackupError::MissingPrivileges(missing) => {
            assert_eq!(missing, &["BYPASSRLS"])
        }
        other => panic!("refused for another reason: {other}"),
    }
    assert!(refused.to_string().contains("BYPASSRLS"), "{refused}");

    assert_eq!(hash_table(&mut db.conn(), "site_settings"), before);
    assert_eq!(scratch_databases(&db), 0);
}

/// A migration that fails on the backup's rows stops the restore: the live
/// database is as it was and the scratch database is gone.
#[test]
fn a_failed_upgrade_leaves_the_live_database_and_drops_the_scratch_one() {
    with_upload_dir();
    let dir = tempfile::tempdir().expect("tempdir");
    // A ticket in a workflow state that doesn't exist: loads with the
    // triggers off, then fails the migration that validates the
    // workspace-scoped foreign keys.
    let broken = rewrite_fixture("backups/1.0.12/plain.zip", dir.path(), |name, content| {
        if name == "data/tickets.json" {
            String::from_utf8(content)
                .expect("utf-8")
                .replacen("\"workflow_state_id\":2", "\"workflow_state_id\":999", 1)
                .into_bytes()
        } else {
            content
        }
    });
    let db = TestDb::new();
    let before: Vec<String> = user_tables(&mut db.conn())
        .iter()
        .map(|t| hash_table(&mut db.conn(), t))
        .collect();

    let failed = backup_service::restore_database(&mut db.conn(), &broken, None, options(&db))
        .expect_err("the upgrade fails");
    assert!(
        matches!(failed, backup_service::BackupError::UpgradeFailed(_)),
        "{failed}"
    );
    assert!(failed.to_string().contains("migration"), "{failed}");

    let after: Vec<String> = user_tables(&mut db.conn())
        .iter()
        .map(|t| hash_table(&mut db.conn(), t))
        .collect();
    assert_eq!(before, after, "the live database is unchanged");
    assert_eq!(scratch_databases(&db), 0, "the scratch database is dropped");
}

/// While one restore runs, another of the same database is refused.
#[test]
fn a_second_restore_is_refused_while_one_runs() {
    with_upload_dir();
    let db = TestDb::new();
    // Stand in for a restore in progress: hold its lock.
    let mut holder = PgConnection::establish(db.url()).expect("connect");
    diesel::sql_query("SELECT pg_advisory_lock($1)")
        .bind::<BigInt, _>(backup_service::RESTORE_LOCK_KEY)
        .execute(&mut holder)
        .expect("take the restore lock");

    let refused =
        backup_service::restore_database(&mut db.conn(), &plain_1_0_12(), None, options(&db))
            .expect_err("refused while the lock is held");
    assert!(
        matches!(refused, backup_service::BackupError::RestoreInProgress),
        "{refused}"
    );

    drop(holder);
    backup_service::restore_database(&mut db.conn(), &plain_1_0_12(), None, options(&db))
        .expect("restores once the first is done");
}

/// A scratch database a killed restore left behind is dropped by the next
/// restore of the same database.
#[test]
fn a_leftover_scratch_database_is_dropped() {
    with_upload_dir();
    let db = TestDb::new();
    let leftover = format!("{}dead", scratch_prefix(&db));
    db.conn()
        .batch_execute(&format!("CREATE DATABASE \"{leftover}\""))
        .expect("plant a leftover");
    assert_eq!(scratch_databases(&db), 1);

    let archive = {
        let mut conn = db.conn();
        let job = seed_backup_job(&mut conn);
        backup_service::create_backup(&mut conn, job, None).expect("create backup")
    };
    backup_service::restore_database(&mut db.conn(), &archive, None, options(&db))
        .expect("restore");
    assert_eq!(scratch_databases(&db), 0, "the leftover is dropped");
}

/// A 1.0 backup predates `instance_settings`; restoring it keeps the live
/// row (the pasted licence, the push mode) instead of clearing it.
#[test]
fn instance_settings_survive_an_upgrade_restore() {
    with_upload_dir();
    let db = TestDb::new();
    db.conn()
        .batch_execute("INSERT INTO instance_settings (id, push_mode) VALUES (true, 'off')")
        .expect("instance settings");

    backup_service::restore_database(&mut db.conn(), &plain_1_0_12(), None, options(&db))
        .expect("restore");
    let kept: Name = diesel::sql_query("SELECT push_mode::text AS name FROM instance_settings")
        .get_result(&mut db.conn())
        .expect("the live row is kept");
    assert_eq!(kept.name, "off");
}

/// A table loaded in many small statements comes out identical to one
/// loaded in a single statement.
#[test]
fn a_chunked_load_matches_a_single_statement_load() {
    with_upload_dir();
    let source = TestDb::new();
    let archive = {
        let mut conn = source.conn();
        seed_two_workspaces(&mut conn);
        let job = seed_backup_job(&mut conn);
        // Encrypted, so no webhook secret is minted afresh on each restore.
        backup_service::create_backup(&mut conn, job, Some("chunks")).expect("create backup")
    };

    let whole = TestDb::new();
    backup_service::restore_database(&mut whole.conn(), &archive, Some("chunks"), options(&whole))
        .expect("restore in one statement per table");
    let chunked = TestDb::new();
    backup_service::restore_database(
        &mut chunked.conn(),
        &archive,
        Some("chunks"),
        backup_service::RestoreOptions {
            chunk_bytes: Some(256),
            ..options(&chunked)
        },
    )
    .expect("restore in chunks");

    for table in user_tables(&mut whole.conn()) {
        assert_eq!(
            hash_table(&mut whole.conn(), &table),
            hash_table(&mut chunked.conn(), &table),
            "{table}"
        );
    }
}
