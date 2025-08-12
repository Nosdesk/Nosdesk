use actix_web::{web, HttpResponse, Responder, Result as ActixResult};
use chrono::Local;
// Removed unused import: use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use uuid::Uuid;

use crate::db;
use crate::models::{NewTicket, TicketStatus, TicketPriority, TicketUpdate, ArticleContentChunk, TicketsJson, TicketJson, UserRole};
use crate::repository;

// Helper function to validate assignee role
fn validate_assignee_role(assignee_uuid: &Uuid, conn: &mut crate::db::DbConnection) -> Result<(), HttpResponse> {
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
        },
        Err(_) => {
            Err(HttpResponse::BadRequest().json(json!({
                "error": "User not found", 
                "message": "The specified assignee does not exist"
            })))
        }
    }
}

// Helper function to parse and validate assignee from string (for update operations)
fn parse_and_validate_assignee_string(assignee_str: &str, conn: &mut crate::db::DbConnection) -> Result<Uuid, HttpResponse> {
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
            },
            Err(_) => {
                Err(HttpResponse::BadRequest().json(json!({
                    "error": "User not found",
                    "message": "The specified assignee does not exist"
                })))
            }
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
            },
            Err(_) => {
                Err(HttpResponse::BadRequest().json(json!({
                    "error": "User not found",
                    "message": "The specified assignee does not exist"
                })))
            }
        }
    }
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
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
        },
        Err(e) => {
            eprintln!("Error fetching paginated tickets: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to get paginated tickets")
        },
    }
}

// Get a ticket by ID with comments and related info
pub async fn get_ticket(
    pool: web::Data<crate::db::Pool>,
    params: web::Path<i32>,
) -> impl Responder {
    let ticket_id = params.into_inner();
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };
    
    match repository::get_complete_ticket(&mut conn, ticket_id) {
        Ok(complete_ticket) => {
            HttpResponse::Ok().json(complete_ticket)
        },
        Err(_) => HttpResponse::NotFound().json("Ticket not found"),
    }
}

// Create a new ticket
pub async fn create_ticket(
    pool: web::Data<crate::db::Pool>,
    ticket: web::Json<NewTicket>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let mut new_ticket = ticket.into_inner();

    // Validate assignee role if assignee is set
    if let Some(assignee_uuid) = new_ticket.assignee_uuid {
        if let Err(response) = validate_assignee_role(&assignee_uuid, &mut conn) {
            return response;
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let new_ticket = ticket.into_inner();

    // Validate assignee role if assignee is set
    if let Some(assignee_uuid) = new_ticket.assignee_uuid {
        if let Err(response) = validate_assignee_role(&assignee_uuid, &mut conn) {
            return response;
        }
    }

    match repository::update_ticket(&mut conn, ticket_id, new_ticket) {
        Ok(ticket) => HttpResponse::Ok().json(ticket),
        Err(e) => {
            // Just return a generic error - we can't easily check for NotFound without downcast_ref
            HttpResponse::InternalServerError().json(format!("Failed to update ticket: {}", e))
        },
    }
}

// Delete a ticket with comprehensive cleanup
pub async fn delete_ticket(
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<std::sync::Arc<dyn crate::utils::storage::Storage>>,
    path: web::Path<i32>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Use the comprehensive deletion function that cleans up files
    match repository::delete_ticket_with_cleanup(&mut conn, ticket_id, storage.as_ref().clone()).await {
        Ok(0) => HttpResponse::NotFound().json("Ticket not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
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

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
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
    auth: actix_web_httpauth::extractors::bearer::BearerAuth,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Validate token and get authenticated user's UUID
    let claims = match crate::handlers::auth::validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json("Invalid or expired token"),
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
    match repository::create_ticket(&mut conn, empty_ticket) {
        Ok(ticket) => {
            // Create empty article content for the ticket
            let new_article_content = crate::models::NewArticleContent {
                content: "".to_string(), // Empty string for content
                ticket_id: ticket.id,
            };
            
            match repository::create_article_content(&mut conn, new_article_content) {
                Ok(_) => {
                    // Return the complete ticket with article content
                    match repository::get_complete_ticket(&mut conn, ticket.id) {
                        Ok(complete_ticket) => HttpResponse::Created().json(complete_ticket),
                        Err(_) => HttpResponse::Created().json(ticket), // Fallback to just the ticket if getting complete ticket fails
                    }
                },
                Err(_) => {
                    // If article content creation fails, still return the ticket
                    HttpResponse::Created().json(ticket)
                }
            }
        },
        Err(e) => {
            println!("Error creating empty ticket: {:?}", e);
            HttpResponse::InternalServerError().json(format!("Failed to create empty ticket: {}", e))
        }
    }
}

// Update ticket partially
pub async fn update_ticket_partial(
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    auth: actix_web_httpauth::extractors::bearer::BearerAuth,
    params: web::Path<i32>,
    body: web::Json<Value>,
) -> impl Responder {
    let ticket_id = params.into_inner();
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };
    
    // Get user info for SSE events
    let user_info = match crate::handlers::auth::validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json("Invalid token"),
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
            // Get the updated ticket to include user information in SSE events
            let updated_ticket = match repository::get_complete_ticket(&mut conn, ticket_id) {
                Ok(ticket) => ticket,
                Err(_) => return HttpResponse::InternalServerError().json("Failed to fetch updated ticket"),
            };
            
            // Broadcast SSE events for each field that was updated, including user info for assignee/requester
            for (key, value) in body.as_object().unwrap_or(&serde_json::Map::new()) {
                let mut event_value = value.clone();
                
                // For assignee and requester updates, include the user information
                if key == "assignee" || key == "requester" {
                    let user_info_field = if key == "assignee" { 
                        updated_ticket.assignee_user.as_ref()
                    } else { 
                        updated_ticket.requester_user.as_ref()
                    };
                    
                    if let Some(user_info) = user_info_field {
                        event_value = serde_json::json!({
                            "uuid": value,
                            "user_info": user_info
                        });
                    }
                }
                
                println!("Broadcasting SSE event for ticket {}: {} = {:?}", ticket_id, key, event_value);
                crate::handlers::sse::broadcast_ticket_updated(
                    &sse_state,
                    ticket_id,
                    key,
                    event_value,
                    &user_info.sub,
                );
            }
            
            // Return the updated complete ticket
            HttpResponse::Ok().json(updated_ticket)
        },
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::link_tickets(&mut conn, ticket_id, linked_ticket_id) {
        Ok(_) => {
            // Broadcast SSE event for ticket linking
            println!("Broadcasting SSE event: Ticket {} linked to ticket {}", ticket_id, linked_ticket_id);
            crate::handlers::sse::broadcast_ticket_linked(&sse_state, ticket_id, linked_ticket_id);
            
            HttpResponse::Ok().json(json!({"success": true}))
        },
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::unlink_tickets(&mut conn, ticket_id, linked_ticket_id) {
        Ok(_) => {
            // Broadcast SSE event for ticket unlinking
            println!("Broadcasting SSE event: Ticket {} unlinked from ticket {}", ticket_id, linked_ticket_id);
            crate::handlers::sse::broadcast_ticket_unlinked(&sse_state, ticket_id, linked_ticket_id);
            
            HttpResponse::Ok().json(json!({"success": true}))
        },
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::add_device_to_ticket(&mut conn, ticket_id, device_id) {
        Ok(_) => {
            // Broadcast SSE event for device linking
            println!("Broadcasting SSE event: Device {} linked to ticket {}", device_id, ticket_id);
            crate::handlers::sse::broadcast_device_linked(&sse_state, ticket_id, device_id);
            
            HttpResponse::Ok().json(json!({"success": true}))
        },
        Err(e) => {
            println!("Error adding device {} to ticket {}: {:?}", device_id, ticket_id, e);
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::remove_device_from_ticket(&mut conn, ticket_id, device_id) {
        Ok(0) => HttpResponse::NotFound().json("Device not associated with ticket"),
        Ok(_) => {
            // Broadcast SSE event for device unlinking
            println!("Broadcasting SSE event: Device {} unlinked from ticket {}", device_id, ticket_id);
            crate::handlers::sse::broadcast_device_unlinked(&sse_state, ticket_id, device_id);
            
            HttpResponse::Ok().json(json!({"success": true}))
        },
        Err(e) => {
            println!("Error removing device {} from ticket {}: {:?}", device_id, ticket_id, e);
            HttpResponse::InternalServerError().json("Failed to remove device from ticket")
        }
    }
} 