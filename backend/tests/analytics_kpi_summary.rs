//! DB-backed coverage for the consolidated KPI-summary query
//! (`analytics::kpi_summary`).
//!
//! The summary collapses the dashboard KPI rail's three parallel
//! `/kpi` calls into one connection. This asserts the single-pass
//! conditional aggregation counts created / resolved / open correctly
//! against a controlled set of tickets, that the prior-period deltas
//! match, and that each sparkline sums to its headline count.
//!
//! Runs under the test pool's `app.workspace_id = 1` GUC, so RLS
//! scopes every aggregate to the seeded workspace exactly as it does
//! in production.

#![allow(clippy::expect_used)]

use chrono::{TimeZone, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Nullable, Timestamptz};

use backend::models::NewTicket;
use backend::repository::analytics::{self, KpiSummaryQuery};

mod common;

const WS: i32 = 1;

fn default_state_id(conn: &mut PgConnection) -> i32 {
    use backend::schema::workflow_states::dsl as s;
    s::workflow_states
        .filter(s::workspace_id.eq(WS))
        .filter(s::is_default.eq(true))
        .select(s::id)
        .first(conn)
        .expect("default workflow state seeded")
}

/// Insert a ticket then force its `created_at` / `closed_at` to fixed
/// instants so it lands in (or out of) the test windows. `closed`
/// `None` leaves the ticket open.
fn ticket(
    conn: &mut PgConnection,
    state_id: i32,
    created: chrono::DateTime<Utc>,
    closed: Option<chrono::DateTime<Utc>>,
) {
    use backend::schema::tickets;
    let id: i32 = diesel::insert_into(tickets::table)
        .values(&NewTicket {
            title: "t".to_string(),
            workflow_state_id: state_id,
            ..Default::default()
        })
        .returning(tickets::id)
        .get_result(conn)
        .expect("insert ticket");

    diesel::sql_query("UPDATE tickets SET created_at = $1, closed_at = $2 WHERE id = $3")
        .bind::<Timestamptz, _>(created)
        .bind::<Nullable<Timestamptz>, _>(closed)
        .bind::<diesel::sql_types::Integer, _>(id)
        .execute(conn)
        .expect("set ticket times");
}

#[test]
fn kpi_summary_counts_deltas_and_sparklines() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let state = default_state_id(&mut conn);

    // Window: 2026-03-10 .. 2026-03-17 (a week). Prior: the week before.
    let from = Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap();
    let to = Utc.with_ymd_and_hms(2026, 3, 17, 0, 0, 0).unwrap();
    let prior_from = Utc.with_ymd_and_hms(2026, 3, 3, 0, 0, 0).unwrap();
    let prior_to = from;

    let in_win = Utc.with_ymd_and_hms(2026, 3, 12, 9, 0, 0).unwrap();
    let in_win2 = Utc.with_ymd_and_hms(2026, 3, 13, 9, 0, 0).unwrap();
    let before = Utc.with_ymd_and_hms(2026, 3, 1, 9, 0, 0).unwrap();
    let in_prior = Utc.with_ymd_and_hms(2026, 3, 5, 9, 0, 0).unwrap();

    // created in-window, still open
    ticket(&mut conn, state, in_win, None);
    ticket(&mut conn, state, in_win2, None);
    // created before window, still open (open snapshot only)
    ticket(&mut conn, state, before, None);
    // created before window, resolved in-window
    ticket(&mut conn, state, before, Some(in_win));
    // created in-window, resolved in-window
    ticket(&mut conn, state, in_win, Some(in_win2));
    // created in the prior window (drives the created delta)
    ticket(&mut conn, state, in_prior, None);

    let q = KpiSummaryQuery {
        from,
        to,
        prior: Some((prior_from, prior_to)),
        include_sparkline: true,
        tz: "UTC".to_string(),
    };
    let r = analytics::kpi_summary(&mut conn, q).expect("kpi_summary");

    // created_at in [from, to): the two open-in-window + the one
    // created-and-resolved-in-window = 3.
    assert_eq!(r.created.value, 3, "created count");
    // closed_at in [from, to): the before->in-window resolve + the
    // in-window->in-window resolve = 2.
    assert_eq!(r.resolved.value, 2, "resolved count");
    // closed_at IS NULL (snapshot, window-independent): the three open
    // tickets + the one created in the prior window and never closed = 4.
    assert_eq!(r.open.value, 4, "open snapshot count");

    // Prior window created exactly one ticket; delta = 3 - 1 = 2,
    // pct = (2/1)*100 = 200.0.
    assert_eq!(r.created.delta_value, Some(2), "created delta");
    assert_eq!(r.created.delta_pct, Some(200.0), "created delta pct");
    // No ticket was resolved in the prior window; baseline 0 => pct
    // undefined (None), delta is the full value.
    assert_eq!(r.resolved.delta_value, Some(2), "resolved delta");
    assert_eq!(r.resolved.delta_pct, None, "resolved pct undefined vs zero");
    // Open is a snapshot: never a delta.
    assert_eq!(r.open.delta_value, None, "open has no delta");

    // Created/resolved sparklines sum to their headline counts.
    let created_spark = r.created.sparkline.expect("created sparkline");
    assert_eq!(
        created_spark.iter().sum::<i64>(),
        3,
        "created sparkline sums"
    );
    let resolved_spark = r.resolved.sparkline.expect("resolved sparkline");
    assert_eq!(
        resolved_spark.iter().sum::<i64>(),
        2,
        "resolved sparkline sums"
    );

    // Open is a backlog trend, not an event count: the open count at the
    // window start (tickets 3, 4, 6 => 3) plus the running net flow
    // (created − resolved) per day. `generate_series` spans both window
    // edges inclusively, so a 7-day window yields 8 daily buckets
    // (03-10..03-17), the last one empty.
    //   created  = [0,0,2,1,0,0,0,0]  (03-12: t1,t5; 03-13: t2)
    //   resolved = [0,0,1,1,0,0,0,0]  (03-12: t4;   03-13: t5)
    //   net      = [0,0,1,0,0,0,0,0]
    //   open     = 3 + cumsum(net) = [3,3,4,4,4,4,4,4]
    let open_spark = r.open.sparkline.expect("open sparkline");
    assert_eq!(
        open_spark,
        vec![3, 3, 4, 4, 4, 4, 4, 4],
        "open backlog series"
    );
    assert_eq!(
        open_spark.len(),
        created_spark.len(),
        "open series aligns with the created buckets"
    );
    assert_eq!(
        *open_spark.last().unwrap(),
        r.open.value,
        "the series ends on the open snapshot when the window ends now"
    );
}

#[test]
fn kpi_summary_without_prior_has_no_deltas() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let state = default_state_id(&mut conn);
    let from = Utc.with_ymd_and_hms(2026, 3, 10, 0, 0, 0).unwrap();
    let to = Utc.with_ymd_and_hms(2026, 3, 17, 0, 0, 0).unwrap();
    ticket(
        &mut conn,
        state,
        Utc.with_ymd_and_hms(2026, 3, 12, 9, 0, 0).unwrap(),
        None,
    );

    let q = KpiSummaryQuery {
        from,
        to,
        prior: None,
        include_sparkline: false,
        tz: "UTC".to_string(),
    };
    let r = analytics::kpi_summary(&mut conn, q).expect("kpi_summary");
    assert_eq!(r.created.value, 1);
    assert_eq!(r.created.delta_value, None, "no prior => no delta");
    assert_eq!(r.created.delta_pct, None);
    assert!(r.created.sparkline.is_none(), "sparkline suppressed");
}
