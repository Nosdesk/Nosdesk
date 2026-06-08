//! Platform-admin handlers for workspace lifecycle ops (Phase 4 W1).
//!
//! Mounted under `/api/admin/workspaces`. Distinct from the
//! `/api/internal/v1/workspaces/...` M5 surface, which is keyed on a
//! platform-scoped API token from the control plane. This one is keyed
//! on a logged-in admin user's session/JWT.
//!
//! Auth gate is `require_platform_admin`, the W2 successor to the
//! legacy `require_admin` that read the now-deprecated `users.role`
//! column. The handlers themselves don't otherwise differ.
//!
//! Every write path uses [`PlatformConn`](crate::extractors::PlatformConn)
//! so the `nosdesk_admin` BYPASSRLS role handles the cross-tenant
//! UPDATE / DELETE; tenant RLS is meaningless for workspaces lifecycle
//! anyway because `workspaces` itself doesn't carry a workspace_id.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

use std::sync::Arc;

use crate::extractors::PlatformConn;
use crate::handlers::errors;
use crate::models::{NewWorkspace, Workspace, WorkspaceMember, WorkspaceRole};
use crate::repository::workspaces::{self, CreateWorkspaceError, UpdateMembershipRoleResult};
use crate::services::search::{indexing_tasks, SearchService};
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
    if let Err(resp) = rbac::require_platform_admin(&req) {
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
    if let Err(resp) = rbac::require_platform_admin(&req) {
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
    if let Err(resp) = rbac::require_platform_admin(&req) {
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
    if let Err(resp) = rbac::require_platform_admin(&req) {
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
    if let Err(resp) = rbac::require_platform_admin(&req) {
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
    if let Err(resp) = rbac::require_platform_admin(&req) {
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

// =====================================================================
// Phase 4 W3: workspace membership management
// =====================================================================
//
// Mounted under /api/admin/workspaces/{id}/members. Gated on
// require_platform_admin; workspace-admin-side member management
// lives on a separate route under /api/workspaces/{id}/members in a
// later workstream.

#[derive(Debug, Serialize)]
struct MemberSummary {
    workspace_id: i32,
    user_uuid: Uuid,
    role: String,
    invited_at: chrono::DateTime<chrono::Utc>,
    accepted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<WorkspaceMember> for MemberSummary {
    fn from(m: WorkspaceMember) -> Self {
        Self {
            workspace_id: m.workspace_id,
            user_uuid: m.user_uuid,
            role: m.role,
            invited_at: m.invited_at,
            accepted_at: m.accepted_at,
        }
    }
}

pub async fn list_members(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(resp) = rbac::require_platform_admin(&req) {
        return resp;
    }
    let workspace_id = path.into_inner();
    match pc.run(|conn| workspaces::list_workspace_members(conn, workspace_id)) {
        Ok(rows) => {
            let body: Vec<MemberSummary> = rows.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(body)
        }
        Err(e) => {
            error!(error = ?e, workspace_id, "admin/workspaces members list failed");
            errors::internal("Failed to list members")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct AddMemberRequest {
    pub user_uuid: Uuid,
    pub role: String,
}

fn validate_workspace_role(role: &str) -> Option<WorkspaceRole> {
    match role {
        "owner" => Some(WorkspaceRole::Owner),
        "admin" => Some(WorkspaceRole::Admin),
        "agent" => Some(WorkspaceRole::Agent),
        "member" => Some(WorkspaceRole::Member),
        _ => None,
    }
}

pub async fn add_member(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<i32>,
    body: web::Json<AddMemberRequest>,
    // Best-effort search reindex: optional so the membership op doesn't
    // hard-depend on the search subsystem (and test apps need not wire it).
    search_service: Option<web::Data<Arc<SearchService>>>,
) -> impl Responder {
    if let Err(resp) = rbac::require_platform_admin(&req) {
        return resp;
    }
    let workspace_id = path.into_inner();
    let AddMemberRequest { user_uuid, role } = body.into_inner();

    let parsed_role = match validate_workspace_role(&role) {
        Some(r) => r,
        None => {
            return errors::bad_request("role must be one of: owner, admin, agent, member");
        }
    };

    // Confirm the user exists; otherwise we'd silently fail an FK
    // check at INSERT time with a less-useful 500. Lookup uses
    // PlatformConn so BYPASSRLS sees rows in every workspace.
    let user_exists = pc.run(|conn| {
        use crate::schema::users;
        use diesel::dsl::count_star;
        users::table
            .filter(users::uuid.eq(user_uuid))
            .filter(users::deleted_at.is_null())
            .select(count_star())
            .get_result::<i64>(conn)
            .map(|n| n > 0)
    });
    match user_exists {
        Ok(true) => {}
        Ok(false) => {
            return errors::not_found_msg(format!("user {user_uuid} not found"));
        }
        Err(e) => {
            error!(error = ?e, %user_uuid, "admin/workspaces add_member user lookup failed");
            return errors::internal("User lookup failed");
        }
    }

    // Confirm the workspace exists (refuse to invite into an
    // archived workspace).
    let ws_lookup = pc.run(|conn| workspaces::find_by_id(conn, workspace_id));
    match ws_lookup {
        Ok(Some(_)) => {}
        Ok(None) => return errors::not_found_msg(format!("workspace id={workspace_id} not found")),
        Err(e) => {
            error!(error = ?e, workspace_id, "admin/workspaces add_member workspace lookup failed");
            return errors::internal("Workspace lookup failed");
        }
    }

    match pc
        .run(|conn| workspaces::add_membership(conn, workspace_id, user_uuid, parsed_role.as_str()))
    {
        Ok(n) if n > 0 => {
            info!(workspace_id, %user_uuid, role = %parsed_role.as_str(), "admin/workspaces member added");
            // The user's search doc carries one workspace tag per
            // membership; refresh it so the user becomes searchable in
            // this workspace (and stays gated out of others).
            if let Some(search_service) = &search_service {
                indexing_tasks::spawn_reindex_user(search_service.get_ref().clone(), user_uuid);
            }
            HttpResponse::Created().json(serde_json::json!({
                "workspace_id": workspace_id,
                "user_uuid": user_uuid,
                "role": parsed_role.as_str(),
            }))
        }
        Ok(_) => {
            // ON CONFLICT DO NOTHING fired — the membership row
            // already existed. Idempotent: return 200 with the
            // current state instead of 409 (consistent with how
            // every other "add to a set" admin op behaves here).
            HttpResponse::Ok().json(serde_json::json!({
                "workspace_id": workspace_id,
                "user_uuid": user_uuid,
                "status": "already_member",
            }))
        }
        Err(e) => {
            error!(error = ?e, workspace_id, %user_uuid, "admin/workspaces add_member failed");
            errors::internal("Failed to add member")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateMemberRoleRequest {
    pub role: String,
}

pub async fn update_member_role(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<(i32, Uuid)>,
    body: web::Json<UpdateMemberRoleRequest>,
) -> impl Responder {
    if let Err(resp) = rbac::require_platform_admin(&req) {
        return resp;
    }
    let (workspace_id, user_uuid) = path.into_inner();
    let new_role = body.into_inner().role;

    let parsed_role = match validate_workspace_role(&new_role) {
        Some(r) => r,
        None => {
            return errors::bad_request("role must be one of: owner, admin, agent, member");
        }
    };

    match pc.run(|conn| {
        workspaces::update_membership_role(conn, workspace_id, user_uuid, parsed_role.as_str())
    }) {
        Ok(UpdateMembershipRoleResult::Updated(m)) => {
            info!(workspace_id, %user_uuid, role = %parsed_role.as_str(), "admin/workspaces member role updated");
            HttpResponse::Ok().json(MemberSummary::from(m))
        }
        Ok(UpdateMembershipRoleResult::NotFound) => errors::not_found_msg(format!(
            "no membership row for user {user_uuid} in workspace {workspace_id}"
        )),
        Ok(UpdateMembershipRoleResult::LastOwner) => {
            HttpResponse::Conflict().json(serde_json::json!({
                "error": "last_owner",
                "message": "cannot demote the only owner; promote another member first",
            }))
        }
        Err(e) => {
            error!(error = ?e, workspace_id, %user_uuid, "admin/workspaces update_member_role failed");
            errors::internal("Failed to update member role")
        }
    }
}

pub async fn remove_member(
    req: HttpRequest,
    mut pc: PlatformConn,
    path: web::Path<(i32, Uuid)>,
    // Best-effort search reindex (see add_member).
    search_service: Option<web::Data<Arc<SearchService>>>,
) -> impl Responder {
    if let Err(resp) = rbac::require_platform_admin(&req) {
        return resp;
    }
    let (workspace_id, user_uuid) = path.into_inner();

    match pc.run(|conn| workspaces::remove_membership(conn, workspace_id, user_uuid)) {
        Ok(1) => {
            info!(workspace_id, %user_uuid, "admin/workspaces member removed");
            // Refresh the user's search doc so the now-removed workspace
            // tag drops off and they stop matching searches in it.
            if let Some(search_service) = &search_service {
                indexing_tasks::spawn_reindex_user(search_service.get_ref().clone(), user_uuid);
            }
            HttpResponse::NoContent().finish()
        }
        Ok(0) => {
            // Either the row didn't exist OR removal would have left
            // the workspace owner-less. Probe the membership table to
            // distinguish so the response matches the caller's reality.
            let probe = pc.run(|conn| workspaces::membership(conn, workspace_id, user_uuid));
            match probe {
                Ok(Some(row)) if row.role == "owner" => {
                    HttpResponse::Conflict().json(serde_json::json!({
                        "error": "last_owner",
                        "message": "cannot remove the only owner; promote another member first",
                    }))
                }
                Ok(None) => errors::not_found_msg(format!(
                    "no membership row for user {user_uuid} in workspace {workspace_id}"
                )),
                Ok(Some(_)) => {
                    // Shouldn't happen — non-owner rows can always be
                    // removed. Log and surface as 500 if it does.
                    error!(workspace_id, %user_uuid, "admin/workspaces remove_member returned 0 rows but row exists and isn't owner");
                    errors::internal("Inconsistent membership state")
                }
                Err(e) => {
                    error!(error = ?e, workspace_id, %user_uuid, "admin/workspaces remove_member probe failed");
                    errors::internal("Failed to remove member")
                }
            }
        }
        Ok(n) => {
            // Should never happen — composite PK guarantees at most one row.
            error!(workspace_id, %user_uuid, deleted = n, "admin/workspaces remove_member deleted unexpected row count");
            errors::internal("Inconsistent membership state")
        }
        Err(e) => {
            error!(error = ?e, workspace_id, %user_uuid, "admin/workspaces remove_member failed");
            errors::internal("Failed to remove member")
        }
    }
}

// =====================================================================
// /api/me/workspaces — caller's own memberships (no admin gate)
// =====================================================================

#[derive(Debug, Serialize)]
struct MyWorkspaceEntry {
    workspace_id: i32,
    workspace_uuid: Uuid,
    slug: String,
    name: String,
    custom_domain: Option<String>,
    role: String,
    invited_at: chrono::DateTime<chrono::Utc>,
    accepted_at: Option<chrono::DateTime<chrono::Utc>>,
}

pub async fn list_my_workspaces(req: HttpRequest, mut pc: PlatformConn) -> impl Responder {
    let claims = match rbac::require_auth(&req) {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => {
            return errors::bad_request("token subject is not a valid user identifier");
        }
    };

    match pc.run(|conn| workspaces::list_memberships_for_user(conn, user_uuid)) {
        Ok(rows) => {
            let body: Vec<MyWorkspaceEntry> = rows
                .into_iter()
                .map(|(m, w)| MyWorkspaceEntry {
                    workspace_id: w.id,
                    workspace_uuid: w.uuid,
                    slug: w.slug,
                    name: w.name,
                    custom_domain: w.custom_domain,
                    role: m.role,
                    invited_at: m.invited_at,
                    accepted_at: m.accepted_at,
                })
                .collect();
            HttpResponse::Ok().json(body)
        }
        Err(e) => {
            error!(error = ?e, %user_uuid, "me/workspaces list failed");
            errors::internal("Failed to load memberships")
        }
    }
}
