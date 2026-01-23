use actix_web::{web, HttpResponse, Responder, HttpRequest};
use serde_json::json;
use chrono::{Duration, Utc};
use tracing::{info, warn, error};

use crate::db::DbConnection;
use crate::models::{PasswordResetRequest, PasswordResetResponse, PasswordResetCompleteRequest};
use crate::repository;
use crate::utils::auth::hash_password;
use crate::utils::reset_tokens::{TokenType, ResetTokenUtils};
use crate::utils::email::EmailService;
use crate::utils::email_branding::get_email_branding;

/// Rate limiting: Maximum password reset requests per user within time window
const MAX_RESET_REQUESTS_PER_HOUR: i64 = 3;

/// Request a password reset - sends email with reset link
pub async fn request_password_reset(
    db_pool: web::Data<crate::db::Pool>,
    request_data: web::Json<PasswordResetRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate email format
    let email = request_data.email.trim().to_lowercase();
    if email.is_empty() || !email.contains('@') {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid email address"
        }));
    }

    // Extract IP address and user agent for audit trail
    let ip_address = http_request.peer_addr()
        .map(|addr| addr.ip().to_string());

    let user_agent = http_request.headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Find user by email
    // IMPORTANT: We return the same response regardless of whether the user exists
    // This prevents account enumeration attacks
    let user = match repository::get_user_by_email(&email, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            // User doesn't exist - return success anyway to prevent enumeration
            info!("Password reset requested for non-existent email: {}", email);
            return HttpResponse::Ok().json(PasswordResetResponse {
                message: "If an account with that email exists, a password reset link has been sent.".to_string(),
            });
        }
    };

    // Check rate limiting - count recent tokens for this user
    let since = Utc::now() - Duration::hours(1);
    let recent_count = match repository::reset_tokens::count_recent_tokens(
        &mut conn,
        user.uuid,
        TokenType::PasswordReset.as_str(),
        since,
    ) {
        Ok(count) => count,
        Err(e) => {
            error!("Failed to check rate limit for password reset: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to process reset request"
            }));
        }
    };

    if recent_count >= MAX_RESET_REQUESTS_PER_HOUR {
        warn!("Rate limit exceeded for password reset: user_uuid={}, ip={:?}",
              user.uuid, ip_address);
        // Return success message to prevent enumeration, but don't send email
        return HttpResponse::Ok().json(PasswordResetResponse {
            message: "If an account with that email exists, a password reset link has been sent.".to_string(),
        });
    }

    // Generate reset token
    let reset_token = ResetTokenUtils::create_reset_token(user.uuid, TokenType::PasswordReset);

    // Store token hash in database
    if let Err(e) = repository::reset_tokens::create_reset_token(
        &mut conn,
        &reset_token.token_hash,
        user.uuid,
        TokenType::PasswordReset.as_str(),
        ip_address.as_deref(),
        user_agent.as_deref(),
        reset_token.expires_at,
        None, // No metadata needed for password reset
    ) {
        error!("Failed to create password reset token: {}", e);
        return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to process reset request"
        }));
    }

    // Get base URL from environment or request
    let base_url = std::env::var("FRONTEND_URL")
        .unwrap_or_else(|_| {
            // Fallback to constructing from request
            let scheme = if http_request.connection_info().scheme() == "https" {
                "https"
            } else {
                "http"
            };
            let host = http_request.connection_info().host().to_string();
            format!("{scheme}://{host}")
        });

    // Get user's primary email for sending reset link
    let user_email = match crate::repository::user_helpers::get_primary_email(&user.uuid, &mut conn) {
        Some(email) => email,
        None => {
            warn!("User {} has no primary email - cannot send password reset", user.uuid);
            // Return success anyway to prevent enumeration
            return HttpResponse::Ok().json(PasswordResetResponse {
                message: "If an account with that email exists, a password reset link has been sent.".to_string(),
            });
        }
    };

    // Send password reset email
    let email_service = match EmailService::from_env() {
        Ok(service) => service,
        Err(e) => {
            error!("Failed to initialize email service: {}", e);
            // Return success to user anyway (no enumeration)
            return HttpResponse::Ok().json(PasswordResetResponse {
                message: "If an account with that email exists, a password reset link has been sent.".to_string(),
            });
        }
    };

    // Get branding for email
    let branding = get_email_branding(&mut conn, &base_url);

    // Send the email asynchronously
    match email_service.send_password_reset_email(
        &user_email,
        &user.name,
        &reset_token.raw_token,
        &branding,
    ).await {
        Ok(_) => {
            info!("Password reset email sent to: {} (user_uuid={})", user_email, user.uuid);
        },
        Err(e) => {
            error!("Failed to send password reset email to {}: {}", user_email, e);
            // Don't fail the request - return success to prevent enumeration
        }
    }

    // Always return the same success message (no enumeration)
    HttpResponse::Ok().json(PasswordResetResponse {
        message: "If an account with that email exists, a password reset link has been sent.".to_string(),
    })
}

/// Complete password reset using token
pub async fn reset_password_with_token(
    db_pool: web::Data<crate::db::Pool>,
    request_data: web::Json<PasswordResetCompleteRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate new password
    if request_data.new_password.len() < 8 {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Password must be at least 8 characters long"
        }));
    } else if request_data.new_password.len() > 128 {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Password must be less than 128 characters"
        }));
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

    // Get the user
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            error!("User not found for password reset: user_uuid={}, error={}", user_uuid, e);
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid or expired token"
            }));
        }
    };

    // Hash the new password
    let new_password_hash = match hash_password(&request_data.new_password) {
        Ok(hash) => hash,
        Err(e) => {
            error!("Failed to hash new password: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error processing new password"
            }));
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
            .filter(user_auth_identities::provider_type.eq("local"))
    )
    .set(user_auth_identities::password_hash.eq(Some(new_password_hash)))
    .execute(&mut conn) {
        error!("Failed to update password hash: {:?}", e);
        return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Error updating password"
        }));
    }

    // Update password_changed_at timestamp in users table
    match diesel::update(crate::schema::users::table.find(&user.uuid))
        .set(crate::schema::users::password_changed_at.eq(now))
        .execute(&mut conn) {
        Ok(_) => {
            info!("Password reset successfully for user: {} (uuid={})", user.name, user.uuid);

            // Log security event for password reset
            if let Err(e) = log_password_reset_event(&user.uuid, &http_request, &mut conn).await {
                warn!("Failed to log password reset event: {}", e);
                // Don't fail the password reset if logging fails
            }

            // Revoke all sessions for security (user must log in again)
            match crate::repository::active_sessions::revoke_other_sessions(
                &mut conn,
                &user.uuid,
                None, // Revoke ALL sessions including current (user must re-login)
            ) {
                Ok(revoked_count) => {
                    if revoked_count > 0 {
                        info!("Revoked {} session(s) after password reset for user: {}",
                              revoked_count, user.name);
                    }
                },
                Err(e) => {
                    warn!("Failed to revoke sessions after password reset for user {}: {}",
                          user.uuid, e);
                    // Don't fail the password reset if session revocation fails
                }
            }

            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "Password reset successfully. Please log in with your new password."
            }))
        },
        Err(e) => {
            error!("Failed to update password: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error updating password"
            }))
        }
    }
}

/// Helper function to log password reset security event
async fn log_password_reset_event(
    user_uuid: &uuid::Uuid,
    request: &HttpRequest,
    conn: &mut DbConnection,
) -> Result<(), Box<dyn std::error::Error>> {
    use diesel::prelude::*;
    use crate::schema::security_events;

    // Extract IP address and user agent
    let ip_address = request.peer_addr()
        .and_then(|addr| addr.ip().to_string().parse().ok());

    let user_agent = request.headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    #[derive(diesel::Insertable)]
    #[diesel(table_name = security_events)]
    struct NewSecurityEvent {
        user_uuid: uuid::Uuid,
        event_type: String,
        ip_address: Option<ipnetwork::IpNetwork>,
        user_agent: Option<String>,
        location: Option<String>,
        details: Option<serde_json::Value>,
        severity: String,
        created_at: chrono::NaiveDateTime,
        session_id: Option<i32>,
    }

    let new_event = NewSecurityEvent {
        user_uuid: *user_uuid,
        event_type: "password_reset".to_string(),
        ip_address,
        user_agent,
        location: None,
        details: Some(json!({
            "action": "password_reset_completed",
            "method": "email_token",
            "success": true
        })),
        severity: "info".to_string(),
        created_at: Utc::now().naive_utc(),
        session_id: None,
    };

    diesel::insert_into(security_events::table)
        .values(&new_event)
        .execute(conn)?;

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
