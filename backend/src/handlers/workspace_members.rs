//! Tenant-facing workspace member management.
//!
//! Mounted under `/api/workspace/members` (note: singular, no id in the
//! path). Every operation acts on the request's resolved
//! [`WorkspaceContext`] workspace, never an id from the path or body, so
//! a workspace admin can only ever manage their OWN workspace — there is
//! no id to point at someone else's. This is the deliberate difference
//! from the operator console at `/api/admin/workspaces/{id}/members`
//! (platform-admin, cross-tenant): re-gating those `{id}` routes on
//! workspace role would be unsafe, because `require_workspace_role`
//! resolves the caller's role in the CONTEXT workspace, not the path id.
//!
//! Authorization is tiered above the repository's last-owner guard:
//! owners may manage every role (including other owners); admins may only
//! manage agents and members, and may only assign agent/member. So an
//! admin can neither touch another admin/owner nor escalate anyone (or
//! themselves) to admin/owner. Inviting brand-new people is a separate
//! path (`POST /api/users`, already workspace-admin gated).
//!
//! `workspace_members` is a no-RLS meta-table and `nosdesk_app` lacks
//! UPDATE/DELETE on it, so writes run under a BYPASSRLS actor context
//! (like the operator handlers). Safety comes entirely from the gate
//! plus pinning every write to `WorkspaceContext.workspace_id`.

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::Pool;
use crate::extractors::WorkspaceContext;
use crate::handlers::errors;
use crate::models::{Claims, WorkspaceMember, WorkspaceRole};
use crate::repository::workspaces::{self, UpdateMembershipRoleResult};
use crate::services::search::{indexing_tasks, SearchService};
use crate::sync::actor::ActorContext;
use crate::sync::session::with_actor_bypass_context;
use crate::utils::rbac;

#[derive(Debug, Serialize)]
struct MemberView {
    user_uuid: Uuid,
    role: String,
    invited_at: chrono::DateTime<chrono::Utc>,
    accepted_at: Option<chrono::DateTime<chrono::Utc>>,
}

impl From<WorkspaceMember> for MemberView {
    fn from(m: WorkspaceMember) -> Self {
        Self {
            user_uuid: m.user_uuid,
            role: m.role,
            invited_at: m.invited_at,
            accepted_at: m.accepted_at,
        }
    }
}

/// Tiered authorization: which target roles a caller may act on. Owners
/// manage everyone; admins manage only roles strictly below admin
/// (agent, member). The last-owner guard in the repository still applies
/// on top of this for owners.
fn can_manage(caller: WorkspaceRole, target_current: WorkspaceRole) -> bool {
    match caller {
        WorkspaceRole::Owner => true,
        WorkspaceRole::Admin => target_current < WorkspaceRole::Admin,
        _ => false,
    }
}

/// Which roles a caller may assign. Same tier: an admin can only set
/// agent/member, never grant admin/owner (no self- or peer-escalation).
fn can_assign(caller: WorkspaceRole, new_role: WorkspaceRole) -> bool {
    match caller {
        WorkspaceRole::Owner => true,
        WorkspaceRole::Admin => new_role < WorkspaceRole::Admin,
        _ => false,
    }
}

fn parse_role(role: &str) -> Option<WorkspaceRole> {
    match role {
        "owner" => Some(WorkspaceRole::Owner),
        "admin" => Some(WorkspaceRole::Admin),
        "agent" => Some(WorkspaceRole::Agent),
        "member" => Some(WorkspaceRole::Member),
        _ => None,
    }
}

fn forbidden_tier() -> HttpResponse {
    errors::forbidden(
        "You can only manage members and agents; managing admins or owners requires the owner role",
    )
}

/// Actor context for a membership write, attributed to the calling
/// admin + their workspace. The `tr_audit_workspace_members` trigger
/// reads `app.actor_uuid` from this, so the audit_log row records WHO
/// changed the membership. Post-auth `claims.sub` is always a valid
/// uuid; the fallback keeps the workspace pinned if it somehow isn't.
fn caller_actor(claims: &Claims, workspace_id: i32) -> ActorContext {
    match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => ActorContext::user_at_workspace(uuid, workspace_id),
        Err(_) => ActorContext::system("workspace:members").with_workspace(workspace_id),
    }
}

/// `GET /api/workspace/members` — list members of the caller's workspace.
pub async fn list_members(
    req: HttpRequest,
    pool: web::Data<Pool>,
    ctx: WorkspaceContext,
) -> impl Responder {
    if let Err(resp) = rbac::require_workspace_role(&req, WorkspaceRole::Admin) {
        return resp;
    }
    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!(error = ?e, "workspace members: pool exhausted");
            return errors::internal("Database connection failed");
        }
    };
    let actor = ActorContext::system("workspace:members:list").with_workspace(ctx.workspace_id);
    let result = with_actor_bypass_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
        workspaces::list_workspace_members(conn, ctx.workspace_id)
    });
    match result {
        Ok(rows) => {
            let body: Vec<MemberView> = rows.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(body)
        }
        Err(e) => {
            error!(error = ?e, workspace_id = ctx.workspace_id, "workspace members list failed");
            errors::internal("Failed to list members")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateRoleRequest {
    pub role: String,
}

/// Outcome of a manage operation, carried out of the bypass transaction
/// so the HTTP mapping happens outside the closure (which can only
/// return `diesel::Error`).
enum ManageOutcome {
    NotFound,
    Forbidden,
    LastOwner,
    UpdatedRole(WorkspaceMember),
    Removed,
    /// The target is a control-plane-owned staff seat in hosted mode; the
    /// product must not mutate it locally. Hand off to the control plane.
    ExternallyManaged,
}

/// `PATCH /api/workspace/members/{user_uuid}` — change a member's role.
pub async fn update_member_role(
    req: HttpRequest,
    pool: web::Data<Pool>,
    ctx: WorkspaceContext,
    path: web::Path<Uuid>,
    body: web::Json<UpdateRoleRequest>,
) -> impl Responder {
    let (caller, caller_role) =
        match rbac::require_workspace_role_detailed(&req, WorkspaceRole::Admin) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
    let target = path.into_inner();
    let Some(new_role) = parse_role(&body.into_inner().role) else {
        return errors::bad_request("role must be one of: owner, admin, agent, member");
    };

    // Reject an assignment the caller's tier can't grant before touching
    // the DB (also re-checked against the target's current role inside
    // the transaction so the two checks can't race).
    if !can_assign(caller_role, new_role) {
        return forbidden_tier();
    }

    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!(error = ?e, "workspace members: pool exhausted");
            return errors::internal("Database connection failed");
        }
    };
    let actor = caller_actor(&caller, ctx.workspace_id);
    let outcome =
        with_actor_bypass_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
            let Some(existing) = workspaces::membership(conn, ctx.workspace_id, target)? else {
                return Ok(ManageOutcome::NotFound);
            };
            if !can_manage(caller_role, WorkspaceRole::from_db(&existing.role)) {
                return Ok(ManageOutcome::Forbidden);
            }
            // Gated write: in hosted mode a staff seat (or a promotion into one)
            // is owned by the control plane and refused here, so the product
            // can't desync the projection.
            match workspaces::update_membership_role_gated(
                conn,
                ctx.workspace_id,
                target,
                &existing.role,
                new_role.as_str(),
            ) {
                Ok(UpdateMembershipRoleResult::Updated(m)) => Ok(ManageOutcome::UpdatedRole(m)),
                Ok(UpdateMembershipRoleResult::NotFound) => Ok(ManageOutcome::NotFound),
                Ok(UpdateMembershipRoleResult::LastOwner) => Ok(ManageOutcome::LastOwner),
                Err(workspaces::MembershipWriteError::ExternallyManaged) => {
                    Ok(ManageOutcome::ExternallyManaged)
                }
                Err(workspaces::MembershipWriteError::Db(e)) => Err(e),
            }
        });

    match outcome {
        Ok(ManageOutcome::UpdatedRole(m)) => {
            info!(workspace_id = ctx.workspace_id, %target, role = %new_role.as_str(), "workspace member role updated");
            HttpResponse::Ok().json(MemberView::from(m))
        }
        Ok(ManageOutcome::Forbidden) => forbidden_tier(),
        Ok(ManageOutcome::NotFound) => {
            errors::not_found_msg(format!("user {target} is not a member of this workspace"))
        }
        Ok(ManageOutcome::LastOwner) => HttpResponse::Conflict().json(serde_json::json!({
            "error": "last_owner",
            "message": "cannot demote the only owner; promote another member first",
        })),
        Ok(ManageOutcome::ExternallyManaged) => errors::externally_managed(),
        Ok(ManageOutcome::Removed) => {
            // Unreachable in the update path.
            errors::internal("Inconsistent membership state")
        }
        Err(e) if workspaces::is_seat_limit_violation(&e) => {
            warn!(workspace_id = ctx.workspace_id, %target, "promotion blocked by workspace seat limit");
            HttpResponse::Forbidden().json(serde_json::json!({
                "error": "seat_limit_reached",
                "message": "This workspace has reached its seat limit. Contact support to add more seats.",
            }))
        }
        Err(e) => {
            error!(error = ?e, workspace_id = ctx.workspace_id, %target, "workspace member role update failed");
            errors::internal("Failed to update member role")
        }
    }
}

/// `DELETE /api/workspace/members/{user_uuid}` — remove a member.
pub async fn remove_member(
    req: HttpRequest,
    pool: web::Data<Pool>,
    ctx: WorkspaceContext,
    path: web::Path<Uuid>,
    // Best-effort search reindex so the removed workspace tag drops off
    // the user's search doc; optional so tests need not wire it.
    search_service: Option<web::Data<Arc<SearchService>>>,
) -> impl Responder {
    let (caller, caller_role) =
        match rbac::require_workspace_role_detailed(&req, WorkspaceRole::Admin) {
            Ok(v) => v,
            Err(resp) => return resp,
        };
    let target = path.into_inner();

    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!(error = ?e, "workspace members: pool exhausted");
            return errors::internal("Database connection failed");
        }
    };
    let actor = caller_actor(&caller, ctx.workspace_id);
    let outcome =
        with_actor_bypass_context::<_, diesel::result::Error>(&mut conn, &actor, |conn| {
            let Some(existing) = workspaces::membership(conn, ctx.workspace_id, target)? else {
                return Ok(ManageOutcome::NotFound);
            };
            if !can_manage(caller_role, WorkspaceRole::from_db(&existing.role)) {
                return Ok(ManageOutcome::Forbidden);
            }
            // Gated write: in hosted mode a staff seat is control-plane-owned
            // and refused here (the CP revokes seats through its own path).
            // remove_membership returns 0 both for "not found" (excluded above)
            // and "would orphan the last owner"; since the row exists, 0 here
            // means the last-owner guard fired.
            match workspaces::remove_membership_gated(
                conn,
                ctx.workspace_id,
                target,
                &existing.role,
            ) {
                Ok(1) => Ok(ManageOutcome::Removed),
                Ok(_) => Ok(ManageOutcome::LastOwner),
                Err(workspaces::MembershipWriteError::ExternallyManaged) => {
                    Ok(ManageOutcome::ExternallyManaged)
                }
                Err(workspaces::MembershipWriteError::Db(e)) => Err(e),
            }
        });

    match outcome {
        Ok(ManageOutcome::Removed) => {
            info!(workspace_id = ctx.workspace_id, %target, "workspace member removed");
            if let Some(search_service) = &search_service {
                indexing_tasks::spawn_reindex_user(search_service.get_ref().clone(), target);
            }
            HttpResponse::NoContent().finish()
        }
        Ok(ManageOutcome::Forbidden) => forbidden_tier(),
        Ok(ManageOutcome::NotFound) => {
            errors::not_found_msg(format!("user {target} is not a member of this workspace"))
        }
        Ok(ManageOutcome::LastOwner) => HttpResponse::Conflict().json(serde_json::json!({
            "error": "last_owner",
            "message": "cannot remove the only owner; promote another member first",
        })),
        Ok(ManageOutcome::ExternallyManaged) => errors::externally_managed(),
        Ok(ManageOutcome::UpdatedRole(_)) => {
            // Unreachable in the remove path.
            errors::internal("Inconsistent membership state")
        }
        Err(e) => {
            error!(error = ?e, workspace_id = ctx.workspace_id, %target, "workspace member removal failed");
            errors::internal("Failed to remove member")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use WorkspaceRole::{Admin, Agent, Member, Owner};

    #[test]
    fn owner_can_manage_every_role() {
        for target in [Owner, Admin, Agent, Member] {
            assert!(can_manage(Owner, target), "owner should manage {target:?}");
        }
    }

    #[test]
    fn owner_can_assign_every_role() {
        for role in [Owner, Admin, Agent, Member] {
            assert!(can_assign(Owner, role), "owner should assign {role:?}");
        }
    }

    #[test]
    fn admin_manages_only_agents_and_members() {
        assert!(can_manage(Admin, Agent));
        assert!(can_manage(Admin, Member));
        // An admin cannot act on peers or owners — no admin-vs-admin
        // lockout, no touching the owner.
        assert!(!can_manage(Admin, Admin));
        assert!(!can_manage(Admin, Owner));
    }

    #[test]
    fn admin_assigns_only_agent_or_member() {
        assert!(can_assign(Admin, Agent));
        assert!(can_assign(Admin, Member));
        // No self- or peer-escalation: an admin can't grant admin/owner.
        assert!(!can_assign(Admin, Admin));
        assert!(!can_assign(Admin, Owner));
    }

    #[test]
    fn sub_admin_roles_can_manage_nobody() {
        // These never pass the require_workspace_role(Admin) gate, but the
        // tier function still fails closed for them.
        for caller in [Agent, Member] {
            for target in [Owner, Admin, Agent, Member] {
                assert!(!can_manage(caller, target));
                assert!(!can_assign(caller, target));
            }
        }
    }

    #[test]
    fn parse_role_round_trips_known_values_and_rejects_others() {
        assert_eq!(parse_role("owner"), Some(Owner));
        assert_eq!(parse_role("admin"), Some(Admin));
        assert_eq!(parse_role("agent"), Some(Agent));
        assert_eq!(parse_role("member"), Some(Member));
        assert_eq!(parse_role("superuser"), None);
        assert_eq!(parse_role(""), None);
    }
}
