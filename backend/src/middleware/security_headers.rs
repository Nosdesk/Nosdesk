use actix_web::{
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::header,
    Error,
};
use futures::future::LocalBoxFuture;
use std::future::{ready, Ready};

/// Security headers middleware
/// Adds essential security headers to all responses following OWASP best practices
pub struct SecurityHeaders;

impl SecurityHeaders {
    /// Get Content-Security-Policy header value based on environment
    /// Development has relaxed rules for hot reload, production is strict
    fn get_csp_header() -> String {
        let env = std::env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "development".to_string())
            .to_lowercase();

        if env == "production" {
            // Strict CSP for production
            concat!(
                "default-src 'self'; ",
                "script-src 'self'; ",
                "worker-src 'self' blob:; ", // Allow web workers from blob URLs
                "style-src 'self' 'unsafe-inline'; ", // unsafe-inline needed for some frameworks
                "img-src 'self' data: https:; ",
                "font-src 'self' data:; ",
                "connect-src 'self' blob:; ", // blob: for voice note uploads
                "media-src 'self' blob:; ", // blob: for audio/video playback (voice notes)
                "frame-ancestors 'none'; ",
                "base-uri 'self'; ",
                "form-action 'self'"
            )
            .to_string()
        } else {
            // Relaxed CSP for development (allows Vue dev server hot reload)
            concat!(
                "default-src 'self'; ",
                "script-src 'self' 'unsafe-eval'; ", // unsafe-eval for Vue dev tools
                "worker-src 'self' blob:; ", // Allow web workers from blob URLs
                "style-src 'self' 'unsafe-inline'; ",
                "img-src 'self' data: https:; ",
                "font-src 'self' data:; ",
                "connect-src 'self' ws: wss: blob:; ", // WebSocket for hot reload, blob: for voice notes
                "media-src 'self' blob:; ", // blob: for audio/video playback (voice notes)
                "frame-ancestors 'none'; ",
                "base-uri 'self'; ",
                "form-action 'self'"
            )
            .to_string()
        }
    }

    /// Check if HSTS should be enabled (production only)
    fn should_enable_hsts() -> bool {
        let env = std::env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "development".to_string())
            .to_lowercase();
        env == "production"
    }
}

impl<S, B> Transform<S, ServiceRequest> for SecurityHeaders
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = SecurityHeadersMiddleware<S>;
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ready(Ok(SecurityHeadersMiddleware {
            service,
            csp_header: Self::get_csp_header(),
            enable_hsts: Self::should_enable_hsts(),
        }))
    }
}

pub struct SecurityHeadersMiddleware<S> {
    service: S,
    csp_header: String,
    enable_hsts: bool,
}

impl<S, B> Service<ServiceRequest> for SecurityHeadersMiddleware<S>
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
        let csp_header = self.csp_header.clone();
        let enable_hsts = self.enable_hsts;

        // Capture request path for cache control decisions
        let path = req.path().to_string();

        let fut = self.service.call(req);

        Box::pin(async move {
            let mut res = fut.await?;

            let headers = res.headers_mut();

            // Cache-Control headers based on path
            // Only add if not already set by the handler
            if !headers.contains_key(header::CACHE_CONTROL) {
                if path.starts_with("/assets/") {
                    // Hashed assets can be cached forever (immutable)
                    // Vite uses content hashes, so different content = different URL
                    headers.insert(
                        header::CACHE_CONTROL,
                        "public, max-age=31536000, immutable".parse().unwrap(),
                    );
                } else if path.starts_with("/pdfjs/") {
                    // PDF.js assets can also be cached long-term
                    headers.insert(
                        header::CACHE_CONTROL,
                        "public, max-age=31536000, immutable".parse().unwrap(),
                    );
                }
                // For other paths, let the specific handlers set Cache-Control
            }

            // Content-Security-Policy
            if !headers.contains_key(header::CONTENT_SECURITY_POLICY) {
                headers.insert(
                    header::CONTENT_SECURITY_POLICY,
                    csp_header.parse().unwrap(),
                );
            }

            // X-Frame-Options (prevents clickjacking)
            if !headers.contains_key(header::X_FRAME_OPTIONS) {
                headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
            }

            // X-Content-Type-Options (prevents MIME sniffing)
            if !headers.contains_key(header::X_CONTENT_TYPE_OPTIONS) {
                headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
            }

            // X-XSS-Protection (legacy, but still good to have)
            if !headers.contains_key("X-XSS-Protection") {
                headers.insert(
                    "X-XSS-Protection".parse().unwrap(),
                    "1; mode=block".parse().unwrap(),
                );
            }

            // Referrer-Policy (controls referrer information)
            if !headers.contains_key(header::REFERRER_POLICY) {
                headers.insert(
                    header::REFERRER_POLICY,
                    "strict-origin-when-cross-origin".parse().unwrap(),
                );
            }

            // Permissions-Policy (formerly Feature-Policy)
            // Allow microphone for voice notes feature
            if !headers.contains_key("Permissions-Policy") {
                headers.insert(
                    "Permissions-Policy".parse().unwrap(),
                    "geolocation=(), microphone=(self), camera=()".parse().unwrap(),
                );
            }

            // Strict-Transport-Security (HSTS) - only in production with HTTPS
            if enable_hsts && !headers.contains_key(header::STRICT_TRANSPORT_SECURITY) {
                headers.insert(
                    header::STRICT_TRANSPORT_SECURITY,
                    "max-age=31536000; includeSubDomains".parse().unwrap(), // 1 year
                );
            }

            Ok(res)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_csp_header_generation() {
        // Just test that it returns a non-empty string
        let csp = SecurityHeaders::get_csp_header();
        assert!(!csp.is_empty());
        assert!(csp.contains("default-src 'self'"));
    }

    #[test]
    fn test_hsts_environment_check() {
        // Should be false by default (development)
        // This test depends on environment variable
        let should_enable = SecurityHeaders::should_enable_hsts();
        assert!(!should_enable || std::env::var("ENVIRONMENT").unwrap_or_default() == "production");
    }
}
