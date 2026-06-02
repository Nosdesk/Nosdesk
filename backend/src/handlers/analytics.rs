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

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::repository::analytics::{
    self, KpiMetric, KpiQuery, TimeseriesQuery, TsMeasure, TsTimeField,
};

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

    let query = KpiQuery {
        metric,
        from: params.from,
        to: params.to,
        prior,
        include_sparkline: params.sparkline,
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
pub struct TimeseriesParams {
    /// Measure identifier. v1: `count` only.
    pub measure: String,
    /// Time field to bucketise by: `created_at`, `closed_at`, or
    /// `resolved_at` (alias for closed_at).
    pub time_field: String,
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
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

    let q = TimeseriesQuery {
        measure,
        time_field,
        from: params.from,
        to: params.to,
    };

    match tc.run(|conn| analytics::timeseries(conn, q)) {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => {
            error!(error = %e, "timeseries query failed");
            errors::internal("timeseries unavailable")
        }
    }
}
