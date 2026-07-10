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

use crate::db::Pool;
use crate::extractors::PlatformConn;
use crate::handlers::errors;
use crate::models::{NewWorkspace, Workspace, WorkspaceMember, WorkspaceRole};
use crate::repository::workspaces::{self, CreateWorkspaceError, UpdateMembershipRoleResult};
use crate::services::search::{indexing_tasks, SearchService};
use crate::utils::rbac;
use crate::utils::workspace_slug::validate_slug;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/workspaces",
        web::get().to(crate::handlers::admin_workspaces::list_workspaces),
    )
    .route(
        "/admin/workspaces",
        web::post().to(crate::handlers::admin_workspaces::create_workspace),
    )
    .route(
        "/admin/edition",
        web::get().to(crate::handlers::admin_workspaces::get_edition),
    )
    .route(
        "/admin/workspaces/{id}",
        web::patch().to(crate::handlers::admin_workspaces::rename_workspace),
    )
    .route(
        "/admin/workspaces/{id}",
        web::delete().to(crate::handlers::admin_workspaces::hard_delete_workspace),
    )
    .route(
        "/admin/workspaces/{id}/archive",
        web::post().to(crate::handlers::admin_workspaces::archive_workspace),
    )
    .route(
        "/admin/workspaces/{id}/restore",
        web::post().to(crate::handlers::admin_workspaces::restore_workspace),
    )
    // Workspace membership (admin only, Phase 4 W3).
    // Cross-tenant membership management for the
    // platform admin. Workspace-admin self-service
    // member management is a separate route under
    // /api/workspaces/{id}/members (later workstream).
    .route(
        "/admin/workspaces/{id}/members",
        web::get().to(crate::handlers::admin_workspaces::list_members),
    )
    .route(
        "/admin/workspaces/{id}/members",
        web::post().to(crate::handlers::admin_workspaces::add_member),
    )
    .route(
        "/admin/workspaces/{id}/members/{user_uuid}",
        web::patch().to(crate::handlers::admin_workspaces::update_member_role),
    )
    .route(
        "/admin/workspaces/{id}/members/{user_uuid}",
        web::delete().to(crate::handlers::admin_workspaces::remove_member),
    )
    // Caller's own workspace memberships — backs
    // the frontend workspace switcher. Authenticated,
    // no admin gate. Phase 4 W3.
    .route(
        "/me/workspaces",
        web::get().to(crate::handlers::admin_workspaces::list_my_workspaces),
    )
    // Tenant self-serve member management for the caller's
    // OWN workspace (context-scoped, no id in the path).
    // Workspace-admin gated; distinct from the platform-
    // admin operator console at /admin/workspaces/{id}/members.
    // Phase 4 W3 / P1.3.
    .route(
        "/workspace/members",
        web::get().to(crate::handlers::workspace_members::list_members),
    )
    .route(
        "/workspace/members/{user_uuid}",
        web::patch().to(crate::handlers::workspace_members::update_member_role),
    )
    .route(
        "/workspace/members/{user_uuid}",
        web::delete().to(crate::handlers::workspace_members::remove_member),
    );
}

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

/// Edition + workspace-limit summary for the admin UI. Lets
/// AdminWorkspacesView reflect the self-hosted single-workspace cap
/// (disable Create + show an upgrade note) rather than hard-coding it
/// client-side. The server gate in `create_workspace` is the real
/// enforcement; this is purely for display.
pub async fn get_edition(req: HttpRequest, mut pc: PlatformConn) -> impl Responder {
    if let Err(resp) = rbac::require_platform_admin(&req) {
        return resp;
    }
    let edition = crate::license::current();
    let self_hosted = crate::middleware::DeploymentMode::current()
        == crate::middleware::DeploymentMode::SelfHosted;
    let active = pc.run(workspaces::count_active_workspaces).unwrap_or(0);
    let max = edition.max_workspaces();
    // Gated on the edition's workspace cap, not the deployment mode (see
    // license::workspace_creation_allowed).
    let can_create = crate::license::workspace_creation_allowed(edition, active as u64);
    HttpResponse::Ok().json(serde_json::json!({
        "edition": edition.name(),
        "self_hosted": self_hosted,
        "max_workspaces": max,
        "active_workspaces": active,
        "can_create_workspace": can_create,
        "license": edition.license().map(|l| serde_json::json!({
            "licensee": l.licensee,
            "license_id": l.license_id,
            "max_workspaces": l.max_workspaces,
            "expires_at": l.expires_at,
        })),
    }))
}

#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub slug: String,
    pub name: String,
}

pub async fn create_workspace(
    req: HttpRequest,
    pool: web::Data<Pool>,
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

    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!(error = ?e, "admin/workspaces pool checkout failed");
            return errors::internal("Failed to create workspace");
        }
    };

    // License gate. Capped at the edition's workspace limit (Community = 1;
    // Enterprise = the licensed count). Gated on the edition, NOT on
    // NOSDESK_DEPLOYMENT_MODE: keying it on the mode let a self-hoster flip to
    // `hosted` and skip the cap with no license. Hosted deployments provision
    // through the control-plane /api/internal surface (uncapped, authoritative
    // for its own billing), never this self-serve route, so the cap applies
    // here in every mode. See license::workspace_creation_allowed.
    let edition = crate::license::current();
    let max = edition.max_workspaces();
    let active = match workspaces::count_active_workspaces(&mut conn) {
        Ok(n) => n,
        Err(e) => {
            error!(error = ?e, "admin/workspaces license-gate count failed");
            return errors::internal("Failed to create workspace");
        }
    };
    if !crate::license::workspace_creation_allowed(edition, active as u64) {
        warn!(
            active,
            max,
            edition = edition.name(),
            "admin/workspaces blocked by license cap"
        );
        let message = if edition.is_enterprise() {
            format!("Your Enterprise license permits {max} active workspace(s); archive one before creating another.")
        } else {
            format!(
                "The Community edition is limited to {max} active workspace(s). \
                 An Enterprise license is required to create more."
            )
        };
        return HttpResponse::PaymentRequired().json(serde_json::json!({
            "error": "license_required",
            "message": message,
            "edition": edition.name(),
            "max_workspaces": max,
            "active_workspaces": active,
        }));
    }

    let record = NewWorkspace {
        uuid: Uuid::now_v7(),
        slug: slug.clone(),
        name: name.clone(),
        // Operator-provisioned workspaces are uncapped; the seat cap is a
        // self-serve-trial guardrail set by the control plane.
        seat_limit: None,
    };

    // Create the workspace and seed its default content (workflow states, SLA,
    // categories, asset kinds) in one bypass-context transaction, mirroring the
    // control-plane provisioning path: the `workspaces` insert needs the
    // nosdesk_admin BYPASSRLS role, then the session pins to the new workspace
    // so each seeded row's workspace_id + audit context resolves to it. A seed
    // failure rolls the workspace row back rather than leaving it unusable.
    let provision_actor = crate::sync::actor::ActorContext::system("workspace:provision");
    let result = crate::sync::session::with_actor_bypass_context::<Workspace, CreateWorkspaceError>(
        &mut conn,
        &provision_actor,
        |c| {
            let ws = workspaces::create_workspace(c, &record)?;
            let seed_actor = crate::sync::actor::ActorContext::system("workspace:provision")
                .with_workspace(ws.id);
            crate::sync::session::set_actor(c, &seed_actor)?;
            crate::services::seed::seed_workspace_defaults(c, None)?;
            Ok(ws)
        },
    );

    match result {
        Ok(ws) => {
            info!(workspace_uuid = %ws.uuid, workspace_id = ws.id, slug = %ws.slug, "admin/workspaces created + seeded");
            HttpResponse::Created().json(WorkspaceSummary::from(ws))
        }
        Err(CreateWorkspaceError::SlugTaken) => {
            warn!(slug = %slug, "admin/workspaces slug collision");
            HttpResponse::Conflict().json(serde_json::json!({
                "error": "slug_taken",
                "message": format!("slug '{slug}' is unavailable, please choose another"),
            }))
        }
        Err(CreateWorkspaceError::Db(e)) => {
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
