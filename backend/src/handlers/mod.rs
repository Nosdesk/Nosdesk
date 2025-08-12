// Reexport handlers
pub mod collaboration;
pub mod auth;
pub mod users;
pub mod files;
pub mod tickets;
pub mod projects;
pub mod devices;
pub mod documentation;
pub mod auth_providers;
pub mod microsoft_graph;
pub mod msgraph_integration;
pub mod sse;

// Import all handlers from modules
pub use auth::*;
pub use users::*;
pub use files::*;
pub use tickets::*;
pub use projects::*;
pub use devices::*;
pub use documentation::*;
pub use auth_providers::*;
pub use microsoft_graph::*;
pub use msgraph_integration::{get_connection_status, test_connection, sync_data, get_sync_progress_endpoint, get_active_syncs, cancel_sync_session, get_last_sync, get_entra_object_id};

// Import necessary types for placeholders
use actix_web::{web, HttpResponse, Responder};
use serde_json::json;

// Re-export validation utilities
// pub use crate::utils::validation;

// Placeholders for handlers that haven't been implemented in dedicated modules yet

// Ticket comments and attachments
pub async fn get_comments_by_ticket_id(
    path: web::Path<i32>,
    pool: web::Data<crate::db::Pool>
) -> impl Responder {
    let ticket_id = path.into_inner();
    println!("Getting comments for ticket: {}", ticket_id);
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            println!("Database connection error: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"}));
        }
    };
    
    match crate::repository::comments::get_comments_with_attachments_by_ticket_id(&mut conn, ticket_id) {
        Ok(comments) => {
            // Format the comments for the frontend
            let formatted_comments: Vec<serde_json::Value> = comments.into_iter().map(|c| {
                let created_at = c.comment.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
                json!({
                    "id": c.comment.id,
                    "content": c.comment.content,
                    "user_id": c.comment.user_id,  // Use user_id, not user_uuid
                    "created_at": created_at,
                    "createdAt": created_at,
                    "ticket_id": c.comment.ticket_id,
                    "attachments": c.attachments,
                    "user": c.user
                })
            }).collect();
            
            println!("Successfully retrieved {} comments for ticket {}", formatted_comments.len(), ticket_id);
            HttpResponse::Ok().json(formatted_comments)
        },
        Err(e) => {
            println!("Error retrieving comments for ticket {}: {}", ticket_id, e);
            HttpResponse::InternalServerError().json(json!({"error": format!("Failed to retrieve comments: {}", e)}))
        }
    }
}

pub async fn add_comment_to_ticket(
    path: web::Path<i32>,
    comment_data: web::Json<crate::models::NewCommentWithAttachments>,
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    auth: actix_web_httpauth::extractors::bearer::BearerAuth,
) -> impl Responder {
    let ticket_id = path.into_inner();
    println!("Adding comment to ticket {}", ticket_id);
    println!("Comment content: {}", comment_data.content);
    println!("Attachments count: {}", comment_data.attachments.len());
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            println!("Database connection error: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"}));
        }
    };

    // Validate JWT token and get authenticated user information (SECURE)
    let claims = match crate::handlers::auth::validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({"error": "Invalid or expired token"})),
    };

    // Parse the authenticated user's UUID from the JWT claims
    let user_uuid_parsed = match crate::utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid user UUID in token"})),
    };

    // Get the authenticated user's information
    let (user_id, user_info) = match crate::repository::users::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => {
            println!("Authenticated user: {} ({})", user.name, user.uuid);
            (user.id, Some(crate::models::UserInfo::from(user)))
        },
        Err(e) => {
            println!("Error: Authenticated user UUID '{}' not found in database: {:?}", claims.sub, e);
            return HttpResponse::InternalServerError().json(json!({"error": "User account not found"}));
        }
    };

    // Create the new comment using the authenticated user's ID
    let new_comment = crate::models::NewComment {
        content: comment_data.content.clone(),
        user_id: user_id,  // Use the user_id looked up from JWT token
        ticket_id,
    };

    // Insert the comment
    match crate::repository::comments::create_comment(&mut conn, new_comment) {
        Ok(comment) => {
            println!("Created comment with ID: {}", comment.id);
            
            // Now associate any attachments with this comment
            let mut attachments = Vec::new();
            for attachment_data in &comment_data.attachments {
                println!("Processing attachment: {:?}", attachment_data);
                // Find the existing attachment (uploaded to temp) by ID if available
                if let Some(id) = attachment_data.id {
                    println!("Looking up attachment ID {}", id);
                    match crate::repository::comments::get_attachment_by_id(&mut conn, id) {
                        Ok(mut attachment) => {
                            println!("Found attachment: {:?}", attachment);
                            // Update the attachment with the comment_id
                            attachment.comment_id = Some(comment.id);
                            
                            // Get the file path from the URL
                            let file_path = attachment.url.trim_start_matches("/uploads/temp/");
                            let old_path = format!("uploads/temp/{}", file_path);
                            let new_dir = format!("uploads/tickets/{}", ticket_id);
                            let new_path = format!("{}/{}", new_dir, file_path);
                            
                            println!("Moving file from {} to {}", old_path, new_path);
                            
                            // Create the ticket directory if it doesn't exist
                            if !std::path::Path::new(&new_dir).exists() {
                                match std::fs::create_dir_all(&new_dir) {
                                    Ok(_) => println!("Created ticket directory: {}", new_dir),
                                    Err(e) => println!("Error creating ticket directory: {}", e),
                                }
                            }
                            
                            // Move the file from temp to the ticket directory
                            match std::fs::rename(&old_path, &new_path) {
                                Ok(_) => {
                                    println!("Moved file from {} to {}", old_path, new_path);
                                    // Update the URL to point to the new location
                                    attachment.url = format!("/uploads/tickets/{}/{}", ticket_id, file_path);
                                },
                                Err(e) => {
                                    println!("Error moving file: {}", e);
                                    // If move fails, try copying instead
                                    match std::fs::copy(&old_path, &new_path) {
                                        Ok(_) => {
                                            println!("Copied file from {} to {}", old_path, new_path);
                                            attachment.url = format!("/uploads/tickets/{}/{}", ticket_id, file_path);
                                            // Try to delete the original file
                                            if let Err(e) = std::fs::remove_file(&old_path) {
                                                println!("Warning: Failed to remove original file after copy: {}", e);
                                            }
                                        },
                                        Err(e) => println!("Error copying file: {}", e),
                                    }
                                }
                            }
                            
                            // Update the attachment in the database
                            let updated_attachment = crate::models::NewAttachment {
                                url: attachment.url.clone(),
                                name: attachment.name.clone(),
                                comment_id: Some(comment.id),
                            };
                            
                            println!("Updating attachment in database: {}", attachment.id);
                            
                            // Fix the diesel update query
                            use diesel::prelude::*;
                            match diesel::update(crate::schema::attachments::table.find(attachment.id))
                                .set(&updated_attachment)
                                .execute(&mut conn) {
                                Ok(_) => {
                                    println!("Updated attachment: {}", attachment.id);
                                    attachments.push(attachment);
                                },
                                Err(e) => println!("Error updating attachment: {}", e),
                            }
                        },
                        Err(e) => println!("Error finding attachment {}: {}", id, e),
                    }
                }
            }
            
            // Get user info
            let user = user_info;
            
            // Format the ISO timestamp correctly for JavaScript
            let created_at = comment.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            
            // Format the data to match what TicketView.vue expects
            let response = json!({
                "id": comment.id,
                "content": comment.content,
                "user_uuid": user_uuid_parsed.to_string(),  // Return user_uuid for frontend compatibility
                "user_id": comment.user_id,
                "created_at": created_at,
                "createdAt": created_at,
                "ticket_id": comment.ticket_id,
                "attachments": attachments,
                "user": user
            });
            
            // Broadcast SSE event for the new comment
            crate::handlers::sse::broadcast_comment_added(&sse_state, ticket_id, response.clone());
            
            println!("Successfully created comment with {} attachments", attachments.len());
            println!("Returning JSON response: {}", response);
            HttpResponse::Created().json(response)
        },
        Err(e) => {
            println!("Error creating comment: {}", e);
            HttpResponse::InternalServerError().json(json!({"error": format!("Failed to create comment: {}", e)}))
        }
    }
}

pub async fn delete_comment(
    path: web::Path<i32>,
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    let comment_id = path.into_inner();
    println!("Deleting comment with ID: {}", comment_id);
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            println!("Database connection error: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"}));
        }
    };
    
    // Get the comment first to find the ticket_id for SSE broadcasting
    let ticket_id = match crate::repository::comments::get_comment_by_id(&mut conn, comment_id) {
        Ok(comment) => comment.ticket_id,
        Err(_) => {
            return HttpResponse::NotFound().json(json!({"error": "Comment not found"}));
        }
    };
    
    match crate::repository::comments::delete_comment(&mut conn, comment_id) {
        Ok(deleted) => {
            if deleted > 0 {
                // Broadcast SSE event for the deleted comment
                crate::handlers::sse::broadcast_comment_deleted(&sse_state, ticket_id, comment_id);
                
                println!("Successfully deleted comment");
                HttpResponse::Ok().json(json!({"success": true, "message": "Comment deleted"}))
            } else {
                println!("Comment not found in database");
                HttpResponse::NotFound().json(json!({"error": "Comment not found"}))
            }
        },
        Err(e) => {
            println!("Error deleting comment: {}", e);
            HttpResponse::InternalServerError().json(json!({"error": format!("Failed to delete comment: {}", e)}))
        }
    }
}

pub async fn add_attachment_to_comment(_: web::Path<i32>, _: web::Data<crate::db::Pool>) -> impl Responder {
    HttpResponse::Ok().json(json!({"message": "Add attachment to comment handler placeholder"}))
}

pub async fn delete_attachment(
    path: web::Path<i32>,
    pool: web::Data<crate::db::Pool>
) -> impl Responder {
    let attachment_id = path.into_inner();
    println!("Deleting attachment with ID: {}", attachment_id);
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            println!("Database connection error: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"}));
        }
    };
    
    // First, get the attachment to find the file path
    match crate::repository::comments::get_attachment_by_id(&mut conn, attachment_id) {
        Ok(attachment) => {
            println!("Found attachment: {:?}", attachment);
            
            // Extract the file path
            let file_path = if attachment.url.starts_with("/uploads/temp/") {
                format!("uploads/temp/{}", attachment.url.trim_start_matches("/uploads/temp/"))
            } else if attachment.url.starts_with("/uploads/tickets/") {
                format!("uploads{}", attachment.url)
            } else {
                format!("uploads{}", attachment.url)
            };
            
            println!("Attempting to delete file at: {}", file_path);
            
            // Try to delete the file
            if std::path::Path::new(&file_path).exists() {
                match std::fs::remove_file(&file_path) {
                    Ok(_) => println!("Successfully deleted file: {}", file_path),
                    Err(e) => println!("Warning: Failed to delete file: {}", e),
                }
            } else {
                println!("File does not exist: {}", file_path);
            }
            
            // Delete the database record
            match crate::repository::comments::delete_attachment(&mut conn, attachment_id) {
                Ok(deleted) => {
                    if deleted > 0 {
                        println!("Successfully deleted attachment from database");
                        HttpResponse::Ok().json(json!({"success": true, "message": "Attachment deleted"}))
                    } else {
                        println!("Attachment not found in database");
                        HttpResponse::NotFound().json(json!({"error": "Attachment not found"}))
                    }
                },
                Err(e) => {
                    println!("Error deleting attachment from database: {}", e);
                    HttpResponse::InternalServerError().json(json!({"error": format!("Failed to delete attachment: {}", e)}))
                }
            }
        },
        Err(e) => {
            println!("Error finding attachment {}: {}", attachment_id, e);
            HttpResponse::NotFound().json(json!({"error": "Attachment not found"}))
        }
    }
}