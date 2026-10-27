//! Workspace imports as jobs (`services::workspace_import_jobs`): upload an
//! export into an existing, untouched workspace, check it, confirm it, follow
//! it, reset it, retry its files, and invite the staff it created.
//!
//! Two sets of routes over the same jobs:
//!
//! - `/workspace/imports/...`: the workspace owner, for their own workspace.
//!   Self-hosted only: on hosted every route answers 404. A move into Cloud
//!   is run by Nosdesk support on request, through the platform-admin routes
//!   below, never through an owner's upload to the shared machine.
//! - `/admin/workspaces/{id}/imports/...`: a platform admin, for any
//!   workspace (ops and recovery).
//!
//! Every refusal answers `{error, code, args}`.

use actix_multipart::Multipart;
use actix_web::http::StatusCode;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use futures::TryStreamExt;
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

use crate::db::Pool;
use crate::errors::{self, ApiError};
use crate::extractors::WorkspaceContext;
use crate::middleware::workspace_context::{is_hosted, DeploymentMode};
use crate::models::{WorkspaceImportJob, WorkspaceImportJobChange, WorkspaceRole};
use crate::repository::workspace_import_jobs as repo;
use crate::services::imports::refusal::Refused;
use crate::services::imports::sign_in;
use crate::services::search::SearchService;
use crate::services::uploaded_archive::spool;
use crate::services::workspace_import_jobs::{
    self as jobs, in_workspace, Origin, ParkError, ResetError, Runtime, CHECKING, FAILED,
    IMPORTING, PREVIEWED, UPLOADED,
};
use crate::sync::actor::ActorContext;
use crate::utils::rbac;
use crate::utils::storage::Storage;

pub fn config(cfg: &mut web::ServiceConfig) {
    // Literal paths before the `{job}` wildcard.
    cfg.service(
        web::resource("/workspace/imports")
            .route(web::get().to(tenant_list))
            .route(web::post().to(tenant_upload)),
    )
    .service(web::resource("/workspace/imports/latest").route(web::get().to(tenant_latest)))
    .service(web::resource("/workspace/imports/{job}").route(web::get().to(tenant_get)))
    .service(web::resource("/workspace/imports/{job}/check").route(web::post().to(tenant_check)))
    .service(
        web::resource("/workspace/imports/{job}/confirm").route(web::post().to(tenant_confirm)),
    )
    .service(web::resource("/workspace/imports/{job}/reset").route(web::post().to(tenant_reset)))
    .service(
        web::resource("/workspace/imports/{job}/retry-files")
            .route(web::post().to(tenant_retry_files)),
    )
    .service(web::resource("/workspace/imports/{job}/invite").route(web::post().to(tenant_invite)))
    .service(
        web::resource("/admin/workspaces/{id}/imports")
            .route(web::get().to(admin_list))
            .route(web::post().to(admin_upload)),
    )
    .service(web::resource("/admin/workspaces/{id}/imports/{job}").route(web::get().to(admin_get)))
    .service(
        web::resource("/admin/workspaces/{id}/imports/{job}/check")
            .route(web::post().to(admin_check)),
    )
    .service(
        web::resource("/admin/workspaces/{id}/imports/{job}/confirm")
            .route(web::post().to(admin_confirm)),
    )
    .service(
        web::resource("/admin/workspaces/{id}/imports/{job}/reset")
            .route(web::post().to(admin_reset)),
    )
    .service(
        web::resource("/admin/workspaces/{id}/imports/{job}/retry-files")
            .route(web::post().to(admin_retry_files)),
    );
}

/// The body of check, confirm and retry files. The password opens a sealed
/// archive and is never stored. `regenerate_uuids` (check only) gives every
/// imported row a new uuid: a copy of a workspace into the server that still
/// holds it.
#[derive(Debug, Default, Deserialize)]
pub struct ActionBody {
    #[serde(default)]
    pub password: Option<String>,
    #[serde(default)]
    pub regenerate_uuids: Option<bool>,
}

/// The body of invite: a subset of the staff the import created, or all of
/// them.
#[derive(Debug, Default, Deserialize)]
pub struct InviteBody {
    #[serde(default)]
    pub user_uuids: Option<Vec<Uuid>>,
}

/// Which workspace's jobs a request reaches, as whom.
struct Scope {
    workspace_id: i32,
    origin: Origin,
    rt: Runtime,
}

impl Scope {
    fn actor(&self) -> ActorContext {
        match self.origin.user() {
            Some(user) => ActorContext::user(user, None).with_workspace(self.workspace_id),
            None => {
                ActorContext::system("handler:workspace_import").with_workspace(self.workspace_id)
            }
        }
    }

    /// The workspace an owner's reset seeds its welcome page for.
    fn seed_author(&self) -> Option<Uuid> {
        match self.origin {
            Origin::Owner { user } => Some(user),
            Origin::Operator { .. } => None,
        }
    }

    fn mode(&self) -> DeploymentMode {
        match self.origin {
            Origin::Owner { .. } => DeploymentMode::SelfHosted,
            Origin::Operator { .. } => DeploymentMode::current(),
        }
    }
}

fn refused(status: StatusCode, refused: Refused) -> HttpResponse {
    errors::with_fields(
        status,
        refused.code,
        refused.message,
        serde_json::json!({ "args": refused.args }),
    )
}

fn not_found(id: Uuid) -> HttpResponse {
    errors::not_found_with_code(
        format!("workspace import {id} not found"),
        "workspace_import.not_found",
    )
}

/// A bespoke refusal as an error a helper returns with `?`.
fn fail(response: HttpResponse) -> actix_web::Error {
    errors::from_response("workspace_import", response)
}

fn internal(e: impl std::fmt::Display) -> HttpResponse {
    tracing::error!(error = %e, "workspace import route failed");
    errors::internal("The workspace import couldn't be read or changed.")
}

fn runtime(req: &HttpRequest, pool: &web::Data<Pool>) -> Runtime {
    let storage: Arc<dyn Storage> = req
        .app_data::<web::Data<Arc<dyn Storage>>>()
        .map(|d| d.get_ref().clone())
        .unwrap_or_else(crate::utils::storage::process_storage);
    let search = req
        .app_data::<web::Data<Arc<SearchService>>>()
        .map(|d| d.get_ref().clone());
    Runtime {
        pool: pool.get_ref().clone(),
        storage,
        search,
    }
}

/// The owner's scope: their own workspace, self-hosted only.
fn tenant_scope(req: &HttpRequest, pool: &web::Data<Pool>) -> actix_web::Result<Scope> {
    if is_hosted() {
        return Err(fail(errors::not_found_with_code(
            "Moving a workspace into Nosdesk Cloud is done by Nosdesk support. Contact support to \
             arrange it.",
            "workspace_import.not_on_cloud",
        )));
    }
    let claims = rbac::require_workspace_role(req, WorkspaceRole::Owner)?;
    let user =
        Uuid::parse_str(&claims.sub).map_err(|_| ApiError::Unauthorized("Invalid user".into()))?;
    let workspace_id = req
        .extensions()
        .get::<WorkspaceContext>()
        .map(|w| w.workspace_id)
        .ok_or_else(|| ApiError::NotFound("Workspace not found".into()))?;
    Ok(Scope {
        workspace_id,
        origin: Origin::Owner { user },
        rt: runtime(req, pool),
    })
}

/// A platform admin's scope: workspace `workspace_id`, which must exist.
async fn admin_scope(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
    workspace_id: i32,
) -> actix_web::Result<Scope> {
    let claims = rbac::require_platform_admin(req)?;
    let scope = Scope {
        workspace_id,
        origin: Origin::Operator {
            user: Uuid::parse_str(&claims.sub).ok(),
        },
        rt: runtime(req, pool),
    };
    let exists = in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
        crate::repository::workspaces::find_by_id(conn, workspace_id)
    })
    .await
    .map_err(|e| fail(internal(e)))?;
    if exists.is_none_or(|w| w.archived_at.is_some()) {
        return Err(fail(errors::not_found_with_code(
            format!("There's no workspace {workspace_id} to import into."),
            "workspace_not_found",
        )));
    }
    Ok(scope)
}

/// The job, put back first if its background run died.
async fn load(scope: &Scope, id: Uuid) -> actix_web::Result<WorkspaceImportJob> {
    let workspace_id = scope.workspace_id;
    in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
        jobs::settle(conn, workspace_id, id)
    })
    .await
    .map_err(|e| fail(internal(e)))?
    .ok_or_else(|| fail(not_found(id)))
}

/// Apply `change` when the job is in one of `from`; 409 naming the state
/// it is in otherwise.
async fn move_job(
    scope: &Scope,
    id: Uuid,
    from: &'static [&'static str],
    change: WorkspaceImportJobChange,
) -> actix_web::Result<WorkspaceImportJob> {
    let workspace_id = scope.workspace_id;
    let moved = in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
        repo::transition(conn, workspace_id, id, from, change)
    })
    .await
    .map_err(|e| fail(internal(e)))?;
    match moved {
        Some(job) => Ok(job),
        None => {
            let job = load(scope, id).await?;
            Err(fail(refused(
                StatusCode::CONFLICT,
                Refused::new(
                    "workspace_import.wrong_state",
                    format!("This import is {} and can't do that now.", job.state),
                )
                .arg("state", job.state),
            )))
        }
    }
}

// ---------------------------------------------------------------------------
// Shared route bodies
// ---------------------------------------------------------------------------

async fn list(scope: Scope) -> HttpResponse {
    let workspace_id = scope.workspace_id;
    match in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
        repo::recent(conn, workspace_id, 20)
    })
    .await
    {
        Ok(list) => HttpResponse::Ok().json(list.iter().map(jobs::view).collect::<Vec<_>>()),
        Err(e) => internal(e),
    }
}

async fn latest(scope: Scope) -> HttpResponse {
    let workspace_id = scope.workspace_id;
    let latest = match in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
        repo::recent(conn, workspace_id, 1)
    })
    .await
    {
        Ok(mut list) => list.pop(),
        Err(e) => return internal(e),
    };
    match latest {
        Some(job) => match load(&scope, job.id).await {
            Ok(job) => HttpResponse::Ok().json(jobs::view(&job)),
            Err(e) => e.error_response(),
        },
        None => HttpResponse::Ok().json(serde_json::Value::Null),
    }
}

async fn get(scope: Scope, id: Uuid) -> HttpResponse {
    match load(&scope, id).await {
        Ok(job) => HttpResponse::Ok().json(jobs::view(&job)),
        Err(e) => e.error_response(),
    }
}

/// What a multipart upload carried.
struct Upload {
    archive: spool::Spooled,
    password: Option<String>,
    regenerate_uuids: bool,
}

/// A small text field, at most 4 KiB.
async fn small_field(field: &mut actix_multipart::Field) -> Result<String, Refused> {
    let mut buf = Vec::new();
    while let Some(chunk) = field
        .try_next()
        .await
        .map_err(|e| Refused::new("upload_unreadable", e.to_string()))?
    {
        buf.extend_from_slice(&chunk);
        if buf.len() > 4096 {
            return Err(Refused::new(
                "upload_unreadable",
                "A form field is too long.",
            ));
        }
    }
    Ok(String::from_utf8_lossy(&buf).trim().to_string())
}

async fn read_upload(payload: &mut Multipart) -> Result<Upload, (StatusCode, Refused)> {
    let bad = |r: Refused| (StatusCode::BAD_REQUEST, r);
    let mut archive = None;
    let mut password = None;
    let mut regenerate_uuids = false;
    while let Some(mut field) = payload
        .try_next()
        .await
        .map_err(|e| bad(Refused::new("upload_unreadable", e.to_string())))?
    {
        let name = field
            .content_disposition()
            .get_name()
            .unwrap_or_default()
            .to_string();
        match name.as_str() {
            "archive" => {
                let spooled = spool::spool_field(&mut field, jobs::max_upload_bytes(), None)
                    .await
                    .map_err(|e| {
                        let status = match e {
                            spool::SpoolError::TooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
                            _ => StatusCode::BAD_REQUEST,
                        };
                        (status, Refused::from(e))
                    })?;
                archive = Some(spooled);
            }
            "password" => {
                password = Some(small_field(&mut field).await.map_err(bad)?)
                    .filter(|p: &String| !p.is_empty());
            }
            "regenerate_uuids" => {
                regenerate_uuids = matches!(
                    small_field(&mut field).await.map_err(bad)?.as_str(),
                    "true" | "1"
                );
            }
            _ => {
                small_field(&mut field).await.map_err(bad)?;
            }
        }
    }
    let archive = archive.ok_or_else(|| {
        bad(Refused::new(
            "workspace_import.no_file",
            "Choose a Nosdesk workspace export to upload (form field \"archive\").",
        ))
    })?;
    Ok(Upload {
        archive,
        password,
        regenerate_uuids,
    })
}

/// 409 naming the workspace's live import.
fn already_running(live: &WorkspaceImportJob) -> HttpResponse {
    refused(StatusCode::CONFLICT, jobs::in_progress(live))
}

async fn upload(scope: Scope, mut payload: Multipart) -> HttpResponse {
    let workspace_id = scope.workspace_id;
    let live = match in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
        repo::live(conn, workspace_id)
    })
    .await
    {
        Ok(live) => live,
        Err(e) => return internal(e),
    };
    if let Some(live) = live {
        return already_running(&live);
    }
    let upload = match read_upload(&mut payload).await {
        Ok(upload) => upload,
        Err((status, r)) => return refused(status, r),
    };
    let parked = jobs::park(
        &scope.rt,
        &scope.origin,
        workspace_id,
        jobs::Upload {
            path: upload.archive.path(),
            name: upload.archive.filename.clone(),
            size: upload.archive.size,
            password_given: upload.password.is_some(),
            regenerate_uuids: upload.regenerate_uuids,
        },
    )
    .await;
    drop(upload.archive);
    let job = match parked {
        Ok(job) => job,
        Err(ParkError::InProgress(live)) => return already_running(&live),
        Err(ParkError::NotStored) => {
            return errors::internal_with_code("The upload couldn't be stored.", "upload_not_saved")
        }
        Err(ParkError::Failed(e)) => return internal(e),
    };
    if job.state == CHECKING {
        jobs::spawn_check(
            scope.rt.clone(),
            scope.origin.clone(),
            workspace_id,
            job.id,
            upload.password,
            upload.regenerate_uuids,
        );
    }
    HttpResponse::Accepted().json(jobs::view(&job))
}

async fn check(scope: Scope, id: Uuid, body: ActionBody) -> HttpResponse {
    let job = match load(&scope, id).await {
        Ok(job) => job,
        Err(e) => return e.error_response(),
    };
    if job.archive_key.is_none() {
        return refused(
            StatusCode::CONFLICT,
            Refused::new(
                "workspace_import.archive_missing",
                "The uploaded export is no longer stored. Upload it again.",
            ),
        );
    }
    let password = body.password.filter(|p| !p.is_empty());
    if job.sealed && password.is_none() {
        return refused(StatusCode::BAD_REQUEST, jobs::password_required());
    }
    let regenerate = body.regenerate_uuids.unwrap_or(job.regenerate_uuids);
    let job = match move_job(
        &scope,
        id,
        &[UPLOADED, PREVIEWED],
        WorkspaceImportJobChange {
            state: Some(CHECKING.into()),
            regenerate_uuids: Some(regenerate),
            error: Some(None),
            ..Default::default()
        },
    )
    .await
    {
        Ok(job) => job,
        Err(e) => return e.error_response(),
    };
    jobs::spawn_check(
        scope.rt.clone(),
        scope.origin.clone(),
        scope.workspace_id,
        id,
        password,
        regenerate,
    );
    HttpResponse::Accepted().json(jobs::view(&job))
}

async fn confirm(scope: Scope, id: Uuid, body: ActionBody) -> HttpResponse {
    let job = match load(&scope, id).await {
        Ok(job) => job,
        Err(e) => return e.error_response(),
    };
    let password = body.password.filter(|p| !p.is_empty());
    if job.state == PREVIEWED && job.sealed && password.is_none() {
        return refused(StatusCode::BAD_REQUEST, jobs::password_required());
    }
    let job = match move_job(&scope, id, &[PREVIEWED], jobs::confirming(&scope.origin)).await {
        Ok(job) => job,
        Err(e) => return e.error_response(),
    };
    jobs::spawn_import(
        scope.rt.clone(),
        scope.origin.clone(),
        scope.workspace_id,
        id,
        password,
        false,
    );
    HttpResponse::Accepted().json(jobs::view(&job))
}

async fn retry_files(scope: Scope, id: Uuid, body: ActionBody) -> HttpResponse {
    let job = match load(&scope, id).await {
        Ok(job) => job,
        Err(e) => return e.error_response(),
    };
    if jobs::actions(&job)["retry_files"] != true {
        return refused(
            StatusCode::CONFLICT,
            Refused::new(
                "workspace_import.no_files_to_retry",
                "This import has no files left to restore.",
            )
            .arg("state", job.state),
        );
    }
    let password = body.password.filter(|p| !p.is_empty());
    if job.sealed && password.is_none() {
        return refused(StatusCode::BAD_REQUEST, jobs::password_required());
    }
    let job = match move_job(
        &scope,
        id,
        &[FAILED],
        WorkspaceImportJobChange {
            state: Some(IMPORTING.into()),
            stage: Some(Some("files".into())),
            finished_at: Some(None),
            ..Default::default()
        },
    )
    .await
    {
        Ok(job) => job,
        Err(e) => return e.error_response(),
    };
    jobs::spawn_import(
        scope.rt.clone(),
        scope.origin.clone(),
        scope.workspace_id,
        id,
        password,
        true,
    );
    HttpResponse::Accepted().json(jobs::view(&job))
}

async fn reset(scope: Scope, id: Uuid) -> HttpResponse {
    match jobs::reset(
        &scope.rt,
        &scope.origin,
        scope.workspace_id,
        id,
        scope.mode(),
        scope.seed_author(),
    )
    .await
    {
        Ok(job) => HttpResponse::Ok().json(jobs::view(&job)),
        Err(ResetError::NotFound) => not_found(id),
        Err(ResetError::Refused { status, refused: r }) => refused(
            StatusCode::from_u16(status).unwrap_or(StatusCode::CONFLICT),
            r,
        ),
        Err(ResetError::Failed(e)) => internal(e),
    }
}

// ---------------------------------------------------------------------------
// Tenant routes
// ---------------------------------------------------------------------------

/// `GET /api/workspace/imports`: the workspace's imports, newest first.
pub async fn tenant_list(req: HttpRequest, pool: web::Data<Pool>) -> HttpResponse {
    match tenant_scope(&req, &pool) {
        Ok(scope) => list(scope).await,
        Err(e) => e.error_response(),
    }
}

/// `GET /api/workspace/imports/latest`: the newest import, or `null`.
pub async fn tenant_latest(req: HttpRequest, pool: web::Data<Pool>) -> HttpResponse {
    match tenant_scope(&req, &pool) {
        Ok(scope) => latest(scope).await,
        Err(e) => e.error_response(),
    }
}

/// `POST /api/workspace/imports` (multipart `archive`, optional `password`):
/// park the export and start checking it. 202 with the job.
pub async fn tenant_upload(
    req: HttpRequest,
    pool: web::Data<Pool>,
    payload: Multipart,
) -> HttpResponse {
    match tenant_scope(&req, &pool) {
        Ok(scope) => upload(scope, payload).await,
        Err(e) => e.error_response(),
    }
}

/// `GET /api/workspace/imports/{job}`.
pub async fn tenant_get(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> HttpResponse {
    let scope = match tenant_scope(&req, &pool) {
        Ok(scope) => scope,
        Err(e) => return e.error_response(),
    };
    let id = path.into_inner();
    let job = match load(&scope, id).await {
        Ok(job) => job,
        Err(e) => return e.error_response(),
    };
    let mut view = jobs::view(&job);
    if job.applied_at.is_some() && job.pruned_at.is_none() {
        let staff = jobs::created_staff(&job);
        if let Ok(waiting) = in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
            sign_in::waiting(conn, &staff)
        })
        .await
        {
            view["sign_in"] = serde_json::json!({
                "pending": waiting.len(),
                "invited": waiting.iter().filter(|w| w.invited).count(),
                "email_ready": sign_in::email_ready(),
            });
        }
    }
    HttpResponse::Ok().json(view)
}

/// `POST /api/workspace/imports/{job}/check` `{password?, regenerate_uuids?}`.
pub async fn tenant_check(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: Option<web::Json<ActionBody>>,
) -> HttpResponse {
    match tenant_scope(&req, &pool) {
        Ok(scope) => {
            check(
                scope,
                path.into_inner(),
                body.map(|b| b.into_inner()).unwrap_or_default(),
            )
            .await
        }
        Err(e) => e.error_response(),
    }
}

/// `POST /api/workspace/imports/{job}/confirm` `{password?}`: import.
pub async fn tenant_confirm(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: Option<web::Json<ActionBody>>,
) -> HttpResponse {
    match tenant_scope(&req, &pool) {
        Ok(scope) => {
            confirm(
                scope,
                path.into_inner(),
                body.map(|b| b.into_inner()).unwrap_or_default(),
            )
            .await
        }
        Err(e) => e.error_response(),
    }
}

/// `POST /api/workspace/imports/{job}/reset`.
pub async fn tenant_reset(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> HttpResponse {
    match tenant_scope(&req, &pool) {
        Ok(scope) => reset(scope, path.into_inner()).await,
        Err(e) => e.error_response(),
    }
}

/// `POST /api/workspace/imports/{job}/retry-files` `{password?}`.
pub async fn tenant_retry_files(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: Option<web::Json<ActionBody>>,
) -> HttpResponse {
    match tenant_scope(&req, &pool) {
        Ok(scope) => {
            retry_files(
                scope,
                path.into_inner(),
                body.map(|b| b.into_inner()).unwrap_or_default(),
            )
            .await
        }
        Err(e) => e.error_response(),
    }
}

/// `POST /api/workspace/imports/{job}/invite` `{user_uuids?}`: an invitation
/// email, the one Resend invitation sends, to each staff member the import
/// created who can't sign in yet and holds no open invitation. Self-hosted
/// with email set up only. Answers how many were sent and the sign-in line.
pub async fn tenant_invite(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: Option<web::Json<InviteBody>>,
) -> HttpResponse {
    let scope = match tenant_scope(&req, &pool) {
        Ok(scope) => scope,
        Err(e) => return e.error_response(),
    };
    let id = path.into_inner();
    let job = match load(&scope, id).await {
        Ok(job) => job,
        Err(e) => return e.error_response(),
    };
    if jobs::actions(&job)["invite"] != true {
        return errors::conflict_with_code(
            "This import didn't create anyone to invite.",
            "import_not_committed",
        );
    }
    if !crate::middleware::workspace_context::local_credentials_permitted() {
        return errors::conflict_with_code(
            "People here sign in with the workspace's sign-in provider, so there are no invites to send.",
            "import_invites_not_offered",
        );
    }
    if !sign_in::email_ready() {
        return errors::conflict_with_code(
            "Email isn't set up, so invites can't be sent.",
            "import_invites_email_off",
        );
    }
    let mut staff = jobs::created_staff(&job);
    if let Some(only) = body.and_then(|b| b.into_inner().user_uuids) {
        staff.retain(|u| only.contains(u));
    }
    let owner = scope.origin.user();
    let checked = in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
        let people: Vec<sign_in::Waiting> = sign_in::waiting(conn, &staff)?
            .into_iter()
            .filter(|w| !w.invited)
            .collect();
        let mut uuids: Vec<Uuid> = people.iter().map(|p| p.user).collect();
        uuids.extend(owner);
        let names = crate::handlers::imports::names_of(conn, uuids)?;
        Ok((people, names))
    })
    .await;
    let (people, names) = match checked {
        Ok(checked) => checked,
        Err(e) => return internal(e),
    };
    if people.is_empty() {
        return errors::conflict_with_code(
            "Everyone this import created can sign in or has an open invitation.",
            "import_nothing_to_invite",
        );
    }
    let admin_name = owner
        .and_then(|o| names.get(&o).cloned())
        .unwrap_or_else(|| "An administrator".to_string());

    // The invitation helper takes a connection pinned to the request's
    // workspace, as Resend invitation does.
    let mut conn = match crate::handlers::helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return internal(e),
    };
    crate::handlers::helpers::pin_request_workspace(&req, &mut conn);
    let mut sent = 0usize;
    let mut failed = 0usize;
    for person in &people {
        let name = names.get(&person.user).map_or("", String::as_str);
        match crate::handlers::users::send_user_invitation(
            &mut conn,
            &req,
            person.user,
            &person.email,
            name,
            &admin_name,
        )
        .await
        {
            crate::handlers::users::SendInvitationResult::Success
            | crate::handlers::users::SendInvitationResult::Capped => sent += 1,
            other => {
                tracing::error!(id = %id, error_kind = %other, "failed to send a workspace import invite");
                failed += 1;
            }
        }
    }
    drop(conn);
    tracing::info!(id = %id, count = sent, failed, "workspace import invites sent");
    let staff = jobs::created_staff(&job);
    let sign_in = in_workspace(&scope.rt.pool, scope.actor(), move |conn| {
        sign_in::waiting(conn, &staff)
    })
    .await
    .map(|w| {
        serde_json::json!({
            "pending": w.len(),
            "invited": w.iter().filter(|w| w.invited).count(),
            "email_ready": true,
        })
    })
    .unwrap_or(serde_json::Value::Null);
    HttpResponse::Ok().json(serde_json::json!({
        "sent": sent,
        "failed": failed,
        "sign_in": sign_in,
    }))
}

// ---------------------------------------------------------------------------
// Platform-admin routes
// ---------------------------------------------------------------------------

/// `GET /api/admin/workspaces/{id}/imports`.
pub async fn admin_list(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> HttpResponse {
    match admin_scope(&req, &pool, path.into_inner()).await {
        Ok(scope) => list(scope).await,
        Err(e) => e.error_response(),
    }
}

/// `POST /api/admin/workspaces/{id}/imports` (multipart `archive`, optional
/// `password`, `regenerate_uuids`).
pub async fn admin_upload(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    payload: Multipart,
) -> HttpResponse {
    match admin_scope(&req, &pool, path.into_inner()).await {
        Ok(scope) => upload(scope, payload).await,
        Err(e) => e.error_response(),
    }
}

/// `GET /api/admin/workspaces/{id}/imports/{job}`.
pub async fn admin_get(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(i32, Uuid)>,
) -> HttpResponse {
    let (workspace_id, id) = path.into_inner();
    match admin_scope(&req, &pool, workspace_id).await {
        Ok(scope) => get(scope, id).await,
        Err(e) => e.error_response(),
    }
}

/// `POST /api/admin/workspaces/{id}/imports/{job}/check`.
pub async fn admin_check(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(i32, Uuid)>,
    body: Option<web::Json<ActionBody>>,
) -> HttpResponse {
    let (workspace_id, id) = path.into_inner();
    match admin_scope(&req, &pool, workspace_id).await {
        Ok(scope) => check(scope, id, body.map(|b| b.into_inner()).unwrap_or_default()).await,
        Err(e) => e.error_response(),
    }
}

/// `POST /api/admin/workspaces/{id}/imports/{job}/confirm`.
pub async fn admin_confirm(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(i32, Uuid)>,
    body: Option<web::Json<ActionBody>>,
) -> HttpResponse {
    let (workspace_id, id) = path.into_inner();
    match admin_scope(&req, &pool, workspace_id).await {
        Ok(scope) => confirm(scope, id, body.map(|b| b.into_inner()).unwrap_or_default()).await,
        Err(e) => e.error_response(),
    }
}

/// `POST /api/admin/workspaces/{id}/imports/{job}/reset`.
pub async fn admin_reset(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(i32, Uuid)>,
) -> HttpResponse {
    let (workspace_id, id) = path.into_inner();
    match admin_scope(&req, &pool, workspace_id).await {
        Ok(scope) => reset(scope, id).await,
        Err(e) => e.error_response(),
    }
}

/// `POST /api/admin/workspaces/{id}/imports/{job}/retry-files`.
pub async fn admin_retry_files(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(i32, Uuid)>,
    body: Option<web::Json<ActionBody>>,
) -> HttpResponse {
    let (workspace_id, id) = path.into_inner();
    match admin_scope(&req, &pool, workspace_id).await {
        Ok(scope) => retry_files(scope, id, body.map(|b| b.into_inner()).unwrap_or_default()).await,
        Err(e) => e.error_response(),
    }
}
