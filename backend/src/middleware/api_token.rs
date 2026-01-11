//! API Token Authentication Middleware
//!
//! Provides Bearer token authentication for programmatic API access.
//! Works alongside cookie-based authentication.

use actix_web::{dev::ServiceRequest, web, Error, HttpMessage};
use std::net::IpAddr;
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::models::Claims;
use crate::repository::api_tokens::{get_valid_api_token, hash_token, update_token_last_used};

/// Marker struct to indicate request was authenticated via API token
/// This is used by CSRF middleware to skip validation for API token requests
#[derive(Clone, Debug)]
pub struct ApiTokenAuth {
    pub token_uuid: uuid::Uuid,
}

/// Extract Bearer token from Authorization header
pub fn extract_bearer_token(req: &ServiceRequest) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Extract client IP from request
fn extract_client_ip(req: &ServiceRequest) -> Option<IpAddr> {
    // Try X-Forwarded-For first (for reverse proxy setups)
    if let Some(forwarded) = req.headers().get("X-Forwarded-For") {
        if let Ok(forwarded_str) = forwarded.to_str() {
            if let Some(first_ip) = forwarded_str.split(',').next() {
                if let Ok(ip) = first_ip.trim().parse::<IpAddr>() {
                    return Some(ip);
                }
            }
        }
    }

    // Fall back to connection info
    req.connection_info()
        .realip_remote_addr()
        .and_then(|addr| {
            addr.split(':')
                .next()
                .and_then(|ip| ip.parse::<IpAddr>().ok())
        })
}

/// Try to authenticate request via Bearer token
/// Returns Ok(Some(Claims)) if authenticated, Ok(None) if no Bearer token, Err on auth failure
pub fn try_bearer_auth(
    req: &ServiceRequest,
    pool: &web::Data<Pool>,
) -> Result<Option<Claims>, Error> {
    // Check for Bearer token
    let token = match extract_bearer_token(req) {
        Some(t) => t,
        None => return Ok(None), // No Bearer token, let cookie auth handle it
    };

    // Validate token format (should start with nsk_)
    if !token.starts_with("nsk_") {
        warn!(path = %req.path(), "Invalid API token format");
        return Err(actix_web::error::ErrorUnauthorized("Invalid API token format"));
    }

    debug!(path = %req.path(), "Attempting Bearer token authentication");

    // Get database connection
    let mut conn = pool
        .get()
        .map_err(|e| {
            error!("Database connection failed: {}", e);
            actix_web::error::ErrorInternalServerError("Database connection failed")
        })?;

    // Hash token and look up
    let token_hash = hash_token(&token);
    let api_token = match get_valid_api_token(&mut conn, &token_hash) {
        Ok(t) => t,
        Err(diesel::result::Error::NotFound) => {
            warn!(path = %req.path(), "API token not found or expired");
            return Err(actix_web::error::ErrorUnauthorized("Invalid or expired API token"));
        }
        Err(e) => {
            error!("Error looking up API token: {}", e);
            return Err(actix_web::error::ErrorInternalServerError("Authentication error"));
        }
    };

    // Get user information
    let user = match crate::repository::get_user_by_uuid(&api_token.user_uuid, &mut conn) {
        Ok(u) => u,
        Err(e) => {
            error!("Failed to get user for API token: {}", e);
            return Err(actix_web::error::ErrorInternalServerError("Authentication error"));
        }
    };

    // Get user's primary email
    let email = crate::repository::user_emails::get_user_emails_by_uuid(&mut conn, &api_token.user_uuid)
        .ok()
        .and_then(|emails| emails.into_iter().find(|e| e.is_primary).map(|e| e.email))
        .unwrap_or_else(|| "unknown@example.com".to_string());

    // Update last_used_at
    let client_ip = extract_client_ip(req);
    let ip_network = client_ip.map(|ip| {
        use ipnetwork::IpNetwork;
        match ip {
            IpAddr::V4(v4) => IpNetwork::V4(ipnetwork::Ipv4Network::from(v4)),
            IpAddr::V6(v6) => IpNetwork::V6(ipnetwork::Ipv6Network::from(v6)),
        }
    });

    if let Err(e) = update_token_last_used(&mut conn, api_token.id, ip_network) {
        warn!("Failed to update token last_used_at: {}", e);
    }

    // Create claims from API token
    let now = chrono::Utc::now();
    let claims = Claims {
        sub: api_token.user_uuid.to_string(),
        name: user.name,
        email,
        role: format!("{:?}", user.role).to_lowercase(),
        scope: "full".to_string(), // API tokens always have full scope
        exp: (now + chrono::Duration::hours(24)).timestamp() as usize,
        iat: now.timestamp() as usize,
    };

    info!(
        user = %claims.sub,
        token_uuid = %api_token.uuid,
        "API token authentication successful"
    );

    Ok(Some(claims))
}

/// Middleware function that supports both Bearer token and cookie authentication
/// This should replace cookie_auth_middleware in routes that need to support API tokens
pub async fn dual_auth_middleware(
    req: actix_web::dev::ServiceRequest,
    next: actix_web::middleware::Next<impl actix_web::body::MessageBody>,
) -> Result<actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>, Error> {
    let pool = req
        .app_data::<web::Data<Pool>>()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("Database pool not found"))?
        .clone();

    // Try Bearer token authentication first
    match try_bearer_auth(&req, &pool)? {
        Some(claims) => {
            // Mark this request as authenticated via API token
            req.extensions_mut().insert(ApiTokenAuth {
                token_uuid: uuid::Uuid::parse_str(&claims.sub).unwrap_or_default(),
            });
            // Insert claims for handler use
            req.extensions_mut().insert(claims);
            // Continue without cookie auth
            return next.call(req).await;
        }
        None => {
            // No Bearer token, fall through to cookie auth
        }
    }

    // Fall back to cookie-based authentication
    use crate::utils::jwt::JwtUtils;

    let mut conn = pool
        .get()
        .map_err(|_| actix_web::error::ErrorInternalServerError("Database connection failed"))?;

    // Extract access token from httpOnly cookie
    let token = req
        .cookie(crate::utils::cookies::ACCESS_TOKEN_COOKIE)
        .ok_or_else(|| {
            warn!(path = %req.path(), "No access_token cookie and no Bearer token");
            actix_web::error::ErrorUnauthorized("Authentication required")
        })?;

    // Validate token and get claims
    let (claims, _user) = JwtUtils::authenticate_with_token(token.value(), &mut conn)
        .await
        .map_err(|err| {
            error!(error = ?err, "Cookie auth: token validation failed");
            actix_web::error::ErrorUnauthorized("Invalid or expired token")
        })?;

    info!(user = %claims.sub, "Cookie auth: user authenticated successfully");

    // Insert claims into request extensions
    req.extensions_mut().insert(claims);

    // Continue to the handler
    next.call(req).await
}
