use actix_web::{web, HttpMessage, HttpResponse};

use crate::errors::ApiError;
use actix_multipart::Multipart;
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::db::{DbConnection, Pool};
use crate::extractors::{AuthContext, ScopedStorage, TenantConn};
use crate::models::{NewAttachment, WorkspaceRole};
use crate::repository;
use crate::repository::file_access::{self, TicketFile};
use crate::repository::ticket_visibility::{self, VisibilityContext};
use crate::sync::actor::ActorContext;
use crate::sync::session;
use crate::utils::file_validation::FileValidator;
use crate::utils::storage::{Caching, Storage, WorkspaceScopedStorage};
use diesel::QueryResult;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/upload", web::post().to(crate::handlers::upload_files));
}

// Upload files using the storage abstraction
pub async fn upload_files(
    mut payload: Multipart,
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Received file upload request");

    // The insert runs through TenantConn so `app.workspace_id` is set:
    // `attachments.workspace_id` is NOT NULL and defaults from that GUC,
    // and the table FORCEs RLS. A plain pooled connection leaves the GUC
    // unset, so the default resolves to NULL and the insert fails.
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

        // Generate PDF thumbnail if applicable, beside the PDF in the
        // workspace's storage so it moves with the PDF when attached.
        let thumbnail_url = if detected_mime == "application/pdf" {
            match crate::utils::pdf::store_pdf_thumbnail(
                storage.0.as_ref(),
                &file_data,
                &stored_file.path,
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
            comment_id: None, // Not linked to a comment yet
            // Only the uploader can preview the draft or attach it to a comment.
            uploaded_by: Some(auth.user_uuid),
            transcription: transcription_text.clone(),
        };

        debug!(attachment = ?new_attachment, "Creating attachment record in database");

        // Save the attachment to the database
        match tc.run(|conn| crate::repository::create_attachment(conn, new_attachment)) {
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
// Auth: the route is wrapped with `dual_auth_middleware`. The browser loads
// these URLs directly, so the workspace comes from the file, not the request
// (see `authorize_at_owning_workspace`), and it both gates access and scopes
// storage. Whether the caller may load it is `repository::file_access`: an
// attachment through its row to its reply, a notes image by its ticket.
pub async fn serve_ticket_file(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    pool: web::Data<Pool>,
    auth: AuthContext,
    base_storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let filename = path.into_inner();

    let (workspace_id, ()) = authorize_ticket_file(&pool, &auth, TicketFile::from_path(&filename))?;
    let storage = WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace_id);

    let file_path = format!("tickets/{filename}");
    serve_or_not_found(storage, &file_path, &req, Caching::Private).await
}

// Serve temp (pre-attachment staging) files.
//
// Temp objects aren't tied to a ticket yet, but the upload created an
// `attachments` row carrying the workspace_id, so the workspace comes from that
// row and a member of another workspace gets a 404.
pub async fn serve_temp_file(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    pool: web::Data<Pool>,
    auth: AuthContext,
    base_storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let filename = path.into_inner();

    let workspace_id = authorize_temp_file_access(&pool, &auth, &filename)?;
    let storage = WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace_id);

    let file_path = format!("temp/{filename}");
    serve_or_not_found(storage, &file_path, &req, Caching::Private).await
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

/// Authorize a file the browser loads directly, from the resource that owns it.
///
/// Direct loads (img, audio, PDF.js, download links) carry the session but never
/// the `X-Nosdesk-Workspace` header, so these routes can't take a `TenantConn`:
/// it would 400 ("No workspace selected") under hosted selection. `owner` finds
/// the owning workspace on an elevated (BYPASSRLS) read, the one cross-tenant
/// step, which reveals only a workspace id. Then, as the caller pinned to that
/// workspace under RLS, the caller's membership there is read and held to the
/// same rule as every agent-app request (`admits_agent_surface`: on hosted,
/// staff seats only), and `check` gets the workspace and the caller's role and
/// returns what the handler needs, or `None` to deny. A workspace-bound
/// credential (an API token) is held to its own workspace here too.
///
/// Every denial is a 404, never a 403, so a probe can't tell a missing file
/// from one in a workspace it can't see.
pub(crate) fn authorize_at_owning_workspace<T>(
    pool: &Pool,
    auth: &AuthContext,
    owner: impl FnOnce(&mut DbConnection) -> QueryResult<Option<i32>>,
    check: impl FnOnce(&mut DbConnection, i32, WorkspaceRole) -> QueryResult<Option<T>>,
) -> Result<(i32, T), actix_web::Error> {
    authorize_located(
        pool,
        auth,
        |c| Ok(owner(c)?.map(|workspace_id| (workspace_id, ()))),
        |c, workspace_id, role, ()| check(c, workspace_id, role),
    )
}

/// [`authorize_at_owning_workspace`] for a resource whose elevated lookup finds
/// more than its workspace: `owner` returns the workspace and what it found
/// there, and `check` gets that back, so the resource is looked up once.
pub(crate) fn authorize_located<L, T>(
    pool: &Pool,
    auth: &AuthContext,
    owner: impl FnOnce(&mut DbConnection) -> QueryResult<Option<(i32, L)>>,
    check: impl FnOnce(&mut DbConnection, i32, WorkspaceRole, L) -> QueryResult<Option<T>>,
) -> Result<(i32, T), actix_web::Error> {
    let mut conn = pool.get().map_err(|e| {
        error!(error = ?e, "file access: pool acquire failed");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    let lookup_actor = ActorContext::system("file_access");
    let (workspace_id, located) = session::with_actor_bypass_context(
        &mut conn,
        &lookup_actor,
        |c| -> QueryResult<Option<(i32, L)>> {
            let Some((workspace_id, located)) = owner(c)? else {
                return Ok(None);
            };
            if let Some(bound) = auth.workspace_binding {
                if repository::workspaces::uuid_for_id(c, workspace_id)? != Some(bound) {
                    return Ok(None);
                }
            }
            Ok(Some((workspace_id, located)))
        },
    )
    .map_err(|e| {
        error!(error = ?e, "file access: workspace lookup failed");
        actix_web::error::ErrorInternalServerError("Authorization check failed")
    })?
    .ok_or_else(|| actix_web::error::ErrorNotFound("File not found"))?;

    let actor = ActorContext::user_at_workspace(auth.user_uuid, workspace_id);
    let granted = session::with_actor_context(&mut conn, &actor, |c| {
        let Some(member) = repository::workspaces::membership(c, workspace_id, auth.user_uuid)?
        else {
            return Ok(None);
        };
        if !repository::workspaces::admits_agent_surface(&member.role) {
            return Ok(None);
        }
        check(
            c,
            workspace_id,
            WorkspaceRole::from_db(&member.role),
            located,
        )
    })
    .map_err(|e| {
        error!(error = ?e, workspace_id, "file access: authorization lookup failed");
        actix_web::error::ErrorInternalServerError("Authorization check failed")
    })?
    .ok_or_else(|| actix_web::error::ErrorNotFound("File not found"))?;

    Ok((workspace_id, granted))
}

/// The caller's view of tickets in the workspace the connection is pinned to,
/// from their role there (as `authorize_at_owning_workspace` read it).
pub(crate) fn viewer_in_workspace(auth: &AuthContext, role: WorkspaceRole) -> VisibilityContext {
    VisibilityContext::new(auth.user_uuid, auth.platform_role, Some(role))
}

/// Authorize a file under `tickets/` (see `repository::file_access`): found
/// once, elevated and among the caller's workspaces, then decided under that
/// workspace's pin.
fn authorize_ticket_file(
    pool: &Pool,
    auth: &AuthContext,
    file: TicketFile,
) -> Result<(i32, ()), actix_web::Error> {
    let caller = auth.user_uuid;
    authorize_located(
        pool,
        auth,
        |c| file_access::locate(c, &file, caller),
        |c, _, role, located| {
            let vis = viewer_in_workspace(auth, role);
            Ok(file_access::can_load(c, &vis, &located)?.then_some(()))
        },
    )
}

/// Authorize access to a staging (temp) file: a draft that only its uploader may
/// load until a comment attaches it and it moves under its ticket. The owning
/// workspace comes from its `attachments` row. A PDF's server-rendered
/// thumbnail has no row of its own, so it is authorized as its PDF.
fn authorize_temp_file_access(
    pool: &Pool,
    auth: &AuthContext,
    filename: &str,
) -> Result<i32, actix_web::Error> {
    let urls: Vec<String> = std::iter::once(filename.to_string())
        .chain(crate::utils::pdf::pdf_paths_for_thumbnail(filename))
        .map(|name| format!("/uploads/temp/{name}"))
        .collect();
    let caller = auth.user_uuid;
    let (workspace_id, ()) = authorize_located(
        pool,
        auth,
        |c| repository::comments::attachment_locations(c, &urls, caller),
        |c, _, _, ids| {
            let own = repository::comments::is_own_draft_upload(c, &ids, caller)?;
            Ok(own.then_some(()))
        },
    )?;
    Ok(workspace_id)
}

/// Serve a stored object, mapping any storage error to a 404.
pub(crate) async fn serve_or_not_found(
    storage: Arc<dyn Storage>,
    file_path: &str,
    req: &actix_web::HttpRequest,
    caching: Caching,
) -> Result<HttpResponse, actix_web::Error> {
    match crate::utils::storage::serve_file_from_storage(storage, file_path, req, caching).await {
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
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    let ticket_id = path.into_inner();
    info!(
        ticket_id = ticket_id,
        "Received ticket note image upload request"
    );

    // Workspace + ticket-visibility gate, mirroring the GET sibling
    // (serve_ticket_note_image). TenantConn pins `app.workspace_id` so the
    // lookup is scoped to the caller's membership-gated workspace and a
    // cross-workspace ticket id 404s. The previous raw pooled connection left
    // the GUC cleared (RLS-zero under the NOBYPASSRLS app role) and skipped the
    // access check entirely, relying solely on RLS — which would leak under a
    // misconfigured BYPASSRLS role.
    authorize_ticket_access(&mut tc, &auth, ticket_id)?;

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
    pool: web::Data<Pool>,
    auth: AuthContext,
    base_storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let (ticket_id, filename) = path.into_inner();

    // A notes image follows its ticket (see `repository::file_access`). The
    // workspace comes from the ticket so the direct image load works without a
    // selection header (see `authorize_at_owning_workspace`).
    let (workspace_id, ()) = authorize_ticket_file(&pool, &auth, TicketFile::Note { ticket_id })?;
    let storage = WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace_id);

    // Serve from tickets/{ticket_id}/notes/ folder
    let file_path = format!("tickets/{ticket_id}/notes/{filename}");
    serve_or_not_found(storage, &file_path, &req, Caching::Private).await
}

/// Clean up temp files older than 24 hours (admin endpoint)
/// Should be called via cron job or scheduled task
pub async fn cleanup_temp_files(req: actix_web::HttpRequest) -> Result<HttpResponse, ApiError> {
    // Verify admin access
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return Err(ApiError::Unauthorized("Authentication required".into())),
    };

    if !crate::utils::rbac::is_platform_admin(&claims) {
        return Err(ApiError::Forbidden(
            "Only administrators can cleanup temp files".into(),
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
