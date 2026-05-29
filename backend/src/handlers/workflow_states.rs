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
use serde::{Deserialize, Serialize};
use tracing::{error, info};
use uuid::Uuid;

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::{
    Claims, NewWorkflowState, WorkflowState, WorkflowStateCategory, WorkflowStateUpdate,
};
use crate::repository::workflow_states as repo;
use crate::utils::rbac::require_admin;

#[derive(Debug, Serialize)]
pub struct WorkflowStatesResponse {
    pub states: Vec<WorkflowState>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub name: String,
    pub category: WorkflowStateCategory,
    pub color: String,
    /// Optional pause-SLA override. When `None`, the default is
    /// derived from the category (active runs the clock, every other
    /// category pauses it). Setting it explicitly lets an admin keep
    /// a "Waiting on customer" status modelled under active while
    /// still pausing the timer.
    pub pauses_sla: Option<bool>,
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
    pub pauses_sla: Option<bool>,
}

/// Pull the JWT subject UUID off the request, used for the
/// info!(actor=...) logging breadcrumbs alongside the structured
/// `ActorContext` TenantConn injects.
fn actor_uuid(req: &HttpRequest) -> Option<Uuid> {
    req.extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok())
}

/// GET /api/workflow-states
pub async fn list(mut tc: TenantConn, _req: HttpRequest) -> impl Responder {
    match tc.run(|conn| repo::list_all(conn)) {
        Ok(states) => {
            let active = states
                .into_iter()
                .filter(|s| s.archived_at.is_none())
                .collect();
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
    mut tc: TenantConn,
    body: web::Json<CreateBody>,
    req: HttpRequest,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

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
    let next_position: i32 = match tc.run(|conn| repo::list_all(conn)) {
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

    // Default to the legacy category-derived rule (active = clock
    // running, everything else = paused) when the caller didn't pick
    // a side. Keeps existing seed/onboarding flows behaving as before
    // while letting an admin override per state.
    let pauses_sla = body
        .pauses_sla
        .unwrap_or(body.category != WorkflowStateCategory::Active);

    let new = NewWorkflowState {
        name: trimmed.to_string(),
        category: body.category,
        color: body.color.clone(),
        position: next_position,
        is_default: false,
        created_by: actor,
        pauses_sla,
    };

    let created = tc.run(|conn| repo::create(conn, new));
    match created {
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
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<PatchBody>,
    req: HttpRequest,
) -> impl Responder {
    let id = path.into_inner();
    if let Err(e) = require_admin(&req) {
        return e;
    }

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

    let patch = WorkflowStateUpdate {
        name: body.name.as_ref().map(|s| s.trim().to_string()),
        color: body.color.clone(),
        position: body.position,
        // Force this off — the promote path sets it itself, and the
        // regular update path must not flip default state directly
        // (the caller would skip the demote-old-default emit).
        is_default: None,
        archived_at: None,
        pauses_sla: body.pauses_sla,
    };
    let promote = matches!(body.is_default, Some(true));
    let result = tc.run(|conn| {
        if promote {
            repo::promote_default(conn, id, patch)
        } else {
            repo::update(conn, id, patch)
        }
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
pub async fn archive(mut tc: TenantConn, path: web::Path<i32>, req: HttpRequest) -> impl Responder {
    let id = path.into_inner();
    if let Err(e) = require_admin(&req) {
        return e;
    }

    // Refuse to archive the workspace default — a default must always
    // exist for new tickets to land somewhere sensible.
    match tc.run(|conn| repo::find_by_id(conn, id)) {
        Ok(Some(s)) if s.is_default => {
            return errors::bad_request(
                "Cannot archive the default state; promote a different state first",
            );
        }
        Ok(Some(_)) => {}
        Ok(None) => return errors::not_found_msg("Workflow state not found"),
        Err(e) => {
            error!(error = %e, state_id = id, "failed to look up workflow state for archive");
            return errors::internal("Failed to archive workflow state");
        }
    }

    let actor = actor_uuid(&req);
    let archived = tc.run(|conn| repo::archive(conn, id));
    match archived {
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
