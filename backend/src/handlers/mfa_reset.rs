use actix_web::{web, HttpRequest, HttpResponse, Responder};
use serde_json::json;
use chrono::{Duration, Utc};

use crate::models::{MfaResetRequest, MfaResetResponse, MfaResetCompleteRequest};
use crate::repository;
use crate::utils::reset_tokens::{ResetTokenUtils, TokenType};
use crate::utils::email::EmailService;

/// Maximum MFA reset requests per user per hour (rate limiting)
const MAX_MFA_RESET_REQUESTS_PER_HOUR: i64 = 3;

/// Request MFA reset - Send email with reset link
pub async fn request_mfa_reset(
    db_pool: web::Data<crate::db::Pool>,
    request_data: web::Json<MfaResetRequest>,
    http_request: HttpRequest,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Find user by email
    let user = match repository::get_user_by_email(&request_data.email, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            // Don't reveal if user exists or not (prevent account enumeration)
            return HttpResponse::Ok().json(MfaResetResponse {
                message: "If an account with that email exists and has MFA enabled, a reset link has been sent.".to_string(),
                token_id: None,
                requires_admin_approval: false,
            });
        }
    };

    // Verify password
    let password_hash = match String::from_utf8(user.password_hash.clone()) {
        Ok(hash) if !hash.is_empty() => hash,
        _ => {
            // Don't reveal if password authentication is available
            return HttpResponse::Ok().json(MfaResetResponse {
                message: "If an account with that email exists and has MFA enabled, a reset link has been sent.".to_string(),
                token_id: None,
                requires_admin_approval: false,
            });
        }
    };

    // Verify password matches
    if !bcrypt::verify(&request_data.password, &password_hash).unwrap_or(false) {
        // Wrong password - return success message to prevent enumeration
        return HttpResponse::Ok().json(MfaResetResponse {
            message: "If an account with that email exists and has MFA enabled, a reset link has been sent.".to_string(),
            token_id: None,
            requires_admin_approval: false,
        });
    }

    // Check if user has MFA enabled
    if !user.mfa_enabled {
        // MFA not enabled - return success message to prevent enumeration
        return HttpResponse::Ok().json(MfaResetResponse {
            message: "If an account with that email exists and has MFA enabled, a reset link has been sent.".to_string(),
            token_id: None,
            requires_admin_approval: false,
        });
    }

    // Check rate limiting - how many MFA reset requests in the last hour?
    let since = Utc::now() - Duration::hours(1);
    let recent_count = match repository::reset_tokens::count_recent_tokens(
        &mut conn,
        user.uuid,
        TokenType::MfaReset.as_str(),
        since,
    ) {
        Ok(count) => count,
        Err(e) => {
            tracing::error!("Failed to count recent MFA reset tokens: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to process MFA reset request"
            }));
        }
    };

    if recent_count >= MAX_MFA_RESET_REQUESTS_PER_HOUR {
        // Rate limit exceeded - still return success to prevent enumeration
        tracing::warn!("MFA reset rate limit exceeded for user: {}", user.uuid);
        return HttpResponse::Ok().json(MfaResetResponse {
            message: "If an account with that email exists and has MFA enabled, a reset link has been sent.".to_string(),
            token_id: None,
            requires_admin_approval: false,
        });
    }

    // Generate reset token (15 minute expiration for MFA reset)
    let reset_token_data = ResetTokenUtils::create_reset_token(user.uuid, TokenType::MfaReset);

    // Extract IP address and user agent
    let ip_address = http_request.peer_addr()
        .map(|addr| addr.ip().to_string());

    let user_agent = http_request.headers()
        .get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Store token in database
    if let Err(e) = repository::reset_tokens::create_reset_token(
        &mut conn,
        &reset_token_data.token_hash,
        user.uuid,
        TokenType::MfaReset.as_str(),
        ip_address.as_deref(),
        user_agent.as_deref(),
        reset_token_data.expires_at,
        None, // No metadata needed for MFA reset
    ) {
        tracing::error!("Failed to create MFA reset token: {}", e);
        return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to process MFA reset request"
        }));
    }

    // Send email with reset link
    let email_service = match EmailService::from_env() {
        Ok(service) => service,
        Err(e) => {
            tracing::error!("Failed to initialize email service: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Email service not configured"
            }));
        }
    };

    // Get base URL from environment or request
    let base_url = std::env::var("FRONTEND_URL")
        .unwrap_or_else(|_| "http://localhost:8080".to_string());

    // Send MFA reset email
    if let Err(e) = email_service.send_mfa_reset_email(
        &user.email,
        &user.name,
        &reset_token_data.raw_token,
        &base_url,
    ).await {
        tracing::error!("Failed to send MFA reset email: {}", e);
        return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to send reset email"
        }));
    }

    tracing::info!("MFA reset requested for user: {}", user.uuid);

    HttpResponse::Ok().json(MfaResetResponse {
        message: "If an account with that email exists and has MFA enabled, a reset link has been sent.".to_string(),
        token_id: None,
        requires_admin_approval: false,
    })
}

/// Complete MFA reset - Validate token and return limited-scope JWT for MFA management
pub async fn complete_mfa_reset(
    db_pool: web::Data<crate::db::Pool>,
    request_data: web::Json<MfaResetCompleteRequest>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate and consume the token
    let user_uuid = match repository::reset_tokens::validate_and_consume_token(
        &mut conn,
        &request_data.token,
        TokenType::MfaReset.as_str(),
    ) {
        Ok(uuid) => uuid,
        Err(e) => {
            tracing::warn!("Invalid MFA reset token attempt: {}", e);
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
            tracing::error!("User not found for valid MFA reset token: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "User not found"
            }));
        }
    };

    // Create a limited-scope JWT token for MFA recovery (15 minutes)
    let limited_token = match crate::utils::jwt::JwtUtils::create_mfa_recovery_token(&user) {
        Ok(token) => token,
        Err(e) => {
            tracing::error!("Failed to create MFA recovery token: {}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create recovery session"
            }));
        }
    };

    // Log security event
    use diesel::prelude::*;
    use crate::schema::security_events;

    #[derive(diesel::Insertable)]
    #[diesel(table_name = security_events)]
    struct NewSecurityEvent {
        user_uuid: uuid::Uuid,
        event_type: String,
        ip_address: Option<String>,
        user_agent: Option<String>,
        location: Option<String>,
        details: Option<serde_json::Value>,
        severity: String,
        created_at: chrono::NaiveDateTime,
        session_id: Option<i32>,
    }

    let new_event = NewSecurityEvent {
        user_uuid,
        event_type: "mfa_recovery_link_used".to_string(),
        ip_address: None,
        user_agent: None,
        location: None,
        details: Some(json!({
            "action": "mfa_recovery_session_created",
            "success": true
        })),
        severity: "warning".to_string(), // MFA recovery is a security-sensitive action
        created_at: chrono::Utc::now().naive_utc(),
        session_id: None,
    };

    if let Err(e) = diesel::insert_into(security_events::table)
        .values(&new_event)
        .execute(&mut conn) {
        tracing::warn!("Failed to log MFA recovery security event: {}", e);
        // Don't fail if logging fails
    }

    tracing::info!("MFA recovery session created for user: {}", user.uuid);

    HttpResponse::Ok().json(json!({
        "status": "success",
        "message": "Recovery link validated. You can now manage your MFA settings.",
        "token": limited_token,
        "user": crate::models::UserResponse::from(user),
        "scope": "mfa_recovery",
        "expires_in": 900 // 15 minutes in seconds
    }))
}
