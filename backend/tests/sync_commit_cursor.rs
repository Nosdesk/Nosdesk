//! Proves the commit-safe change-feed cursor never skips a row that
//! commits out of `sync_id` order.
//!
//! `sync_id` is a sequence assigned at INSERT, so its order can diverge
//! from commit order. The bug: writer A takes a lower `sync_id` but
//! commits *after* writer B (higher `sync_id`); a `sync_id > cursor`
//! drain run between the two commits delivers B, advances the cursor
//! past it, and then permanently skips A.
//!
//! The fix (see `backend::sync::feed`): record each row's `xid8` and
//! only serve rows below the commit horizon `pg_snapshot_xmin`, ordered
//! by `(xid8, sync_id)`. This test reproduces the exact interleaving
//! with two real connections — one holding an in-flight insert — and:
//!
//!   1. while A is in-flight, asserts the *naive* `sync_id > cursor`
//!      query (the old, buggy cursor) returns only B — i.e. it would
//!      deliver B, advance past it, and skip A. This negative control
//!      proves the scenario actually triggers the bug, so the positive
//!      assertions below aren't vacuous;
//!   2. asserts the commit-safe query (the real shipped
//!      `feed::below_horizon()` predicate) returns nothing yet — B is
//!      held back by the horizon while A's lower-`sync_id` txn is open;
//!   3. once A commits, asserts the commit-safe query delivers BOTH
//!      rows in `(xid8, sync_id)` order — neither skipped — while the
//!      naive cursor sitting where it advanced to (past B) now returns
//!      nothing, having lost A for good.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::BigInt;

use backend::schema::sync_actions;
use backend::sync::feed::below_horizon;

mod common;

#[derive(QueryableByName)]
struct SyncIdRow {
    #[diesel(sql_type = BigInt)]
    sync_id: i64,
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

/// Commit-safe delivery: test rows (`sync_id > floor`) below the commit
/// horizon, in `(xid8, sync_id)` order. Uses the real shipped
/// `feed::below_horizon()` predicate — the same one the delta endpoint
/// and the outbox drain filter on — so the test can't drift from the
/// production query.
fn settled_sync_ids(conn: &mut PgConnection, floor: i64) -> Vec<i64> {
    sync_actions::table
        .filter(below_horizon())
        .filter(sync_actions::sync_id.gt(floor))
        .order((sync_actions::xid8.asc(), sync_actions::sync_id.asc()))
        .select(sync_actions::sync_id)
        .load::<i64>(conn)
        .expect("settled rows")
}

/// The OLD, buggy cursor: `sync_id > floor` with no commit horizon.
/// Kept only as a negative control to demonstrate the skip.
fn naive_sync_ids(conn: &mut PgConnection, floor: i64) -> Vec<i64> {
    sync_actions::table
        .filter(sync_actions::sync_id.gt(floor))
        .order(sync_actions::sync_id.asc())
        .select(sync_actions::sync_id)
        .load::<i64>(conn)
        .expect("naive rows")
}

#[test]
fn out_of_order_commit_is_not_skipped() {
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    // Floor out any seed/pre-existing sync_actions so we only observe
    // the two rows this test writes.
    let floor: i64 = {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query("SELECT COALESCE(max(sync_id), 0) AS sync_id FROM sync_actions")
            .get_result::<SyncIdRow>(&mut conn)
            .expect("floor")
            .sync_id
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

    // 1. Negative control. While A is in-flight, the OLD `sync_id > floor`
    //    cursor sees B (committed) but not A (uncommitted, MVCC-invisible
    //    to conn_b). It would deliver B and advance its cursor to sync_b
    //    — exactly the move that strands A. This proves the interleaving
    //    really triggers the bug.
    assert_eq!(
        naive_sync_ids(&mut conn_b, floor),
        vec![sync_b],
        "the naive cursor delivers B and advances past it while A is in-flight"
    );

    // 2. The commit-safe query holds B back: with A's lower-sync_id txn
    //    still open, the horizon hasn't advanced past either row, so the
    //    cursor can't move and can't skip A.
    assert!(
        settled_sync_ids(&mut conn_b, floor).is_empty(),
        "B must be withheld while A's lower-sync_id txn is still in-flight"
    );

    // A commits. Both rows are now settled.
    diesel::sql_query("COMMIT")
        .execute(&mut conn_a)
        .expect("commit a");

    // 3a. The commit-safe query now delivers BOTH, in (xid8, sync_id)
    //     order — A first (lower xid8), neither skipped.
    assert_eq!(
        settled_sync_ids(&mut conn_b, floor),
        vec![sync_a, sync_b],
        "both rows delivered once settled, in commit-safe order, none skipped"
    );

    // 3b. Punchline: the naive cursor, having advanced to sync_b in step
    //     1, now sees nothing newer — A (lower sync_id) is lost for good.
    //     The commit-safe cursor is the only one that recovers it.
    assert!(
        naive_sync_ids(&mut conn_b, sync_b).is_empty(),
        "the naive cursor advanced past sync_b and has permanently skipped A"
    );
}
