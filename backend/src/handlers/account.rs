//! `/account`: a person's own account, from the agent app (`/api/account`) or
//! the portal (`/api/portal/account`).
//!
//! One route table, mounted under each app's session middleware: the path says
//! which session acts, and CSRF checks that app's cookie. Every route acts on
//! the caller. Admins change other people through `/users/{uuid}`. A portal
//! session opened from one ticket's link reaches none of it
//! ([`full_portal_session`]).

use actix_multipart::Multipart;
use actix_web::body::MessageBody;
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::middleware::Next;
use actix_web::{web, Error, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::db::Pool;
use crate::errors::ApiError;
use crate::extractors::TenantConn;
use crate::handlers::helpers;
use crate::handlers::portal::{PortalContext, PortalScope};
use crate::models::UserUpdateWithPassword;
use crate::services::search::SearchService;
use crate::utils::rbac;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("", web::get().to(get_account))
        .route("", web::patch().to(update_account))
        .route("/image", web::post().to(upload_image))
        .route(
            "/password",
            web::post().to(crate::handlers::change_password),
        )
        .service(
            web::scope("/mfa")
                .route("/setup", web::post().to(crate::handlers::mfa_setup))
                .route(
                    "/verify-setup",
                    web::post().to(crate::handlers::mfa_verify_setup),
                )
                .route("/enable", web::post().to(crate::handlers::mfa_enable))
                .route("/disable", web::post().to(crate::handlers::mfa_disable))
                .route(
                    "/regenerate-backup-codes",
                    web::post().to(crate::handlers::mfa_regenerate_backup_codes),
                )
                .route("/status", web::get().to(crate::handlers::mfa_status)),
        )
        .service(
            web::scope("/passkeys")
                .route(
                    "/register/start",
                    web::post().to(crate::handlers::start_passkey_registration),
                )
                .route(
                    "/register/finish",
                    web::post().to(crate::handlers::finish_passkey_registration),
                )
                .route("", web::get().to(crate::handlers::list_passkeys))
                .route(
                    "/{credential_id}",
                    web::patch().to(crate::handlers::rename_passkey),
                )
                .route(
                    "/{credential_id}",
                    web::delete().to(crate::handlers::delete_passkey),
                ),
        )
        .service(
            web::scope("/sessions")
                .route("", web::get().to(crate::handlers::get_user_sessions))
                .route(
                    "/others",
                    web::delete().to(crate::handlers::revoke_all_other_sessions),
                )
                // After "/others", which this would otherwise absorb.
                .route(
                    "/{session_id}",
                    web::delete().to(crate::handlers::revoke_session),
                ),
        )
        .service(
            web::scope("/step-up")
                .route("", web::get().to(crate::handlers::step_up::status))
                .route("", web::post().to(crate::handlers::step_up::confirm))
                .route(
                    "/passkey/start",
                    web::post().to(crate::handlers::step_up::start_passkey),
                )
                .route(
                    "/passkey/finish",
                    web::post().to(crate::handlers::step_up::finish_passkey),
                ),
        )
        .route(
            "/notifications",
            web::get().to(crate::handlers::notifications::get_preferences),
        )
        .route(
            "/notifications",
            web::put().to(crate::handlers::notifications::update_preference),
        )
        .route(
            "/notifications/interrupt",
            web::get().to(crate::handlers::notifications::get_interrupt_preferences),
        )
        .route(
            "/notifications/interrupt",
            web::put().to(crate::handlers::notifications::set_interrupt_preferences),
        );
}

/// Refuses a portal session limited to one ticket: a link's session views
/// that request and changes nothing about the account.
pub async fn full_portal_session(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let scope = req.extensions().get::<PortalContext>().map(|p| p.scope);
    match scope {
        Some(PortalScope::Full) => next.call(req).await,
        Some(PortalScope::Ticket(_)) => Err(crate::errors::forbidden_error(
            "Sign in to manage your account",
        )),
        None => Err(crate::errors::unauthorized_error("Authentication required")),
    }
}

fn caller_uuid(req: &HttpRequest) -> Result<uuid::Uuid, ApiError> {
    let claims = rbac::require_auth(req)?;
    crate::utils::parse_uuid(&claims.sub)
        .map_err(|_| ApiError::BadRequest("Invalid user UUID in token".into()))
}

/// `GET /account`: the caller's profile and which parts of it they manage
/// here. `sections` is the server's answer, so neither app decides it.
pub async fn get_account(
    pool: web::Data<Pool>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    let claims = rbac::require_auth(&req)?;
    let mut conn = helpers::db_conn(&pool)?;
    let profile = crate::handlers::auth::current_user_response(&req, &claims, &mut conn)?;
    let uuid = profile.uuid;
    let local = crate::middleware::workspace_context::local_credentials_permitted();
    let has_password =
        crate::repository::user_auth_identities::get_local_password_hash(&uuid, &mut conn).is_ok();
    let name =
        !crate::repository::workspaces::nosdesk_account_managed(&mut conn, &[uuid]).contains(&uuid);
    let mut body = serde_json::to_value(&profile)
        .map_err(|_| ApiError::Internal("Failed to load profile".into()))?;
    body["sections"] = json!({
        "name": name,
        "password": local && has_password,
        "authenticator": local && has_password,
        "passkeys": local,
    });
    Ok(HttpResponse::Ok().json(body))
}

/// What a person changes about themselves here.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountUpdate {
    pub name: Option<String>,
    pub pronouns: Option<String>,
    pub theme: Option<String>,
    /// Empty string clears it back to the workspace default.
    pub locale: Option<String>,
    /// Empty string clears it back to the workspace default.
    pub timezone: Option<String>,
}

/// `PATCH /account`: the caller's own profile, through the same update as
/// `PUT /users/{uuid}` for themselves.
pub async fn update_account(
    tc: TenantConn,
    search_service: web::Data<Arc<SearchService>>,
    pool: web::Data<Pool>,
    req: HttpRequest,
    body: web::Json<AccountUpdate>,
) -> Result<impl Responder, ApiError> {
    let uuid = caller_uuid(&req)?;
    let body = body.into_inner();
    let update = UserUpdateWithPassword {
        name: body.name,
        role: None,
        pronouns: body.pronouns,
        avatar_url: None,
        banner_url: None,
        avatar_thumb: None,
        theme: body.theme,
        password: None,
        signature: None,
        dashboard_layout: None,
        locale: body.locale,
        timezone: body.timezone,
    };
    Ok(crate::handlers::users::update_user_by_uuid(
        tc,
        search_service,
        pool,
        req,
        web::Path::from(uuid.to_string()),
        web::Json(update),
    )
    .await)
}

/// `POST /account/image?type_=avatar|banner`: the caller's own photo or banner.
pub async fn upload_image(
    payload: Multipart,
    tc: TenantConn,
    search_service: web::Data<Arc<SearchService>>,
    type_query: web::Query<crate::handlers::users::UserImageTypeQuery>,
    pool: web::Data<Pool>,
    req: HttpRequest,
) -> Result<impl Responder, ApiError> {
    let uuid = caller_uuid(&req)?;
    Ok(crate::handlers::users::upload_user_image(
        web::Path::from(uuid.to_string()),
        payload,
        tc,
        search_service,
        type_query,
        pool,
        req,
    )
    .await)
}
