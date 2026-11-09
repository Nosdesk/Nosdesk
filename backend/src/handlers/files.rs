use actix_web::{web, HttpResponse};

use actix_multipart::Multipart;
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::db::{DbConnection, Pool};
use crate::extractors::{AuthContext, ScopedStorage, TenantConn};
use crate::models::{NewAttachment, WorkspaceRole};
use crate::repository;
use crate::repository::file_access::{self, TicketFile};
use crate::repository::ticket_visibility::{self, VisibilityContext};
use crate::services::upload_intake::{self as intake, Part, UploadPolicy};
use crate::sync::actor::ActorContext;
use crate::sync::session;
use crate::utils::storage::{Caching, Storage, WorkspaceScopedStorage};
use diesel::QueryResult;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/upload", web::post().to(crate::handlers::upload_files));
}

// Upload reply and note attachments, staged in `temp/` until a reply claims
// them. Each file is spooled to disk by the intake, which owns the cap, the
// type check and the hash, and stored from there.
pub async fn upload_files(
    req: actix_web::HttpRequest,
    payload: Multipart,
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    info!("Received file upload request");

    // The insert runs through TenantConn so `app.workspace_id` is set:
    // `attachments.workspace_id` is NOT NULL and defaults from that GUC,
    // and the table FORCEs RLS. A plain pooled connection leaves the GUC
    // unset, so the default resolves to NULL and the insert fails.
    let policy = UploadPolicy::attachments();
    let mut upload =
        intake::area(&req).multipart(intake::content_length(&req), payload, &policy)?;
    let mut uploaded_attachments = Vec::new();
    // Applies to the files after it; the composer sends it first.
    let mut transcription_text: Option<String> = None;

    while let Some(part) = upload.next().await? {
        let file = match part {
            Part::File(file) => file,
            Part::Text { name, value } => {
                if name == "transcription" && !value.is_empty() {
                    transcription_text = Some(value);
                }
                continue;
            }
        };

        let stored_file = file
            .store(storage.0.as_ref(), &policy.folder)
            .await
            .map_err(|e| {
                error!(error = ?e, filename = %file.sanitized_name, "Failed to store file");
                crate::errors::internal_error("Failed to store file")
            })?;

        // Generate PDF thumbnail if applicable, beside the PDF in the
        // workspace's storage so it moves with the PDF when attached.
        let thumbnail_url = if file.mime == "application/pdf" {
            match crate::utils::pdf::store_pdf_thumbnail(
                storage.0.as_ref(),
                file.path(),
                &stored_file.path,
            )
            .await
            {
                Ok(Some(url)) => {
                    info!(thumbnail_url = %url, filename = %file.sanitized_name, "Generated PDF thumbnail");
                    Some(url)
                }
                Ok(None) => {
                    debug!(filename = %file.sanitized_name, "PDF thumbnail generation not available");
                    None
                }
                Err(e) => {
                    warn!(error = %e, filename = %file.sanitized_name, "Failed to generate PDF thumbnail");
                    None
                }
            }
        } else {
            None
        };

        // Create a new attachment record in the database
        let new_attachment = NewAttachment {
            url: stored_file.url.clone(),
            name: file.display_name.clone(),
            file_size: Some(file.size as i64),
            mime_type: Some(file.mime.clone()),
            checksum: Some(file.sha256.clone()),
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
                    "name": attachment.name,
                    "transcription": attachment.transcription,
                    "thumbnail_url": thumbnail_url
                });
                info!(attachment_id = attachment.id, filename = %file.sanitized_name, "Attachment created successfully");
                uploaded_attachments.push(attachment_json);
            }
            Err(e) => {
                error!(error = ?e, "Error creating attachment record");
                return Err(crate::errors::internal_error(
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

    let (workspace_id, name) =
        authorize_ticket_file(&pool, &auth, TicketFile::from_path(&filename))?;
    let storage = WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace_id);

    let file_path = format!("tickets/{filename}");
    serve_named_or_not_found(storage, &file_path, &req, Caching::Private, name.as_deref()).await
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

    let (workspace_id, name) = authorize_temp_file_access(&pool, &auth, &filename)?;
    let storage = WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace_id);

    let file_path = format!("temp/{filename}");
    serve_named_or_not_found(storage, &file_path, &req, Caching::Private, name.as_deref()).await
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
            crate::errors::internal_error("Authorization check failed")
        })?;
    if !allowed {
        return Err(crate::errors::not_found_error("File not found"));
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
/// same rule as every agent-app request (`admits_agent_surface`: staff seats
/// only), and `check` gets the workspace and the caller's role and
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
        crate::errors::internal_error("Database error")
    })?;

    let lookup_actor = ActorContext::system("file_access");
    // cross-tenant: finds which workspace owns the file before any is known; the access check below
    // runs pinned to it.
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
        crate::errors::internal_error("Authorization check failed")
    })?
    .ok_or_else(|| crate::errors::not_found_error("File not found"))?;

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
        crate::errors::internal_error("Authorization check failed")
    })?
    .ok_or_else(|| crate::errors::not_found_error("File not found"))?;

    Ok((workspace_id, granted))
}

/// The caller's view of tickets in the workspace the connection is pinned to,
/// from their role there (as `authorize_at_owning_workspace` read it).
pub(crate) fn viewer_in_workspace(auth: &AuthContext, role: WorkspaceRole) -> VisibilityContext {
    VisibilityContext::new(auth.user_uuid, auth.platform_role, Some(role))
}

/// Authorize a file under `tickets/` (see `repository::file_access`): found
/// once, elevated and among the caller's workspaces, then decided under that
/// workspace's pin. Returns the workspace and, for an attachment's own file,
/// its name as uploaded.
fn authorize_ticket_file(
    pool: &Pool,
    auth: &AuthContext,
    file: TicketFile,
) -> Result<(i32, Option<String>), actix_web::Error> {
    let caller = auth.user_uuid;
    authorize_located(
        pool,
        auth,
        |c| file_access::locate(c, &file, caller),
        |c, _, role, located| {
            let vis = viewer_in_workspace(auth, role);
            Ok(file_access::can_load(c, &vis, &located)?.map(|loadable| loadable.name))
        },
    )
}

/// Authorize access to a staging (temp) file: a draft that only its uploader may
/// load until a comment attaches it and it moves under its ticket. The owning
/// workspace comes from its `attachments` row. A PDF's server-rendered
/// thumbnail has no row of its own, so it is authorized as its PDF. Returns
/// the workspace and, for the draft's own file, its name as uploaded.
fn authorize_temp_file_access(
    pool: &Pool,
    auth: &AuthContext,
    filename: &str,
) -> Result<(i32, Option<String>), actix_web::Error> {
    let urls: Vec<String> = std::iter::once(filename.to_string())
        .chain(crate::utils::pdf::pdf_paths_for_thumbnail(filename))
        .map(|name| format!("/uploads/temp/{name}"))
        .collect();
    let caller = auth.user_uuid;
    authorize_located(
        pool,
        auth,
        |c| repository::comments::attachment_locations(c, &urls, caller),
        |c, _, _, ids| {
            let own = repository::comments::own_draft_uploads(c, &ids, caller)?;
            if own.is_empty() {
                return Ok(None);
            }
            let name = own
                .into_iter()
                .find(|(url, _)| *url == urls[0])
                .map(|(_, name)| name);
            Ok(Some(name))
        },
    )
}

/// Serve a stored object, mapping any storage error to a 404.
pub(crate) async fn serve_or_not_found(
    storage: Arc<dyn Storage>,
    file_path: &str,
    req: &actix_web::HttpRequest,
    caching: Caching,
) -> Result<HttpResponse, actix_web::Error> {
    serve_named_or_not_found(storage, file_path, req, caching, None).await
}

/// [`serve_or_not_found`], named `display_name` for download when given (an
/// attachment's name as uploaded). The object is found by `file_path` alone.
pub(crate) async fn serve_named_or_not_found(
    storage: Arc<dyn Storage>,
    file_path: &str,
    req: &actix_web::HttpRequest,
    caching: Caching,
    display_name: Option<&str>,
) -> Result<HttpResponse, actix_web::Error> {
    match crate::utils::storage::serve_named_file_from_storage(
        storage,
        file_path,
        req,
        caching,
        display_name,
    )
    .await
    {
        Ok(response) => Ok(response),
        Err(e) => {
            warn!(error = ?e, file_path = %file_path, "Error serving file");
            Err(crate::errors::not_found_error("File not found"))
        }
    }
}

/// Upload images for ticket notes (collaborative editor)
/// Images are stored in tickets/{ticket_id}/notes/ folder
pub async fn upload_ticket_note_image(
    req: actix_web::HttpRequest,
    path: web::Path<i32>,
    payload: Multipart,
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
    // misconfigured BYPASSRLS role. Notes are staff-edit only, so their
    // images are too.
    if !auth.can_handle_tickets() {
        return Err(crate::errors::forbidden_error(
            "Only the helpdesk team can add images to ticket notes",
        ));
    }
    authorize_ticket_access(&mut tc, &auth, ticket_id)?;

    let policy = UploadPolicy::ticket_note_images(ticket_id);
    let mut upload =
        intake::area(&req).multipart(intake::content_length(&req), payload, &policy)?;
    let mut uploaded_files = Vec::new();

    while let Some(part) = upload.next().await? {
        let Part::File(file) = part else { continue };

        let stored_file = file
            .store(storage.0.as_ref(), &policy.folder)
            .await
            .map_err(|e| {
                error!(error = ?e, filename = %file.sanitized_name, "Failed to store file");
                crate::errors::internal_error("Failed to store file")
            })?;

        info!(url = %stored_file.url, filename = %file.sanitized_name, "Stored ticket note image");

        uploaded_files.push(json!({
            "url": stored_file.url,
            "name": file.display_name,
            "size": file.size
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
// ticket-access: authorize_ticket_file checks the note's ticket at its owning workspace
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
    let (workspace_id, _) = authorize_ticket_file(&pool, &auth, TicketFile::Note { ticket_id })?;
    let storage = WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace_id);

    // Serve from tickets/{ticket_id}/notes/ folder
    let file_path = format!("tickets/{ticket_id}/notes/{filename}");
    serve_or_not_found(storage, &file_path, &req, Caching::Private).await
}
