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
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope,
    TokenResponse, AuthUrl, TokenUrl, UserInfoUrl, EndSessionUrl,
    reqwest::async_http_client,
    AdditionalProviderMetadata, ProviderMetadata,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

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
    /// OIDC logout URI for single sign-out (reserved for future use)
    #[allow(dead_code)]
    pub logout_uri: Option<String>,
}

impl OidcConfig {
    /// Load OIDC configuration from environment variables
    pub fn from_env() -> Result<Self, String> {
        let client_id = config_utils::get_oidc_client_id()
            .map_err(|e| format!("OIDC_CLIENT_ID: {e}"))?;
        let client_secret = config_utils::get_oidc_client_secret()
            .map_err(|e| format!("OIDC_CLIENT_SECRET: {e}"))?;
        let redirect_uri = config_utils::get_oidc_redirect_uri()
            .map_err(|e| format!("OIDC_REDIRECT_URI: {e}"))?;

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

// Cached OIDC configuration (loaded once at startup)
lazy_static::lazy_static! {
    /// Cached OIDC configuration - None if OIDC is not enabled or config is invalid
    static ref OIDC_CONFIG: Option<OidcConfig> = {
        if !config_utils::is_oidc_enabled() {
            return None;
        }
        match OidcConfig::from_env() {
            Ok(config) => Some(config),
            Err(e) => {
                // Note: Using eprintln here because tracing may not be initialized yet during lazy_static initialization
                eprintln!("Failed to load OIDC config: {e}");
                None
            }
        }
    };
}

/// Get the cached OIDC configuration (if enabled and valid)
#[allow(dead_code)]
pub fn get_config() -> Option<&'static OidcConfig> {
    OIDC_CONFIG.as_ref()
}

/// Get the OIDC display name (returns "OIDC" as default if not configured)
pub fn get_display_name_cached() -> String {
    OIDC_CONFIG
        .as_ref()
        .map(|c| c.display_name.clone())
        .unwrap_or_else(|| "OIDC".to_string())
}

/// Get the cached end_session_endpoint URL (from discovery or manual config)
/// This is used for RP-initiated logout
pub async fn get_end_session_endpoint() -> Option<String> {
    // First, ensure the client is initialized (which populates the endpoint cache)
    let _ = get_oidc_client().await;

    // Return the cached endpoint
    let endpoint_guard = END_SESSION_ENDPOINT.read().await;
    endpoint_guard.clone()
}

/// Generate a logout URL for RP-initiated logout (OpenID Connect RP-Initiated Logout 1.0)
///
/// Parameters:
/// - `post_logout_redirect_uri`: Where to redirect after logout (must be registered with the OIDC provider)
/// - `id_token_hint`: Optional ID token to hint which user is logging out (recommended for security)
/// - `state`: Optional state parameter for CSRF protection on the redirect
///
/// Returns the full logout URL or None if OIDC logout is not configured
pub async fn generate_logout_url(
    post_logout_redirect_uri: &str,
    id_token_hint: Option<&str>,
    state: Option<&str>,
) -> Option<String> {
    let end_session_endpoint = get_end_session_endpoint().await?;

    // Build query parameters
    let mut params: Vec<(&str, String)> = vec![
        ("post_logout_redirect_uri", post_logout_redirect_uri.to_string()),
    ];

    // id_token_hint helps the OP identify the user without requiring confirmation
    if let Some(token) = id_token_hint {
        params.push(("id_token_hint", token.to_string()));
    }

    // state is optional CSRF protection for the redirect
    if let Some(s) = state {
        params.push(("state", s.to_string()));
    }

    // Include client_id for providers that require it
    if let Some(config) = get_config() {
        params.push(("client_id", config.client_id.clone()));
    }

    // Build query string using urlencoding
    let query_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // Append query to endpoint URL
    let separator = if end_session_endpoint.contains('?') { "&" } else { "?" };
    let logout_url = format!("{end_session_endpoint}{separator}{query_string}");

    info!("OIDC: Generated logout URL: {}", logout_url);
    Some(logout_url)
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

/// Additional provider metadata for logout support (RP-Initiated Logout 1.0)
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LogoutProviderMetadata {
    /// URL for the end session endpoint (for RP-initiated logout)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_session_endpoint: Option<EndSessionUrl>,
}
impl AdditionalProviderMetadata for LogoutProviderMetadata {}

/// Provider metadata type with logout support
type ProviderMetadataWithLogout = ProviderMetadata<
    LogoutProviderMetadata,
    openidconnect::core::CoreAuthDisplay,
    openidconnect::core::CoreClientAuthMethod,
    openidconnect::core::CoreClaimName,
    openidconnect::core::CoreClaimType,
    openidconnect::core::CoreGrantType,
    openidconnect::core::CoreJweContentEncryptionAlgorithm,
    openidconnect::core::CoreJweKeyManagementAlgorithm,
    openidconnect::core::CoreJwsSigningAlgorithm,
    openidconnect::core::CoreJsonWebKeyType,
    openidconnect::core::CoreJsonWebKeyUse,
    openidconnect::core::CoreJsonWebKey,
    openidconnect::core::CoreResponseMode,
    openidconnect::core::CoreResponseType,
    openidconnect::core::CoreSubjectIdentifierType,
>;

/// Cached OIDC client for reuse
static OIDC_CLIENT: once_cell::sync::Lazy<Arc<RwLock<Option<CoreClient>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

/// Cached end_session_endpoint URL (discovered from provider metadata)
static END_SESSION_ENDPOINT: once_cell::sync::Lazy<Arc<RwLock<Option<String>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

/// Initialize or get the cached OIDC client
pub async fn get_oidc_client() -> Result<CoreClient, String> {
    // Check for cached client
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
        .map_err(|e| format!("Invalid redirect URI: {e}"))?;

    if let Some(issuer_url) = &config.issuer_url {
        // Auto-discovery mode
        info!("OIDC: Using auto-discovery from issuer: {}", issuer_url);

        let issuer = IssuerUrl::new(issuer_url.clone())
            .map_err(|e| format!("Invalid issuer URL: {e}"))?;

        // Discover provider metadata with logout support
        let provider_metadata: ProviderMetadataWithLogout =
            ProviderMetadataWithLogout::discover_async(issuer, async_http_client)
                .await
                .map_err(|e| format!("OIDC discovery failed: {e}"))?;

        // Cache the end_session_endpoint if available from discovery or config fallback
        if let Some(end_session_url) = provider_metadata.additional_metadata().end_session_endpoint.as_ref() {
            let url_string = end_session_url.url().to_string();
            info!("OIDC: Discovered end_session_endpoint: {}", url_string);
            let mut endpoint_guard = END_SESSION_ENDPOINT.write().await;
            *endpoint_guard = Some(url_string);
        } else if let Some(ref logout_uri) = config.logout_uri {
            // Fallback to manually configured logout URI if discovery doesn't provide one
            info!("OIDC: Using manually configured logout URI as fallback: {}", logout_uri);
            let mut endpoint_guard = END_SESSION_ENDPOINT.write().await;
            *endpoint_guard = Some(logout_uri.clone());
        } else {
            info!("OIDC: Provider does not advertise end_session_endpoint and no OIDC_LOGOUT_URI configured");
        }

        // Convert to CoreProviderMetadata for client creation
        // Re-discover with CoreProviderMetadata since the client expects that type
        let core_metadata = CoreProviderMetadata::discover_async(
            IssuerUrl::new(issuer_url.clone()).unwrap(),
            async_http_client
        ).await.map_err(|e| format!("OIDC discovery failed: {e}"))?;

        let client = CoreClient::from_provider_metadata(
            core_metadata,
            client_id,
            Some(client_secret),
        )
        .set_redirect_uri(redirect_url);

        Ok(client)
    } else {
        // Manual configuration mode
        info!("OIDC: Using manual configuration");

        let auth_url = AuthUrl::new(config.auth_uri.clone().unwrap())
            .map_err(|e| format!("Invalid auth URI: {e}"))?;
        let token_url = TokenUrl::new(config.token_uri.clone().unwrap())
            .map_err(|e| format!("Invalid token URI: {e}"))?;

        // Cache the logout_uri from config if provided (for manual configuration)
        if let Some(ref logout_uri) = config.logout_uri {
            info!("OIDC: Using manually configured logout URI: {}", logout_uri);
            let mut endpoint_guard = END_SESSION_ENDPOINT.write().await;
            *endpoint_guard = Some(logout_uri.clone());
        }

        // Manual config creates a minimal client without provider metadata
        // Note: This won't have JWKS for ID token signature verification
        // Consider fetching JWKS separately if needed
        let client = CoreClient::new(
            client_id,
            Some(client_secret),
            IssuerUrl::new("https://placeholder.invalid".to_string()).unwrap(), // Placeholder
            auth_url,
            Some(token_url),
            config.userinfo_uri.as_ref().and_then(|u| UserInfoUrl::new(u.clone()).ok()),
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
    _redirect_uri: Option<String>,
    _user_connection: Option<bool>,
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
        .map_err(|e| format!("Token exchange failed: {e}"))?;

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
        .map_err(|e| format!("ID token verification failed: {e}")).cloned()
}

/// Extract user info from ID token claims
fn extract_user_info(
    claims: &CoreIdTokenClaims,
    _config: &OidcConfig,
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
