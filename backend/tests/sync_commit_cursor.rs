//! Proves the commit-safe change-feed cursor never skips a row that
//! commits out of `sync_id` order.
//!
//! `sync_id` is a sequence assigned at INSERT, so its order can diverge
//! from commit order. The bug: writer A takes a lower `sync_id` but
//! commits *after* writer B (higher `sync_id`); a `sync_id > cursor`
//! drain run between the two commits delivers B, advances the cursor
//! past it, and then permanently skips A.
//!
//! The fix (see `crate::sync::feed`): record each row's `xid8` and only
//! serve rows below the commit horizon `pg_snapshot_xmin`, ordered by
//! `(xid8, sync_id)`. This test reproduces the exact interleaving with
//! two real connections — one holding an in-flight insert — and asserts:
//!
//!   1. while A is in-flight, B (committed, higher sync_id) is held back
//!      by the horizon (so the cursor can't advance past it and skip A);
//!   2. once A commits, BOTH rows are delivered in `(xid8, sync_id)`
//!      order — neither is lost.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::BigInt;

mod common;

#[derive(QueryableByName)]
struct SyncIdRow {
    #[diesel(sql_type = BigInt)]
    sync_id: i64,
}

#[derive(QueryableByName)]
struct CountRow {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Insert one `sync_actions` row on `conn` (workspace 1) and return its
/// `sync_id`. The row's `xid8`/`workspace_id` come from their column
/// defaults, so the connection must have `app.workspace_id` set.
fn insert_action(conn: &mut PgConnection, aggregate_id: &str) -> i64 {
    diesel::sql_query(format!(
        "INSERT INTO sync_actions (aggregate, aggregate_id, op, event_type, data, groups) \
         VALUES ('ticket', '{aggregate_id}', 'U', 'ticket.updated', '{{}}'::jsonb, \
         ARRAY['workspace:1']) RETURNING sync_id"
    ))
    .get_result::<SyncIdRow>(conn)
    .expect("insert sync_action")
    .sync_id
}

/// Count test rows (sync_id > floor) currently below the commit horizon.
fn settled_count(conn: &mut PgConnection, floor: i64) -> i64 {
    diesel::sql_query(format!(
        "SELECT count(*) AS n FROM sync_actions \
         WHERE sync_id > {floor} \
           AND xid8 < (pg_snapshot_xmin(pg_current_snapshot())::text::bigint)"
    ))
    .get_result::<CountRow>(conn)
    .expect("settled count")
    .n
}

/// Settled test rows in commit-safe `(xid8, sync_id)` order.
fn settled_sync_ids(conn: &mut PgConnection, floor: i64) -> Vec<i64> {
    diesel::sql_query(format!(
        "SELECT sync_id FROM sync_actions \
         WHERE sync_id > {floor} \
           AND xid8 < (pg_snapshot_xmin(pg_current_snapshot())::text::bigint) \
         ORDER BY xid8, sync_id"
    ))
    .load::<SyncIdRow>(conn)
    .expect("settled rows")
    .into_iter()
    .map(|r| r.sync_id)
    .collect()
}

#[test]
fn out_of_order_commit_is_not_skipped() {
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    // Floor out any seed/pre-existing sync_actions so we only observe
    // the two rows this test writes.
    let floor: i64 = {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query("SELECT COALESCE(max(sync_id), 0) AS n FROM sync_actions")
            .get_result::<CountRow>(&mut conn)
            .expect("floor")
            .n
    };

    // Writer A: insert in an OPEN transaction and hold it — A has a lower
    // sync_id but has not committed yet.
    let mut conn_a = pool.get().expect("conn_a");
    diesel::sql_query("BEGIN")
        .execute(&mut conn_a)
        .expect("begin a");
    diesel::sql_query("SET LOCAL app.workspace_id = '1'")
        .execute(&mut conn_a)
        .expect("guc a");
    let sync_a = insert_action(&mut conn_a, "A");

    // Writer B: insert and COMMIT (autocommit) — higher sync_id, but it
    // settles first.
    let mut conn_b = pool.get().expect("conn_b");
    diesel::sql_query("SET app.workspace_id = '1'")
        .execute(&mut conn_b)
        .expect("guc b");
    let sync_b = insert_action(&mut conn_b, "B");
    assert!(sync_b > sync_a, "B must take a higher sync_id than A");

    // 1. While A is in-flight, the horizon must hold B back. The OLD
    //    `sync_id > cursor` drain would have delivered B here and then
    //    skipped A forever. The commit-safe query sees neither yet.
    assert_eq!(
        settled_count(&mut conn_b, floor),
        0,
        "B must be withheld while A's lower-sync_id txn is still in-flight"
    );

    // 2. A commits. Now both are settled and delivered in (xid8, sync_id)
    //    order — A first (it took the lower xid8), neither skipped.
    diesel::sql_query("COMMIT")
        .execute(&mut conn_a)
        .expect("commit a");

    assert_eq!(
        settled_sync_ids(&mut conn_b, floor),
        vec![sync_a, sync_b],
        "both rows delivered once settled, in commit-safe order, none skipped"
    );
}
