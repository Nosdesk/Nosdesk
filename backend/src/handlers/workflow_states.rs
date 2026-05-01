//! Workflow state read endpoint.
//!
//! - `GET /api/workflow-states` — list every non-archived state. Used by
//!   the frontend to render named states + colors + categories. The
//!   admin write endpoints (create, rename, reorder, archive) ship with
//!   the workflow customisation UI in a later commit.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Serialize;
use tracing::error;

use crate::db::Pool;
use crate::handlers::{errors, helpers};
use crate::models::WorkflowState;
use crate::repository::workflow_states as repo;

#[derive(Debug, Serialize)]
pub struct WorkflowStatesResponse {
    pub states: Vec<WorkflowState>,
}

/// GET /api/workflow-states
pub async fn list(pool: web::Data<Pool>, req: HttpRequest) -> impl Responder {
    let (_claims, _user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    match repo::list_all(&mut conn) {
        Ok(states) => {
            let active = states.into_iter().filter(|s| s.archived_at.is_none()).collect();
            HttpResponse::Ok().json(WorkflowStatesResponse { states: active })
        }
        Err(e) => {
            error!(error = %e, "failed to list workflow states");
            errors::internal("Failed to list workflow states")
        }
    }
}
