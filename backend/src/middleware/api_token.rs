//! API Token Authentication Middleware
//!
//! Provides Bearer token authentication for programmatic API access.
//! Works alongside cookie-based authentication.

use actix_web::{
    body::MessageBody,
    dev::{ServiceRequest, ServiceResponse},
    middleware::Next,
    web, Error, HttpMessage,
};
use std::net::IpAddr;
use tracing::{debug, error, info, warn};

/// An RFC 6750 `401` in the API's `{error, code}` envelope, with its
/// `WWW-Authenticate: Bearer` challenge.
///
/// Per section 3, include `error="invalid_token"` (code `INVALID_TOKEN`) only
/// when a credential was presented and failed; send a bare `Bearer` challenge
/// (code `AUTH_REQUIRED`) when none was presented, so a client isn't told
/// *why* auth is needed when it offered nothing.
fn bearer_unauthorized(invalid_token: bool, message: &'static str) -> Error {
    if invalid_token {
        crate::errors::invalid_token_error(message)
    } else {
        crate::errors::unauthorized_error(message)
    }
}

use crate::db::Pool;
use crate::middleware::request_context;
use crate::models::Claims;
use crate::repository::api_tokens::{get_valid_api_token, hash_token, update_token_last_used};
use crate::sync::actor::ActorContext;
use crate::sync::session;

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

/// Try to authenticate request via Bearer token.
/// Returns Ok(Some(outcome)) if authenticated, Ok(None) if no Bearer
/// token, Err on auth failure.
pub fn try_bearer_auth(
    req: &ServiceRequest,
    pool: &web::Data<Pool>,
) -> Result<Option<Claims>, Error> {
    // Check for Bearer token
    let token = match extract_bearer_token(req) {
        Some(t) => t,
        None => return Ok(None), // No Bearer token, let cookie auth handle it
    };

    // Validate token format (should start with nsk_)
    if !token.starts_with("nsk_") {
        warn!(path = %req.path(), "Invalid API token format");
        return Err(bearer_unauthorized(true, "Invalid API token format"));
    }

    debug!(path = %req.path(), "Attempting Bearer token authentication");

    // Get database connection
    let mut conn = pool.get().map_err(|e| {
        error!("Database connection failed: {}", e);
        crate::errors::internal_error("Database connection failed")
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

    // cross-tenant: looks the bearer token up before any workspace is pinned, so a token from
    // another workspace gets the same 401 as an unknown one.
    let lookup_result = session::with_actor_bypass_context(&mut conn, &bypass_actor, |conn| {
        let api_token = get_valid_api_token(conn, &token_hash)?;
        // Active-only — soft-deleted users can't authenticate via
        // an API token even if the token itself is still valid.
        // F2C.2 H4.
        let user = crate::repository::users::find_active_by_uuid(&api_token.user_uuid, conn)?;
        let email =
            crate::repository::user_emails::get_user_emails_by_uuid(conn, &api_token.user_uuid)
                .ok()
                .and_then(|emails| emails.into_iter().find(|e| e.is_primary).map(|e| e.email))
                .unwrap_or_else(|| "unknown@example.com".to_string());

        // The workspace the token was minted in. Resolved here, inside the
        // bypass, because the request has no pin yet; it becomes the ceiling
        // the membership gate enforces. Archive state is deliberately ignored
        // (see `uuid_for_id`): an archived binding is still the token's
        // binding, and refusing it is the gate's job, not this lookup's.
        let workspace_uuid =
            crate::repository::workspaces::uuid_for_id(conn, api_token.workspace_id)?
                .ok_or(diesel::result::Error::NotFound)?;

        let refusal = ceiling_refusal(conn, &api_token, &user)?;

        // Update last_used_at inside the same bypass txn so the policy
        // doesn't reject the UPDATE; only for a token that is let in.
        if refusal.is_none() {
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
        }

        Ok::<_, diesel::result::Error>((api_token, user, email, workspace_uuid, refusal))
    });

    let (api_token, user, email, bound_workspace) = match lookup_result {
        Ok((api_token, user, email, workspace_uuid, None)) => {
            (api_token, user, email, workspace_uuid)
        }
        Ok((api_token, _, _, _, Some(refusal))) => {
            // Same answer as an unknown token, so a probe can't tell a
            // promoted holder from a revoked token; the log says which.
            match refusal {
                CeilingRefusal::MakerNotAdmin => warn!(
                    token_uuid = %api_token.uuid,
                    "API token refused: its maker is no longer an admin of its workspace"
                ),
                CeilingRefusal::RoleAboveCeiling => warn!(
                    token_uuid = %api_token.uuid,
                    "API token refused: its holder's workspace role is above the one it was made for"
                ),
                CeilingRefusal::PlatformRoleChanged => warn!(
                    token_uuid = %api_token.uuid,
                    "API token refused: its holder has a platform role it wasn't made with"
                ),
            }
            return Err(bearer_unauthorized(true, "Invalid or expired API token"));
        }
        Err(diesel::result::Error::NotFound) => {
            warn!(path = %req.path(), "API token not found or expired");
            return Err(bearer_unauthorized(true, "Invalid or expired API token"));
        }
        Err(e) => {
            error!("Error looking up API token: {}", e);
            return Err(crate::errors::internal_error("Authentication error"));
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
        platform_role: user.platform_role.clone(),
        scope,
        sid: None,
        // The token's binding, carried on the claims so every surface that
        // already checks `workspace_uuid` (the collab doc gate) applies it too.
        workspace_uuid: Some(bound_workspace),
        exp: (now + chrono::Duration::hours(24)).timestamp() as usize,
        iat: now.timestamp() as usize,
        file_path: None,
    };

    info!(
        user = %claims.sub,
        token_uuid = %api_token.uuid,
        "API token authentication successful"
    );

    // Mark the request as made with an API token, for the routes a token may
    // not use (managing API tokens).
    req.extensions_mut().insert(ApiTokenRequest {
        token_uuid: api_token.uuid,
    });

    Ok(Some(claims))
}

/// On a request authenticated with an API token (not a signed-in session):
/// which token. API tokens can't manage API tokens, or a token could mint
/// itself a successor that escapes its ceiling and its maker.
#[derive(Debug, Clone, Copy)]
pub struct ApiTokenRequest {
    pub token_uuid: uuid::Uuid,
}

/// Why a token may no longer act for its holder, or `None` when it may. One
/// made for someone else acts at no more than the roles it was made for
/// (`role_ceiling`, `platform_role_ceiling`), and only while its maker is
/// still an admin of its workspace: the maker holds its secret. Refusing it
/// here, the one place an API token authenticates, keeps every route, sync
/// stream and collab or SSE connection token from seeing a higher role. A
/// token made for oneself follows one's own role. Runs inside the lookup's
/// bypass transaction, since the request has no workspace pin yet.
fn ceiling_refusal(
    conn: &mut crate::db::DbConnection,
    token: &crate::models::ApiToken,
    holder: &crate::models::User,
) -> diesel::QueryResult<Option<CeilingRefusal>> {
    use crate::models::{PlatformRole, WorkspaceRole};
    use crate::repository::workspaces::membership;

    if token.created_by == token.user_uuid {
        return Ok(None);
    }
    let maker_is_admin =
        match crate::repository::users::find_active_by_uuid(&token.created_by, conn) {
            Ok(_) => membership(conn, token.workspace_id, token.created_by)?
                .is_some_and(|m| WorkspaceRole::from_db(&m.role) >= WorkspaceRole::Admin),
            Err(diesel::result::Error::NotFound) => false,
            Err(e) => return Err(e),
        };
    if !maker_is_admin {
        return Ok(Some(CeilingRefusal::MakerNotAdmin));
    }
    // A token from before ceilings were recorded and missed by the backfill
    // reads as made for a plain member.
    let ceiling = token
        .role_ceiling
        .as_deref()
        .map(WorkspaceRole::from_db)
        .unwrap_or(WorkspaceRole::Member);
    if let Some(m) = membership(conn, token.workspace_id, token.user_uuid)? {
        if WorkspaceRole::from_db(&m.role) > ceiling {
            return Ok(Some(CeilingRefusal::RoleAboveCeiling));
        }
    }
    // Platform roles aren't ordered: none, or the one it was made with.
    let live = PlatformRole::from_db(&holder.platform_role);
    let platform_ceiling = token
        .platform_role_ceiling
        .as_deref()
        .map(PlatformRole::from_db)
        .unwrap_or(PlatformRole::User);
    if live == PlatformRole::User || live == platform_ceiling {
        Ok(None)
    } else {
        Ok(Some(CeilingRefusal::PlatformRoleChanged))
    }
}

/// Why [`ceiling_refusal`] refused a token, for the log line.
enum CeilingRefusal {
    MakerNotAdmin,
    RoleAboveCeiling,
    PlatformRoleChanged,
}

/// Middleware function that supports both Bearer token and cookie authentication
/// This should replace cookie_auth_middleware in routes that need to support API tokens
/// The single authentication path shared by both auth middlewares.
///
/// Validates the request's credential, runs the workspace-membership gate, and
/// populates the request context. `accept_api_tokens` is the ONLY policy
/// difference between the two route families:
/// - `true` ([`dual_auth_middleware`] routes): a `nsk_` personal API token is
///   accepted.
/// - `false` ([`crate::middleware::cookie_auth_middleware`] routes): a `nsk_`
///   bearer is ignored and the request falls back to the cookie, so API tokens
///   can't reach cookie-only routes.
///
/// Session-JWT bearers (native/mobile, no cookie jar) and the access-token
/// cookie are accepted by both, via the SAME `authenticate_with_token` path, so
/// the active_sessions check + logout / reuse-detection apply uniformly.
pub(crate) async fn authenticate<B: MessageBody>(
    req: ServiceRequest,
    next: Next<B>,
    accept_api_tokens: bool,
) -> Result<ServiceResponse<B>, Error> {
    use crate::utils::jwt::JwtUtils;

    let pool = req
        .app_data::<web::Data<Pool>>()
        .ok_or_else(|| crate::errors::internal_error("Database pool not found"))?
        .clone();

    // A Bearer credential is either a personal API token (`nsk_…`, looked up in
    // the DB) or a session JWT (native/mobile). Branch on the unambiguous prefix;
    // a JWT never starts `nsk_`.
    if let Some(raw) = extract_bearer_token(&req) {
        if raw.starts_with("nsk_") {
            if accept_api_tokens {
                let claims = try_bearer_auth(&req, &pool)?
                    .ok_or_else(|| bearer_unauthorized(true, "Invalid API token"))?;
                let mut conn = pool
                    .get()
                    .map_err(|_| crate::errors::internal_error("Database connection failed"))?;
                // Membership gate plus the token's own workspace binding, so a
                // token minted in workspace A is refused against workspace B
                // even when its owner is a member of both. `try_bearer_auth`
                // always sets the binding.
                let bound = claims.workspace_uuid.ok_or_else(|| {
                    crate::errors::internal_error("API token has no workspace binding")
                })?;
                crate::middleware::cookie_auth::enforce_api_token_workspace(
                    &req, &mut conn, &claims, bound,
                )?;
                drop(conn);
                return finalize(req, next, claims, false).await;
            }
            // `nsk_` on a cookie-only route: ignore it and fall through to the
            // cookie below, so API tokens can't authenticate here.
        } else {
            // Session-JWT bearer.
            let mut conn = pool
                .get()
                .map_err(|_| crate::errors::internal_error("Database connection failed"))?;
            let (claims, _user) = JwtUtils::authenticate_with_token(&raw, &mut conn)
                .await
                .map_err(|err| {
                    warn!(error = ?err, "Bearer JWT auth: token validation failed");
                    bearer_unauthorized(true, "Invalid or expired token")
                })?;

            // Scope guard (MANDATORY): only genuine agent sessions carry scope
            // "full". Connection tokens ("sse" / "collab" / "portal") could
            // over-authorize if replayed as a bearer OR replanted as the access
            // cookie, so the same guard runs on the cookie path below.
            if claims.scope != "full" {
                warn!(path = %req.path(), scope = %claims.scope, "Rejected non-session JWT on bearer path");
                return Err(bearer_unauthorized(true, "Token not valid for this API"));
            }

            crate::middleware::cookie_auth::enforce_workspace_membership(&req, &mut conn, &claims)?;
            drop(conn);
            return finalize(req, next, claims, true).await;
        }
    }

    // No usable Bearer token — fall back to the access-token cookie (web).
    let mut conn = pool
        .get()
        .map_err(|_| crate::errors::internal_error("Database connection failed"))?;

    let token = req
        .cookie(&crate::utils::cookies::cookie_name(
            crate::utils::cookies::ACCESS_TOKEN_COOKIE,
        ))
        .ok_or_else(|| {
            warn!(path = %req.path(), "No access_token cookie and no session bearer");
            bearer_unauthorized(false, "Authentication required")
        })?;

    let (claims, _user) = JwtUtils::authenticate_with_token(token.value(), &mut conn)
        .await
        .map_err(|err| {
            warn!(error = ?err, "Cookie auth: token validation failed");
            bearer_unauthorized(true, "Invalid or expired token")
        })?;

    // Full-scope guard, mirroring the bearer path. Connection tokens
    // (sse/collab/portal) ride in query strings, so they leak into logs /
    // history / Referer; a leaked one must not be replantable as the access
    // cookie to reach Any-scoped or file routes. A legitimate access cookie is
    // always minted with scope "full".
    if claims.scope != "full" {
        warn!(path = %req.path(), scope = %claims.scope, "Rejected non-session JWT on cookie path");
        return Err(bearer_unauthorized(true, "Token not valid for this API"));
    }

    info!(user = %claims.sub, "Auth: user authenticated successfully");

    crate::middleware::cookie_auth::enforce_workspace_membership(&req, &mut conn, &claims)?;
    drop(conn);
    finalize(req, next, claims, true).await
}

/// Common tail: release the pooled connection BEFORE running the handler
/// (holding it across `next.call()` pins one connection per request, and since
/// handlers acquire their own, a concurrent burst near the pool size deadlocks),
/// then publish the actor context + claims and run the handler. The caller drops
/// `conn` before calling this. A `session` token's response says how long it
/// has left ([`crate::utils::jwt::ACCESS_EXPIRES_IN_HEADER`]); an API token's
/// doesn't, since nothing refreshes one.
async fn finalize<B: MessageBody>(
    req: ServiceRequest,
    next: Next<B>,
    claims: Claims,
    session: bool,
) -> Result<ServiceResponse<B>, Error> {
    // Scope enforcement runs here, at the one point every authenticated
    // request passes, rather than in a middleware each new scope has to
    // remember to stack alongside the auth wrap. `/api/collaboration` is what
    // forgetting looked like: it wrapped `dual_auth_middleware` but not the
    // scope middleware, so a narrowed token reached
    // `POST /api/collaboration/token` and traded itself for an unrestricted
    // `collab` JWT. The policy already returned `Full` for that path; it was
    // simply never asked.
    if !crate::middleware::token_scope::claims_may_call(&claims, req.method(), req.path()) {
        return Err(crate::errors::forbidden_error(
            "API token scope does not permit this request",
        ));
    }
    request_context::populate(&req, &claims);
    let expires_at = claims.exp;
    req.extensions_mut().insert(claims);
    let mut res = next.call(req).await?;
    if session {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as usize)
            .unwrap_or(0);
        res.headers_mut().insert(
            actix_web::http::header::HeaderName::from_static(
                crate::utils::jwt::ACCESS_EXPIRES_IN_HEADER,
            ),
            actix_web::http::header::HeaderValue::from(expires_at.saturating_sub(now)),
        );
    }
    Ok(res)
}

/// Cookie OR session-JWT bearer OR `nsk_` API token. For routes that allow
/// programmatic API tokens.
pub async fn dual_auth_middleware(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    authenticate(req, next, true).await
}

/// The file serve routes: [`dual_auth_middleware`], plus a file token in the
/// query string for a media element that loads a file straight from the server
/// and can't send a header (the app's `<video>`, which has no cookie for the
/// server's origin either).
///
/// The token is taken only for a GET or HEAD on a serve route (see
/// [`crate::middleware::token_scope::is_file_read`]) that carries no bearer and
/// no access cookie; anything else authenticates exactly as `dual_auth` does.
/// It must be a `files` token: an `sse` or `collab` token is refused here, and a
/// file token is refused on every other route because only `full` claims pass
/// the bearer and cookie paths. The token names one file and its workspace: a
/// request for any other path is refused, and the handler holds the file to the
/// workspace and runs the same per-file checks a session gets.
pub async fn file_auth_middleware(
    req: ServiceRequest,
    next: Next<impl MessageBody>,
) -> Result<ServiceResponse<impl MessageBody>, Error> {
    let file_token = if crate::middleware::token_scope::is_file_read(req.method(), req.path())
        && extract_bearer_token(&req).is_none()
        && req
            .cookie(&crate::utils::cookies::cookie_name(
                crate::utils::cookies::ACCESS_TOKEN_COOKIE,
            ))
            .is_none()
    {
        web::Query::<FileTokenQuery>::from_query(req.query_string())
            .ok()
            .and_then(|q| q.into_inner().token)
            .filter(|t| !t.is_empty())
    } else {
        None
    };
    let Some(token) = file_token else {
        return authenticate(req, next, true).await;
    };

    use crate::utils::jwt::{JwtUtils, FILES_SCOPE};
    let pool = req
        .app_data::<web::Data<Pool>>()
        .ok_or_else(|| crate::errors::internal_error("Database pool not found"))?
        .clone();
    let mut conn = pool
        .get()
        .map_err(|_| crate::errors::internal_error("Database connection failed"))?;
    let (claims, _user) = JwtUtils::authenticate_with_token(&token, &mut conn)
        .await
        .map_err(|err| {
            warn!(error = ?err, "File token auth: token validation failed");
            bearer_unauthorized(true, "Invalid or expired token")
        })?;
    if claims.scope != FILES_SCOPE {
        warn!(path = %req.path(), scope = %claims.scope, "Rejected non-file token on a file route");
        return Err(bearer_unauthorized(true, "Token not valid for this API"));
    }
    let Some(token_workspace) = claims.workspace_uuid else {
        return Err(bearer_unauthorized(true, "Token not valid for this API"));
    };
    if !claims
        .file_path
        .as_deref()
        .is_some_and(|file| same_file_path(file, req.path()))
    {
        warn!(path = %req.path(), "Rejected a file token minted for another file");
        return Err(bearer_unauthorized(true, "Token not valid for this API"));
    }

    // Membership in the token's workspace, as the SSE and collab connections
    // check theirs. The handler then holds the file to this workspace through
    // the token's binding (`authorize_located`).
    use crate::middleware::cookie_auth::{GateOutcome, UnresolvedPolicy, WorkspaceCarrier};
    if let GateOutcome::Scoped(ctx) = crate::middleware::cookie_auth::resolve_pin_and_gate(
        &mut conn,
        &claims,
        WorkspaceCarrier::ConnectionToken {
            token_workspace: Some(token_workspace),
            req: req.request(),
        },
        UnresolvedPolicy::Deny,
    )? {
        req.extensions_mut().insert(ctx);
    }
    drop(conn);

    let mut res = finalize(req, next, claims, false).await?;
    // The page that asked for this file is on another origin (the app's), and
    // the app-wide `same-origin` policy would stop the media element reading
    // it. Safe to relax here: the response was authorised by the token in the
    // URL, not by anything the browser attaches on its own.
    res.headers_mut().insert(
        actix_web::http::header::HeaderName::from_static("cross-origin-resource-policy"),
        actix_web::http::header::HeaderValue::from_static("cross-origin"),
    );
    Ok(res)
}

/// Whether a file token minted for `claimed` opens `requested`, compared
/// percent-decoded: the page that minted it and the player that loads it may
/// encode a space or a non-ASCII name differently (`clip café.mov`,
/// `clip%20caf%C3%A9.mov`), and both name the same file. Decoded to bytes, so a
/// sequence that isn't UTF-8 is compared as it is rather than refused.
fn same_file_path(claimed: &str, requested: &str) -> bool {
    urlencoding::decode_binary(claimed.as_bytes())
        == urlencoding::decode_binary(requested.as_bytes())
}

/// The query string of a file route loaded with a file token.
#[derive(serde::Deserialize)]
struct FileTokenQuery {
    token: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::same_file_path;

    #[test]
    fn a_file_path_matches_however_it_is_encoded() {
        let raw = "/api/files/temp/0190_clip café.mov";
        for requested in [
            raw,
            "/api/files/temp/0190_clip%20caf%C3%A9.mov",
            "/api/files/temp/0190_clip%20caf%c3%a9.mov",
            "/api/files/temp/0190%5Fclip%20caf%C3%A9.mov",
        ] {
            assert!(same_file_path(raw, requested), "{requested}");
            assert!(same_file_path(requested, raw), "{requested}");
        }
        assert!(!same_file_path(raw, "/api/files/temp/0190_clip+café.mov"));
        assert!(!same_file_path(raw, "/api/files/temp/0190_other.mov"));
        assert!(!same_file_path(raw, "/api/files/temp/0190_clip café.mov/"));
    }
}
