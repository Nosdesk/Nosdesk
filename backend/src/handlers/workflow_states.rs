//! Workflow state endpoints.
//!
//! - `GET /api/workflow-states` — auth required. Lists active states.
//! - `POST /api/admin/workflow-states` — admin. Creates a new named
//!   state inside an existing category.
//! - `PATCH /api/admin/workflow-states/{id}` — admin. Rename, recolor,
//!   reorder, set as workspace default.
//! - `DELETE /api/admin/workflow-states/{id}` — admin. Soft-archive the
//!   state. Existing tickets keep referencing it; reactivation clears
//!   the archived_at timestamp.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::Connection;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::{errors, helpers};
use crate::models::{Claims, NewWorkflowState, WorkflowState, WorkflowStateCategory, WorkflowStateUpdate};
use crate::repository::workflow_states as repo;
use crate::schema::workflow_states;

#[derive(Debug, Serialize)]
pub struct WorkflowStatesResponse {
    pub states: Vec<WorkflowState>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub name: String,
    pub category: WorkflowStateCategory,
    pub color: String,
}

#[derive(Debug, Deserialize)]
pub struct PatchBody {
    pub name: Option<String>,
    pub color: Option<String>,
    pub position: Option<i32>,
    /// When `Some(true)`, atomically clears the previous default and
    /// sets this state as the workspace default. `Some(false)` is
    /// rejected — there must always be exactly one default.
    pub is_default: Option<bool>,
}

fn actor_uuid(req: &HttpRequest) -> Option<Uuid> {
    req.extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok())
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

/// POST /api/admin/workflow-states
pub async fn create(
    pool: web::Data<Pool>,
    body: web::Json<CreateBody>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let trimmed = body.name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return errors::bad_request("Name must be 1 to 64 characters");
    }
    if body.color.trim().is_empty() {
        return errors::bad_request("Color is required");
    }

    let actor = actor_uuid(&req);

    // Pick the next position inside the category so the new state
    // lands at the bottom of its column.
    let next_position: i32 = match repo::list_all(&mut conn) {
        Ok(rows) => rows
            .iter()
            .filter(|s| s.category == body.category && s.archived_at.is_none())
            .map(|s| s.position)
            .max()
            .map(|p| p + 1)
            .unwrap_or(0),
        Err(e) => {
            error!(error = %e, "failed to list workflow states for position calc");
            return errors::internal("Failed to create workflow state");
        }
    };

    let new = NewWorkflowState {
        name: trimmed.to_string(),
        category: body.category,
        color: body.color.clone(),
        position: next_position,
        is_default: false,
        created_by: actor,
    };

    match repo::create(&mut conn, new) {
        Ok(state) => {
            info!(
                actor = ?actor,
                state_id = state.id,
                name = %state.name,
                category = %state.category.as_str(),
                "workflow state created"
            );
            HttpResponse::Created().json(state)
        }
        Err(e) => {
            error!(error = %e, "failed to create workflow state");
            errors::internal("Failed to create workflow state")
        }
    }
}

/// PATCH /api/admin/workflow-states/{id}
pub async fn patch(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<PatchBody>,
    req: HttpRequest,
) -> impl Responder {
    let id = path.into_inner();
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    if let Some(ref n) = body.name {
        let t = n.trim();
        if t.is_empty() || t.len() > 64 {
            return errors::bad_request("Name must be 1 to 64 characters");
        }
    }
    if matches!(body.is_default, Some(false)) {
        return errors::bad_request(
            "Setting is_default to false directly is not allowed; promote a different state instead",
        );
    }

    let actor = actor_uuid(&req);

    // Default-promotion writes two rows (clear old default, set new),
    // so wrap the patch in a transaction.
    let result = conn.transaction::<WorkflowState, diesel::result::Error, _>(|conn| {
        if matches!(body.is_default, Some(true)) {
            diesel::update(workflow_states::table.filter(workflow_states::is_default.eq(true)))
                .set(workflow_states::is_default.eq(false))
                .execute(conn)?;
        }
        let patch = WorkflowStateUpdate {
            name: body.name.as_ref().map(|s| s.trim().to_string()),
            color: body.color.clone(),
            position: body.position,
            is_default: body.is_default,
            archived_at: None,
        };
        diesel::update(workflow_states::table.find(id))
            .set(&patch)
            .get_result::<WorkflowState>(conn)
    });

    match result {
        Ok(state) => {
            repo::invalidate_cache();
            info!(
                actor = ?actor,
                state_id = state.id,
                "workflow state updated"
            );
            HttpResponse::Ok().json(state)
        }
        Err(diesel::result::Error::NotFound) => errors::not_found_msg("Workflow state not found"),
        Err(e) => {
            error!(error = %e, state_id = id, "failed to update workflow state");
            errors::internal("Failed to update workflow state")
        }
    }
}

/// DELETE /api/admin/workflow-states/{id}
pub async fn archive(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    req: HttpRequest,
) -> impl Responder {
    let id = path.into_inner();
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Refuse to archive the workspace default — a default must always
    // exist for new tickets to land somewhere sensible.
    match repo::find_by_id(&mut conn, id) {
        Ok(Some(s)) if s.is_default => {
            return errors::bad_request("Cannot archive the default state; promote a different state first");
        }
        Ok(Some(_)) => {}
        Ok(None) => return errors::not_found_msg("Workflow state not found"),
        Err(e) => {
            error!(error = %e, state_id = id, "failed to look up workflow state for archive");
            return errors::internal("Failed to archive workflow state");
        }
    }

    let actor = actor_uuid(&req);
    match repo::archive(&mut conn, id) {
        Ok(state) => {
            info!(actor = ?actor, state_id = state.id, "workflow state archived");
            HttpResponse::Ok().json(state)
        }
        Err(e) => {
            error!(error = %e, state_id = id, "failed to archive workflow state");
            errors::internal("Failed to archive workflow state")
        }
    }
}
