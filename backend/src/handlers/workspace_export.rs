//! Platform-admin endpoints to export and import a single workspace as a
//! portable archive.
//!
//! Thin wrappers over `services::workspace_export` / `workspace_import`.
//! Operator-only (`require_platform_admin`); both run through the BYPASSRLS
//! `PlatformConn`, which they require — under the runtime `nosdesk_app` role with
//! no workspace pin, RLS would filter the export's reads to zero rows, and the
//! import writes cross-tenant.

use actix_multipart::Multipart;
use actix_web::http::header;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use futures::StreamExt;
use serde::Deserialize;

use crate::extractors::PlatformConn;
use crate::handlers::errors;
use crate::utils::rbac;
use crate::utils::storage::{process_storage, WorkspaceScopedStorage};

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
    let include_sensitive = password.is_some();

    // 1. Collect the row dumps (sync, in the BYPASSRLS transaction).
    let (dumps, meta) = match pc.run(|conn| {
        crate::services::workspace_export::collect_workspace_rows(
            conn,
            workspace_id,
            include_sensitive,
        )
        .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    }) {
        Ok(v) => v,
        Err(e) => return errors::db_error(&e),
    };

    // 2. Read the workspace's files through the storage abstraction (local or
    //    S3) so file migration works on hosted, not just the local filesystem.
    let scoped = WorkspaceScopedStorage::arc(process_storage(), workspace_id);
    let paths = match scoped.list_prefix("").await {
        Ok(p) => p,
        Err(e) => return errors::internal(format!("listing workspace files: {e:?}")),
    };
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(paths.len());
    for p in paths {
        match scoped.get_file(&p).await {
            Ok(bytes) => files.push((p, bytes)),
            Err(e) => return errors::internal(format!("reading workspace file {p}: {e:?}")),
        }
    }

    // 3. Assemble + seal (sync).
    match crate::services::workspace_export::assemble_workspace_archive(
        &dumps,
        &meta,
        workspace_id,
        &files,
        password.as_deref(),
    ) {
        Ok(bytes) => HttpResponse::Ok()
            .content_type("application/octet-stream")
            .insert_header((
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"workspace-{workspace_id}.nosdesk\""),
            ))
            .body(bytes),
        Err(e) => errors::internal(format!("assembling archive: {e}")),
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

    // Scaling ceiling: export/import hold the whole archive in memory (peak ~3x
    // the archive size: files Vec + zip buffer + sealed buffer), because the
    // AES-GCM envelope seals the entire inner zip as one blob — it can't be
    // produced/consumed incrementally. Bounded-memory streaming would need a
    // chunked-AEAD format (a deliberate crypto change, shared with the whole-DB
    // backup); deferred until a tenant's archive approaches the instance RAM
    // budget. This cap bounds that memory (configurable to raise it) and defends
    // against accidental-huge / zip-bomb uploads on top of the platform-admin gate.
    let max_archive_bytes: u64 = std::env::var("NOSDESK_MAX_IMPORT_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2 * 1024 * 1024 * 1024);

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
                Ok(d) => {
                    buf.extend_from_slice(&d);
                    if name == "archive" && buf.len() as u64 > max_archive_bytes {
                        return errors::bad_request(format!(
                            "archive exceeds the {max_archive_bytes}-byte limit \
                             (raise NOSDESK_MAX_IMPORT_BYTES)"
                        ));
                    }
                }
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

    // Read + verify the archive (sync, no DB).
    let contents =
        match crate::services::workspace_import::read_archive(&archive, password.as_deref()) {
            Ok(c) => c,
            Err(e) => return errors::bad_request(format!("invalid archive: {e}")),
        };

    let opts = crate::services::workspace_import::ImportOptions {
        slug_override,
        uuid_override: None,
        regenerate_uuids,
    };

    // 1. Import the rows (sync, BYPASSRLS, atomic).
    let result = match pc.run(|conn| {
        crate::services::workspace_import::import_workspace(conn, &contents, &opts)
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    }) {
        Ok(r) => r,
        Err(e) => return errors::db_error(&e),
    };

    // 2. Restore files through the storage abstraction into the NEW workspace
    //    (local or S3). Archive entries are `files/{logical}`. NOTE: the rows are
    //    already committed above, so a failure here leaves a workspace with
    //    partial files; the error response carries workspace_id so the operator
    //    can clean up.
    let scoped = WorkspaceScopedStorage::arc(process_storage(), result.workspace_id);
    let mut files_restored = 0i64;
    let mut files_skipped = 0i64;
    for (name, bytes) in &contents.files {
        let logical = name.strip_prefix("files/").unwrap_or(name);
        // Zip-slip guard: reject `..` / absolute / backslash paths from the
        // untrusted archive before writing (matches the whole-DB restore).
        if logical.is_empty() || !crate::utils::storage::is_safe_storage_path(logical) {
            log::warn!("workspace import: skipping unsafe file entry '{name}'");
            files_skipped += 1;
            continue;
        }
        if let Err(e) = scoped
            .put_file(bytes, logical, content_type_for(logical))
            .await
        {
            return HttpResponse::InternalServerError().json(serde_json::json!({
                "error": format!("file restore failed after {files_restored} files: {e:?}"),
                "partial": true,
                "workspace_id": result.workspace_id,
                "rows_imported": result.rows_imported,
                "files_restored": files_restored,
            }));
        }
        files_restored += 1;
    }

    HttpResponse::Ok().json(serde_json::json!({
        "workspace_id": result.workspace_id,
        "slug": result.slug,
        "tables_imported": result.tables_imported,
        "rows_imported": result.rows_imported,
        "files_restored": files_restored,
        "files_skipped": files_skipped,
    }))
}

/// Best-effort content type from a file extension, so restored images serve with
/// a displayable type. Unknown extensions fall back to octet-stream.
fn content_type_for(path: &str) -> &'static str {
    match path
        .rsplit('.')
        .next()
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("svg") => "image/svg+xml",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        _ => "application/octet-stream",
    }
}
