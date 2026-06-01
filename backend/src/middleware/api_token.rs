//! API Token Authentication Middleware
//!
//! Provides Bearer token authentication for programmatic API access.
//! Works alongside cookie-based authentication.

use actix_web::{dev::ServiceRequest, web, Error, HttpMessage};
use std::net::IpAddr;
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::middleware::request_context;
use crate::models::Claims;
use crate::repository::api_tokens::{get_valid_api_token, hash_token, update_token_last_used};
use crate::sync::actor::ActorContext;
use crate::sync::session;

/// Marker struct to indicate request was authenticated via API token.
/// Used by CSRF middleware to skip validation for API token requests
/// and by the platform-scope extractor below to gate internal-only
/// provisioning endpoints.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct ApiTokenAuth {
    pub token_uuid: uuid::Uuid,
    /// Mirrors `api_tokens.is_platform_scoped`. True only for tokens
    /// minted operator-side for the control plane → product
    /// provisioning callbacks. Handlers that require it must use
    /// the `PlatformScope` extractor.
    pub is_platform_scoped: bool,
}

/// Extract Bearer token from Authorization header
pub fn extract_bearer_token(req: &ServiceRequest) -> Option<String> {
    req.headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .and_then(|auth| auth.strip_prefix("Bearer "))
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

/// Extract client IP from request. Delegates to the central
/// `utils::client_ip` helper so this middleware obeys the same
/// `TRUSTED_PROXIES` gate as every other rate-limit / audit
/// surface. Previously this read `X-Forwarded-For` unconditionally,
/// which let an attacker on a direct connection forge any source
/// IP they wanted.
fn extract_client_ip(req: &ServiceRequest) -> Option<IpAddr> {
    crate::utils::client_ip::from_service_request(req)
}

/// Outcome of a successful Bearer auth: the synthesised Claims plus
/// the `is_platform_scoped` mirror from the api_tokens row. The flag
/// can't ride on `Claims` (which is a JWT body shape and goes over
/// the wire for cookie auth) so we surface it separately and the
/// middleware stitches it into `ApiTokenAuth` for downstream
/// extractors.
pub struct BearerAuthOutcome {
    pub claims: Claims,
    pub is_platform_scoped: bool,
}

/// Try to authenticate request via Bearer token.
/// Returns Ok(Some(outcome)) if authenticated, Ok(None) if no Bearer
/// token, Err on auth failure.
pub fn try_bearer_auth(
    req: &ServiceRequest,
    pool: &web::Data<Pool>,
) -> Result<Option<BearerAuthOutcome>, Error> {
    // Check for Bearer token
    let token = match extract_bearer_token(req) {
        Some(t) => t,
        None => return Ok(None), // No Bearer token, let cookie auth handle it
    };

    // Validate token format (should start with nsk_)
    if !token.starts_with("nsk_") {
        warn!(path = %req.path(), "Invalid API token format");
        return Err(actix_web::error::ErrorUnauthorized(
            "Invalid API token format",
        ));
    }

    debug!(path = %req.path(), "Attempting Bearer token authentication");

    // Get database connection
    let mut conn = pool.get().map_err(|e| {
        error!("Database connection failed: {}", e);
        actix_web::error::ErrorInternalServerError("Database connection failed")
    })?;

    // Bearer-token auth runs after the workspace-context middleware
    // but before the request has any user-actor context, so the
    // api_tokens lookup (RLS-enabled) needs an explicit bypass: a
    // request to subdomain X with a token belonging to workspace Y
    // shouldn't silently fail with "invalid token" — we want the
    // token-not-found and the cross-workspace cases to share one
    // 401 response. with_actor_bypass_context elevates to
    // nosdesk_admin (BYPASSRLS) for the duration of the lookup;
    // the user/email reads are non-RLS but are wrapped here too
    // for atomicity and so the bypass auto-clears at txn commit.
    let token_hash = hash_token(&token);
    let bypass_actor = ActorContext::system("middleware:api_token");

    let lookup_result = session::with_actor_bypass_context(&mut conn, &bypass_actor, |conn| {
        let api_token = get_valid_api_token(conn, &token_hash)?;
        // Active-only — soft-deleted users can't authenticate via
        // an API token even if the token itself is still valid.
        // F2C.2 H4 (see docs/auth-convergence.md).
        let user = crate::repository::users::find_active_by_uuid(&api_token.user_uuid, conn)?;
        let email =
            crate::repository::user_emails::get_user_emails_by_uuid(conn, &api_token.user_uuid)
                .ok()
                .and_then(|emails| emails.into_iter().find(|e| e.is_primary).map(|e| e.email))
                .unwrap_or_else(|| "unknown@example.com".to_string());

        // Update last_used_at inside the same bypass txn so the
        // policy doesn't reject the UPDATE.
        let client_ip = extract_client_ip(req);
        let ip_network = client_ip.map(|ip| {
            use ipnetwork::IpNetwork;
            match ip {
                IpAddr::V4(v4) => IpNetwork::V4(ipnetwork::Ipv4Network::from(v4)),
                IpAddr::V6(v6) => IpNetwork::V6(ipnetwork::Ipv6Network::from(v6)),
            }
        });
        if let Err(e) = update_token_last_used(conn, api_token.id, ip_network) {
            warn!("Failed to update token last_used_at: {}", e);
        }

        Ok::<_, diesel::result::Error>((api_token, user, email))
    });

    let (api_token, user, email) = match lookup_result {
        Ok(triple) => triple,
        Err(diesel::result::Error::NotFound) => {
            warn!(path = %req.path(), "API token not found or expired");
            return Err(actix_web::error::ErrorUnauthorized(
                "Invalid or expired API token",
            ));
        }
        Err(e) => {
            error!("Error looking up API token: {}", e);
            return Err(actix_web::error::ErrorInternalServerError(
                "Authentication error",
            ));
        }
    };

    // Project the token's stored scopes into a single space-
    // separated `scope` string (OAuth2 / RFC 6749 convention) so
    // downstream scope-checking middleware can split it without a
    // schema-aware parse. Token-stored scopes is `Vec<Option<String>>`
    // (Diesel's quirky nullable-array shape); we flatten the Some()
    // variants and drop nulls. Empty / unset scopes default to
    // "full" — matching the prior behaviour for tokens created
    // before the scope column was populated. Tokens issued WITH
    // explicit scopes now carry only those scopes, closing the gap
    // where any API token previously bypassed scope enforcement.
    let scope = api_token
        .scopes
        .as_ref()
        .map(|raw| {
            let collected: Vec<&str> = raw
                .iter()
                .filter_map(|s| s.as_deref())
                .filter(|s| !s.is_empty())
                .collect();
            if collected.is_empty() {
                "full".to_string()
            } else {
                collected.join(" ")
            }
        })
        .unwrap_or_else(|| "full".to_string());

    // Create claims from API token (no session — API tokens don't have sessions)
    let now = chrono::Utc::now();
    let claims = Claims {
        sub: api_token.user_uuid.to_string(),
        name: user.name,
        email,
        role: user.role.as_str().to_string(),
        scope,
        sid: None,
        exp: (now + chrono::Duration::hours(24)).timestamp() as usize,
        iat: now.timestamp() as usize,
    };

    info!(
        user = %claims.sub,
        token_uuid = %api_token.uuid,
        is_platform_scoped = api_token.is_platform_scoped,
        "API token authentication successful"
    );

    Ok(Some(BearerAuthOutcome {
        claims,
        is_platform_scoped: api_token.is_platform_scoped,
    }))
}

/// Middleware function that supports both Bearer token and cookie authentication
/// This should replace cookie_auth_middleware in routes that need to support API tokens
pub async fn dual_auth_middleware(
    req: actix_web::dev::ServiceRequest,
    next: actix_web::middleware::Next<impl actix_web::body::MessageBody>,
) -> Result<actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>, Error> {
    let pool = req
        .app_data::<web::Data<Pool>>()
        .ok_or_else(|| actix_web::error::ErrorInternalServerError("Database pool not found"))?
        .clone();

    // Try Bearer token authentication first
    match try_bearer_auth(&req, &pool)? {
        Some(BearerAuthOutcome {
            claims,
            is_platform_scoped,
        }) => {
            // Item U: workspace membership 403 gate. Bearer-token
            // requests go through the same check as cookie-auth so
            // an API token issued in workspace A can't be used to
            // probe workspace B's subdomain.
            //
            // Platform-scoped tokens skip this check by design —
            // they're cross-workspace (M5 provisioning callbacks).
            // The PlatformScope extractor is what gates actual
            // platform-only endpoints; the membership skip here
            // just means platform tokens don't need to belong to
            // the target subdomain's workspace.
            if !is_platform_scoped {
                let mut conn = pool.get().map_err(|_| {
                    actix_web::error::ErrorInternalServerError("Database connection failed")
                })?;
                crate::middleware::cookie_auth::enforce_workspace_membership(
                    &req, &mut conn, &claims,
                )?;
            }
            req.extensions_mut().insert(ApiTokenAuth {
                token_uuid: uuid::Uuid::parse_str(&claims.sub).unwrap_or_default(),
                is_platform_scoped,
            });
            request_context::populate(&req, &claims);
            req.extensions_mut().insert(claims);
            return next.call(req).await;
        }
        None => {
            // No Bearer token, fall through to cookie auth.
        }
    }

    // Fall back to cookie-based authentication.
    use crate::utils::jwt::JwtUtils;

    let mut conn = pool
        .get()
        .map_err(|_| actix_web::error::ErrorInternalServerError("Database connection failed"))?;

    let token = req
        .cookie(crate::utils::cookies::ACCESS_TOKEN_COOKIE)
        .ok_or_else(|| {
            warn!(path = %req.path(), "No access_token cookie and no Bearer token");
            actix_web::error::ErrorUnauthorized("Authentication required")
        })?;

    let (claims, _user) = JwtUtils::authenticate_with_token(token.value(), &mut conn)
        .await
        .map_err(|err| {
            error!(error = ?err, "Cookie auth: token validation failed");
            actix_web::error::ErrorUnauthorized("Invalid or expired token")
        })?;

    info!(user = %claims.sub, "Cookie auth: user authenticated successfully");

    crate::middleware::cookie_auth::enforce_workspace_membership(&req, &mut conn, &claims)?;

    request_context::populate(&req, &claims);
    req.extensions_mut().insert(claims);

    next.call(req).await
}

// =====================================================================
// PlatformScope extractor
// =====================================================================
//
// Add `_: PlatformScope` to a handler's signature to gate it on a
// platform-scoped api_token. The dual_auth_middleware must have run
// first and inserted ApiTokenAuth into request extensions; if the
// request is unauthenticated, used cookie auth, or used a user-bound
// API token, the extractor returns 403.
//
// The handler is still responsible for using `with_actor_bypass_context`
// for its writes — the extractor only enforces that the caller is
// allowed to do so. The bypass elevates the connection to
// `nosdesk_admin` (BYPASSRLS) inside a transaction, which is what
// makes the cross-workspace reach auditable at the Postgres layer
// (matches the existing pattern used by admin / system-actor flows).
//
// Per the M5 product-side handoff (Task 1) + D8.6 in the control-plane
// account-billing-architecture doc.

use actix_web::{FromRequest, HttpRequest};
use std::future::{ready, Ready};

/// Zero-sized extractor: presence in a handler signature gates the
/// route on a platform-scoped API token. Construction has no fields
/// — successful extraction is the signal.
pub struct PlatformScope;

impl FromRequest for PlatformScope {
    type Error = Error;
    type Future = Ready<Result<Self, Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_web::dev::Payload) -> Self::Future {
        let is_platform = req
            .extensions()
            .get::<ApiTokenAuth>()
            .map(|t| t.is_platform_scoped)
            .unwrap_or(false);
        if is_platform {
            ready(Ok(PlatformScope))
        } else {
            warn!(
                path = %req.path(),
                method = %req.method(),
                "platform-scope required, request lacks platform-scoped api_token"
            );
            ready(Err(actix_web::error::ErrorForbidden(
                "platform-scoped api_token required",
            )))
        }
    }
}
