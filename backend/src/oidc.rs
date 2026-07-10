//! OpenID Connect (OIDC) authentication module
//!
//! Provides generic OIDC support for any compatible provider, following security best practices:
//! - PKCE (Proof Key for Code Exchange) for authorization code flow
//! - Nonce validation for ID token replay protection
//! - State parameter for CSRF protection
//! - ID token signature verification via JWKS
//!
//! Configuration is by auto-discovery: provide `OIDC_ISSUER_URL` and the
//! provider's `/.well-known/openid-configuration` supplies the endpoints and
//! the JWKS used to verify ID tokens. (Manual endpoint configuration was
//! removed: without a JWKS/issuer it could not verify signatures, so it failed
//! every login — see the roadmap for a future verifiable manual mode via an
//! explicit `OIDC_JWKS_URI`.)

use openidconnect::{
    core::{
        CoreAuthDisplay, CoreAuthPrompt, CoreClaimName, CoreClaimType, CoreClient,
        CoreClientAuthMethod, CoreGrantType, CoreIdToken, CoreIdTokenClaims, CoreIdTokenVerifier,
        CoreJsonWebKey, CoreJweContentEncryptionAlgorithm, CoreJweKeyManagementAlgorithm,
        CoreProviderMetadata, CoreResponseMode, CoreResponseType, CoreSubjectIdentifierType,
        CoreTokenResponse,
    },
    AdditionalProviderMetadata, AuthorizationCode, AuthorizationRequest, ClientId, ClientSecret,
    CsrfToken, EndSessionUrl, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, Nonce,
    PkceCodeChallenge, PkceCodeVerifier, ProviderMetadata, RedirectUrl, Scope, TokenResponse,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::config_utils;

/// OIDC configuration loaded from environment variables
#[derive(Debug, Clone)]
pub struct OidcConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    /// The OIDC issuer for auto-discovery (`OIDC_ISSUER_URL`). Required: it
    /// supplies the endpoints and the JWKS + issuer that make ID-token
    /// verification possible.
    pub issuer_url: String,
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
        let client_id =
            config_utils::get_oidc_client_id().map_err(|e| format!("OIDC_CLIENT_ID: {e}"))?;
        let client_secret = config_utils::get_oidc_client_secret()
            .map_err(|e| format!("OIDC_CLIENT_SECRET: {e}"))?;
        let redirect_uri =
            config_utils::get_oidc_redirect_uri().map_err(|e| format!("OIDC_REDIRECT_URI: {e}"))?;

        // Auto-discovery is required: it provides the JWKS + issuer used to
        // verify ID tokens. Manual endpoint configuration was removed because
        // it could verify neither, so an OIDC IdP must publish a discovery
        // document at `<issuer>/.well-known/openid-configuration`.
        let issuer_url = config_utils::get_oidc_issuer_url().map_err(|_| {
            "OIDC_ISSUER_URL is required (auto-discovery). Manual endpoint \
             configuration (OIDC_AUTH_URI / OIDC_TOKEN_URI) is no longer supported."
                .to_string()
        })?;

        let scopes: Vec<String> = config_utils::get_oidc_scopes()
            .split_whitespace()
            .map(|s| s.to_string())
            .collect();

        Ok(Self {
            client_id,
            client_secret,
            redirect_uri,
            issuer_url,
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
    // Ensure metadata + endpoint cache are populated
    let _ = get_oidc_client().await;

    let endpoint_guard = END_SESSION_ENDPOINT.read().await;
    endpoint_guard.clone()
}

/// Generate a logout URL for RP-initiated logout (OpenID Connect RP-Initiated Logout 1.0)
pub async fn generate_logout_url(
    post_logout_redirect_uri: &str,
    id_token_hint: Option<&str>,
    state: Option<&str>,
) -> Option<String> {
    let end_session_endpoint = get_end_session_endpoint().await?;

    let mut params: Vec<(&str, String)> = vec![(
        "post_logout_redirect_uri",
        post_logout_redirect_uri.to_string(),
    )];

    if let Some(token) = id_token_hint {
        params.push(("id_token_hint", token.to_string()));
    }

    if let Some(s) = state {
        params.push(("state", s.to_string()));
    }

    if let Some(config) = get_config() {
        params.push(("client_id", config.client_id.clone()));
    }

    let query_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let separator = if end_session_endpoint.contains('?') {
        "&"
    } else {
        "?"
    };
    let logout_url = format!("{end_session_endpoint}{separator}{query_string}");

    info!("OIDC: Generated logout URL: {}", logout_url);
    Some(logout_url)
}

/// User info extracted from OIDC ID token and/or userinfo endpoint
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcUserInfo {
    pub sub: String,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
    pub name: Option<String>,
    pub preferred_username: Option<String>,
    pub given_name: Option<String>,
    pub family_name: Option<String>,
    pub picture: Option<String>,
    pub raw_claims: serde_json::Value,
}

/// OIDC authentication flow data (stored in state JWT)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcAuthData {
    pub pkce_verifier: String,
    pub nonce: String,
}

/// Additional provider metadata for logout support (RP-Initiated Logout 1.0)
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct LogoutProviderMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_session_endpoint: Option<EndSessionUrl>,
}
impl AdditionalProviderMetadata for LogoutProviderMetadata {}

// In openidconnect 4.x the JWS algorithm / key type / key use generic
// parameters were collapsed into associated types on JsonWebKey, so
// ProviderMetadata now takes 12 generics instead of 15.
type ProviderMetadataWithLogout = ProviderMetadata<
    LogoutProviderMetadata,
    CoreAuthDisplay,
    CoreClientAuthMethod,
    CoreClaimName,
    CoreClaimType,
    CoreGrantType,
    CoreJweContentEncryptionAlgorithm,
    CoreJweKeyManagementAlgorithm,
    CoreJsonWebKey,
    CoreResponseMode,
    CoreResponseType,
    CoreSubjectIdentifierType,
>;

// `from_provider_metadata` produces a Client with auth EndpointSet (always
// present in the discovery doc) but token / userinfo as EndpointMaybeSet
// (optional in the spec, surfaced via fallible methods).
type DiscoveredClient = CoreClient<
    EndpointSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointNotSet,
    EndpointMaybeSet,
    EndpointMaybeSet,
>;

/// Wraps the discovery-built client. Kept as an enum so the call sites and the
/// cached-client machinery stay unchanged if a second, verifiable client shape
/// (e.g. a future JWKS-URI manual mode) is added.
pub enum OidcClientKind {
    Discovered(DiscoveredClient),
}

impl OidcClientKind {
    fn clone_kind(&self) -> Self {
        match self {
            Self::Discovered(c) => Self::Discovered(c.clone()),
        }
    }

    pub fn id_token_verifier(&self) -> CoreIdTokenVerifier<'_> {
        match self {
            Self::Discovered(c) => c.id_token_verifier(),
        }
    }

    pub fn authorize_url<NF, SF>(
        &self,
        state_fn: SF,
        nonce_fn: NF,
    ) -> AuthorizationRequest<'_, CoreAuthDisplay, CoreAuthPrompt, CoreResponseType>
    where
        SF: FnOnce() -> CsrfToken + 'static,
        NF: FnOnce() -> Nonce + 'static,
    {
        let flow = openidconnect::AuthenticationFlow::<CoreResponseType>::AuthorizationCode;
        match self {
            Self::Discovered(c) => c.authorize_url(flow, state_fn, nonce_fn),
        }
    }

    pub async fn exchange_code(
        &self,
        code: AuthorizationCode,
        pkce_verifier: PkceCodeVerifier,
        // Per-request redirect override. OAuth requires the token-exchange
        // redirect_uri to equal the one used at authorize time. In hosted
        // mode that's the tenant's own subdomain callback; `None` keeps the
        // client's statically configured redirect.
        redirect_override: Option<RedirectUrl>,
    ) -> Result<CoreTokenResponse, String> {
        match self {
            Self::Discovered(c) => {
                let mut req = c
                    .exchange_code(code)
                    .map_err(|e| format!("Token endpoint not configured: {e}"))?
                    .set_pkce_verifier(pkce_verifier);
                if let Some(r) = redirect_override {
                    req = req.set_redirect_uri(Cow::Owned(r));
                }
                req.request_async(&*OIDC_HTTP_CLIENT)
                    .await
                    .map_err(|e| format!("Token exchange failed: {e}"))
            }
        }
    }
}

/// Cached, fully-built OIDC client (rebuilt lazily, then reused). Avoids
/// running OIDC discovery on every request.
static OIDC_CLIENT: once_cell::sync::Lazy<Arc<RwLock<Option<OidcClientKind>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

/// Cached end_session_endpoint URL (discovered from provider metadata)
static END_SESSION_ENDPOINT: once_cell::sync::Lazy<Arc<RwLock<Option<String>>>> =
    once_cell::sync::Lazy::new(|| Arc::new(RwLock::new(None)));

// openidconnect 4.x uses stateful HTTP clients and only the reqwest::Client
// re-exported by openidconnect implements its AsyncHttpClient trait. We can't
// use the top-level reqwest dependency here even at the same major version,
// because openidconnect pins its own copy and the trait impl is keyed on that
// specific crate's types. The redirect policy must be `none` to avoid SSRF if
// the IdP can be coaxed into redirecting to an internal URL.
static OIDC_HTTP_CLIENT: once_cell::sync::Lazy<openidconnect::reqwest::Client> =
    once_cell::sync::Lazy::new(|| {
        openidconnect::reqwest::Client::builder()
            .redirect(openidconnect::reqwest::redirect::Policy::none())
            .build()
            .expect("Failed to build OIDC HTTP client")
    });

/// Initialize or get the cached OIDC client.
pub async fn get_oidc_client() -> Result<OidcClientKind, String> {
    {
        let client_guard = OIDC_CLIENT.read().await;
        if let Some(client) = client_guard.as_ref() {
            return Ok(client.clone_kind());
        }
    }

    let config = OidcConfig::from_env()?;
    let client = create_oidc_client(&config).await?;

    let cloned = client.clone_kind();
    {
        let mut client_guard = OIDC_CLIENT.write().await;
        *client_guard = Some(client);
    }

    Ok(cloned)
}

/// Create an OIDC client from configuration
async fn create_oidc_client(config: &OidcConfig) -> Result<OidcClientKind, String> {
    let client_id = ClientId::new(config.client_id.clone());
    let client_secret = ClientSecret::new(config.client_secret.clone());
    let redirect_url = RedirectUrl::new(config.redirect_uri.clone())
        .map_err(|e| format!("Invalid redirect URI: {e}"))?;

    info!(
        "OIDC: Using auto-discovery from issuer: {}",
        config.issuer_url
    );

    let issuer = IssuerUrl::new(config.issuer_url.clone())
        .map_err(|e| format!("Invalid issuer URL: {e}"))?;

    let provider_metadata: ProviderMetadataWithLogout =
        ProviderMetadataWithLogout::discover_async(issuer.clone(), &*OIDC_HTTP_CLIENT)
            .await
            .map_err(|e| format!("OIDC discovery failed: {e}"))?;

    if let Some(end_session_url) = provider_metadata
        .additional_metadata()
        .end_session_endpoint
        .as_ref()
    {
        let url_string = end_session_url.url().to_string();
        info!("OIDC: Discovered end_session_endpoint: {}", url_string);
        let mut endpoint_guard = END_SESSION_ENDPOINT.write().await;
        *endpoint_guard = Some(url_string);
    } else if let Some(ref logout_uri) = config.logout_uri {
        info!(
            "OIDC: Using manually configured logout URI as fallback: {}",
            logout_uri
        );
        let mut endpoint_guard = END_SESSION_ENDPOINT.write().await;
        *endpoint_guard = Some(logout_uri.clone());
    } else {
        info!(
            "OIDC: Provider does not advertise end_session_endpoint and no \
             OIDC_LOGOUT_URI configured"
        );
    }

    // Re-discover as CoreProviderMetadata to feed the client. The logout-augmented
    // metadata above carries the same data but uses a different additional-metadata type,
    // so it isn't directly compatible with from_provider_metadata's CoreClient signature.
    let core_metadata = CoreProviderMetadata::discover_async(issuer, &*OIDC_HTTP_CLIENT)
        .await
        .map_err(|e| format!("OIDC discovery failed: {e}"))?;

    let client = CoreClient::from_provider_metadata(core_metadata, client_id, Some(client_secret))
        .set_redirect_uri(redirect_url);

    Ok(OidcClientKind::Discovered(client))
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
    // Per-tenant OAuth callback override (hosted mode). Must equal the
    // redirect_uri later presented at token exchange. `None` uses the
    // client's configured `OIDC_REDIRECT_URI`.
    callback_redirect: Option<String>,
) -> Result<(String, OidcAuthData), String> {
    let client = get_oidc_client().await?;
    let config = OidcConfig::from_env()?;

    let (pkce_challenge, pkce_verifier) = generate_pkce();

    let nonce = generate_nonce();
    let nonce_secret = nonce.secret().to_string();

    let mut auth_request = client
        .authorize_url(CsrfToken::new_random, move || nonce.clone())
        .set_pkce_challenge(pkce_challenge);

    for scope in &config.scopes {
        if scope != "openid" {
            auth_request = auth_request.add_scope(Scope::new(scope.clone()));
        }
    }

    if let Some(r) = callback_redirect {
        let redirect_url = RedirectUrl::new(r).map_err(|e| format!("Invalid redirect URI: {e}"))?;
        auth_request = auth_request.set_redirect_uri(Cow::Owned(redirect_url));
    }

    let (auth_url, _csrf_token, _nonce) = auth_request.url();

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
    // Per-tenant OAuth callback override (hosted mode); must equal the
    // redirect_uri used at authorize time. `None` uses the configured one.
    callback_redirect: Option<String>,
) -> Result<OidcUserInfo, String> {
    let client = get_oidc_client().await?;
    let config = OidcConfig::from_env()?;

    let pkce_verifier = PkceCodeVerifier::new(auth_data.pkce_verifier.clone());

    let redirect_override = match callback_redirect {
        Some(r) => Some(RedirectUrl::new(r).map_err(|e| format!("Invalid redirect URI: {e}"))?),
        None => None,
    };

    debug!("OIDC: Exchanging authorization code for tokens");
    let token_response = client
        .exchange_code(
            AuthorizationCode::new(code.to_string()),
            pkce_verifier,
            redirect_override,
        )
        .await?;

    let id_token = token_response
        .id_token()
        .ok_or_else(|| "No ID token in response".to_string())?;

    let nonce = Nonce::new(auth_data.nonce.clone());
    let claims = verify_id_token(&client, id_token, &nonce)?;

    let user_info = extract_user_info(&claims, &config)?;

    info!(
        "OIDC: Successfully authenticated user with sub: {}",
        user_info.sub
    );

    Ok(user_info)
}

/// Verify an ID token presented by the native (mobile) app and return its user
/// info.
///
/// The app is its own *public* OIDC client (`native_client_id`, no secret): it
/// runs the Authorization-Code + PKCE flow against the IdP itself and posts the
/// resulting ID token to the product, which trusts it only after this check. The
/// token's audience is the native client, not the confidential web
/// `OIDC_CLIENT_ID`, so we build a verifier for that audience (reusing the
/// issuer's discovery + JWKS). We verify signature, issuer, audience and expiry.
/// The `nonce` is the app's replay protection (it generated and verifies it),
/// so it is intentionally not re-checked here.
pub async fn verify_native_id_token(
    id_token_str: &str,
    native_client_id: &str,
) -> Result<OidcUserInfo, String> {
    use std::str::FromStr;

    let config = OidcConfig::from_env()?;
    let issuer = IssuerUrl::new(config.issuer_url.clone())
        .map_err(|e| format!("Invalid issuer URL: {e}"))?;

    let core_metadata = CoreProviderMetadata::discover_async(issuer, &*OIDC_HTTP_CLIENT)
        .await
        .map_err(|e| format!("OIDC discovery failed: {e}"))?;

    // Public client (no secret); the verifier checks the signature against the
    // discovered JWKS plus issuer and audience (== native_client_id).
    let native_client = CoreClient::from_provider_metadata(
        core_metadata,
        ClientId::new(native_client_id.to_string()),
        None,
    );

    let id_token =
        CoreIdToken::from_str(id_token_str).map_err(|e| format!("Malformed ID token: {e}"))?;

    let verifier = native_client.id_token_verifier();
    let claims = id_token
        .claims(&verifier, |_: Option<&Nonce>| Ok::<(), String>(()))
        .map_err(|e| format!("ID token verification failed: {e}"))?;

    extract_user_info(claims, &config)
}

/// Verify a Microsoft v2.0 ID token against the configured tenant's discovery
/// document.
///
/// Discovery runs on `login.microsoftonline.com/{tenant}/v2.0`, so the
/// verifier's issuer check binds the token to that exact tenant: a Microsoft
/// v2.0 ID token's `iss` encodes the tenant GUID, and a token minted for any
/// other tenant fails the issuer match. That is the cryptographic tenant proof
/// (equivalent to, and stronger than, reading the `tid` claim by hand). The
/// verifier also checks the JWKS signature, the audience (== `client_id`) and
/// expiry; `nonce` is the flow's replay protection, echoed by Azure.
///
/// Requires a SPECIFIC tenant (GUID or verified domain): a multi-tenant
/// authority (`common`/`organizations`/`consumers`) has no concrete issuer to
/// discover, which is one reason those are rejected upstream
/// (`config_utils::get_microsoft_tenant_id`).
pub async fn verify_microsoft_id_token(
    id_token_str: &str,
    tenant_id: &str,
    client_id: &str,
    nonce: &str,
) -> Result<(), String> {
    use std::str::FromStr;

    let issuer = IssuerUrl::new(format!(
        "https://login.microsoftonline.com/{tenant_id}/v2.0"
    ))
    .map_err(|e| format!("Invalid Microsoft issuer URL: {e}"))?;

    let metadata = CoreProviderMetadata::discover_async(issuer, &*OIDC_HTTP_CLIENT)
        .await
        .map_err(|e| format!("Microsoft OIDC discovery failed: {e}"))?;

    let client =
        CoreClient::from_provider_metadata(metadata, ClientId::new(client_id.to_string()), None);

    let id_token =
        CoreIdToken::from_str(id_token_str).map_err(|e| format!("Malformed ID token: {e}"))?;

    let expected_nonce = Nonce::new(nonce.to_string());
    let verifier = client.id_token_verifier();
    id_token
        .claims(&verifier, &expected_nonce)
        .map_err(|e| format!("Microsoft ID token verification failed: {e}"))?;
    Ok(())
}

/// Verify ID token signature and claims
fn verify_id_token(
    client: &OidcClientKind,
    id_token: &CoreIdToken,
    nonce: &Nonce,
) -> Result<CoreIdTokenClaims, String> {
    let verifier: CoreIdTokenVerifier = client.id_token_verifier();

    id_token
        .claims(&verifier, nonce)
        .map_err(|e| format!("ID token verification failed: {e}"))
        .cloned()
}

/// Extract user info from ID token claims
fn extract_user_info(
    claims: &CoreIdTokenClaims,
    _config: &OidcConfig,
) -> Result<OidcUserInfo, String> {
    let sub = claims.subject().to_string();

    let email = claims.email().map(|e| e.to_string());
    let email_verified = claims.email_verified();

    let name = claims
        .name()
        .and_then(|n| n.get(None).map(|s| s.to_string()));

    let given_name = claims
        .given_name()
        .and_then(|n| n.get(None).map(|s| s.to_string()));

    let family_name = claims
        .family_name()
        .and_then(|n| n.get(None).map(|s| s.to_string()));

    let preferred_username = claims.preferred_username().map(|u| u.to_string());

    let picture = claims
        .picture()
        .and_then(|p| p.get(None).map(|u| u.to_string()));

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

/// The display name for a user, honoring the operator-configured
/// `OIDC_USERNAME_CLAIM` (default `preferred_username`) with fallbacks through
/// the standard name claims. Returns `None` when the IdP sent no usable name so
/// the caller can defer to its own fallback (e.g. the email local-part) rather
/// than surfacing an opaque `sub`. An operator who explicitly selects `sub`
/// gets it.
pub fn display_name_from_claim(user_info: &OidcUserInfo, username_claim: &str) -> Option<String> {
    let from_claim = match username_claim {
        "preferred_username" => user_info.preferred_username.clone(),
        "email" => user_info.email.clone(),
        "name" => user_info.name.clone(),
        "sub" => Some(user_info.sub.clone()),
        _ => user_info.preferred_username.clone(),
    };

    from_claim
        .or_else(|| user_info.name.clone())
        .or_else(|| user_info.preferred_username.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_pkce_generation() {
        let (_challenge, verifier) = generate_pkce();
        assert!(!verifier.secret().is_empty());
    }

    #[test]
    fn test_nonce_generation() {
        let nonce1 = generate_nonce();
        let nonce2 = generate_nonce();
        assert_ne!(nonce1.secret(), nonce2.secret());
    }

    fn user(v: serde_json::Value) -> OidcUserInfo {
        serde_json::from_value(v).expect("valid OidcUserInfo fixture")
    }

    #[test]
    fn display_name_honors_configured_claim() {
        let u = user(json!({
            "sub": "s", "email": "a@x.com", "name": "Full Name",
            "preferred_username": "flast", "raw_claims": {},
        }));
        // Operator-selected claim wins.
        assert_eq!(
            display_name_from_claim(&u, "name").as_deref(),
            Some("Full Name")
        );
        assert_eq!(
            display_name_from_claim(&u, "email").as_deref(),
            Some("a@x.com")
        );
        assert_eq!(display_name_from_claim(&u, "sub").as_deref(), Some("s"));
        // Default claim.
        assert_eq!(
            display_name_from_claim(&u, "preferred_username").as_deref(),
            Some("flast")
        );
    }

    #[test]
    fn display_name_falls_back_to_none_not_sub() {
        // Configured claim absent -> name -> preferred_username -> None (so the
        // caller defers to its own fallback rather than exposing the raw sub).
        let u = user(json!({"sub": "opaque-123", "email": "a@x.com", "raw_claims": {}}));
        assert_eq!(display_name_from_claim(&u, "preferred_username"), None);
        // But an explicit `sub` selection is respected.
        assert_eq!(
            display_name_from_claim(&u, "sub").as_deref(),
            Some("opaque-123")
        );
    }
}
