use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
// Removed unused import: use diesel::prelude::*;
use querystring;
use reqwest;
use serde::Deserialize;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};
use urlencoding;

use crate::db::{DbConnection, Pool};
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::models::{AuthProvider, OAuthExchangeRequest, OAuthRequest, OAuthState};
use crate::utils::jwt::JWT_SECRET;
use diesel::prelude::*;
// Auth providers are now configured via environment variables
use crate::config_utils;
use crate::oidc;
use crate::repository::user_auth_identities;
use crate::services::search::{indexing_tasks, SearchService};
use std::sync::Arc;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/admin/auth/providers",
        web::get().to(crate::handlers::get_auth_providers),
    );
}

// Structure for OAuth logout requests
#[derive(Deserialize, Debug)]
pub struct OAuthLogoutRequest {
    pub provider_type: String,
    pub redirect_uri: String,
}

// Helper functions for environment-based auth providers
fn get_provider_by_type(provider_type: &str) -> Result<AuthProvider, diesel::result::Error> {
    match provider_type {
        "local" => Ok(AuthProvider::new(
            1,
            "Local".to_string(),
            "local".to_string(),
            true,
            true,
        )),
        "microsoft" => {
            if std::env::var("MICROSOFT_CLIENT_ID").is_ok()
                && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok()
                && std::env::var("MICROSOFT_TENANT_ID").is_ok()
            {
                Ok(AuthProvider::new(
                    2,
                    "Microsoft".to_string(),
                    "microsoft".to_string(),
                    true,
                    false,
                ))
            } else {
                Err(diesel::result::Error::NotFound)
            }
        }
        "oidc" => {
            if config_utils::is_oidc_enabled() {
                Ok(AuthProvider::new(
                    3,
                    crate::oidc::get_display_name_cached(),
                    "oidc".to_string(),
                    true,
                    false,
                ))
            } else {
                Err(diesel::result::Error::NotFound)
            }
        }
        _ => Err(diesel::result::Error::NotFound),
    }
}

// Get all authentication providers (admin only) - now returns environment-based config
pub async fn get_auth_providers(db_pool: web::Data<Pool>, req: HttpRequest) -> impl Responder {
    // Get database connection
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Extract claims from cookie auth middleware
    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return errors::unauthorized("Authentication required"),
    };

    if !crate::utils::rbac::is_platform_admin(&claims) {
        return errors::forbidden("Only administrators can manage authentication providers");
    }

    // Return hardcoded providers based on environment configuration
    let mut providers = vec![json!({
        "id": 1,
        "name": "Local",
        "provider_type": "local",
        "enabled": true,
        "is_default": true
    })];

    // Check if Microsoft is configured
    if std::env::var("MICROSOFT_CLIENT_ID").is_ok()
        && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok()
    {
        providers.push(json!({
            "id": 2,
            "name": "Microsoft",
            "provider_type": "microsoft",
            "enabled": true,
            "is_default": false
        }));
    }

    // Check if OIDC is configured
    if config_utils::is_oidc_enabled() {
        providers.push(json!({
            "id": 3,
            "name": crate::oidc::get_display_name_cached(),
            "provider_type": "oidc",
            "enabled": true,
            "is_default": false
        }));
    }

    HttpResponse::Ok().json(providers)
}

// Get enabled authentication providers (for login page) - now environment-based
pub async fn get_enabled_auth_providers(_db_pool: web::Data<Pool>) -> impl Responder {
    // Hosted mode trusts exactly one identity source: the platform OIDC.
    // Local password + Microsoft are never offered, so the login page
    // shows only single sign-on (and the frontend can auto-initiate it).
    let hosted =
        crate::middleware::DeploymentMode::current() == crate::middleware::DeploymentMode::Hosted;

    let mut providers = Vec::new();

    if !hosted {
        providers.push(json!({
            "id": 1,
            "provider_type": "local",
            "name": "Local",
            "is_default": true
        }));

        // Check if Microsoft is configured
        if std::env::var("MICROSOFT_CLIENT_ID").is_ok()
            && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok()
        {
            providers.push(json!({
                "id": 2,
                "provider_type": "microsoft",
                "name": "Microsoft",
                "is_default": false
            }));
        }
    }

    // Check if OIDC is configured. In hosted mode it is the default (and
    // only) provider, so the client can redirect straight to it.
    if config_utils::is_oidc_enabled() {
        providers.push(json!({
            "id": 3,
            "provider_type": "oidc",
            "name": crate::oidc::get_display_name_cached(),
            "is_default": hosted
        }));
    }

    HttpResponse::Ok().json(providers)
}

// Generate OAuth authorization URL
pub async fn oauth_authorize(
    db_pool: web::Data<Pool>,
    oauth_request: web::Json<OAuthRequest>,
    req: HttpRequest,
) -> impl Responder {
    // Get database connection
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Check if this is a user connection request
    let is_user_connection = oauth_request.user_connection.unwrap_or(false);

    let _user_uuid = if is_user_connection {
        // Extract claims from cookie auth middleware for user connection
        let claims = match req.extensions().get::<crate::models::Claims>() {
            Some(claims) => claims.clone(),
            None => return errors::unauthorized("Authentication required for user connection"),
        };

        Some(claims.sub)
    } else {
        None
    };

    let provider_type = &oauth_request.provider_type;

    // Get the provider by type
    let provider = match get_provider_by_type(provider_type) {
        Ok(p) => {
            if !p.enabled {
                return errors::bad_request(format!("{} authentication is not enabled", p.name));
            }
            p
        }
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return errors::not_found_msg(format!(
                    "{} authentication provider not found",
                    provider_type
                ));
            } else {
                error!(provider = %provider_type, error = ?e, "Failed to get auth provider");
                return errors::internal("Failed to retrieve authentication provider");
            }
        }
    };

    // Login no longer binds a workspace into the signed state. Hosted agent
    // login is workspace-agnostic (Model C: resolve an existing seat at the
    // callback, select a workspace post-login); self-hosted has one workspace
    // (the bootstrap), resolved at the callback. The Model B per-tenant binding
    // was retired with per-tenant federation.

    // For Microsoft Entra, generate the authorization URL
    if provider.provider_type == "microsoft" {
        // Get the provider configuration from environment variables
        let client_id = match config_utils::get_microsoft_client_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get client_id for Microsoft provider");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        let tenant_id = match config_utils::get_microsoft_tenant_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get tenant_id for Microsoft provider");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        let redirect_uri_config = match config_utils::get_microsoft_redirect_uri() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get redirect_uri for Microsoft provider");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        // Generate a JWT state token
        let (state, binding) = match create_oauth_state(
            "microsoft",
            oauth_request.redirect_uri.clone(),
            oauth_request.user_connection,
        ) {
            Ok(pair) => pair,
            Err(e) => {
                error!(error = %e, "Failed to create OAuth state token");
                return errors::internal("Failed to initiate authentication flow");
            }
        };

        // Create the authorization URL
        let auth_url = format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/authorize?client_id={client_id}&response_type=code&redirect_uri={redirect_uri_config}&response_mode=query&scope=User.Read&state={state}"
        );

        // Bind the flow to this user-agent (RFC 9700 §2.1).
        HttpResponse::Ok()
            .cookie(crate::utils::cookies::create_oauth_state_cookie(&binding))
            .json(json!({
                "auth_url": auth_url,
                "state": state
            }))
    } else if provider.provider_type == "oidc" {
        // Per-tenant OAuth callback (hosted mode): each tenant authenticates
        // on its own subdomain. Bind it into the signed state so the token
        // exchange in the callback presents the identical redirect_uri.
        let callback_redirect = oauth_callback_redirect_uri(&req);

        // Generate OIDC authorization URL with PKCE
        match oidc::generate_auth_url(
            oauth_request.redirect_uri.clone(),
            oauth_request.user_connection,
            callback_redirect.clone(),
        )
        .await
        {
            Ok((auth_url, auth_data)) => {
                // Create state JWT with PKCE verifier and nonce
                let (state, binding) = match create_oauth_state_with_oidc(
                    "oidc",
                    oauth_request.redirect_uri.clone(),
                    oauth_request.user_connection,
                    Some(auth_data.pkce_verifier),
                    Some(auth_data.nonce),
                    callback_redirect,
                ) {
                    Ok(pair) => pair,
                    Err(e) => {
                        error!(error = %e, "Failed to create OAuth state token for OIDC");
                        return errors::internal("Failed to initiate authentication flow");
                    }
                };

                // The auth_url from openidconnect library already includes a state
                // Replace with JWT state
                let auth_url_with_state = replace_state_in_url(&auth_url, &state);

                // Bind the flow to this user-agent (RFC 9700 §2.1).
                HttpResponse::Ok()
                    .cookie(crate::utils::cookies::create_oauth_state_cookie(&binding))
                    .json(json!({
                        "auth_url": auth_url_with_state,
                        "state": state
                    }))
            }
            Err(e) => {
                error!(error = %e, "Failed to generate OIDC authorization URL");
                errors::internal(format!("Failed to initiate OIDC authentication: {}", e))
            }
        }
    } else {
        errors::bad_request(format!(
            "{} authentication is not implemented",
            provider.name
        ))
    }
}

// Helper function to replace the state parameter in an OAuth URL
fn replace_state_in_url(url: &str, new_state: &str) -> String {
    // Parse the URL and replace the state parameter
    if let Some(query_start) = url.find('?') {
        let (base, query) = url.split_at(query_start + 1);
        let mut new_params: Vec<String> = Vec::new();
        let mut state_replaced = false;

        for param in query.split('&') {
            if param.starts_with("state=") {
                new_params.push(format!("state={}", urlencoding::encode(new_state)));
                state_replaced = true;
            } else {
                new_params.push(param.to_string());
            }
        }

        if !state_replaced {
            new_params.push(format!("state={}", urlencoding::encode(new_state)));
        }

        format!("{}{}", base, new_params.join("&"))
    } else {
        format!("{}?state={}", url, urlencoding::encode(new_state))
    }
}

// Handle OAuth callback and token exchange
pub async fn oauth_callback(
    db_pool: web::Data<Pool>,
    query: web::Query<OAuthExchangeRequest>,
    // Best-effort search reindex: optional so login doesn't hard-depend on
    // the search subsystem (and test apps need not wire it).
    search_service: Option<web::Data<Arc<SearchService>>>,
    request: actix_web::HttpRequest,
) -> impl Responder {
    // Get database connection
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Verify state parameter is present
    let state = match &query.state {
        Some(state) => state,
        None => return errors::bad_request("Missing state parameter"),
    };

    // Verify code parameter is present
    let code = match &query.code {
        Some(code) => code,
        None => return errors::bad_request("Missing authorization code"),
    };

    // Verify the state JWT
    let state_data = match verify_oauth_state(state) {
        Ok(data) => data,
        Err(e) => {
            warn!(error = %e, "Failed to verify OAuth state");
            return errors::bad_request("Invalid or expired state parameter");
        }
    };

    // RFC 9700 §2.1: confirm this callback completes in the SAME user-agent that
    // started the flow. The signed state has integrity but is not session-bound,
    // so without this an attacker who runs their own flow could CSRF the
    // resulting (code, state) onto a victim and swap them into the attacker's
    // account. The `oauth_state` cookie was set at initiation; an attacker can't
    // set it in the victim's browser.
    if let Some(expected) = &state_data.binding {
        let presented = request
            .cookie(crate::utils::cookies::OAUTH_STATE_COOKIE)
            .map(|c| c.value().to_string());
        let matches = presented
            .as_deref()
            .map(|p| constant_time_eq::constant_time_eq(p.as_bytes(), expected.as_bytes()))
            .unwrap_or(false);
        if !matches {
            warn!("OAuth callback rejected: state-binding cookie missing or mismatched");
            return errors::bad_request("Invalid or expired state parameter");
        }
    }
    // `binding == None` is a legacy in-flight state minted before this field
    // existed; allowed transitionally, and such states expire within the
    // 10-minute state lifetime, after which the binding check is mandatory.

    // Get the provider by type
    let provider_type = &state_data.provider_type;
    let provider = match get_provider_by_type(provider_type) {
        Ok(p) => {
            if !p.enabled {
                return errors::bad_request(format!("{} authentication is not enabled", p.name));
            }
            p
        }
        Err(e) => {
            error!(provider = %provider_type, error = ?e, "Failed to get provider in callback");
            return errors::internal("Authentication provider error");
        }
    };

    // Check if this is a user connection request (vs. a standard login)
    let is_connection = state_data.user_connection.unwrap_or(false);

    // Process based on provider type
    if provider.provider_type == "microsoft" {
        // Get the provider configuration from environment variables
        let _client_id = match config_utils::get_microsoft_client_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get client_id for Microsoft provider in callback");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        let _tenant_id = match config_utils::get_microsoft_tenant_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get tenant_id for Microsoft provider in callback");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        let _client_secret = match config_utils::get_microsoft_client_secret() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get client_secret for Microsoft provider in callback");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        let _redirect_uri_config = match config_utils::get_microsoft_redirect_uri() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get redirect_uri for Microsoft provider in callback");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        // Exchange authorization code for an access token
        let token_result = exchange_microsoft_code_for_token(&provider, code, &mut conn).await;

        match token_result {
            Ok((access_token, _refresh_token)) => {
                // Get user info from Microsoft
                let user_info = match get_microsoft_user_info(&access_token).await {
                    Ok(info) => info,
                    Err(e) => {
                        error!(error = ?e, "Failed to get Microsoft user info");
                        return errors::internal("Failed to get user information from Microsoft");
                    }
                };

                // Extract unique identifier for Microsoft (object ID)
                let provider_user_id = match user_info.get("id").and_then(|id| id.as_str()) {
                    Some(id) => id.to_string(),
                    None => {
                        error!("No ID found in Microsoft user info");
                        return errors::internal("Invalid user information from Microsoft");
                    }
                };

                // Extract email from user info
                let _email = match user_info
                    .get("mail")
                    .or_else(|| user_info.get("userPrincipalName"))
                    .and_then(|e| e.as_str())
                {
                    Some(email) => email.to_string(),
                    None => {
                        error!("No email found in Microsoft user info");
                        return errors::internal(
                            "Invalid user information from Microsoft (no email)",
                        );
                    }
                };

                // Handle account connection vs normal login
                if is_connection {
                    // This is a connection request - check if this identity is already linked to another account
                    match user_auth_identities::find_user_by_identity(
                        &provider.provider_type,
                        &provider_user_id,
                        &mut conn,
                    ) {
                        Ok(Some(existing_user_uuid)) => {
                            // Found an existing identity - verify the user still exists
                            match crate::repository::users::find_active_by_uuid(
                                &existing_user_uuid,
                                &mut conn,
                            ) {
                                Ok(_user) => {
                                    // User exists, can't reconnect
                                    return errors::bad_request("This Microsoft account is already connected to another user account");
                                }
                                Err(_) => {
                                    // User doesn't exist (orphaned record) - clean it up and proceed
                                    warn!(
                                        user_uuid = %existing_user_uuid,
                                        provider = %provider.provider_type,
                                        external_id = %provider_user_id,
                                        "Found orphaned auth identity, cleaning up"
                                    );
                                    // Delete the orphaned identity
                                    if let Err(e) = diesel::delete(
                                        crate::schema::user_auth_identities::table
                                            .filter(
                                                crate::schema::user_auth_identities::provider_type
                                                    .eq(&provider.provider_type),
                                            )
                                            .filter(
                                                crate::schema::user_auth_identities::external_id
                                                    .eq(&provider_user_id),
                                            ),
                                    )
                                    .execute(&mut conn)
                                    {
                                        error!(error = ?e, "Failed to clean up orphaned auth identity");
                                    }
                                    // Allow connection to proceed
                                }
                            }
                        }
                        Ok(None) => {
                            // Identity not yet linked - proceed
                        }
                        Err(e) => {
                            error!(error = ?e, "Failed to check existing identity");
                            return errors::internal("Failed to verify Microsoft account status");
                        }
                    }

                    // The connecting user's UUID is carried in the
                    // signed state's redirect_uri. oauth_connect sets it
                    // authoritatively from the authenticated session and
                    // strips any caller-supplied value, and the state JWT
                    // is integrity-protected, so this value is trusted.
                    let user_uuid_param = if state_data.redirect_uri.contains('?') {
                        let query_params = state_data.redirect_uri.split('?').nth(1).unwrap_or("");
                        let params = querystring::querify(query_params);
                        params
                            .iter()
                            .find(|(k, _)| *k == "user_uuid")
                            .map(|(_, v)| v.to_string())
                    } else {
                        None
                    };

                    let user_uuid = match user_uuid_param {
                        Some(uuid) => uuid,
                        None => {
                            // If not explicit in URL params, the user should be authenticated
                            // Get from request headers (auth token)
                            let redirect_parts: Vec<&str> =
                                state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];

                            // Error case - can't determine user
                            let error_url = format!(
                                "{}?auth_error={}",
                                redirect_path, "Could not determine user account for connection"
                            );

                            return HttpResponse::Found()
                                .append_header(("Location", error_url))
                                .finish();
                        }
                    };

                    // Add the identity to the user account
                    match add_oauth_identity_to_user(&user_uuid, &user_info, &provider, &mut conn)
                        .await
                    {
                        Ok(_) => {
                            // Successful connection
                            let redirect_parts: Vec<&str> =
                                state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            let success_url = format!("{redirect_path}?auth_success=true");

                            // Redirect to success page
                            HttpResponse::Found()
                                .append_header(("Location", success_url))
                                .finish()
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to connect account");

                            // Error connecting
                            let redirect_parts: Vec<&str> =
                                state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            let error_url = format!(
                                "{}?auth_error={}",
                                redirect_path,
                                urlencoding::encode(&format!("Failed to connect account: {e}"))
                            );

                            HttpResponse::Found()
                                .append_header(("Location", error_url))
                                .finish()
                        }
                    }
                } else {
                    // Regular login/signup flow
                    let user = match resolve_login_user(
                        &user_info,
                        &provider.provider_type,
                        &state_data,
                        &mut conn,
                    )
                    .await
                    {
                        Ok(user) => user,
                        Err(resp) => return resp,
                    };
                    info!(user_uuid = %user.uuid, "OAuth: Completing login");
                    // OAuth provisioning mints users with no search observer, so
                    // this reindex both indexes a first-login user and refreshes
                    // the workspace tags when a login grants membership in a new
                    // workspace.
                    if let Some(search_service) = &search_service {
                        indexing_tasks::spawn_reindex_user(
                            search_service.get_ref().clone(),
                            user.uuid,
                        );
                    }
                    crate::handlers::auth::complete_login_redirect(
                        user,
                        &request,
                        &mut conn,
                        &safe_post_login_location(&state_data.redirect_uri),
                    )
                }
            }
            Err(e) => {
                error!(error = ?e, "Failed to exchange code for token");
                errors::internal("Failed to authenticate with Microsoft")
            }
        }
    } else if provider.provider_type == "oidc" {
        // OIDC callback handling with PKCE
        let pkce_verifier = match &state_data.pkce_verifier {
            Some(v) => v.clone(),
            None => {
                error!("OIDC callback missing PKCE verifier in state");
                return errors::bad_request("Invalid authentication state (missing PKCE verifier)");
            }
        };

        let nonce = match &state_data.nonce {
            Some(n) => n.clone(),
            None => {
                error!("OIDC callback missing nonce in state");
                return errors::bad_request("Invalid authentication state (missing nonce)");
            }
        };

        // Create auth data for token exchange
        let auth_data = oidc::OidcAuthData {
            pkce_verifier,
            nonce,
        };

        // Exchange code for tokens and get user info. The redirect_uri must
        // match the one bound at initiation (the tenant's own callback in
        // hosted mode); it travels in the signed state, tamper-proof.
        match oidc::exchange_code(code, &auth_data, state_data.callback_redirect_uri.clone()).await
        {
            Ok(user_info) => {
                // Check if this is a user connection request (vs. a standard login)
                if is_connection {
                    // Connection request - check if this identity is already linked
                    match user_auth_identities::find_user_by_identity(
                        "oidc",
                        &user_info.sub,
                        &mut conn,
                    ) {
                        Ok(Some(existing_user_uuid)) => {
                            // Found an existing identity - verify the user still exists
                            match crate::repository::users::find_active_by_uuid(
                                &existing_user_uuid,
                                &mut conn,
                            ) {
                                Ok(_user) => {
                                    return errors::bad_request("This OIDC account is already connected to another user account");
                                }
                                Err(_) => {
                                    // User doesn't exist (orphaned record) - clean it up
                                    warn!(
                                        user_uuid = %existing_user_uuid,
                                        provider = "oidc",
                                        external_id = %user_info.sub,
                                        "Found orphaned auth identity, cleaning up"
                                    );
                                    if let Err(e) = diesel::delete(
                                        crate::schema::user_auth_identities::table
                                            .filter(
                                                crate::schema::user_auth_identities::provider_type
                                                    .eq("oidc"),
                                            )
                                            .filter(
                                                crate::schema::user_auth_identities::external_id
                                                    .eq(&user_info.sub),
                                            ),
                                    )
                                    .execute(&mut conn)
                                    {
                                        error!(error = ?e, "Failed to clean up orphaned auth identity");
                                    }
                                }
                            }
                        }
                        Ok(None) => {
                            // Identity not yet linked - proceed
                        }
                        Err(e) => {
                            error!(error = ?e, "Failed to check existing OIDC identity");
                            return errors::internal("Failed to verify OIDC account status");
                        }
                    }

                    // Extract user UUID from the redirect URL params
                    let user_uuid_param = if state_data.redirect_uri.contains('?') {
                        let query_params = state_data.redirect_uri.split('?').nth(1).unwrap_or("");
                        let params = querystring::querify(query_params);
                        params
                            .iter()
                            .find(|(key, _)| *key == "user_uuid")
                            .map(|(_, value)| value.to_string())
                    } else {
                        None
                    };

                    let user_uuid = match user_uuid_param {
                        Some(uuid_str) => match uuid::Uuid::parse_str(&uuid_str) {
                            Ok(uuid) => uuid,
                            Err(_) => {
                                return errors::bad_request("Invalid user UUID in redirect URI");
                            }
                        },
                        None => {
                            return errors::bad_request("Missing user UUID for account connection");
                        }
                    };

                    // Create the identity link
                    let oidc_config = match oidc::OidcConfig::from_env() {
                        Ok(c) => c,
                        Err(e) => {
                            error!(error = %e, "Failed to load OIDC config");
                            return errors::internal("OIDC configuration error");
                        }
                    };

                    let display_name = oidc::get_display_name(&user_info, &oidc_config);
                    let new_identity = crate::models::NewUserAuthIdentity {
                        user_uuid,
                        provider_type: "oidc".to_string(),
                        external_id: user_info.sub.clone(),
                        email: user_info.email.clone(),
                        metadata: Some(serde_json::json!({
                            "display_name": display_name
                        })),
                        password_hash: None,
                        workspace_id: None,
                    };
                    match user_auth_identities::create_identity(new_identity, &mut conn) {
                        Ok(_) => {
                            let redirect_parts: Vec<&str> =
                                state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            let success_url = format!("{redirect_path}?auth_success=true");

                            HttpResponse::Found()
                                .append_header(("Location", success_url))
                                .finish()
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to connect OIDC account");
                            let redirect_parts: Vec<&str> =
                                state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            let error_url = format!(
                                "{}?auth_error={}",
                                redirect_path,
                                urlencoding::encode(&format!("Failed to connect account: {e}"))
                            );

                            HttpResponse::Found()
                                .append_header(("Location", error_url))
                                .finish()
                        }
                    }
                } else {
                    // Regular login/signup flow
                    let oidc_user_info = serde_json::json!({
                        "id": user_info.sub,
                        "mail": user_info.email,
                        "displayName": user_info.name.clone().or_else(|| user_info.preferred_username.clone()),
                        "givenName": user_info.given_name,
                        "surname": user_info.family_name,
                    });

                    let user = match resolve_login_user(
                        &oidc_user_info,
                        &oidc_identity_issuer(),
                        &state_data,
                        &mut conn,
                    )
                    .await
                    {
                        Ok(user) => user,
                        Err(resp) => return resp,
                    };
                    info!(user_uuid = %user.uuid, "OIDC: Completing login");
                    // Index / refresh the user's search doc with current
                    // workspace memberships (see above).
                    if let Some(search_service) = &search_service {
                        indexing_tasks::spawn_reindex_user(
                            search_service.get_ref().clone(),
                            user.uuid,
                        );
                    }
                    crate::handlers::auth::complete_login_redirect(
                        user,
                        &request,
                        &mut conn,
                        &safe_post_login_location(&state_data.redirect_uri),
                    )
                }
            }
            Err(e) => {
                error!(error = %e, "Failed to exchange OIDC code for token");
                errors::internal(format!("Failed to authenticate with OIDC provider: {}", e))
            }
        }
    } else {
        errors::bad_request(format!(
            "{} authentication callback is not implemented",
            provider.name
        ))
    }
}

// Handle OAuth logout request
pub async fn oauth_logout(
    db_pool: web::Data<Pool>,
    logout_request: web::Json<OAuthLogoutRequest>,
) -> impl Responder {
    // Get database connection
    let _conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let provider_type = &logout_request.provider_type;

    // Get the provider by type
    let provider = match get_provider_by_type(provider_type) {
        Ok(p) => {
            if !p.enabled {
                return errors::bad_request(format!("{} authentication is not enabled", p.name));
            }
            p
        }
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return errors::not_found_msg(format!(
                    "{} authentication provider not found",
                    provider_type
                ));
            } else {
                error!(provider = %provider_type, error = ?e, "Failed to get auth provider for logout");
                return errors::internal("Failed to retrieve authentication provider");
            }
        }
    };

    // Generate logout URL based on provider type
    match provider.provider_type.as_str() {
        "microsoft" => {
            // For Microsoft Entra, generate the logout URL
            let tenant_id = match config_utils::get_microsoft_tenant_id() {
                Ok(val) => val,
                Err(e) => {
                    error!(error = ?e, "Failed to get tenant_id for Microsoft provider logout");
                    return errors::internal(format!(
                        "Microsoft authentication is not properly configured: {}",
                        e
                    ));
                }
            };

            // URL encode the redirect URI
            let encoded_redirect = urlencoding::encode(&logout_request.redirect_uri);

            // Create the logout URL
            let logout_url = format!(
                "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/logout?post_logout_redirect_uri={encoded_redirect}"
            );

            HttpResponse::Ok().json(json!({
                "logout_url": logout_url
            }))
        }
        "oidc" => {
            // For OIDC providers, use the RP-initiated logout flow
            // Generate the logout URL with the redirect URI
            match oidc::generate_logout_url(
                &logout_request.redirect_uri,
                None, // id_token_hint - could be passed from frontend if available
                None, // state - could be used for CSRF protection
            )
            .await
            {
                Some(logout_url) => HttpResponse::Ok().json(json!({
                    "logout_url": logout_url
                })),
                None => {
                    // OIDC provider doesn't support logout or isn't configured
                    HttpResponse::Ok().json(json!({
                        "logout_url": null,
                        "message": "OIDC provider does not support RP-initiated logout or end_session_endpoint is not configured"
                    }))
                }
            }
        }
        _ => errors::bad_request(format!("{} logout is not implemented", provider.name)),
    }
}

// JWT State Management

// Create a signed state JWT for OAuth flow
/// Returns `(state_jwt, binding)`. The caller MUST set the `binding` in the
/// `oauth_state` cookie (RFC 9700 §2.1) so the callback can confirm the flow
/// completes in the same user-agent that started it.
fn create_oauth_state(
    provider_type: &str,
    redirect_uri: Option<String>,
    user_connection: Option<bool>,
) -> Result<(String, String), String> {
    create_oauth_state_with_oidc(
        provider_type,
        redirect_uri,
        user_connection,
        None,
        None,
        None,
    )
}

// Create a signed state JWT for OAuth/OIDC flow with optional PKCE and nonce.
// Returns `(state_jwt, binding)` — see `create_oauth_state`.
#[allow(clippy::too_many_arguments)]
fn create_oauth_state_with_oidc(
    provider_type: &str,
    redirect_uri: Option<String>,
    user_connection: Option<bool>,
    pkce_verifier: Option<String>,
    nonce: Option<String>,
    callback_redirect_uri: Option<String>,
) -> Result<(String, String), String> {
    // Get the JWT secret from environment or configuration
    let secret = JWT_SECRET.clone();

    // Create expiration timestamp (10 minutes from now)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as usize;
    let exp = now + (10 * 60); // 10 minutes

    // One 128-bit random value serves as both the (legacy) state value and the
    // user-agent binding. It rides inside the signed, tamper-proof JWT AND is
    // set in the `oauth_state` cookie by the caller; the callback requires the
    // two to match.
    let state = format!("{:x}", rand::random::<u128>());
    let claims = OAuthState {
        state: state.clone(),
        redirect_uri: redirect_uri.unwrap_or_else(|| "/".to_string()),
        provider_type: provider_type.to_string(),
        exp,
        user_connection,
        pkce_verifier,
        nonce,
        callback_redirect_uri,
        binding: Some(state.clone()),
    };

    // Create the token
    match jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    ) {
        Ok(token) => Ok((token, state)),
        Err(e) => Err(format!("Failed to create state JWT: {e}")),
    }
}

// Verify and decode the state JWT
fn verify_oauth_state(token: &str) -> Result<OAuthState, String> {
    // Get the JWT secret from environment or configuration
    let secret = JWT_SECRET.clone();

    // Verify the token
    match jsonwebtoken::decode::<OAuthState>(
        token,
        &jsonwebtoken::DecodingKey::from_secret(secret.as_bytes()),
        &jsonwebtoken::Validation::default(),
    ) {
        Ok(data) => Ok(data.claims),
        Err(e) => Err(format!("Invalid state JWT: {e}")),
    }
}

/// Send an HTTP request with bounded retries on transport-level
/// failures only (connection refused, DNS error, request-build
/// error, timeout). Once the server has responded with anything —
/// even a 5xx — we return that response without retrying. For
/// OAuth this is the only safe behaviour: the auth code is
/// consumed atomically when the server validates it, and a retry
/// after partial failure would either waste the code or, worse,
/// double-spend it if the original call actually succeeded server-
/// side.
///
/// Backoff is short and capped: 250ms, 500ms, 1000ms before the
/// final attempt. Total wait of ~1.75s is small compared with the
/// alternative of asking the user to re-trigger the entire OAuth
/// flow on a transient blip.
async fn retry_transport<F>(
    _client: &reqwest::Client,
    max_attempts: u32,
    mut build: F,
) -> Result<reqwest::Response, reqwest::Error>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        match build().send().await {
            Ok(res) => return Ok(res),
            Err(e) if attempt >= max_attempts => return Err(e),
            Err(e) if !(e.is_connect() || e.is_timeout() || e.is_request()) => {
                // Other reqwest errors (eg. body decode) shouldn't
                // happen before send() returns, but if they do
                // they're not worth retrying.
                return Err(e);
            }
            Err(e) => {
                let delay_ms = 250u64.saturating_mul(1 << (attempt - 1));
                warn!(
                    attempt,
                    delay_ms,
                    error = %e,
                    "Transient HTTP failure during auth flow; retrying",
                );
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }
    }
}

// Helper function to exchange Microsoft code for token
async fn exchange_microsoft_code_for_token(
    _provider: &AuthProvider,
    code: &str,
    _conn: &mut DbConnection,
) -> Result<(String, Option<String>), String> {
    // Get provider configuration from environment variables
    let client_id = config_utils::get_microsoft_client_id()
        .map_err(|e| format!("Failed to get client_id: {e}"))?;

    let tenant_id = config_utils::get_microsoft_tenant_id()
        .map_err(|e| format!("Failed to get tenant_id: {e}"))?;

    let client_secret = config_utils::get_microsoft_client_secret()
        .map_err(|e| format!("Failed to get client_secret: {e}"))?;

    let redirect_uri_config = config_utils::get_microsoft_redirect_uri()
        .map_err(|e| format!("Failed to get redirect_uri: {e}"))?;

    // Prepare the token request
    let params = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("code", code),
        ("redirect_uri", redirect_uri_config.as_str()),
        ("grant_type", "authorization_code"),
    ];

    // Make the token request, retrying on transport-level failures
    // only. We never retry once we've received a response: OAuth
    // codes are single-use, and if Microsoft validated the code
    // before the network failed we'd get `invalid_grant` on retry
    // anyway. Safer to bail and let the user re-trigger the flow
    // than to silently consume a code twice.
    let client = reqwest::Client::new();
    let token_url = format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token");
    let res = retry_transport(&client, 3, || client.post(&token_url).form(&params))
        .await
        .map_err(|e| format!("Failed to send token request: {e}"))?;

    // Parse the response
    let token_response = match res.json::<serde_json::Value>().await {
        Ok(json) => json,
        Err(e) => return Err(format!("Failed to parse token response: {e}")),
    };

    // Extract tokens
    let access_token = match token_response.get("access_token") {
        Some(token) => match token.as_str() {
            Some(t) => t.to_string(),
            None => return Err("Invalid access token format".to_string()),
        },
        None => return Err("No access token in response".to_string()),
    };

    let refresh_token = token_response
        .get("refresh_token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());

    Ok((access_token, refresh_token))
}

// Helper function to get user info from Microsoft Graph API.
// Safe to retry transport failures here since GET /me is
// idempotent and the access token is valid for ~1 hour.
async fn get_microsoft_user_info(access_token: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let res = retry_transport(&client, 3, || {
        client
            .get("https://graph.microsoft.com/v1.0/me")
            .header("Authorization", format!("Bearer {access_token}"))
    })
    .await
    .map_err(|e| format!("Failed to get user info: {e}"))?;

    match res.json::<serde_json::Value>().await {
        Ok(json) => Ok(json),
        Err(e) => Err(format!("Failed to parse user info response: {e}")),
    }
}

/// The OAuth callback `redirect_uri` for THIS request, or `None` to use the
/// statically configured `OIDC_REDIRECT_URI`.
///
/// In hosted mode one app serves every tenant subdomain, so a single static
/// redirect can't work: each tenant authenticates on its own origin. We
/// build the callback from the request `Host` (the same header the tenant
/// middleware resolves the workspace from) over `https` (hosted is always
/// TLS-terminated). The control plane registers each tenant's callback on
/// the Hydra client, and Hydra rejects any redirect_uri not in that set, so
/// a spoofed `Host` can't redirect a code anywhere unregistered. In
/// self-hosted mode there is one origin and one configured redirect, so we
/// return `None`.
fn oauth_callback_redirect_uri(request: &HttpRequest) -> Option<String> {
    let host = request
        .headers()
        .get(actix_web::http::header::HOST)
        .and_then(|h| h.to_str().ok());
    callback_redirect_for(host, crate::middleware::DeploymentMode::current())
}

/// Pure decision behind [`oauth_callback_redirect_uri`], split out for unit
/// tests. Strips any port and lowercases, matching the tenant middleware's
/// host normalisation so the redirect host is exactly the resolved tenant.
fn callback_redirect_for(
    host: Option<&str>,
    mode: crate::middleware::DeploymentMode,
) -> Option<String> {
    if mode != crate::middleware::DeploymentMode::Hosted {
        return None;
    }
    let host = host?;
    let host = host.split(':').next().unwrap_or(host).trim().to_lowercase();
    if host.is_empty() {
        return None;
    }
    Some(format!("https://{host}/api/auth/oauth/callback"))
}

/// Build the standard auth-error redirect: bounce to the login redirect target
/// with an `?auth_error=<message>` the frontend surfaces. Strips any existing
/// query so the error is the only param.
fn auth_error_redirect(redirect_uri: &str, message: &str) -> HttpResponse {
    let redirect_path = redirect_uri.split('?').next().unwrap_or("/");
    let error_url = format!(
        "{}?auth_error={}",
        redirect_path,
        urlencoding::encode(message)
    );
    HttpResponse::Found()
        .append_header(("Location", error_url))
        .finish()
}

// Helper function to find or create a user from OAuth profile
/// Lazy OIDC user provisioning, called from the OAuth callback when
/// a user logs in. Extracts identity claims from the provider's
/// user_info JSON (MS Graph shape) and delegates to the shared
/// `services::oauth_provisioning::find_or_create_projected_user` —
/// same core code path the M5 eager-projection endpoint uses, so
/// lazy and eager calls converge to the same row.
///
/// OAuth-created users land as workspace `member` regardless of the
/// global role (which stays `User`). Owner / admin grants come from
/// the eager-projection path during workspace provisioning, not
/// from first-login.
/// Sanitise the post-login redirect target before bouncing the browser to
/// it. `redirect_uri` is client-supplied at initiation (carried through the
/// signed state), so an unchecked absolute URL would be an open redirect we
/// could be phished through. Only same-origin relative paths are honoured;
/// anything else (absolute URL, protocol-relative `//host`, or empty) falls
/// back to the app root.
fn safe_post_login_location(redirect_uri: &str) -> String {
    if redirect_uri.starts_with('/') && !redirect_uri.starts_with("//") {
        redirect_uri.to_string()
    } else {
        "/".to_string()
    }
}

/// Identity key (`user_auth_identities.provider_type`) for an OIDC login.
///
/// In hosted mode this must be the platform issuer (`OIDC_ISSUER_URL`,
/// e.g. `https://api.nosdesk.dev`), because the control plane projects
/// each seat under `(iss, sub)`. Resolving a login by the literal
/// `"oidc"` would miss that seat entirely. In discovery mode the token's
/// `iss` is verified to equal `OIDC_ISSUER_URL`, so the configured value
/// is the authoritative issuer.
///
/// In self-hosted mode we keep the legacy `"oidc"` key so identities
/// written before this change (which all used `"oidc"`) still resolve.
///
/// The issuer is used verbatim (no normalisation): it must byte-match the
/// `iss` the control plane stored at projection time, which is the same
/// string operators set as `OIDC_ISSUER_URL`.
// ===== Native (mobile app) OIDC login =====

/// Public OIDC config the native app needs to run its OWN Authorization-Code +
/// PKCE flow against the IdP: the issuer (for discovery), the app's public
/// client id, and the scopes. The app fetches this from whichever server it's
/// connected to, so it targets the right IdP (staging vs prod) automatically.
pub async fn native_oidc_config() -> HttpResponse {
    if !config_utils::is_oidc_enabled() {
        return errors::not_found("OIDC is not enabled");
    }
    let issuer = match config_utils::get_oidc_issuer_url() {
        Ok(i) => i,
        Err(_) => return errors::not_found("Native OIDC requires OIDC_ISSUER_URL"),
    };
    HttpResponse::Ok().json(json!({
        "issuer": issuer,
        "client_id": config_utils::get_oidc_native_client_id(),
        "scopes": config_utils::get_oidc_scopes(),
    }))
}

#[derive(Deserialize)]
pub struct NativeOidcLoginRequest {
    pub id_token: String,
}

/// Native-app OIDC login: the app has already run the Authorization-Code + PKCE
/// flow against the IdP (as a public client) and posts the resulting ID token.
/// We verify it (signature / issuer / audience / expiry), resolve the existing
/// seat by `(issuer, sub)` — the same hosted resolution the web callback uses —
/// and mint a normal product session, delivered as body tokens (bearer mode).
pub async fn native_oidc_login(
    db_pool: web::Data<Pool>,
    body: web::Json<NativeOidcLoginRequest>,
    request: HttpRequest,
) -> impl Responder {
    let native_client_id = config_utils::get_oidc_native_client_id();
    let user_info = match oidc::verify_native_id_token(&body.id_token, &native_client_id).await {
        Ok(u) => u,
        Err(e) => {
            warn!(error = %e, "native OIDC login: ID token rejected");
            return errors::unauthorized("Invalid ID token");
        }
    };

    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let iss = oidc_identity_issuer();
    let email = user_info.email.clone().unwrap_or_default();
    let email_verified = user_info.email_verified.unwrap_or(false);

    let user = match crate::services::oauth_provisioning::resolve_user_by_identity_or_email(
        &mut conn,
        &iss,
        &user_info.sub,
        // Global login identity (central IdP), not workspace-scoped.
        None,
        &email,
        email_verified,
        &None,
        &None,
    ) {
        Ok(Some(user)) => user,
        Ok(None) => {
            warn!(%email, "native OIDC login denied: no seat for this identity");
            return errors::forbidden(CENTRAL_LOGIN_NO_SEAT_MSG);
        }
        Err(e) => {
            error!(error = %e, "native OIDC login: seat resolution failed");
            return errors::internal("Failed to authenticate user");
        }
    };

    match crate::handlers::auth::establish_login_session(user, &request, &mut conn) {
        Ok((response, tokens)) => {
            crate::handlers::auth::build_auth_response(&request, response, &tokens)
        }
        Err(resp) => resp,
    }
}

fn oidc_identity_issuer() -> String {
    issuer_for_identity(
        crate::middleware::DeploymentMode::current(),
        config_utils::get_oidc_issuer_url().ok(),
    )
}

/// Pure decision behind [`oidc_identity_issuer`], split out for unit tests
/// (the process-wide `DeploymentMode::current()` is cached, so the mode is
/// passed in).
fn issuer_for_identity(
    mode: crate::middleware::DeploymentMode,
    configured_issuer: Option<String>,
) -> String {
    use crate::middleware::DeploymentMode;
    match mode {
        DeploymentMode::Hosted => configured_issuer
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "oidc".to_string()),
        DeploymentMode::SelfHosted => "oidc".to_string(),
    }
}

/// Extract the login email from an OAuth/OIDC `user_info` blob. MS Graph uses
/// `mail` for cloud accounts and `userPrincipalName` for hybrid AD; the OIDC
/// path normalises its claims into the same `mail` field.
fn oauth_user_email(user_info: &serde_json::Value) -> Result<String, String> {
    user_info
        .get("mail")
        .or_else(|| user_info.get("userPrincipalName"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "No email in user info".to_string())
}

/// Extract the provider subject (OIDC `sub` / MS Graph object id) from a
/// `user_info` blob; stored in `user_auth_identities.external_id`.
fn oauth_user_sub(user_info: &serde_json::Value) -> Result<String, String> {
    user_info
        .get("id")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| "No id in user info".to_string())
}

/// Resolve the local user for a completing OAuth/OIDC login.
///
/// - **Model C (selection mode):** resolve an EXISTING seat only, by identity
///   then email-link. An unknown identity is denied, because the seat and its
///   workspace membership are provisioned upstream by the control plane, not at
///   the login origin. No workspace is involved, and the resolve touches only
///   non-audited, non-RLS identity tables, so it needs no workspace pin.
/// - **Self-hosted:** one workspace (the bootstrap); find-or-create the user as
///   a member of it. Reaching here in HOSTED mode means selection mode is off on
///   a hosted deployment, unsupported since per-tenant federation was retired
///   (hosted login is Model C). Fail closed.
///
/// Returns the resolved user, or an `HttpResponse` (auth-error redirect / 500)
/// the caller should return directly.
async fn resolve_login_user(
    user_info: &serde_json::Value,
    iss: &str,
    state: &OAuthState,
    conn: &mut DbConnection,
) -> Result<crate::models::User, HttpResponse> {
    let email_verified = oauth_email_verified(&state.provider_type, user_info);
    if crate::middleware::workspace_context::selection_resolution_enabled() {
        return resolve_existing_seat_user(user_info, iss, state, email_verified, conn);
    }
    let workspace_id = match crate::middleware::DeploymentMode::current() {
        crate::middleware::DeploymentMode::SelfHosted => crate::sync::actor::BOOTSTRAP_WORKSPACE_ID,
        crate::middleware::DeploymentMode::Hosted => {
            error!("hosted OAuth login reached without selection mode; per-tenant federation is retired");
            return Err(errors::internal("Authentication is misconfigured"));
        }
    };
    find_or_create_oauth_user(user_info, iss, email_verified, conn, workspace_id)
        .await
        .map_err(|e| {
            error!(error = ?e, "Failed to find or create user during login");
            errors::internal("Failed to authenticate user")
        })
}

/// Whether the provider vouches that the login email is verified, gating the
/// email-fallback account link (see `resolve_user_by_identity_or_email`).
///
/// - Microsoft/Entra: the user authenticated against the directory that owns
///   the address, so it is provider-verified. Graph `/me` carries no
///   `email_verified` claim, so trust the directory.
/// - OIDC: trust only an explicit `email_verified == true`. Absent or false is
///   NOT verified, so the email-fallback link is refused.
fn oauth_email_verified(provider_type: &str, user_info: &serde_json::Value) -> bool {
    match provider_type {
        "microsoft" => true,
        _ => user_info
            .get("email_verified")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    }
}

/// Model C central login: resolve an existing seat without ever creating a user
/// or granting membership from the login origin. Unknown identity is denied.
fn resolve_existing_seat_user(
    user_info: &serde_json::Value,
    iss: &str,
    state: &OAuthState,
    email_verified: bool,
    conn: &mut DbConnection,
) -> Result<crate::models::User, HttpResponse> {
    let email = oauth_user_email(user_info).map_err(|e| {
        error!(error = %e, "Central-origin login: cannot read email from user_info");
        auth_error_redirect(&state.redirect_uri, CENTRAL_LOGIN_NO_SEAT_MSG)
    })?;
    let sub = oauth_user_sub(user_info).map_err(|e| {
        error!(error = %e, "Central-origin login: cannot read subject from user_info");
        auth_error_redirect(&state.redirect_uri, CENTRAL_LOGIN_NO_SEAT_MSG)
    })?;
    // Resolve-only: pass no metadata / password_hash since we never create here.
    match crate::services::oauth_provisioning::resolve_user_by_identity_or_email(
        conn,
        iss,
        &sub,
        // Global OIDC login identity (central-origin agent), not workspace-scoped.
        None,
        &email,
        email_verified,
        &None,
        &None,
    ) {
        Ok(Some(user)) => Ok(user),
        Ok(None) => {
            warn!(%email, "Central-origin login denied: no seat for this identity");
            Err(auth_error_redirect(
                &state.redirect_uri,
                CENTRAL_LOGIN_NO_SEAT_MSG,
            ))
        }
        Err(e) => {
            error!(error = %e, "Seat resolution failed during central-origin login");
            Err(errors::internal("Failed to authenticate user"))
        }
    }
}

/// Shown when a central-origin login resolves a valid identity that holds no
/// workspace seat (membership is provisioned upstream, not at login).
const CENTRAL_LOGIN_NO_SEAT_MSG: &str =
    "No workspace access for this account. Ask your administrator to invite you.";

async fn find_or_create_oauth_user(
    user_info: &serde_json::Value,
    // Identity issuer for `user_auth_identities.provider_type`. For
    // Microsoft this is `"microsoft"`; for OIDC it must be the real
    // issuer (the platform `iss`) so a login resolves the seat the
    // control plane projected under `(iss, sub)`, not the literal
    // string `"oidc"`. See `oidc_identity_issuer`.
    iss: &str,
    email_verified: bool,
    conn: &mut DbConnection,
    workspace_id: i32,
) -> Result<crate::models::User, String> {
    use crate::services::oauth_provisioning::{find_or_create_projected_user, ProjectedUserInput};

    let email = oauth_user_email(user_info)?;

    let name = match user_info.get("displayName") {
        Some(name) => match name.as_str() {
            Some(n) => n.to_string(),
            None => return Err("Invalid name format".to_string()),
        },
        None => return Err("No name in user info".to_string()),
    };

    // MS Graph object id maps onto OIDC `sub`, stored in
    // `user_auth_identities.external_id`.
    let provider_user_id = oauth_user_sub(user_info)?;

    // Random password lands on the identity row so the legacy
    // password-fallback path doesn't see a NULL hash. Eager path
    // omits this; users provisioned ahead of first login don't
    // need a fallback secret since they're going to authenticate
    // via OIDC anyway.
    let random_password = format!("{:x}", rand::random::<u128>());
    let password_hash = crate::utils::auth::hash_password(&random_password)
        .map_err(|e| format!("Failed to hash password: {e}"))?;

    let input = ProjectedUserInput {
        iss: iss.to_string(),
        sub: provider_user_id,
        // OIDC login is a global platform identity, not workspace-scoped.
        identity_workspace_id: None,
        email,
        email_verified,
        name: Some(name),
        role: "member".to_string(),
        workspace_id,
        password_hash: Some(password_hash),
        metadata: Some(user_info.clone()),
    };

    // find_or_create_projected_user runs several writes (the `users`
    // insert is audited) with no internal transaction, so wrap it in
    // the actor context here: that opens one transaction, pins
    // `app.workspace_id` for the audit trigger, and rolls every write
    // back together on failure. System attribution is correct for both
    // the create case (no session yet) and the IdP-driven refresh of
    // an existing user.
    let actor =
        crate::sync::actor::ActorContext::system("oauth_callback").with_workspace(workspace_id);
    crate::sync::session::with_actor_context_str(conn, &actor, |c| {
        find_or_create_projected_user(c, input).map(|outcome| outcome.into_user())
    })
}

// Helper function to add an OAuth identity to an existing user
async fn add_oauth_identity_to_user(
    user_uuid: &str,
    user_info: &serde_json::Value,
    provider: &AuthProvider,
    conn: &mut DbConnection,
) -> Result<(), String> {
    // Parse UUID from string
    let parsed_uuid = match crate::utils::parse_uuid(user_uuid) {
        Ok(uuid) => uuid,
        Err(e) => return Err(format!("Invalid UUID format: {e}")),
    };

    // First find the user by UUID
    let user = match crate::repository::users::find_active_by_uuid(&parsed_uuid, conn) {
        Ok(user) => user,
        Err(e) => return Err(format!("User not found: {e:?}")),
    };

    // Extract unique identifier for Microsoft (object ID)
    let external_id = match user_info.get("id") {
        Some(id) => match id.as_str() {
            Some(i) => i.to_string(),
            None => return Err("Invalid id format".to_string()),
        },
        None => return Err("No id in user info".to_string()),
    };

    // Extract email from user info (optional)
    let email = user_info
        .get("mail")
        .or_else(|| user_info.get("userPrincipalName"))
        .and_then(|e| e.as_str())
        .map(|e| e.to_string());

    // Create a new identity for the user
    let new_identity = crate::models::NewUserAuthIdentity {
        user_uuid: user.uuid,
        provider_type: provider.provider_type.clone(),
        external_id,
        email: email.clone(),
        metadata: Some(user_info.clone()),
        password_hash: None, // No password for OAuth identities
        workspace_id: None,
    };

    // Save the identity to the database
    match crate::repository::user_auth_identities::create_identity(new_identity, conn) {
        Ok(_) => {}
        Err(e) => return Err(format!("Failed to create auth identity: {e:?}")),
    }

    // Also add the OAuth provider email to user_emails table if
    // present. Only insert on explicit NotFound — see the matched
    // sister branch above for why "any error == not found" is the
    // wrong check (it can either duplicate rows on transient errors
    // or mask real DB problems).
    if let Some(email_address) = email {
        match crate::repository::user_emails::find_user_by_any_email(conn, &email_address) {
            Err(diesel::result::Error::NotFound) => {
                let new_email = crate::models::NewUserEmail {
                    user_uuid: user.uuid,
                    email: email_address.to_lowercase(),
                    email_type: "work".to_string(),
                    is_primary: false,
                    is_verified: true,
                    source: Some(provider.provider_type.clone()),
                };
                if let Err(e) = diesel::insert_into(crate::schema::user_emails::table)
                    .values(&new_email)
                    .execute(conn)
                {
                    error!(
                        provider = %provider.provider_type,
                        user_uuid = %user.uuid,
                        error = ?e,
                        "Failed to insert OAuth email into user_emails; user can log in but email-based flows will not find them",
                    );
                }
            }
            Ok(_) => {}
            Err(e) => {
                error!(
                    provider = %provider.provider_type,
                    user_uuid = %user.uuid,
                    error = ?e,
                    "Failed to look up OAuth email in user_emails; skipping insert",
                );
            }
        }
    }

    Ok(())
}

/// Remove every occurrence of `key` from the query string of a
/// (possibly relative) URI, preserving the path and any other params.
/// Used to strip a caller-supplied `user_uuid` from an OAuth connect
/// redirect before we set it authoritatively from the authenticated
/// session. Trusting a client-provided `user_uuid` is an account-
/// linking takeover vector. See security-audit-2026-06.
fn strip_query_param(uri: &str, key: &str) -> String {
    match uri.split_once('?') {
        None => uri.to_string(),
        Some((path, query)) => {
            let kept: Vec<&str> = query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .filter(|pair| pair.split('=').next().unwrap_or("") != key)
                .collect();
            if kept.is_empty() {
                path.to_string()
            } else {
                format!("{path}?{}", kept.join("&"))
            }
        }
    }
}

// Direct endpoint for connecting a new authentication method to an existing user
pub async fn oauth_connect(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    oauth_request: web::Json<OAuthRequest>,
) -> impl Responder {
    // Get database connection
    let mut conn = match helpers::db_conn(&db_pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    // Verify user exists
    let user_uuid = match crate::utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID in token"),
    };

    let user = match crate::repository::users::find_active_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            error!(user_uuid = %user_uuid, error = ?e, "Failed to find user by UUID");
            return errors::not_found_msg("User not found");
        }
    };

    let provider_type = &oauth_request.provider_type;

    // Get the provider by type
    let provider = match get_provider_by_type(provider_type) {
        Ok(p) => {
            if !p.enabled {
                return errors::bad_request(format!("{} authentication is not enabled", p.name));
            }
            p
        }
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return errors::not_found_msg(format!(
                    "{} authentication provider not found",
                    provider_type
                ));
            } else {
                error!(provider = %provider_type, error = ?e, "Failed to get auth provider for connect");
                return errors::internal("Failed to retrieve authentication provider");
            }
        }
    };

    // For Microsoft Entra, generate the authorization URL
    if provider.provider_type == "microsoft" {
        // Get the provider configuration from environment variables
        let client_id = match config_utils::get_microsoft_client_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get client_id for Microsoft provider connect");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        let tenant_id = match config_utils::get_microsoft_tenant_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get tenant_id for Microsoft provider connect");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        let redirect_uri_config = match config_utils::get_microsoft_redirect_uri() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get redirect_uri for Microsoft provider connect");
                return errors::internal(format!(
                    "Microsoft authentication is not properly configured: {}",
                    e
                ));
            }
        };

        // Prepare the redirect URI. The connecting user's UUID is set
        // authoritatively from the authenticated session; any caller-
        // supplied user_uuid is stripped first. The callback links the
        // IdP identity to whatever UUID the signed state carries, so
        // trusting a client-provided value here would let an attacker
        // link their own IdP account to a victim's UUID (account
        // takeover). The state JWT is signed, so a server-set UUID
        // cannot be tampered with at the callback. See
        // security-audit-2026-06.
        let base_redirect_uri = strip_query_param(
            &oauth_request
                .redirect_uri
                .clone()
                .unwrap_or_else(|| "/profile/settings".to_string()),
            "user_uuid",
        );
        let separator = if base_redirect_uri.contains('?') {
            "&"
        } else {
            "?"
        };
        let actual_redirect_uri = format!("{base_redirect_uri}{separator}user_uuid={}", user.uuid);

        // Generate a JWT state token with user_connection=true. Connecting
        // an identity to an already-authenticated account doesn't
        // provision workspace membership, so no workspace is bound.
        let (state, binding) = match create_oauth_state(
            &provider.provider_type,
            Some(actual_redirect_uri),
            Some(true),
        ) {
            Ok(pair) => pair,
            Err(e) => {
                error!(error = %e, "Failed to create OAuth state token for connect");
                return errors::internal("Failed to initiate authentication flow");
            }
        };

        // Create the authorization URL
        let auth_url = format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/authorize?client_id={client_id}&response_type=code&redirect_uri={redirect_uri_config}&response_mode=query&scope=User.Read&state={state}"
        );

        // Bind the flow to this user-agent (RFC 9700 §2.1).
        HttpResponse::Ok()
            .cookie(crate::utils::cookies::create_oauth_state_cookie(&binding))
            .json(json!({
                "auth_url": auth_url,
                "state": state
            }))
    } else {
        errors::bad_request(format!(
            "{} authentication is not implemented",
            provider.name
        ))
    }
}

#[cfg(test)]
mod oauth_state_binding_tests {
    use super::{create_oauth_state, verify_oauth_state};

    /// The state JWT helpers read the process-global `JWT_SECRET` lazy-static,
    /// which panics if unset. Ensure it's present before first access so these
    /// tests pass in isolation, not just when another test set it first.
    fn ensure_jwt_secret() {
        if std::env::var("JWT_SECRET").is_err() {
            std::env::set_var("JWT_SECRET", "test-jwt-secret-for-oauth-state-binding");
        }
    }

    #[test]
    fn state_embeds_binding_and_round_trips() {
        ensure_jwt_secret();
        let (token, binding) = create_oauth_state("oidc", None, None).unwrap();
        assert!(
            !binding.is_empty(),
            "binding must be a non-empty random value"
        );
        let state = verify_oauth_state(&token).unwrap();
        // The cookie value (binding) the caller sets must equal what the signed
        // state carries, so the callback's constant-time compare can match them.
        assert_eq!(state.binding.as_deref(), Some(binding.as_str()));
    }

    #[test]
    fn two_flows_get_distinct_bindings() {
        ensure_jwt_secret();
        let (_, b1) = create_oauth_state("oidc", None, None).unwrap();
        let (_, b2) = create_oauth_state("oidc", None, None).unwrap();
        assert_ne!(b1, b2, "each flow must bind to a fresh random value");
    }
}

#[cfg(test)]
mod connect_redirect_tests {
    use super::strip_query_param;

    #[test]
    fn strips_caller_supplied_user_uuid() {
        // No query: untouched.
        assert_eq!(
            strip_query_param("/profile/settings", "user_uuid"),
            "/profile/settings"
        );
        // Sole param: query removed entirely.
        assert_eq!(strip_query_param("/p?user_uuid=victim", "user_uuid"), "/p");
        // Mixed params: only user_uuid dropped, order preserved.
        assert_eq!(
            strip_query_param("/p?foo=1&user_uuid=victim", "user_uuid"),
            "/p?foo=1"
        );
        assert_eq!(
            strip_query_param("/p?user_uuid=victim&foo=1", "user_uuid"),
            "/p?foo=1"
        );
        // Repeated keys: all removed.
        assert_eq!(
            strip_query_param("/p?user_uuid=a&user_uuid=b", "user_uuid"),
            "/p"
        );
        // Unrelated query: preserved.
        assert_eq!(strip_query_param("/p?foo=1", "user_uuid"), "/p?foo=1");
    }
}

#[cfg(test)]
mod login_resolution_tests {
    use super::{auth_error_redirect, oauth_email_verified, oauth_user_email, oauth_user_sub};
    use serde_json::json;

    #[test]
    fn email_reads_mail_then_upn() {
        // OIDC-normalised claims + MS Graph cloud accounts use `mail`.
        assert_eq!(
            oauth_user_email(&json!({"mail": "a@x.com"})).unwrap(),
            "a@x.com"
        );
        // Hybrid AD falls back to userPrincipalName.
        assert_eq!(
            oauth_user_email(&json!({"userPrincipalName": "b@x.com"})).unwrap(),
            "b@x.com"
        );
        // `mail` wins when both present.
        assert_eq!(
            oauth_user_email(&json!({"mail": "a@x.com", "userPrincipalName": "b@x.com"})).unwrap(),
            "a@x.com"
        );
        // Neither present -> error (never silently log in without an email).
        assert!(oauth_user_email(&json!({"id": "1"})).is_err());
    }

    #[test]
    fn sub_reads_id() {
        assert_eq!(oauth_user_sub(&json!({"id": "abc"})).unwrap(), "abc");
        assert!(oauth_user_sub(&json!({"mail": "a@x.com"})).is_err());
    }

    #[test]
    fn email_verified_trusts_microsoft_and_explicit_oidc_claim() {
        // Entra directory emails are provider-verified; Graph sends no claim.
        assert!(oauth_email_verified("microsoft", &json!({})));
        // OIDC: only an explicit true counts as verified.
        assert!(oauth_email_verified(
            "oidc",
            &json!({"email_verified": true})
        ));
        assert!(!oauth_email_verified(
            "oidc",
            &json!({"email_verified": false})
        ));
        // Absent or non-bool claim is NOT verified (refuses the email link).
        assert!(!oauth_email_verified("oidc", &json!({})));
        assert!(!oauth_email_verified(
            "oidc",
            &json!({"email_verified": "true"})
        ));
    }

    #[test]
    fn auth_error_redirect_strips_query_and_encodes_message() {
        let resp = auth_error_redirect("/login?next=/x", "No seat here");
        assert_eq!(resp.status(), actix_web::http::StatusCode::FOUND);
        let loc = resp.headers().get("location").unwrap().to_str().unwrap();
        // Existing query stripped; message percent-encoded onto the clean path.
        assert_eq!(loc, "/login?auth_error=No%20seat%20here");
    }
}

#[cfg(test)]
mod hosted_auth_tests {
    use super::{callback_redirect_for, issuer_for_identity, safe_post_login_location};
    use crate::middleware::DeploymentMode;

    #[test]
    fn post_login_location_allows_relative_blocks_absolute() {
        // Same-origin relative paths pass through.
        assert_eq!(safe_post_login_location("/"), "/");
        assert_eq!(safe_post_login_location("/tickets/42"), "/tickets/42");
        // Absolute, protocol-relative, scheme, or empty fall back to root
        // (no open redirect after authentication).
        assert_eq!(safe_post_login_location("https://evil.example"), "/");
        assert_eq!(safe_post_login_location("//evil.example"), "/");
        assert_eq!(safe_post_login_location("javascript:alert(1)"), "/");
        assert_eq!(safe_post_login_location(""), "/");
    }

    #[test]
    fn hosted_identity_uses_configured_issuer_verbatim() {
        // The issuer must byte-match what the control plane stored as
        // `(iss, sub)`; no trailing-slash normalisation.
        assert_eq!(
            issuer_for_identity(
                DeploymentMode::Hosted,
                Some("https://api.nosdesk.dev".to_string())
            ),
            "https://api.nosdesk.dev"
        );
        assert_eq!(
            issuer_for_identity(
                DeploymentMode::Hosted,
                Some("https://api.nosdesk.dev/".to_string())
            ),
            "https://api.nosdesk.dev/"
        );
    }

    #[test]
    fn hosted_identity_falls_back_when_issuer_absent_or_empty() {
        assert_eq!(issuer_for_identity(DeploymentMode::Hosted, None), "oidc");
        assert_eq!(
            issuer_for_identity(DeploymentMode::Hosted, Some(String::new())),
            "oidc"
        );
    }

    #[test]
    fn self_hosted_identity_keeps_legacy_oidc_key() {
        // Preserves resolution of identities written before this change.
        assert_eq!(
            issuer_for_identity(
                DeploymentMode::SelfHosted,
                Some("https://idp.example".to_string())
            ),
            "oidc"
        );
    }

    #[test]
    fn callback_redirect_is_per_tenant_in_hosted_mode() {
        assert_eq!(
            callback_redirect_for(Some("mercury.nosdesk.dev"), DeploymentMode::Hosted),
            Some("https://mercury.nosdesk.dev/api/auth/oauth/callback".to_string())
        );
        // Port stripped, host lowercased.
        assert_eq!(
            callback_redirect_for(Some("Venus.Nosdesk.Dev:8080"), DeploymentMode::Hosted),
            Some("https://venus.nosdesk.dev/api/auth/oauth/callback".to_string())
        );
    }

    #[test]
    fn callback_redirect_is_none_when_unusable_or_self_hosted() {
        // Self-hosted uses the statically configured OIDC_REDIRECT_URI.
        assert_eq!(
            callback_redirect_for(Some("mercury.nosdesk.dev"), DeploymentMode::SelfHosted),
            None
        );
        // Hosted but no/empty Host: fail to None rather than emit a bad URI.
        assert_eq!(callback_redirect_for(None, DeploymentMode::Hosted), None);
        assert_eq!(
            callback_redirect_for(Some(""), DeploymentMode::Hosted),
            None
        );
    }
}
