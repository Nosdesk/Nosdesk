//! `/api/auth/step-up`: the endpoints behind the step-up dialog (see
//! `services::step_up`). All sit behind cookie auth and confirm the caller's
//! own session. The provider route lives with the OAuth flow it shares, in
//! `auth_providers` (`start_step_up_provider`).

use actix_web::{web, HttpRequest, HttpResponse};
use serde_json::json;
use tracing::{error, warn};
use uuid::Uuid;
use webauthn_rs::prelude::{Passkey, PublicKeyCredential};

use crate::db::{DbConnection, Pool};
use crate::errors::ApiError;
use crate::handlers::helpers;
use crate::models::{Claims, StepUpCredential, User};
use crate::services::step_up::{self, Method};
use crate::utils::{parse_uuid, rbac, webauthn};

/// The caller, their session, and a connection.
fn caller(
    pool: &web::Data<Pool>,
    req: &HttpRequest,
) -> Result<(Claims, User, Uuid, DbConnection), ApiError> {
    let claims = rbac::require_auth(req)?;
    if !claims.is_account_session() {
        return Err(ApiError::Forbidden(
            "This action requires a full session".into(),
        ));
    }
    let user_uuid = parse_uuid(&claims.sub)
        .map_err(|_| ApiError::BadRequest("Invalid user UUID in token".into()))?;
    let sid = claims
        .session_uuid()
        .ok_or_else(|| ApiError::BadRequest("Sign in again to confirm it's you".into()))?;
    let mut conn = helpers::db_conn(pool)?;
    let user = crate::repository::get_user_by_uuid(&user_uuid, &mut conn)
        .map_err(|_| ApiError::NotFoundMsg("User not found".into()))?;
    Ok((claims, user, sid, conn))
}

/// A 429 once a person has used up their attempts.
async fn too_many_attempts(user_uuid: &Uuid) -> Option<HttpResponse> {
    step_up::attempts_exhausted(user_uuid)
        .await
        .then(step_up::too_many_attempts)
}

/// Record a confirmation by `method` on the caller's session and answer with
/// until when it lasts.
fn confirmed(
    conn: &mut DbConnection,
    req: &HttpRequest,
    user: &User,
    sid: &Uuid,
    method: Method,
) -> Result<HttpResponse, ApiError> {
    let until = step_up::record_confirmed(conn, req, &user.uuid, sid, method)
        .ok_or_else(|| ApiError::BadRequest("Sign in again to confirm it's you".into()))?;
    Ok(HttpResponse::Ok().json(json!({ "confirmed_until": until })))
}

/// `GET /api/auth/step-up`: whether this session is confirmed, until when,
/// and how the caller can confirm (best first).
pub async fn status(pool: web::Data<Pool>, req: HttpRequest) -> Result<HttpResponse, ApiError> {
    let (claims, user, sid, mut conn) = caller(&pool, &req)?;
    let (mut methods, mut provider) = step_up::methods(&mut conn, &user);
    // A provider confirmation reads the agent app's cookies, so a portal
    // session confirms another way or signs in again.
    if claims.is_portal_session() {
        methods.retain(|m| !matches!(m, Method::Provider));
        provider = None;
    }
    let confirmed_until = step_up::session_confirmed_until(&mut conn, &sid, &user.uuid);
    let email = crate::repository::user_helpers::get_primary_email(&user.uuid, &mut conn);
    Ok(HttpResponse::Ok().json(json!({
        "confirmed_until": confirmed_until,
        "window_minutes": step_up::WINDOW_MINUTES,
        "methods": methods,
        "provider": provider,
        "email": email,
    })))
}

/// `POST /api/auth/step-up`: confirm with a password (local or LDAP) or an
/// authenticator / backup code.
pub async fn confirm(
    pool: web::Data<Pool>,
    req: HttpRequest,
    body: web::Json<StepUpCredential>,
) -> Result<HttpResponse, ApiError> {
    let (_claims, user, sid, mut conn) = caller(&pool, &req)?;
    if let Some(refused) = too_many_attempts(&user.uuid).await {
        return Ok(refused);
    }
    match step_up::check_credential(&mut conn, &user, &body).await {
        Some(method) => confirmed(&mut conn, &req, &user, &sid, method),
        None => Err(ApiError::BadRequest("That isn't right. Try again.".into())),
    }
}

/// `POST /api/auth/step-up/passkey/start`: a passkey challenge limited to the
/// caller's own passkeys, with user verification required.
pub async fn start_passkey(
    pool: web::Data<Pool>,
    req: HttpRequest,
) -> Result<HttpResponse, ApiError> {
    if crate::handlers::auth::hosted_local_auth_disabled() {
        return Err(ApiError::Forbidden(
            "Passkeys are not available here".into(),
        ));
    }
    let (_claims, user, sid, mut conn) = caller(&pool, &req)?;
    let passkeys: Vec<Passkey> = webauthn::load_user_passkey_data(&mut conn, &user.uuid)
        .map_err(|e| {
            error!(error = ?e, "step-up: loading passkeys failed");
            ApiError::Internal("Failed to load passkeys".into())
        })?
        .credentials
        .into_iter()
        .map(|c| c.credential)
        .collect();
    if passkeys.is_empty() {
        return Err(ApiError::BadRequest("This account has no passkey".into()));
    }
    let webauthn = webauthn::webauthn_for_request(&req).map_err(|e| {
        error!(error = ?e, "step-up: building the WebAuthn verifier failed");
        ApiError::Internal("WebAuthn is not configured for this workspace".into())
    })?;
    let (challenge, state) = webauthn
        .start_passkey_authentication(&passkeys)
        .map_err(|e| {
            error!(error = ?e, "step-up: starting the passkey challenge failed");
            ApiError::Internal("Failed to generate authentication challenge".into())
        })?;
    webauthn::store_step_up_auth_state(&sid, &state)
        .await
        .map_err(|e| {
            error!(error = ?e, "step-up: storing the passkey challenge failed");
            ApiError::Internal("Failed to store authentication state".into())
        })?;
    let challenge = serde_json::to_value(&challenge).unwrap_or_else(|_| json!({}));
    Ok(HttpResponse::Ok().json(challenge.get("publicKey").cloned().unwrap_or(challenge)))
}

/// `POST /api/auth/step-up/passkey/finish`: confirm with the passkey
/// assertion for the challenge from `start_passkey`.
pub async fn finish_passkey(
    pool: web::Data<Pool>,
    req: HttpRequest,
    body: web::Json<crate::handlers::passkeys::FinishLoginRequest>,
) -> Result<HttpResponse, ApiError> {
    if crate::handlers::auth::hosted_local_auth_disabled() {
        return Err(ApiError::Forbidden(
            "Passkeys are not available here".into(),
        ));
    }
    let (_claims, user, sid, mut conn) = caller(&pool, &req)?;
    if let Some(refused) = too_many_attempts(&user.uuid).await {
        return Ok(refused);
    }
    let state = webauthn::take_step_up_auth_state(&sid).await.map_err(|e| {
        warn!(error = ?e, "step-up: passkey challenge not found");
        ApiError::BadRequest("The passkey request expired. Try again.".into())
    })?;
    let assertion: PublicKeyCredential = serde_json::from_value(json!({
        "id": body.id,
        "rawId": body.raw_id,
        "response": body.response,
        "type": body.credential_type,
        "clientExtensionResults": body.client_extension_results.clone().unwrap_or(json!({})),
        "authenticatorAttachment": body.authenticator_attachment.clone(),
    }))
    .map_err(|_| ApiError::BadRequest("Invalid authentication response".into()))?;
    let webauthn = webauthn::webauthn_for_request(&req).map_err(|e| {
        error!(error = ?e, "step-up: building the WebAuthn verifier failed");
        ApiError::Internal("WebAuthn is not configured for this workspace".into())
    })?;
    let result = webauthn
        .finish_passkey_authentication(&assertion, &state)
        .map_err(|e| {
            warn!(error = ?e, "step-up: passkey assertion refused");
            ApiError::BadRequest("That passkey didn't confirm it's you. Try again.".into())
        })?;
    // The challenge only allowed this person's passkeys; check the owner anyway.
    let credential_id = webauthn::credential_id_to_string(result.cred_id());
    let owner =
        crate::repository::passkey_credentials::find_by_credential_id(&mut conn, &credential_id)
            .ok()
            .flatten()
            .map(|row| row.user_uuid);
    if owner != Some(user.uuid) {
        warn!(user_uuid = %user.uuid, "step-up: passkey belongs to another account");
        return Err(ApiError::BadRequest(
            "That passkey didn't confirm it's you. Try again.".into(),
        ));
    }
    if let Err(e) = webauthn::update_credential_post_auth(&mut conn, &user.uuid, &result) {
        warn!(error = ?e, "step-up: saving the passkey counter failed");
    }
    confirmed(&mut conn, &req, &user, &sid, Method::Passkey)
}
