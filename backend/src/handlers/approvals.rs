//! Approving requests.
//!
//! Approvers decide in one of two places. Staff approvers decide in the app
//! (`/api/tickets/{id}/approval`, next to the ticket). Everyone else, such as a
//! requester's manager, gets an email whose signed link signs them in to the
//! portal's approval page (`/api/portal/auth/approval`, then
//! `/api/portal/approvals/{id}`).
//!
//! The approval endpoints authorize on the approval row (the caller is an
//! approver in the ticket's current round), not on the requester's portal
//! visibility: a manager can decide on a request they couldn't otherwise see,
//! and sees only what the decision needs.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;

use crate::db::Pool;
use crate::errors::{self, ApiError};
use crate::extractors::{AuthContext, TenantConn, WorkspaceContext};
use crate::handlers::portal::{mint_portal_session, portal_path, PortalContext};
use crate::repository::ticket_approvals::{self as approvals, DecideError};
use crate::repository::ticket_visibility::VisibilityContext;

/// The approval link from an approver's email (public, in `/api/portal/auth`).
pub fn portal_auth_config(cfg: &mut web::ServiceConfig) {
    cfg.route("/approval", web::get().to(approval_link_callback));
}

/// The portal approval pages (in the authenticated `/api/portal` scope).
pub fn portal_config(cfg: &mut web::ServiceConfig) {
    cfg.route("/approvals", web::get().to(list_my_approvals))
        .route("/approvals/{ticket_id}", web::get().to(get_my_approval))
        .route("/approvals/{ticket_id}", web::post().to(decide_my_approval));
}

/// The ticket's approval in the app.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/tickets/{id}/approval", web::get().to(get_ticket_approval))
        .route(
            "/tickets/{id}/approval/decide",
            web::post().to(decide_ticket_approval),
        )
        .route(
            "/tickets/{id}/approval/skip",
            web::post().to(skip_ticket_approval),
        );
}

#[derive(Deserialize)]
pub struct DecideRequest {
    /// `approve` or `decline`.
    pub decision: String,
    pub comment: Option<String>,
}

#[derive(Deserialize)]
pub struct SkipRequest {
    pub comment: Option<String>,
}

#[derive(Deserialize)]
pub struct LinkQuery {
    t: String,
}

fn decide_error(e: DecideError) -> HttpResponse {
    match e {
        DecideError::NotWaiting => errors::conflict("This approval has already been decided"),
        DecideError::CommentRequired => {
            errors::bad_request("Say why you're declining, so the requester knows")
        }
        DecideError::Db(e) => {
            tracing::error!(error = %e, "approvals: decision failed");
            errors::internal("Failed to record the decision")
        }
    }
}

/// A held request that just went ahead gets routed now. Best effort: the
/// decision stands either way.
fn after_decision(conn: &mut crate::db::DbConnection, ticket_id: i32) {
    if let Err(e) =
        crate::services::assignment::AssignmentEngine::assign_after_approval(conn, ticket_id)
    {
        tracing::warn!(error = ?e, "approvals: assignment after approval failed");
    }
}

fn approve_flag(decision: &str) -> Option<bool> {
    match decision {
        "approve" => Some(true),
        "decline" => Some(false),
        _ => None,
    }
}

/// `GET /api/portal/auth/approval?t=…`: signs the approver in and opens the
/// approval page. Following the link decides nothing (mail scanners follow
/// links); the page's buttons do.
pub async fn approval_link_callback(
    req: HttpRequest,
    query: web::Query<LinkQuery>,
    pool: web::Data<Pool>,
) -> Result<HttpResponse, ApiError> {
    let failed = || {
        HttpResponse::Found()
            .append_header(("Location", portal_path("/login?signin_error=1")))
            .finish()
    };
    let Some(ctx) = req.extensions().get::<WorkspaceContext>().cloned() else {
        return Err(ApiError::BadRequest("No workspace for this origin".into()));
    };
    let Some((approver, ticket_id)) =
        crate::utils::portal_ticket_link::verify_approval(ctx.workspace_id, &query.t)
    else {
        return Ok(failed());
    };
    let mut conn = pool
        .get()
        .map_err(|_| ApiError::Internal("Database connection failed".into()))?;
    if !crate::middleware::cookie_auth::is_workspace_member(&mut conn, ctx.workspace_id, approver) {
        return Ok(failed());
    }
    let Ok(user) = crate::repository::users::find_active_by_uuid(&approver, &mut conn) else {
        return Ok(failed());
    };
    if let Err(e) = crate::repository::user_emails::mark_primary_verified(&mut conn, &user.uuid) {
        tracing::warn!(error = ?e, "approval link: could not mark email verified");
    }
    let session = mint_portal_session(&user, ctx.workspace_uuid, &req, &mut conn)?;
    Ok(HttpResponse::Found()
        .cookie(session.access)
        .cookie(session.refresh)
        .cookie(session.csrf)
        .append_header(("Location", portal_path(&format!("/approvals/{ticket_id}"))))
        .finish())
}

/// `GET /api/portal/approvals`: requests waiting for the signed-in person.
pub async fn list_my_approvals(mut tc: TenantConn, portal: PortalContext) -> impl Responder {
    let me = portal.user_uuid;
    let result = tc.run(|conn| {
        let waiting = approvals::waiting_for(conn, me)?;
        waiting
            .iter()
            .map(|(_, ticket)| approvals::summary(conn, ticket))
            .collect::<diesel::QueryResult<Vec<_>>>()
    });
    match result {
        Ok(list) => HttpResponse::Ok().json(json!({ "approvals": list })),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to list approvals");
            errors::internal("Failed to load approvals")
        }
    }
}

/// `GET /api/portal/approvals/{ticket_id}`: one request the signed-in person
/// approves (waiting or already decided by them).
pub async fn get_my_approval(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<i32>,
) -> impl Responder {
    let (me, ticket_id) = (portal.user_uuid, path.into_inner());
    let result = tc.run(|conn| {
        let Some(mine) = approvals::for_approver(conn, ticket_id, me)? else {
            return Ok(None);
        };
        let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
        let summary = approvals::summary(conn, &ticket)?;
        Ok::<_, diesel::result::Error>(Some((summary, mine)))
    });
    match result {
        Ok(Some((summary, mine))) => HttpResponse::Ok().json(json!({
            "approval": summary,
            "mine": { "decision": mine.decision, "comment": mine.comment },
            "can_decide": mine.decision.is_none()
                && summary.approval_state.as_deref() == Some(approvals::PENDING),
        })),
        Ok(None) => errors::not_found("Approval not found"),
        Err(e) => {
            tracing::error!(error = ?e, "portal: failed to load approval");
            errors::internal("Failed to load the approval")
        }
    }
}

/// `POST /api/portal/approvals/{ticket_id}`: approve or decline.
pub async fn decide_my_approval(
    mut tc: TenantConn,
    portal: PortalContext,
    path: web::Path<i32>,
    body: web::Json<DecideRequest>,
) -> impl Responder {
    let (me, ticket_id) = (portal.user_uuid, path.into_inner());
    let Some(approve) = approve_flag(&body.decision) else {
        return errors::bad_request("Decision must be 'approve' or 'decline'");
    };
    let comment = body.comment.clone();
    match tc.run(|conn| {
        Ok::<_, diesel::result::Error>(
            approvals::decide(conn, ticket_id, me, approve, comment.as_deref(), "portal")
                .inspect(|_| after_decision(conn, ticket_id)),
        )
    }) {
        Ok(Ok(state)) => HttpResponse::Ok().json(json!({ "approval_state": state })),
        Ok(Err(e)) => decide_error(e),
        Err(e) => decide_error(DecideError::Db(e.to_string())),
    }
}

/// Whether `auth` may skip a waiting approval under the workspace's setting.
/// Admins can always skip one nobody can approve (no approver on file).
fn may_skip(skip_by: &str, auth: &AuthContext, no_approver: bool) -> bool {
    let admin = auth.is_workspace_admin();
    match skip_by {
        "agents" => auth.can_handle_tickets(),
        "admins" => admin,
        _ => admin && no_approver,
    }
}

/// `GET /api/tickets/{id}/approval`: where the ticket's approval stands, and
/// what the caller can do about it.
pub async fn get_ticket_approval(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let ctx = VisibilityContext::from_auth(&auth);
    let me = auth.user_uuid;
    let result = tc.run(|conn| {
        let mine = approvals::for_approver(conn, ticket_id, me)?;
        if mine.is_none()
            && !crate::repository::ticket_visibility::can_view_ticket(conn, &ctx, ticket_id)?
        {
            return Ok(None);
        }
        let ticket = crate::repository::tickets::get_ticket_by_id(conn, ticket_id)?;
        let summary = approvals::summary(conn, &ticket)?;
        let settings = crate::repository::site_settings::get_site_settings(conn)?;
        Ok::<_, diesel::result::Error>(Some((summary, mine, settings.approval_skip_by)))
    });
    match result {
        Ok(Some((summary, mine, skip_by))) => {
            let pending = summary.approval_state.as_deref() == Some(approvals::PENDING);
            let no_approver = summary.approvers.is_empty();
            HttpResponse::Ok().json(json!({
                "approval": summary,
                "no_approver": no_approver,
                "can_decide": pending && mine.as_ref().is_some_and(|m| m.decision.is_none()),
                "can_skip": pending && may_skip(&skip_by, &auth, no_approver),
            }))
        }
        Ok(None) => errors::not_found("Ticket not found"),
        Err(e) => {
            tracing::error!(error = ?e, "approvals: failed to load ticket approval");
            errors::internal("Failed to load the approval")
        }
    }
}

/// `POST /api/tickets/{id}/approval/decide`: a staff approver decides in the app.
pub async fn decide_ticket_approval(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    body: web::Json<DecideRequest>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let Some(approve) = approve_flag(&body.decision) else {
        return errors::bad_request("Decision must be 'approve' or 'decline'");
    };
    let comment = body.comment.clone();
    let me = auth.user_uuid;
    match tc.run(|conn| {
        Ok::<_, diesel::result::Error>(
            approvals::decide(conn, ticket_id, me, approve, comment.as_deref(), "app")
                .inspect(|_| after_decision(conn, ticket_id)),
        )
    }) {
        Ok(Ok(state)) => HttpResponse::Ok().json(json!({ "approval_state": state })),
        Ok(Err(e)) => decide_error(e),
        Err(e) => decide_error(DecideError::Db(e.to_string())),
    }
}

/// `POST /api/tickets/{id}/approval/skip`: staff skip a waiting approval, if
/// the workspace lets them. Recorded with who and why.
pub async fn skip_ticket_approval(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    body: web::Json<SkipRequest>,
) -> impl Responder {
    let ticket_id = path.into_inner();
    let ctx = VisibilityContext::from_auth(&auth);
    let comment = body.comment.clone();
    let me = auth.user_uuid;
    let result = tc.run(|conn| {
        if !crate::repository::ticket_visibility::can_view_ticket(conn, &ctx, ticket_id)? {
            return Ok(Err(errors::not_found("Ticket not found")));
        }
        let skip_by = crate::repository::site_settings::get_site_settings(conn)?.approval_skip_by;
        let no_approver = approvals::current_round(conn, ticket_id)?.is_empty();
        if !may_skip(&skip_by, &auth, no_approver) {
            return Ok(Err(errors::forbidden(
                "You can't skip approvals in this workspace",
            )));
        }
        Ok::<_, diesel::result::Error>(Ok(approvals::skip(conn, ticket_id, me, comment.as_deref())))
    });
    match result {
        Ok(Ok(Ok(()))) => HttpResponse::Ok().json(json!({ "approval_state": approvals::SKIPPED })),
        Ok(Ok(Err(e))) => decide_error(e),
        Ok(Err(resp)) => resp,
        Err(e) => decide_error(DecideError::Db(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth(role: crate::models::WorkspaceRole) -> AuthContext {
        AuthContext {
            user_uuid: uuid::Uuid::new_v4(),
            platform_role: crate::models::PlatformRole::User,
            workspace_role: Some(role),
            workspace_binding: None,
            name: "Test".into(),
            group_ids: vec![],
        }
    }

    #[test]
    fn skipping_follows_the_workspace_setting() {
        use crate::models::WorkspaceRole;
        let (agent, admin) = (auth(WorkspaceRole::Agent), auth(WorkspaceRole::Admin));
        assert!(may_skip("agents", &agent, false));
        assert!(!may_skip("admins", &agent, false));
        assert!(may_skip("admins", &admin, false));
        assert!(!may_skip("nobody", &admin, false));
        // Nobody to approve it: admins can always let it through.
        assert!(may_skip("nobody", &admin, true));
        assert!(!may_skip("nobody", &agent, true));
    }
}
