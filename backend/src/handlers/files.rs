use actix_web::{web, HttpMessage, HttpResponse};

use crate::handlers::errors;
use actix_multipart::Multipart;
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::extractors::{AuthContext, ScopedStorage, TenantConn};
use crate::models::NewAttachment;
use crate::repository::ticket_visibility::{self, VisibilityContext};
use crate::utils::file_validation::FileValidator;
use crate::utils::storage::Storage;

// Upload files using the storage abstraction
pub async fn upload_files(
    mut payload: Multipart,
    pool: web::Data<crate::db::Pool>,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Received file upload request");

    let mut conn = pool.get().map_err(|e| {
        error!(error = ?e, "Database connection error");
        actix_web::error::ErrorInternalServerError("Database connection error")
    })?;

    let mut uploaded_attachments = Vec::new();
    let mut transcription_text: Option<String> = None;

    // Process each field in the multipart form
    while let Some(mut field) = payload.try_next().await? {
        let field_name = field.name();

        // Handle transcription field
        if field_name == "transcription" {
            // SECURITY: Limit transcription size to prevent memory exhaustion attacks
            // 64KB is more than enough for any realistic voice transcription (~10,000+ words)
            const MAX_TRANSCRIPTION_SIZE: usize = 64 * 1024;

            let mut text_data = Vec::new();
            while let Some(chunk) = field.next().await {
                let data = chunk.map_err(|e| {
                    error!(error = ?e, "Error reading transcription chunk");
                    actix_web::error::ErrorInternalServerError("Error reading transcription")
                })?;

                if text_data.len() + data.len() > MAX_TRANSCRIPTION_SIZE {
                    return Err(actix_web::error::ErrorBadRequest(
                        "Transcription too large (max 64KB)",
                    ));
                }

                text_data.extend_from_slice(&data);
            }
            if !text_data.is_empty() {
                transcription_text = Some(String::from_utf8_lossy(&text_data).to_string());
            }
            continue;
        }

        // Check if the field name is "files"
        if field_name != "files" {
            debug!(field_name = %field_name, "Skipping non-file field");
            continue;
        }

        // Get the filename from the field
        let content_disposition = field.content_disposition();
        let original_filename = content_disposition
            .get_filename()
            .ok_or_else(|| actix_web::error::ErrorBadRequest("Filename is required"))?;

        // SECURITY: Sanitize filename to prevent path traversal attacks
        let sanitized_filename = FileValidator::sanitize_filename(original_filename)
            .map_err(|e| {
                warn!(error = ?e, original_filename = %original_filename, "Filename sanitization failed");
                actix_web::error::ErrorBadRequest(format!("Invalid filename: {e}"))
            })?;

        debug!(original_filename = %original_filename, sanitized_filename = %sanitized_filename, "Processing uploaded file");

        // Read the field data with incremental size validation
        let mut file_data = Vec::new();
        let mut total_size = 0usize;

        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| {
                error!(error = ?e, "Error reading chunk");
                actix_web::error::ErrorInternalServerError("Error reading chunk")
            })?;

            // SECURITY: Validate chunk doesn't cause file to exceed max size
            // This prevents memory exhaustion attacks
            FileValidator::validate_chunk_size(total_size, data.len())?;

            total_size += data.len();
            file_data.extend_from_slice(&data);
        }

        debug!(filename = %sanitized_filename, bytes = total_size, "File data read complete");

        // SECURITY: Validate file type using magic number detection AND extension check
        // This uses a blocklist approach - blocking dangerous types while allowing most files
        let detected_mime = FileValidator::validate_file(&file_data, Some(&sanitized_filename))
            .map_err(|e| {
                warn!(error = ?e, filename = %sanitized_filename, "File validation failed");
                actix_web::error::ErrorBadRequest(format!("Invalid file: {e}"))
            })?;

        debug!(mime_type = %detected_mime, filename = %sanitized_filename, "File validated");

        // SECURITY: Compute SHA-256 checksum for file integrity verification
        use ring::digest;
        let checksum_bytes = digest::digest(&digest::SHA256, &file_data);
        let checksum = checksum_bytes
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();

        // Store the file using the storage abstraction with validated MIME type
        let stored_file = storage
            .0
            .store_file(&file_data, &sanitized_filename, &detected_mime, "temp")
            .await
            .map_err(|e| {
                error!(error = ?e, filename = %sanitized_filename, "Failed to store file");
                actix_web::error::ErrorInternalServerError("Failed to store file")
            })?;

        // Generate PDF thumbnail if applicable
        let thumbnail_url = if detected_mime == "application/pdf" {
            let storage_path =
                std::env::var("STORAGE_PATH").unwrap_or_else(|_| "uploads".to_string());
            match crate::utils::pdf::generate_and_store_pdf_thumbnail(
                &file_data,
                &stored_file.path,
                &storage_path,
            )
            .await
            {
                Ok(Some(url)) => {
                    info!(thumbnail_url = %url, filename = %sanitized_filename, "Generated PDF thumbnail");
                    Some(url)
                }
                Ok(None) => {
                    debug!(filename = %sanitized_filename, "PDF thumbnail generation not available");
                    None
                }
                Err(e) => {
                    warn!(error = %e, filename = %sanitized_filename, "Failed to generate PDF thumbnail");
                    None
                }
            }
        } else {
            None
        };

        // Create a new attachment record in the database
        let new_attachment = NewAttachment {
            url: stored_file.url.clone(),
            name: sanitized_filename.clone(),
            file_size: Some(total_size as i64),
            mime_type: Some(detected_mime.clone()),
            checksum: Some(checksum),
            comment_id: None,  // Not linked to a comment yet
            uploaded_by: None, // Will be set when attached to a comment
            transcription: transcription_text.clone(),
        };

        debug!(attachment = ?new_attachment, "Creating attachment record in database");

        // Save the attachment to the database
        match crate::repository::create_attachment(&mut conn, new_attachment) {
            Ok(attachment) => {
                let attachment_json = json!({
                    "id": attachment.id,
                    "url": stored_file.url,
                    "name": sanitized_filename,
                    "transcription": attachment.transcription,
                    "thumbnail_url": thumbnail_url
                });
                info!(attachment_id = attachment.id, filename = %sanitized_filename, "Attachment created successfully");
                uploaded_attachments.push(attachment_json);
            }
            Err(e) => {
                error!(error = ?e, "Error creating attachment record");
                return Err(actix_web::error::ErrorInternalServerError(
                    "Error creating attachment record",
                ));
            }
        }
    }

    info!(count = uploaded_attachments.len(), "File upload complete");
    Ok(HttpResponse::Ok().json(uploaded_attachments))
}

// Serve ticket attachment files.
//
// Auth + tenancy: the route is wrapped with `dual_auth_middleware`, which
// authenticates the caller (cookie or Bearer) and enforces that they're a
// member of the resolved workspace. The path is `tickets/{ticket_id}/...`,
// so we additionally require that the caller can view that ticket via
// `authorize_ticket_access` (which runs under `TenantConn`, so RLS scopes
// the check to the caller's workspace and end-user visibility applies).
pub async fn serve_ticket_file(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    let filename = path.into_inner();

    // The first path segment is the owning ticket id (moved attachments
    // live under tickets/{ticket_id}/...). A path without a numeric leading
    // segment can't be tied to a ticket for authorization, so deny it.
    let ticket_id = filename
        .split('/')
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .ok_or_else(|| actix_web::error::ErrorNotFound("File not found"))?;

    authorize_ticket_access(&mut tc, &auth, ticket_id)?;

    let file_path = format!("tickets/{filename}");
    serve_or_not_found(storage.get(), &file_path, &req).await
}

// Serve temp (pre-attachment staging) files.
//
// Temp objects aren't tied to a ticket yet, but the upload created an
// `attachments` row carrying the workspace_id. We authorize by looking that
// row up under `TenantConn`, so RLS only matches it for the workspace that
// uploaded it (a member of another workspace gets a 404).
pub async fn serve_temp_file(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    mut tc: TenantConn,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    let filename = path.into_inner();

    let public_url = format!("/uploads/temp/{filename}");
    let owned = tc
        .run(|conn| {
            use crate::schema::attachments;
            use diesel::dsl::{exists, select};
            use diesel::prelude::*;
            select(exists(
                attachments::table.filter(attachments::url.eq(&public_url)),
            ))
            .get_result::<bool>(conn)
        })
        .map_err(|e| {
            error!(error = ?e, "temp file authorization lookup failed");
            actix_web::error::ErrorInternalServerError("Authorization check failed")
        })?;
    if !owned {
        return Err(actix_web::error::ErrorNotFound("File not found"));
    }

    let file_path = format!("temp/{filename}");
    serve_or_not_found(storage.get(), &file_path, &req).await
}

/// Authorize access to a ticket's files: the caller must be able to view the
/// ticket. Run under `TenantConn`, `can_view_ticket` is RLS-scoped to the
/// caller's workspace (so a ticket in another workspace reads as "not
/// viewable") and applies end-user requester/watcher visibility. A denied
/// check returns 404 rather than 403 so cross-tenant probes can't tell a
/// missing ticket from one they simply can't see.
fn authorize_ticket_access(
    tc: &mut TenantConn,
    auth: &AuthContext,
    ticket_id: i32,
) -> Result<(), actix_web::Error> {
    let ctx = VisibilityContext::from_auth(auth);
    let allowed = tc
        .run(|conn| ticket_visibility::can_view_ticket(conn, &ctx, ticket_id))
        .map_err(|e| {
            error!(error = ?e, ticket_id, "ticket file authorization lookup failed");
            actix_web::error::ErrorInternalServerError("Authorization check failed")
        })?;
    if !allowed {
        return Err(actix_web::error::ErrorNotFound("File not found"));
    }
    Ok(())
}

/// Serve a stored object, mapping any storage error to a 404.
async fn serve_or_not_found(
    storage: Arc<dyn Storage>,
    file_path: &str,
    req: &actix_web::HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    match crate::utils::storage::serve_file_from_storage(storage, file_path, req).await {
        Ok(response) => Ok(response),
        Err(e) => {
            warn!(error = ?e, file_path = %file_path, "Error serving file");
            Err(actix_web::error::ErrorNotFound("File not found"))
        }
    }
}

/// Upload images for ticket notes (collaborative editor)
/// Images are stored in tickets/{ticket_id}/notes/ folder
pub async fn upload_ticket_note_image(
    path: web::Path<i32>,
    mut payload: Multipart,
    pool: web::Data<crate::db::Pool>,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    let ticket_id = path.into_inner();
    info!(
        ticket_id = ticket_id,
        "Received ticket note image upload request"
    );

    // Verify ticket exists
    let mut conn = pool.get().map_err(|e| {
        error!(error = ?e, "Database connection error");
        actix_web::error::ErrorInternalServerError("Database connection error")
    })?;

    // Check if ticket exists
    crate::repository::tickets::get_ticket_by_id(&mut conn, ticket_id)
        .map_err(|_| actix_web::error::ErrorNotFound("Ticket not found"))?;

    let mut uploaded_files = Vec::new();

    // Process each field in the multipart form
    while let Some(mut field) = payload.try_next().await? {
        let field_name = field.name();
        if field_name != "files" {
            debug!(field_name = %field_name, "Skipping non-file field");
            continue;
        }

        // Get the filename from the field
        let content_disposition = field.content_disposition();
        let original_filename = content_disposition
            .get_filename()
            .ok_or_else(|| actix_web::error::ErrorBadRequest("Filename is required"))?;

        // SECURITY: Sanitize filename to prevent path traversal attacks
        let sanitized_filename = FileValidator::sanitize_filename(original_filename)
            .map_err(|e| {
                warn!(error = ?e, original_filename = %original_filename, "Filename sanitization failed");
                actix_web::error::ErrorBadRequest(format!("Invalid filename: {e}"))
            })?;

        debug!(original_filename = %original_filename, sanitized_filename = %sanitized_filename, "Processing ticket note image");

        // Read the field data with incremental size validation
        let mut file_data = Vec::new();
        let mut total_size = 0usize;

        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| {
                error!(error = ?e, "Error reading chunk");
                actix_web::error::ErrorInternalServerError("Error reading chunk")
            })?;

            // SECURITY: Validate chunk doesn't cause file to exceed max size (10MB for images)
            const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;
            if total_size + data.len() > MAX_IMAGE_SIZE {
                return Err(actix_web::error::ErrorBadRequest(
                    "File too large (max 10MB)",
                ));
            }

            total_size += data.len();
            file_data.extend_from_slice(&data);
        }

        debug!(filename = %sanitized_filename, bytes = total_size, "File data read complete");

        // SECURITY: Validate file type with extension check
        let detected_mime = FileValidator::validate_file(&file_data, Some(&sanitized_filename))
            .map_err(|e| {
                warn!(error = ?e, filename = %sanitized_filename, "File validation failed");
                actix_web::error::ErrorBadRequest(format!("Invalid file: {e}"))
            })?;

        // Only allow image types for ticket note images
        if !detected_mime.starts_with("image/") {
            return Err(actix_web::error::ErrorBadRequest(
                "Only image files are allowed",
            ));
        }

        debug!(mime_type = %detected_mime, filename = %sanitized_filename, "File validated");

        // Store in tickets/{ticket_id}/notes/ folder
        let folder = format!("tickets/{ticket_id}/notes");
        let stored_file = storage
            .0
            .store_file(&file_data, &sanitized_filename, &detected_mime, &folder)
            .await
            .map_err(|e| {
                error!(error = ?e, filename = %sanitized_filename, "Failed to store file");
                actix_web::error::ErrorInternalServerError("Failed to store file")
            })?;

        info!(url = %stored_file.url, filename = %sanitized_filename, "Stored ticket note image");

        uploaded_files.push(json!({
            "url": stored_file.url,
            "name": sanitized_filename,
            "size": total_size
        }));
    }

    info!(
        ticket_id = ticket_id,
        count = uploaded_files.len(),
        "Ticket note image upload complete"
    );
    Ok(HttpResponse::Ok().json(uploaded_files))
}

/// Serve ticket note images
/// Path format: tickets/{ticket_id}/notes/{filename}
pub async fn serve_ticket_note_image(
    path: web::Path<(i32, String)>,
    req: actix_web::HttpRequest,
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    let (ticket_id, filename) = path.into_inner();

    // Same workspace + visibility gate as ticket attachments; the route
    // carries the ticket id directly.
    authorize_ticket_access(&mut tc, &auth, ticket_id)?;

    // Serve from tickets/{ticket_id}/notes/ folder
    let file_path = format!("tickets/{ticket_id}/notes/{filename}");
    serve_or_not_found(storage.get(), &file_path, &req).await
}

/// Clean up temp files older than 24 hours (admin endpoint)
/// Should be called via cron job or scheduled task
pub async fn cleanup_temp_files(req: actix_web::HttpRequest) -> actix_web::Result<HttpResponse> {
    // Verify admin access
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return Ok(errors::unauthorized("Authentication required")),
    };

    if !crate::utils::rbac::is_platform_admin(&claims) {
        return Ok(errors::forbidden(
            "Only administrators can cleanup temp files",
        ));
    }

    let storage_path = std::env::var("STORAGE_PATH").unwrap_or_else(|_| "uploads".to_string());
    let temp_dir = format!("{storage_path}/temp");
    let max_age = std::time::Duration::from_secs(24 * 60 * 60); // 24 hours

    let mut files_removed = 0;
    let mut files_checked = 0;
    let mut bytes_freed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&temp_dir) {
        for entry in entries.flatten() {
            files_checked += 1;
            let path = entry.path();

            if path.is_file() {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        if let Ok(age) = std::time::SystemTime::now().duration_since(modified) {
                            if age > max_age {
                                let size = metadata.len();
                                if let Err(e) = std::fs::remove_file(&path) {
                                    errors.push(format!("Failed to delete {path:?}: {e}"));
                                } else {
                                    files_removed += 1;
                                    bytes_freed += size;
                                    debug!(path = ?path, age_hours = age.as_secs() / 3600, "Removed stale temp file");
                                }
                            }
                        }
                    }
                }
            }
        }
    } else {
        info!(temp_dir = %temp_dir, "Temp directory does not exist or is not accessible");
    }

    info!(
        files_checked,
        files_removed,
        bytes_freed_mb = bytes_freed / (1024 * 1024),
        "Temp file cleanup completed"
    );

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "status": "success",
        "message": "Temp file cleanup completed",
        "stats": {
            "files_checked": files_checked,
            "files_removed": files_removed,
            "bytes_freed": bytes_freed,
            "bytes_freed_mb": bytes_freed / (1024 * 1024),
            "errors": errors
        }
    })))
}
