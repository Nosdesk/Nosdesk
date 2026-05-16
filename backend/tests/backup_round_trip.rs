//! End-to-end backup smoke test.
//!
//! Seed → `create_backup` → DELETE the seeded rows → `restore_database` →
//! verify the deleted rows came back. Marked `#[ignore]` because:
//!
//!   * Writes commit (no test transaction) so the test must clean up after
//!     itself by deleting rows it inserted by primary key. Concurrent test
//!     runs against the same database would interfere.
//!   * The backup zip is written under `UPLOAD_DIR/backups/`, persisting
//!     across runs unless deleted (which we do).
//!
//! Run as part of pre-release sanity check:
//!
//! ```text
//! cd backend && cargo test --test backup_round_trip -- --ignored
//! ```
//!
//! Operators should run this in isolation (no other tests in parallel).
//!
//! Why integration test, not unit test: the library-side `test_helpers`
//! module is `#[cfg(test)]` gated, so it's invisible to the integration
//! test binary. We inline what we need (pool builder + user insert) here.

use backend::models::{NewBackupJob, NewUser, User, UserRole};
use backend::repository::backup as backup_repo;
use backend::services::backup as backup_service;
use diesel::prelude::*;
use diesel::r2d2::{self, ConnectionManager};
use diesel::sql_types::BigInt;
use uuid::Uuid;

type Pool = r2d2::Pool<ConnectionManager<diesel::PgConnection>>;

fn build_pool() -> Pool {
    dotenvy::dotenv().ok();
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set for integration tests");
    let manager = ConnectionManager::<diesel::PgConnection>::new(url);
    r2d2::Pool::builder()
        .max_size(2)
        .build(manager)
        .expect("build pool")
}

#[derive(diesel::QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    count: i64,
}

fn count_table(conn: &mut diesel::PgConnection, table: &str) -> i64 {
    let q = format!("SELECT COUNT(*) AS count FROM {}", table);
    diesel::sql_query(q)
        .get_result::<CountRow>(conn)
        .unwrap_or_else(|e| panic!("count of {table} failed: {e}"))
        .count
}

fn insert_user(conn: &mut diesel::PgConnection, name: &str) -> User {
    use backend::schema::users;
    let new_user = NewUser {
        uuid: Uuid::new_v4(),
        name: name.to_string(),
        role: UserRole::User,
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

fn delete_users(conn: &mut diesel::PgConnection, uuids: &[Uuid]) {
    use backend::schema::users;
    diesel::delete(users::table.filter(users::uuid.eq_any(uuids)))
        .execute(conn)
        .expect("delete users by uuid");
}

#[ignore = "destructive smoke test; run pre-release with --ignored"]
#[test]
fn backup_round_trip_preserves_seeded_rows() {
    let pool = build_pool();
    let mut conn = pool.get().expect("pooled connection");

    // Capture pre-test baseline so cleanup at the end can assert we
    // returned the database to the same state we found it in.
    let users_pre = count_table(&mut conn, "users");

    // Seed: 5 users with deterministic prefix so the cleanup DELETE is
    // narrowly scoped to rows we own.
    let nonce = Uuid::now_v7();
    let mut seeded_uuids: Vec<Uuid> = Vec::new();
    for i in 0..5 {
        let u = insert_user(&mut conn, &format!("backup-rt-{i}-{nonce}"));
        seeded_uuids.push(u.uuid);
    }

    let users_after_seed = count_table(&mut conn, "users");
    assert_eq!(
        users_after_seed,
        users_pre + 5,
        "seed inserted exactly 5 users"
    );

    // create_backup expects a backup_jobs row to update on completion;
    // mirror what handlers/backup.rs:61 does and seed one first.
    let job = backup_repo::create_backup_job(
        &mut conn,
        NewBackupJob {
            job_type: "export".to_string(),
            status: "processing".to_string(),
            include_sensitive: false,
            created_by: None,
        },
    )
    .expect("create_backup_job seeded");

    // Create the backup. The zip lands under UPLOAD_DIR/backups/ (default
    // /app/uploads/backups in the dev container); we reach back in to
    // delete it during cleanup so a re-run starts clean.
    let backup_path =
        backup_service::create_backup(&mut conn, job.id, None).expect("create_backup succeeded");
    assert!(backup_path.exists(), "backup zip exists on disk");

    // Wipe the seeded rows. Restore should reinsert them.
    delete_users(&mut conn, &seeded_uuids);
    let users_after_delete = count_table(&mut conn, "users");
    assert_eq!(
        users_after_delete, users_pre,
        "delete removed all seeded users"
    );

    // The new restore semantics are full-replace, not merge: we
    // truncate the user-table chain before restoring. CASCADE
    // unwinds FKs from comments / tickets / etc. and the backup
    // brings those back too. `force_non_empty: true` is required
    // because the test DB still has other tables (settings,
    // workflow_states, etc.) populated by migrations.
    diesel::sql_query("TRUNCATE TABLE users RESTART IDENTITY CASCADE")
        .execute(&mut conn)
        .expect("truncate users cascade");

    let stats = backup_service::restore_database(
        &mut conn,
        &backup_path,
        None,
        backup_service::RestoreOptions {
            force_non_empty: true,
            ignore_schema_mismatch: false,
        },
    )
    .expect("restore_database succeeded");
    assert!(stats.tables_restored > 0, "at least one table restored");
    assert!(
        stats.records_restored >= 5,
        "at least our 5 seeded users restored"
    );

    let users_after_restore = count_table(&mut conn, "users");
    assert_eq!(
        users_after_restore, users_after_seed,
        "restore brought back exactly the deleted users"
    );

    // Verify the restored rows are the actual ones we seeded (not just
    // any 5 users that happened to appear).
    use backend::schema::users;
    let restored_count: i64 = users::table
        .filter(users::uuid.eq_any(&seeded_uuids))
        .count()
        .get_result(&mut conn)
        .expect("count restored users");
    assert_eq!(
        restored_count, 5,
        "all 5 seeded uuids are present after restore"
    );

    // Cleanup: drop seeded rows, the backup_jobs row, and the backup file.
    // Leaves the database at the same row count we observed at test start.
    delete_users(&mut conn, &seeded_uuids);
    let users_final = count_table(&mut conn, "users");
    assert_eq!(users_final, users_pre, "cleanup restored pre-test count");

    use backend::schema::backup_jobs;
    let _ =
        diesel::delete(backup_jobs::table.filter(backup_jobs::id.eq(job.id))).execute(&mut conn);

    let _ = std::fs::remove_file(&backup_path);
}
