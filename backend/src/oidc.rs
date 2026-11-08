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
    CsrfToken, EndSessionUrl, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, LoginHint,
    Nonce, PkceCodeChallenge, PkceCodeVerifier, ProviderMetadata, RedirectUrl, Scope,
    TokenResponse,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use tracing::{debug, info, warn};

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
    instance_client()
        .await
        .ok()
        .and_then(|cached| cached.end_session_endpoint.clone())
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

    // Only send `client_id` when there is NO `id_token_hint`. With a hint,
    // Hydra identifies the client from the token, and a `client_id` that
    // disagrees with the token's audience (e.g. the web client_id here vs a
    // mobile id_token minted for the native client) is rejected. The hint is
    // sufficient on its own.
    if id_token_hint.is_none() {
        if let Some(config) = get_config() {
            params.push(("client_id", config.client_id.clone()));
        }
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

/// The instance's OIDC client and the end_session_endpoint its discovery
/// document names (or `OIDC_LOGOUT_URI`), built once per process.
struct InstanceClient {
    client: OidcClientKind,
    end_session_endpoint: Option<String>,
}

/// Cached instance client. Built on first use, or by [`warm_client`] at hosted
/// startup, then reused. Concurrent first callers share one build; a failed
/// build leaves it empty, so the next caller tries again.
static OIDC_CLIENT: tokio::sync::OnceCell<InstanceClient> = tokio::sync::OnceCell::const_new();

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
    instance_client()
        .await
        .map(|cached| cached.client.clone_kind())
}

/// How long building the instance client may take. Every sign-in waiting on
/// the build waits this long at most when the issuer hangs.
const CLIENT_BUILD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

async fn instance_client() -> Result<&'static InstanceClient, String> {
    build_once(&OIDC_CLIENT, CLIENT_BUILD_TIMEOUT, || async {
        let config = OidcConfig::from_env()?;
        create_oidc_client(&config).await
    })
    .await
}

/// Fill `cell` with `build`, bounded by `limit`. A build that fails or times
/// out leaves the cell empty, so the next caller tries again.
async fn build_once<F, Fut>(
    cell: &tokio::sync::OnceCell<InstanceClient>,
    limit: std::time::Duration,
    build: F,
) -> Result<&InstanceClient, String>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<InstanceClient, String>>,
{
    cell.get_or_try_init(|| async {
        tokio::time::timeout(limit, build())
            .await
            .map_err(|_| format!("OIDC discovery timed out after {}s", limit.as_secs_f64()))?
    })
    .await
}

/// Whether the instance client is already built, so the next sign-in skips
/// discovery.
pub fn is_client_warm() -> bool {
    OIDC_CLIENT.initialized()
}

/// Build the instance client ahead of the first sign-in. A failure only logs:
/// the first sign-in then tries again.
pub async fn warm_client() {
    let started = std::time::Instant::now();
    match instance_client().await {
        Ok(_) => info!(
            elapsed_ms = started.elapsed().as_millis() as u64,
            "OIDC: client warmed"
        ),
        Err(e) => warn!(
            error = %e,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "OIDC: warming the client failed; the first sign-in will build it"
        ),
    }
}

/// Create an OIDC client from configuration: one discovery fetch (the
/// document, then its JWKS).
async fn create_oidc_client(config: &OidcConfig) -> Result<InstanceClient, String> {
    info!(
        "OIDC: Using auto-discovery from issuer: {}",
        config.issuer_url
    );

    let issuer = IssuerUrl::new(config.issuer_url.clone())
        .map_err(|e| format!("Invalid issuer URL: {e}"))?;

    let provider_metadata: ProviderMetadataWithLogout =
        ProviderMetadataWithLogout::discover_async(issuer, &*OIDC_HTTP_CLIENT)
            .await
            .map_err(|e| format!("OIDC discovery failed: {e}"))?;

    client_from_metadata(config, provider_metadata)
}

/// The instance client from a discovery document. `from_provider_metadata`
/// takes any additional metadata, so the logout-aware document feeds the
/// client directly.
fn client_from_metadata(
    config: &OidcConfig,
    provider_metadata: ProviderMetadataWithLogout,
) -> Result<InstanceClient, String> {
    let client_id = ClientId::new(config.client_id.clone());
    let client_secret = ClientSecret::new(config.client_secret.clone());
    let redirect_url = RedirectUrl::new(config.redirect_uri.clone())
        .map_err(|e| format!("Invalid redirect URI: {e}"))?;

    let end_session_endpoint = if let Some(end_session_url) = provider_metadata
        .additional_metadata()
        .end_session_endpoint
        .as_ref()
    {
        let url_string = end_session_url.url().to_string();
        info!("OIDC: Discovered end_session_endpoint: {}", url_string);
        Some(url_string)
    } else if let Some(ref logout_uri) = config.logout_uri {
        info!(
            "OIDC: Using manually configured logout URI as fallback: {}",
            logout_uri
        );
        Some(logout_uri.clone())
    } else {
        info!(
            "OIDC: Provider does not advertise end_session_endpoint and no \
             OIDC_LOGOUT_URI configured"
        );
        None
    };

    let client =
        CoreClient::from_provider_metadata(provider_metadata, client_id, Some(client_secret))
            .set_redirect_uri(redirect_url);

    Ok(InstanceClient {
        client: OidcClientKind::Discovered(client),
        end_session_endpoint,
    })
}

/// Generate PKCE challenge and verifier pair
pub fn generate_pkce() -> (PkceCodeChallenge, PkceCodeVerifier) {
    PkceCodeChallenge::new_random_sha256()
}

/// Generate a cryptographically random nonce
pub fn generate_nonce() -> Nonce {
    Nonce::new_random()
}

/// Ask the provider to check a credential again, even for a person it
/// already has signed in: a step-up confirmation.
#[derive(Debug, Clone, Copy)]
pub struct Reauthenticate<'a> {
    /// The address the person is confirming as, for the provider to show or
    /// prefill. Only a hint: the callback still checks who came back.
    pub login_hint: Option<&'a str>,
}

/// Generate the authorization URL for OIDC login
pub async fn generate_auth_url(
    _redirect_uri: Option<String>,
    _user_connection: Option<bool>,
    // Per-tenant OAuth callback override (hosted mode). Must equal the
    // redirect_uri later presented at token exchange. `None` uses the
    // client's configured `OIDC_REDIRECT_URI`.
    callback_redirect: Option<String>,
    reauthenticate: Option<Reauthenticate<'_>>,
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

    // Both: `prompt=login` asks for a credential, and a `max_age` obliges the
    // provider to return `auth_time`, which the callback checks against the
    // flow's start (so a stripped `prompt` is refused, not trusted).
    if let Some(reauth) = reauthenticate {
        auth_request = auth_request.add_prompt(CoreAuthPrompt::Login).set_max_age(
            std::time::Duration::from_secs(crate::services::step_up::PROVIDER_MAX_AGE_SECS),
        );
        if let Some(hint) = reauth.login_hint.filter(|h| !h.is_empty()) {
            auth_request = auth_request.set_login_hint(LoginHint::new(hint.to_string()));
        }
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
) -> Result<(OidcUserInfo, String), String> {
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

    // Serialised (compact-JWT) id_token, kept for a later RP-initiated logout
    // as the `id_token_hint`. The session row is where it comes to rest.
    let id_token_str = id_token.to_string();

    info!(
        "OIDC: Successfully authenticated user with sub: {}",
        user_info.sub
    );

    Ok((user_info, id_token_str))
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
/// expiry; `nonce` is the flow's replay protection, echoed by Azure. Returns
/// what the token says about the sign-in (`auth_time`, `amr`), each only when
/// the app registration asks for it as an optional claim.
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
) -> Result<crate::services::step_up::ProviderClaims, String> {
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
    let claims = id_token
        .claims(&verifier, &expected_nonce)
        .map_err(|e| format!("Microsoft ID token verification failed: {e}"))?;
    Ok(crate::services::step_up::ProviderClaims {
        auth_time: claims.auth_time(),
        amr: claims
            .auth_method_refs()
            .map(|amr| amr.iter().map(|m| m.as_str().to_owned()).collect()),
    })
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
    Ok(claims_to_user_info(claims))
}

/// The user info carried by verified ID token claims.
fn claims_to_user_info(claims: &CoreIdTokenClaims) -> OidcUserInfo {
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

    OidcUserInfo {
        sub,
        email,
        email_verified,
        name,
        preferred_username,
        given_name,
        family_name,
        picture,
        raw_claims,
    }
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

// ---------------------------------------------------------------------------
// Workspace providers (requester SSO)
//
// The functions above drive the instance's own OIDC client (env-configured,
// used for staff). Requester SSO uses a workspace's own provider, so these take
// the issuer and client explicitly. Discovery documents are cached per issuer.
// ---------------------------------------------------------------------------

/// How long a workspace provider's discovery document is reused.
const DISCOVERY_TTL: std::time::Duration = std::time::Duration::from_secs(3600);

static DISCOVERY_CACHE: once_cell::sync::Lazy<
    dashmap::DashMap<String, (std::time::Instant, CoreProviderMetadata)>,
> = once_cell::sync::Lazy::new(dashmap::DashMap::new);

/// On hosted, every URL the server will call for a workspace provider (the
/// issuer, and the token and key endpoints its discovery document names) must
/// be HTTPS on a public address under the egress policy. Hosted issuers are
/// built server-side from the Entra and Google presets; this is defence in
/// depth. Self-hosted skips it: the admin runs the machine, and an internal IdP
/// (Keycloak on the LAN) is a normal setup.
async fn check_egress(url: &str) -> Result<(), String> {
    if !crate::middleware::workspace_context::is_hosted() {
        return Ok(());
    }
    let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL {url}: {e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!("{url} must use https"));
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| format!("{url} has no host"))?;
    crate::utils::egress::resolve_and_validate(host, parsed.port_or_known_default().unwrap_or(443))
        .await
        .map(|_| ())
        .map_err(|e| format!("{host} isn't reachable under the egress policy: {e}"))
}

async fn discover(issuer_url: &str) -> Result<CoreProviderMetadata, String> {
    if let Some(hit) = DISCOVERY_CACHE.get(issuer_url) {
        if hit.0.elapsed() < DISCOVERY_TTL {
            return Ok(hit.1.clone());
        }
    }
    check_egress(issuer_url).await?;
    let issuer =
        IssuerUrl::new(issuer_url.to_string()).map_err(|e| format!("Invalid issuer URL: {e}"))?;
    let metadata = CoreProviderMetadata::discover_async(issuer, &*OIDC_HTTP_CLIENT)
        .await
        .map_err(|e| format!("OIDC discovery failed: {e}"))?;
    check_egress(metadata.jwks_uri().url().as_str()).await?;
    if let Some(token) = metadata.token_endpoint() {
        check_egress(token.url().as_str()).await?;
    }
    DISCOVERY_CACHE.insert(
        issuer_url.to_string(),
        (std::time::Instant::now(), metadata.clone()),
    );
    Ok(metadata)
}

/// A client for a workspace's provider, redirecting to `redirect_uri`.
pub async fn provider_client(
    issuer_url: &str,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
) -> Result<OidcClientKind, String> {
    let metadata = discover(issuer_url).await?;
    let redirect = RedirectUrl::new(redirect_uri.to_string())
        .map_err(|e| format!("Invalid redirect URI: {e}"))?;
    let client = CoreClient::from_provider_metadata(
        metadata,
        ClientId::new(client_id.to_string()),
        Some(ClientSecret::new(client_secret.to_string())),
    )
    .set_redirect_uri(redirect);
    Ok(OidcClientKind::Discovered(client))
}

/// Check a provider's discovery document can be fetched (the admin page's
/// "test" before saving).
pub async fn check_discovery(issuer_url: &str) -> Result<(), String> {
    discover(issuer_url).await.map(|_| ())
}

/// A workspace provider's issuer (as its discovery document states it) and
/// signing keys (a JWK set), for checking a token the provider issued straight
/// to one of our clients (the Teams tab's access token). `fresh` refetches when
/// the cached keys are over five minutes old: a token signed with a key we
/// haven't seen yet means the provider rotated.
pub async fn provider_keys(
    issuer_url: &str,
    fresh: bool,
) -> Result<(String, serde_json::Value), String> {
    if fresh {
        DISCOVERY_CACHE.remove_if(issuer_url, |_, hit| {
            hit.0.elapsed() > std::time::Duration::from_secs(300)
        });
    }
    let metadata = discover(issuer_url).await?;
    let keys = serde_json::to_value(metadata.jwks()).map_err(|e| e.to_string())?;
    Ok((metadata.issuer().to_string(), keys))
}

/// Tests: serve `issuer_url`'s discovery from memory, with `jwks` as its keys.
#[cfg(test)]
pub(crate) fn prime_discovery_for_test(issuer_url: &str, jwks: serde_json::Value) {
    let metadata: CoreProviderMetadata = serde_json::from_value(serde_json::json!({
        "issuer": issuer_url,
        "authorization_endpoint": format!("{issuer_url}/authorize"),
        "jwks_uri": format!("{issuer_url}/keys"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["pairwise"],
        "id_token_signing_alg_values_supported": ["RS256"],
    }))
    .expect("test discovery document");
    let metadata = metadata.set_jwks(serde_json::from_value(jwks).expect("test JWK set"));
    DISCOVERY_CACHE.insert(
        issuer_url.to_string(),
        (std::time::Instant::now(), metadata),
    );
}

/// Start a sign-in with a workspace provider. `state` is the caller's own
/// signed value, returned by the IdP on the callback.
pub fn begin_provider_login(client: &OidcClientKind, state: String) -> (String, OidcAuthData) {
    let (pkce_challenge, pkce_verifier) = generate_pkce();
    let nonce = generate_nonce();
    let nonce_secret = nonce.secret().to_string();
    let (url, _, _) = client
        .authorize_url(move || CsrfToken::new(state), move || nonce)
        .set_pkce_challenge(pkce_challenge)
        .add_scope(Scope::new("email".into()))
        .add_scope(Scope::new("profile".into()))
        .url();
    (
        url.to_string(),
        OidcAuthData {
            pkce_verifier: pkce_verifier.secret().to_string(),
            nonce: nonce_secret,
        },
    )
}

/// Finish a sign-in with a workspace provider: exchange the code and verify the
/// ID token (signature, issuer, audience, expiry, nonce). Returns the person
/// and what the token says about the sign-in.
pub async fn finish_provider_login(
    client: &OidcClientKind,
    code: &str,
    auth_data: &OidcAuthData,
) -> Result<(OidcUserInfo, crate::services::step_up::ProviderClaims), String> {
    let token_response = client
        .exchange_code(
            AuthorizationCode::new(code.to_string()),
            PkceCodeVerifier::new(auth_data.pkce_verifier.clone()),
            None,
        )
        .await?;
    let id_token = token_response
        .id_token()
        .ok_or_else(|| "No ID token in response".to_string())?;
    let claims = verify_id_token(client, id_token, &Nonce::new(auth_data.nonce.clone()))?;
    let mut info = claims_to_user_info(&claims);
    // Microsoft omits `email` for most work accounts unless an optional claim
    // is configured; `preferred_username` is then the UPN, whose domain the
    // tenant must have verified.
    if info.email.is_none() {
        info.email = info.preferred_username.clone().filter(|u| u.contains('@'));
    }
    // Entra's stable ids for the person (tenant and object id), which the core
    // claims type drops. The token was verified above.
    let payload = id_token
        .to_string()
        .split('.')
        .nth(1)
        .and_then(|p| {
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, p).ok()
        })
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok());
    if let (Some(payload), Some(raw)) = (payload, info.raw_claims.as_object_mut()) {
        for claim in ["tid", "oid"] {
            if let Some(v) = payload.get(claim).filter(|v| v.is_string()) {
                raw.insert(claim.to_string(), v.clone());
            }
        }
    }
    let sign_in = crate::services::step_up::ProviderClaims {
        auth_time: claims.auth_time(),
        amr: claims
            .auth_method_refs()
            .map(|amr| amr.iter().map(|m| m.as_str().to_owned()).collect()),
    };
    Ok((info, sign_in))
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

    fn config(issuer_url: &str, logout_uri: Option<&str>) -> OidcConfig {
        OidcConfig {
            client_id: "nosdesk-product".into(),
            client_secret: "secret".into(),
            redirect_uri: "https://app.example.test/api/auth/oauth/callback".into(),
            issuer_url: issuer_url.into(),
            display_name: "Nosdesk".into(),
            scopes: vec!["openid".into(), "email".into()],
            username_claim: "email".into(),
            logout_uri: logout_uri.map(str::to_string),
        }
    }

    fn discovery_document(issuer: &str, end_session: bool) -> serde_json::Value {
        let mut doc = json!({
            "issuer": issuer,
            "authorization_endpoint": format!("{issuer}/oauth2/auth"),
            "token_endpoint": format!("{issuer}/oauth2/token"),
            "jwks_uri": format!("{issuer}/.well-known/jwks.json"),
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
        });
        if end_session {
            doc["end_session_endpoint"] = json!(format!("{issuer}/oauth2/sessions/logout"));
        }
        doc
    }

    #[test]
    fn the_client_keeps_the_documents_end_session_endpoint() {
        let issuer = "https://id.example.test";
        let metadata = serde_json::from_value(discovery_document(issuer, true)).unwrap();
        let built = client_from_metadata(
            &config(issuer, Some("https://elsewhere.test/logout")),
            metadata,
        )
        .expect("client");
        assert_eq!(
            built.end_session_endpoint.as_deref(),
            Some("https://id.example.test/oauth2/sessions/logout")
        );
        let (url, _) = begin_provider_login(&built.client, "state".into());
        assert!(
            url.starts_with("https://id.example.test/oauth2/auth?"),
            "{url}"
        );
    }

    #[test]
    fn without_one_the_client_falls_back_to_the_configured_logout_uri() {
        let issuer = "https://id.example.test";
        let metadata = serde_json::from_value(discovery_document(issuer, false)).unwrap();
        let built = client_from_metadata(
            &config(issuer, Some("https://elsewhere.test/logout")),
            metadata,
        )
        .expect("client");
        assert_eq!(
            built.end_session_endpoint.as_deref(),
            Some("https://elsewhere.test/logout")
        );

        let metadata = serde_json::from_value(discovery_document(issuer, false)).unwrap();
        let built = client_from_metadata(&config(issuer, None), metadata).expect("client");
        assert_eq!(built.end_session_endpoint, None);
    }

    type Hits = std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, usize>>>;

    /// A local issuer serving a discovery document and an empty JWKS, counting
    /// requests per path. Returns its URL, the counts and the server task.
    async fn local_issuer() -> (String, Hits, tokio::task::JoinHandle<()>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let issuer = format!("http://{}", listener.local_addr().unwrap());
        let hits: Hits = Default::default();
        let document = discovery_document(&issuer, true).to_string();
        let server = {
            let hits = hits.clone();
            tokio::spawn(async move {
                loop {
                    let Ok((mut socket, _)) = listener.accept().await else {
                        return;
                    };
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        }
                    }
                    let request = String::from_utf8_lossy(&buf);
                    let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                    *hits.lock().unwrap().entry(path.clone()).or_default() += 1;
                    let body = match path.as_str() {
                        "/.well-known/openid-configuration" => document.clone(),
                        "/.well-known/jwks.json" => r#"{"keys":[]}"#.to_string(),
                        _ => String::new(),
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.shutdown().await;
                }
            })
        };
        (issuer, hits, server)
    }

    /// Building the client fetches the discovery document once and its JWKS
    /// once.
    #[tokio::test]
    async fn building_the_client_fetches_discovery_once() {
        use std::collections::HashMap;

        let (issuer, hits, server) = local_issuer().await;
        let built = create_oidc_client(&config(&issuer, None))
            .await
            .expect("client from the local issuer");
        server.abort();

        assert_eq!(
            built.end_session_endpoint,
            Some(format!("{issuer}/oauth2/sessions/logout"))
        );
        let hits = hits.lock().unwrap().clone();
        assert_eq!(
            hits,
            HashMap::from([
                ("/.well-known/openid-configuration".to_string(), 1),
                ("/.well-known/jwks.json".to_string(), 1),
            ])
        );
    }

    /// An issuer that accepts and never answers fails the build within the
    /// bound and leaves the cache empty, so a later build still succeeds.
    #[tokio::test]
    async fn a_hung_issuer_times_out_and_a_later_build_succeeds() {
        let hung = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hung_issuer = format!("http://{}", hung.local_addr().unwrap());
        // Accept connections and hold them open without a reply.
        let holder = tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((socket, _)) = hung.accept().await {
                held.push(socket);
            }
        });

        let cell = tokio::sync::OnceCell::new();
        let limit = std::time::Duration::from_millis(300);
        let started = std::time::Instant::now();
        let hung_config = config(&hung_issuer, None);
        let err = build_once(&cell, limit, || create_oidc_client(&hung_config))
            .await
            .err()
            .expect("a hung issuer fails the build");
        assert!(err.contains("timed out"), "{err}");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(3),
            "the build gave up at the bound, not later: {:?}",
            started.elapsed()
        );
        assert!(!cell.initialized(), "a timed-out build caches nothing");
        holder.abort();

        let (issuer, _, server) = local_issuer().await;
        let live_config = config(&issuer, None);
        let built = build_once(&cell, limit, || create_oidc_client(&live_config))
            .await
            .expect("the next build succeeds");
        server.abort();
        assert_eq!(
            built.end_session_endpoint,
            Some(format!("{issuer}/oauth2/sessions/logout"))
        );
        assert!(cell.initialized());
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
