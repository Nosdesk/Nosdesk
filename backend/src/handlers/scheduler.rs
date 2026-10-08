//! Admin endpoint exposing the periodic-job status registry.
//!
//! Read-only view on what the scheduler has done lately — last run,
//! duration, outcome, failure count per job. Useful for sanity
//! checking "is the MS Graph sync actually running" without digging
//! through container logs.

use actix_web::{web, HttpRequest, HttpResponse};
use serde::Serialize;

use crate::db::Pool;
use crate::errors::ApiError;
use crate::handlers::helpers;
use crate::services::scheduler::{PeriodicStatus, StatusRegistry};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/scheduler/status",
        web::get().to(crate::handlers::scheduler::get_status),
    );
}

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
) -> Result<HttpResponse, ApiError> {
    // The jobs are the instance's. On hosted that's every workspace on it,
    // so only a platform admin reads them; self-hosted, the instance is the
    // workspace admin's own. The status is in memory: the connection is only
    // for the workspace-admin check.
    match crate::middleware::DeploymentMode::current() {
        crate::middleware::DeploymentMode::Hosted => {
            crate::utils::rbac::require_platform_admin(&req)?;
        }
        crate::middleware::DeploymentMode::SelfHosted => {
            helpers::admin_conn(&req, &pool)?;
        }
    }

    let Ok(map) = statuses.read() else {
        return Err(ApiError::Internal("scheduler status lock poisoned".into()));
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

    Ok(HttpResponse::Ok().json(rows))
}
