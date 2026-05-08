//! Partition provisioning for `sync_actions` and `audit_log`.
//!
//! Both event tables are RANGE-partitioned by `occurred_at` on a
//! monthly cadence. The substrate migration provisions four months
//! by hand; in production we need partitions to keep rolling forward
//! so an INSERT after the last provisioned month doesn't error out.
//!
//! This module owns that responsibility. The architecture doc
//! mentions `pg_partman` as one option, but pg_partman is a separate
//! Postgres extension that complicates deploys (requires shared
//! preload, schema setup, ongoing version pinning). A small
//! purpose-built Rust task is simpler, easier to test, and good
//! enough for the cadence we need.
//!
//! Behaviour: on every call, ensure both tables have partitions for
//! the next 60 days. Idempotent — uses `pg_inherits` to detect
//! already-attached partitions and skips the create/attach when
//! they exist. The `partition_max_provisioned` system_meta key is
//! advanced once new partitions are created.
//!
//! ## Lock-friendly attach
//!
//! `CREATE TABLE x PARTITION OF parent FOR VALUES ...` takes
//! `ACCESS EXCLUSIVE` on `parent` for the duration, blocking every
//! concurrent INSERT and SELECT (Postgres docs §5.12.2.2). On a hot
//! audit_log / sync_actions parent that's a rolling freeze every
//! daily rotation tick — fine in dev, real at hosted-SaaS load.
//!
//! Two-step LIKE + ATTACH avoids it:
//! 1. `CREATE TABLE child (LIKE parent INCLUDING ALL)` — no parent
//!    lock; child is a free-standing table at this point.
//! 2. `ALTER TABLE child ADD CONSTRAINT … CHECK (time_col >= … AND
//!    time_col < …)` — pre-adds the matching range constraint so
//!    Postgres can skip the validation scan during attach.
//! 3. `ALTER TABLE parent ATTACH PARTITION child FOR VALUES …`
//!    takes only `SHARE UPDATE EXCLUSIVE` on parent; doesn't block
//!    concurrent reads / writes.
//! 4. `ALTER TABLE child DROP CONSTRAINT …` — the redundant CHECK is
//!    no longer needed once the partition bound enforces the same
//!    invariant.
//!
//! Reference: [Supabase: Dynamic Table Partitioning in Postgres](https://supabase.com/blog/postgres-dynamic-table-partitioning).

use chrono::{Datelike, NaiveDate, Utc};
use diesel::prelude::*;
use diesel::sql_types::Bool;
use serde_json::Value;
use tracing::info;

use crate::db::DbConnection;
use crate::sync::system_meta;

#[derive(QueryableByName)]
struct AttachCheck {
    #[diesel(sql_type = Bool)]
    attached: bool,
}

/// Returns true if `child` is already attached as a partition of
/// `parent`. Uses `to_regclass` so the lookup gracefully returns
/// false when either table doesn't exist yet.
fn is_attached(
    conn: &mut DbConnection,
    parent: &str,
    child: &str,
) -> Result<bool, diesel::result::Error> {
    let stmt = format!(
        "SELECT EXISTS ( \
            SELECT 1 FROM pg_inherits \
            WHERE inhparent = to_regclass('{parent}') \
              AND inhrelid  = to_regclass('{child}') \
         ) AS attached"
    );
    let rows: Vec<AttachCheck> = diesel::sql_query(stmt).load(conn)?;
    Ok(rows.first().map(|r| r.attached).unwrap_or(false))
}

/// Lock-friendly attach of a single monthly partition. No-op when the
/// partition is already attached. Wraps the four-step LIKE + ATTACH
/// dance in one transaction so a mid-sequence failure rolls back to a
/// clean state (next call sees an unattached child and proceeds).
fn ensure_one_partition(
    conn: &mut DbConnection,
    parent: &str,
    child: &str,
    time_col: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<(), diesel::result::Error> {
    if is_attached(conn, parent, child)? {
        return Ok(());
    }

    let from_iso = from.format("%Y-%m-%d").to_string();
    let to_iso = to.format("%Y-%m-%d").to_string();
    let constraint = format!("{child}_range_check");

    conn.transaction(|conn| {
        // 1. Free-standing table cloning the parent's structure.
        //    No lock on parent.
        diesel::sql_query(format!(
            "CREATE TABLE IF NOT EXISTS {child} (LIKE {parent} INCLUDING ALL)"
        ))
        .execute(conn)?;

        // 2. Make sure no leftover constraint from a prior partial
        //    run trips the ADD below. DROP IF EXISTS is idempotent.
        diesel::sql_query(format!(
            "ALTER TABLE {child} DROP CONSTRAINT IF EXISTS {constraint}"
        ))
        .execute(conn)?;

        // 3. Pre-add the matching CHECK so the subsequent ATTACH
        //    skips the partition validation scan (otherwise Postgres
        //    scans every row in the table to prove it fits the
        //    range).
        diesel::sql_query(format!(
            "ALTER TABLE {child} ADD CONSTRAINT {constraint} \
             CHECK ({time_col} >= '{from_iso}' AND {time_col} < '{to_iso}')"
        ))
        .execute(conn)?;

        // 4. Lock-friendly attach. SHARE UPDATE EXCLUSIVE on parent;
        //    doesn't block concurrent INSERT/SELECT.
        diesel::sql_query(format!(
            "ALTER TABLE {parent} ATTACH PARTITION {child} \
             FOR VALUES FROM ('{from_iso}') TO ('{to_iso}')"
        ))
        .execute(conn)?;

        // 5. The CHECK is now redundant with the partition bound;
        //    drop it so future schema introspection is clean.
        diesel::sql_query(format!(
            "ALTER TABLE {child} DROP CONSTRAINT {constraint}"
        ))
        .execute(conn)?;

        Ok(())
    })
}

/// Provision partitions for both event tables out to `lookahead_days`
/// past today. Idempotent: a `pg_inherits` lookup short-circuits
/// already-attached partitions, and the create/attach sequence is
/// transactional so partial failures don't leave half-attached
/// orphans. Returns the list of partition names considered (the union
/// of "newly created" and "already existed"); telling the two apart
/// would require an extra round-trip per partition for marginal
/// telemetry value.
pub fn ensure_partitions(
    conn: &mut DbConnection,
    lookahead_days: i64,
) -> Result<Vec<String>, diesel::result::Error> {
    let today = Utc::now().date_naive();
    let target = today + chrono::Duration::days(lookahead_days);

    let mut touched: Vec<String> = Vec::new();
    let mut month = first_of_month(today);
    while month <= target {
        let next = next_month(month);
        // Both event tables partition on `occurred_at`.
        for parent in &["sync_actions", "audit_log"] {
            let name = format!("{}_{:04}_{:02}", parent, month.year(), month.month());
            ensure_one_partition(conn, parent, &name, "occurred_at", month, next)?;
            touched.push(name);
        }
        month = next;
    }

    // Advance the watermark in system_meta. A failure here doesn't
    // unwind the partition creates — the watermark is a soft cache
    // that callers can recompute by calling this fn again or by
    // querying `pg_partition_tree('sync_actions')` directly. Bubble
    // the error so the scheduler logs it as a job failure rather
    // than silently masking it.
    let watermark = first_of_month(target).format("%Y-%m-01").to_string();
    system_meta::put(
        conn,
        system_meta::KEY_PARTITION_MAX_PROVISIONED,
        &Value::String(watermark.clone()),
    )?;

    info!(
        watermark = %watermark,
        partitions_touched = touched.len(),
        "Sync partitions ensured"
    );
    Ok(touched)
}

fn first_of_month(d: NaiveDate) -> NaiveDate {
    NaiveDate::from_ymd_opt(d.year(), d.month(), 1).expect("valid date")
}

fn next_month(d: NaiveDate) -> NaiveDate {
    let (y, m) = if d.month() == 12 {
        (d.year() + 1, 1)
    } else {
        (d.year(), d.month() + 1)
    };
    NaiveDate::from_ymd_opt(y, m, 1).expect("valid date")
}

/// Convenience: parse the watermark stored in system_meta back to a
/// NaiveDate. Returns None if the key is missing or unparsable.
pub fn read_watermark(conn: &mut DbConnection) -> Option<NaiveDate> {
    let raw = system_meta::get(conn, system_meta::KEY_PARTITION_MAX_PROVISIONED).ok()??;
    let s = raw.as_str()?;
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::setup_test_connection;

    #[test]
    fn ensure_partitions_is_idempotent() {
        let mut conn = setup_test_connection();
        // Already-provisioned months from the substrate migration
        // (May–Aug 2026) should not produce errors on a re-run.
        let touched = ensure_partitions(&mut conn, 60).expect("first run");
        // Touched should at least include current month; could
        // include more depending on `now()`. Just assert non-empty.
        assert!(!touched.is_empty());
        // Second call: same plan, all CREATE TABLE IF NOT EXISTS,
        // no error.
        let _ = ensure_partitions(&mut conn, 60).expect("second run");
    }

    #[test]
    fn watermark_round_trips() {
        let mut conn = setup_test_connection();
        let _ = ensure_partitions(&mut conn, 30).expect("ensure");
        let watermark = read_watermark(&mut conn);
        assert!(
            watermark.is_some(),
            "watermark should be readable after ensure_partitions"
        );
    }

    /// Newly-provisioned partitions actually attach to their parents
    /// (rather than landing as free-standing tables that the
    /// substrate would never route writes to). Catches regressions
    /// in the LIKE + ATTACH sequence.
    #[test]
    fn ensured_partitions_are_attached_to_parent() {
        let mut conn = setup_test_connection();
        let _ = ensure_partitions(&mut conn, 30).expect("ensure");
        // Whatever month the test runs in, both parents must have
        // an attached partition for it.
        let today = Utc::now().date_naive();
        let month = first_of_month(today);
        for parent in &["sync_actions", "audit_log"] {
            let name = format!("{}_{:04}_{:02}", parent, month.year(), month.month());
            assert!(
                is_attached(&mut conn, parent, &name).expect("is_attached"),
                "{name} should be attached as a partition of {parent}"
            );
        }
    }

    /// The redundant CHECK constraint added during the lock-friendly
    /// attach should be dropped before the function returns. A
    /// lingering constraint would clutter schema introspection and
    /// add a small cost to every row insert.
    #[test]
    fn no_residual_range_check_constraints() {
        use diesel::sql_types::BigInt;

        #[derive(QueryableByName)]
        struct Count {
            #[diesel(sql_type = BigInt)]
            n: i64,
        }

        let mut conn = setup_test_connection();
        let _ = ensure_partitions(&mut conn, 30).expect("ensure");
        let rows: Vec<Count> = diesel::sql_query(
            "SELECT count(*) AS n FROM pg_constraint \
             WHERE conname LIKE '%_range_check'",
        )
        .load(&mut conn)
        .expect("count constraints");
        assert_eq!(
            rows.first().map(|r| r.n).unwrap_or(-1),
            0,
            "no range_check constraints should remain after partition attach"
        );
    }
}
