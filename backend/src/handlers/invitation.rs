use actix_web::{web, HttpMessage, HttpRequest, HttpResponse};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::db::DbConnection;
use crate::errors::{self, ApiError};
use crate::handlers::helpers;
use crate::models::{
    AcceptInvitationRequest, AcceptInvitationResponse, ValidateInvitationRequest,
    ValidateInvitationResponse,
};
use crate::repository;
use crate::services::search::indexing_tasks;
use crate::services::search::SearchService;
use crate::utils::auth::hash_password;
use crate::utils::reset_tokens::TokenType;

/// Validate an invitation token without consuming it
/// This endpoint allows the frontend to check if a token is valid before showing the password form
pub async fn validate_invitation(
    db_pool: web::Data<crate::db::Pool>,
    request_data: web::Json<ValidateInvitationRequest>,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&db_pool)?;

    // Hash the token to look it up
    let token_hash = crate::utils::reset_tokens::ResetTokenUtils::hash_token(&request_data.token);

    // Find the token
    let token = match repository::reset_tokens::find_token_by_hash(&mut conn, &token_hash) {
        Ok(token) => token,
        Err(_) => {
            return Ok(HttpResponse::Ok().json(ValidateInvitationResponse {
                valid: false,
                user_email: None,
                user_name: None,
                invited_by: None,
                workspace_name: None,
                message: Some("Invalid or expired invitation link".to_string()),
                context: None,
                reason: None,
                password_required: false,
            }));
        }
    };

    // Check token type
    if token.token_type != TokenType::Invitation.as_str() {
        return Ok(HttpResponse::Ok().json(ValidateInvitationResponse {
            valid: false,
            user_email: None,
            user_name: None,
            invited_by: None,
            workspace_name: None,
            message: Some("Invalid invitation link".to_string()),
            context: None,
            reason: None,
            password_required: false,
        }));
    }

    // Derive context from the metadata we stamped when issuing the token,
    // so the frontend can swap copy to "confirm your ticket submission"
    // instead of the generic onboarding flow. Known
    // before the used/expired checks so those states keep the right copy.
    let context = token
        .metadata
        .as_ref()
        .and_then(|m| m.get("source"))
        .and_then(|s| s.as_str())
        .map(|s| match s {
            "guest_ticket_submission" => "guest_ticket".to_string(),
            other => other.to_string(),
        })
        .or_else(|| Some("invitation".to_string()));

    // Check if already used
    if token.is_used {
        return Ok(HttpResponse::Ok().json(ValidateInvitationResponse {
            valid: false,
            user_email: None,
            user_name: None,
            invited_by: None,
            workspace_name: None,
            message: Some("This invitation has already been used".to_string()),
            context,
            reason: Some("used".to_string()),
            password_required: false,
        }));
    }

    // Check if expired
    let expires_at_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(token.expires_at, Utc);
    if crate::utils::reset_tokens::ResetTokenUtils::is_token_expired(expires_at_utc) {
        return Ok(HttpResponse::Ok().json(ValidateInvitationResponse {
            valid: false,
            user_email: None,
            user_name: None,
            invited_by: None,
            workspace_name: None,
            message: Some("This invitation has expired".to_string()),
            context,
            reason: Some("expired".to_string()),
            password_required: false,
        }));
    }

    // Get user information
    let user = match repository::get_user_by_uuid(&token.user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return Ok(HttpResponse::Ok().json(ValidateInvitationResponse {
                valid: false,
                user_email: None,
                user_name: None,
                invited_by: None,
                workspace_name: None,
                message: Some("User not found".to_string()),
                context: None,
                reason: None,
                password_required: false,
            }));
        }
    };

    // Get user's primary email
    let user_email = repository::user_helpers::get_primary_email(&user.uuid, &mut conn);

    let (invited_by, workspace_name) = greeting_fields(token.metadata.as_ref());
    let password_required = context.as_deref() != Some("guest_ticket")
        || crate::middleware::workspace_context::local_credentials_permitted();
    Ok(HttpResponse::Ok().json(ValidateInvitationResponse {
        valid: true,
        user_email,
        user_name: Some(user.name),
        message: None,
        invited_by,
        workspace_name,
        context,
        reason: None,
        password_required,
    }))
}

/// Accept an invitation and set the user's password
pub async fn accept_invitation(
    db_pool: web::Data<crate::db::Pool>,
    search_service: web::Data<Arc<SearchService>>,
    request_data: web::Json<AcceptInvitationRequest>,
    http_request: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&db_pool)?;

    // Password-based invitation acceptance writes a local credential, which
    // hosted deployments disable in favour of SSO onboarding. Refuse before
    // consuming the token so it stays valid for the SSO path.
    if crate::handlers::auth::hosted_local_auth_disabled() {
        return Err(ApiError::BadRequest(
            "Password-based sign-up is not available for this deployment".into(),
        ));
    }

    // Validate password
    if request_data.password.len() < 8 {
        return Err(ApiError::BadRequest(
            "Password must be at least 8 characters long".into(),
        ));
    } else if request_data.password.len() > 128 {
        return Err(ApiError::BadRequest(
            "Password must be less than 128 characters".into(),
        ));
    }

    // Validate and consume the invitation token
    let user_uuid = match repository::reset_tokens::validate_and_consume_token(
        &mut conn,
        &request_data.token,
        TokenType::Invitation.as_str(),
    ) {
        Ok(uuid) => uuid,
        Err(e) => {
            warn!("Invalid invitation token: {}", e);
            return Ok(errors::bad_request(e));
        }
    };

    // Get the user
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            error!(
                "User not found for invitation acceptance: user_uuid={}, error={}",
                user_uuid, e
            );
            return Err(ApiError::BadRequest("Invalid or expired invitation".into()));
        }
    };

    // Hash the password
    let password_hash = match hash_password(&request_data.password) {
        Ok(hash) => hash,
        Err(e) => {
            error!("Failed to hash password: {}", e);
            return Err(ApiError::Internal("Error processing password".into()));
        }
    };

    // Update the user's password hash in user_auth_identities
    use crate::schema::user_auth_identities;
    use diesel::prelude::*;

    // First, check if a local auth identity already exists
    let existing_identity: Option<i32> = user_auth_identities::table
        .filter(user_auth_identities::user_uuid.eq(&user.uuid))
        .filter(user_auth_identities::provider_type.eq("local"))
        .select(user_auth_identities::id)
        .first(&mut conn)
        .optional()
        .ok()
        .flatten();

    // Hosted disables local passwords, so accepting an invitation to set one is
    // not applicable there (staff are control-plane seats, requesters use the
    // portal). Fail cleanly rather than 500 inside the credential writes below.
    if !crate::middleware::workspace_context::local_credentials_permitted() {
        return Ok(errors::local_auth_disabled());
    }

    if existing_identity.is_some() {
        // Update existing local auth identity
        if let Err(e) = repository::user_auth_identities::update_local_password_hash(
            &mut conn,
            &user.uuid,
            &password_hash,
        ) {
            error!("Failed to update password hash for invitation: {:?}", e);
            return Err(ApiError::Internal("Error setting password".into()));
        }
    } else {
        // Create new local auth identity
        let user_email = repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
            .unwrap_or_else(|| format!("user-{}", user.uuid));

        let auth_identity = crate::models::NewUserAuthIdentity {
            user_uuid: user.uuid,
            provider_type: "local".to_string(),
            external_id: user_email.clone(),
            email: Some(user_email.clone()),
            metadata: None,
            password_hash: Some(password_hash.clone()),
            workspace_id: None,
        };

        if let Err(e) =
            repository::user_auth_identities::create_local_identity(auth_identity, &mut conn)
        {
            error!("Failed to create auth identity for invitation: {:?}", e);
            return Err(ApiError::Internal("Error setting password".into()));
        }
    }

    complete_verification(
        &db_pool,
        &mut conn,
        &search_service,
        &user,
        Completion::PasswordSet,
    )?;

    // Log security event for invitation acceptance
    if let Err(e) =
        record_verification_event(&user.uuid, "invitation_accepted", &http_request, &mut conn)
    {
        warn!("Failed to log invitation acceptance event: {}", e);
        // Don't fail the request if logging fails
    }

    info!(
        "Invitation accepted successfully for user: {} (uuid={})",
        user.name, user.uuid
    );

    Ok(HttpResponse::Ok().json(AcceptInvitationResponse {
        success: true,
        message:
            "Your account has been activated. You can now log in with your email and password."
                .to_string(),
    }))
}

/// Confirm a guest ticket submission from the emailed link, without setting a
/// password. Hosted has no local credentials, so the password step of
/// [`accept_invitation`] can't run there; confirming the address is what
/// releases the held ticket either way.
///
/// The token is checked first and claimed only after the release succeeds, so
/// a failed release leaves the link usable. Releasing is idempotent, so a
/// double click can't release twice.
pub async fn confirm_guest_submission(
    db_pool: web::Data<crate::db::Pool>,
    search_service: web::Data<Arc<SearchService>>,
    request_data: web::Json<ValidateInvitationRequest>,
    http_request: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let mut conn = helpers::db_conn(&db_pool)?;
    let invalid = || ApiError::BadRequest("Invalid or expired confirmation link".into());

    let token_hash = crate::utils::reset_tokens::ResetTokenUtils::hash_token(&request_data.token);
    let token = repository::reset_tokens::find_token_by_hash(&mut conn, &token_hash)
        .map_err(|_| invalid())?;
    let expires_at = chrono::DateTime::<Utc>::from_naive_utc_and_offset(token.expires_at, Utc);
    let is_guest = token
        .metadata
        .as_ref()
        .and_then(|m| m.get("source"))
        .and_then(|s| s.as_str())
        == Some("guest_ticket_submission");
    if token.token_type != TokenType::Invitation.as_str()
        || token.is_used
        || crate::utils::reset_tokens::ResetTokenUtils::is_token_expired(expires_at)
        || !is_guest
    {
        return Err(invalid());
    }

    let user = repository::get_user_by_uuid(&token.user_uuid, &mut conn).map_err(|_| invalid())?;

    let released = complete_verification(
        &db_pool,
        &mut conn,
        &search_service,
        &user,
        Completion::GuestConfirmed,
    )?;

    if let Err(e) = repository::reset_tokens::validate_and_consume_token(
        &mut conn,
        &request_data.token,
        TokenType::Invitation.as_str(),
    ) {
        // A concurrent confirm claimed it first; the release already happened.
        info!(user_uuid = %user.uuid, error = %e, "Guest confirmation token already claimed");
    }

    if let Err(e) = record_verification_event(
        &user.uuid,
        "guest_submission_confirmed",
        &http_request,
        &mut conn,
    ) {
        warn!("Failed to log guest confirmation event: {}", e);
    }

    // On a hosted tenant origin, confirming is signing in: open a portal
    // session and send them to the request they just confirmed. Self-hosted has
    // no portal yet, so the page shows the confirmed state instead.
    let portal_ctx = http_request
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .cloned()
        .filter(|_| crate::middleware::workspace_context::is_hosted());
    if let (Some(ctx), Some(ticket_id)) = (portal_ctx, released.iter().max().copied()) {
        match crate::handlers::portal::mint_portal_session(
            &user,
            ctx.workspace_uuid,
            &http_request,
            &mut conn,
        ) {
            Ok(session) => {
                return Ok(HttpResponse::Ok()
                    .cookie(session.access)
                    .cookie(session.refresh)
                    .cookie(session.csrf)
                    .json(json!({
                        "success": true,
                        "message": "Your request has been confirmed.",
                        "redirect_to": format!("/tickets/{ticket_id}"),
                    })));
            }
            Err(e) => {
                warn!(user_uuid = %user.uuid, error = ?e, "Guest confirm: could not start a portal session")
            }
        }
    }

    Ok(HttpResponse::Ok().json(AcceptInvitationResponse {
        success: true,
        message: "Your request has been confirmed.".to_string(),
    }))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Completion {
    /// Invitation accepted with a new local password.
    PasswordSet,
    /// Guest submission confirmed without a password. The release is the
    /// point of the request, so a failed release is an error the caller
    /// surfaces (the token is still unclaimed and the link can be retried).
    GuestConfirmed,
}

/// Steps shared by every token-verified acceptance, returning the ids of any
/// guest tickets released: stamp the membership
/// accepted, mark the primary email verified, and release any guest tickets
/// held on this confirmation.
fn complete_verification(
    db_pool: &web::Data<crate::db::Pool>,
    conn: &mut DbConnection,
    search_service: &web::Data<Arc<SearchService>>,
    user: &crate::models::User,
    completion: Completion,
) -> Result<Vec<i32>, ApiError> {
    // Pre-session (token-verified) flow: resolve the audit workspace once
    // from the user's primary membership, then thread it through the audited
    // writes below (users.password_changed_at and the ticket release). The
    // lookup reads workspace_members (RLS-isolated) and is cross-tenant here
    // (token-verified, no request workspace pinned), so it runs elevated;
    // otherwise the runtime role reads no membership and the accept 500s.
    // Fail loud if the user genuinely has no membership.
    let workspace_id =
        // cross-tenant: pre-session workspace resolution: only the invited user is known here.
        match crate::sync::session::background_run(db_pool, "background:invitation_accept", |c| {
            crate::repository::workspaces::primary_workspace_for_user(c, user.uuid)
        }) {
            Ok(ws) => ws,
            Err(e) => {
                error!(
                    user_uuid = %user.uuid,
                    error = ?e,
                    "Failed to resolve primary workspace for invitation accept"
                );
                return Err(ApiError::Internal("Failed to complete invitation".into()));
            }
        };
    let actor = crate::sync::actor::ActorContext::user_at_workspace(user.uuid, workspace_id);

    // Update password_changed_at timestamp in users table
    if completion == Completion::PasswordSet {
        let now = Utc::now().naive_utc();
        if let Err(e) = crate::sync::session::with_actor_context(conn, &actor, |c| {
            repository::users::set_password_changed_at(c, &user.uuid, now)
        }) {
            warn!("Failed to update password_changed_at: {:?}", e);
            // Don't fail the request for this
        }
    }

    // Stamp the user's membership(s) as accepted so the workspace
    // members list stops showing them as a pending invite. accepted_at
    // is display-only (the 403 membership gate checks row existence,
    // not this column), so a best-effort failure here doesn't block the
    // accept. Audited table, so route through with_actor_context.
    if let Err(e) = crate::sync::session::with_actor_context(conn, &actor, |c| {
        repository::workspaces::mark_memberships_accepted(c, user.uuid)
    }) {
        warn!("Failed to stamp workspace membership accepted_at: {:?}", e);
        // Don't fail the request for this
    }

    // Mark user's primary email as verified (they proved ownership by receiving the invitation)
    if let Err(e) = repository::user_emails::mark_primary_verified(conn, &user.uuid) {
        warn!("Failed to mark email as verified: {:?}", e);
        // Don't fail the request for this
    }

    // Release any guest-submission tickets that were waiting on this
    // verification. Each newly-verified ticket is pushed into the live SSE
    // stream and the search index so techs pick it up immediately — the
    // same side-effects that would have fired at submit time for a
    // non-gated ticket.
    let mut released_ids = Vec::new();
    match crate::sync::session::with_actor_context(conn, &actor, |c| {
        repository::tickets::verify_pending_tickets_for_user(c, user.uuid)
    }) {
        Ok(released) if !released.is_empty() => {
            released_ids = released.iter().map(|t| t.id).collect();
            info!(
                user_uuid = %user.uuid,
                count = released.len(),
                "Released pending guest tickets on invitation acceptance"
            );
            for ticket in released {
                indexing_tasks::spawn_index_ticket(
                    search_service.get_ref().clone(),
                    ticket.clone(),
                    None,
                );
                // Released tickets reach clients through the sync pool;
                // no discrete SSE (the event has no consumer).
            }
        }
        Ok(_) => {}
        Err(e) => {
            warn!(user_uuid = %user.uuid, error = %e, "Failed to release pending tickets");
            if completion == Completion::GuestConfirmed {
                return Err(ApiError::Internal("Failed to confirm submission".into()));
            }
        }
    }

    Ok(released_ids)
}

/// Record a token-verified acceptance as a security event.
fn record_verification_event(
    user_uuid: &uuid::Uuid,
    action: &'static str,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::utils::security_events::{record_security_event, SecurityEventInput};

    record_security_event(
        conn,
        SecurityEventInput {
            user_uuid: Some(*user_uuid),
            event_type: action,
            severity: "info",
            details: Some(json!({
                "action": action,
                "success": true
            })),
            request: Some(request),
        },
    )?;

    Ok(())
}

/// Who invited the person and into which workspace, from the metadata
/// stamped when the token was issued (`prepare_invitation`). Either may be
/// missing on older tokens or guest confirmations; blank strings count as
/// missing so the accept page falls back to its generic heading.
pub(crate) fn greeting_fields(
    metadata: Option<&serde_json::Value>,
) -> (Option<String>, Option<String>) {
    let field = |k: &str| {
        metadata
            .and_then(|m| m.get(k))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    (field("invited_by"), field("workspace_name"))
}

#[cfg(test)]
mod greeting_tests {
    use super::greeting_fields;
    use serde_json::json;

    #[test]
    fn reads_both_fields_and_treats_blank_as_missing() {
        let m = json!({ "invited_by": " Ana ", "workspace_name": "Acme Support", "source": "x" });
        assert_eq!(
            greeting_fields(Some(&m)),
            (Some("Ana".into()), Some("Acme Support".into()))
        );
        let blank = json!({ "invited_by": "  ", "workspace_name": "" });
        assert_eq!(greeting_fields(Some(&blank)), (None, None));
        assert_eq!(greeting_fields(None), (None, None));
        let old = json!({ "source": "guest_ticket_submission" });
        assert_eq!(greeting_fields(Some(&old)), (None, None));
    }
}
