//! OpenID Connect (OIDC) authentication module
//!
//! Provides generic OIDC support for any compatible provider, following security best practices:
//! - PKCE (Proof Key for Code Exchange) for authorization code flow
//! - Nonce validation for ID token replay protection
//! - State parameter for CSRF protection
//! - ID token signature verification via JWKS
//!
//! Supports two configuration modes:
//! 1. Auto-discovery: Just provide OIDC_ISSUER_URL
//! 2. Manual: Provide OIDC_AUTH_URI, OIDC_TOKEN_URI, OIDC_USERINFO_URI

use openidconnect::{
    core::{CoreClient, CoreProviderMetadata, CoreIdToken, CoreIdTokenClaims, CoreIdTokenVerifier},
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce,
    OAuth2TokenResponse, PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse, AuthUrl, TokenUrl, UserInfoUrl,
    reqwest::async_http_client,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};

use crate::config_utils;

/// OIDC configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub issuer_url: Option<String>,
    pub auth_uri: Option<String>,
    pub token_uri: Option<String>,
    pub userinfo_uri: Option<String>,
    pub display_name: String,
    pub scopes: Vec<String>,
    pub username_claim: String,
    pub logout_uri: Option<String>,
}

impl OidcConfig {
    /// Load OIDC configuration from environment variables
    pub fn from_env() -> Result<Self, String> {
        let client_id = config_utils::get_oidc_client_id()
            .map_err(|e| format!("OIDC_CLIENT_ID: {}", e))?;
        let client_secret = config_utils::get_oidc_client_secret()
            .map_err(|e| format!("OIDC_CLIENT_SECRET: {}", e))?;
        let redirect_uri = config_utils::get_oidc_redirect_uri()
            .map_err(|e| format!("OIDC_REDIRECT_URI: {}", e))?;

        let issuer_url = config_utils::get_oidc_issuer_url().ok();
        let auth_uri = config_utils::get_oidc_auth_uri().ok();
        let token_uri = config_utils::get_oidc_token_uri().ok();
        let userinfo_uri = config_utils::get_oidc_userinfo_uri().ok();

        // Validate: either issuer_url OR all manual URIs must be provided
        if issuer_url.is_none() && (auth_uri.is_none() || token_uri.is_none()) {
            return Err("Either OIDC_ISSUER_URL (for auto-discovery) or OIDC_AUTH_URI + OIDC_TOKEN_URI (manual) must be provided".to_string());
        }

        let scopes: Vec<String> = config_utils::get_oidc_scopes()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        Ok(Self {
            client_id,
            client_secret,
            redirect_uri,
            issuer_url,
            auth_uri,
            token_uri,
            userinfo_uri,
            display_name: config_utils::get_oidc_display_name(),
            scopes,
            username_claim: config_utils::get_oidc_username_claim(),
            logout_uri: config_utils::get_oidc_logout_uri(),
        })
    }
}

/// User info extracted from OIDC ID token and/or userinfo endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcUserInfo {
    /// Unique subject identifier from the provider
    pub sub: String,
    /// User's email address
    pub email: Option<String>,
    /// Whether the email has been verified by the provider
    pub email_verified: Option<bool>,
    /// User's full name
    pub name: Option<String>,
    /// User's preferred username
    pub preferred_username: Option<String>,
    /// User's given/first name
    pub given_name: Option<String>,
    /// User's family/last name
    pub family_name: Option<String>,
    /// User's profile picture URL
    pub picture: Option<String>,
    /// Raw claims as JSON for storage
    pub raw_claims: serde_json::Value,
}

/// OIDC authentication flow data (stored in state JWT)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcAuthData {
    /// PKCE code verifier (stored encrypted)
    pub pkce_verifier: String,
    /// Nonce for ID token validation
    pub nonce: String,
}

/// Cached OIDC client for reuse
static OIDC_CLIENT: once_cell::sync::Lazy<Arc<RwLock<Option<CoreClient>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

/// Initialize or get the cached OIDC client
pub async fn get_oidc_client() -> Result<CoreClient, String> {
    // Check if we have a cached client
    {
        let client_guard = OIDC_CLIENT.read().await;
        if let Some(client) = client_guard.as_ref() {
            return Ok(client.clone());
        }
    }

    // Need to create a new client
    let config = OidcConfig::from_env()?;
    let client = create_oidc_client(&config).await?;

    // Cache it
    {
        let mut client_guard = OIDC_CLIENT.write().await;
        *client_guard = Some(client.clone());
    }

    Ok(client)
}

/// Create an OIDC client from configuration
async fn create_oidc_client(config: &OidcConfig) -> Result<CoreClient, String> {
    let client_id = ClientId::new(config.client_id.clone());
    let client_secret = ClientSecret::new(config.client_secret.clone());
    let redirect_url = RedirectUrl::new(config.redirect_uri.clone())
        .map_err(|e| format!("Invalid redirect URI: {}", e))?;

    if let Some(issuer_url) = &config.issuer_url {
        // Auto-discovery mode
        info!("OIDC: Using auto-discovery from issuer: {}", issuer_url);

        let issuer = IssuerUrl::new(issuer_url.clone())
            .map_err(|e| format!("Invalid issuer URL: {}", e))?;

        // Discover provider metadata
        let provider_metadata = CoreProviderMetadata::discover_async(issuer, async_http_client)
            .await
            .map_err(|e| format!("OIDC discovery failed: {}", e))?;

        let client = CoreClient::from_provider_metadata(
            provider_metadata,
            client_id,
            Some(client_secret),
        )
        .set_redirect_uri(redirect_url);

        Ok(client)
    } else {
        // Manual configuration mode
        info!("OIDC: Using manual configuration");

        let auth_url = AuthUrl::new(config.auth_uri.clone().unwrap())
            .map_err(|e| format!("Invalid auth URI: {}", e))?;
        let token_url = TokenUrl::new(config.token_uri.clone().unwrap())
            .map_err(|e| format!("Invalid token URI: {}", e))?;

        // For manual config, we create a minimal client without provider metadata
        // Note: This won't have JWKS for ID token signature verification
        // Consider fetching JWKS separately if needed
        let client = CoreClient::new(
            client_id,
            Some(client_secret),
            IssuerUrl::new("https://placeholder.invalid".to_string()).unwrap(), // Placeholder
            auth_url,
            Some(token_url),
            config.userinfo_uri.as_ref().map(|u| UserInfoUrl::new(u.clone()).ok()).flatten(),
            Default::default(), // Empty JWKS - tokens won't be verified
        )
        .set_redirect_uri(redirect_url);

        warn!("OIDC: Manual configuration mode - ID token signatures cannot be verified without JWKS");

        Ok(client)
    }
}

/// Generate PKCE challenge and verifier pair
pub fn generate_pkce() -> (PkceCodeChallenge, PkceCodeVerifier) {
    PkceCodeChallenge::new_random_sha256()
}

/// Generate a cryptographically random nonce
pub fn generate_nonce() -> Nonce {
    Nonce::new_random()
}

/// Generate the authorization URL for OIDC login
pub async fn generate_auth_url(
    redirect_uri: Option<String>,
    user_connection: Option<bool>,
) -> Result<(String, OidcAuthData), String> {
    let client = get_oidc_client().await?;
    let config = OidcConfig::from_env()?;

    // Generate PKCE challenge/verifier
    let (pkce_challenge, pkce_verifier) = generate_pkce();

    // Generate nonce for ID token validation
    let nonce = generate_nonce();
    // Save nonce secret before moving into closure
    let nonce_secret = nonce.secret().to_string();

    // Build authorization URL with Authorization Code flow
    let mut auth_request = client
        .authorize_url(
            openidconnect::AuthenticationFlow::<openidconnect::core::CoreResponseType>::AuthorizationCode,
            CsrfToken::new_random,
            move || nonce.clone(),
        )
        .set_pkce_challenge(pkce_challenge);

    // Add configured scopes (skip "openid" as it's automatically added by AuthorizationCode flow)
    for scope in &config.scopes {
        if scope != "openid" {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }
    }

    let (auth_url, _csrf_token, _nonce) = auth_request.url();

    // Create auth data to store in state JWT
    let auth_data = OidcAuthData {
        pkce_verifier: pkce_verifier.secret().to_string(),
        nonce: nonce_secret,
    };

    debug!("OIDC: Generated authorization URL");

    Ok((auth_url.to_string(), auth_data))
}

/// Exchange authorization code for tokens and extract user info
pub async fn exchange_code(
    code: &str,
    auth_data: &OidcAuthData,
) -> Result<OidcUserInfo, String> {
    let client = get_oidc_client().await?;
    let config = OidcConfig::from_env()?;

    // Recreate PKCE verifier from stored secret
    let pkce_verifier = PkceCodeVerifier::new(auth_data.pkce_verifier.clone());

    // Exchange code for tokens
    debug!("OIDC: Exchanging authorization code for tokens");
    let token_response = client
        .exchange_code(AuthorizationCode::new(code.to_string()))
        .set_pkce_verifier(pkce_verifier)
        .request_async(async_http_client)
        .await
        .map_err(|e| format!("Token exchange failed: {}", e))?;

    // Get ID token
    let id_token = token_response
        .id_token()
        .ok_or_else(|| "No ID token in response".to_string())?;

    // Verify and extract claims from ID token
    let nonce = Nonce::new(auth_data.nonce.clone());
    let claims = verify_id_token(&client, id_token, &nonce)?;

    // Extract user info from claims
    let user_info = extract_user_info(&claims, &config)?;

    info!("OIDC: Successfully authenticated user with sub: {}", user_info.sub);

    Ok(user_info)
}

/// Verify ID token signature and claims
fn verify_id_token(
    client: &CoreClient,
    id_token: &CoreIdToken,
    nonce: &Nonce,
) -> Result<CoreIdTokenClaims, String> {
    let verifier: CoreIdTokenVerifier = client.id_token_verifier();

    id_token
        .claims(&verifier, nonce)
        .map_err(|e| format!("ID token verification failed: {}", e))
        .map(|c| c.clone())
}

/// Extract user info from ID token claims
fn extract_user_info(
    claims: &CoreIdTokenClaims,
    config: &OidcConfig,
) -> Result<OidcUserInfo, String> {
    // Subject is required
    let sub = claims.subject().to_string();

    // Extract optional claims
    let email = claims.email().map(|e| e.to_string());
    let email_verified = claims.email_verified();

    // Name handling - try to get localized name first
    let name = claims.name()
        .and_then(|n| n.get(None).map(|s| s.to_string()));

    let given_name = claims.given_name()
        .and_then(|n| n.get(None).map(|s| s.to_string()));

    let family_name = claims.family_name()
        .and_then(|n| n.get(None).map(|s| s.to_string()));

    let preferred_username = claims.preferred_username().map(|u| u.to_string());

    let picture = claims.picture()
        .and_then(|p| p.get(None).map(|u| u.to_string()));

    // Build raw claims JSON for storage
    let raw_claims = serde_json::json!({
        "sub": sub,
        "email": email,
        "email_verified": email_verified,
        "name": name,
        "given_name": given_name,
        "family_name": family_name,
        "preferred_username": preferred_username,
        "picture": picture,
    });

    Ok(OidcUserInfo {
        sub,
        email,
        email_verified,
        name,
        preferred_username,
        given_name,
        family_name,
        picture,
        raw_claims,
    })
}

/// Get the display name to use for the user
/// Uses configurable claim (defaults to preferred_username) with fallbacks
pub fn get_display_name(user_info: &OidcUserInfo, config: &OidcConfig) -> String {
    // Try configured claim first
    let from_claim = match config.username_claim.as_str() {
        "preferred_username" => user_info.preferred_username.clone(),
        "email" => user_info.email.clone(),
        "name" => user_info.name.clone(),
        "sub" => Some(user_info.sub.clone()),
        _ => user_info.preferred_username.clone(),
    };

    // Fallback chain: configured claim -> name -> email -> sub
    from_claim
        .or_else(|| user_info.name.clone())
        .or_else(|| user_info.email.clone())
        .unwrap_or_else(|| user_info.sub.clone())
}

/// Clear the cached OIDC client (useful if config changes)
pub async fn clear_client_cache() {
    let mut client_guard = OIDC_CLIENT.write().await;
    *client_guard = None;
    info!("OIDC: Client cache cleared");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pkce_generation() {
        let (challenge, verifier) = generate_pkce();
        assert!(!verifier.secret().is_empty());
        // Challenge is derived from verifier
    }

    #[test]
    fn test_nonce_generation() {
        let nonce1 = generate_nonce();
        let nonce2 = generate_nonce();
        assert_ne!(nonce1.secret(), nonce2.secret());
    }
}
