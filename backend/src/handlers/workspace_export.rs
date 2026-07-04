//! Platform-admin endpoints to export and import a single workspace as a
//! portable archive.
//!
//! Thin wrappers over `services::workspace_export` / `workspace_import`.
//! Operator-only (`require_platform_admin`); both run through the BYPASSRLS
//! `PlatformConn`, which they require — under the runtime `nosdesk_app` role with
//! no workspace pin, RLS would filter the export's reads to zero rows, and the
//! import writes cross-tenant.

use std::path::PathBuf;

use actix_multipart::Multipart;
use actix_web::http::header;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use futures::StreamExt;
use serde::Deserialize;

use crate::extractors::PlatformConn;
use crate::handlers::errors;
use crate::utils::rbac;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/workspaces/{id}/export",
        web::post().to(export_workspace),
    )
    .route("/admin/workspaces/import", web::post().to(import_workspace));
}

#[derive(Debug, Deserialize)]
pub struct WorkspaceExportRequest {
    /// When present, the archive is sealed (AES-256-GCM) and sensitive auth
    /// fields are kept. Absent yields a plaintext zip with `SENSITIVE_FIELDS`
    /// stripped. Carried in the body (never the URL) so it isn't logged/cached.
    #[serde(default)]
    pub password: Option<String>,
}

/// `POST /api/admin/workspaces/{id}/export` — download a workspace-scoped
/// archive (rows for the workspace's tenant tables plus the workspace row and
/// its member users). Held in memory and streamed as an attachment.
pub async fn export_workspace(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<i32>,
    body: web::Json<WorkspaceExportRequest>,
) -> impl Responder {
    if let Err(resp) = rbac::require_platform_admin(&req) {
        return resp;
    }
    let workspace_id = path.into_inner();
    let password = body.into_inner().password;

    match pc.run(|conn| {
        crate::services::workspace_export::export_workspace(conn, workspace_id, password.as_deref())
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    }) {
        Ok(bytes) => HttpResponse::Ok()
            .content_type("application/octet-stream")
            .insert_header((
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"workspace-{workspace_id}.nosdesk\""),
            ))
            .body(bytes),
        Err(e) => errors::db_error(&e),
    }
}

/// `POST /api/admin/workspaces/import` — reconstruct a workspace from an uploaded
/// export archive as a NEW workspace (fresh id, all integer keys remapped).
///
/// Multipart fields: `archive` (the file, required), `password` (optional, for a
/// sealed archive), `regenerate_uuids` ("true" to clone into the same cell;
/// default preserves uuids for a cross-cell region move), `slug` (optional slug
/// override). Returns the new workspace id + import counts as JSON.
pub async fn import_workspace(
    req: HttpRequest,
    mut pc: PlatformConn,
    mut payload: Multipart,
) -> impl Responder {
    if let Err(resp) = rbac::require_platform_admin(&req) {
        return resp;
    }

    let mut archive: Vec<u8> = Vec::new();
    let mut password: Option<String> = None;
    let mut regenerate_uuids = false;
    let mut slug_override: Option<String> = None;

    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(f) => f,
            Err(e) => return errors::bad_request(format!("upload error: {e}")),
        };
        let name = field
            .content_disposition()
            .get_name()
            .unwrap_or_default()
            .to_string();
        let mut buf = Vec::new();
        while let Some(chunk) = field.next().await {
            match chunk {
                Ok(d) => buf.extend_from_slice(&d),
                Err(e) => return errors::bad_request(format!("upload error: {e}")),
            }
        }
        match name.as_str() {
            "archive" => archive = buf,
            "password" => {
                let s = String::from_utf8_lossy(&buf).to_string();
                if !s.is_empty() {
                    password = Some(s);
                }
            }
            "regenerate_uuids" => {
                regenerate_uuids = matches!(String::from_utf8_lossy(&buf).trim(), "true" | "1");
            }
            "slug" => {
                let s = String::from_utf8_lossy(&buf).trim().to_string();
                if !s.is_empty() {
                    slug_override = Some(s);
                }
            }
            _ => {}
        }
    }

    if archive.is_empty() {
        return errors::bad_request("no archive uploaded (multipart field 'archive')");
    }

    let uploads_dir = std::env::var("UPLOAD_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/app/uploads"));
    let opts = crate::services::workspace_import::ImportOptions {
        slug_override,
        uuid_override: None,
        uploads_dir,
        regenerate_uuids,
    };

    match pc.run(|conn| {
        crate::services::workspace_import::import_workspace(
            conn,
            &archive,
            password.as_deref(),
            opts,
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    }) {
        Ok(result) => HttpResponse::Ok().json(serde_json::json!({
            "workspace_id": result.workspace_id,
            "slug": result.slug,
            "tables_imported": result.tables_imported,
            "rows_imported": result.rows_imported,
            "files_restored": result.files_restored,
        })),
        Err(e) => errors::db_error(&e),
    }
}
