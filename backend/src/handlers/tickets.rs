use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use tracing::{debug, error, warn, info};
use uuid::Uuid;

use crate::models::{AssignmentTrigger, Claims, NewTicket, TicketPriority, TicketStatus, TicketUpdate, TicketsJson, UserRole};
use crate::repository;
use crate::services::assignment::AssignmentEngine;
use crate::services::notifications::{
    NotificationService,
    types::{NotificationTypeCode, NotificationPayload, NotificationEntity, NotificationActor},
};
use crate::utils::rbac::{is_admin, is_technician_or_admin};
use crate::utils::sse::SseBroadcaster;

// Helper type for database operations with proper error handling
type DbResult<T> = Result<T, HttpResponse>;

// Helper function to get database connection with error handling
async fn get_db_conn(pool: &web::Data<crate::db::Pool>) -> DbResult<crate::db::DbConnection> {
    pool.get()
        .map_err(|_| HttpResponse::InternalServerError().json("Database connection error"))
}

// Helper function to extract user UUID from JWT claims
fn get_user_uuid_from_claims(claims: &Claims) -> Result<Uuid, HttpResponse> {
    Uuid::parse_str(&claims.sub)
        .map_err(|_| HttpResponse::BadRequest().json(json!({
            "error": "Invalid user UUID",
            "message": "The user UUID in the authentication token is invalid"
        })))
}

// Helper function to validate assignee role
fn validate_assignee_role(
    assignee_uuid: &Uuid,
    conn: &mut crate::db::DbConnection,
) -> Result<(), HttpResponse> {
    match crate::repository::users::get_user_by_uuid(assignee_uuid, conn) {
        Ok(user) => {
            // Check if user has technician or admin role
            if user.role != UserRole::Technician && user.role != UserRole::Admin {
                Err(HttpResponse::BadRequest().json(json!({
                    "error": "Invalid assignee",
                    "message": "Only technicians and administrators can be assigned to tickets"
                })))
            } else {
                Ok(())
            }
        }
        Err(_) => Err(HttpResponse::BadRequest().json(json!({
            "error": "User not found",
            "message": "The specified assignee does not exist"
        }))),
    }
}

// Helper function to parse and validate assignee from string (for update operations)
fn parse_and_validate_assignee_string(
    assignee_str: &str,
    conn: &mut crate::db::DbConnection,
) -> Result<Uuid, HttpResponse> {
    // Try to parse as UUID first
    if let Ok(uuid) = Uuid::parse_str(assignee_str) {
        // Use the same validation logic but adapted for the update context
        match crate::repository::users::get_user_by_uuid(&uuid, conn) {
            Ok(user) => {
                if user.role != UserRole::Technician && user.role != UserRole::Admin {
                    Err(HttpResponse::BadRequest().json(json!({
                        "error": "Invalid assignee",
                        "message": "Only technicians and administrators can be assigned to tickets"
                    })))
                } else {
                    Ok(uuid)
                }
            }
            Err(_) => Err(HttpResponse::BadRequest().json(json!({
                "error": "User not found",
                "message": "The specified assignee does not exist"
            }))),
        }
    } else {
        // Try to look up by name
        match crate::repository::users::get_user_by_name(assignee_str, conn) {
            Ok(user) => {
                if user.role != UserRole::Technician && user.role != UserRole::Admin {
                    Err(HttpResponse::BadRequest().json(json!({
                        "error": "Invalid assignee",
                        "message": "Only technicians and administrators can be assigned to tickets"
                    })))
                } else {
                    Ok(user.uuid)
                }
            }
            Err(_) => Err(HttpResponse::BadRequest().json(json!({
                "error": "User not found",
                "message": "The specified assignee does not exist"
            }))),
        }
    }
}

// Simple helper to broadcast SSE events without blocking
async fn broadcast_sse_simple(
    sse_state: web::Data<crate::handlers::sse::SseState>,
    ticket_id: i32,
    event_type: String,
    data: serde_json::Value,
) {
    use crate::utils::sse::SseBroadcaster;

    tokio::spawn(async move {
        match event_type.as_str() {
            "ticket_updated" => {
                if let (Some(key), Some(value), Some(user_sub)) = (
                    data.get("key").and_then(|v| v.as_str()),
                    data.get("value"),
                    data.get("user_sub").and_then(|v| v.as_str()),
                ) {
                    SseBroadcaster::broadcast_ticket_updated(
                        &sse_state,
                        ticket_id,
                        key,
                        value.clone(),
                        user_sub,
                    )
                    .await;
                }
            }
            "ticket_linked" => {
                if let Some(linked_id) = data.get("linked_ticket_id").and_then(|v| v.as_u64()) {
                    SseBroadcaster::broadcast_ticket_linked(
                        &sse_state,
                        ticket_id,
                        linked_id as i32,
                    )
                    .await;
                }
            }
            "ticket_unlinked" => {
                if let Some(linked_id) = data.get("linked_ticket_id").and_then(|v| v.as_u64()) {
                    SseBroadcaster::broadcast_ticket_unlinked(
                        &sse_state,
                        ticket_id,
                        linked_id as i32,
                    )
                    .await;
                }
            }
            "device_linked" => {
                if let Some(device_id) = data.get("device_id").and_then(|v| v.as_u64()) {
                    SseBroadcaster::broadcast_device_linked(
                        &sse_state,
                        ticket_id,
                        device_id as i32,
                    )
                    .await;
                }
            }
            "device_unlinked" => {
                if let Some(device_id) = data.get("device_id").and_then(|v| v.as_u64()) {
                    SseBroadcaster::broadcast_device_unlinked(
                        &sse_state,
                        ticket_id,
                        device_id as i32,
                    )
                    .await;
                }
            }
            _ => warn!(event_type = %event_type, "Unknown SSE event type"),
        }
    });
}

// Pagination query parameters
#[derive(Deserialize)]
pub struct PaginationParams {
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
    #[serde(rename = "sortField")]
    sort_field: Option<String>,
    #[serde(rename = "sortDirection")]
    sort_direction: Option<String>,
    search: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    category: Option<String>,
    assignee: Option<String>,
    requester: Option<String>,
    // Date filtering parameters
    #[serde(rename = "createdAfter")]
    created_after: Option<String>,
    #[serde(rename = "createdBefore")]
    created_before: Option<String>,
    #[serde(rename = "createdOn")]
    created_on: Option<String>,
    #[serde(rename = "modifiedAfter")]
    modified_after: Option<String>,
    #[serde(rename = "modifiedBefore")]
    modified_before: Option<String>,
    #[serde(rename = "modifiedOn")]
    modified_on: Option<String>,
    #[serde(rename = "closedAfter")]
    closed_after: Option<String>,
    #[serde(rename = "closedBefore")]
    closed_before: Option<String>,
    #[serde(rename = "closedOn")]
    closed_on: Option<String>,
}

// Paginated response
#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    data: Vec<T>,
    total: i64,
    page: i64,
    #[serde(rename = "pageSize")]
    page_size: i64,
    #[serde(rename = "totalPages")]
    total_pages: i64,
}

// Get all tickets
pub async fn get_tickets(pool: web::Data<crate::db::Pool>) -> impl Responder {
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::get_all_tickets(&mut conn) {
        Ok(tickets) => HttpResponse::Ok().json(tickets),
        Err(_) => HttpResponse::InternalServerError().json("Failed to get tickets"),
    }
}

// Get paginated tickets
pub async fn get_paginated_tickets(
    pool: web::Data<crate::db::Pool>,
    query: web::Query<PaginationParams>,
) -> impl Responder {
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    // Extract and validate pagination parameters
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(10).clamp(1, 100);

    match repository::get_paginated_tickets_with_users(
        &mut conn,
        page,
        page_size,
        query.sort_field.clone(),
        query.sort_direction.clone(),
        query.search.clone(),
        query.status.clone(),
        query.priority.clone(),
        query.category.clone(),
        query.assignee.clone(),
        query.requester.clone(),
        query.created_after.clone(),
        query.created_before.clone(),
        query.created_on.clone(),
        query.modified_after.clone(),
        query.modified_before.clone(),
        query.modified_on.clone(),
        query.closed_after.clone(),
        query.closed_before.clone(),
        query.closed_on.clone(),
    ) {
        Ok((tickets, total)) => {
            // Calculate total pages
            let total_pages = (total as f64 / page_size as f64).ceil() as i64;

            // Create paginated response
            let response = PaginatedResponse {
                data: tickets,
                total,
                page,
                page_size,
                total_pages,
            };

            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(error = ?e, "Failed to fetch paginated tickets");
            HttpResponse::InternalServerError().json("Failed to get paginated tickets")
        }
    }
}

// Get a ticket by ID with comments and related info
pub async fn get_ticket(
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
    claims: web::ReqData<Claims>,
) -> impl Responder {
    use crate::repository::user_ticket_views::UserTicketViewsRepository;

    let ticket_id = params.into_inner();
    let claims_inner = claims.into_inner();

    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    // Get the ticket first
    let complete_ticket = match repository::get_complete_ticket(&mut conn, ticket_id) {
        Ok(ticket) => ticket,
        Err(_) => return HttpResponse::NotFound().json("Ticket not found"),
    };

    // Record the view (don't fail the request if this fails)
    let user_uuid = match get_user_uuid_from_claims(&claims_inner) {
        Ok(uuid) => uuid,
        Err(_) => {
            // Log but don't fail - still return the ticket
            warn!("Invalid user UUID in claims, cannot record view");
            return HttpResponse::Ok().json(complete_ticket);
        }
    };

    let view_repo = UserTicketViewsRepository::new(pool.get_ref().clone());
    if let Err(e) = view_repo.record_view(user_uuid, ticket_id) {
        warn!(user_uuid = %user_uuid, error = ?e, "Failed to record ticket view");
    }

    HttpResponse::Ok().json(complete_ticket)
}

// Create a new ticket
pub async fn create_ticket(
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    ticket: web::Json<NewTicket>,
) -> impl Responder {
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };
    let new_ticket = ticket.into_inner();

    // Validate assignee role if assignee is set
    if let Some(assignee_uuid) = new_ticket.assignee_uuid {
        if let Err(e) = validate_assignee_role(&assignee_uuid, &mut conn) {
            return e;
        }
    }

    match repository::create_ticket(&mut conn, new_ticket) {
        Ok(ticket) => {
            // Broadcast ticket creation via SSE
            crate::utils::sse::SseBroadcaster::broadcast_ticket_created(
                &sse_state,
                ticket.id,
                serde_json::to_value(&ticket).unwrap_or_default(),
            ).await;

            HttpResponse::Created().json(ticket)
        },
        Err(_) => HttpResponse::InternalServerError().json("Failed to create ticket"),
    }
}

// Update a ticket
pub async fn update_ticket(
    pool: web::Data<crate::db::Pool>,
    path: web::Path<i32>,
    ticket: web::Json<NewTicket>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };
    let new_ticket = ticket.into_inner();

    // Validate assignee role if assignee is set
    if let Some(assignee_uuid) = new_ticket.assignee_uuid {
        if let Err(e) = validate_assignee_role(&assignee_uuid, &mut conn) {
            return e;
        }
    }

    match repository::update_ticket(&mut conn, ticket_id, new_ticket) {
        Ok(ticket) => HttpResponse::Ok().json(ticket),
        Err(e) => {
            HttpResponse::InternalServerError().json(format!("Failed to update ticket: {}", e))
        }
    }
}

// Delete a ticket with comprehensive cleanup
pub async fn delete_ticket(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<std::sync::Arc<dyn crate::utils::storage::Storage>>,
    path: web::Path<i32>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can delete tickets"
        }));
    }

    let ticket_id = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    // Use the comprehensive deletion function that cleans up files
    match repository::delete_ticket_with_cleanup(&mut conn, ticket_id, storage.as_ref().clone())
        .await
    {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                HttpResponse::NoContent().finish()
            } else {
                HttpResponse::NotFound().json("Ticket not found")
            }
        }
        Err(_) => HttpResponse::InternalServerError().json("Failed to delete ticket"),
    }
}

// Import tickets from JSON file
pub async fn import_tickets_from_json(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    json_path: web::Path<String>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can import tickets"
        }));
    }

    let json_path_str = json_path.into_inner();
    let path = Path::new(&json_path_str);

    let json_content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(e) => {
            return HttpResponse::InternalServerError()
                .json(json!({ "error": format!("Failed to read file: {}", e) }));
        }
    };

    // Parse the JSON
    let tickets_json: TicketsJson = match serde_json::from_str(&json_content) {
        Ok(tickets) => tickets,
        Err(_) => return HttpResponse::BadRequest().json("Failed to parse JSON"),
    };

    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    // Import each ticket
    let mut imported_count = 0;
    let mut failed_count = 0;

    for ticket_json in tickets_json.tickets {
        match repository::import_ticket_from_json(&mut conn, &ticket_json) {
            Ok(_) => imported_count += 1,
            Err(_) => failed_count += 1,
        }
    }

    HttpResponse::Ok().json(json!({
        "imported": imported_count,
        "failed": failed_count
    }))
}

// Import tickets from JSON string
pub async fn import_tickets_from_json_string(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    tickets_json: web::Json<TicketsJson>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can import tickets"
        }));
    }

    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    // Import each ticket
    let mut imported_count = 0;
    let mut failed_count = 0;

    for ticket_json in tickets_json.tickets.iter() {
        match repository::import_ticket_from_json(&mut conn, ticket_json) {
            Ok(_) => imported_count += 1,
            Err(_) => failed_count += 1,
        }
    }

    HttpResponse::Ok().json(json!({
        "imported": imported_count,
        "failed": failed_count
    }))
}

// Create an empty ticket with default values
pub async fn create_empty_ticket(
    pool: web::Data<crate::db::Pool>,
    notification_service: web::Data<NotificationService>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    // Extract claims from request extensions (set by cookie_auth_middleware)
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    // Parse the user UUID from the JWT claims
    let user_uuid = match crate::utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID in token"),
    };

    // Create a new ticket with default values using the authenticated user's UUID
    let empty_ticket = NewTicket {
        title: "New Ticket".to_string(),
        description: Some("".to_string()),
        status: TicketStatus::Open,
        priority: TicketPriority::Medium,
        requester_uuid: Some(user_uuid), // Use authenticated user's UUID
        assignee_uuid: None,
        category_id: None,
    };

    // Create the ticket and then add empty article content
    let mut ticket = match repository::create_ticket(&mut conn, empty_ticket) {
        Ok(ticket) => ticket,
        Err(e) => {
            error!(error = ?e, "Failed to create empty ticket");
            return HttpResponse::InternalServerError()
                .json(format!("Failed to create empty ticket: {}", e));
        }
    };

    // Run automatic assignment rules if no assignee
    if ticket.assignee_uuid.is_none() {
        if let Some(result) = AssignmentEngine::evaluate_rules(&mut conn, &ticket, AssignmentTrigger::TicketCreated) {
            // Update ticket with auto-assigned user
            if let Some(assigned_uuid) = result.assigned_user_uuid {
                let assign_update = TicketUpdate {
                    assignee_uuid: Some(Some(assigned_uuid)),
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    ..Default::default()
                };
                if let Ok(updated) = repository::update_ticket_partial(&mut conn, ticket.id, assign_update) {
                    ticket = updated;
                    info!(
                        ticket_id = ticket.id,
                        assignee = %assigned_uuid,
                        rule = %result.rule_name,
                        method = %result.method,
                        "Auto-assigned new ticket"
                    );

                    // Send notification to the auto-assigned user
                    let notification_service = notification_service.clone();
                    let ticket_id = ticket.id;
                    let ticket_title = ticket.title.clone();
                    let rule_name = result.rule_name.clone();

                    tokio::spawn(async move {
                        let payload = NotificationPayload::new(
                            NotificationTypeCode::TicketAssigned,
                            assigned_uuid,
                            NotificationActor {
                                uuid: Uuid::nil(), // System actor
                                name: "System".to_string(),
                                avatar_thumb: None,
                            },
                            NotificationEntity::Ticket {
                                id: ticket_id,
                                title: ticket_title,
                            },
                        )
                        .with_body(format!(
                            "You have been auto-assigned to ticket #{} (Rule: {})",
                            ticket_id, rule_name
                        ));

                        if let Err(e) = notification_service.notify(payload).await {
                            warn!(error = %e, "Failed to send auto-assignment notification");
                        }
                    });
                }
            }
        }
    }

    // Create empty article content for the ticket
    let new_article_content = crate::models::NewArticleContent {
        ticket_id: ticket.id,
        yjs_state_vector: None,
        yjs_document: None,
        yjs_client_id: None,
    };

    // Try to create article content, but don't fail if it doesn't work
    if let Err(_) = repository::create_article_content(&mut conn, new_article_content) {
        // If article content creation fails, still return the ticket
        return HttpResponse::Created().json(ticket);
    }

    // Return the complete ticket with article content
    match repository::get_complete_ticket(&mut conn, ticket.id) {
        Ok(complete_ticket) => HttpResponse::Created().json(complete_ticket),
        Err(_) => HttpResponse::Created().json(ticket), // Fallback to just the ticket if getting complete ticket fails
    }
}

// Update ticket partially
pub async fn update_ticket_partial(
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    notification_service: web::Data<NotificationService>,
    req: HttpRequest,
    params: web::Path<i32>,
    body: web::Json<Value>,
) -> impl Responder {
    let ticket_id = params.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from request extensions (set by cookie_auth_middleware)
    let user_info = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    // Get the current ticket state for detecting changes (for notifications)
    let old_ticket = repository::get_ticket_by_id(&mut conn, ticket_id).ok();

    // Parse JSON and build TicketUpdate with user lookups
    let mut ticket_update = TicketUpdate {
        title: None,
        description: None,
        status: None,
        priority: None,
        requester_uuid: None,
        assignee_uuid: None,
        updated_at: Some(chrono::Utc::now().naive_utc()),
        closed_at: None,
        category_id: None,
    };

    // Handle simple string fields
    if let Some(title) = body.get("title").and_then(|v| v.as_str()) {
        ticket_update.title = Some(title.to_string());
    }

    if let Some(description) = body.get("description").and_then(|v| v.as_str()) {
        ticket_update.description = Some(description.to_string());
    }

    // Handle enum fields
    if let Some(status_str) = body.get("status").and_then(|v| v.as_str()) {
        match status_str {
            "open" => ticket_update.status = Some(crate::models::TicketStatus::Open),
            "in-progress" => ticket_update.status = Some(crate::models::TicketStatus::InProgress),
            "closed" => ticket_update.status = Some(crate::models::TicketStatus::Closed),
            _ => {}
        }
    }

    if let Some(priority_str) = body.get("priority").and_then(|v| v.as_str()) {
        match priority_str {
            "low" => ticket_update.priority = Some(crate::models::TicketPriority::Low),
            "medium" => ticket_update.priority = Some(crate::models::TicketPriority::Medium),
            "high" => ticket_update.priority = Some(crate::models::TicketPriority::High),
            _ => {}
        }
    }

    // Handle requester (can be name, UUID, or empty string for unassign)
    if let Some(requester_str) = body.get("requester").and_then(|v| v.as_str()) {
        if requester_str.is_empty() {
            // Empty string means unassign
            ticket_update.requester_uuid = Some(None);
        } else if let Ok(uuid) = Uuid::parse_str(requester_str) {
            // It's already a UUID
            ticket_update.requester_uuid = Some(Some(uuid));
        } else {
            // Try to look up by name
            match crate::repository::users::get_user_by_name(requester_str, &mut conn) {
                Ok(user) => ticket_update.requester_uuid = Some(Some(user.uuid)),
                Err(_) => {
                    warn!(name = %requester_str, "Could not find user by name");
                }
            }
        }
    }

    // Handle assignee (can be name, UUID, or empty string for unassign)
    if let Some(assignee_str) = body.get("assignee").and_then(|v| v.as_str()) {
        if assignee_str.is_empty() {
            // Empty string means unassign
            ticket_update.assignee_uuid = Some(None);
        } else {
            // Parse and validate assignee
            match parse_and_validate_assignee_string(assignee_str, &mut conn) {
                Ok(uuid) => ticket_update.assignee_uuid = Some(Some(uuid)),
                Err(response) => return response,
            }
        }
    }

    // Handle category_id (can be a number or null to unassign)
    if body.get("category_id").is_some() {
        match body.get("category_id") {
            Some(Value::Number(n)) => {
                if let Some(id) = n.as_i64() {
                    ticket_update.category_id = Some(Some(id as i32));
                }
            }
            Some(Value::Null) => {
                // Null means remove category
                ticket_update.category_id = Some(None);
            }
            _ => {}
        }
    }

    // Track if category was changed for auto-assignment
    let category_changed = body.get("category_id").is_some();

    // Update the ticket
    match repository::update_ticket_partial(&mut conn, ticket_id, ticket_update) {
        Ok(updated_ticket) => {
            // Run automatic assignment rules if category changed and no assignee
            if category_changed && updated_ticket.assignee_uuid.is_none() {
                if let Some(result) = AssignmentEngine::evaluate_rules(
                    &mut conn,
                    &updated_ticket,
                    AssignmentTrigger::CategoryChanged,
                ) {
                    // Update ticket with auto-assigned user
                    if let Some(assigned_uuid) = result.assigned_user_uuid {
                        let assign_update = TicketUpdate {
                            assignee_uuid: Some(Some(assigned_uuid)),
                            updated_at: Some(chrono::Utc::now().naive_utc()),
                            ..Default::default()
                        };
                        if repository::update_ticket_partial(&mut conn, ticket_id, assign_update).is_ok() {
                            info!(
                                ticket_id,
                                assignee = %assigned_uuid,
                                rule = %result.rule_name,
                                method = %result.method,
                                "Auto-assigned ticket on category change"
                            );

                            // Get user info for the SSE event
                            let assignee_user = repository::get_user_by_uuid(&assigned_uuid, &mut conn).ok();
                            let user_info_for_sse = assignee_user.as_ref()
                                .map(|u| crate::models::UserInfoWithAvatar {
                                    uuid: u.uuid,
                                    name: u.name.clone(),
                                    avatar_url: u.avatar_url.clone(),
                                    avatar_thumb: u.avatar_thumb.clone(),
                                });

                            // Broadcast the assignment SSE event with user info
                            broadcast_sse_simple(
                                sse_state.clone(),
                                ticket_id,
                                "ticket_updated".to_string(),
                                json!({
                                    "key": "assignee",
                                    "value": {
                                        "uuid": assigned_uuid.to_string(),
                                        "user_info": user_info_for_sse
                                    },
                                    "user_sub": "system",
                                    "auto_assigned": true,
                                    "rule_name": result.rule_name
                                }),
                            )
                            .await;

                            // Send notification to the auto-assigned user
                            if let Some(ref assignee) = assignee_user {
                                let notification_service = notification_service.clone();
                                let ticket_title = updated_ticket.title.clone();
                                let assignee_uuid = assignee.uuid;
                                let rule_name = result.rule_name.clone();

                                tokio::spawn(async move {
                                    let payload = NotificationPayload::new(
                                        NotificationTypeCode::TicketAssigned,
                                        assignee_uuid,
                                        NotificationActor {
                                            uuid: Uuid::nil(), // System actor
                                            name: "System".to_string(),
                                            avatar_thumb: None,
                                        },
                                        NotificationEntity::Ticket {
                                            id: ticket_id,
                                            title: ticket_title,
                                        },
                                    )
                                    .with_body(format!(
                                        "You have been auto-assigned to ticket #{} (Rule: {})",
                                        ticket_id, rule_name
                                    ));

                                    if let Err(e) = notification_service.notify(payload).await {
                                        warn!(error = %e, "Failed to send auto-assignment notification");
                                    }
                                });
                            }
                        }
                    }
                }
            }

            // Broadcast SSE events IMMEDIATELY after DB update for low latency
            // Don't wait for fetching complete ticket data
            for (key, value) in body.0.as_object().unwrap_or(&serde_json::Map::new()) {
                debug!(ticket_id = ticket_id, key = %key, value = ?value, "Broadcasting SSE event");
                broadcast_sse_simple(
                    sse_state.clone(),
                    ticket_id,
                    "ticket_updated".to_string(),
                    json!({
                        "key": key,
                        "value": value,
                        "user_sub": user_info.sub
                    }),
                )
                .await;
            }

            // Now fetch the complete ticket for the response
            // This happens after SSE broadcast so it doesn't delay real-time updates
            let updated_ticket = match repository::get_complete_ticket(&mut conn, ticket_id) {
                Ok(ticket) => ticket,
                Err(_) => {
                    return HttpResponse::InternalServerError()
                        .json("Failed to fetch updated ticket")
                }
            };

            // Trigger notifications for relevant changes (runs async, doesn't block response)
            if let Some(ref old) = old_ticket {
                // Get actor info for notifications
                let actor_uuid = Uuid::parse_str(&user_info.sub).ok();
                let actor = actor_uuid.and_then(|uuid| {
                    repository::get_user_by_uuid(&uuid, &mut conn).ok().map(|user| {
                        NotificationActor {
                            uuid: user.uuid,
                            name: user.name.clone(),
                            avatar_thumb: user.avatar_thumb.clone(),
                        }
                    })
                });

                if let Some(actor) = actor {
                    let notification_service = notification_service.clone();
                    let ticket_title = updated_ticket.ticket.title.clone();
                    let new_assignee = updated_ticket.ticket.assignee_uuid;
                    let old_assignee = old.assignee_uuid;
                    let new_status = updated_ticket.ticket.status.clone();
                    let old_status = old.status.clone();
                    let requester_uuid = updated_ticket.ticket.requester_uuid;
                    let actor_clone = actor.clone();

                    // Spawn async task for notifications to not block response
                    tokio::spawn(async move {
                        // Notify new assignee if assignment changed
                        if new_assignee != old_assignee {
                            if let Some(assignee_uuid) = new_assignee {
                                let payload = NotificationPayload::new(
                                    NotificationTypeCode::TicketAssigned,
                                    assignee_uuid,
                                    actor_clone.clone(),
                                    NotificationEntity::Ticket {
                                        id: ticket_id,
                                        title: ticket_title.clone(),
                                    },
                                )
                                .with_body(format!("You have been assigned to ticket #{}", ticket_id));

                                if let Err(e) = notification_service.notify(payload).await {
                                    warn!(error = %e, "Failed to send assignment notification");
                                }
                            }
                        }

                        // Notify requester if status changed to closed
                        if new_status != old_status {
                            if let Some(requester) = requester_uuid {
                                let payload = NotificationPayload::new(
                                    NotificationTypeCode::TicketStatusChanged,
                                    requester,
                                    actor_clone.clone(),
                                    NotificationEntity::Ticket {
                                        id: ticket_id,
                                        title: ticket_title.clone(),
                                    },
                                )
                                .with_body(format!(
                                    "Ticket #{} status changed to {}",
                                    ticket_id,
                                    match new_status {
                                        TicketStatus::Open => "open",
                                        TicketStatus::InProgress => "in-progress",
                                        TicketStatus::Closed => "closed",
                                    }
                                ));

                                if let Err(e) = notification_service.notify(payload).await {
                                    warn!(error = %e, "Failed to send status change notification");
                                }
                            }
                        }
                    });
                }
            }

            // Return the updated complete ticket
            HttpResponse::Ok().json(updated_ticket)
        }
        Err(e) => {
            error!(error = ?e, "Failed to update ticket");
            HttpResponse::InternalServerError().json("Failed to update ticket")
        }
    }
}

// Link tickets
pub async fn link_tickets(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can link tickets"
        }));
    }

    let (ticket_id, linked_ticket_id) = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::link_tickets(&mut conn, ticket_id, linked_ticket_id) {
        Ok(_) => {
            debug!(ticket_id = ticket_id, linked_ticket_id = linked_ticket_id, "Broadcasting SSE event for ticket linking");

            // Broadcast SSE event for ticket linking
            broadcast_sse_simple(
                sse_state.clone(),
                ticket_id,
                "ticket_linked".to_string(),
                json!({
                    "linked_ticket_id": linked_ticket_id
                }),
            )
            .await;

            HttpResponse::Ok().json(json!({"success": true}))
        }
        Err(e) => {
            error!(error = ?e, "Failed to link tickets");
            HttpResponse::InternalServerError().json("Failed to link tickets")
        }
    }
}

// Unlink tickets
pub async fn unlink_tickets(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can unlink tickets"
        }));
    }

    let (ticket_id, linked_ticket_id) = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::unlink_tickets(&mut conn, ticket_id, linked_ticket_id) {
        Ok(_) => {
            debug!(ticket_id = ticket_id, linked_ticket_id = linked_ticket_id, "Broadcasting SSE event for ticket unlinking");

            // Broadcast SSE event for ticket unlinking
            broadcast_sse_simple(
                sse_state.clone(),
                ticket_id,
                "ticket_unlinked".to_string(),
                json!({
                    "linked_ticket_id": linked_ticket_id
                }),
            )
            .await;

            HttpResponse::Ok().json(json!({"success": true}))
        }
        Err(e) => {
            error!(error = ?e, "Failed to unlink tickets");
            HttpResponse::InternalServerError().json("Failed to unlink tickets")
        }
    }
}

// Add device to ticket
pub async fn add_device_to_ticket(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can add devices to tickets"
        }));
    }

    let (ticket_id, device_id) = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::add_device_to_ticket(&mut conn, ticket_id, device_id) {
        Ok(_) => {
            debug!(ticket_id = ticket_id, device_id = device_id, "Broadcasting SSE event for device linking");

            // Broadcast SSE event for device linking
            broadcast_sse_simple(
                sse_state.clone(),
                ticket_id,
                "device_linked".to_string(),
                json!({
                    "device_id": device_id
                }),
            )
            .await;

            HttpResponse::Ok().json(json!({"success": true}))
        }
        Err(e) => {
            error!(ticket_id = ticket_id, device_id = device_id, error = ?e, "Failed to add device to ticket");
            HttpResponse::InternalServerError().json("Failed to add device to ticket")
        }
    }
}

// Remove device from ticket
pub async fn remove_device_from_ticket(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can remove devices from tickets"
        }));
    }

    let (ticket_id, device_id) = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::remove_device_from_ticket(&mut conn, ticket_id, device_id) {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                debug!(ticket_id = ticket_id, device_id = device_id, "Broadcasting SSE event for device unlinking");

                // Broadcast SSE event for device unlinking
                broadcast_sse_simple(
                    sse_state.clone(),
                    ticket_id,
                    "device_unlinked".to_string(),
                    json!({
                        "device_id": device_id
                    }),
                )
                .await;

                HttpResponse::Ok().json(json!({"success": true}))
            } else {
                HttpResponse::NotFound().json("Device not associated with ticket")
            }
        }
        Err(e) => {
            error!(ticket_id = ticket_id, device_id = device_id, error = ?e, "Failed to remove device from ticket");
            HttpResponse::InternalServerError().json("Failed to remove device from ticket")
        }
    }
}

// Get recent tickets for the authenticated user
pub async fn get_recent_tickets(
    pool: web::Data<crate::db::Pool>,
    claims: web::ReqData<Claims>,
) -> impl Responder {
    use crate::repository::user_ticket_views::UserTicketViewsRepository;

    let claims_inner = claims.into_inner();
    let user_uuid = match get_user_uuid_from_claims(&claims_inner) {
        Ok(uuid) => uuid,
        Err(e) => return e,
    };

    let repo = UserTicketViewsRepository::new(pool.get_ref().clone());

    match repo.get_recent_tickets(user_uuid, 15) {
        Ok(tickets) => HttpResponse::Ok().json(tickets),
        Err(e) => {
            error!(error = ?e, "Failed to fetch recent tickets");
            HttpResponse::InternalServerError().json("Failed to fetch recent tickets")
        }
    }
}

// Record a ticket view
pub async fn record_ticket_view(
    pool: web::Data<crate::db::Pool>,
    path: web::Path<i32>,
    claims: web::ReqData<Claims>,
) -> impl Responder {
    use crate::repository::user_ticket_views::UserTicketViewsRepository;

    let ticket_id = path.into_inner();
    let claims_inner = claims.into_inner();
    let user_uuid = match get_user_uuid_from_claims(&claims_inner) {
        Ok(uuid) => uuid,
        Err(e) => return e,
    };

    let repo = UserTicketViewsRepository::new(pool.get_ref().clone());

    match repo.record_view(user_uuid, ticket_id) {
        Ok(_) => HttpResponse::Ok().json(json!({"success": true})),
        Err(e) => {
            error!(error = ?e, "Failed to record ticket view");
            HttpResponse::InternalServerError().json("Failed to record ticket view")
        }
    }
}

// Bulk ticket operations request
#[derive(Debug, serde::Deserialize)]
pub struct BulkActionRequest {
    action: String,
    ids: Vec<i32>,
    value: Option<String>,
}

// Perform bulk operations on tickets
pub async fn bulk_tickets(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<std::sync::Arc<dyn crate::utils::storage::Storage>>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    body: web::Json<BulkActionRequest>,
) -> impl Responder {
    // Extract claims and check authentication
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    let action = body.action.as_str();
    let ids = &body.ids;

    if ids.is_empty() {
        return HttpResponse::BadRequest().json(json!({
            "error": "Bad Request",
            "message": "No ticket IDs provided"
        }));
    }

    match action {
        "delete" => {
            // Only admins can bulk delete
            if !is_admin(&claims) {
                return HttpResponse::Forbidden().json(json!({
                    "error": "Forbidden",
                    "message": "Only administrators can delete tickets"
                }));
            }

            let mut deleted = 0;
            for id in ids {
                match repository::delete_ticket_with_cleanup(&mut conn, *id, storage.as_ref().clone()).await {
                    Ok(rows) => deleted += rows,
                    Err(e) => {
                        error!(ticket_id = id, error = ?e, "Failed to delete ticket");
                    }
                }
            }

            HttpResponse::Ok().json(json!({ "affected": deleted }))
        }

        "set-status" => {
            let status_str = match &body.value {
                Some(v) => v.as_str(),
                None => return HttpResponse::BadRequest().json(json!({
                    "error": "Bad Request",
                    "message": "Status value required"
                })),
            };

            let status = match status_str {
                "open" => crate::models::TicketStatus::Open,
                "in-progress" => crate::models::TicketStatus::InProgress,
                "closed" => crate::models::TicketStatus::Closed,
                _ => return HttpResponse::BadRequest().json(json!({
                    "error": "Bad Request",
                    "message": "Invalid status value"
                })),
            };

            let mut updated = 0;
            for id in ids {
                let update = TicketUpdate {
                    title: None,
                    description: None,
                    status: Some(status.clone()),
                    priority: None,
                    requester_uuid: None,
                    assignee_uuid: None,
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    closed_at: if status == crate::models::TicketStatus::Closed {
                        Some(Some(chrono::Utc::now().naive_utc()))
                    } else {
                        None
                    },
                    category_id: None,
                };

                if repository::update_ticket_partial(&mut conn, *id, update).is_ok() {
                    updated += 1;
                    // Send SSE update
                    SseBroadcaster::broadcast_ticket_updated(
                        &sse_state,
                        *id,
                        "status",
                        json!(status_str),
                        &claims.sub,
                    ).await;
                }
            }

            HttpResponse::Ok().json(json!({ "affected": updated }))
        }

        "set-priority" => {
            let priority_str = match &body.value {
                Some(v) => v.as_str(),
                None => return HttpResponse::BadRequest().json(json!({
                    "error": "Bad Request",
                    "message": "Priority value required"
                })),
            };

            let priority = match priority_str {
                "low" => crate::models::TicketPriority::Low,
                "medium" => crate::models::TicketPriority::Medium,
                "high" => crate::models::TicketPriority::High,
                _ => return HttpResponse::BadRequest().json(json!({
                    "error": "Bad Request",
                    "message": "Invalid priority value"
                })),
            };

            let mut updated = 0;
            for id in ids {
                let update = TicketUpdate {
                    title: None,
                    description: None,
                    status: None,
                    priority: Some(priority.clone()),
                    requester_uuid: None,
                    assignee_uuid: None,
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    closed_at: None,
                    category_id: None,
                };

                if repository::update_ticket_partial(&mut conn, *id, update).is_ok() {
                    updated += 1;
                    // Send SSE update
                    SseBroadcaster::broadcast_ticket_updated(
                        &sse_state,
                        *id,
                        "priority",
                        json!(priority_str),
                        &claims.sub,
                    ).await;
                }
            }

            HttpResponse::Ok().json(json!({ "affected": updated }))
        }

        "assign" => {
            let assignee_str = match &body.value {
                Some(v) => v.as_str(),
                None => return HttpResponse::BadRequest().json(json!({
                    "error": "Bad Request",
                    "message": "Assignee value required"
                })),
            };

            let assignee_uuid = if assignee_str.is_empty() {
                None
            } else {
                match Uuid::parse_str(assignee_str) {
                    Ok(uuid) => Some(uuid),
                    Err(_) => return HttpResponse::BadRequest().json(json!({
                        "error": "Bad Request",
                        "message": "Invalid assignee UUID"
                    })),
                }
            };

            let mut updated = 0;
            for id in ids {
                let update = TicketUpdate {
                    title: None,
                    description: None,
                    status: None,
                    priority: None,
                    requester_uuid: None,
                    assignee_uuid: Some(assignee_uuid),
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                    closed_at: None,
                    category_id: None,
                };

                if repository::update_ticket_partial(&mut conn, *id, update).is_ok() {
                    updated += 1;
                    // Send SSE update
                    SseBroadcaster::broadcast_ticket_updated(
                        &sse_state,
                        *id,
                        "assignee_uuid",
                        json!(assignee_str),
                        &claims.sub,
                    ).await;
                }
            }

            HttpResponse::Ok().json(json!({ "affected": updated }))
        }

        _ => HttpResponse::BadRequest().json(json!({
            "error": "Bad Request",
            "message": format!("Unknown action: {}", action)
        })),
    }
}
