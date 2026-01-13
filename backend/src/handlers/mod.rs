// Reexport handlers
pub mod api_tokens;
pub mod assignment_rules;
pub mod collaboration;
pub mod auth;
pub mod users;
pub mod files;
pub mod tickets;
pub mod projects;
pub mod devices;
pub mod documentation;
pub mod auth_providers;
pub mod email;
pub mod microsoft_graph;
pub mod msgraph_integration;
pub mod sse;
pub mod password_reset;
pub mod mfa_reset;
pub mod invitation;
pub mod system;
pub mod debug;
pub mod branding;
pub mod backup;
pub mod groups;
pub mod categories;
pub mod notifications;
pub mod webhooks;
pub mod plugins;

// Import all handlers from modules
pub use auth::*;
// Export specific items from users to avoid conflicts
pub use users::{
    get_users, get_paginated_users, get_users_batch, create_user,
    get_user_by_uuid, update_user_by_uuid, delete_user, upload_user_image,
    get_user_emails, get_user_with_emails, add_user_email, update_user_email, delete_user_email,
    cleanup_stale_images,
    get_user_auth_identities, delete_user_auth_identity,
    get_user_auth_identities_by_uuid, delete_user_auth_identity_by_uuid,
    resend_invitation, bulk_users
};
pub use files::*;
// Export specific items from tickets to avoid conflicts
pub use tickets::{
    get_tickets, get_paginated_tickets, get_recent_tickets, create_ticket,
    create_empty_ticket, get_ticket, update_ticket, update_ticket_partial,
    delete_ticket, record_ticket_view, import_tickets_from_json,
    import_tickets_from_json_string, link_tickets, unlink_tickets,
    add_device_to_ticket, remove_device_from_ticket, bulk_tickets
};
pub use projects::*;
// Export specific items from devices to avoid conflicts
pub use devices::{
    get_all_devices, get_paginated_devices, get_paginated_devices_excluding,
    create_device, get_device_by_id, update_device, delete_device,
    get_user_devices, unmanage_device, bulk_devices
};
pub use documentation::*;
pub use auth_providers::*;
pub use microsoft_graph::*;
pub use msgraph_integration::{get_connection_status, get_config_validation, test_connection, sync_data, get_sync_progress_endpoint, get_active_syncs, cancel_sync_session, get_last_sync, get_entra_object_id};

// Import necessary types for placeholders
use actix_web::{web, HttpResponse, HttpMessage, Responder};
use serde_json::json;
use std::sync::Arc;
use tracing::{info, warn, error, debug};
use uuid::Uuid;

use crate::services::notifications::{
    NotificationService,
    types::{NotificationTypeCode, NotificationPayload, NotificationEntity, NotificationActor},
};

use once_cell::sync::Lazy;
use regex::Regex;

// Pre-compiled regexes for performance (compiled once, reused)
static MENTION_UUID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"@\[[^\]]+\]\(([a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})\)").unwrap()
});
static MENTION_DISPLAY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"@\[([^\]]+)\]\([a-f0-9-]+\)").unwrap()
});
static HTML_TAG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<[^>]+>").unwrap()
});
static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\s+").unwrap()
});

/// Parse @mentions from comment content
/// Returns a list of unique UUIDs mentioned
/// Supports format: @[Display Name](uuid)
fn parse_mentions(content: &str) -> Vec<Uuid> {
    let mut mentions: Vec<Uuid> = MENTION_UUID_RE
        .captures_iter(content)
        .filter_map(|cap| {
            cap.get(1)
                .and_then(|m| Uuid::parse_str(m.as_str()).ok())
        })
        .collect();

    // Remove duplicates while preserving order
    mentions.sort();
    mentions.dedup();
    mentions
}

/// Strip HTML tags and clean up text for notification previews
/// Also removes @mention syntax: @[Name](uuid) -> @Name
fn strip_html_for_preview(content: &str) -> String {
    // Convert @[Name](uuid) mentions to just @Name
    let with_clean_mentions = MENTION_DISPLAY_RE.replace_all(content, "@$1");
    // Strip HTML tags
    let without_html = HTML_TAG_RE.replace_all(&with_clean_mentions, "");
    // Normalize whitespace (collapse multiple spaces/newlines)
    let normalized = WHITESPACE_RE.replace_all(&without_html, " ");
    normalized.trim().to_string()
}

/// Truncate text for notification preview (adds "..." if truncated)
fn truncate_preview(text: &str, max_len: usize) -> String {
    if text.len() > max_len {
        format!("{}...", text.chars().take(max_len).collect::<String>())
    } else {
        text.to_string()
    }
}

// Re-export validation utilities
// pub use crate::utils::validation;

// Placeholders for handlers that haven't been implemented in dedicated modules yet

// Ticket comments and attachments
pub async fn get_comments_by_ticket_id(
    path: web::Path<i32>,
    pool: web::Data<crate::db::Pool>
) -> impl Responder {
    let ticket_id = path.into_inner();
    debug!(ticket_id, "Getting comments for ticket");

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
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
                    "user_uuid": c.comment.user_uuid.to_string(),
                    "created_at": created_at,
                    "createdAt": created_at,
                    "ticket_id": c.comment.ticket_id,
                    "attachments": c.attachments,
                    "user": c.user
                })
            }).collect();

            debug!(ticket_id, comment_count = formatted_comments.len(), "Successfully retrieved comments");
            HttpResponse::Ok().json(formatted_comments)
        },
        Err(e) => {
            error!(ticket_id, error = %e, "Error retrieving comments");
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
    notification_service: web::Data<NotificationService>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    let ticket_id = path.into_inner();
    debug!(ticket_id, "Adding comment to ticket");
    debug!(content = %comment_data.content, "Comment content");
    debug!(attachments_count = comment_data.attachments.len(), "Attachments count");

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"}));
        }
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({"error": "Authentication required"})),
    };

    // Parse the authenticated user's UUID from the JWT claims
    let user_uuid_parsed = match crate::utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({"error": "Invalid user UUID in token"})),
    };

    // Get the authenticated user's full information for notifications
    let commenter_user = match crate::repository::users::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => {
            debug!(user_name = %user.name, user_uuid = %user.uuid, "Authenticated user");
            user
        },
        Err(e) => {
            error!(user_uuid = %claims.sub, error = ?e, "Authenticated user UUID not found in database");
            return HttpResponse::InternalServerError().json(json!({"error": "User account not found"}));
        }
    };

    // Extract user info for the response
    let user_info = Some(crate::models::UserInfoWithAvatar {
        uuid: commenter_user.uuid,
        name: commenter_user.name.clone(),
        avatar_url: commenter_user.avatar_url.clone(),
        avatar_thumb: commenter_user.avatar_thumb.clone(),
    });

    // Get ticket info for notifications
    let ticket = crate::repository::get_ticket_by_id(&mut conn, ticket_id).ok();

    // Create the new comment using the authenticated user's UUID
    let new_comment = crate::models::NewComment {
        content: comment_data.content.clone(),
        user_uuid: user_uuid_parsed,  // Use the user_uuid from JWT token
        ticket_id,
    };

    // Insert the comment
    match crate::repository::comments::create_comment(&mut conn, new_comment) {
        Ok(comment) => {
            debug!(comment_id = comment.id, "Created comment");
            
            // Now associate any attachments with this comment
            let mut attachments = Vec::new();
            let mut attachment_errors = Vec::new();
            
            for attachment_data in &comment_data.attachments {
                debug!(attachment = ?attachment_data, "Processing attachment");
                // Find the existing attachment (uploaded to temp) by ID if available
                if let Some(id) = attachment_data.id {
                    debug!(attachment_id = id, "Looking up attachment");
                    match crate::repository::comments::get_attachment_by_id(&mut conn, id) {
                        Ok(mut attachment) => {
                            debug!(attachment = ?attachment, "Found attachment");
                            // Update the attachment with the comment_id
                            attachment.comment_id = Some(comment.id);
                            
                            // Get the file path from the URL and use storage abstraction
                            let file_path = attachment.url.trim_start_matches("/uploads/temp/");
                            let old_storage_path = format!("temp/{}", file_path);
                            let new_storage_path = format!("tickets/{}/{}", ticket_id, file_path);

                            debug!(from = %old_storage_path, to = %new_storage_path, "Moving file using storage abstraction");
                            
                            // Use storage abstraction to move the file
                            match storage.move_file(&old_storage_path, &new_storage_path).await {
                                Ok(_) => {
                                    debug!(from = %old_storage_path, to = %new_storage_path, "Moved file using storage");
                                    // Update the URL to point to the new location (keep /uploads prefix for frontend compatibility)
                                    attachment.url = format!("/uploads/tickets/{}/{}", ticket_id, file_path);

                                    // Also move PDF thumbnail if it exists
                                    if attachment.mime_type.as_deref() == Some("application/pdf") {
                                        let thumb_suffix = "_thumb.webp";
                                        let old_thumb_path = old_storage_path
                                            .strip_suffix(".pdf")
                                            .or_else(|| old_storage_path.strip_suffix(".PDF"))
                                            .map(|base| format!("{}{}", base, thumb_suffix));
                                        let new_thumb_path = new_storage_path
                                            .strip_suffix(".pdf")
                                            .or_else(|| new_storage_path.strip_suffix(".PDF"))
                                            .map(|base| format!("{}{}", base, thumb_suffix));

                                        if let (Some(old_thumb), Some(new_thumb)) = (old_thumb_path, new_thumb_path) {
                                            if let Err(e) = storage.move_file(&old_thumb, &new_thumb).await {
                                                debug!(error = ?e, "PDF thumbnail not found or couldn't be moved (this is OK if no thumbnail was generated)");
                                            } else {
                                                debug!(from = %old_thumb, to = %new_thumb, "Moved PDF thumbnail");
                                            }
                                        }
                                    }
                                },
                                Err(e) => {
                                    warn!(error = ?e, "Error moving file with storage, falling back to filesystem");
                                    // Fallback to filesystem operations if storage fails
                                    let old_fs_path = format!("uploads/{}", old_storage_path);
                                    let new_fs_path = format!("uploads/{}", new_storage_path);
                                    let new_fs_dir = format!("uploads/tickets/{}", ticket_id);
                                    
                                    // Create directory if it doesn't exist
                                    if !std::path::Path::new(&new_fs_dir).exists() {
                                        if let Err(e) = std::fs::create_dir_all(&new_fs_dir) {
                                            error!(error = %e, directory = %new_fs_dir, "Error creating ticket directory");
                                        }
                                    }

                                    // Try to move the file using filesystem operations
                                    if let Err(e) = std::fs::rename(&old_fs_path, &new_fs_path) {
                                        warn!(error = %e, "Error moving file with filesystem");
                                        // If move fails, try to copy and then delete
                                        if let Err(e) = std::fs::copy(&old_fs_path, &new_fs_path) {
                                            error!(error = %e, file = %attachment.name, "Error copying file");
                                            attachment_errors.push(format!("Failed to copy file {}: {}", attachment.name, e));
                                        } else {
                                            // Try to delete the original file
                                            if let Err(e) = std::fs::remove_file(&old_fs_path) {
                                                warn!(error = %e, path = %old_fs_path, "Error removing original file");
                                            }
                                            // Update the URL to point to the new location
                                            attachment.url = format!("/uploads/tickets/{}/{}", ticket_id, file_path);

                                            // Also move PDF thumbnail if it exists (filesystem fallback)
                                            if attachment.mime_type.as_deref() == Some("application/pdf") {
                                                let old_thumb = old_fs_path.replace(".pdf", "_thumb.webp").replace(".PDF", "_thumb.webp");
                                                let new_thumb = new_fs_path.replace(".pdf", "_thumb.webp").replace(".PDF", "_thumb.webp");
                                                let _ = std::fs::rename(&old_thumb, &new_thumb)
                                                    .or_else(|_| std::fs::copy(&old_thumb, &new_thumb).map(|_| ()));
                                            }
                                        }
                                    } else {
                                        // Update the URL to point to the new location
                                        attachment.url = format!("/uploads/tickets/{}/{}", ticket_id, file_path);

                                        // Also move PDF thumbnail if it exists (filesystem fallback)
                                        if attachment.mime_type.as_deref() == Some("application/pdf") {
                                            let old_thumb = old_fs_path.replace(".pdf", "_thumb.webp").replace(".PDF", "_thumb.webp");
                                            let new_thumb = new_fs_path.replace(".pdf", "_thumb.webp").replace(".PDF", "_thumb.webp");
                                            let _ = std::fs::rename(&old_thumb, &new_thumb)
                                                .or_else(|_| std::fs::copy(&old_thumb, &new_thumb).map(|_| ()));
                                        }
                                    }
                                }
                            }
                            
                            // Create updated attachment for database update
                            let updated_attachment = crate::models::NewAttachment {
                                url: attachment.url.clone(),
                                name: attachment.name.clone(),
                                file_size: attachment.file_size,
                                mime_type: attachment.mime_type.clone(),
                                checksum: attachment.checksum.clone(),
                                comment_id: Some(comment.id),
                                uploaded_by: Some(user_uuid_parsed),
                                transcription: attachment.transcription.clone(),
                            };

                            debug!(attachment_id = attachment.id, "Updating attachment in database");

                            // Fix the diesel update query
                            use diesel::prelude::*;
                            match diesel::update(crate::schema::attachments::table.find(attachment.id))
                                .set(&updated_attachment)
                                .execute(&mut conn) {
                                Ok(_) => {
                                    debug!(attachment_id = attachment.id, "Updated attachment");
                                    attachments.push(attachment);
                                },
                                Err(e) => {
                                    error!(error = %e, attachment_name = %attachment.name, "Error updating attachment");
                                    attachment_errors.push(format!("Failed to update attachment {} in database: {}", attachment.name, e));
                                }
                            }
                        },
                        Err(e) => {
                            error!(attachment_id = id, error = %e, "Error finding attachment");
                            attachment_errors.push(format!("Failed to find attachment ID {}: {}", id, e));
                        }
                    }
                }
            }

            // Log any attachment processing errors
            if !attachment_errors.is_empty() {
                warn!(errors = ?attachment_errors, "Some attachments had processing errors");
            }
            
            // Get user info
            let user = user_info;
            
            // Format the ISO timestamp correctly for JavaScript
            let created_at = comment.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
            
            // Format the data to match what TicketView.vue expects
            let response = json!({
                "id": comment.id,
                "content": comment.content,
                "user_uuid": comment.user_uuid.to_string(),
                "created_at": created_at,
                "createdAt": created_at,
                "ticket_id": comment.ticket_id,
                "attachments": attachments,
                "user": user
            });
            
            // Broadcast SSE event for the new comment AFTER all file operations are complete
            // This prevents stream interruption during file processing
            debug!(comment_id = comment.id, attachments_count = attachments.len(), "Broadcasting SSE event for comment");

            // Small delay to ensure stream stability after file operations
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

            debug!(ticket_id, "SSE: About to broadcast comment-added event");
            
            // Use the centralized SSE broadcasting utility
            use crate::utils::sse::SseBroadcaster;
            SseBroadcaster::broadcast_comment_added(&sse_state, ticket_id, response.clone()).await;

            // Also broadcast the ticket modified date update
            SseBroadcaster::broadcast_ticket_updated(
                &sse_state,
                ticket_id,
                "modified",
                serde_json::json!(chrono::Utc::now()),
                &claims.sub,
            ).await;

            debug!(ticket_id, "SSE: Successfully broadcasted comment-added and modified events");

            // Send notifications to ticket participants (requester, assignee, and @mentioned users)
            if let Some(ref ticket_info) = ticket {
                let commenter_uuid = commenter_user.uuid;
                let commenter_name = commenter_user.name.clone();
                let commenter_avatar = commenter_user.avatar_thumb.clone();
                let ticket_title = ticket_info.title.clone();
                let ticket_requester = ticket_info.requester_uuid;
                let ticket_assignee = ticket_info.assignee_uuid;
                let comment_id = comment.id;
                // Strip HTML and clean up mentions for notification preview
                let comment_preview = truncate_preview(&strip_html_for_preview(&comment_data.content), 100);

                // Parse @mentions from comment content (now extracts UUIDs directly)
                let mentioned_users: Vec<Uuid> = parse_mentions(&comment_data.content)
                    .into_iter()
                    .filter(|uuid| *uuid != commenter_uuid)
                    .collect();
                debug!(mentioned_users = ?mentioned_users, "Parsed @mentions from comment");

                let notification_service = notification_service.clone();
                tokio::spawn(async move {
                    let actor = NotificationActor {
                        uuid: commenter_uuid,
                        name: commenter_name,
                        avatar_thumb: commenter_avatar,
                    };

                    // Collect recipients for CommentAdded (requester and assignee, excluding commenter and mentioned users)
                    let mut comment_recipients = Vec::new();
                    if let Some(requester) = ticket_requester {
                        if requester != commenter_uuid && !mentioned_users.contains(&requester) {
                            comment_recipients.push(requester);
                        }
                    }
                    if let Some(assignee) = ticket_assignee {
                        if assignee != commenter_uuid && !comment_recipients.contains(&assignee) && !mentioned_users.contains(&assignee) {
                            comment_recipients.push(assignee);
                        }
                    }

                    // Send CommentAdded notification to requester/assignee
                    for recipient in comment_recipients {
                        let payload = NotificationPayload::new(
                            NotificationTypeCode::CommentAdded,
                            recipient,
                            actor.clone(),
                            NotificationEntity::Comment {
                                id: comment_id,
                                ticket_id,
                                ticket_title: ticket_title.clone(),
                            },
                        )
                        .with_body(&comment_preview);

                        if let Err(e) = notification_service.notify(payload).await {
                            warn!(error = %e, recipient = %recipient, "Failed to send comment notification");
                        }
                    }

                    // Send Mentioned notification to @mentioned users
                    for mentioned_uuid in mentioned_users {
                        let payload = NotificationPayload::new(
                            NotificationTypeCode::Mentioned,
                            mentioned_uuid,
                            actor.clone(),
                            NotificationEntity::Comment {
                                id: comment_id,
                                ticket_id,
                                ticket_title: ticket_title.clone(),
                            },
                        )
                        .with_body(&comment_preview);

                        if let Err(e) = notification_service.notify(payload).await {
                            warn!(error = %e, recipient = %mentioned_uuid, "Failed to send mention notification");
                        }
                    }
                });
            }

            info!(ticket_id, attachments_count = attachments.len(), "Successfully created comment");
            debug!(response = %response, "Returning JSON response");
            HttpResponse::Created().json(response)
        },
        Err(e) => {
            error!(error = %e, "Error creating comment");
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
    debug!(comment_id, "Deleting comment");

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
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

                info!(comment_id, "Successfully deleted comment");
                HttpResponse::Ok().json(json!({"success": true, "message": "Comment deleted"}))
            } else {
                warn!(comment_id, "Comment not found in database");
                HttpResponse::NotFound().json(json!({"error": "Comment not found"}))
            }
        },
        Err(e) => {
            error!(comment_id, error = %e, "Error deleting comment");
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
    debug!(attachment_id, "Deleting attachment");

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json(json!({"error": "Database connection error"}));
        }
    };
    
    // First, get the attachment to find the file path
    match crate::repository::comments::get_attachment_by_id(&mut conn, attachment_id) {
        Ok(attachment) => {
            debug!(attachment = ?attachment, "Found attachment");

            // Extract the storage path from the URL
            let storage_path = if attachment.url.starts_with("/uploads/temp/") {
                attachment.url.trim_start_matches("/uploads/").to_string()
            } else if attachment.url.starts_with("/uploads/tickets/") {
                attachment.url.trim_start_matches("/uploads/").to_string()
            } else {
                attachment.url.trim_start_matches("/uploads/").to_string()
            };

            debug!(storage_path = %storage_path, "Attempting to delete file from storage");

            // Delete the file using the storage abstraction
            match storage.delete_file(&storage_path).await {
                Ok(_) => debug!(storage_path = %storage_path, "Successfully deleted file from storage"),
                Err(e) => warn!(error = ?e, storage_path = %storage_path, "Failed to delete file from storage"),
            }

            // Delete the database record
            match crate::repository::comments::delete_attachment(&mut conn, attachment_id) {
                Ok(deleted) => {
                    if deleted > 0 {
                        info!(attachment_id, "Successfully deleted attachment from database");
                        HttpResponse::Ok().json(json!({"success": true, "message": "Attachment deleted"}))
                    } else {
                        warn!(attachment_id, "Attachment not found in database");
                        HttpResponse::NotFound().json(json!({"error": "Attachment not found"}))
                    }
                },
                Err(e) => {
                    error!(attachment_id, error = %e, "Error deleting attachment from database");
                    HttpResponse::InternalServerError().json(json!({"error": format!("Failed to delete attachment: {}", e)}))
                }
            }
        },
        Err(e) => {
            error!(attachment_id, error = %e, "Error finding attachment");
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
        warn!(filename = %filename, "Security violation: Attempted to access non-avatar/banner/thumb file");
        return HttpResponse::Forbidden().finish();
    };

    // Serve the file using storage abstraction
    match crate::utils::storage::serve_file_from_storage(storage.as_ref().clone(), &storage_path, &req).await {
        Ok(response) => response,
        Err(e) => {
            error!(storage_path = %storage_path, error = ?e, "Error serving public file");
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
            error!(file_path = %file_path, error = ?e, "Error serving protected file");
            HttpResponse::NotFound().finish()
        }
    }
}