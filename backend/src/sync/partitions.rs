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
use diesel::sql_types::{Bool, Text};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::db::DbConnection;
use crate::sync::system_meta;

#[derive(QueryableByName)]
struct AttachCheck {
    #[diesel(sql_type = Bool)]
    attached: bool,
}

/// Postgres identifiers are formatted directly into DDL strings here
/// (binds don't apply to identifiers), so any caller that hands user
/// input to these helpers becomes a SQL-injection vector. Today every
/// caller passes static literals (`"sync_actions"`, `"audit_log"`,
/// `"occurred_at"`, plus the chrono-formatted partition name), but
/// the type signature is just `&str`. This validator forecloses the
/// foot-gun: identifiers must match `^[a-z_][a-z0-9_]{0,62}$`. The
/// 63-char limit matches Postgres' `NAMEDATALEN - 1` truncation
/// boundary.
fn validate_identifier(s: &str) -> Result<(), diesel::result::Error> {
    use diesel::result::{DatabaseErrorKind, Error};

    if s.is_empty() || s.len() > 63 {
        return Err(Error::DatabaseError(
            DatabaseErrorKind::Unknown,
            Box::new(format!("invalid identifier length: {:?}", s)),
        ));
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !(first.is_ascii_lowercase() || first == '_') {
        return Err(Error::DatabaseError(
            DatabaseErrorKind::Unknown,
            Box::new(format!("invalid identifier first char: {:?}", s)),
        ));
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
        return Err(Error::DatabaseError(
            DatabaseErrorKind::Unknown,
            Box::new(format!("invalid identifier chars: {:?}", s)),
        ));
    }
    Ok(())
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
///
/// ## Multi-machine safety
///
/// Hosted runs multiple app instances that each provision partitions at
/// boot and on a daily tick against the *same* database. Concurrent
/// provisioning of the same child is not benign: `CREATE TABLE IF NOT
/// EXISTS` still races (two sessions abort one with a duplicate
/// `pg_type` row), and `ATTACH PARTITION` of an already-attached child
/// errors outright. Either aborts the whole transaction, which is fatal
/// on the eager startup path (the process refuses to bind the listener).
///
/// A transaction-scoped advisory lock keyed on the child name is taken
/// as the *first* statement inside the transaction so the entire
/// create + attach sequence is single-writer across machines. It
/// auto-releases on commit or rollback, so a mid-sequence failure can't
/// strand it. This mirrors the boot-time serialization in
/// `services::admin_setup`. After acquiring the lock we re-check
/// `is_attached` (READ COMMITTED, so we see a peer's committed ATTACH)
/// and no-op if a peer won the race.
fn ensure_one_partition(
    conn: &mut DbConnection,
    parent: &str,
    child: &str,
    time_col: &str,
    from: NaiveDate,
    to: NaiveDate,
) -> Result<(), diesel::result::Error> {
    validate_identifier(parent)?;
    validate_identifier(child)?;
    validate_identifier(time_col)?;

    if is_attached(conn, parent, child)? {
        return Ok(());
    }

    let from_iso = from.format("%Y-%m-%d").to_string();
    let to_iso = to.format("%Y-%m-%d").to_string();
    let constraint = format!("{child}_range_check");

    conn.transaction(|conn| {
        // 0. Serialize concurrent provisioners on this child before any DDL
        //    (see "Multi-machine safety" above). Namespaced hashtext keeps
        //    the key clear of the fixed advisory-lock keys elsewhere; a
        //    collision would only cost a brief wait, never correctness.
        diesel::sql_query("SELECT pg_advisory_xact_lock(hashtext('partition:' || $1))")
            .bind::<Text, _>(child)
            .execute(conn)?;

        // 0a. Re-check under the lock: a peer may have created + attached this
        //     child between the lock-free fast path above and this point.
        if is_attached(conn, parent, child)? {
            return Ok(());
        }

        // 1. Free-standing table cloning the parent's structure.
        //    No lock on parent.
        diesel::sql_query(format!(
            "CREATE TABLE IF NOT EXISTS {child} (LIKE {parent} INCLUDING ALL)"
        ))
        .execute(conn)?;

        // 1a. Match the parent's owner (Phase 3i.4: nosdesk_admin).
        //     CREATE TABLE sets owner = current session role, which
        //     here is the migration / scheduler login role (usually
        //     `nosdesk` superuser). Without this step, new children
        //     drift away from the parent's `nosdesk_admin` ownership
        //     and the partition rotator silently regains its
        //     superuser dependency for the DDL below.
        diesel::sql_query(format!("ALTER TABLE {child} OWNER TO nosdesk_admin")).execute(conn)?;

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
        diesel::sql_query(format!("ALTER TABLE {child} DROP CONSTRAINT {constraint}"))
            .execute(conn)?;

        // 6. CREATE TABLE LIKE INCLUDING ALL does NOT copy RLS
        //    state. Without these, a direct query against the
        //    child bypasses the parent's workspace-isolation
        //    policy entirely (a non-superuser nosdesk_app role
        //    with blanket SELECT can read everything in the
        //    child). Mirror the parent's policy here so the
        //    child is fail-closed for direct queries and
        //    transparent for parent-routed queries (which is the
        //    normal path; partitions are an implementation
        //    detail of audit_log / sync_actions).
        let policy_name = format!("{child}_workspace_isolation");
        diesel::sql_query(format!("ALTER TABLE {child} ENABLE ROW LEVEL SECURITY"))
            .execute(conn)?;
        diesel::sql_query(format!("ALTER TABLE {child} FORCE ROW LEVEL SECURITY")).execute(conn)?;
        diesel::sql_query(format!("DROP POLICY IF EXISTS {policy_name} ON {child}"))
            .execute(conn)?;
        diesel::sql_query(format!(
            "CREATE POLICY {policy_name} ON {child} \
             USING (workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int) \
             WITH CHECK (workspace_id = NULLIF(current_setting('app.workspace_id', true), '')::int)"
        ))
        .execute(conn)?;

        // 7. REVOKE app-role access on the new child so direct
        //    queries (`SELECT FROM audit_log_2026_07`) fail loudly
        //    with "permission denied" rather than returning zero
        //    rows via RLS. Postgres routes parent-queries through
        //    parent ACLs, so the legitimate access path is
        //    unaffected. Defense in depth for any future code
        //    that accidentally hardcodes a partition name.
        diesel::sql_query(format!(
            "REVOKE SELECT, INSERT, UPDATE, DELETE ON {child} FROM nosdesk_app, nosdesk_admin"
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

    // W6b: report any rows that landed in the default partitions. If the
    // rotator catches up (this very call), future writes go to the proper
    // monthly child, but rows already in the default need an operator-run
    // recovery (move them, then drop the default partition for that month
    // so the index plan stays clean). Logging here surfaces the lag without
    // failing the job.
    check_default_partition_drift(conn, "audit_log_default");
    check_default_partition_drift(conn, "sync_actions_default");

    Ok(touched)
}

/// List the names of `parent`'s range-partition children whose upper bound
/// is `<=` `cutoff`. The default partition is deliberately excluded — it
/// has no range bound, and dropping it would close W6b's parachute.
///
/// This is split from `drop_partitions_older_than` so it can be exercised
/// from inside a test transaction (the actual DETACH CONCURRENTLY can't be).
pub fn partitions_eligible_for_drop(
    conn: &mut DbConnection,
    parent: &str,
    cutoff: NaiveDate,
) -> Result<Vec<String>, diesel::result::Error> {
    use diesel::sql_types::Text;

    #[derive(diesel::QueryableByName)]
    struct PartitionInfo {
        #[diesel(sql_type = Text)]
        child_name: String,
        #[diesel(sql_type = Text)]
        range_expr: String,
    }

    // pg_get_expr renders the partition bound expression as
    // `FOR VALUES FROM ('<ts>') TO ('<ts>')` for our RANGE partitions; we
    // parse the upper bound below.
    let rows: Vec<PartitionInfo> = diesel::sql_query(
        "SELECT
             c.relname AS child_name,
             pg_get_expr(c.relpartbound, c.oid) AS range_expr
         FROM pg_inherits i
         JOIN pg_class c ON c.oid = i.inhrelid
         JOIN pg_class p ON p.oid = i.inhparent
         WHERE p.relname = $1
         AND c.relname <> $2",
    )
    .bind::<Text, _>(parent)
    .bind::<Text, _>(format!("{parent}_default"))
    .load(conn)?;

    let mut eligible = Vec::new();
    for row in rows {
        let Some(upper_bound) = parse_partition_upper_bound(&row.range_expr) else {
            debug!(
                child = %row.child_name,
                expr = %row.range_expr,
                "skipping partition: could not parse upper bound"
            );
            continue;
        };
        if upper_bound <= cutoff {
            eligible.push(row.child_name);
        }
    }
    Ok(eligible)
}

/// Drop range partitions of `parent` whose upper bound is `<=` `cutoff`.
///
/// Uses DETACH PARTITION CONCURRENTLY (PG14+) so the parent's lock window
/// stays at SHARE UPDATE EXCLUSIVE — concurrent reads/writes on the parent
/// keep flowing. The plain ATTACH dual was W6a's lock-friendly partner.
///
/// CONCURRENTLY can't run inside a BEGIN block (Postgres rejects with a
/// hard error); this helper assumes the caller is operating in autocommit
/// (Diesel's default for raw sql_query outside `conn.transaction(...)`).
/// The detach + drop sequence is emitted as two separate statements, with
/// no surrounding transaction.
///
/// Returns the names of partitions that were detached + dropped.
pub fn drop_partitions_older_than(
    conn: &mut DbConnection,
    parent: &str,
    cutoff: NaiveDate,
) -> Result<Vec<String>, diesel::result::Error> {
    let eligible = partitions_eligible_for_drop(conn, parent, cutoff)?;
    let mut dropped = Vec::new();
    for child in eligible {
        let detach = format!("ALTER TABLE {parent} DETACH PARTITION {child} CONCURRENTLY");
        diesel::sql_query(&detach).execute(conn)?;
        let drop = format!("DROP TABLE {child}");
        diesel::sql_query(&drop).execute(conn)?;
        dropped.push(child);
    }
    Ok(dropped)
}

/// Parse the upper bound out of a partition's FOR VALUES clause.
///
/// Format: `FOR VALUES FROM ('<ts>') TO ('<ts>')`. Postgres normalises
/// the bound literal to a full timestamp like
/// `'2026-06-01 00:00:00+00'` even when the partition was created from
/// a date-only literal, so we accept the date prefix and ignore the rest.
/// Returns `None` for the DEFAULT partition's `DEFAULT` literal and any
/// other unparseable shape.
fn parse_partition_upper_bound(expr: &str) -> Option<NaiveDate> {
    let to_idx = expr.find(" TO (")?;
    let after_to = &expr[to_idx + 5..];
    let start = after_to.find('\'')? + 1;
    let end = after_to[start..].find('\'')?;
    let bound = &after_to[start..start + end];
    // Take the first 10 chars (YYYY-MM-DD) — this works whether Postgres
    // rendered the bound as a bare date or a full timestamptz.
    let date_str = bound.get(..10)?;
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

/// Best-effort drift check. Errors are logged, not propagated — the rotator's
/// job is to provision partitions; default-partition observation is an
/// adjacent concern that shouldn't fail the rotation tick.
fn check_default_partition_drift(conn: &mut DbConnection, table: &str) {
    use diesel::sql_types::BigInt;

    #[derive(diesel::QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = BigInt)]
        count: i64,
    }

    // The default partition may not exist yet on databases that pre-date the
    // 2026-05-11-200000_default_partitions migration; treat ProgrammingError
    // (relation does not exist) as zero rows rather than logging spuriously.
    let q = format!("SELECT COUNT(*) AS count FROM {}", table);
    match diesel::sql_query(q).get_result::<CountRow>(conn) {
        Ok(row) if row.count > 0 => {
            warn!(
                table = table,
                rows = row.count,
                "default partition has rows; rotation may have lagged. \
                 See docs/runbooks/partition-recovery.md"
            );
        }
        Ok(_) => {}
        Err(e) => {
            debug!(
                table = table,
                error = %e,
                "default partition drift check skipped (likely missing on legacy schema)"
            );
        }
    }
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

    /// A connection for exercising partition provisioning DDL.
    ///
    /// `setup_test_connection` drops to the RLS-scoped `nosdesk_app` role,
    /// which (like production's runtime role) can't `CREATE`/`ATTACH`
    /// partitions (no `CREATE` on schema `public`, not the parents' owner).
    /// Production runs provisioning on the privileged `MIGRATION_DATABASE_URL`
    /// role, so these tests `RESET ROLE` back to the superuser login role to
    /// match. Without it they'd only pass while the migration's fixed seed
    /// months still cover `now() + lookahead`, a calendar-dependent green.
    fn provisioning_conn() -> DbConnection {
        let mut conn = setup_test_connection();
        diesel::sql_query("RESET ROLE")
            .execute(&mut conn)
            .expect("reset to privileged role for partition DDL");
        conn
    }

    #[test]
    fn ensure_partitions_is_idempotent() {
        let mut conn = provisioning_conn();
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
        let mut conn = provisioning_conn();
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
        let mut conn = provisioning_conn();
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

    #[test]
    fn validate_identifier_accepts_table_names_used_in_practice() {
        assert!(validate_identifier("sync_actions").is_ok());
        assert!(validate_identifier("audit_log").is_ok());
        assert!(validate_identifier("occurred_at").is_ok());
        assert!(validate_identifier("sync_actions_2026_05").is_ok());
        assert!(validate_identifier("_leading_underscore").is_ok());
    }

    #[test]
    fn validate_identifier_rejects_injection_attempts() {
        // Each of these would, if format!'d into DDL, land in a
        // place a real attacker would target.
        assert!(validate_identifier("").is_err());
        assert!(validate_identifier("Capitalised").is_err());
        assert!(validate_identifier("with-dash").is_err());
        assert!(validate_identifier("with space").is_err());
        assert!(validate_identifier("trailing;DROP TABLE").is_err());
        assert!(validate_identifier("'quoted'").is_err());
        assert!(validate_identifier("9_leading_digit").is_err());
        // 64 chars: just past the Postgres NAMEDATALEN-1 boundary.
        assert!(validate_identifier(&"a".repeat(64)).is_err());
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

        let mut conn = provisioning_conn();
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

    #[test]
    fn parse_partition_upper_bound_handles_canonical_form() {
        // Bare-date form (what we'd write in CREATE TABLE).
        let expr = "FOR VALUES FROM ('2026-05-01') TO ('2026-06-01')";
        assert_eq!(
            parse_partition_upper_bound(expr),
            Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap())
        );
        // Full-timestamp form (what pg_get_expr actually returns for a
        // timestamptz partitioning column — the date literal is normalised
        // to a full instant).
        let expr = "FOR VALUES FROM ('2026-05-01 00:00:00+00') TO ('2026-06-01 00:00:00+00')";
        assert_eq!(
            parse_partition_upper_bound(expr),
            Some(NaiveDate::from_ymd_opt(2026, 6, 1).unwrap())
        );
    }

    #[test]
    fn parse_partition_upper_bound_returns_none_for_default() {
        // pg_get_expr renders the default partition as `DEFAULT`, not the
        // FOR VALUES form. We must skip those rather than parse them.
        assert_eq!(parse_partition_upper_bound("DEFAULT"), None);
        assert_eq!(parse_partition_upper_bound(""), None);
    }

    /// The candidate query picks up partitions whose upper bound is past
    /// the cutoff, and never the default partition. Driven through the
    /// candidate-only path because the actual DETACH CONCURRENTLY in
    /// `drop_partitions_older_than` cannot run inside a transaction block
    /// (Postgres rejects), and our test runner wraps every connection in
    /// one. The full DDL path is exercised in production at runtime; we
    /// rely on Postgres' own well-tested DETACH semantics for the rest.
    #[test]
    fn partitions_eligible_for_drop_finds_old_ranges_and_skips_default() {
        let mut conn = setup_test_connection();
        let _ = ensure_partitions(&mut conn, 30).expect("ensure");
        let cutoff = Utc::now().date_naive() + chrono::Duration::days(36500);

        let eligible =
            partitions_eligible_for_drop(&mut conn, "audit_log", cutoff).expect("eligible");

        assert!(
            !eligible.is_empty(),
            "expected at least one range partition to be eligible, got an empty list"
        );
        assert!(
            eligible.iter().all(|n| n != "audit_log_default"),
            "default partition was incorrectly included: {eligible:?}"
        );
    }

    /// A cutoff in the past matches no partition; the eligible list is empty.
    #[test]
    fn partitions_eligible_for_drop_empty_when_cutoff_is_in_distant_past() {
        let mut conn = setup_test_connection();
        let _ = ensure_partitions(&mut conn, 30).expect("ensure");
        let cutoff = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();

        let eligible =
            partitions_eligible_for_drop(&mut conn, "audit_log", cutoff).expect("eligible");
        assert!(
            eligible.is_empty(),
            "no partitions should match an ancient cutoff"
        );
    }
}
