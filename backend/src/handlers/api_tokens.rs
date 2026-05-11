//! API Token Handlers
//!
//! Admin endpoints for managing API tokens for programmatic access.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::helpers;
use crate::handlers::errors;
use crate::models::{Claims, CreateApiTokenRequest};
use crate::repository::api_tokens;
use crate::utils::rbac::require_admin;

/// List all API tokens (admin only)
pub async fn list_api_tokens(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match api_tokens::list_all_api_tokens(&mut conn) {
        Ok(tokens) => match api_tokens::enrich_tokens_with_users(&mut conn, tokens) {
            Ok(enriched) => HttpResponse::Ok().json(enriched),
            Err(e) => {
                error!("Failed to enrich tokens: {}", e);
                errors::internal("Failed to get tokens")
            }
        },
        Err(e) => {
            error!("Failed to list tokens: {}", e);
            errors::internal("Failed to list tokens")
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
        None => return errors::unauthorized("Authentication required"),
    };

    let created_by = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    // Validate token name
    if body.name.trim().is_empty() {
        return errors::bad_request("Token name is required");
    }

    if body.name.len() > 255 {
        return errors::bad_request("Token name must be 255 characters or less");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Verify the target user exists
    match crate::repository::get_user_by_uuid(&body.user_uuid, &mut conn) {
        Ok(_) => {}
        Err(Error::NotFound) => {
            return errors::not_found_msg("Target user not found");
        }
        Err(e) => {
            error!("Failed to verify user: {}", e);
            return errors::internal("Failed to verify user");
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
            errors::internal("Failed to create token")
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

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match api_tokens::get_api_token_by_uuid(&mut conn, token_uuid) {
        Ok(token) => match api_tokens::enrich_tokens_with_users(&mut conn, vec![token]) {
            Ok(mut enriched) => {
                if let Some(token_info) = enriched.pop() {
                    HttpResponse::Ok().json(token_info)
                } else {
                    errors::not_found_msg("Token not found")
                }
            }
            Err(e) => {
                error!("Failed to enrich token: {}", e);
                errors::internal("Failed to get token")
            }
        },
        Err(Error::NotFound) => errors::not_found_msg("Token not found"),
        Err(e) => {
            error!("Failed to get token: {}", e);
            errors::internal("Failed to get token")
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
        None => return errors::unauthorized("Authentication required"),
    };

    let admin_uuid = Uuid::parse_str(&claims.sub).ok();
    let token_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Verify token exists before revoking
    match api_tokens::get_api_token_by_uuid(&mut conn, token_uuid) {
        Ok(token) => {
            if token.revoked_at.is_some() {
                return errors::bad_request("Token is already revoked");
            }
        }
        Err(Error::NotFound) => {
            return errors::not_found_msg("Token not found");
        }
        Err(e) => {
            error!("Failed to get token: {}", e);
            return errors::internal("Failed to get token");
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
        Ok(_) => errors::not_found_msg("Token not found"),
        Err(e) => {
            error!("Failed to revoke token: {}", e);
            errors::internal("Failed to revoke token")
        }
    }
}

#[cfg(test)]
mod tests {
    //! Permission-boundary tests for the API-token surface. Token
    //! management has admin-equivalent power (an issued token grants
    //! the requester's full role for 24h), so the gate must be wired.
    //! The rbac module covers gate behaviour exhaustively; here we
    //! prove the wiring by asserting unauthenticated and user-role
    //! requests are turned away on the list endpoint.
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App};

    fn test_app(pool: crate::db::Pool) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/admin/api-tokens", web::get().to(list_api_tokens))
    }

    #[actix_web::test]
    async fn list_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/api-tokens")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn list_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/api-tokens")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
