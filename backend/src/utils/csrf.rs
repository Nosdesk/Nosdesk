use rand::Rng;
use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    Error, http::Method,
};
use futures::future::LocalBoxFuture;
use std::future::{ready, Ready};

/// Generate a cryptographically secure CSRF token (32 bytes = 64 hex chars)
pub fn generate_csrf_token() -> String {
    let token_bytes: [u8; 32] = rand::thread_rng().gen();
    hex::encode(token_bytes)
}

/// Validate a CSRF token by comparing it to the expected value
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
            tracing::debug!("🔒 CSRF: Skipping validation for Bearer token request to {}", req.path());
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
            || path == "/api/auth/mfa-login"
            || path == "/api/auth/mfa-setup-login"
            || path == "/api/auth/mfa-enable-login"
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
            || path.starts_with("/api/auth/mfa-reset/")
            || path.starts_with("/api/auth/invitation/")
            || path == "/api/debug/frontend-logs";

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

        // Extract CSRF token from cookie
        let cookie_token = req
            .cookie(crate::utils::cookies::CSRF_TOKEN_COOKIE)
            .map(|c| c.value().to_string());

        tracing::debug!("🔒 CSRF Check for {}: header={:?}, cookie={:?}",
            path,
            header_token.as_ref().map(|t| &t[..10]),
            cookie_token.as_ref().map(|t| &t[..10])
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
                    Err(actix_web::error::ErrorForbidden("CSRF token required in header"))
                });
            }
            (Some(_), None) => {
                tracing::warn!("🔒 CSRF failed for {}: Missing csrf_token cookie", path);
                return Box::pin(async move {
                    Err(actix_web::error::ErrorForbidden("CSRF token required in cookie"))
                });
            }
            (None, None) => {
                tracing::warn!("🔒 CSRF failed for {}: Both header and cookie missing", path);
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
