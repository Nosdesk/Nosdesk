use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
// Removed unused import: use diesel::prelude::*;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use urlencoding;
use querystring;
use serde::Deserialize;
use reqwest;
use tracing::{error, info, warn};

use crate::db::{Pool, DbConnection};
use crate::utils::jwt::JWT_SECRET;
use crate::models::{
    OAuthRequest, OAuthExchangeRequest,
    OAuthState, AuthProvider
};
use diesel::prelude::*;
// Auth providers are now configured via environment variables
use crate::repository::user_auth_identities;
use crate::config_utils;
use crate::utils;
use crate::oidc;

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
                && std::env::var("MICROSOFT_TENANT_ID").is_ok() {
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
        },
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
        },
        _ => Err(diesel::result::Error::NotFound),
    }
}

#[allow(dead_code)]
fn get_provider_by_id(provider_id: i32) -> Result<AuthProvider, diesel::result::Error> {
    match provider_id {
        1 => Ok(AuthProvider::new(
            1,
            "Local".to_string(),
            "local".to_string(),
            true,
            true,
        )),
        2 => {
            if std::env::var("MICROSOFT_CLIENT_ID").is_ok()
                && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok()
                && std::env::var("MICROSOFT_TENANT_ID").is_ok() {
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
        },
        3 => {
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
        },
        _ => Err(diesel::result::Error::NotFound),
    }
}

// Get all authentication providers (admin only) - now returns environment-based config
pub async fn get_auth_providers(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
) -> impl Responder {
    // Get database connection
    let conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Check if the user is an admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Only administrators can manage authentication providers"
        }));
    }

    // Return hardcoded providers based on environment configuration
    let mut providers = vec![
        json!({
            "id": 1,
            "name": "Local",
            "provider_type": "local",
            "enabled": true,
            "is_default": true
        })
    ];

    // Check if Microsoft is configured
    if std::env::var("MICROSOFT_CLIENT_ID").is_ok() && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok() {
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
pub async fn get_enabled_auth_providers(
    _db_pool: web::Data<Pool>,
) -> impl Responder {
    // Return enabled providers based on environment configuration
    let mut providers = vec![
        json!({
            "id": 1,
            "provider_type": "local",
            "name": "Local",
            "is_default": true
        })
    ];

    // Check if Microsoft is configured
    if std::env::var("MICROSOFT_CLIENT_ID").is_ok() && std::env::var("MICROSOFT_CLIENT_SECRET").is_ok() {
        providers.push(json!({
            "id": 2,
            "provider_type": "microsoft",
            "name": "Microsoft",
            "is_default": false
        }));
    }

    // Check if OIDC is configured
    if config_utils::is_oidc_enabled() {
        providers.push(json!({
            "id": 3,
            "provider_type": "oidc",
            "name": crate::oidc::get_display_name_cached(),
            "is_default": false
        }));
    }

    HttpResponse::Ok().json(providers)
}



// Update an authentication provider (admin only) - now returns not implemented
#[allow(dead_code)]
pub async fn update_auth_provider(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    _path: web::Path<i32>,
    _provider_data: web::Json<serde_json::Value>,
) -> impl Responder {
    // Get database connection
    let _conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Check if the user is an admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Only administrators can manage authentication providers"
        }));
    }

    // Auth providers are now configured via environment variables
    HttpResponse::BadRequest().json(json!({
        "status": "error",
        "message": "Authentication providers are now configured via environment variables. Please update your .env file instead."
    }))
}





// Generate OAuth authorization URL
pub async fn oauth_authorize(
    db_pool: web::Data<Pool>,
    oauth_request: web::Json<OAuthRequest>,
    req: HttpRequest,
) -> impl Responder {
    // Get database connection
    let conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Check if this is a user connection request
    let is_user_connection = oauth_request.user_connection.unwrap_or(false);

    let _user_uuid = if is_user_connection {
        // Extract claims from cookie auth middleware for user connection
        let claims = match req.extensions().get::<crate::models::Claims>() {
            Some(claims) => claims.clone(),
            None => return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Authentication required for user connection"
            })),
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
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": format!("{} authentication is not enabled", p.name)
                }));
            }
            p
        },
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return HttpResponse::NotFound().json(json!({
                    "status": "error",
                    "message": format!("{} authentication provider not found", provider_type)
                }));
            } else {
                error!(provider = %provider_type, error = ?e, "Failed to get auth provider");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to retrieve authentication provider"
                }));
            }
        }
    };

    // For Microsoft Entra, generate the authorization URL
    if provider.provider_type == "microsoft" {
        // Get the provider configuration from environment variables
        let client_id = match config_utils::get_microsoft_client_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get client_id for Microsoft provider");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        let tenant_id = match config_utils::get_microsoft_tenant_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get tenant_id for Microsoft provider");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        let redirect_uri_config = match config_utils::get_microsoft_redirect_uri() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get redirect_uri for Microsoft provider");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        // Generate a JWT state token
        let state = match create_oauth_state("microsoft", oauth_request.redirect_uri.clone(), oauth_request.user_connection) {
            Ok(token) => token,
            Err(e) => {
                error!(error = %e, "Failed to create OAuth state token");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to initiate authentication flow"
                }));
            }
        };
        
        // Create the authorization URL
        let auth_url = format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/authorize?client_id={client_id}&response_type=code&redirect_uri={redirect_uri_config}&response_mode=query&scope=User.Read&state={state}"
        );

        HttpResponse::Ok().json(json!({
            "auth_url": auth_url,
            "state": state
        }))
    } else if provider.provider_type == "oidc" {
        // Generate OIDC authorization URL with PKCE
        match oidc::generate_auth_url(oauth_request.redirect_uri.clone(), oauth_request.user_connection).await {
            Ok((auth_url, auth_data)) => {
                // Create state JWT with PKCE verifier and nonce
                let state = match create_oauth_state_with_oidc(
                    "oidc",
                    oauth_request.redirect_uri.clone(),
                    oauth_request.user_connection,
                    Some(auth_data.pkce_verifier),
                    Some(auth_data.nonce),
                ) {
                    Ok(token) => token,
                    Err(e) => {
                        error!(error = %e, "Failed to create OAuth state token for OIDC");
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Failed to initiate authentication flow"
                        }));
                    }
                };

                // The auth_url from openidconnect library already includes a state
                // Replace with JWT state
                let auth_url_with_state = replace_state_in_url(&auth_url, &state);

                HttpResponse::Ok().json(json!({
                    "auth_url": auth_url_with_state,
                    "state": state
                }))
            },
            Err(e) => {
                error!(error = %e, "Failed to generate OIDC authorization URL");
                HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Failed to initiate OIDC authentication: {}", e)
                }))
            }
        }
    } else {
        HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": format!("{} authentication is not implemented", provider.name)
        }))
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
    request: actix_web::HttpRequest,
) -> impl Responder {
    // Get database connection
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Verify state parameter is present
    let state = match &query.state {
        Some(state) => state,
        None => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Missing state parameter"
        })),
    };

    // Verify code parameter is present
    let code = match &query.code {
        Some(code) => code,
        None => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Missing authorization code"
        })),
    };

    // Verify the state JWT
    let state_data = match verify_oauth_state(state) {
        Ok(data) => data,
        Err(e) => {
            warn!(error = %e, "Failed to verify OAuth state");
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid or expired state parameter"
            }));
        }
    };

    // Get the provider by type
    let provider_type = &state_data.provider_type;
    let provider = match get_provider_by_type(provider_type) {
        Ok(p) => {
            if !p.enabled {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": format!("{} authentication is not enabled", p.name)
                }));
            }
            p
        },
        Err(e) => {
            error!(provider = %provider_type, error = ?e, "Failed to get provider in callback");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Authentication provider error"
            }));
        }
    };

    // Check if this is a user connection request (vs. a standard login)
    let is_connection = state_data.user_connection.unwrap_or(false);

    // Process based on provider type
    if provider.provider_type == "microsoft" {
        // Get the provider configuration from environment variables
        let client_id = match config_utils::get_microsoft_client_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get client_id for Microsoft provider in callback");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        let tenant_id = match config_utils::get_microsoft_tenant_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get tenant_id for Microsoft provider in callback");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        let client_secret = match config_utils::get_microsoft_client_secret() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get client_secret for Microsoft provider in callback");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        let redirect_uri_config = match config_utils::get_microsoft_redirect_uri() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get redirect_uri for Microsoft provider in callback");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
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
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Failed to get user information from Microsoft"
                        }));
                    }
                };

                // Extract unique identifier for Microsoft (object ID)
                let provider_user_id = match user_info.get("id").and_then(|id| id.as_str()) {
                    Some(id) => id.to_string(),
                    None => {
                        error!("No ID found in Microsoft user info");
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Invalid user information from Microsoft"
                        }));
                    }
                };

                // Extract email from user info
                let email = match user_info.get("mail")
                    .or_else(|| user_info.get("userPrincipalName"))
                    .and_then(|e| e.as_str()) {
                    Some(email) => email.to_string(),
                    None => {
                        error!("No email found in Microsoft user info");
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Invalid user information from Microsoft (no email)"
                        }));
                    }
                };

                // Handle account connection vs normal login
                if is_connection {
                    // This is a connection request - check if this identity is already linked to another account
                    match user_auth_identities::find_user_by_identity(&provider.provider_type, &provider_user_id, &mut conn) {
                        Ok(Some(existing_user_uuid)) => {
                            // Found an existing identity - verify the user still exists
                            match crate::repository::users::get_user_by_uuid(&existing_user_uuid, &mut conn) {
                                Ok(_user) => {
                                    // User exists, can't reconnect
                                    return HttpResponse::BadRequest().json(json!({
                                        "status": "error",
                                        "message": "This Microsoft account is already connected to another user account"
                                    }));
                                },
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
                                            .filter(crate::schema::user_auth_identities::provider_type.eq(&provider.provider_type))
                                            .filter(crate::schema::user_auth_identities::external_id.eq(&provider_user_id))
                                    ).execute(&mut conn) {
                                        error!(error = ?e, "Failed to clean up orphaned auth identity");
                                    }
                                    // Allow connection to proceed
                                }
                            }
                        },
                        Ok(None) => {
                            // Identity not yet linked - proceed
                        },
                        Err(e) => {
                            error!(error = ?e, "Failed to check existing identity");
                            return HttpResponse::InternalServerError().json(json!({
                                "status": "error",
                                "message": "Failed to verify Microsoft account status"
                            }));
                        }
                    }
                    
                    // Extract user UUID from the redirect URL params if present
                    // For added security, we get the UUID from the query string in redirect_uri if provided
                    let user_uuid_param = if state_data.redirect_uri.contains('?') {
                        let query_params = state_data.redirect_uri.split('?').nth(1).unwrap_or("");
                        let params = querystring::querify(query_params);
                        params.iter()
                            .find(|(k, _)| *k == "user_uuid")
                            .map(|(_, v)| v.to_string())
                    } else {
                        None
                    };
                    
                    // Get user UUID from SessionStorage on client side if not in URL
                    // SessionStorage on the frontend should contain the authRedirect with the user's path
                    
                    // Add the identity to the user - for now we use a hardcoded false value
                    // but in a real implementation you'd get the UUID from auth token
                    let user_uuid = match user_uuid_param {
                        Some(uuid) => uuid,
                        None => {
                            // If not explicit in URL params, the user should be authenticated
                            // Get from request headers (auth token)
                            let redirect_parts: Vec<&str> = state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            
                            // Error case - can't determine user
                            let error_url = format!("{}?auth_error={}", 
                                redirect_path,
                                "Could not determine user account for connection");
                            
                            return HttpResponse::Found()
                                .append_header(("Location", error_url))
                                .finish();
                        }
                    };
                    
                    // Add the identity to the user account
                    match add_oauth_identity_to_user(&user_uuid, &user_info, &provider, &mut conn).await {
                        Ok(_) => {
                            // Successful connection
                            let redirect_parts: Vec<&str> = state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            let success_url = format!("{redirect_path}?auth_success=true");

                            // Redirect to success page
                            HttpResponse::Found()
                                .append_header(("Location", success_url))
                                .finish()
                        },
                        Err(e) => {
                            error!(error = %e, "Failed to connect account");

                            // Error connecting
                            let redirect_parts: Vec<&str> = state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            let error_url = format!("{}?auth_error={}",
                                redirect_path,
                                urlencoding::encode(&format!("Failed to connect account: {e}")));

                            HttpResponse::Found()
                                .append_header(("Location", error_url))
                                .finish()
                        }
                    }
                } else {
                    // Regular login/signup flow
                    // Find or create user based on OAuth identity
                    let user_result = find_or_create_oauth_user(&user_info, &provider, &mut conn).await;

                    match user_result {
                        Ok(user) => {
                            // Store user UUID before moving user into create_login_response
                            let user_uuid = user.uuid;

                            // Create standard login response with httpOnly cookies
                            match crate::utils::jwt::helpers::create_login_response(user, &mut conn) {
                                Ok((response, tokens)) => {
                                    info!(user_uuid = %user_uuid, "OAuth: Created login response, creating session");

                                    // Create session record after successful OAuth login
                                    if let Err(e) = crate::handlers::auth::create_session_record(&user_uuid, &tokens.access_token, &request, &mut conn).await {
                                        error!(user_uuid = %user_uuid, error = %e, "OAuth: Failed to create session record");
                                        // Return error instead of continuing without session
                                        return HttpResponse::InternalServerError().json(json!({
                                            "status": "error",
                                            "message": "Failed to create authentication session"
                                        }));
                                    }

                                    info!(user_uuid = %user_uuid, "OAuth: Session created successfully");

                                    // Set httpOnly cookies for tokens
                                    HttpResponse::Ok()
                                        .cookie(crate::utils::cookies::create_access_token_cookie(&tokens.access_token))
                                        .cookie(crate::utils::cookies::create_refresh_token_cookie(&tokens.refresh_token))
                                        .cookie(crate::utils::cookies::create_csrf_token_cookie(&tokens.csrf_token))
                                        .json(response)
                                },
                                Err(error_response) => error_response,
                            }
                        },
                        Err(e) => {
                            error!(error = ?e, "Failed to find or create user");
                            HttpResponse::InternalServerError().json(json!({
                                "status": "error",
                                "message": "Failed to authenticate user"
                            }))
                        }
                    }
                }
            },
            Err(e) => {
                error!(error = ?e, "Failed to exchange code for token");
                HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to authenticate with Microsoft"
                }))
            }
        }
    } else if provider.provider_type == "oidc" {
        // OIDC callback handling with PKCE
        let pkce_verifier = match &state_data.pkce_verifier {
            Some(v) => v.clone(),
            None => {
                error!("OIDC callback missing PKCE verifier in state");
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "Invalid authentication state (missing PKCE verifier)"
                }));
            }
        };

        let nonce = match &state_data.nonce {
            Some(n) => n.clone(),
            None => {
                error!("OIDC callback missing nonce in state");
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "Invalid authentication state (missing nonce)"
                }));
            }
        };

        // Create auth data for token exchange
        let auth_data = oidc::OidcAuthData {
            pkce_verifier,
            nonce,
        };

        // Exchange code for tokens and get user info
        match oidc::exchange_code(code, &auth_data).await {
            Ok(user_info) => {
                // Check if this is a user connection request (vs. a standard login)
                if is_connection {
                    // Connection request - check if this identity is already linked
                    match user_auth_identities::find_user_by_identity("oidc", &user_info.sub, &mut conn) {
                        Ok(Some(existing_user_uuid)) => {
                            // Found an existing identity - verify the user still exists
                            match crate::repository::users::get_user_by_uuid(&existing_user_uuid, &mut conn) {
                                Ok(_user) => {
                                    return HttpResponse::BadRequest().json(json!({
                                        "status": "error",
                                        "message": "This OIDC account is already connected to another user account"
                                    }));
                                },
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
                                            .filter(crate::schema::user_auth_identities::provider_type.eq("oidc"))
                                            .filter(crate::schema::user_auth_identities::external_id.eq(&user_info.sub))
                                    ).execute(&mut conn) {
                                        error!(error = ?e, "Failed to clean up orphaned auth identity");
                                    }
                                }
                            }
                        },
                        Ok(None) => {
                            // Identity not yet linked - proceed
                        },
                        Err(e) => {
                            error!(error = ?e, "Failed to check existing OIDC identity");
                            return HttpResponse::InternalServerError().json(json!({
                                "status": "error",
                                "message": "Failed to verify OIDC account status"
                            }));
                        }
                    }

                    // Extract user UUID from the redirect URL params
                    let user_uuid_param = if state_data.redirect_uri.contains('?') {
                        let query_params = state_data.redirect_uri.split('?').nth(1).unwrap_or("");
                        let params = querystring::querify(query_params);
                        params.iter()
                            .find(|(key, _)| *key == "user_uuid")
                            .map(|(_, value)| value.to_string())
                    } else {
                        None
                    };

                    let user_uuid = match user_uuid_param {
                        Some(uuid_str) => {
                            match uuid::Uuid::parse_str(&uuid_str) {
                                Ok(uuid) => uuid,
                                Err(_) => {
                                    return HttpResponse::BadRequest().json(json!({
                                        "status": "error",
                                        "message": "Invalid user UUID in redirect URI"
                                    }));
                                }
                            }
                        },
                        None => {
                            return HttpResponse::BadRequest().json(json!({
                                "status": "error",
                                "message": "Missing user UUID for account connection"
                            }));
                        }
                    };

                    // Create the identity link
                    let oidc_config = match oidc::OidcConfig::from_env() {
                        Ok(c) => c,
                        Err(e) => {
                            error!(error = %e, "Failed to load OIDC config");
                            return HttpResponse::InternalServerError().json(json!({
                                "status": "error",
                                "message": "OIDC configuration error"
                            }));
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
                    };
                    match user_auth_identities::create_identity(new_identity, &mut conn) {
                        Ok(_) => {
                            let redirect_parts: Vec<&str> = state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            let success_url = format!("{redirect_path}?auth_success=true");

                            HttpResponse::Found()
                                .append_header(("Location", success_url))
                                .finish()
                        },
                        Err(e) => {
                            error!(error = %e, "Failed to connect OIDC account");
                            let redirect_parts: Vec<&str> = state_data.redirect_uri.split('?').collect();
                            let redirect_path = redirect_parts[0];
                            let error_url = format!("{}?auth_error={}",
                                redirect_path,
                                urlencoding::encode(&format!("Failed to connect account: {e}")));

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

                    let user_result = find_or_create_oauth_user(&oidc_user_info, &provider, &mut conn).await;

                    match user_result {
                        Ok(user) => {
                            let user_uuid = user.uuid;

                            match crate::utils::jwt::helpers::create_login_response(user, &mut conn) {
                                Ok((response, tokens)) => {
                                    info!(user_uuid = %user_uuid, "OIDC: Created login response, creating session");

                                    if let Err(e) = crate::handlers::auth::create_session_record(&user_uuid, &tokens.access_token, &request, &mut conn).await {
                                        error!(user_uuid = %user_uuid, error = %e, "OIDC: Failed to create session record");
                                        return HttpResponse::InternalServerError().json(json!({
                                            "status": "error",
                                            "message": "Failed to create authentication session"
                                        }));
                                    }

                                    info!(user_uuid = %user_uuid, "OIDC: Session created successfully");

                                    HttpResponse::Ok()
                                        .cookie(crate::utils::cookies::create_access_token_cookie(&tokens.access_token))
                                        .cookie(crate::utils::cookies::create_refresh_token_cookie(&tokens.refresh_token))
                                        .cookie(crate::utils::cookies::create_csrf_token_cookie(&tokens.csrf_token))
                                        .json(response)
                                },
                                Err(error_response) => error_response,
                            }
                        },
                        Err(e) => {
                            error!(error = ?e, "Failed to find or create user from OIDC");
                            HttpResponse::InternalServerError().json(json!({
                                "status": "error",
                                "message": "Failed to authenticate user"
                            }))
                        }
                    }
                }
            },
            Err(e) => {
                error!(error = %e, "Failed to exchange OIDC code for token");
                HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Failed to authenticate with OIDC provider: {}", e)
                }))
            }
        }
    } else {
        HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": format!("{} authentication callback is not implemented", provider.name)
        }))
    }
}

// Handle OAuth logout request
pub async fn oauth_logout(
    db_pool: web::Data<Pool>,
    logout_request: web::Json<OAuthLogoutRequest>,
) -> impl Responder {
    // Get database connection
    let conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    let provider_type = &logout_request.provider_type;

    // Get the provider by type
    let provider = match get_provider_by_type(provider_type) {
        Ok(p) => {
            if !p.enabled {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": format!("{} authentication is not enabled", p.name)
                }));
            }
            p
        },
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return HttpResponse::NotFound().json(json!({
                    "status": "error",
                    "message": format!("{} authentication provider not found", provider_type)
                }));
            } else {
                error!(provider = %provider_type, error = ?e, "Failed to get auth provider for logout");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to retrieve authentication provider"
                }));
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
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": format!("Microsoft authentication is not properly configured: {}", e)
                    }));
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
        },
        "oidc" => {
            // For OIDC providers, use the RP-initiated logout flow
            // Generate the logout URL with the redirect URI
            match oidc::generate_logout_url(
                &logout_request.redirect_uri,
                None, // id_token_hint - could be passed from frontend if available
                None, // state - could be used for CSRF protection
            ).await {
                Some(logout_url) => {
                    HttpResponse::Ok().json(json!({
                        "logout_url": logout_url
                    }))
                },
                None => {
                    // OIDC provider doesn't support logout or isn't configured
                    HttpResponse::Ok().json(json!({
                        "logout_url": null,
                        "message": "OIDC provider does not support RP-initiated logout or end_session_endpoint is not configured"
                    }))
                }
            }
        },
        _ => {
            HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": format!("{} logout is not implemented", provider.name)
            }))
        }
    }
}

// JWT State Management

// Create a signed state JWT for OAuth flow
fn create_oauth_state(provider_type: &str, redirect_uri: Option<String>, user_connection: Option<bool>) -> Result<String, String> {
    create_oauth_state_with_oidc(provider_type, redirect_uri, user_connection, None, None)
}

// Create a signed state JWT for OAuth/OIDC flow with optional PKCE and nonce
fn create_oauth_state_with_oidc(
    provider_type: &str,
    redirect_uri: Option<String>,
    user_connection: Option<bool>,
    pkce_verifier: Option<String>,
    nonce: Option<String>,
) -> Result<String, String> {
    // Get the JWT secret from environment or configuration
    let secret = JWT_SECRET.clone();

    // Create expiration timestamp (10 minutes from now)
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as usize;
    let exp = now + (10 * 60); // 10 minutes

    // Create claims
    let state = format!("{:x}", rand::random::<u128>());
    let claims = OAuthState {
        state: state.clone(),
        redirect_uri: redirect_uri.unwrap_or_else(|| "/".to_string()),
        provider_type: provider_type.to_string(),
        exp,
        user_connection,
        pkce_verifier,
        nonce,
    };

    // Create the token
    match jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    ) {
        Ok(token) => Ok(token),
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

// Helper function to exchange Microsoft code for token
async fn exchange_microsoft_code_for_token(
    provider: &AuthProvider,
    code: &str,
    conn: &mut DbConnection,
) -> Result<(String, Option<String>), String> {
    // Get provider configuration from environment variables
    let client_id = match config_utils::get_microsoft_client_id() {
        Ok(val) => val,
        Err(e) => return Err(format!("Failed to get client_id: {e}")),
    };
    
    let tenant_id = match config_utils::get_microsoft_tenant_id() {
        Ok(val) => val,
        Err(e) => return Err(format!("Failed to get tenant_id: {e}")),
    };
    
    let client_secret = match config_utils::get_microsoft_client_secret() {
        Ok(val) => val,
        Err(e) => return Err(format!("Failed to get client_secret: {e}")),
    };
    
    let redirect_uri_config = match config_utils::get_microsoft_redirect_uri() {
        Ok(val) => val,
        Err(e) => return Err(format!("Failed to get redirect_uri: {e}")),
    };
    
    // Prepare the token request
    let params = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("code", code),
        ("redirect_uri", redirect_uri_config.as_str()),
        ("grant_type", "authorization_code"),
    ];
    
    // Make the token request
    let client = reqwest::Client::new();
    let res = match client
        .post(format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"))
        .form(&params)
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => return Err(format!("Failed to send token request: {e}")),
    };
    
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
    
    let refresh_token = token_response.get("refresh_token").and_then(|t| t.as_str()).map(|s| s.to_string());
    
    Ok((access_token, refresh_token))
}

// Helper function to get user info from Microsoft Graph API
async fn get_microsoft_user_info(access_token: &str) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let res = match client
        .get("https://graph.microsoft.com/v1.0/me")
        .header("Authorization", format!("Bearer {access_token}"))
        .send()
        .await
    {
        Ok(res) => res,
        Err(e) => return Err(format!("Failed to get user info: {e}")),
    };
    
    match res.json::<serde_json::Value>().await {
        Ok(json) => Ok(json),
        Err(e) => Err(format!("Failed to parse user info response: {e}")),
    }
}

// Helper function to find or create a user from OAuth profile
async fn find_or_create_oauth_user(
    user_info: &serde_json::Value,
    provider: &AuthProvider,
    conn: &mut DbConnection,
) -> Result<crate::models::User, String> {
    // Extract email from user info
    let email = match user_info.get("mail").or_else(|| user_info.get("userPrincipalName")) {
        Some(email) => match email.as_str() {
            Some(e) => e.to_string(),
            None => return Err("Invalid email format".to_string()),
        },
        None => return Err("No email in user info".to_string()),
    };
    
    // Extract name from user info
    let name = match user_info.get("displayName") {
        Some(name) => match name.as_str() {
            Some(n) => n.to_string(),
            None => return Err("Invalid name format".to_string()),
        },
        None => return Err("No name in user info".to_string()),
    };
    
    // Extract unique identifier for Microsoft (object ID)
    let provider_user_id = match user_info.get("id") {
        Some(id) => match id.as_str() {
            Some(i) => i.to_string(),
            None => return Err("Invalid id format".to_string()),
        },
        None => return Err("No id in user info".to_string()),
    };
    
    use crate::models::NewUserAuthIdentity;

    // First try to find the user by their external identity
    match user_auth_identities::find_user_by_identity(&provider.provider_type, &provider_user_id, conn) {
        Ok(Some(user_uuid)) => {
            // User found by identity, return the user
            match crate::repository::users::get_user_by_uuid(&user_uuid, conn) {
                Ok(user) => return Ok(user),
                Err(e) => return Err(format!("Error retrieving user: {e:?}")),
            }
        },
        Ok(None) => {
            // No identity found, look for the user by email as a fallback
            match crate::repository::get_user_by_email(&email, conn) {
                Ok(user) => {
                    // User found by email, create an identity for them
                    let new_identity = NewUserAuthIdentity {
                        user_uuid: user.uuid,
                        provider_type: provider.provider_type.clone(),
                        external_id: provider_user_id.clone(),
                        email: Some(email.clone()),
                        metadata: Some(user_info.clone()),
                        password_hash: None, // No password for OAuth identities
                    };

                    match crate::repository::user_auth_identities::create_identity(new_identity, conn) {
                        Ok(_) => {
                            // Identity created, now add the OAuth email to user_emails if not already present
                            if let Err(_) = crate::repository::user_emails::find_user_by_any_email(conn, &email) {
                                let new_email = crate::models::NewUserEmail {
                                    user_uuid: user.uuid,
                                    email: email.to_lowercase(),
                                    email_type: "work".to_string(),
                                    is_primary: false,
                                    is_verified: true,
                                    source: Some(provider.provider_type.clone()),
                                };

                                if let Err(e) = diesel::insert_into(crate::schema::user_emails::table)
                                    .values(&new_email)
                                    .execute(conn)
                                {
                                    warn!(provider = %provider.provider_type, error = ?e, "Failed to add email to user_emails");
                                }
                            }

                            return Ok(user);
                        },
                        Err(e) => {
                            warn!(error = ?e, "Failed to create user identity, will create new user");
                            // Continue to create a new user
                        }
                    }
                },
                Err(_) => {
                    // User not found by email, create a new one
                }
            }
        },
        Err(e) => {
            warn!(error = ?e, "Failed to find user by identity, will create new user");
            // Continue to create a new user
        }
    }
    
    // Create a new user
    use crate::models::UserRole;
    // Removed unused import: use uuid::Uuid;
    
    // Generate a secure random password for the user
    let random_password = format!("{:x}", rand::random::<u128>());
    let password_hash = match crate::utils::auth::hash_password(&random_password) {
        Ok(hash) => hash,
        Err(e) => return Err(format!("Failed to hash password: {e}")),
    };
    
    // Create local user (password will be stored in user_auth_identities)
    let new_user = utils::NewUserBuilder::local_user(name, email.clone(), UserRole::User).build();
    
    match crate::repository::create_user(new_user, conn) {
        Ok(user) => {
            // Create an identity for the new user
            let new_identity = NewUserAuthIdentity {
                user_uuid: user.uuid,
                provider_type: provider.provider_type.clone(),
                external_id: provider_user_id,
                email: Some(email),
                metadata: Some(user_info.clone()),
                password_hash: Some(password_hash), // Add the password hash to the identity
            };
            
            match crate::repository::user_auth_identities::create_identity(new_identity, conn) {
                Ok(_) => Ok(user),
                Err(e) => Err(format!("User created but failed to create identity: {e:?}")),
            }
        },
        Err(e) => Err(format!("Failed to create user: {e:?}")),
    }
}

// Helper function to add an OAuth identity to an existing user
async fn add_oauth_identity_to_user(
    user_uuid: &str,
    user_info: &serde_json::Value,
    provider: &AuthProvider,
    conn: &mut DbConnection
) -> Result<(), String> {
    // Parse UUID from string
    let parsed_uuid = match crate::utils::parse_uuid(user_uuid) {
        Ok(uuid) => uuid,
        Err(e) => return Err(format!("Invalid UUID format: {e}")),
    };

    // First find the user by UUID
    let user = match crate::repository::get_user_by_uuid(&parsed_uuid, conn) {
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
    let email = user_info.get("mail")
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
    };

    // Save the identity to the database
    match crate::repository::user_auth_identities::create_identity(new_identity, conn) {
        Ok(_) => {},
        Err(e) => return Err(format!("Failed to create auth identity: {e:?}")),
    }

    // Also add the OAuth provider email to user_emails table if present
    if let Some(email_address) = email {
        // Check if email already exists in user_emails table
        if let Err(_) = crate::repository::user_emails::find_user_by_any_email(conn, &email_address) {
            // Email doesn't exist, add it
            let new_email = crate::models::NewUserEmail {
                user_uuid: user.uuid,
                email: email_address.to_lowercase(),
                email_type: "work".to_string(), // OAuth emails are typically work emails
                is_primary: false, // Don't make it primary automatically
                is_verified: true, // OAuth providers verify emails
                source: Some(provider.provider_type.clone()),
            };

            if let Err(e) = diesel::insert_into(crate::schema::user_emails::table)
                .values(&new_email)
                .execute(conn)
            {
                warn!(error = ?e, "Failed to add OAuth provider email to user_emails");
                // Don't fail the whole operation if email insert fails
            }
        }
    }

    Ok(())
}

// Direct endpoint for connecting a new authentication method to an existing user
pub async fn oauth_connect(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    oauth_request: web::Json<OAuthRequest>,
) -> impl Responder {
    // Get database connection
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Verify user exists
    let user_uuid = match crate::utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user UUID in token"
        })),
    };
    
    let user = match crate::repository::get_user_by_uuid(&user_uuid, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            error!(user_uuid = %user_uuid, error = ?e, "Failed to find user by UUID");
            return HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": "User not found"
            }));
        }
    };

    let provider_type = &oauth_request.provider_type;

    // Get the provider by type
    let provider = match get_provider_by_type(provider_type) {
        Ok(p) => {
            if !p.enabled {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": format!("{} authentication is not enabled", p.name)
                }));
            }
            p
        },
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return HttpResponse::NotFound().json(json!({
                    "status": "error",
                    "message": format!("{} authentication provider not found", provider_type)
                }));
            } else {
                error!(provider = %provider_type, error = ?e, "Failed to get auth provider for connect");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to retrieve authentication provider"
                }));
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
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        let tenant_id = match config_utils::get_microsoft_tenant_id() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get tenant_id for Microsoft provider connect");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        let redirect_uri_config = match config_utils::get_microsoft_redirect_uri() {
            Ok(val) => val,
            Err(e) => {
                error!(error = ?e, "Failed to get redirect_uri for Microsoft provider connect");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": format!("Microsoft authentication is not properly configured: {}", e)
                }));
            }
        };

        // Prepare redirect URI with user UUID
        let mut actual_redirect_uri = oauth_request.redirect_uri.clone().unwrap_or_else(|| "/profile/settings".to_string());
        if !actual_redirect_uri.contains("user_uuid=") {
            let separator = if actual_redirect_uri.contains('?') { "&" } else { "?" };
            actual_redirect_uri = format!("{}{}user_uuid={}", actual_redirect_uri, separator, user.uuid);
        }

        // Generate a JWT state token with user_connection=true
        let state = match create_oauth_state(&provider.provider_type, Some(actual_redirect_uri), Some(true)) {
            Ok(token) => token,
            Err(e) => {
                error!(error = %e, "Failed to create OAuth state token for connect");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to initiate authentication flow"
                }));
            }
        };
        
        // Create the authorization URL
        let auth_url = format!(
            "https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/authorize?client_id={client_id}&response_type=code&redirect_uri={redirect_uri_config}&response_mode=query&scope=User.Read&state={state}"
        );
        
        HttpResponse::Ok().json(json!({
            "auth_url": auth_url,
            "state": state
        }))
    } else {
        HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": format!("{} authentication is not implemented", provider.name)
        }))
    }
}

// Test Microsoft Entra configuration
#[allow(dead_code)]
pub async fn test_microsoft_config(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    path: web::Path<i32>,
) -> impl Responder {
    // Get database connection
    let conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Check if the user is an admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Only administrators can test authentication providers"
        }));
    }

    let provider_id = path.into_inner();

    // Get the provider
    let provider = match get_provider_by_id(provider_id) {
        Ok(p) => {
            if p.provider_type != "microsoft" {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "This endpoint only supports testing Microsoft Entra configuration"
                }));
            }
            p
        },
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return HttpResponse::NotFound().json(json!({
                    "status": "error",
                    "message": "Authentication provider not found"
                }));
            } else {
                error!(provider_id = provider_id, error = ?e, "Failed to get auth provider for test");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to retrieve authentication provider"
                }));
            }
        }
    };

    // Get required configuration values from environment variables
    let client_id = match config_utils::get_microsoft_client_id() {
        Ok(val) => val,
        Err(e) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": format!("Missing client_id configuration: {}", e)
        })),
    };

    let tenant_id = match config_utils::get_microsoft_tenant_id() {
        Ok(val) => val,
        Err(e) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": format!("Missing tenant_id configuration: {}", e)
        })),
    };

    let client_secret = match config_utils::get_microsoft_client_secret() {
        Ok(val) => val,
        Err(e) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": format!("Missing client_secret configuration: {}", e)
        })),
    };

    let redirect_uri = match config_utils::get_microsoft_redirect_uri() {
        Ok(val) => val,
        Err(e) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": format!("Missing redirect_uri configuration: {}", e)
        })),
    };

    // Test the configuration by attempting to get an access token
    let params = [
        ("client_id", client_id.as_str()),
        ("client_secret", client_secret.as_str()),
        ("grant_type", "client_credentials"),
        ("scope", "https://graph.microsoft.com/.default"),
    ];

    // Make the token request
    let client = reqwest::Client::new();
    match client
        .post(format!("https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token"))
        .form(&params)
        .send()
        .await
    {
        Ok(response) => {
            match response.json::<serde_json::Value>().await {
                Ok(token_response) => {
                    if token_response.get("access_token").is_some() {
                        HttpResponse::Ok().json(json!({
                            "status": "success",
                            "message": "Microsoft Entra configuration is valid",
                            "details": {
                                "client_id_valid": true,
                                "tenant_id_valid": true,
                                "client_secret_valid": true,
                                "redirect_uri_configured": true
                            }
                        }))
                    } else {
                        HttpResponse::BadRequest().json(json!({
                            "status": "error",
                            "message": "Invalid configuration",
                            "details": token_response.get("error_description").and_then(|v| v.as_str()).unwrap_or("Unknown error")
                        }))
                    }
                },
                Err(e) => {
                    error!(error = ?e, "Failed to parse token response during Microsoft config test");
                    HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Failed to parse Microsoft authentication response"
                    }))
                }
            }
        },
        Err(e) => {
            error!(error = ?e, "Failed to test Microsoft configuration");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to test Microsoft configuration"
            }))
        }
    }
}

// Set a provider as default (admin only)
#[allow(dead_code)]
pub async fn set_default_auth_provider(
    db_pool: web::Data<Pool>,
    req: HttpRequest,
    request_data: web::Json<serde_json::Value>,
) -> impl Responder {
    // Get database connection
    let conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Check if the user is an admin
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Only administrators can manage authentication providers"
        }));
    }

    // Extract provider_id from request
    let provider_id = match request_data.get("provider_id").and_then(|v| v.as_i64()) {
        Some(id) => id as i32,
        None => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "provider_id is required"
        })),
    };

    // Verify the provider exists
    match get_provider_by_id(provider_id) {
        Ok(_) => {},
        Err(e) => {
            if let diesel::result::Error::NotFound = e {
                return HttpResponse::NotFound().json(json!({
                    "status": "error",
                    "message": "Authentication provider not found"
                }));
            } else {
                error!(provider_id = provider_id, error = ?e, "Failed to get auth provider for set default");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to retrieve authentication provider"
                }));
            }
        }
    }

    // Setting default providers is not supported for environment-based configuration
    HttpResponse::BadRequest().json(json!({
        "status": "error",
        "message": "Default provider configuration must be done via environment variables. Environment-based providers don't support runtime default changes."
    }))
} 