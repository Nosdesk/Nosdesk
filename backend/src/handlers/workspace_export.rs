//! Platform-admin endpoints to export and import a single workspace as a
//! portable archive.
//!
//! Thin wrappers over `services::workspace_export`, `workspace_import` and
//! `workspace_archive::apply`.
//! Operator-only (`require_platform_admin`); both run through the BYPASSRLS
//! `PlatformConn`, which they require — under the runtime `nosdesk_app` role with
//! no workspace pin, RLS would filter the export's reads to zero rows, and the
//! import writes cross-tenant.

use actix_multipart::Multipart;
use actix_web::http::header;
use actix_web::http::StatusCode;
use actix_web::{web, HttpRequest, HttpResponse};
use futures::StreamExt;
use serde::Deserialize;
use std::sync::Arc;

use crate::errors::{self, ApiError};
use crate::extractors::PlatformConn;
use crate::services::search::{indexing_tasks, SearchService};
use crate::services::workspace_archive::{apply, files};
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
    /// When present, the archive is sealed (AES-256-GCM) and the archive
    /// policy's secret columns are kept. Absent yields a plaintext zip without
    /// them. Carried in the body (never the URL) so it isn't logged/cached.
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
) -> Result<HttpResponse, ApiError> {
    rbac::require_platform_admin(&req)?;
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
        Err(e) => return Err(ApiError::Database(e)),
    };

    // 2. Read the workspace's files through the storage abstraction (local or
    //    S3) so file migration works on hosted, not just the local filesystem.
    let scoped = WorkspaceScopedStorage::arc(process_storage(), workspace_id);
    let paths = match scoped.list_prefix("").await {
        Ok(p) => p,
        Err(e) => {
            return Err(ApiError::Internal(format!(
                "listing workspace files: {e:?}"
            )))
        }
    };
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(paths.len());
    for p in paths
        .into_iter()
        .filter(|p| crate::services::workspace_export::is_exported_file(p))
    {
        match scoped.get_file(&p).await {
            Ok(bytes) => files.push((p, bytes)),
            Err(e) => {
                return Err(ApiError::Internal(format!(
                    "reading workspace file {p}: {e:?}"
                )))
            }
        }
    }

    // 3. People's avatars, and branding from before it was workspace-scoped,
    //    which live outside the workspace's prefix.
    let avatars =
        crate::services::workspace_export::read_avatars(process_storage().as_ref(), &meta).await;
    crate::services::workspace_export::add_legacy_branding(
        process_storage().as_ref(),
        &meta,
        &mut files,
    )
    .await;

    // 4. Assemble + seal (sync).
    match crate::services::workspace_export::assemble_workspace_archive(
        &dumps,
        &meta,
        workspace_id,
        &files,
        &avatars,
        password.as_deref(),
    ) {
        Ok(bytes) => Ok(HttpResponse::Ok()
            .content_type("application/octet-stream")
            .insert_header((
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"workspace-{workspace_id}.nosdesk\""),
            ))
            .body(bytes)),
        Err(e) => Err(ApiError::Internal(format!("assembling archive: {e}"))),
    }
}

/// `POST /api/admin/workspaces/import`: load an uploaded export archive.
///
/// With a `workspace_id` field, into that existing workspace, which must be
/// untouched (`workspace_archive::apply`): it keeps its identity, and 409
/// `workspace_not_empty` names what it already holds. Without one, as a NEW
/// workspace (fresh id, all keys remapped), for an operator's region move.
///
/// Multipart fields: `archive` (the file, required), `password` (optional, for a
/// sealed archive), `workspace_id` (optional target), `regenerate_uuids` ("true"
/// to clone into the same cell; default preserves uuids for a cross-cell region
/// move), `slug` (optional slug override, new workspace only). Returns the
/// workspace id and import counts as JSON.
pub async fn import_workspace(
    req: HttpRequest,
    mut pc: PlatformConn,
    search_service: Option<web::Data<Arc<SearchService>>>,
    mut payload: Multipart,
) -> Result<HttpResponse, ApiError> {
    let claims = rbac::require_platform_admin(&req)?;
    let runner = crate::services::workspace_archive::plan::Runner::Operator {
        user: uuid::Uuid::parse_str(&claims.sub).ok(),
    };

    let mut archive: Vec<u8> = Vec::new();
    let mut password: Option<String> = None;
    let mut regenerate_uuids = false;
    let mut slug_override: Option<String> = None;
    let mut target_workspace: Option<i32> = None;

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
            Err(e) => return Err(ApiError::BadRequest(format!("upload error: {e}"))),
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
                        return Err(ApiError::BadRequest(format!(
                            "archive exceeds the {max_archive_bytes}-byte limit \
                             (raise NOSDESK_MAX_IMPORT_BYTES)"
                        )));
                    }
                }
                Err(e) => return Err(ApiError::BadRequest(format!("upload error: {e}"))),
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
            "workspace_id" => {
                let s = String::from_utf8_lossy(&buf).trim().to_string();
                if !s.is_empty() {
                    match s.parse::<i32>() {
                        Ok(id) => target_workspace = Some(id),
                        Err(_) => {
                            return Err(ApiError::BadRequest(
                                "workspace_id must be a workspace's numeric id".into(),
                            ))
                        }
                    }
                }
            }
            _ => {}
        }
    }

    if archive.is_empty() {
        return Err(ApiError::BadRequest(
            "no archive uploaded (multipart field 'archive')".into(),
        ));
    }

    // Read + verify the archive (sync, no DB).
    let contents =
        match crate::services::workspace_import::read_archive(&archive, password.as_deref()) {
            Ok(c) => c,
            Err(e @ crate::services::backup::BackupError::NotImportable(_)) => {
                return Err(ApiError::BadRequest(e.to_string()))
            }
            Err(e) => return Err(ApiError::BadRequest(format!("invalid archive: {e}"))),
        };
    let mode = crate::middleware::workspace_context::DeploymentMode::current();

    if let Some(workspace_id) = target_workspace {
        return import_into_workspace(
            &mut pc,
            &contents,
            crate::services::workspace_archive::plan::ImportTarget {
                workspace_id,
                mode,
                runner,
            },
            regenerate_uuids,
            search_service.as_ref().map(|d| d.get_ref()),
        )
        .await;
    }

    let opts = crate::services::workspace_import::ImportOptions {
        slug_override,
        uuid_override: None,
        regenerate_uuids,
        mode,
        runner,
    };

    // 1. Import the rows (sync, BYPASSRLS, atomic). A plan refusal is the
    //    archive's problem (400), named by table.
    let mut refused: Option<String> = None;
    let result = match pc.run(|conn| {
        crate::services::workspace_import::import_workspace(conn, &contents, &opts).map_err(|e| {
            if crate::services::workspace_import::plan_refusals(&e).is_some() {
                refused = Some(e.to_string());
            }
            diesel::result::Error::QueryBuilderError(e.to_string().into())
        })
    }) {
        Ok(r) => r,
        Err(_) if refused.is_some() => {
            return Err(ApiError::BadRequest(refused.unwrap_or_default()))
        }
        Err(e) => return Err(ApiError::Database(e)),
    };

    reindex_search(
        search_service.as_ref().map(|d| d.get_ref()),
        result.workspace_id,
    );

    // 2. Restore files into the NEW workspace. The rows are already committed,
    //    so a failure here leaves partial files; the error carries workspace_id.
    let files = files::restore(
        process_storage(),
        result.workspace_id,
        &contents,
        &result.files,
    )
    .await;
    if let Some(response) = files_partial(&files, result.workspace_id, result.rows_imported) {
        return Ok(response);
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "workspace_id": result.workspace_id,
        "slug": result.slug,
        "tables_imported": result.tables_imported,
        "rows_imported": result.rows_imported,
        "files_restored": files.restored,
        "files_present": files.present,
        "files_skipped": files.skipped,
        "report": result.report,
    })))
}

/// Import into the existing workspace `target` (`apply::import_into`), then
/// restore the archive's files into it.
async fn import_into_workspace(
    pc: &mut PlatformConn,
    contents: &crate::services::workspace_import::ArchiveContents,
    target: crate::services::workspace_archive::plan::ImportTarget,
    regenerate_uuids: bool,
    search_service: Option<&Arc<SearchService>>,
) -> Result<HttpResponse, ApiError> {
    let workspace_id = target.workspace_id;
    let held = apply::held_files(process_storage(), workspace_id)
        .await
        .map_err(|e| ApiError::Internal(format!("listing workspace files: {e:?}")))?;

    // Rows, in one transaction on PlatformConn's owner role (the import updates
    // the workspace row and clears its seeded configuration).
    let mut refusal: Option<apply::ApplyError> = None;
    let options = crate::services::workspace_archive::plan::PlanOptions {
        regenerate_row_uuids: regenerate_uuids,
    };
    let applied = match pc.run(|conn| {
        apply::import_into(conn, contents, &target, options, &held).map_err(|e| {
            let message = e.to_string();
            refusal = Some(e);
            diesel::result::Error::QueryBuilderError(message.into())
        })
    }) {
        Ok(applied) => applied,
        Err(e) => {
            let Some(refusal) = refusal else {
                return Err(ApiError::Database(e));
            };
            return match &refusal {
                apply::ApplyError::NotUntouched { found } => Ok(errors::with_fields(
                    StatusCode::CONFLICT,
                    refusal.code(),
                    refusal.to_string(),
                    serde_json::json!({ "found": found }),
                )),
                apply::ApplyError::Busy { .. } => Ok(errors::conflict_with_code(
                    refusal.to_string(),
                    refusal.code(),
                )),
                apply::ApplyError::NoWorkspace { .. } => Ok(errors::not_found_with_code(
                    refusal.to_string(),
                    refusal.code(),
                )),
                apply::ApplyError::Refused(_) => Err(ApiError::BadRequest(refusal.to_string())),
                _ => Err(ApiError::Internal(format!("workspace import: {refusal}"))),
            };
        }
    };

    reindex_search(search_service, workspace_id);

    // Files, after commit: a key already in place is left, so a partial
    // restore can run again with the same plan.
    let files = files::restore(process_storage(), workspace_id, contents, &applied.files).await;
    if let Some(response) = files_partial(&files, workspace_id, applied.rows_imported) {
        return Ok(response);
    }

    Ok(HttpResponse::Ok().json(serde_json::json!({
        "workspace_id": workspace_id,
        "tables_imported": applied.tables_imported,
        "rows_imported": applied.rows_imported,
        "cleared": applied.cleared,
        "data_epoch": applied.fence.epoch,
        "files_restored": files.restored,
        "files_present": files.present,
        "files_skipped": files.skipped,
        "report": applied.report,
    })))
}

/// Index an imported workspace's rows for search, in the background, once
/// they are committed. The load writes them past the incremental search
/// hooks. Other workspaces' documents are left alone.
fn reindex_search(search_service: Option<&Arc<SearchService>>, workspace_id: i32) {
    if let Some(search) = search_service {
        indexing_tasks::spawn_reindex_workspace(Arc::clone(search), workspace_id);
    }
}

/// The response when the file restore stopped part way, if it did.
fn files_partial(
    files: &files::FilesRestored,
    workspace_id: i32,
    rows_imported: i64,
) -> Option<HttpResponse> {
    files.failed.as_ref()?;
    Some(errors::with_fields(
        StatusCode::INTERNAL_SERVER_ERROR,
        "RESTORE_PARTIAL",
        format!("file restore failed after {} files", files.restored),
        serde_json::json!({
            "partial": true,
            "workspace_id": workspace_id,
            "rows_imported": rows_imported,
            "files_restored": files.restored,
            "files_skipped": files.skipped,
        }),
    ))
}
