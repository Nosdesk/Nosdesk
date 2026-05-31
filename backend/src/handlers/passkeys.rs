//! Passkey/WebAuthn Handlers
//!
//! Endpoints for passkey registration, authentication, and management.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{debug, error, info, warn};
use uuid::Uuid;
use webauthn_rs::prelude::*;

use crate::db::Pool;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::models::Claims;
use crate::repository;
use crate::utils::i18n;
use crate::utils::jwt::helpers as jwt_helpers;
use crate::utils::locale::request_locale;
use crate::utils::mfa;
use crate::utils::rate_limit::{get_redis_url, RateLimiter};
use crate::utils::webauthn::{self, credential_id_to_string, StoredPasskeyCredential, WEBAUTHN};

// =============================================================================
// Request/Response Types
// =============================================================================

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct StartRegistrationRequest {
    pub passkey_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FinishRegistrationRequest {
    pub id: String,
    #[serde(rename = "rawId")]
    pub raw_id: String,
    pub response: serde_json::Value,
    #[serde(rename = "type")]
    pub credential_type: String,
    pub passkey_name: Option<String>,
    #[serde(rename = "clientExtensionResults")]
    pub client_extension_results: Option<serde_json::Value>,
    pub authenticator_attachment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StartLoginRequest {
    pub email: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FinishLoginRequest {
    pub id: String,
    #[serde(rename = "rawId")]
    pub raw_id: String,
    pub response: serde_json::Value,
    #[serde(rename = "type")]
    pub credential_type: String,
    #[serde(rename = "clientExtensionResults")]
    pub client_extension_results: Option<serde_json::Value>,
    pub authenticator_attachment: Option<String>,
    /// Session ID for discoverable (usernameless) authentication
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RenamePasskeyRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct DeletePasskeyRequest {
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct PasskeyInfo {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub transports: Vec<String>,
    pub backup_eligible: bool,
}

#[derive(Debug, Serialize)]
pub struct PasskeyListResponse {
    pub passkeys: Vec<PasskeyInfo>,
}

// =============================================================================
// Registration Handlers
// =============================================================================

/// Start passkey registration - generates challenge and options
pub async fn start_passkey_registration(
    req: HttpRequest,
    pool: web::Data<Pool>,
    _body: web::Json<StartRegistrationRequest>,
) -> impl Responder {
    // Get authenticated user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request("Invalid user UUID");
        }
    };

    // Rate limiting: 5 passkey registrations per hour per user
    let redis_url = get_redis_url();
    let rate_key = format!("passkey_registration:{user_uuid}");
    match RateLimiter::check_rate_limit(&redis_url, &rate_key, 5, 3600).await {
        Ok(false) => {
            return errors::too_many_requests(
                "Too many passkey registration attempts. Please try again later.",
                3600,
            );
        }
        Err(e) => {
            warn!("Rate limit check failed for passkey registration: {:?}", e);
            // Continue anyway - fail open for availability
        }
        _ => {}
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get user from database
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return errors::not_found_msg("User not found");
        }
    };

    // Check passkey limit
    match webauthn::can_add_passkey(&mut conn, &user_uuid) {
        Ok(false) => {
            return HttpResponse::BadRequest().json(json!({
                "error": i18n::tr(&request_locale(&req), "backend-error-passkey-max-reached"),
                "code": "backend-error-passkey-max-reached",
                "max_passkeys": webauthn::MAX_PASSKEYS_PER_USER
            }));
        }
        Err(e) => {
            error!("Failed to check passkey count: {:?}", e);
            return errors::internal("Failed to check passkey count");
        }
        Ok(true) => {}
    }

    // Get user's primary email
    let primary_email = match repository::user_helpers::get_primary_email(&user_uuid, &mut conn) {
        Some(email) => email,
        None => {
            return errors::internal("Could not retrieve user email");
        }
    };

    // Existing credentials to exclude from the new registration
    let passkey_data = match webauthn::load_user_passkey_data(&mut conn, &user_uuid) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to load existing passkeys: {:?}", e);
            return errors::internal("Failed to load existing passkeys");
        }
    };
    let exclude_credentials: Vec<CredentialID> = passkey_data
        .credentials
        .iter()
        .map(|c| c.credential.cred_id().clone())
        .collect();

    // Create WebAuthn registration challenge
    let webauthn = &*WEBAUTHN;

    let (ccr, reg_state) = match webauthn.start_passkey_registration(
        user_uuid,
        &primary_email,
        &user.name,
        Some(exclude_credentials),
    ) {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to start passkey registration: {:?}", e);
            return errors::internal("Failed to generate registration challenge");
        }
    };

    // Store registration state in Redis
    if let Err(e) = webauthn::store_registration_state(&user_uuid, &reg_state).await {
        error!("Failed to store registration state: {:?}", e);
        return errors::internal("Failed to store registration state");
    }

    debug!("Started passkey registration for user {}", user_uuid);

    // Return the challenge options to the client
    // Note: ccr contains { public_key: ... } wrapper, but @simplewebauthn/browser expects
    // the inner PublicKeyCredentialCreationOptions directly, so we serialize the whole
    // thing and extract the publicKey field
    let ccr_json = serde_json::to_value(&ccr).unwrap_or(json!({}));

    // webauthn-rs serializes as "publicKey" (camelCase)
    if let Some(public_key) = ccr_json.get("publicKey") {
        // Modify authenticatorSelection to require resident keys (discoverable credentials)
        // This is necessary for usernameless/discoverable authentication to work
        let mut options = public_key.clone();
        if let Some(obj) = options.as_object_mut() {
            // Get or create authenticatorSelection
            let auth_selection = obj
                .entry("authenticatorSelection")
                .or_insert_with(|| json!({}));

            if let Some(auth_obj) = auth_selection.as_object_mut() {
                // Set residentKey to "required" for discoverable credentials
                auth_obj.insert("residentKey".to_string(), json!("required"));
                // Also set requireResidentKey for older browsers
                auth_obj.insert("requireResidentKey".to_string(), json!(true));
            }
        }
        HttpResponse::Ok().json(options)
    } else {
        // Fallback: return the whole response (shouldn't happen)
        HttpResponse::Ok().json(ccr_json)
    }
}

/// Complete passkey registration
pub async fn finish_passkey_registration(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<FinishRegistrationRequest>,
) -> impl Responder {
    // Get authenticated user
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request("Invalid user UUID");
        }
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    if repository::get_user_by_uuid(&user_uuid, &mut conn).is_err() {
        return errors::not_found_msg("User not found");
    }

    // Retrieve registration state from Redis
    let reg_state = match webauthn::get_registration_state(&user_uuid).await {
        Ok(state) => state,
        Err(e) => {
            warn!(
                "Registration state not found for user {}: {:?}",
                user_uuid, e
            );
            return errors::bad_request("Registration challenge expired or not found");
        }
    };

    // Parse the registration response
    let reg_response: RegisterPublicKeyCredential = match serde_json::from_value(json!({
        "id": body.id,
        "rawId": body.raw_id,
        "response": body.response,
        "type": body.credential_type,
        "clientExtensionResults": body.client_extension_results.clone().unwrap_or(json!({})),
        "authenticatorAttachment": body.authenticator_attachment.clone()
    })) {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to parse registration response: {:?}", e);
            return errors::bad_request("Invalid registration response");
        }
    };

    // Complete registration with WebAuthn
    let webauthn = &*WEBAUTHN;
    let passkey = match webauthn.finish_passkey_registration(&reg_response, &reg_state) {
        Ok(pk) => pk,
        Err(e) => {
            error!("Failed to complete passkey registration: {:?}", e);
            return errors::bad_request("Failed to verify registration");
        }
    };

    // Generate and validate passkey name (consistent with rename validation)
    let user_agent = req
        .headers()
        .get("User-Agent")
        .and_then(|v| v.to_str().ok());
    let passkey_name = body
        .passkey_name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty() && name.len() <= 100)
        .map(|name| name.to_string())
        .unwrap_or_else(|| webauthn::generate_passkey_name(user_agent));

    // Create stored credential
    let credential_id = credential_id_to_string(passkey.cred_id());
    let stored_credential = StoredPasskeyCredential {
        id: credential_id.clone(),
        name: passkey_name.clone(),
        credential: passkey,
        transports: vec!["internal".to_string()], // Default, can be updated based on response
        created_at: chrono::Utc::now(),
        last_used_at: None,
        backup_eligible: false,
        backup_state: false,
    };

    if let Err(e) = webauthn::add_credential(&mut conn, &user_uuid, &stored_credential) {
        error!("Failed to save passkey: {:?}", e);
        return errors::internal("Failed to save passkey");
    }

    // W2: record passkey registration to security_events.
    let _ = crate::utils::security_events::record_security_event(
        &mut conn,
        crate::utils::security_events::SecurityEventInput {
            user_uuid: Some(user_uuid),
            event_type: "passkey_registered",
            severity: "info",
            details: Some(json!({ "credential_id": credential_id, "name": passkey_name })),
            request: Some(&req),
            session_id: None,
        },
    );

    info!(
        "Passkey registered for user {}: {}",
        user_uuid, passkey_name
    );

    HttpResponse::Ok().json(json!({
        "success": true,
        "passkey": {
            "id": credential_id,
            "name": passkey_name,
            "created_at": chrono::Utc::now().to_rfc3339()
        }
    }))
}

// =============================================================================
// Authentication Handlers
// =============================================================================

/// Start passkey login - generates challenge for authentication
/// Supports both discoverable (usernameless) and non-discoverable authentication
pub async fn start_passkey_login(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<StartLoginRequest>,
) -> impl Responder {
    // Rate limiting based on IP for discoverable auth, email for non-discoverable
    let redis_url = get_redis_url();

    // Check if this is a discoverable (usernameless) login
    let email = body
        .email
        .as_ref()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
        .map(|e| e.to_lowercase());

    // Rate limit key - use IP for usernameless, email for email-based
    let rate_key = if let Some(ref email) = email {
        format!("passkey_login_attempts:{email}")
    } else {
        let ip = crate::utils::client_ip::from_http_request(&req)
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        format!("passkey_login_attempts:ip:{ip}")
    };

    match RateLimiter::check_rate_limit(&redis_url, &rate_key, 10, 300).await {
        Ok(false) => {
            return errors::too_many_requests(
                "Too many login attempts. Please try again later.",
                300,
            );
        }
        Err(e) => {
            warn!("Rate limit check failed: {:?}", e);
            // Continue anyway - fail open for availability
        }
        _ => {}
    }

    let webauthn = &*WEBAUTHN;

    // AUD-007: every start_login takes the discoverable-auth
    // path, regardless of whether an email was supplied or
    // whether that email maps to a user with passkeys. This
    // makes the response shape (empty allowCredentials, session
    // id) and the work performed (one challenge generation,
    // one Redis write) identical across "no email", "email of
    // nonexistent user", "email of user with no passkeys", and
    // "email of user with passkeys". finish_passkey_login keys
    // off the credential id presented by the authenticator, so
    // the protocol still works end-to-end for real users while
    // missing-account paths fail at finish-time with the same
    // error a wrong-passkey on a real account produces.

    // Do the user lookup even when we won't use the result. The
    // wall-clock cost of `get_user_by_email` and
    // `load_user_passkey_data` would otherwise distinguish
    // email-supplied from no-email calls.
    if let Some(ref email_lower) = email {
        if let Ok(mut conn) = pool.get() {
            if let Ok(user) = repository::user_helpers::get_user_by_email(email_lower, &mut conn) {
                let _ = webauthn::load_user_passkey_data(&mut conn, &user.uuid);
            }
        }
    }

    let session_id = webauthn::generate_auth_session_id();
    let (rcr, auth_state) = match webauthn.start_discoverable_authentication() {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to start passkey authentication: {:?}", e);
            return errors::internal("Failed to generate authentication challenge");
        }
    };

    if let Err(e) = webauthn::store_discoverable_auth_state(&session_id, &auth_state).await {
        error!("Failed to store auth state: {:?}", e);
        return errors::internal("Failed to store authentication state");
    }

    debug!(?email, %session_id, "started passkey authentication");

    let rcr_json = serde_json::to_value(&rcr).unwrap_or(json!({}));
    let mut response = rcr_json.get("publicKey").cloned().unwrap_or(rcr_json);
    if let Some(obj) = response.as_object_mut() {
        obj.insert("sessionId".to_string(), json!(session_id));
    }
    HttpResponse::Ok().json(response)
}

/// Complete passkey login
/// Supports both discoverable (usernameless) and non-discoverable authentication
pub async fn finish_passkey_login(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<FinishLoginRequest>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Parse the credential ID to find the user
    let credential_id = &body.id;

    // Find user with this credential
    let (user, _email) = match find_user_by_credential_id(&mut conn, credential_id) {
        Some(result) => result,
        None => {
            warn!("No user found with credential ID: {}", credential_id);
            return errors::unauthorized("Invalid passkey");
        }
    };

    // Parse the authentication response
    let auth_response: PublicKeyCredential = match serde_json::from_value(json!({
        "id": body.id,
        "rawId": body.raw_id,
        "response": body.response,
        "type": body.credential_type,
        "clientExtensionResults": body.client_extension_results.clone().unwrap_or(json!({})),
        "authenticatorAttachment": body.authenticator_attachment.clone()
    })) {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to parse authentication response: {:?}", e);
            return errors::bad_request("Invalid authentication response");
        }
    };

    let webauthn = &*WEBAUTHN;

    // Run the appropriate finish ceremony and capture the
    // AuthenticationResult so we can persist the bumped sign counter
    // + observed backup_state back to the credential row.
    let auth_result = if let Some(ref session_id) = body.session_id {
        // Discoverable authentication flow
        let auth_state = match webauthn::get_discoverable_auth_state(session_id).await {
            Ok(state) => state,
            Err(e) => {
                warn!("Discoverable auth state not found: {:?}", e);
                return errors::bad_request("Authentication challenge expired or not found");
            }
        };

        // Get the user's passkey for verification
        let passkey_data = match webauthn::load_user_passkey_data(&mut conn, &user.uuid) {
            Ok(data) => data,
            Err(e) => {
                error!("Failed to load passkeys for {}: {:?}", user.uuid, e);
                return errors::internal("Failed to load passkeys");
            }
        };
        let stored_cred = match passkey_data.find_credential(credential_id) {
            Some(cred) => cred,
            None => {
                error!("Credential not found in user's passkey data");
                return errors::unauthorized("Invalid passkey");
            }
        };

        // Complete discoverable authentication
        // Convert Passkey to DiscoverableKey for the API
        let discoverable_key: DiscoverableKey = stored_cred.credential.clone().into();
        let creds = vec![discoverable_key];
        match webauthn.finish_discoverable_authentication(&auth_response, auth_state, &creds) {
            Ok(result) => {
                debug!(
                    "Discoverable passkey authentication successful for user {}",
                    user.uuid
                );
                result
            }
            Err(e) => {
                error!(
                    "Failed to complete discoverable passkey authentication: {:?}",
                    e
                );
                return errors::unauthorized("Authentication failed");
            }
        }
    } else {
        // Non-discoverable authentication flow - try to get state by email
        // Since we found the user by credential ID, get their email
        let email = match repository::user_helpers::get_primary_email(&user.uuid, &mut conn) {
            Some(e) => e,
            None => {
                error!("Could not get email for user {}", user.uuid);
                return errors::internal("User email not found");
            }
        };

        let auth_state = match webauthn::get_authentication_state(&email).await {
            Ok(state) => state,
            Err(e) => {
                warn!("Authentication state not found: {:?}", e);
                return errors::bad_request("Authentication challenge expired or not found");
            }
        };

        // Complete non-discoverable authentication
        match webauthn.finish_passkey_authentication(&auth_response, &auth_state) {
            Ok(result) => {
                debug!("Passkey authentication successful for user {}", user.uuid);
                result
            }
            Err(e) => {
                error!("Failed to complete passkey authentication: {:?}", e);
                return errors::unauthorized("Authentication failed");
            }
        }
    };

    // Persist the bumped sign counter + current backup_state back
    // to the credential row. Replaces the earlier
    // `touch_last_used`-only post-auth which left WebAuthn's clone-
    // detection property inoperative (counter never advanced past
    // registration). Failure is logged but not fatal to the
    // otherwise-successful login.
    let post_auth = match webauthn::update_credential_post_auth(&mut conn, &user.uuid, &auth_result) {
        Ok(outcome) => outcome,
        Err(e) => {
            warn!("Failed to persist post-auth credential update: {:?}", e);
            webauthn::CredentialPostAuthOutcome::default()
        }
    };

    // If the credential's WebAuthn `backup_state` flag flipped
    // between authentications, the credential may now be backed up
    // to a different ecosystem than the one that registered it
    // (WebAuthn L3 §6.1.3 clone-detection signal). Emit a security
    // event so the user / operator can review.
    if let Some((previous_backup_state, new_backup_state)) = post_auth.backup_state_flip {
        let _ = crate::utils::security_events::record_security_event(
            &mut conn,
            crate::utils::security_events::SecurityEventInput {
                user_uuid: Some(user.uuid),
                event_type: "passkey_backup_state_changed",
                severity: "warning",
                details: Some(serde_json::json!({
                    "credential_id": credential_id,
                    "previous_backup_state": previous_backup_state,
                    "new_backup_state": new_backup_state,
                    "reason": "WebAuthn backup_state flip; credential may now be synced to a different ecosystem",
                })),
                request: Some(&req),
                session_id: None,
            },
        );
    }

    // Create session + tokens, return response with auth cookies
    let user_uuid = user.uuid;
    let session = super::auth::create_session_record(&user_uuid, &req, &mut conn).map_err(|e| {
        error!(
            "Failed to create session for passkey login {}: {:?}",
            user_uuid, e
        );
    });
    let session = match session {
        Ok(s) => s,
        Err(_) => return errors::internal("Failed to create authentication session"),
    };
    let family_id = uuid::Uuid::new_v4();

    match jwt_helpers::create_login_response(user, &session.session_id, &family_id, &mut conn) {
        Ok((response, tokens)) => {
            info!("Passkey login successful for user {}", user_uuid);
            super::auth::build_auth_cookie_response(
                json!({
                    "success": true,
                    "csrf_token": response.csrf_token,
                    "user": response.user
                }),
                &tokens,
            )
        }
        Err(error_response) => error_response,
    }
}

/// Find the user that owns a given WebAuthn credential ID.
///
/// `credential_id` has a UNIQUE index on `passkey_credentials`, so
/// this is an O(log n) index lookup followed by the user fetch.
/// Compare to the previous JSONB-blob design which scanned every
/// users row on every login.
fn find_user_by_credential_id(
    conn: &mut crate::db::DbConnection,
    credential_id: &str,
) -> Option<(crate::models::User, String)> {
    let row = repository::passkey_credentials::find_by_credential_id(conn, credential_id)
        .ok()
        .flatten()?;
    let user = repository::get_user_by_uuid(&row.user_uuid, conn).ok()?;
    let email = repository::user_helpers::get_primary_email(&user.uuid, conn)?;
    Some((user, email))
}

// =============================================================================
// Management Handlers
// =============================================================================

/// List all passkeys for the current user
pub async fn list_passkeys(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request("Invalid user UUID");
        }
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    if repository::get_user_by_uuid(&user_uuid, &mut conn).is_err() {
        return errors::not_found_msg("User not found");
    }

    let passkey_data = match webauthn::load_user_passkey_data(&mut conn, &user_uuid) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to load passkeys for {}: {:?}", user_uuid, e);
            return errors::internal("Failed to load passkeys");
        }
    };
    let passkeys: Vec<PasskeyInfo> = passkey_data
        .credentials
        .iter()
        .map(|c| PasskeyInfo {
            id: c.id.clone(),
            name: c.name.clone(),
            created_at: c.created_at.to_rfc3339(),
            last_used_at: c.last_used_at.map(|t| t.to_rfc3339()),
            transports: c.transports.clone(),
            backup_eligible: c.backup_eligible,
        })
        .collect();

    HttpResponse::Ok().json(PasskeyListResponse { passkeys })
}

/// Rename a passkey
pub async fn rename_passkey(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<RenamePasskeyRequest>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request("Invalid user UUID");
        }
    };

    let credential_id = path.into_inner();
    let new_name = body.name.trim();

    if new_name.is_empty() || new_name.len() > 100 {
        return errors::bad_request("Passkey name must be between 1 and 100 characters");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    if repository::get_user_by_uuid(&user_uuid, &mut conn).is_err() {
        return errors::not_found_msg("User not found");
    }

    match webauthn::rename_credential(&mut conn, &user_uuid, &credential_id, new_name) {
        Ok(true) => {}
        Ok(false) => return errors::not_found_msg("Passkey not found"),
        Err(e) => {
            error!("Failed to rename passkey: {:?}", e);
            return errors::internal("Failed to rename passkey");
        }
    }

    info!("Passkey {} renamed for user {}", credential_id, user_uuid);

    HttpResponse::Ok().json(json!({
        "success": true
    }))
}

/// Delete a passkey (requires password verification)
pub async fn delete_passkey(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<DeletePasskeyRequest>,
) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request("Invalid user UUID");
        }
    };

    let credential_id = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    if repository::get_user_by_uuid(&user_uuid, &mut conn).is_err() {
        return errors::not_found_msg("User not found");
    }

    // Verify password
    let password_hash = match get_local_password_hash(&user_uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            return errors::bad_request("Password verification not available for this account");
        }
    };

    let password_valid = bcrypt::verify(&body.password, &password_hash).unwrap_or(false);
    if !password_valid {
        return errors::unauthorized("Incorrect password");
    }

    match webauthn::delete_credential(&mut conn, &user_uuid, &credential_id) {
        Ok(true) => {}
        Ok(false) => return errors::not_found_msg("Passkey not found"),
        Err(e) => {
            error!("Failed to delete passkey: {:?}", e);
            return errors::internal("Failed to delete passkey");
        }
    }

    // W2: record passkey deletion to security_events.
    let _ = crate::utils::security_events::record_security_event(
        &mut conn,
        crate::utils::security_events::SecurityEventInput {
            user_uuid: Some(user_uuid),
            event_type: "passkey_deleted",
            severity: "warning",
            details: Some(json!({ "credential_id": credential_id })),
            request: Some(&req),
            session_id: None,
        },
    );

    info!("Passkey {} deleted for user {}", credential_id, user_uuid);

    HttpResponse::Ok().json(json!({
        "success": true
    }))
}

/// Helper function to get password hash from user_auth_identities for local auth
fn get_local_password_hash(
    user_uuid: &Uuid,
    conn: &mut crate::db::DbConnection,
) -> Result<String, String> {
    use crate::schema::user_auth_identities;
    use diesel::prelude::*;

    let password_hash: Option<String> = user_auth_identities::table
        .filter(user_auth_identities::user_uuid.eq(user_uuid))
        .filter(user_auth_identities::provider_type.eq("local"))
        .select(user_auth_identities::password_hash)
        .first::<Option<String>>(conn)
        .optional()
        .map_err(|e| format!("Database error: {e}"))?
        .flatten();

    password_hash.ok_or_else(|| "No local password found for this user".to_string())
}

// =============================================================================
// MFA Setup Flow Handlers (credential-based, no JWT required)
// =============================================================================

/// Request types for MFA setup flow
#[derive(Debug, Deserialize)]
pub struct PasskeySetupStartRequest {
    pub email: String,
    pub password: String,
    pub passkey_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PasskeySetupFinishRequest {
    pub email: String,
    pub password: String,
    pub id: String,
    #[serde(rename = "rawId")]
    pub raw_id: String,
    pub response: serde_json::Value,
    #[serde(rename = "type")]
    pub credential_type: String,
    pub passkey_name: Option<String>,
    #[serde(rename = "clientExtensionResults")]
    pub client_extension_results: Option<serde_json::Value>,
    pub authenticator_attachment: Option<String>,
}

// Account lockout configuration (same as main login)
const MAX_LOGIN_ATTEMPTS: u32 = 5;
const LOCKOUT_DURATION_SECONDS: u64 = 900; // 15 minutes

/// Start passkey registration during MFA setup flow
/// This endpoint accepts email+password instead of requiring JWT authentication
pub async fn start_passkey_setup_login(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<PasskeySetupStartRequest>,
) -> impl Responder {
    let redis_url = get_redis_url();
    let email_lower = body.email.to_lowercase();
    let client_ip = crate::utils::client_ip::from_http_request(&req);
    let lockout_key = RateLimiter::login_attempt_key(&email_lower, client_ip);

    // Check if account is locked before any validation
    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %email_lower, remaining_seconds, "Passkey setup attempt on locked account");
            return errors::too_many_requests(
                format!(
                    "Account temporarily locked. Try again in {} seconds.",
                    remaining_seconds
                ),
                remaining_seconds as u64,
            );
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            warn!("Failed to check account lockout: {:?}", e);
            // Continue anyway - fail open for availability, but log the issue
        }
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Find user by email
    let user = match repository::user_helpers::get_user_by_email(&email_lower, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            // Record failed attempt even for non-existent users (prevents enumeration)
            let _ = RateLimiter::record_failed_attempt(
                &redis_url,
                &lockout_key,
                LOCKOUT_DURATION_SECONDS,
            )
            .await;
            return errors::unauthorized("Invalid email or password");
        }
    };

    // Verify password
    let password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            let _ = RateLimiter::record_failed_attempt(
                &redis_url,
                &lockout_key,
                LOCKOUT_DURATION_SECONDS,
            )
            .await;
            return errors::unauthorized("Invalid email or password");
        }
    };

    let password_valid = bcrypt::verify(&body.password, &password_hash).unwrap_or(false);
    if !password_valid {
        // Record failed attempt
        match RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS)
            .await
        {
            Ok(attempts) => {
                let remaining = MAX_LOGIN_ATTEMPTS.saturating_sub(attempts);
                if remaining == 0 {
                    warn!(email = %email_lower, "Account locked after failed passkey setup attempts");
                }
            }
            Err(e) => warn!("Failed to record failed attempt: {:?}", e),
        }
        return errors::unauthorized("Invalid email or password");
    }

    // Clear failed attempts on successful password verification
    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful passkey setup auth");
    }

    // Verify that user needs MFA setup (security check)
    if mfa::user_has_mfa_enabled(&user) {
        return errors::bad_request("MFA is already enabled for this account");
    }

    // Verify that MFA is required for this user
    if mfa::validate_mfa_policy(&user, &mut conn).await.is_ok() {
        return errors::bad_request("MFA is not required for this account");
    }

    // Check passkey limit
    match webauthn::can_add_passkey(&mut conn, &user.uuid) {
        Ok(false) => {
            return HttpResponse::BadRequest().json(json!({
                "error": i18n::tr(&request_locale(&req), "backend-error-passkey-max-reached"),
                "code": "backend-error-passkey-max-reached",
                "max_passkeys": webauthn::MAX_PASSKEYS_PER_USER
            }));
        }
        Err(e) => {
            error!("Failed to check passkey count: {:?}", e);
            return errors::internal("Failed to check passkey count");
        }
        Ok(true) => {}
    }

    // Get user's primary email
    let primary_email = match repository::user_helpers::get_primary_email(&user.uuid, &mut conn) {
        Some(email) => email,
        None => {
            return errors::internal("Could not retrieve user email");
        }
    };

    let passkey_data = match webauthn::load_user_passkey_data(&mut conn, &user.uuid) {
        Ok(data) => data,
        Err(e) => {
            error!("Failed to load existing passkeys: {:?}", e);
            return errors::internal("Failed to load existing passkeys");
        }
    };
    let exclude_credentials: Vec<CredentialID> = passkey_data
        .credentials
        .iter()
        .map(|c| c.credential.cred_id().clone())
        .collect();

    // Create WebAuthn registration challenge
    let webauthn = &*WEBAUTHN;

    let (ccr, reg_state) = match webauthn.start_passkey_registration(
        user.uuid,
        &primary_email,
        &user.name,
        Some(exclude_credentials),
    ) {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to start passkey registration: {:?}", e);
            return errors::internal("Failed to generate registration challenge");
        }
    };

    // Store registration state in Redis
    if let Err(e) = webauthn::store_registration_state(&user.uuid, &reg_state).await {
        error!("Failed to store registration state: {:?}", e);
        return errors::internal("Failed to store registration state");
    }

    debug!("Started passkey setup registration for user {}", user.uuid);

    // Return the challenge options
    let ccr_json = serde_json::to_value(&ccr).unwrap_or(json!({}));

    if let Some(public_key) = ccr_json.get("publicKey") {
        let mut options = public_key.clone();
        if let Some(obj) = options.as_object_mut() {
            let auth_selection = obj
                .entry("authenticatorSelection")
                .or_insert_with(|| json!({}));

            if let Some(auth_obj) = auth_selection.as_object_mut() {
                auth_obj.insert("residentKey".to_string(), json!("required"));
                auth_obj.insert("requireResidentKey".to_string(), json!(true));
            }
        }
        HttpResponse::Ok().json(options)
    } else {
        HttpResponse::Ok().json(ccr_json)
    }
}

/// Complete passkey registration during MFA setup flow and log user in
pub async fn finish_passkey_setup_login(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<PasskeySetupFinishRequest>,
) -> impl Responder {
    let redis_url = get_redis_url();
    let email_lower = body.email.to_lowercase();
    let client_ip = crate::utils::client_ip::from_http_request(&req);
    let lockout_key = RateLimiter::login_attempt_key(&email_lower, client_ip);

    // Check if account is locked before any validation
    match RateLimiter::check_lockout(&redis_url, &lockout_key, MAX_LOGIN_ATTEMPTS).await {
        Ok(Some(remaining_seconds)) => {
            warn!(email = %email_lower, remaining_seconds, "Passkey setup finish attempt on locked account");
            return errors::too_many_requests(
                format!(
                    "Account temporarily locked. Try again in {} seconds.",
                    remaining_seconds
                ),
                remaining_seconds as u64,
            );
        }
        Ok(None) => {} // Not locked, continue
        Err(e) => {
            warn!("Failed to check account lockout: {:?}", e);
        }
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Find user by email and verify password again (security)
    let user = match repository::user_helpers::get_user_by_email(&email_lower, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            let _ = RateLimiter::record_failed_attempt(
                &redis_url,
                &lockout_key,
                LOCKOUT_DURATION_SECONDS,
            )
            .await;
            return errors::unauthorized("Invalid email or password");
        }
    };

    let password_hash = match get_local_password_hash(&user.uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            let _ = RateLimiter::record_failed_attempt(
                &redis_url,
                &lockout_key,
                LOCKOUT_DURATION_SECONDS,
            )
            .await;
            return errors::unauthorized("Invalid email or password");
        }
    };

    let password_valid = bcrypt::verify(&body.password, &password_hash).unwrap_or(false);
    if !password_valid {
        match RateLimiter::record_failed_attempt(&redis_url, &lockout_key, LOCKOUT_DURATION_SECONDS)
            .await
        {
            Ok(attempts) => {
                let remaining = MAX_LOGIN_ATTEMPTS.saturating_sub(attempts);
                if remaining == 0 {
                    warn!(email = %email_lower, "Account locked after failed passkey setup finish attempts");
                }
            }
            Err(e) => warn!("Failed to record failed attempt: {:?}", e),
        }
        return errors::unauthorized("Invalid email or password");
    }

    // Clear failed attempts on successful password verification
    if let Err(e) = RateLimiter::clear_attempts(&redis_url, &lockout_key).await {
        warn!(error = %e, "Failed to clear login attempts after successful passkey setup finish auth");
    }

    // Security checks
    if mfa::user_has_mfa_enabled(&user) {
        return errors::bad_request("MFA is already enabled for this account");
    }

    if mfa::validate_mfa_policy(&user, &mut conn).await.is_ok() {
        return errors::bad_request("MFA is not required for this account");
    }

    // Retrieve registration state from Redis
    let reg_state = match webauthn::get_registration_state(&user.uuid).await {
        Ok(state) => state,
        Err(e) => {
            warn!(
                "Registration state not found for user {}: {:?}",
                user.uuid, e
            );
            return errors::bad_request("Registration challenge expired or not found");
        }
    };

    // Parse the registration response
    let reg_response: RegisterPublicKeyCredential = match serde_json::from_value(json!({
        "id": body.id,
        "rawId": body.raw_id,
        "response": body.response,
        "type": body.credential_type,
        "clientExtensionResults": body.client_extension_results.clone().unwrap_or(json!({})),
        "authenticatorAttachment": body.authenticator_attachment.clone()
    })) {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to parse registration response: {:?}", e);
            return errors::bad_request("Invalid registration response");
        }
    };

    // Complete registration with WebAuthn
    let webauthn = &*WEBAUTHN;
    let passkey = match webauthn.finish_passkey_registration(&reg_response, &reg_state) {
        Ok(pk) => pk,
        Err(e) => {
            error!("Failed to complete passkey registration: {:?}", e);
            return errors::bad_request("Failed to verify registration");
        }
    };

    // Generate passkey name
    let user_agent = req
        .headers()
        .get("User-Agent")
        .and_then(|v| v.to_str().ok());
    let passkey_name = body
        .passkey_name
        .as_ref()
        .map(|name| name.trim())
        .filter(|name| !name.is_empty() && name.len() <= 100)
        .map(|name| name.to_string())
        .unwrap_or_else(|| webauthn::generate_passkey_name(user_agent));

    // Create stored credential
    let credential_id = credential_id_to_string(passkey.cred_id());
    let stored_credential = StoredPasskeyCredential {
        id: credential_id.clone(),
        name: passkey_name.clone(),
        credential: passkey,
        transports: vec!["internal".to_string()],
        created_at: chrono::Utc::now(),
        last_used_at: None,
        backup_eligible: false,
        backup_state: false,
    };

    if let Err(e) = webauthn::add_credential(&mut conn, &user.uuid, &stored_credential) {
        error!("Failed to save passkey: {:?}", e);
        return errors::internal("Failed to save passkey");
    }

    // Generate backup codes for recovery (passkey users need
    // these too). Writes directly to the dedicated
    // `user_recovery_codes` table; failure logs but doesn't fail
    // the registration because the passkey is already saved
    // (worst case: user proceeds with no recovery codes and is
    // prompted to regenerate later).
    let (plaintext_codes, hashed_codes) = mfa::generate_backup_codes_async().await;
    let backup_codes_saved = match repository::user_recovery_codes::replace_all(
        &mut conn,
        &user.uuid,
        hashed_codes,
    ) {
        Ok(_) => true,
        Err(e) => {
            warn!(
                "Failed to save backup codes for passkey user {}: {:?}",
                user.uuid, e
            );
            false
        }
    };

    info!(
        "Passkey registered during MFA setup for user {}: {}",
        user.uuid, passkey_name
    );

    // Create session + tokens, return response with auth cookies
    let user_uuid = user.uuid;
    let session = super::auth::create_session_record(&user_uuid, &req, &mut conn).map_err(|e| {
        error!(
            "Failed to create session for passkey setup login {}: {:?}",
            user_uuid, e
        );
    });
    let session = match session {
        Ok(s) => s,
        Err(_) => return errors::internal("Failed to create authentication session"),
    };
    let family_id = uuid::Uuid::new_v4();

    match jwt_helpers::create_login_response(user, &session.session_id, &family_id, &mut conn) {
        Ok((response, tokens)) => {
            info!("Passkey setup login successful for user {}", user_uuid);

            let mut response_json = json!({
                "success": true,
                "csrf_token": response.csrf_token,
                "user": response.user,
                "passkey": {
                    "id": credential_id,
                    "name": passkey_name
                }
            });
            if backup_codes_saved {
                response_json["backup_codes"] = json!(plaintext_codes);
            }

            super::auth::build_auth_cookie_response(response_json, &tokens)
        }
        Err(error_response) => error_response,
    }
}
