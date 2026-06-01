//! Platform-admin handlers for workspace lifecycle ops (Phase 4 W1).
//!
//! Mounted under `/api/admin/workspaces`. Distinct from the
//! `/api/internal/v1/workspaces/...` M5 surface, which is keyed on a
//! platform-scoped API token from the control plane. This one is keyed
//! on a logged-in admin user's session/JWT.
//!
//! Auth gate is currently `require_admin` (global `users.role = admin`).
//! W2 will rename `users.role` to `users.platform_role` and swap this
//! gate to `require_platform_admin`. The handlers themselves don't
//! change at that point.
//!
//! Every write path uses [`PlatformConn`](crate::extractors::PlatformConn)
//! so the `nosdesk_admin` BYPASSRLS role handles the cross-tenant
//! UPDATE / DELETE; tenant RLS is meaningless for workspaces lifecycle
//! anyway because `workspaces` itself doesn't carry a workspace_id.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::extractors::PlatformConn;
use crate::handlers::errors;
use crate::models::{NewWorkspace, Workspace};
use crate::repository::workspaces::{self, CreateWorkspaceError};
use crate::utils::rbac;
use crate::utils::workspace_slug::validate_slug;

#[derive(Debug, Serialize)]
struct WorkspaceSummary {
    id: i32,
    uuid: Uuid,
    slug: String,
    name: String,
    plan: String,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
    custom_domain: Option<String>,
}

impl From<Workspace> for WorkspaceSummary {
    fn from(w: Workspace) -> Self {
        Self {
            id: w.id,
            uuid: w.uuid,
            slug: w.slug,
            name: w.name,
            plan: w.plan,
            archived_at: w.archived_at,
            custom_domain: w.custom_domain,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ListWorkspacesQuery {
    /// Set `?include_archived=true` to see the tombstoned set as well.
    /// Defaults to false (active workspaces only) to keep the common
    /// admin landing view focused.
    #[serde(default)]
    pub include_archived: bool,
}

pub async fn list_workspaces(
    req: HttpRequest,
    mut pc: PlatformConn,
    query: web::Query<ListWorkspacesQuery>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    match pc.run(|conn| workspaces::list_workspaces(conn, query.include_archived)) {
        Ok(rows) => {
            let body: Vec<WorkspaceSummary> = rows.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(body)
        }
        Err(e) => {
            error!(error = ?e, "admin/workspaces list failed");
            errors::internal("Failed to list workspaces")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub slug: String,
    pub name: String,
}

pub async fn create_workspace(
    req: HttpRequest,
    mut pc: PlatformConn,
    body: web::Json<CreateWorkspaceRequest>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let CreateWorkspaceRequest { slug, name } = body.into_inner();

    if let Err(e) = validate_slug(&slug) {
        return errors::bad_request(e.as_message());
    }
    if name.trim().is_empty() {
        return errors::bad_request("name must not be empty");
    }

    let record = NewWorkspace {
        uuid: Uuid::now_v7(),
        slug: slug.clone(),
        name: name.clone(),
    };

    match pc.run(|conn| match workspaces::create_workspace(conn, &record) {
        Ok(ws) => Ok(Ok(ws)),
        Err(CreateWorkspaceError::SlugTaken) => Ok(Err(CreateWorkspaceError::SlugTaken)),
        Err(CreateWorkspaceError::Db(e)) => Err(e),
    }) {
        Ok(Ok(ws)) => {
            info!(workspace_uuid = %ws.uuid, workspace_id = ws.id, slug = %ws.slug, "admin/workspaces created");
            HttpResponse::Created().json(WorkspaceSummary::from(ws))
        }
        Ok(Err(CreateWorkspaceError::SlugTaken)) => {
            warn!(slug = %slug, "admin/workspaces slug collision");
            HttpResponse::Conflict().json(serde_json::json!({
                "error": "slug_taken",
                "message": format!("slug '{slug}' is unavailable, please choose another"),
            }))
        }
        Ok(Err(CreateWorkspaceError::Db(_))) => unreachable!("DB errors are mapped to Err above"),
        Err(e) => {
            error!(error = ?e, slug = %slug, "admin/workspaces create failed");
            errors::internal("Failed to create workspace")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RenameWorkspaceRequest {
    pub name: String,
}

pub async fn rename_workspace(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<i32>,
    body: web::Json<RenameWorkspaceRequest>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let id = path.into_inner();
    let name = body.into_inner().name;
    if name.trim().is_empty() {
        return errors::bad_request("name must not be empty");
    }

    match pc.run(|conn| workspaces::rename_workspace(conn, id, &name)) {
        Ok(Some(ws)) => {
            info!(workspace_id = ws.id, name = %name, "admin/workspaces renamed");
            HttpResponse::Ok().json(WorkspaceSummary::from(ws))
        }
        Ok(None) => errors::not_found_msg(format!("workspace id={id} not found")),
        Err(e) => {
            error!(error = ?e, workspace_id = id, "admin/workspaces rename failed");
            errors::internal("Failed to rename workspace")
        }
    }
}

pub async fn archive_workspace(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let id = path.into_inner();
    match pc.run(|conn| workspaces::archive_workspace(conn, id)) {
        Ok(Some(ws)) => {
            info!(workspace_id = ws.id, slug = %ws.slug, "admin/workspaces archived");
            HttpResponse::Ok().json(WorkspaceSummary::from(ws))
        }
        Ok(None) => errors::not_found_msg(format!("workspace id={id} not found")),
        Err(e) => {
            error!(error = ?e, workspace_id = id, "admin/workspaces archive failed");
            errors::internal("Failed to archive workspace")
        }
    }
}

pub async fn restore_workspace(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let id = path.into_inner();
    match pc.run(|conn| workspaces::restore_workspace(conn, id)) {
        Ok(Some(ws)) => {
            info!(workspace_id = ws.id, slug = %ws.slug, "admin/workspaces restored");
            HttpResponse::Ok().json(WorkspaceSummary::from(ws))
        }
        Ok(None) => errors::not_found_msg(format!("workspace id={id} not found")),
        Err(e) => {
            error!(error = ?e, workspace_id = id, "admin/workspaces restore failed");
            errors::internal("Failed to restore workspace")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct HardDeleteQuery {
    /// Must equal the workspace's slug. Mirrors the user-purge
    /// handler's `?confirm=` shape: a deliberate "type the slug to
    /// confirm" gate so a mis-clicked DELETE can't tombstone a
    /// workspace's last 30 days of activity.
    pub confirm: String,
}

pub async fn hard_delete_workspace(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<i32>,
    query: web::Query<HardDeleteQuery>,
) -> impl Responder {
    if let Err(resp) = rbac::require_admin(&req) {
        return resp;
    }
    let id = path.into_inner();
    let confirm = query.into_inner().confirm;

    // Resolve the workspace first so we can both confirm-match and
    // return a useful 404 / 409. include_archived=true since active
    // rows are rejected by the repo layer's archived_at filter
    // anyway, but we want the same "is this row real?" probe shape
    // either way.
    let lookup = pc.run(|conn| workspaces::list_workspaces(conn, true));
    let ws = match lookup {
        Ok(rows) => match rows.into_iter().find(|w| w.id == id) {
            Some(w) => w,
            None => return errors::not_found_msg(format!("workspace id={id} not found")),
        },
        Err(e) => {
            error!(error = ?e, workspace_id = id, "admin/workspaces hard_delete lookup failed");
            return errors::internal("Workspace lookup failed");
        }
    };

    if confirm != ws.slug {
        return errors::bad_request(
            "confirm query parameter must match the workspace's slug exactly",
        );
    }
    if ws.archived_at.is_none() {
        return HttpResponse::Conflict().json(serde_json::json!({
            "error": "not_archived",
            "message": "workspace must be archived before hard delete; call POST /archive first",
        }));
    }

    // Cutoff is NOW: hard_delete_workspace's WHERE clause enforces
    // `archived_at <= cutoff`. The grace window is enforced by the
    // scheduled job, not by this manual handler — operators sometimes
    // need to clear a workspace immediately (test workspaces, GDPR
    // erasure requests).
    let cutoff = chrono::Utc::now();
    match pc.run(|conn| workspaces::hard_delete_workspace(conn, id, cutoff)) {
        Ok(0) => HttpResponse::Conflict().json(serde_json::json!({
            "error": "not_eligible",
            "message": "workspace state changed during request; refresh and retry",
        })),
        Ok(_) => {
            info!(workspace_id = id, slug = %ws.slug, "admin/workspaces hard-deleted");
            HttpResponse::NoContent().finish()
        }
        Err(e) => {
            error!(error = ?e, workspace_id = id, "admin/workspaces hard_delete failed");
            errors::internal("Failed to hard-delete workspace")
        }
    }
}
