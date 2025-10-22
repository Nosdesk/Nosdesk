use actix_web::cookie::{Cookie, SameSite};
use chrono::{Duration, Utc};

/// Cookie configuration constants
const ACCESS_TOKEN_COOKIE: &str = "access_token";
const REFRESH_TOKEN_COOKIE: &str = "refresh_token";
const CSRF_TOKEN_COOKIE: &str = "csrf_token";

/// Create an httpOnly cookie for the access token (24 hours)
pub fn create_access_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(ACCESS_TOKEN_COOKIE, token.to_string())
        .path("/")
        .http_only(true)
        .secure(is_production()) // HTTPS only in production
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::hours(24))
        .finish()
}

/// Create an httpOnly cookie for the refresh token (7 days)
pub fn create_refresh_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(REFRESH_TOKEN_COOKIE, token.to_string())
        .path("/")
        .http_only(true)
        .secure(is_production())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::days(7))
        .finish()
}

/// Create a cookie for the CSRF token (NOT httpOnly - JS needs to read it)
pub fn create_csrf_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(CSRF_TOKEN_COOKIE, token.to_string())
        .path("/")
        .http_only(false) // JavaScript needs to read this
        .secure(is_production())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::hours(24))
        .finish()
}

/// Create a cookie to delete the access token
pub fn delete_access_token_cookie() -> Cookie<'static> {
    Cookie::build(ACCESS_TOKEN_COOKIE, "")
        .path("/")
        .http_only(true)
        .secure(is_production())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Create a cookie to delete the refresh token
pub fn delete_refresh_token_cookie() -> Cookie<'static> {
    Cookie::build(REFRESH_TOKEN_COOKIE, "")
        .path("/")
        .http_only(true)
        .secure(is_production())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Create a cookie to delete the CSRF token
pub fn delete_csrf_token_cookie() -> Cookie<'static> {
    Cookie::build(CSRF_TOKEN_COOKIE, "")
        .path("/")
        .http_only(false)
        .secure(is_production())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Check if running in production mode
fn is_production() -> bool {
    std::env::var("ENVIRONMENT")
        .unwrap_or_else(|_| "development".to_string())
        .to_lowercase()
        == "production"
}

/// Cookie name constants for extraction
pub const ACCESS_TOKEN_COOKIE_NAME: &str = ACCESS_TOKEN_COOKIE;
pub const REFRESH_TOKEN_COOKIE_NAME: &str = REFRESH_TOKEN_COOKIE;
pub const CSRF_TOKEN_COOKIE_NAME: &str = CSRF_TOKEN_COOKIE;
