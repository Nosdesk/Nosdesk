//! Admin endpoint exposing the periodic-job status registry.
//!
//! Read-only view on what the scheduler has done lately — last run,
//! duration, outcome, failure count per job. Useful for sanity
//! checking "is the MS Graph sync actually running" without digging
//! through container logs.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Serialize;

use crate::db::Pool;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::services::scheduler::{PeriodicStatus, StatusRegistry};

/// One row of the response. Adds `name` so the client can render a
/// list without turning the map into an array at the call site.
#[derive(Debug, Serialize)]
struct JobRow<'a> {
    name: &'a str,
    #[serde(flatten)]
    status: PeriodicStatus,
}

/// GET /api/admin/scheduler/status
pub async fn get_status(
    pool: web::Data<Pool>,
    statuses: web::Data<StatusRegistry>,
    req: HttpRequest,
) -> HttpResponse {
    // Same admin-guard helper the channels endpoints use. We grab a
    // connection purely for the guard — scheduler status is in-memory
    // so no DB work happens downstream.
    if let Err(resp) = helpers::admin_conn(&req, &pool) {
        return resp;
    }

    let Ok(map) = statuses.read() else {
        return errors::internal("scheduler status lock poisoned");
    };

    // Stable alphabetical order so the UI doesn't reshuffle rows on
    // every refresh (HashMap iteration is arbitrary).
    let mut rows: Vec<JobRow> = map
        .iter()
        .map(|(name, status)| JobRow {
            name,
            status: status.clone(),
        })
        .collect();
    rows.sort_by_key(|r| r.name);

    HttpResponse::Ok().json(rows)
}
