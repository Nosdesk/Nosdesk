use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::db::DbConnection;
use crate::handlers::errors;
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
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Hash the token to look it up
    let token_hash = crate::utils::reset_tokens::ResetTokenUtils::hash_token(&request_data.token);

    // Find the token
    let token = match repository::reset_tokens::find_token_by_hash(&mut conn, &token_hash) {
        Ok(token) => token,
        Err(_) => {
            return HttpResponse::Ok().json(ValidateInvitationResponse {
                valid: false,
                user_email: None,
                user_name: None,
                message: Some("Invalid or expired invitation link".to_string()),
                context: None,
            });
        }
    };

    // Check token type
    if token.token_type != TokenType::Invitation.as_str() {
        return HttpResponse::Ok().json(ValidateInvitationResponse {
            valid: false,
            user_email: None,
            user_name: None,
            message: Some("Invalid invitation link".to_string()),
            context: None,
        });
    }

    // Check if already used
    if token.is_used {
        return HttpResponse::Ok().json(ValidateInvitationResponse {
            valid: false,
            user_email: None,
            user_name: None,
            message: Some("This invitation has already been used".to_string()),
            context: None,
        });
    }

    // Check if expired
    let expires_at_utc = chrono::DateTime::<Utc>::from_naive_utc_and_offset(token.expires_at, Utc);
    if crate::utils::reset_tokens::ResetTokenUtils::is_token_expired(expires_at_utc) {
        return HttpResponse::Ok().json(ValidateInvitationResponse {
            valid: false,
            user_email: None,
            user_name: None,
            message: Some("This invitation has expired".to_string()),
            context: None,
        });
    }

    // Get user information
    let user = match repository::get_user_by_uuid(&token.user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::Ok().json(ValidateInvitationResponse {
                valid: false,
                user_email: None,
                user_name: None,
                message: Some("User not found".to_string()),
                context: None,
            });
        }
    };

    // Get user's primary email
    let user_email = repository::user_helpers::get_primary_email(&user.uuid, &mut conn);

    // Derive context from the metadata we stamped when issuing the token,
    // so the frontend can swap copy to "confirm your ticket submission"
    // instead of the generic onboarding flow.
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

    HttpResponse::Ok().json(ValidateInvitationResponse {
        valid: true,
        user_email,
        user_name: Some(user.name),
        message: None,
        context,
    })
}

/// Accept an invitation and set the user's password
pub async fn accept_invitation(
    db_pool: web::Data<crate::db::Pool>,
    search_service: web::Data<Arc<SearchService>>,
    request_data: web::Json<AcceptInvitationRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Validate password
    if request_data.password.len() < 8 {
        return errors::bad_request("Password must be at least 8 characters long");
    } else if request_data.password.len() > 128 {
        return errors::bad_request("Password must be less than 128 characters");
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
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": e
            }));
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
            return errors::bad_request("Invalid or expired invitation");
        }
    };

    // Hash the password
    let password_hash = match hash_password(&request_data.password) {
        Ok(hash) => hash,
        Err(e) => {
            error!("Failed to hash password: {}", e);
            return errors::internal("Error processing password");
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

    if existing_identity.is_some() {
        // Update existing local auth identity
        if let Err(e) = diesel::update(
            user_auth_identities::table
                .filter(user_auth_identities::user_uuid.eq(&user.uuid))
                .filter(user_auth_identities::provider_type.eq("local")),
        )
        .set(user_auth_identities::password_hash.eq(Some(&password_hash)))
        .execute(&mut conn)
        {
            error!("Failed to update password hash for invitation: {:?}", e);
            return errors::internal("Error setting password");
        }
    } else {
        // Create new local auth identity
        let user_email = repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
            .unwrap_or_else(|| format!("user-{}", user.uuid));

        #[derive(diesel::Insertable)]
        #[diesel(table_name = user_auth_identities)]
        struct NewLocalAuthIdentity<'a> {
            user_uuid: uuid::Uuid,
            provider_type: &'a str,
            external_id: &'a str,
            email: Option<&'a str>,
            password_hash: Option<&'a str>,
        }

        let auth_identity = NewLocalAuthIdentity {
            user_uuid: user.uuid,
            provider_type: "local",
            external_id: &user_email,
            email: Some(&user_email),
            password_hash: Some(&password_hash),
        };

        if let Err(e) = diesel::insert_into(user_auth_identities::table)
            .values(&auth_identity)
            .execute(&mut conn)
        {
            error!("Failed to create auth identity for invitation: {:?}", e);
            return errors::internal("Error setting password");
        }
    }

    // Pre-session (token-verified) flow: resolve the audit workspace
    // once from the user's primary membership, then thread it through
    // the audited writes below (users.password_changed_at and the
    // ticket release). Fail loud if the user has no membership —
    // silently attributing to workspace 1 mis-routes the audit row
    // under hosted multi-tenancy.
    let workspace_id =
        match crate::repository::workspaces::primary_workspace_for_user(&mut conn, user.uuid) {
            Ok(ws) => ws,
            Err(e) => {
                error!(
                    user_uuid = %user.uuid,
                    error = ?e,
                    "Failed to resolve primary workspace for invitation accept"
                );
                return errors::internal("Failed to complete invitation");
            }
        };
    let actor = crate::sync::actor::ActorContext::user_at_workspace(user.uuid, workspace_id);

    // Update password_changed_at timestamp in users table
    let now = Utc::now().naive_utc();
    if let Err(e) = crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
        diesel::update(crate::schema::users::table.find(&user.uuid))
            .set(crate::schema::users::password_changed_at.eq(now))
            .execute(c)
    }) {
        warn!("Failed to update password_changed_at: {:?}", e);
        // Don't fail the request for this
    }

    // Mark user's primary email as verified (they proved ownership by receiving the invitation)
    use crate::schema::user_emails;
    if let Err(e) = diesel::update(
        user_emails::table
            .filter(user_emails::user_uuid.eq(&user.uuid))
            .filter(user_emails::is_primary.eq(true)),
    )
    .set(user_emails::is_verified.eq(true))
    .execute(&mut conn)
    {
        warn!("Failed to mark email as verified: {:?}", e);
        // Don't fail the request for this
    }

    // Release any guest-submission tickets that were waiting on this
    // verification. Each newly-verified ticket is pushed into the live SSE
    // stream and the search index so techs pick it up immediately — the
    // same side-effects that would have fired at submit time for a
    // non-gated ticket.
    match crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
        repository::tickets::verify_pending_tickets_for_user(c, user.uuid)
    }) {
        Ok(released) if !released.is_empty() => {
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
        }
    }

    // Log security event for invitation acceptance
    if let Err(e) = log_invitation_acceptance_event(&user.uuid, &http_request, &mut conn).await {
        warn!("Failed to log invitation acceptance event: {}", e);
        // Don't fail the request if logging fails
    }

    info!(
        "Invitation accepted successfully for user: {} (uuid={})",
        user.name, user.uuid
    );

    HttpResponse::Ok().json(AcceptInvitationResponse {
        success: true,
        message:
            "Your account has been activated. You can now log in with your email and password."
                .to_string(),
    })
}

/// Helper function to log invitation acceptance security event
async fn log_invitation_acceptance_event(
    user_uuid: &uuid::Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::utils::security_events::{record_security_event, SecurityEventInput};

    record_security_event(
        conn,
        SecurityEventInput {
            user_uuid: Some(*user_uuid),
            event_type: "invitation_accepted",
            severity: "info",
            details: Some(json!({
                "action": "invitation_accepted",
                "success": true
            })),
            request: Some(request),
            session_id: None,
        },
    )?;

    Ok(())
}
