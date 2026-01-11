//! API Token Handlers
//!
//! Admin endpoints for managing API tokens for programmatic access.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{Claims, CreateApiTokenRequest};
use crate::repository::api_tokens;
use crate::utils::rbac::require_admin;

/// List all API tokens (admin only)
pub async fn list_api_tokens(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Database connection error: {}", e);
            return HttpResponse::InternalServerError().json("Database connection error");
        }
    };

    match api_tokens::list_all_api_tokens(&mut conn) {
        Ok(tokens) => match api_tokens::enrich_tokens_with_users(&mut conn, tokens) {
            Ok(enriched) => HttpResponse::Ok().json(enriched),
            Err(e) => {
                error!("Failed to enrich tokens: {}", e);
                HttpResponse::InternalServerError().json("Failed to get tokens")
            }
        },
        Err(e) => {
            error!("Failed to list tokens: {}", e);
            HttpResponse::InternalServerError().json("Failed to list tokens")
        }
    }
}

/// Create a new API token (admin only)
pub async fn create_api_token(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<CreateApiTokenRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let created_by = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    // Validate token name
    if body.name.trim().is_empty() {
        return HttpResponse::BadRequest().json("Token name is required");
    }

    if body.name.len() > 255 {
        return HttpResponse::BadRequest().json("Token name must be 255 characters or less");
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Database connection error: {}", e);
            return HttpResponse::InternalServerError().json("Database connection error");
        }
    };

    // Verify the target user exists
    match crate::repository::get_user_by_uuid(&body.user_uuid, &mut conn) {
        Ok(_) => {}
        Err(Error::NotFound) => {
            return HttpResponse::NotFound().json("Target user not found");
        }
        Err(e) => {
            error!("Failed to verify user: {}", e);
            return HttpResponse::InternalServerError().json("Failed to verify user");
        }
    }

    match api_tokens::create_api_token(
        &mut conn,
        body.user_uuid,
        body.name.trim().to_string(),
        created_by,
        body.expires_in_days,
        body.scopes.clone(),
    ) {
        Ok(created) => {
            info!(
                "API token created: {} for user {} by admin {}",
                created.uuid, body.user_uuid, created_by
            );
            HttpResponse::Created().json(created)
        }
        Err(e) => {
            error!("Failed to create token: {}", e);
            HttpResponse::InternalServerError().json("Failed to create token")
        }
    }
}

/// Get a single API token by UUID (admin only)
pub async fn get_api_token(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let token_uuid = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Database connection error: {}", e);
            return HttpResponse::InternalServerError().json("Database connection error");
        }
    };

    match api_tokens::get_api_token_by_uuid(&mut conn, token_uuid) {
        Ok(token) => match api_tokens::enrich_tokens_with_users(&mut conn, vec![token]) {
            Ok(mut enriched) => {
                if let Some(token_info) = enriched.pop() {
                    HttpResponse::Ok().json(token_info)
                } else {
                    HttpResponse::NotFound().json("Token not found")
                }
            }
            Err(e) => {
                error!("Failed to enrich token: {}", e);
                HttpResponse::InternalServerError().json("Failed to get token")
            }
        },
        Err(Error::NotFound) => HttpResponse::NotFound().json("Token not found"),
        Err(e) => {
            error!("Failed to get token: {}", e);
            HttpResponse::InternalServerError().json("Failed to get token")
        }
    }
}

/// Revoke an API token (admin only)
pub async fn revoke_api_token(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let admin_uuid = Uuid::parse_str(&claims.sub).ok();
    let token_uuid = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!("Database connection error: {}", e);
            return HttpResponse::InternalServerError().json("Database connection error");
        }
    };

    // Verify token exists before revoking
    match api_tokens::get_api_token_by_uuid(&mut conn, token_uuid) {
        Ok(token) => {
            if token.revoked_at.is_some() {
                return HttpResponse::BadRequest().json("Token is already revoked");
            }
        }
        Err(Error::NotFound) => {
            return HttpResponse::NotFound().json("Token not found");
        }
        Err(e) => {
            error!("Failed to get token: {}", e);
            return HttpResponse::InternalServerError().json("Failed to get token");
        }
    }

    match api_tokens::revoke_api_token(&mut conn, token_uuid) {
        Ok(count) if count > 0 => {
            info!(
                "API token {} revoked by admin {:?}",
                token_uuid, admin_uuid
            );
            HttpResponse::NoContent().finish()
        }
        Ok(_) => HttpResponse::NotFound().json("Token not found"),
        Err(e) => {
            error!("Failed to revoke token: {}", e);
            HttpResponse::InternalServerError().json("Failed to revoke token")
        }
    }
}
