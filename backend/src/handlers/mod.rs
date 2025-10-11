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
use std::sync::Arc;

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
    storage: web::Data<std::sync::Arc<dyn crate::utils::storage::Storage>>,
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
            let mut attachment_errors = Vec::new();
            
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
                            
                            // Get the file path from the URL and use storage abstraction
                            let file_path = attachment.url.trim_start_matches("/uploads/temp/");
                            let old_storage_path = format!("temp/{}", file_path);
                            let new_storage_path = format!("tickets/{}/{}", ticket_id, file_path);
                            
                            println!("Moving file from {} to {} using storage abstraction", old_storage_path, new_storage_path);
                            
                            // Use storage abstraction to move the file
                            match storage.move_file(&old_storage_path, &new_storage_path).await {
                                Ok(_) => {
                                    println!("Moved file from {} to {} using storage", old_storage_path, new_storage_path);
                                    // Update the URL to point to the new location (keep /uploads prefix for frontend compatibility)
                                    attachment.url = format!("/uploads/tickets/{}/{}", ticket_id, file_path);
                                },
                                Err(e) => {
                                    println!("Error moving file with storage: {:?}", e);
                                    // Fallback to filesystem operations if storage fails
                                    let old_fs_path = format!("uploads/{}", old_storage_path);
                                    let new_fs_path = format!("uploads/{}", new_storage_path);
                                    let new_fs_dir = format!("uploads/tickets/{}", ticket_id);
                                    
                                    // Create directory if it doesn't exist
                                    if !std::path::Path::new(&new_fs_dir).exists() {
                                        if let Err(e) = std::fs::create_dir_all(&new_fs_dir) {
                                            println!("Error creating ticket directory: {}", e);
                                        }
                                    }
                                    
                                    // Try to move the file using filesystem operations
                                    if let Err(e) = std::fs::rename(&old_fs_path, &new_fs_path) {
                                        println!("Error moving file with filesystem: {}", e);
                                        // If move fails, try to copy and then delete
                                        if let Err(e) = std::fs::copy(&old_fs_path, &new_fs_path) {
                                            println!("Error copying file: {}", e);
                                            attachment_errors.push(format!("Failed to copy file {}: {}", attachment.name, e));
                                        } else {
                                            // Try to delete the original file
                                            if let Err(e) = std::fs::remove_file(&old_fs_path) {
                                                println!("Error removing original file: {}", e);
                                            }
                                            // Update the URL to point to the new location
                                            attachment.url = format!("/uploads/tickets/{}/{}", ticket_id, file_path);
                                        }
                                    } else {
                                        // Update the URL to point to the new location
                                        attachment.url = format!("/uploads/tickets/{}/{}", ticket_id, file_path);
                                    }
                                }
                            }
                            
                            // Create updated attachment for database update
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
                                Err(e) => {
                                    println!("Error updating attachment: {}", e);
                                    attachment_errors.push(format!("Failed to update attachment {} in database: {}", attachment.name, e));
                                }
                            }
                        },
                        Err(e) => {
                            println!("Error finding attachment {}: {}", id, e);
                            attachment_errors.push(format!("Failed to find attachment ID {}: {}", id, e));
                        }
                    }
                }
            }
            
            // Log any attachment processing errors
            if !attachment_errors.is_empty() {
                println!("Warning: Some attachments had processing errors: {:?}", attachment_errors);
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
            
            // Broadcast SSE event for the new comment AFTER all file operations are complete
            // This prevents stream interruption during file processing
            println!("Broadcasting SSE event for comment {} with {} attachments", comment.id, attachments.len());
            
            // Small delay to ensure stream stability after file operations
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            
            println!("SSE: About to broadcast comment-added event for ticket {}", ticket_id);
            
            // Use the centralized SSE broadcasting utility
            use crate::utils::sse::SseBroadcaster;
            SseBroadcaster::broadcast_comment_added(&sse_state, ticket_id, response.clone()).await;
            
            println!("SSE: Successfully broadcasted comment-added event for ticket {}", ticket_id);
            
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
                // Broadcast SSE event for the deleted comment using centralized utility
                use crate::utils::sse::SseBroadcaster;
                SseBroadcaster::broadcast_comment_deleted(&sse_state, ticket_id, comment_id).await;
                
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
    pool: web::Data<crate::db::Pool>,
    storage: Arc<dyn crate::utils::storage::Storage>
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
            
            // Extract the storage path from the URL
            let storage_path = if attachment.url.starts_with("/uploads/temp/") {
                attachment.url.trim_start_matches("/uploads/").to_string()
            } else if attachment.url.starts_with("/uploads/tickets/") {
                attachment.url.trim_start_matches("/uploads/").to_string()
            } else {
                attachment.url.trim_start_matches("/uploads/").to_string()
            };
            
            println!("Attempting to delete file from storage at: {}", storage_path);
            
            // Delete the file using the storage abstraction
            match storage.delete_file(&storage_path).await {
                Ok(_) => println!("Successfully deleted file from storage: {}", storage_path),
                Err(e) => println!("Warning: Failed to delete file from storage: {:?}", e),
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

// Secure public file serving - ONLY for user avatars, banners, and thumbs
pub async fn serve_public_file(
    filename: web::Path<String>,
    req: actix_web::HttpRequest,
    storage: web::Data<Arc<dyn crate::utils::storage::Storage>>,
) -> impl Responder {
    let filename = filename.into_inner();
    
    // Determine the storage path based on the request URI
    let uri = req.uri().to_string();
    let storage_path = if uri.starts_with("/uploads/users/avatars/") {
        format!("users/avatars/{}", filename)
    } else if uri.starts_with("/uploads/users/banners/") {
        format!("users/banners/{}", filename)
    } else if uri.starts_with("/uploads/users/thumbs/") {
        format!("users/thumbs/{}", filename)
    } else {
        eprintln!("Security violation: Attempted to access non-avatar/banner/thumb file: {}", filename);
        return HttpResponse::Forbidden().finish();
    };
    
    // Serve the file using storage abstraction
    match crate::utils::storage::serve_file_from_storage(storage.as_ref().clone(), &storage_path, &req).await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Error serving public file {}: {:?}", storage_path, e);
            HttpResponse::NotFound().finish()
        }
    }
}

pub async fn serve_protected_file(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    storage: web::Data<Arc<dyn crate::utils::storage::Storage>>,
) -> impl Responder {
    let file_path = path.into_inner();
    
    // For protected files, serve using storage abstraction
    match crate::utils::storage::serve_file_from_storage(storage.as_ref().clone(), &file_path, &req).await {
        Ok(response) => response,
        Err(e) => {
            eprintln!("Error serving protected file {}: {:?}", file_path, e);
            HttpResponse::NotFound().finish()
        }
    }
}