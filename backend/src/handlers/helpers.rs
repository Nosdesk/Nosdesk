use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::handlers::errors;
use crate::models::{Claims, User};
use crate::repository;
use crate::sync::actor::ActorContext;
use crate::utils;

/// Default page size for list endpoints when the caller doesn't
/// specify one. Twenty is a sensible default for tabular UIs.
pub const DEFAULT_LIMIT: i64 = 20;

/// Cap on caller-supplied limits. Anything larger gets clamped down
/// so a single request can't run an unbounded query.
pub const MAX_LIMIT: i64 = 200;

/// Apply default + cap to a caller-supplied limit. Every list
/// endpoint should funnel its `?limit=` through this so the
/// worst-case query cost is bounded uniformly.
pub fn clamp_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT)
}

/// Clamp a caller-supplied offset to a non-negative value. Pairs
/// with [`clamp_limit`] for offset-based pagination.
pub fn clamp_offset(offset: Option<i64>) -> i64 {
    offset.unwrap_or(0).max(0)
}

/// Get a database connection from the pool. Re-exports the
/// canonical implementation in [`errors::db_conn`] so existing call
/// sites keep working — pool exhaustion now returns a 503 with a
/// structured error body and a Retry-After header instead of a
/// generic 500.
pub fn db_conn(pool: &web::Data<Pool>) -> Result<DbConnection, HttpResponse> {
    errors::db_conn(pool)
}

/// Pin `app.workspace_id` on a raw connection from the request's resolved
/// workspace, so RLS-scoped tenant reads/writes on that connection see the
/// tenant's rows (production connects as the NOBYPASSRLS `nosdesk_app`
/// role). Session-scoped, but the pool scrubs `app.*` on every checkout
/// (`ResettingManager`), so the pin can't outlive the request; a later
/// `with_actor_context` overrides it per transaction.
///
/// No-op when the request didn't resolve a workspace (apex / platform
/// routes that don't touch tenant tables). This is what makes the legacy
/// raw-conn helpers safe-by-default, the way `TenantConn` already is.
pub fn pin_request_workspace(req: &HttpRequest, conn: &mut DbConnection) {
    if let Some(ws) = request_workspace_id(req) {
        pin_workspace(conn, ws);
    }
}

/// The workspace a request resolved to, from the actor the auth middleware
/// pinned (preferred) or the `WorkspaceContext` the host / selection resolver
/// attached. `None` for apex / platform routes that never resolved a
/// workspace. The single source raw-conn callers use to build a workspace-
/// pinned actor for `with_actor_context`.
pub fn request_workspace_id(req: &HttpRequest) -> Option<i32> {
    req.extensions()
        .get::<crate::middleware::RequestContext>()
        .and_then(|ctx| ctx.actor.workspace_id)
        .or_else(|| {
            req.extensions()
                .get::<crate::extractors::WorkspaceContext>()
                .map(|w| w.workspace_id)
        })
}

/// Pin a known `workspace_id` on a raw connection (session-scoped). The
/// lower-level half of [`pin_request_workspace`], for the handlers that
/// already hold a resolved [`WorkspaceContext`] and don't need to re-derive
/// it from the request extensions.
pub fn pin_workspace(conn: &mut DbConnection, workspace_id: i32) {
    use diesel::prelude::*;
    let _ = diesel::sql_query("SELECT set_config('app.workspace_id', $1, false) AS set_config")
        .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
        .execute(conn);
}

/// Extract claims + user UUID + DB connection from a request.
/// Combines the three most common boilerplate blocks into one call.
///
/// The returned connection is pinned to the request's workspace (see
/// [`pin_request_workspace`]) so RLS-scoped reads on it are tenant-correct
/// without each handler remembering to scope.
pub fn auth_conn(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
) -> Result<(Claims, Uuid, DbConnection), HttpResponse> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| errors::unauthorized("Authentication required"))?;
    let mut conn = db_conn(pool)?;
    let user_uuid =
        Uuid::parse_str(&claims.sub).map_err(|_| errors::internal("Invalid user UUID"))?;
    pin_request_workspace(req, &mut conn);
    Ok((claims, user_uuid, conn))
}

/// Admin-only helper with no target user: enforce admin role, return a
/// pooled DB connection. Use this for admin-settings endpoints that
/// act on *singletons* (site_settings, channels, etc.) rather than a
/// specific target user — the target-user variant [`admin_user_conn`]
/// is for endpoints like "admin updates user X's role."
pub fn admin_conn(req: &HttpRequest, pool: &web::Data<Pool>) -> Result<DbConnection, HttpResponse> {
    let claims = req
        .extensions()
        .get::<Claims>()
        .cloned()
        .ok_or_else(|| errors::unauthorized("Authentication required"))?;
    let mut conn = db_conn(pool)?;
    // Pin the request's workspace so the membership lookup below is
    // RLS-scoped to the workspace the caller is acting in, not collapsed
    // onto the bootstrap workspace.
    pin_request_workspace(req, &mut conn);
    // Admin tier = platform admin, or workspace admin/owner in the
    // request's workspace (the claims carry only the platform role, so
    // the workspace half is looked up). Mirrors the old derived-admin
    // gate now that the legacy UserRole projection is gone.
    let is_admin = crate::utils::rbac::is_platform_admin(&claims)
        || crate::utils::parse_uuid(&claims.sub)
            .ok()
            .and_then(|uuid| crate::repository::user_helpers::workspace_role(&mut conn, uuid))
            .is_some_and(|r| r.meets(crate::models::WorkspaceRole::Admin));
    if !is_admin {
        return Err(errors::forbidden("Admin required"));
    }
    Ok(conn)
}

/// Admin-only helper: authenticate caller, enforce admin role, parse target UUID, load target user.
/// Returns (admin Claims, target User, DbConnection) or an appropriate error response.
pub fn admin_user_conn(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
    target_uuid_str: &str,
) -> Result<(Claims, User, DbConnection), HttpResponse> {
    let (claims, _caller_uuid, mut conn) = auth_conn(req, pool)?;

    let target_uuid = utils::parse_uuid(target_uuid_str)
        .map_err(|_| errors::bad_request("Invalid UUID format"))?;

    // Was platform-admin-only. Now a workspace admin may recover a member of
    // their OWN workspace (self-hosted), bounded to accounts they wholly own.
    // The isolation lives in this gate, not the loose platform check.
    authorize_target_user_action(req, pool, &claims, target_uuid, true)?;

    let user = repository::get_user_by_uuid(&target_uuid, &mut conn)
        .map_err(|_| errors::not_found("User"))?;

    Ok((claims, user, conn))
}

/// Why an admin action on a target user was denied (mapped to an HTTP response
/// by [`authorize_target_user_action`]). Split from the response so the
/// decision is unit-testable.
enum TargetActionDenied {
    NotWorkspaceAdmin,
    TargetNotInWorkspace,
    TargetInOtherWorkspaces,
}

/// Core decision for the self-hosted workspace-admin path (platform-admin and
/// hosted mode are handled by the wrapper). `conn` is pinned to `ws_id` for the
/// RLS-scoped membership reads and, when `require_sole_workspace`, reused to run
/// the cross-workspace count under BYPASSRLS.
fn target_action_decision(
    conn: &mut DbConnection,
    ws_id: i32,
    caller_uuid: Uuid,
    target_uuid: Uuid,
    require_sole_workspace: bool,
    actor: &ActorContext,
) -> Result<(), TargetActionDenied> {
    use crate::models::WorkspaceRole;

    // Scope the RLS-isolated membership reads to the caller's workspace.
    pin_workspace(conn, ws_id);

    // Caller must be an admin/owner of this workspace.
    let caller_admin = repository::workspaces::membership(conn, ws_id, caller_uuid)
        .ok()
        .flatten()
        .is_some_and(|m| WorkspaceRole::from_db(&m.role).meets(WorkspaceRole::Admin));
    if !caller_admin {
        return Err(TargetActionDenied::NotWorkspaceAdmin);
    }

    // Target must be a member of the caller's workspace. This membership
    // resolution IS the tenant-isolation boundary: without it a workspace-A
    // admin could act on any global user uuid, including workspace-B users.
    if repository::workspaces::membership(conn, ws_id, target_uuid)
        .ok()
        .flatten()
        .is_none()
    {
        return Err(TargetActionDenied::TargetNotInWorkspace);
    }

    // Account-global credential ops (Decision 1): refuse when the target belongs
    // to more than one workspace, since a single workspace's admin does not own
    // the whole account. The cross-workspace count MUST run under BYPASSRLS:
    // workspace_members is FORCE-isolated to `ws_id` on the pinned nosdesk_app
    // conn, so a plain read would always see exactly one row and this guard would
    // silently pass. (list_memberships_for_user excludes archived workspaces, so
    // a stale archived membership does not block recovery.)
    if require_sole_workspace {
        // Fail closed: a failed cross-workspace count denies recovery rather
        // than falling through to allow.
        let count = crate::sync::session::with_actor_bypass_context(conn, actor, |c| {
            Ok::<usize, diesel::result::Error>(
                repository::workspaces::list_memberships_for_user(c, target_uuid)?.len(),
            )
        })
        .unwrap_or(usize::MAX);
        if count > 1 {
            return Err(TargetActionDenied::TargetInOtherWorkspaces);
        }
    }

    Ok(())
}

/// Authorize an admin action on a TARGET user (credential recovery, security
/// posture read, invitation resend). Platform admins pass (cross-tenant
/// operators). In hosted mode these functions are control-plane owned, so
/// non-platform callers are refused with a control-plane hand-off. In
/// self-hosted, a workspace admin may act only on a member of their own
/// workspace; `require_sole_workspace` additionally refuses a target who also
/// belongs to other workspaces (account-global credential ops). See
/// docs/architecture/workspace-function-tiers.md (PR 2b).
pub fn authorize_target_user_action(
    req: &HttpRequest,
    pool: &web::Data<Pool>,
    claims: &Claims,
    target_uuid: Uuid,
    require_sole_workspace: bool,
) -> Result<(), HttpResponse> {
    if crate::utils::rbac::is_platform_admin(claims) {
        return Ok(());
    }
    if !crate::middleware::workspace_context::local_credentials_permitted() {
        return Err(errors::forbidden(
            "In hosted deployments, member account recovery is handled from the \
             Nosdesk control plane; it is only available in self-hosted mode.",
        ));
    }
    let caller_uuid =
        utils::parse_uuid(&claims.sub).map_err(|_| errors::bad_request("Invalid caller"))?;
    let ws_id = match request_workspace_id(req) {
        Some(id) => id,
        None => return Err(errors::internal("Workspace context missing")),
    };
    let mut conn = db_conn(pool)?;
    let actor = actor_for(req, "member_recovery_gate");
    match target_action_decision(
        &mut conn,
        ws_id,
        caller_uuid,
        target_uuid,
        require_sole_workspace,
        &actor,
    ) {
        Ok(()) => Ok(()),
        Err(TargetActionDenied::NotWorkspaceAdmin) => Err(errors::forbidden(
            "This action requires workspace admin privileges.",
        )),
        Err(TargetActionDenied::TargetNotInWorkspace) => Err(errors::forbidden(
            "This user is not a member of your workspace.",
        )),
        Err(TargetActionDenied::TargetInOtherWorkspaces) => Err(errors::forbidden(
            "This member belongs to other workspaces; account recovery must go through \
             an instance administrator.",
        )),
    }
}

/// Build an `ActorContext` from the JWT claims attached to the
/// request. Falls back to a system actor named after `system_ref`
/// when no claims are present (background tasks, public endpoints
/// that wandered into a write path, etc.) — this guarantees every
/// emit gets attributed to *something* rather than producing rows
/// with nil-UUID actors that pollute audit queries.
///
/// The actor's `workspace_id` is sourced from the `RequestContext`
/// that the auth middleware populates, which carries the pin set
/// by `WorkspaceContextMiddleware` ahead of auth. Without this,
/// downstream `with_actor_context` calls leave `app.workspace_id`
/// unset and the strict RLS policy returns zero rows (the failure
/// mode that hit the user-purge handlers before this fix). Tests
/// that inject `Claims` directly without `RequestContext` get the
/// ambient workspace GUC set by `setup_test_connection` instead.
pub fn actor_for(req: &HttpRequest, system_ref: &'static str) -> ActorContext {
    let uuid = req
        .extensions()
        .get::<Claims>()
        .and_then(|c| Uuid::parse_str(&c.sub).ok());
    // Prefer the auth-populated RequestContext actor pin; fall back to the
    // WorkspaceContext that WorkspaceContextMiddleware sets on EVERY route
    // (including public, unauthenticated ones), so workspace-scoped reads
    // work outside the auth middleware too.
    let workspace_id = req
        .extensions()
        .get::<crate::middleware::RequestContext>()
        .and_then(|ctx| ctx.actor.workspace_id)
        .or_else(|| {
            req.extensions()
                .get::<crate::extractors::WorkspaceContext>()
                .map(|w| w.workspace_id)
        });
    let mut actor = match uuid {
        Some(u) => ActorContext::user(u, None),
        None => ActorContext::system(system_ref),
    };
    if let Some(ws) = workspace_id {
        actor = actor.with_workspace(ws);
    }
    actor
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewWorkspace;
    use crate::repository::workspaces::{add_membership, create_workspace};
    use crate::schema::workspace_members;
    use crate::sync::actor::ActorContext;
    use crate::sync::session::with_actor_bypass_context;
    use crate::test_helpers::{setup_test_pool, TestFixtures};
    use diesel::prelude::*;

    // workspaces / members are BYPASSRLS writes (nosdesk_app only has SELECT).
    fn bypass<T, E: From<diesel::result::Error>>(
        conn: &mut DbConnection,
        f: impl FnOnce(&mut DbConnection) -> Result<T, E>,
    ) -> Result<T, E> {
        with_actor_bypass_context(conn, &ActorContext::system("test:helpers:target-gate"), f)
    }

    fn seed_ws(conn: &mut DbConnection, slug: &str) -> i32 {
        bypass(conn, |c| {
            create_workspace(
                c,
                &NewWorkspace {
                    uuid: Uuid::now_v7(),
                    slug: slug.to_string(),
                    name: format!("Workspace {slug}"),
                    seat_limit: None,
                },
            )
        })
        .expect("create workspace")
        .id
    }

    // The cross-workspace isolation this gate exists to enforce. The sole-
    // workspace count runs through the real BYPASSRLS path; a plain (RLS-scoped)
    // count on the pinned nosdesk_app conn would see one row and wrongly admit a
    // multi-workspace target, which is the exact bug this test guards.
    #[test]
    fn target_action_decision_enforces_workspace_isolation() {
        let pool = setup_test_pool();
        let mut conn = pool.get().expect("conn");

        let ws_a = seed_ws(&mut conn, "gatea");
        let ws_b = seed_ws(&mut conn, "gateb");

        let admin = TestFixtures::create_user(&mut conn, "Gate Admin", "user");
        let solo = TestFixtures::create_user(&mut conn, "Solo Member", "user");
        let shared = TestFixtures::create_user(&mut conn, "Shared Member", "user");
        let outsider = TestFixtures::create_user(&mut conn, "Outsider", "user");

        bypass(&mut conn, |c| {
            // create_user auto-enrolls each user in the default workspace (1);
            // clear that so each has exactly the memberships this test sets up.
            diesel::delete(
                workspace_members::table.filter(workspace_members::user_uuid.eq_any([
                    admin.uuid,
                    solo.uuid,
                    shared.uuid,
                    outsider.uuid,
                ])),
            )
            .execute(c)?;
            add_membership(c, ws_a, admin.uuid, "admin")?;
            add_membership(c, ws_a, solo.uuid, "member")?;
            add_membership(c, ws_a, shared.uuid, "member")?;
            add_membership(c, ws_b, shared.uuid, "member")?; // also in B
            add_membership(c, ws_b, outsider.uuid, "member")?; // only in B
            Ok::<(), diesel::result::Error>(())
        })
        .expect("seed memberships");

        let actor = ActorContext::system("test:helpers:target-gate");

        // Sole member of A -> allowed.
        assert!(
            target_action_decision(&mut conn, ws_a, admin.uuid, solo.uuid, true, &actor).is_ok()
        );

        // Member of A who also lives in B -> refused (account-global op).
        assert!(matches!(
            target_action_decision(&mut conn, ws_a, admin.uuid, shared.uuid, true, &actor),
            Err(TargetActionDenied::TargetInOtherWorkspaces)
        ));

        // A user not in A -> refused (the isolation boundary).
        assert!(matches!(
            target_action_decision(&mut conn, ws_a, admin.uuid, outsider.uuid, true, &actor),
            Err(TargetActionDenied::TargetNotInWorkspace)
        ));

        // Non-admin caller (solo is a plain member) -> refused.
        assert!(matches!(
            target_action_decision(&mut conn, ws_a, solo.uuid, shared.uuid, true, &actor),
            Err(TargetActionDenied::NotWorkspaceAdmin)
        ));

        // resend path (no sole-workspace requirement): a multi-workspace member
        // of A is allowed, only membership in the caller's workspace matters.
        assert!(
            target_action_decision(&mut conn, ws_a, admin.uuid, shared.uuid, false, &actor).is_ok()
        );
    }
}
