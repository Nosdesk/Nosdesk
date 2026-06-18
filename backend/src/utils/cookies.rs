use actix_web::cookie::{Cookie, SameSite};

/// Cookie configuration constants
pub const ACCESS_TOKEN_COOKIE: &str = "access_token";
pub const REFRESH_TOKEN_COOKIE: &str = "refresh_token";
pub const CSRF_TOKEN_COOKIE: &str = "csrf_token";

/// Create an httpOnly cookie for the access token (15 minutes)
pub fn create_access_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(ACCESS_TOKEN_COOKIE, token.to_string())
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag()) // HTTPS unless explicit dev (see below)
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::minutes(15))
        .finish()
}

/// Create an httpOnly cookie for the refresh token (7 days, scoped to refresh endpoint)
pub fn create_refresh_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(REFRESH_TOKEN_COOKIE, token.to_string())
        .path("/api/auth/refresh")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::days(7))
        .finish()
}

/// Create a cookie for the CSRF token (NOT httpOnly - JS needs to read it)
pub fn create_csrf_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(CSRF_TOKEN_COOKIE, token.to_string())
        .path("/")
        .http_only(false) // JavaScript needs to read this
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::minutes(15))
        .finish()
}

/// Create a cookie to delete the access token
pub fn delete_access_token_cookie() -> Cookie<'static> {
    Cookie::build(ACCESS_TOKEN_COOKIE, "")
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Create a cookie to delete the refresh token
pub fn delete_refresh_token_cookie() -> Cookie<'static> {
    Cookie::build(REFRESH_TOKEN_COOKIE, "")
        .path("/api/auth/refresh")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Create a cookie to delete the CSRF token
pub fn delete_csrf_token_cookie() -> Cookie<'static> {
    Cookie::build(CSRF_TOKEN_COOKIE, "")
        .path("/")
        .http_only(false)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Binds an in-progress OAuth/OIDC login to the browser that started it
/// (RFC 9700 §2.1). Set at initiation carrying the flow's random binding value;
/// the callback rejects unless this cookie matches the value in the signed
/// state, so an attacker can't CSRF their own `(code, state)` onto a victim.
pub const OAUTH_STATE_COOKIE: &str = "oauth_state";

/// Cookie binding an OAuth flow to its initiating user-agent. `SameSite=Lax`
/// (NOT Strict): the IdP redirects the browser back to the callback as a
/// cross-site top-level navigation, and a Strict cookie would not be sent. Lives
/// as long as the state JWT (10 min). Scoped to the OAuth endpoints.
pub fn create_oauth_state_cookie(binding: &str) -> Cookie<'static> {
    Cookie::build(OAUTH_STATE_COOKIE, binding.to_string())
        .path("/api/auth/oauth")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Lax)
        .max_age(actix_web::cookie::time::Duration::minutes(10))
        .finish()
}

/// Cookie that clears [`OAUTH_STATE_COOKIE`] once a flow completes.
pub fn delete_oauth_state_cookie() -> Cookie<'static> {
    Cookie::build(OAUTH_STATE_COOKIE, "")
        .path("/api/auth/oauth")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Lax)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Whether auth cookies receive the `Secure` attribute.
///
/// **Fail-closed:** `ENVIRONMENT` unset / empty / anything other than an
/// explicit local-dev label is treated as needing `Secure=true`, so a
/// production deployment that forgets `ENVIRONMENT=production` still does not
/// emit session cookies valid over plaintext HTTP.
///
/// Set `ENVIRONMENT=development` or `ENVIRONMENT=dev` for intentional HTTP
/// local setups (Docker Compose on localhost, etc.).
fn auth_cookies_use_secure_flag() -> bool {
    match std::env::var("ENVIRONMENT") {
        Ok(v) => {
            let v = v.trim().to_ascii_lowercase();
            if v.is_empty() {
                true
            } else {
                !(v == "development" || v == "dev")
            }
        }
        Err(_) => true,
    }
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
        assert_eq!(cookie.path(), Some("/api/auth/refresh"));
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
    fn oauth_state_cookie_is_lax_and_http_only() {
        let cookie = create_oauth_state_cookie("bind-abc");
        assert_eq!(cookie.name(), OAUTH_STATE_COOKIE);
        assert_eq!(cookie.value(), "bind-abc");
        assert!(cookie.http_only().unwrap_or(false));
        // MUST be Lax, not Strict: the IdP redirects the browser back to the
        // callback cross-site, and a Strict cookie would not be sent.
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
        assert_eq!(cookie.path(), Some("/api/auth/oauth"));
    }

    #[test]
    fn delete_cookies_have_zero_max_age() {
        let del_access = delete_access_token_cookie();
        assert_eq!(del_access.value(), "");
        assert_eq!(
            del_access.max_age(),
            Some(actix_web::cookie::time::Duration::seconds(0))
        );

        let del_refresh = delete_refresh_token_cookie();
        assert_eq!(del_refresh.value(), "");
        assert_eq!(
            del_refresh.max_age(),
            Some(actix_web::cookie::time::Duration::seconds(0))
        );
        assert_eq!(del_refresh.path(), Some("/api/auth/refresh"));

        let del_csrf = delete_csrf_token_cookie();
        assert_eq!(del_csrf.value(), "");
        assert_eq!(
            del_csrf.max_age(),
            Some(actix_web::cookie::time::Duration::seconds(0))
        );
    }

    #[test]
    fn access_token_max_age_is_15_minutes() {
        let cookie = create_access_token_cookie("t");
        assert_eq!(
            cookie.max_age(),
            Some(actix_web::cookie::time::Duration::minutes(15))
        );
    }

    #[test]
    fn auth_cookies_secure_when_environment_unset_or_non_dev() {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _g = LOCK.lock().unwrap();

        std::env::remove_var("ENVIRONMENT");
        assert!(
            super::auth_cookies_use_secure_flag(),
            "unset ENVIRONMENT must default Secure cookies on"
        );

        std::env::set_var("ENVIRONMENT", "");
        assert!(
            super::auth_cookies_use_secure_flag(),
            "empty ENVIRONMENT must keep Secure cookies on"
        );

        std::env::set_var("ENVIRONMENT", "production");
        assert!(super::auth_cookies_use_secure_flag());

        std::env::set_var("ENVIRONMENT", "development");
        assert!(!super::auth_cookies_use_secure_flag());

        std::env::set_var("ENVIRONMENT", "dev");
        assert!(!super::auth_cookies_use_secure_flag());

        std::env::remove_var("ENVIRONMENT");
    }
}
