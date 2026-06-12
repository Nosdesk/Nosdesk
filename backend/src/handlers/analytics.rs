//! Dashboard chart endpoints.
//!
//! `GET /api/dashboard/kpi` and `GET /api/dashboard/timeseries`.
//!
//! Both are workspace-scoped via `TenantConn`; the RLS policy on
//! `tickets` filters every aggregated row to the active workspace
//! before any aggregation happens, so the handler can stay focused
//! on parameter parsing and shape mapping.

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::repository::analytics::{
    self, AnnotationQuery, BreakdownGroupBy, BreakdownQuery, HeatmapQuery, KpiMetric, KpiQuery,
    KpiSummaryQuery, LeaderboardActor, LeaderboardQuery, TimeseriesQuery, TsMeasure, TsTimeField,
};

/// Per-plan cap on top_n; chosen to keep the chart legible. The
/// handler clamps incoming values so a too-large request degrades
/// to the cap rather than a 400.
const TOP_N_MAX: i64 = 50;

/// Allowlist for the audit-annotation `kinds` parameter. Each entry
/// is the `audit_log.table_name` literal the trigger writes. Keep
/// in sync with the analytics overlay docs (§13.2).
const ANNOTATION_TABLES: &[&str] = &["rules", "sla_policies", "working_calendars"];

/// Reusable guard for analytics endpoints that expose staffing,
/// admin-edit timelines, or other data an end-customer shouldn't
/// see. Returns `None` on success (caller continues), `Some(response)`
/// on denial (caller returns immediately).
///
/// Centralising the check here keeps every sensitive endpoint
/// reading from one place — adding a new staff-only chart kind is
/// `staff_gate(&auth)?` at the top of the handler, no policy
/// duplication.
fn staff_gate(auth: &AuthContext) -> Option<HttpResponse> {
    if auth.can_handle_tickets() {
        None
    } else {
        Some(errors::forbidden(
            "Staffing and admin-activity analytics are restricted to technicians and admins",
        ))
    }
}

#[derive(Debug, Deserialize)]
pub struct KpiParams {
    /// Metric identifier. See `analytics::KpiMetric::parse` for the
    /// allowlist.
    pub metric: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Optional prior-period window. Both must be present together;
    /// providing only one is a 400.
    pub prior_from: Option<DateTime<Utc>>,
    pub prior_to: Option<DateTime<Utc>>,
    /// Default `true` — KPI tiles render a tiny sparkline alongside
    /// the headline number. Callers that only need the value can
    /// suppress it with `?sparkline=false`.
    #[serde(default = "default_true")]
    pub sparkline: bool,
    /// IANA timezone the sparkline's daily buckets align to (the
    /// client's effective zone). Absent / invalid falls back to UTC.
    /// Keeps each day's bucket on the user's local day boundary.
    #[serde(default)]
    pub tz: Option<String>,
}

fn default_true() -> bool {
    true
}

pub async fn get_kpi(mut tc: TenantConn, query: web::Query<KpiParams>) -> impl Responder {
    let params = query.into_inner();

    let Some(metric) = KpiMetric::parse(&params.metric) else {
        return errors::bad_request(
            "metric must be one of: tickets_created, tickets_resolved, tickets_open",
        );
    };

    if params.from >= params.to {
        return errors::bad_request("`from` must be earlier than `to`");
    }

    let prior = match (params.prior_from, params.prior_to) {
        (Some(pf), Some(pt)) => {
            if pf >= pt {
                return errors::bad_request("`prior_from` must be earlier than `prior_to`");
            }
            Some((pf, pt))
        }
        (None, None) => None,
        // Half-set prior window is a programming error in the
        // caller; refuse rather than silently dropping one half.
        _ => {
            return errors::bad_request(
                "prior_from and prior_to must be supplied together or omitted together",
            )
        }
    };

    // Validate the timezone against the IANA database; an unknown name
    // (or none) falls back to UTC so a bad param can't 500 the tile or
    // be smuggled into the SQL.
    let tz = params
        .tz
        .as_deref()
        .and_then(|s| crate::utils::locale::parse_timezone(s).ok())
        .map(|z| z.name().to_string())
        .unwrap_or_else(|| "UTC".to_string());

    let query = KpiQuery {
        metric,
        from: params.from,
        to: params.to,
        prior,
        include_sparkline: params.sparkline,
        tz,
    };

    match tc.run(|conn| analytics::kpi(conn, query)) {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            error!(error = %e, "kpi query failed");
            errors::internal("kpi unavailable")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct KpiSummaryParams {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Optional prior-period window for deltas. Both present together
    /// or both omitted; a half-set window is a 400.
    pub prior_from: Option<DateTime<Utc>>,
    pub prior_to: Option<DateTime<Utc>>,
    /// Default `true`. Suppress with `?sparkline=false` on dense
    /// renders that don't show the trend line.
    #[serde(default = "default_true")]
    pub sparkline: bool,
    /// IANA timezone the sparkline buckets align to. Absent / invalid
    /// falls back to UTC.
    #[serde(default)]
    pub tz: Option<String>,
}

/// `GET /api/dashboard/kpi-summary` — created, resolved, and open in
/// one response. Collapses the KPI rail's three parallel `/kpi` calls
/// (three pooled connections, three scans of `tickets`) into a single
/// request whose scalar counts come from one conditional-aggregation
/// pass. Same per-metric `KpiResult` shape, so the frontend types are
/// unchanged.
pub async fn get_kpi_summary(
    mut tc: TenantConn,
    query: web::Query<KpiSummaryParams>,
) -> impl Responder {
    let params = query.into_inner();

    if params.from >= params.to {
        return errors::bad_request("`from` must be earlier than `to`");
    }

    let prior = match (params.prior_from, params.prior_to) {
        (Some(pf), Some(pt)) => {
            if pf >= pt {
                return errors::bad_request("`prior_from` must be earlier than `prior_to`");
            }
            Some((pf, pt))
        }
        (None, None) => None,
        _ => {
            return errors::bad_request(
                "prior_from and prior_to must be supplied together or omitted together",
            )
        }
    };

    let tz = params
        .tz
        .as_deref()
        .and_then(|s| crate::utils::locale::parse_timezone(s).ok())
        .map(|z| z.name().to_string())
        .unwrap_or_else(|| "UTC".to_string());

    let q = KpiSummaryQuery {
        from: params.from,
        to: params.to,
        prior,
        include_sparkline: params.sparkline,
        tz,
    };

    match tc.run(|conn| analytics::kpi_summary(conn, q)) {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            error!(error = %e, "kpi summary query failed");
            errors::internal("kpi summary unavailable")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct TimeseriesParams {
    /// Measure identifier. v1: `count` only.
    pub measure: String,
    /// Time field to bucketise by: `created_at`, `closed_at`, or
    /// `resolved_at` (alias for closed_at).
    pub time_field: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Bucket granularity: `hour` or `day`. Absent / unknown falls
    /// back to `day`. The "today" preset sends `hour` so a sub-day
    /// window renders 24 hourly points rather than one daily dot.
    #[serde(default)]
    pub grain: Option<String>,
    /// IANA timezone the buckets align to (the client's effective
    /// zone). Absent / invalid falls back to UTC. Keeps the bucket
    /// boundaries on the user's local hours / days.
    #[serde(default)]
    pub tz: Option<String>,
}

pub async fn get_timeseries(
    mut tc: TenantConn,
    query: web::Query<TimeseriesParams>,
) -> impl Responder {
    let params = query.into_inner();

    let Some(measure) = TsMeasure::parse(&params.measure) else {
        return errors::bad_request("measure must be one of: count");
    };
    let Some(time_field) = TsTimeField::parse(&params.time_field) else {
        return errors::bad_request(
            "time_field must be one of: created_at, closed_at, resolved_at",
        );
    };
    if params.from >= params.to {
        return errors::bad_request("`from` must be earlier than `to`");
    }

    // Validate the timezone against the IANA database; an unknown name
    // (or none) falls back to UTC so a bad param can't 500 the chart or
    // be smuggled into the SQL.
    let tz = params
        .tz
        .as_deref()
        .and_then(|s| crate::utils::locale::parse_timezone(s).ok())
        .map(|z| z.name().to_string())
        .unwrap_or_else(|| "UTC".to_string());

    let q = TimeseriesQuery {
        measure,
        time_field,
        from: params.from,
        to: params.to,
        grain: analytics::Grain::parse(params.grain.as_deref().unwrap_or("day")),
        tz,
    };

    match tc.run(|conn| analytics::timeseries(conn, q)) {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            error!(error = %e, "timeseries query failed");
            errors::internal("timeseries unavailable")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct BreakdownParams {
    pub group_by: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    #[serde(default)]
    pub top_n: Option<i64>,
}

pub async fn get_breakdown(
    mut tc: TenantConn,
    query: web::Query<BreakdownParams>,
    auth: AuthContext,
) -> impl Responder {
    let params = query.into_inner();
    let Some(group_by) = BreakdownGroupBy::parse(&params.group_by) else {
        return errors::bad_request(
            "group_by must be one of: priority, category_id, assignee_uuid",
        );
    };
    // Group-by priority / category is workspace aggregate (no
    // actor info), open to any authenticated user. Group-by
    // assignee enumerates technicians by ticket volume — staff-
    // only territory.
    if matches!(group_by, BreakdownGroupBy::Assignee) {
        if let Some(deny) = staff_gate(&auth) {
            return deny;
        }
    }
    if params.from >= params.to {
        return errors::bad_request("`from` must be earlier than `to`");
    }
    let top_n = params.top_n.unwrap_or(10).clamp(1, TOP_N_MAX);
    let q = BreakdownQuery {
        group_by,
        from: params.from,
        to: params.to,
        top_n,
    };
    match tc.run(|conn| analytics::breakdown(conn, q)) {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            error!(error = %e, "breakdown query failed");
            errors::internal("breakdown unavailable")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct HeatmapParams {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
}

pub async fn get_heatmap(mut tc: TenantConn, query: web::Query<HeatmapParams>) -> impl Responder {
    let params = query.into_inner();
    if params.from >= params.to {
        return errors::bad_request("`from` must be earlier than `to`");
    }
    let q = HeatmapQuery {
        from: params.from,
        to: params.to,
    };
    match tc.run(|conn| analytics::heatmap(conn, q)) {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            error!(error = %e, "heatmap query failed");
            errors::internal("heatmap unavailable")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct LeaderboardParams {
    pub actor: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    #[serde(default)]
    pub top_n: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct AnnotationParams {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    /// Comma-separated kinds from the ANNOTATION_TABLES allowlist.
    /// Omitting / empty includes every allowed kind.
    #[serde(default)]
    pub kinds: Option<String>,
}

pub async fn get_audit_annotations(
    mut tc: TenantConn,
    query: web::Query<AnnotationParams>,
    auth: AuthContext,
) -> impl Responder {
    // The annotation overlay surfaces admin config edits (rule /
    // SLA / working-calendar history). Staff-only by definition.
    if let Some(deny) = staff_gate(&auth) {
        return deny;
    }
    let params = query.into_inner();
    if params.from >= params.to {
        return errors::bad_request("`from` must be earlier than `to`");
    }
    let tables: Vec<String> = match params.kinds.as_deref() {
        None | Some("") => ANNOTATION_TABLES.iter().map(|s| (*s).to_string()).collect(),
        Some(raw) => {
            let mut out = Vec::new();
            for token in raw.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                if !ANNOTATION_TABLES.contains(&token) {
                    return errors::bad_request(
                        "kinds must be a subset of: rules, sla_policies, working_calendars",
                    );
                }
                out.push(token.to_string());
            }
            out
        }
    };
    let q = AnnotationQuery {
        from: params.from,
        to: params.to,
        tables,
    };
    match tc.run(|conn| analytics::audit_annotations(conn, q)) {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            error!(error = %e, "audit annotations query failed");
            errors::internal("annotations unavailable")
        }
    }
}

pub async fn get_leaderboard(
    mut tc: TenantConn,
    query: web::Query<LeaderboardParams>,
    auth: AuthContext,
) -> impl Responder {
    // Leaderboard always ranks actors (assignee or requester) by
    // ticket volume, which is staffing/customer-list data an
    // end-user shouldn't see.
    if let Some(deny) = staff_gate(&auth) {
        return deny;
    }
    let params = query.into_inner();
    let Some(actor) = LeaderboardActor::parse(&params.actor) else {
        return errors::bad_request("actor must be one of: assignee, requester");
    };
    if params.from >= params.to {
        return errors::bad_request("`from` must be earlier than `to`");
    }
    let top_n = params.top_n.unwrap_or(10).clamp(1, TOP_N_MAX);
    let q = LeaderboardQuery {
        actor,
        from: params.from,
        to: params.to,
        top_n,
    };
    match tc.run(|conn| analytics::leaderboard(conn, q)) {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            error!(error = %e, "leaderboard query failed");
            errors::internal("leaderboard unavailable")
        }
    }
}
