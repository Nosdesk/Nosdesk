//! Analytics aggregation queries for the dashboard's chart widgets.
//!
//! Each public function takes a typed query struct and returns a
//! typed result. Workspace scope comes from the actor context the
//! `TenantConn` extractor sets on the connection, so callers run
//! these via `tc.run(|c| analytics::kpi(c, q))` rather than passing
//! a workspace id explicitly. The RLS policy on `tickets` filters
//! every row that hits these aggregations to the active workspace.
//!
//! This wave (Phase 4) ships the foundation: count-based KPIs and
//! count-based daily time-series over the tickets dataset. The other
//! query kinds in docs/dashboard-and-analytics-plan.md §13.5
//! (`breakdown`, `heatmap`, `leaderboard`, `audit_annotations`) land
//! in later waves and slot into this module alongside these helpers.

use chrono::{DateTime, Utc};
use diesel::dsl::sql;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Timestamptz};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::TicketPriority;
use crate::schema::{audit_log, tickets};

/// Metric identifiers the KPI endpoint understands. Each variant
/// corresponds to a single deterministic SQL aggregation; the
/// frontend's chart-config form validates against the same set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KpiMetric {
    /// Tickets created in the time range (count). `time_field` is
    /// implicitly `created_at`.
    TicketsCreated,
    /// Tickets resolved (closed_at in range, terminal workflow
    /// state). Count.
    TicketsResolved,
    /// Tickets currently open (snapshot at query time). Ignores
    /// the time range; delta vs prior period is `None`.
    TicketsOpen,
}

impl KpiMetric {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "tickets_created" => Some(Self::TicketsCreated),
            "tickets_resolved" => Some(Self::TicketsResolved),
            "tickets_open" => Some(Self::TicketsOpen),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct KpiQuery {
    pub metric: KpiMetric,
    /// Inclusive lower bound of the window. Ignored for snapshot
    /// metrics (`tickets_open`).
    pub from: DateTime<Utc>,
    /// Exclusive upper bound of the window. Ignored for snapshot
    /// metrics.
    pub to: DateTime<Utc>,
    /// Prior-period window for delta computation. When `None` the
    /// result's `delta_value` / `delta_pct` are `None` too.
    pub prior: Option<(DateTime<Utc>, DateTime<Utc>)>,
    /// Whether the response should include the per-day sparkline
    /// covering the primary window. Skip when the caller only
    /// needs the headline number (e.g. some compact tile renders).
    pub include_sparkline: bool,
    /// Validated IANA timezone the sparkline's daily buckets align
    /// to (the user's effective zone), so a day boundary lands on
    /// their local midnight rather than UTC's.
    pub tz: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct KpiResult {
    pub value: i64,
    /// `Some(prior - value? no, value - prior)` when a prior window
    /// was supplied; `None` for snapshot metrics or when no prior
    /// window was given.
    pub delta_value: Option<i64>,
    /// `(value - prior) / prior * 100`, rounded to one decimal.
    /// `None` when prior is `None` or zero (the percent is
    /// undefined when the baseline is zero — the frontend renders
    /// "new" instead of an infinity arrow).
    pub delta_pct: Option<f64>,
    /// Per-day counts spanning the primary window (length matches
    /// the bucket count). `None` when `include_sparkline = false`
    /// or the metric is a snapshot.
    pub sparkline: Option<Vec<i64>>,
}

pub fn kpi(conn: &mut DbConnection, q: KpiQuery) -> QueryResult<KpiResult> {
    let value = kpi_count(conn, q.metric, q.from, q.to)?;

    let (delta_value, delta_pct) = match (q.metric, q.prior) {
        // Snapshot metric never gets a delta — there's no period
        // it's "vs."; the headline number is the answer.
        (KpiMetric::TicketsOpen, _) | (_, None) => (None, None),
        (_, Some((pf, pt))) => {
            let prior = kpi_count(conn, q.metric, pf, pt)?;
            let delta = value - prior;
            let pct = if prior == 0 {
                None
            } else {
                Some(((delta as f64) / (prior as f64) * 1000.0).round() / 10.0)
            };
            (Some(delta), pct)
        }
    };

    let sparkline = if q.include_sparkline && q.metric != KpiMetric::TicketsOpen {
        // The KPI sparkline is a tiny trend line with no time axis, but
        // its daily buckets still align to the user's zone so a day's
        // dot covers their local midnight-to-midnight rather than UTC's.
        // Drop the bucket timestamps, keep the values.
        Some(
            bucketed_counts(conn, q.metric, q.from, q.to, Grain::Day, &q.tz)?
                .into_iter()
                .map(|(_, v)| v)
                .collect(),
        )
    } else {
        None
    };

    Ok(KpiResult {
        value,
        delta_value,
        delta_pct,
        sparkline,
    })
}

fn kpi_count(
    conn: &mut DbConnection,
    metric: KpiMetric,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
) -> QueryResult<i64> {
    match metric {
        KpiMetric::TicketsCreated => tickets::table
            .filter(tickets::created_at.ge(from))
            .filter(tickets::created_at.lt(to))
            .count()
            .get_result(conn),
        KpiMetric::TicketsResolved => tickets::table
            .filter(tickets::closed_at.is_not_null())
            .filter(tickets::closed_at.ge(from))
            .filter(tickets::closed_at.lt(to))
            .count()
            .get_result(conn),
        KpiMetric::TicketsOpen => tickets::table
            .filter(tickets::closed_at.is_null())
            .count()
            .get_result(conn),
    }
}

/// One bucket of the timeseries: a UTC instant (the local bucket
/// start, converted back from the user's zone) and its count.
#[derive(QueryableByName)]
struct BucketRow {
    #[diesel(sql_type = Timestamptz)]
    ts: DateTime<Utc>,
    #[diesel(sql_type = BigInt)]
    value: i64,
}

/// Counts per bucket for the metric, bucketed in the user's timezone.
/// Buckets are `date_trunc(<grain>, col AT TIME ZONE <tz>)` and the
/// whole window — including empty buckets — is filled by Postgres
/// `generate_series`, so the x-axis is continuous and DST-correct
/// (Postgres, not the application, owns the zone arithmetic). Returns
/// each bucket's UTC start instant plus its count, ascending.
///
/// `tz` must be a validated IANA name (see
/// `utils::locale::parse_timezone`) — it is bound, not interpolated.
/// The grain unit/interval and the metric column are code-controlled
/// literals, so the SQL is injection-safe.
fn bucketed_counts(
    conn: &mut DbConnection,
    metric: KpiMetric,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    grain: Grain,
    tz: &str,
) -> QueryResult<Vec<(DateTime<Utc>, i64)>> {
    use diesel::sql_types::Text;

    // Column + presence filter per metric (code-controlled literals).
    let (col, extra_filter) = match metric {
        KpiMetric::TicketsCreated => ("created_at", ""),
        KpiMetric::TicketsResolved => ("closed_at", "AND t.closed_at IS NOT NULL"),
        // Snapshot metric has no time breakdown; caller short-circuits.
        KpiMetric::TicketsOpen => return Ok(Vec::new()),
    };
    let unit = grain.trunc_unit();
    let step = grain.interval_literal();

    // generate_series walks local bucket starts (a zone-less timestamp)
    // from the first bucket through the bucket containing `to`; each is
    // mapped back to a UTC instant via `AT TIME ZONE`. The LEFT JOIN
    // against the grouped counts zero-fills empty buckets.
    let query = format!(
        "SELECT (gs AT TIME ZONE $1) AS ts, COALESCE(c.value, 0) AS value \
         FROM generate_series( \
             date_trunc('{unit}', ($2 AT TIME ZONE $1)), \
             ($3 AT TIME ZONE $1), \
             interval '{step}' \
         ) AS gs \
         LEFT JOIN ( \
             SELECT date_trunc('{unit}', (t.{col} AT TIME ZONE $1)) AS bucket, \
                    COUNT(*)::bigint AS value \
             FROM tickets t \
             WHERE t.{col} >= $2 AND t.{col} < $3 {extra_filter} \
             GROUP BY 1 \
         ) c ON c.bucket = gs \
         ORDER BY gs"
    );

    let rows: Vec<BucketRow> = diesel::sql_query(query)
        .bind::<Text, _>(tz)
        .bind::<Timestamptz, _>(from)
        .bind::<Timestamptz, _>(to)
        .load(conn)?;

    Ok(rows.into_iter().map(|r| (r.ts, r.value)).collect())
}

/// Bucket granularity for the time-series. Presets map to `Hour`
/// (the "today" view, so a sub-day window resolves into 24 hourly
/// points instead of collapsing to one daily dot) or `Day` (every
/// multi-day preset). The KPI sparkline always uses `Day`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grain {
    Hour,
    Day,
    Week,
    Month,
}

impl Grain {
    /// The `date_trunc` unit literal. This is a fixed, code-controlled
    /// string (never user input), so interpolating it into SQL is safe.
    fn trunc_unit(self) -> &'static str {
        match self {
            Grain::Hour => "hour",
            Grain::Day => "day",
            Grain::Week => "week",
            Grain::Month => "month",
        }
    }

    /// The `generate_series` step interval literal (code-controlled).
    fn interval_literal(self) -> &'static str {
        match self {
            Grain::Hour => "1 hour",
            Grain::Day => "1 day",
            Grain::Week => "1 week",
            Grain::Month => "1 month",
        }
    }

    /// Parse the wire value; unknown / absent grains fall back to
    /// `Day` so an unexpected param can't break the chart.
    pub fn parse(s: &str) -> Self {
        match s {
            "hour" => Grain::Hour,
            "week" => Grain::Week,
            "month" => Grain::Month,
            _ => Grain::Day,
        }
    }
}

/// Time-series measure enum. v1 supports a single measure (count);
/// the other measures listed in the plan (avg_response_time, p90_*,
/// sla_breach_rate, ...) land alongside the Wave 5 breakdowns work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TsMeasure {
    Count,
}

impl TsMeasure {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "count" => Some(Self::Count),
            _ => None,
        }
    }
}

/// Time field the count bucketises by. Mirrors the validator
/// allowlist in the dashboard plan §4.2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TsTimeField {
    CreatedAt,
    ClosedAt,
    ResolvedAt,
}

impl TsTimeField {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "created_at" => Some(Self::CreatedAt),
            "closed_at" => Some(Self::ClosedAt),
            // `resolved_at` is an alias for `closed_at` in the
            // current schema — there's no separate resolution
            // timestamp. The chart-config form uses the more
            // accurate name, so map it through.
            "resolved_at" => Some(Self::ResolvedAt),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct TimeseriesQuery {
    pub measure: TsMeasure,
    pub time_field: TsTimeField,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub grain: Grain,
    /// Validated IANA timezone the buckets are aligned to (the user's
    /// effective zone). "today" hourly buckets land on the user's local
    /// hours, daily buckets on their local days.
    pub tz: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeseriesBucket {
    pub ts: DateTime<Utc>,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TimeseriesResult {
    pub buckets: Vec<TimeseriesBucket>,
}

/// Group-by field for a categorical breakdown. Each variant is a
/// typed column on the tickets table; complex group-bys (joins, JSON
/// expansions) are out of scope for v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakdownGroupBy {
    Priority,
    Category,
    Assignee,
}

impl BreakdownGroupBy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "priority" => Some(Self::Priority),
            "category_id" | "category" => Some(Self::Category),
            "assignee_uuid" | "assignee" => Some(Self::Assignee),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct BreakdownQuery {
    pub group_by: BreakdownGroupBy,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// 1..=50 per the plan; the handler clamps before this is reached.
    pub top_n: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BreakdownBucket {
    /// Categorical key as a string (priority enum, category id, or
    /// assignee uuid). The frontend resolves human labels per kind.
    pub key: String,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct BreakdownResult {
    pub buckets: Vec<BreakdownBucket>,
}

pub fn breakdown(conn: &mut DbConnection, q: BreakdownQuery) -> QueryResult<BreakdownResult> {
    // Common filter: tickets created in the window. Other time
    // fields land alongside `time_field` config in a later wave;
    // for now breakdowns assume created_at as the bucketing field,
    // matching the most common analytics question ("of tickets
    // created in this window, what was their breakdown by X").
    let buckets: Vec<BreakdownBucket> = match q.group_by {
        BreakdownGroupBy::Priority => {
            let rows: Vec<(TicketPriority, i64)> = tickets::table
                .filter(tickets::created_at.ge(q.from))
                .filter(tickets::created_at.lt(q.to))
                .group_by(tickets::priority)
                .select((tickets::priority, sql::<BigInt>("COUNT(*)")))
                .order(sql::<BigInt>("COUNT(*)").desc())
                .limit(q.top_n)
                .load(conn)?;
            rows.into_iter()
                .map(|(p, v)| BreakdownBucket {
                    key: priority_key(p),
                    value: v,
                })
                .collect()
        }
        BreakdownGroupBy::Category => {
            let rows: Vec<(Option<i32>, i64)> = tickets::table
                .filter(tickets::created_at.ge(q.from))
                .filter(tickets::created_at.lt(q.to))
                .group_by(tickets::category_id)
                .select((tickets::category_id, sql::<BigInt>("COUNT(*)")))
                .order(sql::<BigInt>("COUNT(*)").desc())
                .limit(q.top_n)
                .load(conn)?;
            rows.into_iter()
                .map(|(id, v)| BreakdownBucket {
                    // Null category_id is "uncategorised"; surface
                    // it as the literal string so the frontend can
                    // render a localised label.
                    key: id.map(|n| n.to_string()).unwrap_or_else(|| "none".into()),
                    value: v,
                })
                .collect()
        }
        BreakdownGroupBy::Assignee => {
            let rows: Vec<(Option<Uuid>, i64)> = tickets::table
                .filter(tickets::created_at.ge(q.from))
                .filter(tickets::created_at.lt(q.to))
                .group_by(tickets::assignee_uuid)
                .select((tickets::assignee_uuid, sql::<BigInt>("COUNT(*)")))
                .order(sql::<BigInt>("COUNT(*)").desc())
                .limit(q.top_n)
                .load(conn)?;
            rows.into_iter()
                .map(|(id, v)| BreakdownBucket {
                    key: id
                        .map(|u| u.to_string())
                        .unwrap_or_else(|| "unassigned".into()),
                    value: v,
                })
                .collect()
        }
    };

    Ok(BreakdownResult { buckets })
}

fn priority_key(p: TicketPriority) -> String {
    match p {
        TicketPriority::Low => "low".into(),
        TicketPriority::Medium => "medium".into(),
        TicketPriority::High => "high".into(),
    }
}

/// Heatmap: tickets created bucketed by (weekday, hour). The result
/// is a flat 7x24 grid; missing cells aren't returned (the frontend
/// renders zero for absent (dow, hour) pairs).
#[derive(Debug)]
pub struct HeatmapQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeatmapCell {
    /// Day-of-week 0..=6 (Postgres EXTRACT(dow ...): 0 = Sunday).
    pub dow: i32,
    /// Hour-of-day 0..=23.
    pub hour: i32,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HeatmapResult {
    pub cells: Vec<HeatmapCell>,
}

pub fn heatmap(conn: &mut DbConnection, q: HeatmapQuery) -> QueryResult<HeatmapResult> {
    // Postgres EXTRACT returns numeric (decimal); cast to integer
    // so the Diesel typed loader maps cleanly to i32.
    let rows: Vec<(i32, i32, i64)> = tickets::table
        .filter(tickets::created_at.ge(q.from))
        .filter(tickets::created_at.lt(q.to))
        .group_by((
            sql::<Integer>("EXTRACT(dow FROM tickets.created_at)::int"),
            sql::<Integer>("EXTRACT(hour FROM tickets.created_at)::int"),
        ))
        .select((
            sql::<Integer>("EXTRACT(dow FROM tickets.created_at)::int"),
            sql::<Integer>("EXTRACT(hour FROM tickets.created_at)::int"),
            sql::<BigInt>("COUNT(*)"),
        ))
        .load(conn)?;
    Ok(HeatmapResult {
        cells: rows
            .into_iter()
            .map(|(dow, hour, value)| HeatmapCell { dow, hour, value })
            .collect(),
    })
}

/// Leaderboard: top-N tickets per actor (assignee or requester).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LeaderboardActor {
    Assignee,
    Requester,
}

impl LeaderboardActor {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "assignee" | "assignee_uuid" => Some(Self::Assignee),
            "requester" | "requester_uuid" => Some(Self::Requester),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct LeaderboardQuery {
    pub actor: LeaderboardActor,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub top_n: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaderboardRow {
    pub actor_uuid: Option<Uuid>,
    pub value: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LeaderboardResult {
    pub rows: Vec<LeaderboardRow>,
}

pub fn leaderboard(conn: &mut DbConnection, q: LeaderboardQuery) -> QueryResult<LeaderboardResult> {
    let rows: Vec<(Option<Uuid>, i64)> = match q.actor {
        LeaderboardActor::Assignee => tickets::table
            .filter(tickets::created_at.ge(q.from))
            .filter(tickets::created_at.lt(q.to))
            .filter(tickets::assignee_uuid.is_not_null())
            .group_by(tickets::assignee_uuid)
            .select((tickets::assignee_uuid, sql::<BigInt>("COUNT(*)")))
            .order(sql::<BigInt>("COUNT(*)").desc())
            .limit(q.top_n)
            .load(conn)?,
        LeaderboardActor::Requester => tickets::table
            .filter(tickets::created_at.ge(q.from))
            .filter(tickets::created_at.lt(q.to))
            .filter(tickets::requester_uuid.is_not_null())
            .group_by(tickets::requester_uuid)
            .select((tickets::requester_uuid, sql::<BigInt>("COUNT(*)")))
            .order(sql::<BigInt>("COUNT(*)").desc())
            .limit(q.top_n)
            .load(conn)?,
    };
    Ok(LeaderboardResult {
        rows: rows
            .into_iter()
            .map(|(actor_uuid, value)| LeaderboardRow { actor_uuid, value })
            .collect(),
    })
}

/// Audit-log marker for time-series chart overlays. The annotation
/// overlay surfaces "this is when the rule was last edited" /
/// "this is when the SLA policy changed" so a reader can correlate
/// a step in the chart with a config change.
#[derive(Debug, Clone, Serialize)]
pub struct AnnotationMarker {
    pub occurred_at: DateTime<Utc>,
    pub table_name: String,
    pub pk_text: String,
    pub actor_uuid: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnnotationResult {
    pub markers: Vec<AnnotationMarker>,
}

#[derive(Debug)]
pub struct AnnotationQuery {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Subset of audit_log.table_name values to include. Limited to
    /// the analytics-relevant config tables (rules, sla_policies,
    /// working_calendars) at the handler boundary.
    pub tables: Vec<String>,
}

pub fn audit_annotations(
    conn: &mut DbConnection,
    q: AnnotationQuery,
) -> QueryResult<AnnotationResult> {
    if q.tables.is_empty() {
        return Ok(AnnotationResult {
            markers: Vec::new(),
        });
    }
    let rows: Vec<(DateTime<Utc>, String, String, Option<Uuid>)> = audit_log::table
        .filter(audit_log::occurred_at.ge(q.from))
        .filter(audit_log::occurred_at.lt(q.to))
        .filter(audit_log::table_name.eq_any(&q.tables))
        .order(audit_log::occurred_at.asc())
        .select((
            audit_log::occurred_at,
            audit_log::table_name,
            audit_log::pk_text,
            audit_log::actor_uuid,
        ))
        // Cap so a chatty migration window can't return tens of
        // thousands of markers; chart overlay needs a handful, not
        // a forest. Newer markers win the cap by virtue of the ASC
        // ordering being stable over the cap point.
        .limit(500)
        .load(conn)?;
    Ok(AnnotationResult {
        markers: rows
            .into_iter()
            .map(
                |(occurred_at, table_name, pk_text, actor_uuid)| AnnotationMarker {
                    occurred_at,
                    table_name,
                    pk_text,
                    actor_uuid,
                },
            )
            .collect(),
    })
}

pub fn timeseries(conn: &mut DbConnection, q: TimeseriesQuery) -> QueryResult<TimeseriesResult> {
    // Map the time-field enum to the matching KPI metric so the
    // daily-counts helper produces the buckets without duplicating
    // the date-truncation SQL. The two metrics here cover the only
    // two time fields the v1 chart supports.
    let metric = match (q.measure, q.time_field) {
        (TsMeasure::Count, TsTimeField::CreatedAt) => KpiMetric::TicketsCreated,
        (TsMeasure::Count, TsTimeField::ClosedAt | TsTimeField::ResolvedAt) => {
            KpiMetric::TicketsResolved
        }
    };

    // bucketed_counts returns the full, zero-filled, tz-aligned series
    // (UTC instant + value per bucket) straight from Postgres.
    let buckets = bucketed_counts(conn, metric, q.from, q.to, q.grain, &q.tz)?
        .into_iter()
        .map(|(ts, value)| TimeseriesBucket { ts, value })
        .collect();

    Ok(TimeseriesResult { buckets })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kpi_metric_parse_round_trip() {
        assert_eq!(
            KpiMetric::parse("tickets_created"),
            Some(KpiMetric::TicketsCreated)
        );
        assert_eq!(
            KpiMetric::parse("tickets_resolved"),
            Some(KpiMetric::TicketsResolved)
        );
        assert_eq!(
            KpiMetric::parse("tickets_open"),
            Some(KpiMetric::TicketsOpen)
        );
        assert_eq!(KpiMetric::parse("bogus"), None);
    }

    #[test]
    fn grain_parse_defaults_to_day() {
        assert_eq!(Grain::parse("hour"), Grain::Hour);
        assert_eq!(Grain::parse("day"), Grain::Day);
        assert_eq!(Grain::parse("week"), Grain::Week);
        assert_eq!(Grain::parse("month"), Grain::Month);
        assert_eq!(Grain::parse(""), Grain::Day);
        assert_eq!(Grain::parse("nonsense"), Grain::Day);
    }

    #[test]
    fn breakdown_group_by_parse_round_trip() {
        assert_eq!(
            BreakdownGroupBy::parse("priority"),
            Some(BreakdownGroupBy::Priority)
        );
        assert_eq!(
            BreakdownGroupBy::parse("category_id"),
            Some(BreakdownGroupBy::Category)
        );
        assert_eq!(
            BreakdownGroupBy::parse("category"),
            Some(BreakdownGroupBy::Category)
        );
        assert_eq!(
            BreakdownGroupBy::parse("assignee_uuid"),
            Some(BreakdownGroupBy::Assignee)
        );
        assert_eq!(BreakdownGroupBy::parse("nope"), None);
    }

    #[test]
    fn leaderboard_actor_parse_aliases() {
        assert_eq!(
            LeaderboardActor::parse("assignee"),
            Some(LeaderboardActor::Assignee)
        );
        assert_eq!(
            LeaderboardActor::parse("assignee_uuid"),
            Some(LeaderboardActor::Assignee)
        );
        assert_eq!(
            LeaderboardActor::parse("requester_uuid"),
            Some(LeaderboardActor::Requester)
        );
        assert_eq!(LeaderboardActor::parse("nope"), None);
    }

    #[test]
    fn ts_time_field_parse_resolved_aliases_closed() {
        assert_eq!(
            TsTimeField::parse("resolved_at"),
            Some(TsTimeField::ResolvedAt)
        );
        assert_eq!(TsTimeField::parse("closed_at"), Some(TsTimeField::ClosedAt));
        assert_eq!(
            TsTimeField::parse("created_at"),
            Some(TsTimeField::CreatedAt)
        );
        assert_eq!(TsTimeField::parse("nope"), None);
    }
}
