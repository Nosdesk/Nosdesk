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
use diesel::Connection;
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{CycleUpdate, NewCycle};
use crate::repository::cycles as repo;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/cycles",
        web::get().to(crate::handlers::cycles::list_workspace),
    )
    .route(
        "/projects/{project_id}/cycles",
        web::get().to(crate::handlers::cycles::list),
    )
    .route(
        "/projects/{project_id}/cycles",
        web::post().to(crate::handlers::cycles::create),
    )
    .route(
        "/cycles/{uuid}",
        web::get().to(crate::handlers::cycles::get_one),
    )
    .route(
        "/cycles/{uuid}",
        web::patch().to(crate::handlers::cycles::patch),
    )
    .route(
        "/cycles/{uuid}",
        web::delete().to(crate::handlers::cycles::archive),
    )
    .route(
        "/cycles/{uuid}/complete",
        web::post().to(crate::handlers::cycles::complete),
    )
    .route(
        "/cycles/{uuid}/stats",
        web::get().to(crate::handlers::cycles::stats),
    )
    .route(
        "/cycles/{uuid}/burnup",
        web::get().to(crate::handlers::cycles::burnup),
    )
    .route(
        "/cycles/{uuid}/tickets",
        web::get().to(crate::handlers::cycles::tickets),
    )
    .route(
        "/cycles/{uuid}/tickets/{ticket_id}",
        web::post().to(crate::handlers::cycles::add_ticket),
    )
    .route(
        "/cycles/{uuid}/tickets/{ticket_id}",
        web::delete().to(crate::handlers::cycles::remove_ticket),
    );
}

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

pub async fn list(mut tc: TenantConn, path: web::Path<i32>, _auth: AuthContext) -> impl Responder {
    let project_id = path.into_inner();
    match tc.run(|conn| repo::list_for_project(conn, project_id)) {
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
    mut tc: TenantConn,
    query: web::Query<WorkspaceListQuery>,
    _auth: AuthContext,
) -> impl Responder {
    let states_owned: Option<Vec<String>> = query.state.as_deref().map(|s| {
        s.split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect()
    });
    let result = tc.run(|conn| {
        let states_ref: Option<Vec<&str>> = states_owned
            .as_ref()
            .map(|v| v.iter().map(|s| s.as_str()).collect());
        let states_slice: Option<&[&str]> = states_ref.as_deref();
        let filter = if states_slice.is_some() {
            states_slice
        } else {
            Some(&["planned", "active"][..])
        };
        repo::list_for_workspace(conn, filter)
    });
    match result {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "failed to list workspace cycles");
            errors::internal("Failed to list cycles")
        }
    }
}

pub async fn get_one(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    match tc.run(|conn| repo::find_by_uuid(conn, uuid)) {
        Ok(Some(cycle)) => HttpResponse::Ok().json(cycle),
        Ok(None) => errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %uuid, "failed to fetch cycle");
            errors::internal("Failed to fetch cycle")
        }
    }
}

pub async fn create(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<CreateBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
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
    let new = NewCycle {
        project_id,
        name: body.name.trim().to_string(),
        start_at: body.start_at,
        end_at: body.end_at,
        state,
        created_by: Some(auth.user_uuid),
    };
    match tc.run(|conn| repo::create(conn, new)) {
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
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    body: web::Json<PatchBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
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
    let patch = CycleUpdate {
        name: body.name.map(|s| s.trim().to_string()),
        start_at: body.start_at,
        end_at: body.end_at,
        state: body.state,
        completion_snapshot: None,
        completed_at: None,
        archived_at: None,
    };
    match tc.run(|conn| repo::update(conn, uuid, patch)) {
        Ok(cycle) => HttpResponse::Ok().json(cycle),
        Err(e) => {
            error!(error = %e, %uuid, "failed to update cycle");
            errors::internal("Failed to update cycle")
        }
    }
}

/// Returns the ticket-id list for a cycle. Phase 8 ScrumBoard uses
/// this to scope its kanban to "tickets in this cycle" without
/// pulling the cycle_tickets aggregate into the sync engine.
pub async fn tickets(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    let result = tc.run(|conn| match repo::find_by_uuid(conn, uuid)? {
        Some(cycle) => repo::ticket_ids_for_cycle(conn, cycle.id).map(Some),
        None => Ok(None),
    });
    match result {
        Ok(Some(ids)) => actix_web::HttpResponse::Ok().json(ids),
        Ok(None) => errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %uuid, "tickets: load failed");
            errors::internal("Failed to fetch cycle tickets")
        }
    }
}

/// Live stats for a cycle. For completed cycles returns the frozen
/// completion_snapshot; for planned/active cycles computes the same
/// shape on the fly. The frontend Burndown widget renders both
/// through the same code path.
pub async fn stats(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    let result = tc.run(|conn| match repo::find_by_uuid(conn, uuid)? {
        Some(cycle) => {
            if let Some(snap) = cycle.completion_snapshot.clone() {
                Ok(Some(snap))
            } else {
                repo::build_completion_snapshot(conn, &cycle).map(Some)
            }
        }
        None => Ok(None),
    });
    match result {
        Ok(Some(snap)) => HttpResponse::Ok().json(snap),
        Ok(None) => errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %uuid, "stats: load failed");
            errors::internal("Failed to fetch cycle stats")
        }
    }
}

/// Count-based burnup series for a cycle: completed vs total scope
/// over the cycle timeline, reconstructed from member add times and
/// ticket close times. Active/planned cycles only; completed cycles
/// keep the frozen snapshot view (no daily series is stored).
pub async fn burnup(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    let result = tc.run(|conn| match repo::find_by_uuid(conn, uuid)? {
        Some(cycle) => repo::build_burnup(conn, &cycle).map(Some),
        None => Ok(None),
    });
    match result {
        Ok(Some(series)) => HttpResponse::Ok().json(series),
        Ok(None) => errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %uuid, "burnup: load failed");
            errors::internal("Failed to fetch cycle burnup")
        }
    }
}

/// Result variants for the cycle complete flow.
enum CompleteOutcome {
    Completed(crate::models::Cycle),
    NotFound,
    AlreadyCompleted,
}

pub async fn complete(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Only technicians and admins can complete cycles");
    }
    let uuid = path.into_inner();
    let result = tc.run(|conn| match repo::find_by_uuid(conn, uuid)? {
        Some(cycle) => {
            if cycle.state == "completed" {
                return Ok(CompleteOutcome::AlreadyCompleted);
            }
            // Build snapshot, carry over incomplete tickets, then mark
            // complete as one atomic unit. The snapshot is built first
            // so it counts the cycle's full membership; carryover then
            // moves the still-open tickets to the next cycle (or the
            // backlog) and records the count under `carried_over`.
            conn.transaction::<_, diesel::result::Error, _>(|conn| {
                let mut snapshot = repo::build_completion_snapshot(conn, &cycle)?;
                let carried = repo::carry_over_incomplete(conn, &cycle)?;
                snapshot["carried_over"] = serde_json::json!(carried);
                let updated = repo::complete(conn, uuid, snapshot)?;
                Ok(CompleteOutcome::Completed(updated))
            })
        }
        None => Ok(CompleteOutcome::NotFound),
    });
    match result {
        Ok(CompleteOutcome::Completed(updated)) => {
            info!(uuid = %updated.uuid, "cycle completed");
            HttpResponse::Ok().json(updated)
        }
        Ok(CompleteOutcome::NotFound) => errors::not_found_msg("Cycle not found"),
        Ok(CompleteOutcome::AlreadyCompleted) => errors::bad_request("Cycle is already completed"),
        Err(e) => {
            error!(error = %e, %uuid, "failed to complete cycle");
            errors::internal("Failed to complete cycle")
        }
    }
}

pub async fn archive(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Only technicians and admins can archive cycles");
    }
    let uuid = path.into_inner();
    match tc.run(|conn| repo::archive(conn, uuid)) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, %uuid, "failed to archive cycle");
            errors::internal("Failed to archive cycle")
        }
    }
}

/// Result variants for adding a ticket to a cycle.
enum AddTicketOutcome {
    Added(crate::models::CycleTicket),
    NotFound,
}

pub async fn add_ticket(
    mut tc: TenantConn,
    path: web::Path<(Uuid, i32)>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Only technicians and admins can move tickets between cycles");
    }
    let (cycle_uuid, ticket_id) = path.into_inner();
    let actor_uuid = auth.user_uuid;
    let result = tc.run(|conn| match repo::find_by_uuid(conn, cycle_uuid)? {
        Some(cycle) => {
            let membership = repo::add_ticket(conn, cycle.id, ticket_id, Some(actor_uuid))?;
            Ok(AddTicketOutcome::Added(membership))
        }
        None => Ok(AddTicketOutcome::NotFound),
    });
    match result {
        Ok(AddTicketOutcome::Added(membership)) => HttpResponse::Created().json(membership),
        Ok(AddTicketOutcome::NotFound) => errors::not_found_msg("Cycle not found"),
        Err(e) => {
            error!(error = %e, %cycle_uuid, ticket_id, "add_ticket failed");
            errors::internal("Failed to add ticket to cycle")
        }
    }
}

pub async fn remove_ticket(
    mut tc: TenantConn,
    path: web::Path<(Uuid, i32)>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Only technicians and admins can move tickets between cycles");
    }
    let (_cycle_uuid, ticket_id) = path.into_inner();
    match tc.run(|conn| repo::remove_ticket(conn, ticket_id)) {
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
