//! How a client wants its session credentials delivered.
//!
//! Web clients use httpOnly cookies (the default). Native/mobile clients can't
//! rely on a cookie jar on a `tauri://` origin, so they opt into bearer mode by
//! sending the `X-Auth-Mode: bearer` request header; the auth endpoints then
//! return the session tokens in the response body (and set no cookies), and the
//! validator accepts the access token as `Authorization: Bearer`.
//!
//! Absent or any other header value means cookie mode, i.e. exactly the
//! existing web behaviour.

use actix_web::dev::ServiceRequest;
use actix_web::HttpRequest;

/// Request header a native client sets to opt into token-in-body auth.
pub const AUTH_MODE_HEADER: &str = "X-Auth-Mode";
const BEARER_VALUE: &str = "bearer";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuthMode {
    /// httpOnly cookies (web). The default.
    Cookie,
    /// Session tokens in the response body, `Authorization: Bearer` on requests.
    Bearer,
}

fn mode_from_header_value(value: Option<&str>) -> AuthMode {
    match value {
        Some(v) if v.trim().eq_ignore_ascii_case(BEARER_VALUE) => AuthMode::Bearer,
        _ => AuthMode::Cookie,
    }
}

/// Resolve the auth mode from a handler's `HttpRequest`.
pub fn auth_mode_from_request(req: &HttpRequest) -> AuthMode {
    mode_from_header_value(
        req.headers()
            .get(AUTH_MODE_HEADER)
            .and_then(|h| h.to_str().ok()),
    )
}

/// Resolve the auth mode from middleware's `ServiceRequest`.
pub fn auth_mode_from_service_request(req: &ServiceRequest) -> AuthMode {
    mode_from_header_value(
        req.headers()
            .get(AUTH_MODE_HEADER)
            .and_then(|h| h.to_str().ok()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_value_parsing() {
        assert_eq!(mode_from_header_value(Some("bearer")), AuthMode::Bearer);
        assert_eq!(mode_from_header_value(Some("Bearer")), AuthMode::Bearer);
        assert_eq!(mode_from_header_value(Some("  bearer ")), AuthMode::Bearer);
        assert_eq!(mode_from_header_value(None), AuthMode::Cookie);
        assert_eq!(mode_from_header_value(Some("")), AuthMode::Cookie);
        assert_eq!(mode_from_header_value(Some("cookie")), AuthMode::Cookie);
        assert_eq!(mode_from_header_value(Some("token")), AuthMode::Cookie);
    }
}
