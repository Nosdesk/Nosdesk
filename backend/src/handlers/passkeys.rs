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
use crate::models::Claims;
use crate::repository;
use crate::utils::webauthn::{
    self, credential_id_to_string, get_user_passkey_data, save_user_passkey_data,
    StoredPasskeyCredential, WEBAUTHN,
};
use crate::utils::cookies::{
    create_access_token_cookie, create_refresh_token_cookie, create_csrf_token_cookie,
};
use crate::utils::jwt::helpers as jwt_helpers;
use crate::utils::rate_limit::{get_redis_url, RateLimiter};

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
            return HttpResponse::Unauthorized().json(json!({
                "error": "Authentication required"
            }));
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "error": "Invalid user UUID"
            }));
        }
    };

    // Rate limiting: 5 passkey registrations per hour per user
    let redis_url = get_redis_url();
    let rate_key = format!("passkey_registration:{user_uuid}");
    match RateLimiter::check_rate_limit(&redis_url, &rate_key, 5, 3600).await {
        Ok(false) => {
            return HttpResponse::TooManyRequests().json(json!({
                "error": "Too many passkey registration attempts. Please try again later."
            }));
        }
        Err(e) => {
            warn!("Rate limit check failed for passkey registration: {:?}", e);
            // Continue anyway - fail open for availability
        }
        _ => {}
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json(json!({
                "error": "Database connection error"
            }));
        }
    };

    // Get user from database
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::NotFound().json(json!({
                "error": "User not found"
            }));
        }
    };

    // Check passkey limit
    if !webauthn::can_add_passkey(&user) {
        return HttpResponse::BadRequest().json(json!({
            "error": "Maximum number of passkeys reached",
            "max_passkeys": webauthn::MAX_PASSKEYS_PER_USER
        }));
    }

    // Get user's primary email
    let primary_email = match repository::user_helpers::get_primary_email(&user_uuid, &mut conn) {
        Some(email) => email,
        None => {
            return HttpResponse::InternalServerError().json(json!({
                "error": "Could not retrieve user email"
            }));
        }
    };

    // Get existing credentials to exclude
    let passkey_data = get_user_passkey_data(&user);
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
            return HttpResponse::InternalServerError().json(json!({
                "error": "Failed to generate registration challenge"
            }));
        }
    };

    // Store registration state in Redis
    if let Err(e) = webauthn::store_registration_state(&user_uuid, &reg_state).await {
        error!("Failed to store registration state: {:?}", e);
        return HttpResponse::InternalServerError().json(json!({
            "error": "Failed to store registration state"
        }));
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
            return HttpResponse::Unauthorized().json(json!({
                "error": "Authentication required"
            }));
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "error": "Invalid user UUID"
            }));
        }
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json(json!({
                "error": "Database connection error"
            }));
        }
    };

    // Get user from database
    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::NotFound().json(json!({
                "error": "User not found"
            }));
        }
    };

    // Retrieve registration state from Redis
    let reg_state = match webauthn::get_registration_state(&user_uuid).await {
        Ok(state) => state,
        Err(e) => {
            warn!("Registration state not found for user {}: {:?}", user_uuid, e);
            return HttpResponse::BadRequest().json(json!({
                "error": "Registration challenge expired or not found"
            }));
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
            return HttpResponse::BadRequest().json(json!({
                "error": "Invalid registration response"
            }));
        }
    };

    // Complete registration with WebAuthn
    let webauthn = &*WEBAUTHN;
    let passkey = match webauthn.finish_passkey_registration(&reg_response, &reg_state) {
        Ok(pk) => pk,
        Err(e) => {
            error!("Failed to complete passkey registration: {:?}", e);
            return HttpResponse::BadRequest().json(json!({
                "error": "Failed to verify registration"
            }));
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

    // Add to user's passkey data
    let mut passkey_data = get_user_passkey_data(&user);
    passkey_data.add_credential(stored_credential);

    // Save to database
    if let Err(e) = save_user_passkey_data(&mut conn, &user_uuid, &passkey_data) {
        error!("Failed to save passkey: {:?}", e);
        return HttpResponse::InternalServerError().json(json!({
            "error": "Failed to save passkey"
        }));
    }

    info!("Passkey registered for user {}: {}", user_uuid, passkey_name);

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
    let email = body.email.as_ref()
        .map(|e| e.trim())
        .filter(|e| !e.is_empty())
        .map(|e| e.to_lowercase());

    // Rate limit key - use IP for usernameless, email for email-based
    let rate_key = if let Some(ref email) = email {
        format!("passkey_login_attempts:{email}")
    } else {
        let ip = req.connection_info().realip_remote_addr()
            .unwrap_or("unknown")
            .to_string();
        format!("passkey_login_attempts:ip:{ip}")
    };

    match RateLimiter::check_rate_limit(&redis_url, &rate_key, 10, 300).await {
        Ok(false) => {
            return HttpResponse::TooManyRequests().json(json!({
                "error": "Too many login attempts. Please try again later."
            }));
        }
        Err(e) => {
            warn!("Rate limit check failed: {:?}", e);
            // Continue anyway - fail open for availability
        }
        _ => {}
    }

    let webauthn = &*WEBAUTHN;

    // Discoverable (usernameless) authentication - no email required
    if email.is_none() {
        // Generate a session ID for this auth attempt
        let session_id = webauthn::generate_auth_session_id();

        // Start discoverable authentication (empty allowCredentials)
        let (rcr, auth_state) = match webauthn.start_discoverable_authentication() {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to start discoverable passkey authentication: {:?}", e);
                return HttpResponse::InternalServerError().json(json!({
                    "error": "Failed to generate authentication challenge"
                }));
            }
        };

        // Store discoverable authentication state in Redis
        if let Err(e) = webauthn::store_discoverable_auth_state(&session_id, &auth_state).await {
            error!("Failed to store discoverable auth state: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "error": "Failed to store authentication state"
            }));
        }

        debug!("Started discoverable passkey authentication, session_id={}", session_id);

        // Return the challenge options with session ID
        let rcr_json = serde_json::to_value(&rcr).unwrap_or(json!({}));

        // Extract publicKey and add session_id
        if let Some(public_key) = rcr_json.get("publicKey") {
            let mut response = public_key.clone();
            if let Some(obj) = response.as_object_mut() {
                obj.insert("sessionId".to_string(), json!(session_id));
            }
            return HttpResponse::Ok().json(response);
        } else {
            let mut response = rcr_json.clone();
            if let Some(obj) = response.as_object_mut() {
                obj.insert("sessionId".to_string(), json!(session_id));
            }
            return HttpResponse::Ok().json(response);
        }
    }

    // Non-discoverable authentication - email required
    let email = email.unwrap();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json(json!({
                "error": "Database connection error"
            }));
        }
    };

    // Find user by email
    let user = match repository::user_helpers::get_user_by_email(&email, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            debug!("User not found for passkey login: {}", email);
            return HttpResponse::BadRequest().json(json!({
                "error": "No passkeys registered for this account"
            }));
        }
    };

    // Get user's passkeys
    let passkey_data = get_user_passkey_data(&user);
    if passkey_data.credentials.is_empty() {
        return HttpResponse::BadRequest().json(json!({
            "error": "No passkeys registered for this account"
        }));
    }

    // Get passkeys for authentication
    let passkeys = passkey_data.get_passkeys();

    // Create authentication challenge
    let (rcr, auth_state) = match webauthn.start_passkey_authentication(&passkeys) {
        Ok(result) => result,
        Err(e) => {
            error!("Failed to start passkey authentication: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "error": "Failed to generate authentication challenge"
            }));
        }
    };

    // Store authentication state in Redis
    if let Err(e) = webauthn::store_authentication_state(&email, &auth_state).await {
        error!("Failed to store authentication state: {:?}", e);
        return HttpResponse::InternalServerError().json(json!({
            "error": "Failed to store authentication state"
        }));
    }

    debug!("Started passkey authentication for {}", email);

    // Return the challenge options to the client
    let rcr_json = serde_json::to_value(&rcr).unwrap_or(json!({}));

    if let Some(public_key) = rcr_json.get("publicKey") {
        HttpResponse::Ok().json(public_key)
    } else {
        HttpResponse::Ok().json(rcr_json)
    }
}

/// Complete passkey login
/// Supports both discoverable (usernameless) and non-discoverable authentication
pub async fn finish_passkey_login(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<FinishLoginRequest>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json(json!({
                "error": "Database connection error"
            }));
        }
    };

    // Parse the credential ID to find the user
    let credential_id = &body.id;

    // Find user with this credential
    let (user, _email) = match find_user_by_credential_id(&mut conn, credential_id) {
        Some(result) => result,
        None => {
            warn!("No user found with credential ID: {}", credential_id);
            return HttpResponse::Unauthorized().json(json!({
                "error": "Invalid passkey"
            }));
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
            return HttpResponse::BadRequest().json(json!({
                "error": "Invalid authentication response"
            }));
        }
    };

    let webauthn = &*WEBAUTHN;

    // Check if this is a discoverable authentication (has session_id)
    if let Some(ref session_id) = body.session_id {
        // Discoverable authentication flow
        let auth_state = match webauthn::get_discoverable_auth_state(session_id).await {
            Ok(state) => state,
            Err(e) => {
                warn!("Discoverable auth state not found: {:?}", e);
                return HttpResponse::BadRequest().json(json!({
                    "error": "Authentication challenge expired or not found"
                }));
            }
        };

        // Get the user's passkey for verification
        let passkey_data = get_user_passkey_data(&user);
        let stored_cred = match passkey_data.find_credential(credential_id) {
            Some(cred) => cred,
            None => {
                error!("Credential not found in user's passkey data");
                return HttpResponse::Unauthorized().json(json!({
                    "error": "Invalid passkey"
                }));
            }
        };

        // Complete discoverable authentication
        // Convert Passkey to DiscoverableKey for the API
        let discoverable_key: DiscoverableKey = stored_cred.credential.clone().into();
        let creds = vec![discoverable_key];
        match webauthn.finish_discoverable_authentication(&auth_response, auth_state, &creds) {
            Ok(_result) => {
                debug!("Discoverable passkey authentication successful for user {}", user.uuid);
            }
            Err(e) => {
                error!("Failed to complete discoverable passkey authentication: {:?}", e);
                return HttpResponse::Unauthorized().json(json!({
                    "error": "Authentication failed"
                }));
            }
        };
    } else {
        // Non-discoverable authentication flow - try to get state by email
        // Since we found the user by credential ID, get their email
        let email = match repository::user_helpers::get_primary_email(&user.uuid, &mut conn) {
            Some(e) => e,
            None => {
                error!("Could not get email for user {}", user.uuid);
                return HttpResponse::InternalServerError().json(json!({
                    "error": "User email not found"
                }));
            }
        };

        let auth_state = match webauthn::get_authentication_state(&email).await {
            Ok(state) => state,
            Err(e) => {
                warn!("Authentication state not found: {:?}", e);
                return HttpResponse::BadRequest().json(json!({
                    "error": "Authentication challenge expired or not found"
                }));
            }
        };

        // Complete non-discoverable authentication
        match webauthn.finish_passkey_authentication(&auth_response, &auth_state) {
            Ok(_result) => {
                debug!("Passkey authentication successful for user {}", user.uuid);
            }
            Err(e) => {
                error!("Failed to complete passkey authentication: {:?}", e);
                return HttpResponse::Unauthorized().json(json!({
                    "error": "Authentication failed"
                }));
            }
        };
    }

    // Update the credential's last_used_at timestamp
    let mut passkey_data = get_user_passkey_data(&user);
    if let Some(cred) = passkey_data.find_credential_mut(credential_id) {
        cred.last_used_at = Some(chrono::Utc::now());
    }

    if let Err(e) = save_user_passkey_data(&mut conn, &user.uuid, &passkey_data) {
        warn!("Failed to update passkey after auth: {:?}", e);
        // Don't fail login for this
    }

    // Create session and tokens using jwt_helpers (same as regular login)
    let user_uuid = user.uuid;

    match jwt_helpers::create_login_response(user, &mut conn) {
        Ok((response, tokens)) => {
            // Create session record after successful login
            if let Err(e) = super::auth::create_session_record(&user_uuid, &tokens.access_token, &req, &mut conn).await {
                warn!("Failed to create session record for passkey login: {:?}", e);
                // Don't fail the login if session creation fails
            }

            info!("Passkey login successful for user {}", user_uuid);

            // Set httpOnly cookies for tokens
            HttpResponse::Ok()
                .cookie(create_access_token_cookie(&tokens.access_token))
                .cookie(create_refresh_token_cookie(&tokens.refresh_token))
                .cookie(create_csrf_token_cookie(&tokens.csrf_token))
                .json(json!({
                    "success": true,
                    "csrf_token": response.csrf_token,
                    "user": response.user
                }))
        }
        Err(error_response) => error_response,
    }
}

/// Helper to find user by credential ID
/// Uses efficient JSONB query instead of loading all users
fn find_user_by_credential_id(
    conn: &mut crate::db::DbConnection,
    credential_id: &str,
) -> Option<(crate::models::User, String)> {
    use crate::schema::users;
    use diesel::prelude::*;
    use diesel::dsl::sql;
    use diesel::sql_types::Bool;

    // Use PostgreSQL JSONB containment operator to find the user with this credential
    // This searches for a credential with matching ID in the credentials array
    // Query: WHERE passkey_credentials->'credentials' @> '[{"id": "credential_id"}]'::jsonb
    let search_pattern = format!(r#"[{{"id": "{}"}}]"#, credential_id.replace('"', r#"\""#));

    let user: Option<crate::models::User> = users::table
        .filter(users::passkey_credentials.is_not_null())
        .filter(sql::<Bool>(&format!(
            "passkey_credentials->'credentials' @> '{search_pattern}'::jsonb"
        )))
        .first(conn)
        .ok();

    if let Some(user) = user {
        // Verify the credential actually exists (defense in depth)
        let passkey_data = get_user_passkey_data(&user);
        if passkey_data.find_credential(credential_id).is_some() {
            if let Some(email) = repository::user_helpers::get_primary_email(&user.uuid, conn) {
                return Some((user, email));
            }
        }
    }

    None
}

// =============================================================================
// Management Handlers
// =============================================================================

/// List all passkeys for the current user
pub async fn list_passkeys(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return HttpResponse::Unauthorized().json(json!({
                "error": "Authentication required"
            }));
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "error": "Invalid user UUID"
            }));
        }
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json(json!({
                "error": "Database connection error"
            }));
        }
    };

    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::NotFound().json(json!({
                "error": "User not found"
            }));
        }
    };

    let passkey_data = get_user_passkey_data(&user);
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
            return HttpResponse::Unauthorized().json(json!({
                "error": "Authentication required"
            }));
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "error": "Invalid user UUID"
            }));
        }
    };

    let credential_id = path.into_inner();
    let new_name = body.name.trim();

    if new_name.is_empty() || new_name.len() > 100 {
        return HttpResponse::BadRequest().json(json!({
            "error": "Passkey name must be between 1 and 100 characters"
        }));
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json(json!({
                "error": "Database connection error"
            }));
        }
    };

    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::NotFound().json(json!({
                "error": "User not found"
            }));
        }
    };

    let mut passkey_data = get_user_passkey_data(&user);

    match passkey_data.find_credential_mut(&credential_id) {
        Some(cred) => {
            cred.name = new_name.to_string();
        }
        None => {
            return HttpResponse::NotFound().json(json!({
                "error": "Passkey not found"
            }));
        }
    }

    if let Err(e) = save_user_passkey_data(&mut conn, &user_uuid, &passkey_data) {
        error!("Failed to rename passkey: {:?}", e);
        return HttpResponse::InternalServerError().json(json!({
            "error": "Failed to rename passkey"
        }));
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
            return HttpResponse::Unauthorized().json(json!({
                "error": "Authentication required"
            }));
        }
    };

    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "error": "Invalid user UUID"
            }));
        }
    };

    let credential_id = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => {
            return HttpResponse::InternalServerError().json(json!({
                "error": "Database connection error"
            }));
        }
    };

    let user = match repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::NotFound().json(json!({
                "error": "User not found"
            }));
        }
    };

    // Verify password
    let password_hash = match get_local_password_hash(&user_uuid, &mut conn) {
        Ok(hash) => hash,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "error": "Password verification not available for this account"
            }));
        }
    };

    let password_valid = bcrypt::verify(&body.password, &password_hash).unwrap_or(false);
    if !password_valid {
        return HttpResponse::Unauthorized().json(json!({
            "error": "Incorrect password"
        }));
    }

    let mut passkey_data = get_user_passkey_data(&user);

    if !passkey_data.remove_credential(&credential_id) {
        return HttpResponse::NotFound().json(json!({
            "error": "Passkey not found"
        }));
    }

    if let Err(e) = save_user_passkey_data(&mut conn, &user_uuid, &passkey_data) {
        error!("Failed to delete passkey: {:?}", e);
        return HttpResponse::InternalServerError().json(json!({
            "error": "Failed to delete passkey"
        }));
    }

    info!("Passkey {} deleted for user {}", credential_id, user_uuid);

    HttpResponse::Ok().json(json!({
        "success": true
    }))
}

/// Helper function to get password hash from user_auth_identities for local auth
fn get_local_password_hash(user_uuid: &Uuid, conn: &mut crate::db::DbConnection) -> Result<String, String> {
    use diesel::prelude::*;
    use crate::schema::user_auth_identities;

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
