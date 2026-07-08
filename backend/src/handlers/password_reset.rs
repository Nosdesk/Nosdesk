use actix_web::{web, HttpRequest, HttpResponse, Responder};
use chrono::{Duration, Utc};
use serde_json::json;
use tracing::{error, info, warn};

use crate::db::DbConnection;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::models::{PasswordResetCompleteRequest, PasswordResetRequest, PasswordResetResponse};
use crate::repository;
use crate::utils::auth::hash_password;
use crate::utils::email::EmailService;
use crate::utils::email_branding::get_email_branding;
use crate::utils::reset_tokens::{ResetTokenUtils, TokenType};

/// Rate limiting: Maximum password reset requests per user within time window
const MAX_RESET_REQUESTS_PER_HOUR: i64 = 3;

/// Request a password reset - sends email with reset link.
///
/// AUD-007: the handler always returns the same generic success
/// message at constant latency. The token-issue + email-send
/// work happens in a detached `tokio::spawn` task so missing
/// users and real users return on the same timeline. The
/// previous in-line `.await` leaked existence via SMTP round-
/// trip latency even though the response body was identical.
pub async fn request_password_reset(
    db_pool: web::Data<crate::db::Pool>,
    request_data: web::Json<PasswordResetRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let email = request_data.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return errors::bad_request("Invalid email address");
    }

    let ip_address =
        crate::utils::client_ip::from_http_request(&http_request).map(|ip| ip.to_string());
    let user_agent = http_request
        .headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let pool = db_pool.clone();
    tokio::spawn(async move {
        if let Err(e) = issue_password_reset(pool, email, ip_address, user_agent).await {
            // Errors are logged inside; this branch is only hit on
            // unrecoverable failures. Never re-throw to the caller.
            error!(error = %e, "password reset background task failed");
        }
    });

    HttpResponse::Ok().json(PasswordResetResponse {
        message: "If an account with that email exists, a password reset link has been sent."
            .to_string(),
    })
}

/// The actual work, off the response path.
///
/// The identity tables this touches (`users`, `reset_tokens`,
/// `user_emails`, `user_preferences`) are global, not RLS-protected, so
/// the cross-tenant email lookup and token issue run on a plain pooled
/// connection. The two RLS-isolated reads/writes (the `site_settings`
/// branding read and the `outbound_emails` enqueue) run pinned to the
/// recipient's workspace via `run_in_workspace` (RLS enforced), so the
/// branding read returns this workspace's settings and the enqueue gets the
/// `app.workspace_id` its NOT NULL default needs.
async fn issue_password_reset(
    pool: web::Data<crate::db::Pool>,
    email: String,
    ip_address: Option<String>,
    user_agent: Option<String>,
) -> Result<(), String> {
    let mut conn = pool.get().map_err(|e| format!("db pool: {e}"))?;

    let user = match repository::get_user_by_email(&email, &mut conn) {
        Ok(u) => u,
        Err(_) => {
            info!("Password reset requested for non-existent email: {}", email);
            return Ok(());
        }
    };

    let since = Utc::now() - Duration::hours(1);
    let recent_count = repository::reset_tokens::count_recent_tokens(
        &mut conn,
        user.uuid,
        TokenType::PasswordReset.as_str(),
        since,
    )
    .map_err(|e| format!("count_recent_tokens: {e}"))?;
    if recent_count >= MAX_RESET_REQUESTS_PER_HOUR {
        warn!(
            "Rate limit exceeded for password reset: user_uuid={}, ip={:?}",
            user.uuid, ip_address
        );
        return Ok(());
    }

    let reset_token = ResetTokenUtils::create_reset_token(user.uuid, TokenType::PasswordReset);
    repository::reset_tokens::create_reset_token(
        &mut conn,
        &reset_token.token_hash,
        user.uuid,
        TokenType::PasswordReset.as_str(),
        ip_address.as_deref(),
        user_agent.as_deref(),
        reset_token.expires_at,
        None,
    )
    .map_err(|e| format!("create_reset_token: {e}"))?;

    let Some(user_email) =
        crate::repository::user_helpers::get_primary_email(&user.uuid, &mut conn)
    else {
        warn!(
            "User {} has no primary email; password reset link not sent",
            user.uuid
        );
        return Ok(());
    };

    let email_service = match EmailService::from_env() {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to initialize email service: {}", e);
            return Ok(());
        }
    };

    // Resolve the recipient's primary workspace so the RLS-isolated branding
    // read and outbound enqueue are scoped correctly, and so the reset link is
    // built on the tenant's own origin. The lookup reads workspace_members
    // (RLS-isolated) and is inherently cross-tenant here (we only have the
    // email, no request workspace), so it runs elevated. A user with no
    // membership gets no email rather than a NULL-workspace insert failure.
    let workspace = match crate::sync::session::background_run(
        &pool,
        "background:password_reset",
        |c| {
            let id = repository::workspaces::primary_workspace_for_user(c, user.uuid)?;
            repository::workspaces::find_by_id(c, id)
        },
    ) {
        Ok(Some(ws)) => ws,
        Ok(None) => {
            warn!(user_uuid = %user.uuid, "password-reset recipient workspace not found");
            return Ok(());
        }
        Err(e) => {
            warn!(user_uuid = %user.uuid, error = %e, "no workspace for password-reset recipient");
            return Ok(());
        }
    };
    let workspace_id = workspace.id;

    // Tenant-canonical link host: the recipient workspace's canonical origin
    // (custom domain or `<slug>.<NOSDESK_TENANT_DOMAIN>`), else FRONTEND_URL.
    // We deliberately do NOT fall back to the request `Host` header: an
    // attacker who forges it could point the reset link at a domain they
    // control and harvest the token (account takeover). If neither a canonical
    // origin nor FRONTEND_URL is configured, refuse to send and tell the
    // operator to set FRONTEND_URL.
    let Some(base_url) = crate::utils::tenant_origin::email_link_base(
        crate::utils::tenant_origin::workspace_origin(&workspace),
    ) else {
        error!(
            user_uuid = %user.uuid,
            "refusing to send password reset: no canonical origin and FRONTEND_URL is unset; \
             set FRONTEND_URL so reset links can't be forged via the Host header"
        );
        return Ok(());
    };

    // Enqueue rather than fire-and-forget. The outbound worker retries with
    // backoff if SMTP burps, applies the suppression list, and respects the
    // circuit breaker. The idempotency key is derived from a hash of the raw
    // reset token so a network blip between this handler and the DB doesn't
    // deliver two copies of the same reset link.
    //
    // The branding read (site_settings) and the enqueue (outbound_emails) are
    // workspace-isolated. Run pinned as the RLS-enforced runtime role
    // (`run_in_workspace`, not the BYPASSRLS background variant): the branding
    // read has no explicit workspace filter, so under bypass it would return an
    // arbitrary tenant's settings — RLS scoping it to this workspace is what
    // keeps the reset email branded for the recipient's own workspace.
    let enqueue = crate::sync::session::run_in_workspace(
        &pool,
        "background:password_reset",
        workspace_id,
        |conn| {
            let branding = get_email_branding(conn, &base_url);
            let locale = crate::repository::user_locale::resolve_effective_locale(conn, user.uuid);
            crate::services::transactional_email::enqueue_password_reset(
                conn,
                &email_service,
                &branding,
                &user_email,
                &user.name,
                &reset_token.raw_token,
                &locale,
            )
        },
    );
    match enqueue {
        Ok(row) => info!(
            queue_id = row.id,
            user_uuid = %user.uuid,
            recipient = %user_email,
            "Password reset email enqueued"
        ),
        Err(e) => error!(
            user_uuid = %user.uuid,
            recipient = %user_email,
            error = %e,
            "Failed to enqueue password reset email"
        ),
    }
    Ok(())
}

/// Complete password reset using token
pub async fn reset_password_with_token(
    db_pool: web::Data<crate::db::Pool>,
    request_data: web::Json<PasswordResetCompleteRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Validate new password
    if request_data.new_password.len() < 8 {
        return errors::bad_request("Password must be at least 8 characters long");
    } else if request_data.new_password.len() > 128 {
        return errors::bad_request("Password must be less than 128 characters");
    }

    // Validate and consume the token
    let user_uuid = match repository::reset_tokens::validate_and_consume_token(
        &mut conn,
        &request_data.token,
        TokenType::PasswordReset.as_str(),
    ) {
        Ok(uuid) => uuid,
        Err(e) => {
            warn!("Invalid password reset token: {}", e);
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": e
            }));
        }
    };

    // Get the user — active-only. A soft-deleted user with a
    // valid reset token must not be allowed to set a new password
    // and re-enter the system. F2C.2 H4. Surfaced upstream as
    // the same generic "Invalid or expired token" so deletion
    // state doesn't leak through the response.
    let user = match repository::users::find_active_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            error!(
                "User not found for password reset: user_uuid={}, error={}",
                user_uuid, e
            );
            return errors::bad_request("Invalid or expired token");
        }
    };

    // Hash the new password
    let new_password_hash = match hash_password(&request_data.new_password) {
        Ok(hash) => hash,
        Err(e) => {
            error!("Failed to hash new password: {}", e);
            return errors::internal("Error processing new password");
        }
    };

    // Update the user's password hash in user_auth_identities and password_changed_at timestamp in users
    use diesel::prelude::*;
    let now = Utc::now().naive_utc();

    // Update password hash in user_auth_identities
    use crate::schema::user_auth_identities;
    if let Err(e) = diesel::update(
        user_auth_identities::table
            .filter(user_auth_identities::user_uuid.eq(&user.uuid))
            .filter(user_auth_identities::provider_type.eq("local")),
    )
    .set(user_auth_identities::password_hash.eq(Some(new_password_hash)))
    .execute(&mut conn)
    {
        error!("Failed to update password hash: {:?}", e);
        return errors::internal("Error updating password");
    }

    // Update password_changed_at timestamp in the audited users
    // table. Pre-session flow (token-verified, no JWT yet), so resolve
    // the audit workspace from the user's primary membership.
    let workspace_id =
        match crate::repository::workspaces::primary_workspace_for_user(&mut conn, user.uuid) {
            Ok(ws) => ws,
            Err(e) => {
                error!(
                    "Failed to resolve primary workspace for password reset: {:?}",
                    e
                );
                return errors::internal("Error updating password");
            }
        };
    let actor = crate::sync::actor::ActorContext::user_at_workspace(user.uuid, workspace_id);
    match crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
        diesel::update(crate::schema::users::table.find(&user.uuid))
            .set(crate::schema::users::password_changed_at.eq(now))
            .execute(c)
    }) {
        Ok(_) => {
            info!(
                "Password reset successfully for user: {} (uuid={})",
                user.name, user.uuid
            );

            // Log security event for password reset
            if let Err(e) = log_password_reset_event(&user.uuid, &http_request, &mut conn).await {
                warn!("Failed to log password reset event: {}", e);
                // Don't fail the password reset if logging fails
            }

            // Revoke all sessions for security (user must log in again)
            match crate::repository::active_sessions::revoke_other_sessions(
                &mut conn, &user.uuid,
                None, // Revoke ALL sessions including current (user must re-login)
            ) {
                Ok(revoked_count) => {
                    if revoked_count > 0 {
                        info!(
                            "Revoked {} session(s) after password reset for user: {}",
                            revoked_count, user.name
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to revoke sessions after password reset for user {}: {}",
                        user.uuid, e
                    );
                    // Don't fail the password reset if session revocation fails
                }
            }

            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "Password reset successfully. Please log in with your new password."
            }))
        }
        Err(e) => {
            error!("Failed to update password: {:?}", e);
            errors::internal("Error updating password")
        }
    }
}

/// Helper function to log password reset security event
async fn log_password_reset_event(
    user_uuid: &uuid::Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    use crate::utils::security_events::{record_security_event, SecurityEventInput};

    record_security_event(
        conn,
        SecurityEventInput {
            user_uuid: Some(*user_uuid),
            event_type: "password_reset",
            severity: "info",
            details: Some(json!({
                "action": "password_reset_completed",
                "method": "email_token",
                "success": true
            })),
            request: Some(request),
            session_id: None,
        },
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiting_constant() {
        assert_eq!(MAX_RESET_REQUESTS_PER_HOUR, 3);
    }
}
