//! Cycle endpoints.
//!
//! - `GET    /api/projects/{project_id}/cycles` — list cycles for
//!   a project (omits archived).
//! - `POST   /api/projects/{project_id}/cycles` — create a cycle.
//! - `GET    /api/cycles/{uuid}` — fetch one.
//! - `PATCH  /api/cycles/{uuid}` — rename / reschedule / promote.
//! - `POST   /api/cycles/{uuid}/complete` — freeze snapshot, mark
//!   completed.
//! - `DELETE /api/cycles/{uuid}` — soft archive.
//! - `POST   /api/cycles/{uuid}/tickets/{ticket_id}` — add ticket.
//! - `DELETE /api/cycles/{uuid}/tickets/{ticket_id}` — remove
//!   ticket from any cycle (the partial unique index makes the
//!   cycle id redundant on delete, but keeping it in the URL keeps
//!   the symmetry).
//!
//! Permissions: writes require technician/admin (project members
//! get write access through the same gate the project router uses);
//! reads open to any authenticated user with project visibility.

use actix_web::{web, HttpResponse, Responder};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::Pool;
use crate::extractors::AuthContext;
use crate::handlers::{errors, helpers};
use crate::models::{CycleUpdate, NewCycle};
use crate::repository::cycles as repo;

const NAME_MIN: usize = 1;
const NAME_MAX: usize = 120;

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub name: String,
    pub start_at: Option<DateTime<Utc>>,
    pub end_at: Option<DateTime<Utc>>,
    /// Optional. Defaults to "planned"; supplying "active" skips
    /// the planned phase but still hits the partial unique index
    /// if another active cycle exists in the same project.
    pub state: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchBody {
    pub name: Option<String>,
    pub start_at: Option<Option<DateTime<Utc>>>,
    pub end_at: Option<Option<DateTime<Utc>>>,
    /// Promote a planned cycle to active (or move it back). Cycle
    /// completion is a separate endpoint because it freezes a
    /// snapshot and can't be undone via this generic patch.
    pub state: Option<String>,
}

pub async fn list(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    _auth: AuthContext,
) -> impl Responder {
    let project_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match repo::list_for_project(&mut conn, project_id) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, project_id, "failed to list cycles");
            errors::internal("Failed to list cycles")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceListQuery {
    /// Comma-separated state filter (e.g. `state=active,planned`).
    /// Omitted means "all non-archived states." `completed` is
    /// excluded by default since the workspace overview is for
    /// in-flight work; an explicit query opts back in.
    pub state: Option<String>,
}

/// Workspace-wide cycles list. Unlike the per-project endpoint
/// this surfaces every project's cycles in one response so the
/// /cycles overview can render an "active across the workspace"
/// view without N round-trips.
pub async fn list_workspace(
    pool: web::Data<Pool>,
    query: web::Query<WorkspaceListQuery>,
    _auth: AuthContext,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let states_owned: Option<Vec<String>> = query
        .state
        .as_deref()
        .map(|s| s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
    let states_ref: Option<Vec<&str>> = states_owned.as_ref().map(|v| v.iter().map(|s| s.as_str()).collect());
    let states_slice: Option<&[&str]> = states_ref.as_deref();
    let filter = if states_slice.is_some() {
        states_slice
    } else {
        Some(&["planned", "active"][..])
    };
    match repo::list_for_workspace(&mut conn, filter) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "failed to list workspace cycles");
            errors::internal("Failed to list cycles")
        }
    }
}

pub async fn get_one(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match repo::find_by_uuid(&mut conn, uuid) {
        Ok(Some(cycle)) => HttpResponse::Ok().json(cycle),
        Ok(None) => errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %uuid, "failed to fetch cycle");
            errors::internal("Failed to fetch cycle")
        }
    }
}

pub async fn create(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<CreateBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Only technicians and admins can create cycles");
    }
    let project_id = path.into_inner();
    let body = body.into_inner();
    if let Err(msg) = validate_name(&body.name) {
        return errors::bad_request(msg);
    }
    let state = body.state.unwrap_or_else(|| "planned".to_string());
    if !is_valid_state(&state) {
        return errors::bad_request("state must be one of: planned, active");
    }
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let new = NewCycle {
        project_id,
        name: body.name.trim().to_string(),
        start_at: body.start_at,
        end_at: body.end_at,
        state,
        created_by: Some(auth.user_uuid),
    };
    match repo::create(&mut conn, new) {
        Ok(cycle) => {
            info!(uuid = %cycle.uuid, project_id, "cycle created");
            HttpResponse::Created().json(cycle)
        }
        Err(e) => {
            error!(error = %e, project_id, "failed to create cycle");
            errors::internal("Failed to create cycle")
        }
    }
}

pub async fn patch(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: web::Json<PatchBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Only technicians and admins can edit cycles");
    }
    let uuid = path.into_inner();
    let body = body.into_inner();
    if let Some(ref n) = body.name {
        if let Err(msg) = validate_name(n) {
            return errors::bad_request(msg);
        }
    }
    if let Some(ref s) = body.state {
        if !is_valid_state(s) {
            return errors::bad_request(
                "state must be one of: planned, active. Use POST /complete to mark completed.",
            );
        }
    }
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let patch = CycleUpdate {
        name: body.name.map(|s| s.trim().to_string()),
        start_at: body.start_at,
        end_at: body.end_at,
        state: body.state,
        completion_snapshot: None,
        completed_at: None,
        archived_at: None,
    };
    match repo::update(&mut conn, uuid, patch) {
        Ok(cycle) => HttpResponse::Ok().json(cycle),
        Err(e) => {
            error!(error = %e, %uuid, "failed to update cycle");
            errors::internal("Failed to update cycle")
        }
    }
}

/// Live stats for a cycle. For completed cycles returns the frozen
/// completion_snapshot; for planned/active cycles computes the same
/// shape on the fly. The frontend Burndown widget renders both
/// through the same code path.
pub async fn stats(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let cycle = match repo::find_by_uuid(&mut conn, uuid) {
        Ok(Some(c)) => c,
        Ok(None) => return errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %uuid, "stats: cycle lookup failed");
            return errors::internal("Failed to fetch cycle stats");
        }
    };
    if let Some(snap) = cycle.completion_snapshot.clone() {
        return HttpResponse::Ok().json(snap);
    }
    match repo::build_completion_snapshot(&mut conn, cycle.id) {
        Ok(s) => HttpResponse::Ok().json(s),
        Err(e) => {
            error!(error = %e, %uuid, "stats: snapshot build failed");
            errors::internal("Failed to build cycle stats")
        }
    }
}

pub async fn complete(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Only technicians and admins can complete cycles");
    }
    let uuid = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let cycle = match repo::find_by_uuid(&mut conn, uuid) {
        Ok(Some(c)) => c,
        Ok(None) => return errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %uuid, "failed to fetch cycle for complete");
            return errors::internal("Failed to complete cycle");
        }
    };
    if cycle.state == "completed" {
        return errors::bad_request("Cycle is already completed");
    }
    let snapshot = match repo::build_completion_snapshot(&mut conn, cycle.id) {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, %uuid, "failed to build completion snapshot");
            return errors::internal("Failed to complete cycle");
        }
    };
    match repo::complete(&mut conn, uuid, snapshot) {
        Ok(updated) => {
            info!(uuid = %updated.uuid, "cycle completed");
            HttpResponse::Ok().json(updated)
        }
        Err(e) => {
            error!(error = %e, %uuid, "failed to mark cycle complete");
            errors::internal("Failed to complete cycle")
        }
    }
}

pub async fn archive(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Only technicians and admins can archive cycles");
    }
    let uuid = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match repo::archive(&mut conn, uuid) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, %uuid, "failed to archive cycle");
            errors::internal("Failed to archive cycle")
        }
    }
}

pub async fn add_ticket(
    pool: web::Data<Pool>,
    path: web::Path<(Uuid, i32)>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Only technicians and admins can move tickets between cycles");
    }
    let (cycle_uuid, ticket_id) = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let cycle = match repo::find_by_uuid(&mut conn, cycle_uuid) {
        Ok(Some(c)) => c,
        Ok(None) => return errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %cycle_uuid, "cycle lookup failed");
            return errors::internal("Failed to add ticket to cycle");
        }
    };
    match repo::add_ticket(&mut conn, cycle.id, ticket_id, Some(auth.user_uuid)) {
        Ok(membership) => HttpResponse::Created().json(membership),
        Err(e) => {
            error!(error = %e, cycle_id = cycle.id, ticket_id, "add_ticket failed");
            errors::internal("Failed to add ticket to cycle")
        }
    }
}

pub async fn remove_ticket(
    pool: web::Data<Pool>,
    path: web::Path<(Uuid, i32)>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_technician_or_admin() {
        return errors::forbidden("Only technicians and admins can move tickets between cycles");
    }
    let (_cycle_uuid, ticket_id) = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match repo::remove_ticket(&mut conn, ticket_id) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, ticket_id, "remove_ticket failed");
            errors::internal("Failed to remove ticket from cycle")
        }
    }
}

// ----- helpers -----

fn validate_name(name: &str) -> Result<(), &'static str> {
    let trimmed = name.trim();
    if trimmed.len() < NAME_MIN || trimmed.len() > NAME_MAX {
        return Err("Name must be 1 to 120 characters");
    }
    Ok(())
}

fn is_valid_state(state: &str) -> bool {
    matches!(state, "planned" | "active")
}
