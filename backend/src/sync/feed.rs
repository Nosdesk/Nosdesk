//! Commit-safe change-feed cursor for `sync_actions`.
//!
//! `sync_id` is a sequence assigned at INSERT, so its order can diverge
//! from *commit* order — a transaction that takes a lower `sync_id` but
//! commits after a higher one was already drained gets skipped by a
//! `sync_id > cursor` query (both the SSE drain and `/delta` had this
//! bug). To make the feed commit-safe we record each row's transaction
//! id (`xid8`, stored as bigint; see migration
//! `2026-06-15-000000_sync_actions_commit_cursor`) and:
//!
//!   * order and cursor by the composite `(xid8, sync_id)`, and
//!   * only serve rows whose `xid8` is below the commit horizon
//!     `pg_snapshot_xmin(pg_current_snapshot())`. The PostgreSQL docs
//!     guarantee "all transaction IDs less than xmin are either
//!     committed and visible, or rolled back and dead", so such rows can
//!     never change after we read them.
//!
//! A late-committing lower-`sync_id` row carries a *higher* `xid8`, so it
//! sorts after the cursor and is delivered on the next poll instead of
//! being skipped.
//!
//! Tradeoff (accepted for v1): the horizon is the oldest in-flight
//! transaction *cluster-wide*, so a long-running transaction delays
//! delivery of newer events until it ends — a bounded, self-healing
//! delay rather than the silent permanent loss the sequence cursor had.
//! Storing the id as `bigint` rather than native `xid8` is deliberate:
//! `xid8` is 64-bit and does not wrap, the cast preserves the full value
//! (the only ceiling is 2^63, i.e. ~250k years), and `bigint` keeps the
//! column mappable by Diesel.
//!
//! This is the canonical Postgres CDC pattern (see Sequin's
//! "Postgres sequences can commit out-of-order").

use crate::db::DbConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool};

/// `(xid8, sync_id)` position in the commit-ordered feed. `Default`
/// `(0, 0)` is "before everything".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FeedCursor {
    pub xid8: i64,
    pub sync_id: i64,
}

/// Diesel predicate: this row's transaction has definitely settled (its
/// `xid8` is below the snapshot's xmin horizon). Use in `.filter(...)`.
pub fn below_horizon() -> diesel::expression::SqlLiteral<Bool> {
    diesel::dsl::sql::<Bool>("xid8 < (pg_snapshot_xmin(pg_current_snapshot())::text::bigint)")
}

/// Read the current commit horizon (`pg_snapshot_xmin`) as a bigint.
/// Used to seed a fresh client's cursor at bootstrap: everything the
/// bootstrap snapshot can see has `xid8 < horizon`, and the delta feed
/// serves `xid8 >= horizon`, so the two partition with no gap and no
/// loss (a small re-delivery overlap is harmless — the client dedupes
/// by `sync_id`).
pub fn current_horizon(conn: &mut DbConnection) -> QueryResult<i64> {
    diesel::select(diesel::dsl::sql::<BigInt>(
        "(pg_snapshot_xmin(pg_current_snapshot())::text::bigint)",
    ))
    .get_result(conn)
}

/// Whether the feed holds the row that marks a change in what `user_uuid`
/// may see (`repository::documentation::ACCESS_CHANGED`) in the span one
/// delta page moved the cursor over, from `from` to `to`. Read apart from the
/// page's own query because the row is addressed to that person alone
/// (`user:<uuid>`), a group clients don't ask the delta for.
///
/// The span is whole transactions: those with `from.xid8 < xid8 <= to.xid8`.
/// The marker shares a transaction with the write that changed the access,
/// and may come after that write's last row on the page (a group join records
/// the membership first), so a span cut at `to.sync_id` would miss it until
/// some later write moved the cursor on. A page that ends inside a transaction
/// claims all of it, and the next page, starting at that xid8, claims none of
/// it, so each marker is reported once. Every transaction up to `to.xid8` has
/// settled, since a row of it is on the page. With `legacy` (a cursor without an
/// xid8, from clients that predate this flag) the span is by `sync_id`, as
/// that delta reads.
pub fn access_changed_between(
    conn: &mut DbConnection,
    user_uuid: uuid::Uuid,
    from: FeedCursor,
    to: FeedCursor,
    legacy: bool,
) -> QueryResult<bool> {
    use crate::schema::sync_actions::dsl as sa;
    let marker = sa::sync_actions
        .filter(sa::aggregate.eq(crate::models::SyncAggregate::User))
        .filter(sa::aggregate_id.eq(user_uuid.to_string()))
        .filter(sa::event_type.eq(crate::repository::documentation::ACCESS_CHANGED))
        .filter(below_horizon())
        .into_boxed();
    let in_span = if legacy {
        if to.sync_id <= from.sync_id {
            return Ok(false);
        }
        marker
            .filter(sa::sync_id.gt(from.sync_id))
            .filter(sa::sync_id.le(to.sync_id))
    } else {
        if to.xid8 <= from.xid8 {
            return Ok(false);
        }
        marker
            .filter(sa::xid8.gt(from.xid8))
            .filter(sa::xid8.le(to.xid8))
    };
    diesel::select(diesel::dsl::exists(in_span)).get_result(conn)
}
