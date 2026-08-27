//! Self-serve workspace data export (Owner-gated). The Art 28(3)(g) "return"
//! path: a workspace Owner exports all of their workspace's data (tickets,
//! comments, attachments, requesters, orgs, documents, ...) as a single ZIP,
//! before an account deletion erases it.
//!
//! Reuses the platform-admin export service (`services::workspace_export`), but:
//! - gated to the workspace Owner (not a cross-tenant platform admin), and the
//!   workspace comes from `WorkspaceContext`, NEVER a path param;
//! - runs as a background job that writes a storage-backed artifact with a
//!   bounded download window (`expires_at`), instead of a synchronous stream;
//! - strips sensitive auth fields (`include_sensitive = false`) since a data
//!   export never needs password hashes; an optional password only SEALS the zip.

use actix_web::http::header;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use chrono::{Duration, Utc};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::db::Pool;
use crate::extractors::{TenantConn, WorkspaceContext};
use crate::handlers::errors;
use crate::models::{
    Claims, NewWorkspaceExportJob, WorkspaceExportJob, WorkspaceExportJobUpdate, WorkspaceRole,
};
use crate::repository::workspace_export_jobs as export_repo;
use crate::services::workspace_export::{assemble_workspace_archive, collect_workspace_rows};
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_bypass_context;
use crate::utils::rbac::require_workspace_role;
use crate::utils::storage::{process_storage, WorkspaceScopedStorage};

/// Cap on the in-memory archive (row dumps + files). A larger workspace is
/// refused with a clear message rather than risking an OOM of the instance.
const MAX_EXPORT_BYTES: usize = 1024 * 1024 * 1024; // 1 GiB
/// How long a completed export stays downloadable, from completion.
const EXPORT_TTL_DAYS: i64 = 7;
/// Minimum spacing between completed exports per workspace.
const MIN_HOURS_BETWEEN_EXPORTS: i64 = 24;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/workspace/export", web::post().to(request_export))
        .route("/workspace/export", web::get().to(list_latest_export))
        .route("/workspace/export/{id}", web::get().to(get_export_status))
        .route(
            "/workspace/export/{id}/download",
            web::get().to(download_export),
        );
}

#[derive(Debug, Deserialize)]
pub struct RequestExportBody {
    /// Optional: when present, the archive is sealed with AES-256-GCM.
    #[serde(default)]
    pub password: Option<String>,
}

fn job_view(job: &WorkspaceExportJob) -> serde_json::Value {
    let now = Utc::now().naive_utc();
    let download_available =
        job.status == "completed" && job.expires_at.map(|e| e > now).unwrap_or(false);
    json!({
        "id": job.id,
        "status": job.status,
        "file_size": job.file_size,
        "error_message": job.error_message,
        "created_at": job.created_at,
        "completed_at": job.completed_at,
        "expires_at": job.expires_at,
        "download_available": download_available,
    })
}

/// POST /api/workspace/export — request a self-serve export of THIS workspace.
/// Owner-only. Returns 202 with the job to poll.
pub async fn request_export(
    pool: web::Data<Pool>,
    mut tc: TenantConn,
    ws: WorkspaceContext,
    req: HttpRequest,
    body: web::Json<RequestExportBody>,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Owner) {
        return resp;
    }
    let workspace_id = ws.workspace_id;
    let requested_by = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());
    let password = body.into_inner().password.filter(|p| !p.is_empty());

    // Rate limit 1: at most one in-flight export.
    match tc.run(|conn| export_repo::has_active(conn, workspace_id)) {
        Ok(true) => return errors::conflict("An export is already in progress."),
        Ok(false) => {}
        Err(e) => return errors::internal(format!("export check: {e}")),
    }
    // Rate limit 2: one completed export per day (a completed export can be
    // re-downloaded within its window, so this bounds the work, not access).
    let since = (Utc::now() - Duration::hours(MIN_HOURS_BETWEEN_EXPORTS)).naive_utc();
    match tc.run(|conn| export_repo::last_completed_at(conn, workspace_id)) {
        Ok(Some(last)) if last > since => {
            return errors::too_many_requests(
                "You can request one export per day. Download your latest export, or try again later.",
                MIN_HOURS_BETWEEN_EXPORTS as u64 * 3600,
            );
        }
        Ok(_) => {}
        Err(e) => return errors::internal(format!("export rate check: {e}")),
    }

    let new_job = NewWorkspaceExportJob {
        workspace_id,
        requested_by,
        status: "processing".to_string(),
    };
    let job = match tc.run(|conn| export_repo::create(conn, new_job)) {
        Ok(j) => j,
        Err(e) => return errors::internal(format!("create export job: {e}")),
    };
    let job_id = job.id;

    // Background: the export must read across RLS, so it runs under the BYPASSRLS
    // role with the workspace_id captured from the (Owner-gated) request context.
    let pool_clone = pool.clone();
    tokio::spawn(async move {
        run_export(pool_clone, job_id, workspace_id, password).await;
    });

    HttpResponse::Accepted().json(job_view(&job))
}

/// GET /api/workspace/export — the workspace's most recent export (or `null`),
/// so the UI can resume an in-flight export or offer a ready download after a
/// reload. Owner-only.
pub async fn list_latest_export(
    mut tc: TenantConn,
    ws: WorkspaceContext,
    req: HttpRequest,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Owner) {
        return resp;
    }
    match tc.run(|conn| export_repo::latest_for_workspace(conn, ws.workspace_id)) {
        Ok(Some(job)) => HttpResponse::Ok().json(job_view(&job)),
        Ok(None) => HttpResponse::Ok().json(serde_json::Value::Null),
        Err(e) => errors::internal(format!("latest export: {e}")),
    }
}

/// GET /api/workspace/export/{id} — poll job status. Owner-only, workspace-scoped.
pub async fn get_export_status(
    mut tc: TenantConn,
    ws: WorkspaceContext,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Owner) {
        return resp;
    }
    let id = path.into_inner();
    match tc.run(|conn| export_repo::get_owned(conn, id, ws.workspace_id)) {
        Ok(Some(job)) => HttpResponse::Ok().json(job_view(&job)),
        Ok(None) => errors::not_found("export"),
        Err(e) => errors::internal(format!("export status: {e}")),
    }
}

/// GET /api/workspace/export/{id}/download — stream the artifact. Owner-only,
/// workspace-scoped. 410 once the download window has passed.
pub async fn download_export(
    mut tc: TenantConn,
    ws: WorkspaceContext,
    req: HttpRequest,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(resp) = require_workspace_role(&req, WorkspaceRole::Owner) {
        return resp;
    }
    let id = path.into_inner();
    let job = match tc.run(|conn| export_repo::get_owned(conn, id, ws.workspace_id)) {
        Ok(Some(j)) => j,
        Ok(None) => return errors::not_found("export"),
        Err(e) => return errors::internal(format!("export lookup: {e}")),
    };
    if job.status != "completed" {
        return errors::not_found("export");
    }
    let now = Utc::now().naive_utc();
    if job.expires_at.map(|e| e < now).unwrap_or(true) {
        return errors::gone("This export has expired. Request a new one.");
    }
    let Some(key) = job.file_path else {
        return errors::not_found("export");
    };

    let scoped = WorkspaceScopedStorage::arc(process_storage(), ws.workspace_id);
    let bytes = match scoped.get_file(&key).await {
        Ok(b) => b,
        Err(e) => return errors::internal(format!("read export artifact: {e:?}")),
    };
    // The artifact is the whole tenant's data: no-store, no CORS wildcard, and an
    // attachment disposition (NOT the generic file proxy, which sets public
    // caching + ACAO:*).
    HttpResponse::Ok()
        .content_type("application/octet-stream")
        .insert_header((
            header::CONTENT_DISPOSITION,
            format!(
                "attachment; filename=\"workspace-{}.nosdesk\"",
                ws.workspace_id
            ),
        ))
        .insert_header((header::CACHE_CONTROL, "no-store"))
        .body(bytes)
}

/// Run the export end to end and record the terminal status. Never panics the
/// worker: any failure is logged and written as `failed` so the poller sees it.
async fn run_export(
    pool: web::Data<Pool>,
    job_id: Uuid,
    workspace_id: i32,
    password: Option<String>,
) {
    let outcome = run_export_inner(&pool, job_id, workspace_id, password).await;

    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            log::error!("workspace export {job_id}: cannot get conn to record status: {e}");
            return;
        }
    };
    let actor = ActorContext::system("background:workspace_export");
    let upd = match outcome {
        Ok((file_path, file_size)) => {
            let now = Utc::now().naive_utc();
            WorkspaceExportJobUpdate {
                status: Some("completed".to_string()),
                file_path: Some(file_path),
                file_size: Some(file_size),
                completed_at: Some(now),
                expires_at: Some(now + Duration::days(EXPORT_TTL_DAYS)),
                ..Default::default()
            }
        }
        Err(msg) => {
            log::error!("workspace export {job_id} failed: {msg}");
            WorkspaceExportJobUpdate {
                status: Some("failed".to_string()),
                error_message: Some(msg),
                ..Default::default()
            }
        }
    };
    let _ = with_actor_bypass_context(&mut conn, &actor, |conn| {
        export_repo::update(conn, job_id, upd).map(|_| ())
    });
}

/// The export work. Returns `(storage_key, byte_size)` on success, or a
/// human-readable failure message. Bounds peak memory: the row dumps and the
/// files are summed against `MAX_EXPORT_BYTES` and a large workspace is refused
/// before it can OOM the instance.
async fn run_export_inner(
    pool: &web::Data<Pool>,
    job_id: Uuid,
    workspace_id: i32,
    password: Option<String>,
) -> Result<(String, i64), String> {
    // 1. Row dumps (sync, BYPASSRLS). include_sensitive = false: a data export
    //    never needs auth secrets.
    let mut conn = pool.get().map_err(|e| format!("pool: {e}"))?;
    let actor = ActorContext::system("background:workspace_export");
    let (dumps, meta) = with_actor_bypass_context(&mut conn, &actor, |conn| {
        collect_workspace_rows(conn, workspace_id, false)
            .map_err(|e| diesel::result::Error::QueryBuilderError(e.to_string().into()))
    })
    .map_err(|e| format!("collect rows: {e}"))?;
    drop(conn);

    let mut total: usize = dumps.iter().map(|d| d.json.len()).sum();
    if total > MAX_EXPORT_BYTES {
        return Err(too_large());
    }

    // 2. Files, read with a cumulative cap.
    let scoped = WorkspaceScopedStorage::arc(process_storage(), workspace_id);
    let paths = scoped
        .list_prefix("")
        .await
        .map_err(|e| format!("list files: {e:?}"))?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::with_capacity(paths.len());
    for p in paths {
        let bytes = scoped
            .get_file(&p)
            .await
            .map_err(|e| format!("read file {p}: {e:?}"))?;
        total += bytes.len();
        if total > MAX_EXPORT_BYTES {
            return Err(too_large());
        }
        files.push((p, bytes));
    }

    // 3. Assemble (sync CPU). 4. Store under the workspace-scoped prefix.
    let archive =
        assemble_workspace_archive(&dumps, &meta, workspace_id, &files, password.as_deref())
            .map_err(|e| format!("assemble archive: {e}"))?;
    let file_size = archive.len() as i64;
    let key = format!("exports/{job_id}.nosdesk");
    scoped
        .put_file(&archive, &key, "application/octet-stream")
        .await
        .map_err(|e| format!("store artifact: {e:?}"))?;
    Ok((key, file_size))
}

fn too_large() -> String {
    "This workspace is too large for a self-serve export. Contact support to arrange one."
        .to_string()
}
