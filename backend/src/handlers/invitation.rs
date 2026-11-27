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
    // A request-form confirmation sets a password only on a guest account
    // with no way to sign in (`guest_confirmation_sets_password`); any other
    // confirms with the POST alone.
    let password_required = if context.as_deref() == Some("guest_ticket") {
        guest_confirmation_sets_password(&mut conn, &user).unwrap_or(false)
    } else {
        true
    };
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

    // A guest confirmation (the request form's link, where local passwords
    // exist) is checked now and spent in the transaction that releases its
    // held request in the workspace it names, as `confirm_guest_submission`
    // does, so a failed release leaves the link usable. Any other invitation, and a
    // guest confirmation already used or expired, is spent (or refused) here,
    // before the password is written, with the same answer.
    let guest = repository::reset_tokens::find_token_by_hash(
        &mut conn,
        &crate::utils::reset_tokens::ResetTokenUtils::hash_token(&request_data.token),
    )
    .ok()
    .filter(|token| is_guest_confirmation(token) && usable_invitation(token));
    let user_uuid = match &guest {
        Some(token) => token.user_uuid,
        None => match repository::reset_tokens::validate_and_consume_token(
            &mut conn,
            &request_data.token,
            TokenType::Invitation.as_str(),
        ) {
            Ok(uuid) => uuid,
            Err(e) => {
                warn!("Invalid invitation token: {}", e);
                return Ok(errors::bad_request(e));
            }
        },
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

    // Hosted disables local passwords, so accepting an invitation to set one is
    // not applicable there (staff are control-plane seats, requesters use the
    // portal). Fail cleanly rather than 500 inside the credential writes below.
    if !crate::middleware::workspace_context::local_credentials_permitted() {
        return Ok(errors::local_auth_disabled());
    }

    match &guest {
        Some(token) => {
            // The password and the confirmation commit together, so a failed
            // release sets no password either.
            let mut failure = None;
            let committed = diesel::Connection::transaction::<_, diesel::result::Error, _>(
                &mut conn,
                |c| {
                    // A request-form confirmation sets a password only on a
                    // guest account with no way to sign in, read in this
                    // transaction before the password is written. Any other
                    // is refused and rolled back, so the link stays unspent
                    // and confirms without one (`confirm_guest_submission`).
                    let done = match guest_confirmation_sets_password(c, &user) {
                        Ok(true) => Ok(()),
                        Ok(false) => Err(ApiError::BadRequest(
                            "This request is confirmed without a password.".into(),
                        )),
                        Err(e) => {
                            error!(user_uuid = %user.uuid, error = %e, "Could not read the account's sign-in methods");
                            Err(ApiError::Internal("Failed to confirm submission".into()))
                        }
                    };
                    let done = done
                        .and_then(|()| store_local_password(c, &user, &password_hash))
                        .and_then(|()| {
                            complete_verification(
                                &db_pool,
                                c,
                                &search_service,
                                &user,
                                Completion::GuestPasswordSet,
                                submitted_in(token),
                                Some(&token.token_hash),
                            )
                        });
                    done.map_err(|e| {
                        failure = Some(e);
                        diesel::result::Error::RollbackTransaction
                    })
                },
            );
            if let Err(e) = committed {
                return Err(failure.unwrap_or_else(|| {
                    error!(error = %e, "Guest confirmation with a password failed");
                    ApiError::Internal("Failed to confirm submission".into())
                }));
            }
        }
        None => {
            store_local_password(&mut conn, &user, &password_hash)?;
            complete_verification(
                &db_pool,
                &mut conn,
                &search_service,
                &user,
                Completion::PasswordSet,
                None,
                // An invitation names no held request.
                None,
            )?;
        }
    }

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

/// Set `user`'s local password to `password_hash`, updating their local
/// identity or creating it.
fn store_local_password(
    conn: &mut DbConnection,
    user: &crate::models::User,
    password_hash: &str,
) -> Result<(), ApiError> {
    // Update the user's password hash in user_auth_identities
    use crate::schema::user_auth_identities;
    use diesel::prelude::*;

    // First, check if a local auth identity already exists
    let existing_identity: Option<i32> = user_auth_identities::table
        .filter(user_auth_identities::user_uuid.eq(&user.uuid))
        .filter(user_auth_identities::provider_type.eq("local"))
        .select(user_auth_identities::id)
        .first(conn)
        .optional()
        .ok()
        .flatten();

    if existing_identity.is_some() {
        // Update existing local auth identity
        if let Err(e) = repository::user_auth_identities::update_local_password_hash(
            conn,
            &user.uuid,
            password_hash,
        ) {
            error!("Failed to update password hash for invitation: {:?}", e);
            return Err(ApiError::Internal("Error setting password".into()));
        }
    } else {
        // Create new local auth identity
        let user_email = repository::user_helpers::get_primary_email(&user.uuid, conn)
            .unwrap_or_else(|| format!("user-{}", user.uuid));

        let auth_identity = crate::models::NewUserAuthIdentity {
            user_uuid: user.uuid,
            provider_type: "local".to_string(),
            external_id: user_email.clone(),
            email: Some(user_email.clone()),
            metadata: None,
            password_hash: Some(password_hash.to_string()),
            workspace_id: None,
        };

        if let Err(e) = repository::user_auth_identities::create_local_identity(auth_identity, conn)
        {
            error!("Failed to create auth identity for invitation: {:?}", e);
            return Err(ApiError::Internal("Error setting password".into()));
        }
    }

    Ok(())
}

/// Confirm a guest ticket submission from the emailed link, without setting a
/// password. Hosted has no local credentials, so the password step of
/// [`accept_invitation`] can't run there; confirming the address is what
/// releases the held ticket either way.
///
/// The token is checked first, then spent in the release's own transaction,
/// so a failed release leaves the link usable and a second click finds it
/// spent.
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
    if !is_guest_confirmation(&token) || !usable_invitation(&token) {
        return Err(invalid());
    }

    // A deleted account's request is never released.
    let user = repository::users::find_active_by_uuid(&token.user_uuid, &mut conn)
        .map_err(|_| invalid())?;
    // The workspace whose form sent the confirmation. A reused account can
    // hold seats in several, and its held request is in this one.
    let submitted_in = submitted_in(&token);
    // Redeemed on another workspace's origin, the link would release there and
    // sign in here; it is refused as any other bad link is.
    let origin_workspace = http_request
        .extensions()
        .get::<crate::extractors::WorkspaceContext>()
        .map(|ctx| ctx.workspace_id);
    if matches!((submitted_in, origin_workspace), (Some(held), Some(here)) if held != here) {
        return Err(invalid());
    }

    let released = complete_verification(
        &db_pool,
        &mut conn,
        &search_service,
        &user,
        Completion::GuestConfirmed,
        submitted_in,
        Some(&token.token_hash),
    )?;

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
    // The newest request it was sent for, or, when those were released
    // already, the newest that rode along.
    let newest = released
        .iter()
        .filter(|t| t.verified)
        .max_by_key(|t| t.id)
        .or_else(|| released.iter().max_by_key(|t| t.id))
        .map(|t| t.number);
    if let (Some(ctx), Some(number)) = (portal_ctx, newest) {
        match crate::handlers::portal::mint_portal_session(
            &user,
            ctx.workspace_uuid,
            &http_request,
            &mut conn,
            crate::handlers::portal::PortalScope::Full,
            // The confirmation link came to their inbox.
            crate::handlers::auth::SignInProof::Credential,
        ) {
            Ok(session) => {
                return Ok(HttpResponse::Ok()
                    .cookie(session.access)
                    .cookie(session.refresh)
                    .cookie(session.csrf)
                    .json(json!({
                        "success": true,
                        "message": "Your request has been confirmed.",
                        "redirect_to": crate::utils::ticket_link::ticket_route(number),
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

/// Whether following a request-form confirmation may set a local password on
/// `user`, its account: only where local passwords exist, on an account with
/// no way to sign in (a password, a provider identity, a passkey) and no
/// verified address. The form holds a request for an account with a verified
/// address only when it has no seat here, and that account signs in its own
/// way, so its confirmation never sets one.
fn guest_confirmation_sets_password(
    conn: &mut DbConnection,
    user: &crate::models::User,
) -> diesel::QueryResult<bool> {
    Ok(
        crate::middleware::workspace_context::local_credentials_permitted()
            && !repository::user_auth_identities::has_credentials(conn, user.uuid)?
            && !repository::user_emails::get_user_emails_by_uuid(conn, &user.uuid)?
                .iter()
                .any(|row| row.is_verified),
    )
}

/// Whether `token` is a request form's confirmation (rather than a staff or
/// requester invitation).
fn is_guest_confirmation(token: &crate::models::ResetToken) -> bool {
    token
        .metadata
        .as_ref()
        .and_then(|m| m.get("source"))
        .and_then(|s| s.as_str())
        == Some("guest_ticket_submission")
}

/// An unused, unexpired invitation token.
fn usable_invitation(token: &crate::models::ResetToken) -> bool {
    let expires_at = chrono::DateTime::<Utc>::from_naive_utc_and_offset(token.expires_at, Utc);
    token.token_type == TokenType::Invitation.as_str()
        && !token.is_used
        && !crate::utils::reset_tokens::ResetTokenUtils::is_token_expired(expires_at)
}

/// How long before its confirmation token a request filed with it can have
/// been written. Until tokens named their tickets the form wrote the ticket
/// in one transaction, then moved its attachments (at most
/// `GUEST_MAX_FILES_PER_TICKET` of `GUEST_MAX_FILE_SIZE_MB` each) and logged
/// the filing, then minted the token in another; both timestamps are the
/// database's `now()`. Two minutes covers that with room, and is short enough
/// that an earlier filing for the address is outside it. Two filings for the
/// address inside that gap could swap tickets; accepted for links that expire
/// within 7 days of this change.
const FILED_WITH_TOKEN_WINDOW: chrono::Duration = chrono::Duration::minutes(2);

/// Which held requests confirming a guest confirmation releases.
enum Release {
    /// The ones its token was sent for (`ticket_ids`), verified, and those
    /// that rode along on it while its address's confirmations were held
    /// (`rode_along`), released unconfirmed.
    Named {
        sent_for: Vec<i32>,
        rode_along: Vec<i32>,
    },
    /// A token from before tokens named their tickets: the request filed
    /// with it, the requester's newest pending one in the workspace written
    /// within [`FILED_WITH_TOKEN_WINDOW`] before or at its minting.
    FiledWith(chrono::DateTime<Utc>),
}

impl Release {
    /// The ticket ids to verify, read in the confirmation's transaction.
    fn ids(
        &self,
        conn: &mut DbConnection,
        user_uuid: uuid::Uuid,
        workspace_id: i32,
    ) -> diesel::QueryResult<Vec<i32>> {
        match self {
            Self::Named { sent_for, .. } => Ok(sent_for.clone()),
            Self::FiledWith(minted_at) => Ok(repository::tickets::pending_ticket_filed_before(
                conn,
                user_uuid,
                workspace_id,
                *minted_at,
                FILED_WITH_TOKEN_WINDOW,
            )?
            .into_iter()
            .collect()),
        }
    }

    /// The ticket ids to release unconfirmed.
    fn rode_along(&self) -> &[i32] {
        match self {
            Self::Named { rode_along, .. } => rode_along,
            Self::FiledWith(_) => &[],
        }
    }
}

/// The ticket ids in a token's metadata under `key`.
fn ids_at(token: &crate::models::ResetToken, key: &str) -> Option<Vec<i32>> {
    token
        .metadata
        .as_ref()
        .and_then(|m| m.get(key))
        .and_then(serde_json::Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(serde_json::Value::as_i64)
                .filter_map(|id| i32::try_from(id).ok())
                .collect()
        })
}

/// The requests a guest confirmation was sent for (`ticket_ids`): confirming
/// releases those and no other pending request naming the address, which may
/// be a stranger's. A token minted before confirmations named them releases
/// the one request filed with it ([`Release::FiledWith`]).
fn named_tickets(token: &crate::models::ResetToken) -> Release {
    let Some(sent_for) = ids_at(token, "ticket_ids") else {
        return Release::FiledWith(token.created_at.and_utc());
    };
    Release::Named {
        sent_for,
        rode_along: ids_at(token, "rode_along").unwrap_or_default(),
    }
}

/// The address a guest confirmation was mailed to (`sent_to`); `None` on one
/// minted before confirmations recorded it, which went to the address typed
/// on the form, so it proves no address.
fn confirmed_address(token: &crate::models::ResetToken) -> Option<String> {
    token
        .metadata
        .as_ref()
        .and_then(|m| m.get("sent_to"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
}

/// The workspace a guest confirmation's form is in; `None` on one minted
/// before confirmations recorded it.
fn submitted_in(token: &crate::models::ResetToken) -> Option<i32> {
    token
        .metadata
        .as_ref()
        .and_then(|m| m.get("workspace_id"))
        .and_then(serde_json::Value::as_i64)
        .and_then(|id| i32::try_from(id).ok())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Completion {
    /// Invitation accepted with a new local password.
    PasswordSet,
    /// Guest submission confirmed without a password. The release is the
    /// point of the request, so a failed release is an error the caller
    /// surfaces (the token is still unclaimed and the link can be retried).
    GuestConfirmed,
    /// Guest submission confirmed by setting a local password: both of the
    /// above.
    GuestPasswordSet,
}

impl Completion {
    fn sets_password(self) -> bool {
        matches!(self, Self::PasswordSet | Self::GuestPasswordSet)
    }

    fn releases_a_guest_request(self) -> bool {
        matches!(self, Self::GuestConfirmed | Self::GuestPasswordSet)
    }
}

/// Steps shared by every token-verified acceptance, returning any guest
/// tickets released: stamp the membership
/// accepted, mark the primary email verified, and release any guest tickets
/// held on this confirmation.
fn complete_verification(
    db_pool: &web::Data<crate::db::Pool>,
    conn: &mut DbConnection,
    search_service: &web::Data<Arc<SearchService>>,
    user: &crate::models::User,
    completion: Completion,
    workspace: Option<i32>,
    confirmation: Option<&str>,
) -> Result<Vec<ReleasedTicket>, ApiError> {
    // Pre-session (token-verified) flow: resolve the audit workspace once,
    // then thread it through the audited writes below
    // (users.password_changed_at, the seat and the ticket release). A guest
    // confirmation names its form's workspace, on the passwordless and the
    // password path alike, and its request is held there; the user's primary
    // membership stands in only for a staff or requester invitation, or a
    // confirmation minted before confirmations named one. The
    // lookup reads workspace_members (RLS-isolated) and is cross-tenant here
    // (token-verified, no request workspace pinned), so it runs elevated;
    // otherwise the runtime role reads no membership and the accept 500s.
    // Fail loud if the user genuinely has no membership.
    let resolved = match workspace {
        Some(ws) => Ok(ws),
        None => {
            // cross-tenant: pre-session workspace resolution: only the invited user is known here.
            crate::sync::session::background_run(db_pool, "background:invitation_accept", |c| {
                crate::repository::workspaces::primary_workspace_for_user(c, user.uuid)
            })
        }
    };
    let workspace_id = match resolved {
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

    // A guest confirmation is one transaction: the password's timestamp,
    // the membership stamp, the verified address, the seat and the release.
    // A failure changes nothing, so the address isn't left verified with its
    // request still held, and the link can be followed again.
    let released = if completion.releases_a_guest_request() {
        // The link is spent in this transaction, and what it releases is read
        // from the row as spent, so a request that rode along on it a moment
        // before is released with the rest. A failure rolls the spend back
        // with everything else, leaving the link usable; a confirm that finds
        // it spent already is refused as any used link is.
        let mut spent_already = false;
        let mut refused = false;
        let done = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
            conn,
            &actor,
            |c| {
                let Some(token) = confirmation
                    .map(|hash| repository::reset_tokens::claim_unused(c, hash))
                    .transpose()?
                    .flatten()
                else {
                    spent_already = true;
                    return Err(diesel::result::Error::RollbackTransaction);
                };
                let release = named_tickets(&token);
                let confirmed_address = confirmed_address(&token);
                // Whoever the request is released under must still be a live
                // account within the form's reach now, whatever it was when the
                // request was filed: no platform role and no staff seat
                // elsewhere. Otherwise it stays held, and the link is answered
                // as an invalid one, unspent.
                let active = match repository::users::find_active_by_uuid(&user.uuid, c) {
                    Ok(_) => true,
                    Err(diesel::result::Error::NotFound) => false,
                    Err(e) => return Err(e),
                };
                if !active
                    || repository::requester_identities::out_of_reach(c, workspace_id, user.uuid)?
                        .is_some()
                {
                    refused = true;
                    return Err(diesel::result::Error::RollbackTransaction);
                }
                if completion.sets_password() {
                    repository::users::set_password_changed_at(
                        c,
                        &user.uuid,
                        Utc::now().naive_utc(),
                    )?;
                }
                repository::workspaces::mark_memberships_accepted(c, user.uuid)?;
                // The confirmation POST proves the mailbox it was sent to. One
                // minted before confirmations recorded that proves nothing: it
                // went to the address typed on the form, which may not be the
                // primary.
                if let Some(address) = confirmed_address.as_deref() {
                    repository::user_emails::mark_address_proven(c, &user.uuid, address)?;
                }
                // A reused account that wrote to this workspace's form from a seat
                // held elsewhere is seated here, now that the address is proven,
                // so the released request's requester is a member when its
                // `ticket.created` goes out and a portal session can be minted.
                // One that can't be (removed here, or seated nowhere) gets
                // nothing released: the request stays held and the link is
                // answered as an invalid one.
                if !repository::workspaces::join_as_requester(c, workspace_id, user.uuid)?
                    && repository::workspaces::membership(c, workspace_id, user.uuid)?.is_none()
                {
                    refused = true;
                    return Err(diesel::result::Error::RollbackTransaction);
                }
                // The requests this confirmation was sent for, verified.
                let ids = release.ids(c, user.uuid, workspace_id)?;
                let mut released =
                    repository::tickets::verify_pending_tickets_for_user(c, user.uuid, &ids)?;
                // Requests that rode along go live but stay unconfirmed: the click
                // proves the mailbox, not that its holder wrote them.
                released.extend(repository::tickets::release_unconfirmed_tickets_for_user(
                    c,
                    user.uuid,
                    release.rode_along(),
                )?);
                Ok(released)
            },
        );
        match done {
            Ok(released) => released,
            Err(_) if spent_already || refused => {
                return Err(ApiError::BadRequest(
                    "Invalid or expired confirmation link".into(),
                ))
            }
            Err(e) => {
                warn!(user_uuid = %user.uuid, error = %e, "Failed to confirm guest submission");
                return Err(ApiError::Internal("Failed to confirm submission".into()));
            }
        }
    } else {
        accept_invitation_steps(conn, &actor, workspace_id, user, completion)
    };

    // Each newly-verified ticket goes to the search index so techs pick it
    // up immediately; the same side-effects that would have fired at submit
    // time for a non-gated ticket. Released tickets reach clients through
    // the sync pool (the repository write emits `ticket.created`), and the
    // rules engine runs on them then.
    if !released.is_empty() {
        info!(
            user_uuid = %user.uuid,
            count = released.len(),
            "Released pending guest tickets on invitation acceptance"
        );
    }
    Ok(released
        .into_iter()
        .map(|ticket| {
            let out = ReleasedTicket {
                id: ticket.id,
                number: ticket.number,
                verified: ticket.verification_state.as_deref() == Some("verified"),
            };
            indexing_tasks::spawn_index_ticket(search_service.get_ref().clone(), ticket, None);
            out
        })
        .collect())
}

/// A staff or requester invitation's accept steps, each best-effort as it
/// always was: the password's timestamp, the membership stamp, the verified
/// address and the seat. It releases no held request: an invitation names
/// none, and one waiting may be a stranger's.
fn accept_invitation_steps(
    conn: &mut DbConnection,
    actor: &crate::sync::actor::ActorContext,
    workspace_id: i32,
    user: &crate::models::User,
    completion: Completion,
) -> Vec<crate::models::Ticket> {
    // Update password_changed_at timestamp in users table
    if completion.sets_password() {
        let now = Utc::now().naive_utc();
        if let Err(e) = crate::sync::session::with_actor_context(conn, actor, |c| {
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
    if let Err(e) = crate::sync::session::with_actor_context(conn, actor, |c| {
        repository::workspaces::mark_memberships_accepted(c, user.uuid)
    }) {
        warn!("Failed to stamp workspace membership accepted_at: {:?}", e);
        // Don't fail the request for this
    }

    // Mark user's primary email as verified (they proved ownership by receiving the invitation)
    if let Err(e) = repository::user_emails::mark_primary_proven(conn, &user.uuid) {
        warn!("Failed to mark email as verified: {:?}", e);
        // Don't fail the request for this
    }

    if let Err(e) = crate::sync::session::with_actor_context(conn, actor, |c| {
        repository::workspaces::join_as_requester(c, workspace_id, user.uuid)
    }) {
        warn!(user_uuid = %user.uuid, error = %e, "Failed to seat the invited person");
    }
    Vec::new()
}

/// A guest ticket a verification released: its id, the number the portal's
/// links use, and whether it was one the link was sent for (verified) rather
/// than one that rode along.
struct ReleasedTicket {
    id: i32,
    number: i32,
    verified: bool,
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
