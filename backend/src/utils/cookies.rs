use actix_web::cookie::{Cookie, SameSite};

/// Cookie configuration constants
pub const ACCESS_TOKEN_COOKIE: &str = "access_token";
pub const REFRESH_TOKEN_COOKIE: &str = "refresh_token";
pub const CSRF_TOKEN_COOKIE: &str = "csrf_token";

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_token_cookie_is_http_only() {
        let cookie = create_access_token_cookie("tok123");
        assert_eq!(cookie.name(), ACCESS_TOKEN_COOKIE);
        assert_eq!(cookie.value(), "tok123");
        assert!(cookie.http_only().unwrap_or(false));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
        assert_eq!(cookie.path(), Some("/"));
    }

    #[test]
    fn refresh_token_cookie_is_http_only() {
        let cookie = create_refresh_token_cookie("ref456");
        assert_eq!(cookie.name(), REFRESH_TOKEN_COOKIE);
        assert!(cookie.http_only().unwrap_or(false));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
    }

    #[test]
    fn csrf_cookie_is_not_http_only() {
        let cookie = create_csrf_token_cookie("csrf789");
        assert_eq!(cookie.name(), CSRF_TOKEN_COOKIE);
        assert_eq!(cookie.value(), "csrf789");
        // CSRF cookie must be readable by JavaScript
        assert!(!cookie.http_only().unwrap_or(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
    }

    #[test]
    fn delete_cookies_have_zero_max_age() {
        let del_access = delete_access_token_cookie();
        assert_eq!(del_access.value(), "");
        assert_eq!(del_access.max_age(), Some(actix_web::cookie::time::Duration::seconds(0)));

        let del_refresh = delete_refresh_token_cookie();
        assert_eq!(del_refresh.value(), "");
        assert_eq!(del_refresh.max_age(), Some(actix_web::cookie::time::Duration::seconds(0)));

        let del_csrf = delete_csrf_token_cookie();
        assert_eq!(del_csrf.value(), "");
        assert_eq!(del_csrf.max_age(), Some(actix_web::cookie::time::Duration::seconds(0)));
    }

    #[test]
    fn access_token_max_age_is_24_hours() {
        let cookie = create_access_token_cookie("t");
        assert_eq!(cookie.max_age(), Some(actix_web::cookie::time::Duration::hours(24)));
    }

    #[test]
    fn refresh_token_max_age_is_7_days() {
        let cookie = create_refresh_token_cookie("t");
        assert_eq!(cookie.max_age(), Some(actix_web::cookie::time::Duration::days(7)));
    }
}

