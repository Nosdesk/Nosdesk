//! API Token Handlers
//!
//! Admin endpoints for managing API tokens for programmatic access.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use tracing::{error, info};
use uuid::Uuid;

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::{Claims, CreateApiTokenRequest, WorkspaceRole};
use crate::repository::api_tokens;
use crate::utils::rbac::require_workspace_role;

/// List all API tokens (admin only)
pub async fn list_api_tokens(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let result = tc.run(|conn| {
        let tokens = api_tokens::list_all_api_tokens(conn)?;
        api_tokens::enrich_tokens_with_users(conn, tokens)
    });

    match result {
        Ok(enriched) => HttpResponse::Ok().json(enriched),
        Err(e) => {
            error!("Failed to list tokens: {}", e);
            errors::internal("Failed to list tokens")
        }
    }
}

/// Create a new API token (admin only)
pub async fn create_api_token(
    req: HttpRequest,
    mut tc: TenantConn,
    body: web::Json<CreateApiTokenRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
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

    // Reject unknown scopes at mint time so a typo can't silently
    // create a token that no endpoint will ever honour.
    if let Some(scopes) = body.scopes.as_ref() {
        if let Some(bad) = scopes
            .iter()
            .find(|s| !crate::utils::rbac::is_valid_token_scope(s))
        {
            return errors::bad_request(format!("Unknown token scope: {bad}"));
        }
    }

    // Outcome: verify-user step can branch on NotFound; collapse to a
    // single tc.run so both queries share one tenant-scoped tx.
    enum Outcome {
        Created(crate::models::ApiTokenCreatedResponse),
        TargetUserNotFound,
    }

    let user_uuid = body.user_uuid;
    let name = body.name.trim().to_string();
    let expires_in_days = body.expires_in_days;
    let scopes = body.scopes.clone();

    let result = tc.run(|conn| {
        // Active-only — don't let an admin (or a self-issuance
        // path racing a delete) mint a token for a soft-deleted
        // user. F2C.2 H4.
        match crate::repository::users::find_active_by_uuid(&user_uuid, conn) {
            Ok(_) => {}
            Err(Error::NotFound) => return Ok(Outcome::TargetUserNotFound),
            Err(e) => return Err(e),
        }
        let created = api_tokens::create_api_token(
            conn,
            user_uuid,
            name,
            created_by,
            expires_in_days,
            scopes,
        )?;
        Ok(Outcome::Created(created))
    });

    match result {
        Ok(Outcome::Created(created)) => {
            info!(
                "API token created: {} for user {} by admin {}",
                created.uuid, body.user_uuid, created_by
            );
            HttpResponse::Created().json(created)
        }
        Ok(Outcome::TargetUserNotFound) => errors::not_found_msg("Target user not found"),
        Err(e) => {
            error!("Failed to create token: {}", e);
            errors::internal("Failed to create token")
        }
    }
}

/// Get a single API token by UUID (admin only)
pub async fn get_api_token(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let token_uuid = path.into_inner();

    enum Outcome {
        Found(crate::models::ApiTokenInfo),
        NotFound,
    }

    let result = tc.run(|conn| {
        let token = match api_tokens::get_api_token_by_uuid(conn, token_uuid) {
            Ok(t) => t,
            Err(Error::NotFound) => return Ok(Outcome::NotFound),
            Err(e) => return Err(e),
        };
        let mut enriched = api_tokens::enrich_tokens_with_users(conn, vec![token])?;
        match enriched.pop() {
            Some(info) => Ok(Outcome::Found(info)),
            None => Ok(Outcome::NotFound),
        }
    });

    match result {
        Ok(Outcome::Found(info)) => HttpResponse::Ok().json(info),
        Ok(Outcome::NotFound) => errors::not_found_msg("Token not found"),
        Err(e) => {
            error!("Failed to get token: {}", e);
            errors::internal("Failed to get token")
        }
    }
}

/// Revoke an API token (admin only)
pub async fn revoke_api_token(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    let admin_uuid = Uuid::parse_str(&claims.sub).ok();
    let token_uuid = path.into_inner();

    enum Outcome {
        Revoked,
        AlreadyRevoked,
        NotFound,
    }

    let result = tc.run(|conn| {
        match api_tokens::get_api_token_by_uuid(conn, token_uuid) {
            Ok(token) => {
                if token.revoked_at.is_some() {
                    return Ok(Outcome::AlreadyRevoked);
                }
            }
            Err(Error::NotFound) => return Ok(Outcome::NotFound),
            Err(e) => return Err(e),
        }

        let count = api_tokens::revoke_api_token(conn, token_uuid)?;
        if count > 0 {
            Ok(Outcome::Revoked)
        } else {
            Ok(Outcome::NotFound)
        }
    });

    match result {
        Ok(Outcome::Revoked) => {
            info!("API token {} revoked by admin {:?}", token_uuid, admin_uuid);
            HttpResponse::NoContent().finish()
        }
        Ok(Outcome::AlreadyRevoked) => errors::bad_request("Token is already revoked"),
        Ok(Outcome::NotFound) => errors::not_found_msg("Token not found"),
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

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
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
