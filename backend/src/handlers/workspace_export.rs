//! Platform-admin endpoint to export a single workspace as a portable archive.
//!
//! Thin wrapper over `services::workspace_export`. Operator-only
//! (`require_platform_admin`); the export runs through the BYPASSRLS
//! `PlatformConn`, which it requires — under the runtime `nosdesk_app` role with
//! no workspace pin, RLS would filter every scoped read to zero rows.

use actix_web::http::header;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;

use crate::extractors::PlatformConn;
use crate::handlers::errors;
use crate::utils::rbac;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/workspaces/{id}/export",
        web::post().to(export_workspace),
    );
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
