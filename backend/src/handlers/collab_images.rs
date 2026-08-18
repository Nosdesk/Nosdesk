//! Images pasted or dropped into a collaborative document.
//!
//! Generic over the three collab resource kinds, keyed on the
//! workspace-namespaced doc_id the editor already holds
//! (`ws-{workspace_uuid}_{kind}-{resource_uuid}`). Documentation pages and
//! collection descriptions previously had no upload route at all: their
//! editor fell through to `POST /api/upload`, which stages into `temp/` and
//! reports a `/uploads/...` URL that `reject_legacy_upload_path` 404s. That
//! staging folder was also the wrong home, because `authorize_temp_file_access`
//! gates on workspace membership alone with no per-document check.
//!
//! The gate here is `can_access_document`, the same primitive the collab
//! WebSocket upgrade and the REST article read use, so "who may see this
//! image" cannot drift from "who may open this document".

use actix_web::{web, HttpResponse};

use actix_multipart::Multipart;
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use std::sync::Arc;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::db::Pool;
use crate::extractors::{AuthContext, ScopedStorage, TenantConn};
use crate::handlers::collaboration::{
    can_access_document, DocAccessor, DocKind, DocumentType, ParsedDocId,
};
use crate::handlers::errors;
use crate::handlers::files::serve_or_not_found;
use crate::sync::actor::ActorContext;
use crate::sync::session;
use crate::utils::file_validation::FileValidator;
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

/// Editor images are capped well below the generic attachment limit: they are
/// inline document content, not file attachments.
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/documents/{doc_id}/images",
        web::post().to(upload_collab_document_image),
    );
}

/// Storage folder for a document's images. The URL suffix after
/// `/api/files/collab/` is byte-identical to this logical key, so the serve
/// route reconstructs exactly what the upload wrote. Change one and you must
/// change the other.
fn image_folder(kind: DocKind, resource_uuid: Uuid) -> String {
    format!("collab/{}/{resource_uuid}", kind.url_token())
}

/// `POST /api/documents/{doc_id}/images`
///
/// Runs on `TenantConn`: this request goes through the axios client, so the
/// workspace selection header is present. The response carries the final
/// `/api/files/collab/...` URL. It must NOT return `stored_file.url`, which is
/// the logical `/uploads/...` form that `reject_legacy_upload_path` 404s.
/// Handing the editor an unreachable src is the exact bug this route fixes.
pub async fn upload_collab_document_image(
    path: web::Path<String>,
    mut payload: Multipart,
    mut tc: TenantConn,
    auth: AuthContext,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    let doc_id = path.into_inner();

    let parsed: ParsedDocId = match DocumentType::from_namespaced_doc_id(&doc_id) {
        Ok(p) => p,
        Err(e) => {
            warn!(doc_id = %doc_id, error = ?e, "Invalid document ID format on image upload");
            return Ok(errors::bad_request(
                "doc_id must be in the workspace-namespaced format ws-{uuid}_{kind}-{uuid}",
            ));
        }
    };

    // Gate BEFORE reading the body so an unauthorized caller cannot make us
    // buffer 10MB. `resolve` runs RLS-scoped, so a doc_id naming another
    // workspace's resource simply does not resolve: that is the cross-workspace
    // guard, and it lands on 404 rather than the 403 an explicit uuid
    // comparison would give.
    let accessor = DocAccessor::from_auth(&auth);
    let gate = tc.run(|conn| {
        let Some(doc_type) = parsed.resolve(conn)? else {
            return Ok(None);
        };
        Ok(Some(can_access_document(conn, &accessor, &doc_type)?))
    });
    match gate {
        Ok(Some(true)) => {}
        Ok(Some(false)) | Ok(None) => {
            return Err(actix_web::error::ErrorNotFound("Document not found"));
        }
        Err(e) => {
            error!(doc_id = %doc_id, error = ?e, "Document access check failed on image upload");
            return Err(actix_web::error::ErrorInternalServerError(
                "Failed to check document access",
            ));
        }
    }

    let folder = image_folder(parsed.kind, parsed.resource_uuid);
    let mut uploaded_files = Vec::new();

    while let Some(mut field) = payload.try_next().await? {
        if field.name() != "files" {
            debug!(field_name = %field.name(), "Skipping non-file field");
            continue;
        }

        let original_filename = field
            .content_disposition()
            .get_filename()
            .ok_or_else(|| actix_web::error::ErrorBadRequest("Filename is required"))?
            .to_string();

        let sanitized_filename = FileValidator::sanitize_filename(&original_filename)
            .map_err(|e| {
                warn!(error = ?e, original_filename = %original_filename, "Filename sanitization failed");
                actix_web::error::ErrorBadRequest(format!("Invalid filename: {e}"))
            })?;

        let mut file_data = Vec::new();
        let mut total_size = 0usize;
        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| {
                error!(error = ?e, "Error reading chunk");
                actix_web::error::ErrorInternalServerError("Error reading chunk")
            })?;
            if total_size + data.len() > MAX_IMAGE_SIZE {
                return Err(actix_web::error::ErrorBadRequest(
                    "File too large (max 10MB)",
                ));
            }
            total_size += data.len();
            file_data.extend_from_slice(&data);
        }

        let detected_mime = FileValidator::validate_file(&file_data, Some(&sanitized_filename))
            .map_err(|e| {
                warn!(error = ?e, filename = %sanitized_filename, "File validation failed");
                actix_web::error::ErrorBadRequest(format!("Invalid file: {e}"))
            })?;

        if !detected_mime.starts_with("image/") {
            return Err(actix_web::error::ErrorBadRequest(
                "Only image files are allowed",
            ));
        }

        let stored = storage
            .0
            .store_file(&file_data, &sanitized_filename, &detected_mime, &folder)
            .await
            .map_err(|e| {
                error!(error = ?e, filename = %sanitized_filename, "Failed to store file");
                actix_web::error::ErrorInternalServerError("Failed to store file")
            })?;

        // `stored.id` is the unique `{uuid7}_{filename}` basename, which is the
        // last segment of the logical path the serve route reconstructs.
        let url = format!(
            "/api/files/collab/{}/{}/{}",
            parsed.kind.url_token(),
            parsed.resource_uuid,
            stored.id
        );
        info!(url = %url, filename = %sanitized_filename, "Stored collab document image");

        uploaded_files.push(json!({
            "url": url,
            "name": sanitized_filename,
            "size": total_size,
        }));
    }

    Ok(HttpResponse::Ok().json(uploaded_files))
}

/// `GET /api/files/collab/{kind}/{resource_uuid}/{filename}`
///
/// The browser loads this straight from an `<img>` tag, which bypasses the
/// axios interceptor carrying `X-Nosdesk-Workspace`, so this route cannot take
/// a `TenantConn` (it would 400 with "No workspace selected" under hosted
/// selection). The workspace is derived from the resource instead, exactly
/// like `authorize_ticket_file_access`.
///
/// The path segments are taken as plain strings so a malformed uuid or an
/// unknown kind answers 404 like everything else in this scope, rather than
/// leaking a 400 that tells a prober its guess was well formed.
pub async fn serve_collab_document_image(
    path: web::Path<(String, String, String)>,
    req: actix_web::HttpRequest,
    pool: web::Data<Pool>,
    auth: AuthContext,
    base_storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let (kind_token, uuid_str, filename) = path.into_inner();

    let kind = DocKind::from_url_token(&kind_token)
        .ok_or_else(|| actix_web::error::ErrorNotFound("File not found"))?;
    let resource_uuid = Uuid::parse_str(&uuid_str)
        .map_err(|_| actix_web::error::ErrorNotFound("File not found"))?;

    let workspace_id = authorize_collab_file_access(&pool, &auth, kind, resource_uuid)?;
    let storage = WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace_id);

    let file_path = format!("{}/{filename}", image_folder(kind, resource_uuid));
    // `serve_file_from_storage` runs `is_safe_storage_path` first, so the
    // greedy `{filename:.*}` tail cannot traverse out of the folder.
    serve_or_not_found(storage, &file_path, &req).await
}

/// Authorize access to a collaborative document's images, deriving the
/// workspace from the resource rather than from the request's selection.
///
/// The direct `<img>` load carries no `X-Nosdesk-Workspace`, so we look the
/// owning workspace up unscoped (the only cross-tenant step, and it reveals
/// only a workspace id), then gate on the caller's membership plus the
/// per-document ACL IN THAT WORKSPACE under RLS. Same shape as
/// `authorize_ticket_file_access`, generalised over the three collab kinds.
///
/// Security is unchanged from the selection path: "having selected the
/// workspace" was never a control, membership and the document gate are, and
/// both still apply. Every denial is a 404, never a 403, so a probe cannot
/// tell a missing image from one it may not see.
fn authorize_collab_file_access(
    pool: &Pool,
    auth: &AuthContext,
    kind: DocKind,
    resource_uuid: Uuid,
) -> Result<i32, actix_web::Error> {
    let mut conn = pool.get().map_err(|e| {
        error!(error = ?e, "collab file access: pool acquire failed");
        actix_web::error::ErrorInternalServerError("Database error")
    })?;

    // Which workspace owns this resource? Unscoped read (BYPASSRLS); reveals
    // only the workspace id, access is gated below.
    let lookup_actor = ActorContext::system("collab_file_access");
    let workspace_id =
        session::with_actor_bypass_context(&mut conn, &lookup_actor, |c| match kind {
            DocKind::Ticket => crate::repository::tickets::workspace_id_by_uuid(c, resource_uuid),
            DocKind::Documentation => {
                crate::repository::documentation::page_workspace_id_by_uuid(c, resource_uuid)
            }
            DocKind::Collection => {
                crate::repository::documentation_collections::collection_workspace_id_by_uuid(
                    c,
                    resource_uuid,
                )
            }
        })
        .map_err(|e| {
            error!(error = ?e, %resource_uuid, "collab file access: workspace lookup failed");
            actix_web::error::ErrorInternalServerError("Authorization check failed")
        })?
        .ok_or_else(|| actix_web::error::ErrorNotFound("File not found"))?;

    // Pinned to the resource's workspace: membership (None = non-member) plus
    // the caller's role *there* drives the document gate.
    let actor = ActorContext::user_at_workspace(auth.user_uuid, workspace_id);
    let allowed = session::with_actor_context(&mut conn, &actor, |c| {
        let Some(accessor) =
            DocAccessor::at_pinned_workspace(c, auth.user_uuid, auth.platform_role)
        else {
            return Ok(false);
        };

        // Re-resolve the UUID under the pin. Not redundant with the lookup
        // above: it re-proves under RLS that the resource really is in the
        // workspace we pinned to, so a bug in the unscoped read cannot widen
        // access, and it fails closed if the row vanished between the two.
        let doc_type = match kind {
            DocKind::Ticket => {
                crate::repository::tickets::id_by_uuid(c, resource_uuid)?.map(DocumentType::Ticket)
            }
            DocKind::Documentation => {
                crate::repository::documentation::page_id_by_uuid(c, resource_uuid)?
                    .map(DocumentType::Documentation)
            }
            DocKind::Collection => {
                crate::repository::documentation_collections::collection_id_by_uuid(
                    c,
                    resource_uuid,
                )?
                .map(DocumentType::Collection)
            }
        };
        let Some(doc_type) = doc_type else {
            return Ok(false);
        };

        can_access_document(c, &accessor, &doc_type)
    })
    .map_err(|e| {
        error!(error = ?e, %resource_uuid, "collab file access: authorization lookup failed");
        actix_web::error::ErrorInternalServerError("Authorization check failed")
    })?;

    if !allowed {
        return Err(actix_web::error::ErrorNotFound("File not found"));
    }
    Ok(workspace_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The URL token is the hinge between three surfaces: the collab doc_id,
    /// the image URL and the storage folder. A one-sided rename would leave
    /// already-stored images unreachable, so lock the round trip down.
    #[test]
    fn url_tokens_round_trip() {
        for kind in [DocKind::Ticket, DocKind::Documentation, DocKind::Collection] {
            assert_eq!(DocKind::from_url_token(kind.url_token()), Some(kind));
        }
    }

    #[test]
    fn unknown_url_token_is_rejected() {
        assert_eq!(DocKind::from_url_token("widget"), None);
        assert_eq!(DocKind::from_url_token(""), None);
        // The doc_id spells documentation pages "doc", not "documentation".
        assert_eq!(DocKind::from_url_token("documentation"), None);
    }

    /// The path segment after `/api/files/collab/` must be byte-identical to
    /// the logical storage key, so the serve route reconstructs exactly what
    /// the upload wrote.
    #[test]
    fn served_url_suffix_matches_storage_key() {
        let uuid = Uuid::parse_str("019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb").expect("valid uuid");
        let folder = image_folder(DocKind::Documentation, uuid);
        assert_eq!(folder, "collab/doc/019eb4e2-dbaa-75e5-9eb2-aa3dc7d8a7cb");

        let stored_name = "019ec111-2222-7333-8444-555566667777_paste.png";
        let url = format!(
            "/api/files/{}/{}/{}",
            "collab",
            format_args!("{}/{}", DocKind::Documentation.url_token(), uuid),
            stored_name
        );
        assert_eq!(
            url.strip_prefix("/api/files/")
                .expect("served under /api/files"),
            format!("{folder}/{stored_name}")
        );
    }
}
