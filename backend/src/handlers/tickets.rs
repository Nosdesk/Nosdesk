use actix_web::{web, HttpResponse, Responder, HttpRequest, HttpMessage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::models::{Claims, NewTicket, TicketPriority, TicketStatus, TicketUpdate, TicketsJson, UserRole};
use crate::repository;

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
            _ => eprintln!("Unknown SSE event type: {}", event_type),
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
    assignee: Option<String>,
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
        query.assignee.clone(),
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
            eprintln!("Error fetching paginated tickets: {:?}", e);
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
            eprintln!("Warning: Invalid user UUID in claims, cannot record view");
            return HttpResponse::Ok().json(complete_ticket);
        }
    };

    let view_repo = UserTicketViewsRepository::new(pool.get_ref().clone());
    if let Err(e) = view_repo.record_view(user_uuid, ticket_id) {
        eprintln!("Warning: Failed to record ticket view for user {}: {:?}", user_uuid, e);
    }

    HttpResponse::Ok().json(complete_ticket)
}

// Create a new ticket
pub async fn create_ticket(
    pool: web::Data<crate::db::Pool>,
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
        Ok(ticket) => HttpResponse::Created().json(ticket),
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
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<std::sync::Arc<dyn crate::utils::storage::Storage>>,
    path: web::Path<i32>,
) -> impl Responder {
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
    pool: web::Data<crate::db::Pool>,
    json_path: web::Path<String>,
) -> impl Responder {
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
    pool: web::Data<crate::db::Pool>,
    tickets_json: web::Json<TicketsJson>,
) -> impl Responder {
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
    };

    // Create the ticket and then add empty article content
    let ticket = match repository::create_ticket(&mut conn, empty_ticket) {
        Ok(ticket) => ticket,
        Err(e) => {
            println!("Error creating empty ticket: {:?}", e);
            return HttpResponse::InternalServerError()
                .json(format!("Failed to create empty ticket: {}", e));
        }
    };

    // Create empty article content for the ticket
    let new_article_content = crate::models::NewArticleContent {
        content: "".to_string(), // Empty string for content
        ticket_id: ticket.id,
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
                    println!("Warning: Could not find user with name '{}'", requester_str);
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

    // Update the ticket
    match repository::update_ticket_partial(&mut conn, ticket_id, ticket_update) {
        Ok(_) => {
            // Broadcast SSE events IMMEDIATELY after DB update for low latency
            // Don't wait for fetching complete ticket data
            for (key, value) in body.0.as_object().unwrap_or(&serde_json::Map::new()) {
                println!(
                    "Broadcasting SSE event for ticket {}: {} = {:?}",
                    ticket_id, key, value
                );
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

            // Return the updated complete ticket
            HttpResponse::Ok().json(updated_ticket)
        }
        Err(e) => {
            println!("Error updating ticket: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to update ticket")
        }
    }
}

// Link tickets
pub async fn link_tickets(
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let (ticket_id, linked_ticket_id) = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::link_tickets(&mut conn, ticket_id, linked_ticket_id) {
        Ok(_) => {
            // Broadcast SSE event for ticket linking
            println!(
                "Broadcasting SSE event: Ticket {} linked to ticket {}",
                ticket_id, linked_ticket_id
            );

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
            println!("Error linking tickets: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to link tickets")
        }
    }
}

// Unlink tickets
pub async fn unlink_tickets(
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let (ticket_id, linked_ticket_id) = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::unlink_tickets(&mut conn, ticket_id, linked_ticket_id) {
        Ok(_) => {
            // Broadcast SSE event for ticket unlinking
            println!(
                "Broadcasting SSE event: Ticket {} unlinked from ticket {}",
                ticket_id, linked_ticket_id
            );

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
            println!("Error unlinking tickets: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to unlink tickets")
        }
    }
}

// Add device to ticket
pub async fn add_device_to_ticket(
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let (ticket_id, device_id) = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::add_device_to_ticket(&mut conn, ticket_id, device_id) {
        Ok(_) => {
            // Broadcast SSE event for device linking
            println!(
                "Broadcasting SSE event: Device {} linked to ticket {}",
                device_id, ticket_id
            );

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
            println!(
                "Error adding device {} to ticket {}: {:?}",
                device_id, ticket_id, e
            );
            HttpResponse::InternalServerError().json("Failed to add device to ticket")
        }
    }
}

// Remove device from ticket
pub async fn remove_device_from_ticket(
    pool: web::Data<crate::db::Pool>,
    path: web::Path<(i32, i32)>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let (ticket_id, device_id) = path.into_inner();
    let mut conn = match get_db_conn(&pool).await {
        Ok(conn) => conn,
        Err(e) => return e,
    };

    match repository::remove_device_from_ticket(&mut conn, ticket_id, device_id) {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                // Broadcast SSE event for device unlinking
                println!(
                    "Broadcasting SSE event: Device {} unlinked from ticket {}",
                    device_id, ticket_id
                );

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
            println!(
                "Error removing device {} from ticket {}: {:?}",
                device_id, ticket_id, e
            );
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
            eprintln!("Error fetching recent tickets: {:?}", e);
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
            eprintln!("Error recording ticket view: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to record ticket view")
        }
    }
}
