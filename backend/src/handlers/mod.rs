// Reexport handlers
pub mod admin_workspaces;
pub mod analytics;
pub mod api_tokens;
pub mod asset_audits;
pub mod asset_kinds;
pub mod asset_usage;
pub mod assets;
pub mod assignment_rules;
pub mod audit;
pub mod audit_log;
pub mod auth;
pub mod auth_providers;
pub mod backup;
pub mod branding;
pub mod bug_reports;
pub mod canned_responses;
pub mod categories;
pub mod channels;
pub mod collaboration;
pub mod csp_reports;
pub mod cycles;
pub mod dashboard;
pub mod debug;
pub mod documentation;
pub mod documentation_collections;
pub mod email;
pub mod email_queue;
pub mod email_suppressions;
pub mod errors;
pub mod feature_flags;
pub mod files;
pub mod groups;
pub mod guest;
pub mod guest_settings;
pub mod health;
pub mod helpers;
pub mod image_proxy;
pub mod imports;
pub mod internal_workspaces;
pub mod invitation;
pub mod knowledge_gaps;
pub mod microsoft_graph;
pub mod msgraph_integration;
pub mod notifications;
pub mod passkeys;
pub mod password_reset;
pub mod plugin_collections;
pub mod plugin_events;
pub mod plugins;
pub mod projects;
pub mod resend_webhook;
pub mod rules;
pub mod saved_views;
pub mod scheduler;
pub mod search;
pub mod sla;
pub mod sse;
pub mod sync;
pub mod system;
pub mod tags;
pub mod ticket_merge;
pub mod ticket_watchers;
pub mod tickets;
pub mod users;
pub mod webhooks;
pub mod workflow_states;

// Import all handlers from modules
pub use auth::*;
// Export specific items from users to avoid conflicts
pub use files::*;
pub use users::{
    add_user_email, admin_delete_user_passkey, admin_disable_user_mfa, admin_reset_user_password,
    bulk_users, cleanup_stale_images, create_user, delete_user, delete_user_auth_identity,
    delete_user_auth_identity_by_uuid, delete_user_email, get_paginated_users,
    get_user_auth_identities, get_user_auth_identities_by_uuid, get_user_by_uuid, get_user_emails,
    get_user_security_info, get_user_with_emails, get_users, get_users_batch, purge_user_now,
    regenerate_avatar_thumbnails, resend_invitation, restore_user, update_user_by_uuid,
    update_user_email, upload_user_image,
};
// Export specific items from tickets to avoid conflicts
pub use projects::*;
pub use tickets::{
    add_device_to_ticket, bulk_tickets, create_empty_ticket, create_ticket, delete_ticket,
    get_paginated_tickets, get_recent_tickets, get_ticket, get_ticket_activity, get_tickets,
    import_tickets_from_json, import_tickets_from_json_string, link_tickets, preview_ticket_field,
    record_ticket_view, remove_device_from_ticket, remove_recent_ticket, unlink_tickets,
    update_ticket, update_ticket_partial,
};
// Export specific items from devices to avoid conflicts
pub use assets::{
    bulk_devices, create_device, delete_device, get_all_devices, get_device_by_id,
    get_paginated_devices, get_paginated_devices_excluding, get_user_devices, unmanage_device,
    update_device,
};
pub use auth_providers::*;
pub use documentation::*;
pub use microsoft_graph::*;
pub use msgraph_integration::{
    cancel_sync_session, get_active_syncs, get_config_validation, get_connection_status,
    get_entra_object_id, get_last_sync, get_sync_progress_endpoint, sync_data, test_connection,
};
pub use passkeys::{
    delete_passkey, finish_passkey_login, finish_passkey_registration, finish_passkey_setup_login,
    list_passkeys, rename_passkey, start_passkey_login, start_passkey_registration,
    start_passkey_setup_login,
};

// Import necessary types for placeholders
use actix_web::{http::StatusCode, web, HttpMessage, HttpResponse, Responder};
use serde_json::json;

use crate::utils::error_response::json_error;
use crate::utils::locale::request_locale;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::services::notifications::{
    types::{NotificationActor, NotificationEntity, NotificationPayload, NotificationTypeCode},
    NotificationService,
};
use crate::services::search::SearchService;

use once_cell::sync::Lazy;
use regex::Regex;

// Pre-compiled regexes for performance (compiled once, reused)
static MENTION_UUID_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"@\[[^\]]+\]\(([a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12})\)")
        .unwrap()
});
static MENTION_DISPLAY_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"@\[([^\]]+)\]\([a-f0-9-]+\)").unwrap());
static HTML_TAG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"<[^>]+>").unwrap());
static WHITESPACE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").unwrap());

/// Parse @mentions from comment content
/// Returns a list of unique UUIDs mentioned
/// Supports format: @[Display Name](uuid)
fn parse_mentions(content: &str) -> Vec<Uuid> {
    let mut mentions: Vec<Uuid> = MENTION_UUID_RE
        .captures_iter(content)
        .filter_map(|cap| cap.get(1).and_then(|m| Uuid::parse_str(m.as_str()).ok()))
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

// Placeholders for handlers that haven't been implemented in dedicated modules yet

// Ticket comments and attachments
pub async fn get_comments_by_ticket_id(
    access: crate::extractors::TicketAccess,
    mut tc: crate::extractors::TenantConn,
) -> impl Responder {
    let ticket_id = access.ticket_id;
    debug!(ticket_id, "Getting comments for ticket");

    match tc.run(|conn| {
        crate::repository::comments::get_comments_with_attachments_by_ticket_id(conn, ticket_id)
    }) {
        Ok(comments) => {
            // Serialize through serde so every field on
            // `CommentWithAttachments` (including `content_format`,
            // `is_internal`, `channel_metadata`, `from_address`) reaches
            // the frontend automatically — the manual JSON construction
            // we used to do here was silently dropping those fields and
            // creating drift between this endpoint and the embedded
            // comments inside `CompleteTicket`. The only enrichment we
            // still do at the boundary is the camelCase `createdAt`
            // alias the UI relies on.
            let formatted_comments: Vec<serde_json::Value> = comments
                .into_iter()
                .map(|c| {
                    let created_at = c
                        .comment
                        .created_at
                        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                        .to_string();
                    let mut value = serde_json::to_value(&c).unwrap_or_default();
                    if let serde_json::Value::Object(ref mut map) = value {
                        // Re-stamp ISO timestamps in the format the UI
                        // expects (Diesel's default goes through chrono
                        // and varies between drivers).
                        map.insert(
                            "created_at".to_string(),
                            serde_json::Value::String(created_at.clone()),
                        );
                        map.insert(
                            "createdAt".to_string(),
                            serde_json::Value::String(created_at),
                        );
                    }
                    value
                })
                .collect();

            debug!(
                ticket_id,
                comment_count = formatted_comments.len(),
                "Successfully retrieved comments"
            );
            HttpResponse::Ok().json(formatted_comments)
        }
        Err(e) => {
            error!(ticket_id, error = %e, "Error retrieving comments");
            HttpResponse::InternalServerError()
                .json(json!({"error": format!("Failed to retrieve comments: {}", e)}))
        }
    }
}

pub async fn add_comment_to_ticket(
    access: crate::extractors::TicketAccess,
    comment_data: web::Json<crate::models::NewCommentWithAttachments>,
    pool: web::Data<crate::db::Pool>,
    mut tc: crate::extractors::TenantConn,
    storage: crate::extractors::ScopedStorage,
    notification_service: web::Data<NotificationService>,
    search_service: web::Data<Arc<SearchService>>,
    email_service: web::Data<Option<Arc<crate::utils::email::EmailService>>>,
    req: actix_web::HttpRequest,
) -> impl Responder {
    let ticket_id = access.ticket_id;
    let user_uuid_parsed = access.auth.user_uuid;

    debug!(ticket_id, "Adding comment to ticket");
    debug!(content = %comment_data.content, "Comment content");
    debug!(
        attachments_count = comment_data.attachments.len(),
        "Attachments count"
    );

    // Still need the raw `Claims` for actor logging on the
    // notification path; the extractor's `auth.claims` carries
    // them but is private. Pull from request extensions, where
    // the JWT middleware deposited them; the extractor's success
    // guarantees this is present.
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return json_error(
                &request_locale(&req),
                "backend-error-auth-required",
                StatusCode::UNAUTHORIZED,
            )
        }
    };

    // Get the authenticated user's full information for notifications
    let commenter_user = match tc
        .run(|conn| crate::repository::users::get_user_by_uuid(&user_uuid_parsed, conn))
    {
        Ok(user) => {
            debug!(user_name = %user.name, user_uuid = %user.uuid, "Authenticated user");
            user
        }
        Err(e) => {
            error!(user_uuid = %claims.sub, error = ?e, "Authenticated user UUID not found in database");
            return json_error(
                &request_locale(&req),
                "backend-error-user-not-found",
                StatusCode::INTERNAL_SERVER_ERROR,
            );
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
    let ticket = tc
        .run(|conn| {
            crate::repository::get_ticket_by_id(conn, ticket_id)
                .map(Some)
                .or_else(|_| Ok::<_, diesel::result::Error>(None))
        })
        .ok()
        .flatten();

    // Create the new comment using the authenticated user's UUID
    let new_comment = crate::models::NewComment {
        content: comment_data.content.clone(),
        user_uuid: user_uuid_parsed, // Use the user_uuid from JWT token
        ticket_id,
        // Editor-driven flow today produces HTML. Older clients that
        // don't send the field rely on the `Default` impl on
        // `ContentFormat`, which is also HTML, so the wire shape is
        // backward-compatible.
        content_format: comment_data.content_format,
        // Email body parts only apply to inbound channel comments,
        // not UI-authored ones.
        ..Default::default()
    };

    // Insert the comment, attributed to the authenticated user.
    // TenantConn primes the actor and workspace GUCs around the
    // repo's own transaction, so observers and RLS both see the
    // request context.
    let create_result = tc.run(|conn| {
        crate::repository::comments::create_comment(
            conn,
            new_comment,
            Some(search_service.get_ref()),
        )
    });
    match create_result {
        Ok(comment) => {
            debug!(comment_id = comment.id, "Created comment");

            // Auto-watch on first comment. Industry default
            // (GitHub, Linear) — once a user engages with a
            // ticket they probably want to hear about replies.
            // `auto_added: true` distinguishes this implicit
            // watch from an explicit bell-toggle so a future
            // "stop auto-watching" preference can drop only the
            // implicit ones. Errors here are non-fatal: a failed
            // watch insert shouldn't fail the comment write.
            if let Err(e) = tc.run(|conn| {
                crate::repository::ticket_watchers::add_watcher(
                    conn,
                    ticket_id,
                    user_uuid_parsed,
                    true,
                )
            }) {
                debug!(error = %e, "auto-watch on comment failed (non-fatal)");
            }

            // Now associate any attachments with this comment
            let mut attachments = Vec::new();
            let mut attachment_errors = Vec::new();

            for attachment_data in &comment_data.attachments {
                debug!(attachment = ?attachment_data, "Processing attachment");
                // Find the existing attachment (uploaded to temp) by ID if available
                if let Some(id) = attachment_data.id {
                    debug!(attachment_id = id, "Looking up attachment");
                    match tc.run(|conn| crate::repository::comments::get_attachment_by_id(conn, id))
                    {
                        Ok(mut attachment) => {
                            debug!(attachment = ?attachment, "Found attachment");
                            // Update the attachment with the comment_id
                            attachment.comment_id = Some(comment.id);

                            // Get the file path from the URL and use storage abstraction
                            let file_path = attachment.url.trim_start_matches("/uploads/temp/");
                            let old_storage_path = format!("temp/{file_path}");
                            let new_storage_path = format!("tickets/{ticket_id}/{file_path}");

                            debug!(from = %old_storage_path, to = %new_storage_path, "Moving file using storage abstraction");

                            // Use storage abstraction to move the file
                            match storage
                                .0
                                .move_file(&old_storage_path, &new_storage_path)
                                .await
                            {
                                Ok(_) => {
                                    debug!(from = %old_storage_path, to = %new_storage_path, "Moved file using storage");
                                    // Update the URL to point to the new location (keep /uploads prefix for frontend compatibility)
                                    attachment.url =
                                        format!("/uploads/tickets/{ticket_id}/{file_path}");

                                    // Also move PDF thumbnail if it exists
                                    if attachment.mime_type.as_deref() == Some("application/pdf") {
                                        let thumb_suffix = "_thumb.webp";
                                        let old_thumb_path = old_storage_path
                                            .strip_suffix(".pdf")
                                            .or_else(|| old_storage_path.strip_suffix(".PDF"))
                                            .map(|base| format!("{base}{thumb_suffix}"));
                                        let new_thumb_path = new_storage_path
                                            .strip_suffix(".pdf")
                                            .or_else(|| new_storage_path.strip_suffix(".PDF"))
                                            .map(|base| format!("{base}{thumb_suffix}"));

                                        if let (Some(old_thumb), Some(new_thumb)) =
                                            (old_thumb_path, new_thumb_path)
                                        {
                                            if let Err(e) =
                                                storage.0.move_file(&old_thumb, &new_thumb).await
                                            {
                                                debug!(error = ?e, "PDF thumbnail not found or couldn't be moved (this is OK if no thumbnail was generated)");
                                            } else {
                                                debug!(from = %old_thumb, to = %new_thumb, "Moved PDF thumbnail");
                                            }
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!(error = ?e, "Error moving file with storage, falling back to filesystem");
                                    // Fallback to filesystem operations if storage fails
                                    let old_fs_path = format!("uploads/{old_storage_path}");
                                    let new_fs_path = format!("uploads/{new_storage_path}");
                                    let new_fs_dir = format!("uploads/tickets/{ticket_id}");

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
                                            attachment_errors.push(format!(
                                                "Failed to copy file {}: {}",
                                                attachment.name, e
                                            ));
                                        } else {
                                            // Try to delete the original file
                                            if let Err(e) = std::fs::remove_file(&old_fs_path) {
                                                warn!(error = %e, path = %old_fs_path, "Error removing original file");
                                            }
                                            // Update the URL to point to the new location
                                            attachment.url =
                                                format!("/uploads/tickets/{ticket_id}/{file_path}");

                                            // Also move PDF thumbnail if it exists (filesystem fallback)
                                            if attachment.mime_type.as_deref()
                                                == Some("application/pdf")
                                            {
                                                let old_thumb = old_fs_path
                                                    .replace(".pdf", "_thumb.webp")
                                                    .replace(".PDF", "_thumb.webp");
                                                let new_thumb = new_fs_path
                                                    .replace(".pdf", "_thumb.webp")
                                                    .replace(".PDF", "_thumb.webp");
                                                let _ = std::fs::rename(&old_thumb, &new_thumb)
                                                    .or_else(|_| {
                                                        std::fs::copy(&old_thumb, &new_thumb)
                                                            .map(|_| ())
                                                    });
                                            }
                                        }
                                    } else {
                                        // Update the URL to point to the new location
                                        attachment.url =
                                            format!("/uploads/tickets/{ticket_id}/{file_path}");

                                        // Also move PDF thumbnail if it exists (filesystem fallback)
                                        if attachment.mime_type.as_deref()
                                            == Some("application/pdf")
                                        {
                                            let old_thumb = old_fs_path
                                                .replace(".pdf", "_thumb.webp")
                                                .replace(".PDF", "_thumb.webp");
                                            let new_thumb = new_fs_path
                                                .replace(".pdf", "_thumb.webp")
                                                .replace(".PDF", "_thumb.webp");
                                            let _ = std::fs::rename(&old_thumb, &new_thumb)
                                                .or_else(|_| {
                                                    std::fs::copy(&old_thumb, &new_thumb)
                                                        .map(|_| ())
                                                });
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

                            debug!(
                                attachment_id = attachment.id,
                                "Updating attachment in database"
                            );

                            // Fix the diesel update query
                            let update_result = tc.run(|conn| {
                                use diesel::prelude::*;
                                diesel::update(
                                    crate::schema::attachments::table.find(attachment.id),
                                )
                                .set(&updated_attachment)
                                .execute(conn)
                            });
                            match update_result {
                                Ok(_) => {
                                    debug!(attachment_id = attachment.id, "Updated attachment");
                                    attachments.push(attachment);
                                }
                                Err(e) => {
                                    error!(error = %e, attachment_name = %attachment.name, "Error updating attachment");
                                    attachment_errors.push(format!(
                                        "Failed to update attachment {} in database: {}",
                                        attachment.name, e
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            error!(attachment_id = id, error = %e, "Error finding attachment");
                            attachment_errors
                                .push(format!("Failed to find attachment ID {id}: {e}"));
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
            let created_at = comment
                .created_at
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string();

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

            // The comment + the ticket's modified-date bump both reach
            // clients through the sync pool (the repository write emits
            // comment.created + the ticket activity bump). No discrete
            // SSE broadcast.

            // Relay the comment back through the originating channel
            // (email today, chat once those adapters exist). Item J
            // Pass 1: enqueue on the durable outbound_emails queue;
            // the worker (services::email_queue::worker) handles SMTP
            // dispatch with retry, idempotency, and crash recovery.
            //
            // Skipped silently when SMTP isn't configured — the worker
            // would just mark every row failed forever. Same gate as
            // the listener spawn in main.rs.
            if let (Some(ticket_info), Some(_email_svc)) =
                (ticket.as_ref(), email_service.get_ref().as_ref())
            {
                crate::services::channels::outbound::enqueue_for_comment(
                    ticket_info.clone(),
                    comment.clone(),
                    pool.get_ref().clone(),
                );
            }

            // Search index update is fired by the CommentCreatedObserver
            // inside `create_comment`, so no manual spawn needed.

            // Send notifications to ticket participants (requester, assignee, and @mentioned users)
            if let Some(ref ticket_info) = ticket {
                let commenter_uuid = commenter_user.uuid;
                let commenter_name = commenter_user.name.clone();
                let commenter_avatar = commenter_user.avatar_thumb.clone();
                let ticket_title = ticket_info.title.clone();
                let ticket_requester = ticket_info.requester_uuid;
                let ticket_assignee = ticket_info.assignee_uuid;
                let comment_id = comment.id;
                let comment_is_internal = comment.is_internal;
                // Strip HTML and clean up mentions for notification preview
                let comment_preview =
                    truncate_preview(&strip_html_for_preview(&comment_data.content), 100);

                // Parse @mentions from comment content (now extracts UUIDs directly)
                let mentioned_users: Vec<Uuid> = parse_mentions(&comment_data.content)
                    .into_iter()
                    .filter(|uuid| *uuid != commenter_uuid)
                    .collect();
                debug!(mentioned_users = ?mentioned_users, "Parsed @mentions from comment");

                let notification_service = notification_service.clone();

                // Collect recipients for CommentAdded.
                // Three sources, deduped + filtered to exclude the
                // commenter and anyone already getting a Mention
                // notification from this same comment:
                //   1. Requester  — original ticket reporter.
                //   2. Assignee   — currently responsible.
                //   3. Watchers   — explicit subscribers via the
                //      bell toggle, plus any past commenter who
                //      was auto-watched.
                // Watchers ship in Phase C4 — this is the
                // notification fan-out that closes the feature
                // loop ("subscribe and get notified").
                let mut comment_recipients = Vec::new();
                if let Some(requester) = ticket_requester {
                    if requester != commenter_uuid && !mentioned_users.contains(&requester) {
                        comment_recipients.push(requester);
                    }
                }
                if let Some(assignee) = ticket_assignee {
                    if assignee != commenter_uuid
                        && !comment_recipients.contains(&assignee)
                        && !mentioned_users.contains(&assignee)
                    {
                        comment_recipients.push(assignee);
                    }
                }
                // Watcher fan-out source depends on the comment's
                // visibility. For an internal note we use the
                // notify-on-internal-only variant so per-watch
                // mute-internal preferences are honoured.
                //
                // Resolve the watcher list synchronously inside the
                // request's TenantConn so the workspace GUC scopes the
                // query — RLS on `ticket_watchers` would otherwise
                // return zero rows in a spawned task that has no
                // workspace context. Failure downgrades to "no watcher
                // notifications this round" rather than blocking the
                // comment.
                let watchers: Vec<Uuid> = tc
                    .run(|conn| {
                        if comment_is_internal {
                            crate::repository::ticket_watchers::watcher_uuids_for_internal_notify(
                                conn, ticket_id,
                            )
                        } else {
                            crate::repository::ticket_watchers::watcher_uuids(conn, ticket_id)
                        }
                    })
                    .unwrap_or_default();
                for watcher in watchers {
                    if watcher == commenter_uuid {
                        continue;
                    }
                    if comment_recipients.contains(&watcher) {
                        continue;
                    }
                    if mentioned_users.contains(&watcher) {
                        continue;
                    }
                    comment_recipients.push(watcher);
                }

                // Strip non-staff recipients from internal-note
                // notifications. Without this gate a requester
                // mentioned in an internal note would receive a
                // notification (and email) about a comment they
                // can't view, leaking the existence of the note
                // and confusing the recipient. The relay layer
                // already drops the outbound email body, but the
                // notification fan-out runs independently.
                let mut mentioned_users = mentioned_users;
                if comment_is_internal {
                    let mut all_candidates: Vec<Uuid> = comment_recipients
                        .iter()
                        .chain(mentioned_users.iter())
                        .copied()
                        .collect();
                    all_candidates.sort();
                    all_candidates.dedup();

                    // Staff = platform admin OR workspace owner/admin/agent
                    // in workspace 1 (single-tenant OSS). Post-W2 replaces
                    // the legacy `users.role IN (admin, technician)` filter.
                    let staff_uuids: std::collections::HashSet<Uuid> = tc
                        .run(|conn| {
                            use crate::schema::{users, workspace_members};
                            use diesel::prelude::*;
                            users::table
                                .filter(users::uuid.eq_any(&all_candidates))
                                .filter(
                                    users::platform_role.eq("platform_admin").or(
                                        diesel::dsl::exists(
                                            workspace_members::table
                                                .filter(
                                                    workspace_members::user_uuid.eq(users::uuid),
                                                )
                                                .filter(workspace_members::workspace_id.eq(1))
                                                .filter(
                                                    workspace_members::role
                                                        .eq_any(vec!["owner", "admin", "agent"]),
                                                ),
                                        ),
                                    ),
                                )
                                .select(users::uuid)
                                .load::<Uuid>(conn)
                        })
                        .unwrap_or_default()
                        .into_iter()
                        .collect();

                    comment_recipients.retain(|u| staff_uuids.contains(u));
                    mentioned_users.retain(|u| staff_uuids.contains(u));
                }

                tokio::spawn(async move {
                    let actor = NotificationActor {
                        uuid: commenter_uuid,
                        name: commenter_name,
                        avatar_thumb: commenter_avatar,
                    };

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

            info!(
                ticket_id,
                attachments_count = attachments.len(),
                "Successfully created comment"
            );
            debug!(response = %response, "Returning JSON response");
            HttpResponse::Created().json(response)
        }
        Err(e) => {
            error!(error = %e, "Error creating comment");
            HttpResponse::InternalServerError()
                .json(json!({"error": format!("Failed to create comment: {}", e)}))
        }
    }
}

pub async fn delete_comment(
    req: actix_web::HttpRequest,
    path: web::Path<i32>,
    mut tc: crate::extractors::TenantConn,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    let comment_id = path.into_inner();
    debug!(comment_id, "Deleting comment");

    // Existence guard so a missing comment returns a localized 404
    // before we attempt the delete.
    if tc
        .run(|conn| crate::repository::comments::get_comment_by_id(conn, comment_id))
        .is_err()
    {
        return json_error(
            &request_locale(&req),
            "backend-error-comment-not-found",
            StatusCode::NOT_FOUND,
        );
    }

    let delete_result = tc.run(|conn| {
        crate::repository::comments::delete_comment(
            conn,
            comment_id,
            Some(search_service.get_ref()),
        )
    });
    match delete_result {
        Ok(deleted) => {
            if deleted > 0 {
                // Search index removal is fired by the
                // CommentDeletedObserver inside `delete_comment`. The
                // deletion reaches clients via the sync pool (the
                // repository write emits `comment.deleted`).

                info!(comment_id, "Successfully deleted comment");
                HttpResponse::Ok().json(json!({"success": true, "message": "Comment deleted"}))
            } else {
                warn!(comment_id, "Comment not found in database");
                json_error(
                    &request_locale(&req),
                    "backend-error-comment-not-found",
                    StatusCode::NOT_FOUND,
                )
            }
        }
        Err(e) => {
            error!(comment_id, error = %e, "Error deleting comment");
            HttpResponse::InternalServerError()
                .json(json!({"error": format!("Failed to delete comment: {}", e)}))
        }
    }
}

pub async fn add_attachment_to_comment(
    _: web::Path<i32>,
    _: web::Data<crate::db::Pool>,
) -> impl Responder {
    HttpResponse::Ok().json(json!({"message": "Add attachment to comment handler placeholder"}))
}

/// Serve the raw RFC-822 source for an email-derived comment as
/// `text/plain` so agents can fall back to the unparsed message
/// when the quote splitter misfires or they need to inspect
/// headers. 404 on comments that have no archived source (UI-
/// authored, chat-relayed, or pre-archive history).
///
/// Visibility is gated by the parent ticket's `can_view_ticket`
/// predicate — same primitive as `TicketAccess` but applied
/// indirectly here because the route is keyed by comment id, not
/// ticket id. Deny maps to 404 (not 403) per the AUD-001 IDOR
/// pattern so the response shape can't be used to enumerate
/// comment ids that the caller doesn't own.
///
/// The body is streamed as `text/plain; charset=utf-8` rather
/// than `message/rfc822` because the intent is human inspection
/// in a browser tab; an `.eml`-typed response would trigger a
/// download in some browsers, which is worse UX for "Show
/// original message."
pub async fn get_comment_raw_eml(
    auth: crate::extractors::AuthContext,
    path: web::Path<i32>,
    mut tc: crate::extractors::TenantConn,
    storage: crate::extractors::ScopedStorage,
) -> impl Responder {
    let comment_id = path.into_inner();

    let comment =
        match tc.run(|conn| crate::repository::comments::get_comment_by_id(conn, comment_id)) {
            Ok(c) => c,
            Err(_) => return HttpResponse::NotFound().finish(),
        };

    let vis = crate::repository::ticket_visibility::VisibilityContext::from_auth(&auth);
    match tc.run(|conn| {
        crate::repository::ticket_visibility::can_view_ticket(conn, &vis, comment.ticket_id)
    }) {
        Ok(true) => {}
        // 404 on deny (not 403): an attacker iterating comment ids
        // mustn't learn which exist on other users' tickets.
        Ok(false) | Err(_) => return HttpResponse::NotFound().finish(),
    }

    let Some(storage_path) = comment.raw_source_uri else {
        // Comment exists and is visible, but has no archived
        // source. UI-authored comments and pre-archive history
        // land here. 404 is correct — there's no resource to
        // serve under this URL.
        return HttpResponse::NotFound().finish();
    };

    match storage.0.get_file(&storage_path).await {
        Ok(bytes) => HttpResponse::Ok()
            .insert_header(("Content-Type", "text/plain; charset=utf-8"))
            .insert_header((
                "Content-Disposition",
                format!("inline; filename=\"comment-{}.eml\"", comment_id),
            ))
            // Mailbox source is immutable once written. Cache it
            // aggressively at the browser; the contents can never
            // change because we never rewrite `.eml` files. 1 hour
            // is conservative; the agent UI re-fetches on tab
            // reload anyway.
            .insert_header(("Cache-Control", "private, max-age=3600"))
            .body(bytes),
        Err(e) => {
            warn!(comment_id, error = ?e, "raw .eml fetch failed; file may have been pruned");
            HttpResponse::NotFound().finish()
        }
    }
}

pub async fn delete_attachment(
    req: actix_web::HttpRequest,
    path: web::Path<i32>,
    mut tc: crate::extractors::TenantConn,
    storage: crate::extractors::ScopedStorage,
) -> impl Responder {
    let attachment_id = path.into_inner();
    debug!(attachment_id, "Deleting attachment");

    // First, get the attachment to find the file path
    match tc.run(|conn| crate::repository::comments::get_attachment_by_id(conn, attachment_id)) {
        Ok(attachment) => {
            debug!(attachment = ?attachment, "Found attachment");

            // Extract the storage path from the URL. The branch split
            // previously logged different cases for temp vs ticket vs
            // other; both have collapsed to the same trim and there
            // is no remaining reason to fork.
            let storage_path = attachment.url.trim_start_matches("/uploads/").to_string();

            debug!(storage_path = %storage_path, "Attempting to delete file from storage");

            // Delete the file using the storage abstraction
            match storage.0.delete_file(&storage_path).await {
                Ok(_) => {
                    debug!(storage_path = %storage_path, "Successfully deleted file from storage")
                }
                Err(e) => {
                    warn!(error = ?e, storage_path = %storage_path, "Failed to delete file from storage")
                }
            }

            // Delete the database record. Actor attribution comes
            // from the request's RequestContext via TenantConn —
            // both the actor GUC and workspace GUC are set for the
            // delete inside the same transaction.
            let delete_result =
                tc.run(|conn| crate::repository::comments::delete_attachment(conn, attachment_id));
            match delete_result {
                Ok(deleted) => {
                    if deleted > 0 {
                        info!(
                            attachment_id,
                            "Successfully deleted attachment from database"
                        );
                        HttpResponse::Ok()
                            .json(json!({"success": true, "message": "Attachment deleted"}))
                    } else {
                        warn!(attachment_id, "Attachment not found in database");
                        json_error(
                            &request_locale(&req),
                            "backend-error-attachment-not-found",
                            StatusCode::NOT_FOUND,
                        )
                    }
                }
                Err(e) => {
                    error!(attachment_id, error = %e, "Error deleting attachment from database");
                    HttpResponse::InternalServerError()
                        .json(json!({"error": format!("Failed to delete attachment: {}", e)}))
                }
            }
        }
        Err(e) => {
            error!(attachment_id, error = %e, "Error finding attachment");
            json_error(
                &request_locale(&req),
                "backend-error-attachment-not-found",
                StatusCode::NOT_FOUND,
            )
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
        format!("users/avatars/{filename}")
    } else if uri.starts_with("/uploads/users/banners/") {
        format!("users/banners/{filename}")
    } else if uri.starts_with("/uploads/users/thumbs/") {
        format!("users/thumbs/{filename}")
    } else {
        warn!(filename = %filename, "Security violation: Attempted to access non-avatar/banner/thumb file");
        return HttpResponse::Forbidden().finish();
    };

    // Serve the file using storage abstraction
    match crate::utils::storage::serve_file_from_storage(
        storage.as_ref().clone(),
        &storage_path,
        &req,
    )
    .await
    {
        Ok(response) => response,
        Err(e) => {
            error!(storage_path = %storage_path, error = ?e, "Error serving public file");
            HttpResponse::NotFound().finish()
        }
    }
}

/// Deliberately reject any `/uploads/{path}` that wasn't matched by the
/// explicit public-asset routes (avatars / banners / thumbs / branding).
///
/// This path used to serve **any** object straight from storage with no
/// authentication, which let `/uploads/tickets/...`, `/uploads/temp/...`,
/// and even `/uploads/email_raw/...` be read by anyone who had (or
/// guessed) the URL, bypassing the token-validated `/api/files/*` routes
/// entirely. Tenant files are now served only through those authenticated,
/// workspace-scoped handlers; the frontend rewrites `/uploads/tickets|temp`
/// to `/api/files/...` (see `fileService.ts`). Anything else hitting this
/// catch-all is either a leaked legacy URL or probing, so it 404s.
pub async fn reject_legacy_upload_path(path: web::Path<String>) -> impl Responder {
    warn!(
        path = %path.into_inner(),
        "Rejected unauthenticated /uploads/ access; tenant files are served via /api/files"
    );
    HttpResponse::NotFound().finish()
}
