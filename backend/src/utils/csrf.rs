use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::Method,
    Error,
};
use futures::future::LocalBoxFuture;
use rand::Rng;
use std::future::{ready, Ready};

/// Generate a cryptographically secure CSRF token (32 bytes = 64 hex chars)
pub fn generate_csrf_token() -> String {
    let token_bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(token_bytes)
}

/// Validate a CSRF token by comparing it to the expected value
/// The CSRF cookie name for a request path. Authenticated portal requests
/// (`/api/portal/...`, excluding the public `/api/portal/auth/` sign-in routes
/// which skip CSRF entirely) carry the portal session's own `portal_csrf`
/// cookie; everything else uses the agent `csrf_token` cookie. Selecting by
/// surface keeps the double-submit check honest across the two session realms.
pub fn csrf_cookie_for_path(path: &str) -> &'static str {
    if path.starts_with("/api/portal/") {
        crate::utils::cookies::PORTAL_CSRF_TOKEN_COOKIE
    } else {
        crate::utils::cookies::CSRF_TOKEN_COOKIE
    }
}

pub fn validate_csrf_token(provided: &str, expected: &str) -> bool {
    // Use constant-time comparison to prevent timing attacks
    use constant_time_eq::constant_time_eq;
    constant_time_eq(provided.as_bytes(), expected.as_bytes())
}

// === CSRF MIDDLEWARE ===

/// CSRF protection middleware using Double Submit Cookie pattern
/// Validates CSRF tokens for state-changing requests (POST, PUT, DELETE, PATCH)
pub struct CsrfProtection;

impl<S, B> Transform<S, ServiceRequest> for CsrfProtection
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = CsrfProtectionMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(CsrfProtectionMiddleware { service }))
    }
}

pub struct CsrfProtectionMiddleware<S> {
    service: S,
}

impl<S, B> Service<ServiceRequest> for CsrfProtectionMiddleware<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        // Only validate CSRF for state-changing methods
        let needs_csrf = matches!(
            req.method(),
            &Method::POST | &Method::PUT | &Method::DELETE | &Method::PATCH
        );

        if !needs_csrf {
            // Skip CSRF validation for safe methods (GET, HEAD, OPTIONS)
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res)
            });
        }

        // Check if this request is authenticated via Bearer token (API token)
        // API tokens don't need CSRF validation as they can't be used in CSRF attacks
        let has_bearer_token = req
            .headers()
            .get("Authorization")
            .and_then(|h| h.to_str().ok())
            .map(|auth| auth.starts_with("Bearer "))
            .unwrap_or(false);

        if has_bearer_token {
            tracing::debug!(
                "🔒 CSRF: Skipping validation for Bearer token request to {}",
                req.path()
            );
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res)
            });
        }

        // Check if this is a public endpoint that doesn't require CSRF
        let path = req.path();
        let is_public_endpoint = path == "/api/auth/login"
            || path == "/api/auth/logout"
            || path == "/api/auth/refresh"
            || path == "/api/auth/mfa-login"
            || path == "/api/auth/mfa-setup-login"
            || path == "/api/auth/mfa-enable-login"
            || path == "/api/auth/passkey-setup-login/start"
            || path == "/api/auth/passkey-setup-login/finish"
            || path == "/api/auth/microsoft"
            || path.starts_with("/api/auth/microsoft/callback")
            || path == "/api/auth/oauth/authorize"
            || path == "/api/auth/oauth/callback"
            || path == "/api/auth/oauth/logout"
            || path == "/api/auth/setup/admin"
            || path == "/api/auth/setup/status"
            || path.starts_with("/api/auth/setup/restore/")
            || path == "/api/auth/register"
            || path.starts_with("/api/auth/password-reset/")
            || path.starts_with("/api/auth/invitation/")
            || path == "/api/auth/passkeys/login/start"
            || path == "/api/auth/passkeys/login/finish"
            || path == "/api/debug/frontend-logs"
            // Public guest surface: unauthenticated, no session cookie, so
            // no CSRF surface to protect. Rate limiting is handled by the
            // scope's dedicated limiter + per-handler Redis counters.
            || path.starts_with("/api/public/")
            // Public portal sign-in (magic-link request + callback):
            // unauthenticated, no portal session cookie yet, so nothing to
            // forge against. The authenticated portal API below validates
            // against the portal_csrf cookie.
            || path.starts_with("/api/portal/auth/")
            // CSP violation reports are sent by browsers without
            // credentials, so there's no session to forge against.
            // Browsers also don't include arbitrary headers, so we
            // can't require an X-CSRF-Token here. Reports are
            // rate-limited and deduplicated server-side.
            || path == "/api/csp-report"
            // Inbound-email webhook: AWS SNS posts here server-to-server with
            // no session cookie, so there's no CSRF surface. Authentication is
            // the SNS message signature, verified inside the handler.
            || path == "/api/inbound/email";

        if is_public_endpoint {
            // Skip CSRF validation for public auth endpoints
            let fut = self.service.call(req);
            return Box::pin(async move {
                let res = fut.await?;
                Ok(res)
            });
        }

        // Extract CSRF token from header
        let header_token = req
            .headers()
            .get("X-CSRF-Token")
            .and_then(|h| h.to_str().ok())
            .map(|s| s.to_string());

        // Extract CSRF token from cookie. The portal is a separate session
        // realm with its own cookie, so pick the cookie that matches the
        // surface this request belongs to.
        let cookie_token = req
            .cookie(csrf_cookie_for_path(path))
            .map(|c| c.value().to_string());

        // Log only a short prefix, taken char-wise so an attacker-
        // supplied header shorter than 10 bytes (or with a multi-byte
        // char straddling the boundary) can't panic the middleware via
        // a byte-index slice. See security-audit-2026-06.
        let prefix = |t: &String| t.chars().take(8).collect::<String>();
        tracing::debug!(
            "🔒 CSRF Check for {}: header={:?}, cookie={:?}",
            path,
            header_token.as_ref().map(prefix),
            cookie_token.as_ref().map(prefix)
        );

        // Validate CSRF token
        match (header_token, cookie_token) {
            (Some(header), Some(cookie)) => {
                if !validate_csrf_token(&header, &cookie) {
                    tracing::error!("🔒 CSRF validation failed for {}: tokens don't match", path);
                    return Box::pin(async move {
                        Err(actix_web::error::ErrorForbidden("Invalid CSRF token"))
                    });
                }
                tracing::debug!("🔒 CSRF validation passed for {}", path);
            }
            (None, Some(_)) => {
                tracing::warn!("🔒 CSRF failed for {}: Missing X-CSRF-Token header", path);
                return Box::pin(async move {
                    Err(actix_web::error::ErrorForbidden(
                        "CSRF token required in header",
                    ))
                });
            }
            (Some(_), None) => {
                tracing::warn!("🔒 CSRF failed for {}: Missing csrf_token cookie", path);
                return Box::pin(async move {
                    Err(actix_web::error::ErrorForbidden(
                        "CSRF token required in cookie",
                    ))
                });
            }
            (None, None) => {
                tracing::warn!(
                    "🔒 CSRF failed for {}: Both header and cookie missing",
                    path
                );
                return Box::pin(async move {
                    Err(actix_web::error::ErrorForbidden("CSRF token required"))
                });
            }
        }

        // CSRF validation passed, continue with request
        let fut = self.service.call(req);
        Box::pin(async move {
            let res = fut.await?;
            Ok(res)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_token_is_64_hex_chars() {
        let token = generate_csrf_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generated_tokens_are_unique() {
        let t1 = generate_csrf_token();
        let t2 = generate_csrf_token();
        assert_ne!(t1, t2);
    }

    #[test]
    fn validate_matching_tokens() {
        assert!(validate_csrf_token("abc123", "abc123"));
    }

    #[test]
    fn portal_paths_use_the_portal_csrf_cookie() {
        use crate::utils::cookies::{CSRF_TOKEN_COOKIE, PORTAL_CSRF_TOKEN_COOKIE};
        assert_eq!(
            csrf_cookie_for_path("/api/portal/tickets"),
            PORTAL_CSRF_TOKEN_COOKIE
        );
        assert_eq!(
            csrf_cookie_for_path("/api/portal/tickets/5/comments"),
            PORTAL_CSRF_TOKEN_COOKIE
        );
        // Agent + everything else keeps the agent cookie.
        assert_eq!(csrf_cookie_for_path("/api/tickets"), CSRF_TOKEN_COOKIE);
        assert_eq!(csrf_cookie_for_path("/api/auth/me"), CSRF_TOKEN_COOKIE);
    }

    #[test]
    fn validate_mismatched_tokens() {
        assert!(!validate_csrf_token("abc123", "xyz789"));
    }

    #[test]
    fn validate_empty_tokens() {
        assert!(validate_csrf_token("", ""));
    }

    #[test]
    fn validate_different_lengths() {
        assert!(!validate_csrf_token("short", "longer_token"));
    }
}
