use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::Deserialize;
use tracing::debug;

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::handlers::sse::{SseEvent, SseState};
use crate::models::{NewProject, ProjectUpdate, WorkspaceRole};
use crate::repository;
use crate::services::search::SearchService;
use crate::utils::rbac::require_workspace_role;
use std::sync::Arc;

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
pub async fn get_all_projects(mut tc: TenantConn) -> impl Responder {
    match tc.run(repository::get_projects_with_ticket_count) {
        Ok(projects) => HttpResponse::Ok().json(projects),
        Err(_) => errors::internal("Failed to get projects"),
    }
}

// Get a single project by ID with ticket count, optionally with the
// full ticket list embedded (`?embed=tickets`).
pub async fn get_project(
    mut tc: TenantConn,
    path: web::Path<i32>,
    query: web::Query<GetProjectQuery>,
) -> impl Responder {
    let project_id = path.into_inner();
    let want_tickets = query
        .embed
        .as_deref()
        .map(|s| s.split(',').map(str::trim).any(|t| t == "tickets"))
        .unwrap_or(false);

    let mut project =
        match tc.run(|conn| repository::get_project_with_ticket_count(conn, project_id)) {
            Ok(p) => p,
            Err(Error::NotFound) => return errors::not_found_msg("Project not found"),
            Err(_) => return errors::internal("Failed to get project"),
        };

    if want_tickets {
        match tc.run(|conn| repository::get_project_tickets(conn, project_id)) {
            Ok(tickets) => project.tickets = Some(tickets),
            Err(_) => return errors::internal("Failed to embed project tickets"),
        }
    }

    HttpResponse::Ok().json(project)
}

// Create a new project (technician or admin only)
pub async fn create_project(
    req: HttpRequest,
    mut tc: TenantConn,
    project: web::Json<NewProject>,
    search_service: Option<web::Data<Arc<SearchService>>>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }

    let observer = search_service
        .as_ref()
        .map(|d| d.get_ref() as &dyn repository::projects::ProjectIndexedObserver);
    match tc.run(|conn| repository::create_project(conn, project.into_inner(), observer)) {
        Ok(project) => HttpResponse::Created().json(project),
        Err(_) => errors::internal("Failed to create project"),
    }
}

// Update an existing project (technician or admin only)
pub async fn update_project(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
    project_update: web::Json<ProjectUpdate>,
    search_service: Option<web::Data<Arc<SearchService>>>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }

    let observer = search_service
        .as_ref()
        .map(|d| d.get_ref() as &dyn repository::projects::ProjectIndexedObserver);
    let project_id = path.into_inner();
    match tc.run(|conn| {
        repository::update_project(conn, project_id, project_update.into_inner(), observer)
    }) {
        Ok(project) => HttpResponse::Ok().json(project),
        Err(e) => match e {
            Error::NotFound => errors::not_found_msg("Project not found"),
            _ => errors::internal("Failed to update project"),
        },
    }
}

// Delete a project (admin only)
pub async fn delete_project(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
    search_service: Option<web::Data<Arc<SearchService>>>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let observer = search_service
        .as_ref()
        .map(|d| d.get_ref() as &dyn repository::projects::ProjectDeletedObserver);
    let project_id = path.into_inner();
    match tc.run(|conn| repository::delete_project(conn, project_id, observer)) {
        Ok(0) => errors::not_found_msg("Project not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(_) => errors::internal("Failed to delete project"),
    }
}

// Get all tickets in a project
pub async fn get_project_tickets(mut tc: TenantConn, path: web::Path<i32>) -> impl Responder {
    let project_id = path.into_inner();
    match tc.run(|conn| repository::get_project_tickets(conn, project_id)) {
        Ok(tickets) => HttpResponse::Ok().json(tickets),
        Err(_) => errors::internal("Failed to get project tickets"),
    }
}

/// Dependency edges for the project's Gantt view. Returns one
/// row per linked_tickets entry where both ends fall inside the
/// project. The Gantt renders `blocks` arrows; other link kinds
/// round-trip so the renderer can switch them on without a
/// backend change.
pub async fn get_project_dependencies(mut tc: TenantConn, path: web::Path<i32>) -> impl Responder {
    let project_id = path.into_inner();
    match tc.run(|conn| repository::linked_tickets::dependencies_for_project(conn, project_id)) {
        Ok(rows) => {
            let payload: Vec<serde_json::Value> = rows
                .into_iter()
                .map(|(from_id, to_id, relation_type)| {
                    serde_json::json!({
                        "from": from_id,
                        "to": to_id,
                        "relation_type": relation_type,
                    })
                })
                .collect();
            HttpResponse::Ok().json(payload)
        }
        Err(_) => errors::internal("Failed to load project dependencies"),
    }
}

// Add a ticket to a project (technician or admin only)
pub async fn add_ticket_to_project(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<SseState>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }

    let source_client_id = extract_sse_client_id(&req);
    let (project_id, ticket_id) = path.into_inner();

    match tc.run(|conn| repository::add_ticket_to_project(conn, project_id, ticket_id)) {
        Ok(association) => {
            debug!(
                ticket_id = ticket_id,
                project_id = project_id,
                "Broadcasting SSE event: Ticket assigned to project"
            );
            sse_state
                .broadcast_event_from(
                    SseEvent::ProjectAssigned {
                        ticket_id,
                        project_id,
                        timestamp: chrono::Utc::now(),
                    },
                    source_client_id,
                )
                .await;

            HttpResponse::Created().json(association)
        }
        Err(_) => errors::internal("Failed to add ticket to project"),
    }
}

/// Body for the kanban quick-add: a title and the column's
/// workflow state. Everything else uses ticket defaults.
#[derive(Deserialize)]
pub struct QuickAddTicket {
    pub title: String,
    pub workflow_state_id: i32,
}

/// Create a ticket directly in a project (kanban column quick-add).
/// The create and the project link happen in one transaction so the
/// new card streams to the board's `project:<id>` sync group
/// immediately (technician or admin only).
pub async fn create_ticket_in_project(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<QuickAddTicket>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }

    let project_id = path.into_inner();
    let body = body.into_inner();
    let title = body.title.trim();
    if title.is_empty() {
        return errors::bad_request("Title must not be empty");
    }

    let requester_uuid = req
        .extensions()
        .get::<crate::models::Claims>()
        .and_then(|c| crate::utils::parse_uuid(&c.sub).ok());

    let new_ticket = crate::models::NewTicket {
        title: title.to_string(),
        workflow_state_id: body.workflow_state_id,
        requester_uuid,
        ..Default::default()
    };

    match tc.run(|conn| repository::create_ticket_in_project(conn, new_ticket, project_id)) {
        Ok(ticket) => HttpResponse::Created().json(ticket),
        Err(Error::NotFound) => errors::not_found_msg("Project not found"),
        Err(_) => errors::internal("Failed to create ticket in project"),
    }
}

// Remove a ticket from a project (technician or admin only)
pub async fn remove_ticket_from_project(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<SseState>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }

    let source_client_id = extract_sse_client_id(&req);
    let (project_id, ticket_id) = path.into_inner();

    match tc.run(|conn| repository::remove_ticket_from_project(conn, project_id, ticket_id)) {
        Ok(0) => errors::not_found_msg("Association not found"),
        Ok(_) => {
            debug!(
                ticket_id = ticket_id,
                project_id = project_id,
                "Broadcasting SSE event: Ticket unassigned from project"
            );
            sse_state
                .broadcast_event_from(
                    SseEvent::ProjectUnassigned {
                        ticket_id,
                        project_id,
                        timestamp: chrono::Utc::now(),
                    },
                    source_client_id,
                )
                .await;

            HttpResponse::NoContent().finish()
        }
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
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<UpdateTicketOrderRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Agent) {
        return e;
    }

    let project_id = path.into_inner();

    // Convert ticket_ids to (ticket_id, display_order) pairs
    let orders: Vec<(i32, i32)> = body
        .ticket_ids
        .iter()
        .enumerate()
        .map(|(idx, &ticket_id)| (ticket_id, idx as i32))
        .collect();

    debug!(project_id, count = orders.len(), "Updating ticket order");

    match tc.run(|conn| repository::update_project_ticket_orders(conn, project_id, orders)) {
        Ok(_) => HttpResponse::Ok().json(serde_json::json!({"success": true})),
        Err(_) => errors::internal("Failed to update ticket order"),
    }
}

#[cfg(test)]
mod tests {
    //! Permission-boundary tests. Projects use a mixed gate:
    //! create/update is technician-or-admin, delete is admin-only.
    //! The most consequential surface is delete (cascades to project
    //! membership), so we test that explicitly.
    use super::*;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/projects/{id}", web::delete().to(delete_project))
    }

    #[actix_web::test]
    async fn delete_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::delete()
            .uri("/projects/1")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn delete_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, "user");
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/projects/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn delete_rejects_technician_role() {
        // Even technicians can't delete projects — only admins.
        // This catches accidental gate downgrades from require_admin
        // to require_technician_or_admin.
        let pool = setup_test_pool();
        let claims = claims_for(&pool, "technician");
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/projects/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
