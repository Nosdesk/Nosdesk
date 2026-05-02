use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use diesel::Connection;
use serde::Deserialize;
use tracing::debug;
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::helpers;
use crate::handlers::errors;
use crate::handlers::sse::{SseEvent, SseState};
use crate::models::{Claims, NewProject, ProjectUpdate};
use crate::repository;
use crate::sync::actor::ActorContext;
use crate::sync::session;
use crate::utils::rbac::{require_admin, require_technician_or_admin};

/// Build an ActorContext from JWT claims attached to the request.
fn actor_for(req: &HttpRequest) -> ActorContext {
    let uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());
    match uuid {
        Some(u) => ActorContext::user(u, None),
        None => ActorContext::system("handler:projects"),
    }
}

/// Run a repository write inside a transaction with the actor GUCs
/// set, so any sync_actions emitted by the repo carry the right
/// actor_uuid.
fn with_actor<T>(
    conn: &mut crate::db::DbConnection,
    actor: &ActorContext,
    f: impl FnOnce(&mut crate::db::DbConnection) -> diesel::QueryResult<T>,
) -> diesel::QueryResult<T> {
    conn.transaction(|conn| {
        session::set_actor(conn, actor)?;
        f(conn)
    })
}

#[derive(Deserialize)]
pub struct GetProjectQuery {
    /// Comma-separated sub-resource keys to embed in the response.
    /// Currently only `tickets` is recognised, more can join as
    /// they're identified. Omit for the legacy lean response.
    pub embed: Option<String>,
}

/// Extract the SSE client ID from the request header (for echo suppression).
fn extract_sse_client_id(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("X-SSE-Client-Id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
}

// Get all projects with ticket counts
pub async fn get_all_projects(
    pool: web::Data<Pool>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::get_projects_with_ticket_count(&mut conn) {
        Ok(projects) => HttpResponse::Ok().json(projects),
        Err(_) => errors::internal("Failed to get projects"),
    }
}

// Get a single project by ID with ticket count, optionally with the
// full ticket list embedded (`?embed=tickets`).
pub async fn get_project(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    query: web::Query<GetProjectQuery>,
) -> impl Responder {
    let project_id = path.into_inner();
    let want_tickets = query
        .embed
        .as_deref()
        .map(|s| s.split(',').map(str::trim).any(|t| t == "tickets"))
        .unwrap_or(false);

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let mut project = match repository::get_project_with_ticket_count(&mut conn, project_id) {
        Ok(p) => p,
        Err(Error::NotFound) => return errors::not_found_msg("Project not found"),
        Err(_) => return errors::internal("Failed to get project"),
    };

    if want_tickets {
        match repository::get_project_tickets(&mut conn, project_id) {
            Ok(tickets) => project.tickets = Some(tickets),
            Err(_) => return errors::internal("Failed to embed project tickets"),
        }
    }

    HttpResponse::Ok().json(project)
}

// Create a new project (technician or admin only)
pub async fn create_project(
    req: HttpRequest,
    pool: web::Data<Pool>,
    project: web::Json<NewProject>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::create_project(conn, project.into_inner())
    }) {
        Ok(project) => HttpResponse::Created().json(project),
        Err(_) => errors::internal("Failed to create project"),
    }
}

// Update an existing project (technician or admin only)
pub async fn update_project(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    project_update: web::Json<ProjectUpdate>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let project_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::update_project(conn, project_id, project_update.into_inner())
    }) {
        Ok(project) => HttpResponse::Ok().json(project),
        Err(e) => {
            match e {
                Error::NotFound => errors::not_found_msg("Project not found"),
                _ => errors::internal("Failed to update project"),
            }
        }
    }
}

// Delete a project (admin only)
pub async fn delete_project(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let project_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::delete_project(conn, project_id)
    }) {
        Ok(0) => errors::not_found_msg("Project not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(_) => errors::internal("Failed to delete project"),
    }
}

// Get all tickets in a project
pub async fn get_project_tickets(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let project_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::get_project_tickets(&mut conn, project_id) {
        Ok(tickets) => HttpResponse::Ok().json(tickets),
        Err(_) => errors::internal("Failed to get project tickets"),
    }
}

// Add a ticket to a project (technician or admin only)
pub async fn add_ticket_to_project(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<SseState>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let source_client_id = extract_sse_client_id(&req);
    let (project_id, ticket_id) = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::add_ticket_to_project(conn, project_id, ticket_id)
    }) {
        Ok(association) => {
            debug!(ticket_id = ticket_id, project_id = project_id, "Broadcasting SSE event: Ticket assigned to project");
            sse_state.broadcast_event_from(
                SseEvent::ProjectAssigned {
                    ticket_id,
                    project_id,
                    timestamp: chrono::Utc::now(),
                },
                source_client_id,
            ).await;

            HttpResponse::Created().json(association)
        },
        Err(_) => errors::internal("Failed to add ticket to project"),
    }
}

// Remove a ticket from a project (technician or admin only)
pub async fn remove_ticket_from_project(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<SseState>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let source_client_id = extract_sse_client_id(&req);
    let (project_id, ticket_id) = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::remove_ticket_from_project(conn, project_id, ticket_id)
    }) {
        Ok(0) => errors::not_found_msg("Association not found"),
        Ok(_) => {
            debug!(ticket_id = ticket_id, project_id = project_id, "Broadcasting SSE event: Ticket unassigned from project");
            sse_state.broadcast_event_from(
                SseEvent::ProjectUnassigned {
                    ticket_id,
                    project_id,
                    timestamp: chrono::Utc::now(),
                },
                source_client_id,
            ).await;

            HttpResponse::NoContent().finish()
        },
        Err(_) => errors::internal("Failed to remove ticket from project"),
    }
}

/// Request body for updating ticket order within a project
#[derive(Debug, serde::Deserialize)]
pub struct UpdateTicketOrderRequest {
    /// List of ticket IDs in their new order
    pub ticket_ids: Vec<i32>,
}

// Update the order of tickets within a project (technician or admin only)
pub async fn update_ticket_order(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateTicketOrderRequest>,
) -> impl Responder {
    if let Err(e) = require_technician_or_admin(&req) {
        return e;
    }

    let project_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Convert ticket_ids to (ticket_id, display_order) pairs
    let orders: Vec<(i32, i32)> = body
        .ticket_ids
        .iter()
        .enumerate()
        .map(|(idx, &ticket_id)| (ticket_id, idx as i32))
        .collect();

    debug!(project_id, count = orders.len(), "Updating ticket order");

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::update_project_ticket_orders(conn, project_id, orders)
    }) {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({"success": true})),
        Err(_) => errors::internal("Failed to update ticket order"),
    }
} 