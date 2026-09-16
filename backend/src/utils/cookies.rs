use actix_web::cookie::{Cookie, SameSite};
use std::borrow::Cow;

/// Base cookie names. The wire name is [`cookie_name`] of these: `__Host-`
/// prefixed in production posture, bare in dev. Readers must go through
/// `cookie_name` too, never the bare constant.
pub const ACCESS_TOKEN_COOKIE: &str = "access_token";
pub const REFRESH_TOKEN_COOKIE: &str = "refresh_token";
pub const CSRF_TOKEN_COOKIE: &str = "csrf_token";

/// Prefix that makes a browser refuse the cookie unless it is `Secure`, has
/// `Path=/` and no `Domain` (RFC 6265bis). A sibling host or a plaintext
/// origin then cannot inject a cookie of this name, which is what keeps the
/// double-submit CSRF cookie and the refresh/OAuth-state cookies honest.
pub const HOST_PREFIX: &str = "__Host-";

/// Wire name for a session cookie: `__Host-<base>` whenever the cookie
/// carries `Secure` (see [`auth_cookies_use_secure_flag`]), the bare base name
/// on plain-HTTP dev where the prefix would make browsers drop it.
pub fn cookie_name(base: &'static str) -> Cow<'static, str> {
    if auth_cookies_use_secure_flag() {
        Cow::Owned(format!("{HOST_PREFIX}{base}"))
    } else {
        Cow::Borrowed(base)
    }
}

/// Create an httpOnly cookie for the access token (15 minutes)
pub fn create_access_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(cookie_name(ACCESS_TOKEN_COOKIE), token.to_string())
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag()) // HTTPS unless explicit dev (see below)
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::minutes(15))
        .finish()
}

/// Create an httpOnly cookie for the refresh token (7 days). `Path=/` because
/// `__Host-` demands it; the path scoping it used to have bought little (the
/// cookie is httpOnly and only the refresh handler reads it).
pub fn create_refresh_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(cookie_name(REFRESH_TOKEN_COOKIE), token.to_string())
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::days(7))
        .finish()
}

/// Create a cookie for the CSRF token (NOT httpOnly - JS needs to read it)
pub fn create_csrf_token_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(cookie_name(CSRF_TOKEN_COOKIE), token.to_string())
        .path("/")
        .http_only(false) // JavaScript needs to read this
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::minutes(15))
        .finish()
}

/// Create a cookie to delete the access token
pub fn delete_access_token_cookie() -> Cookie<'static> {
    Cookie::build(cookie_name(ACCESS_TOKEN_COOKIE), "")
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Create a cookie to delete the refresh token
pub fn delete_refresh_token_cookie() -> Cookie<'static> {
    Cookie::build(cookie_name(REFRESH_TOKEN_COOKIE), "")
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

/// Create a cookie to delete the CSRF token
pub fn delete_csrf_token_cookie() -> Cookie<'static> {
    Cookie::build(cookie_name(CSRF_TOKEN_COOKIE), "")
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
/// as long as the state JWT (10 min). `Path=/` for the `__Host-` prefix.
pub fn create_oauth_state_cookie(binding: &str) -> Cookie<'static> {
    Cookie::build(cookie_name(OAUTH_STATE_COOKIE), binding.to_string())
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Lax)
        .max_age(actix_web::cookie::time::Duration::minutes(10))
        .finish()
}

/// Cookie that clears [`OAUTH_STATE_COOKIE`] once a flow completes.
pub fn delete_oauth_state_cookie() -> Cookie<'static> {
    Cookie::build(cookie_name(OAUTH_STATE_COOKIE), "")
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Lax)
        .max_age(actix_web::cookie::time::Duration::seconds(0))
        .finish()
}

// --- Customer portal session cookies ---
//
// The portal is a separate principal realm on a separate registrable domain
// (`<slug>.nosdesk.app`), so it gets its OWN cookie names. Distinct names mean
// an agent session and a portal session never collide in one browser even if a
// custom domain later puts them under the same registrable domain; the agent
// auth path only ever reads `access_token`, never these. `__Host-`, host-only
// and `SameSite=Strict` like the agent cookies (no `Domain=.` sharing).
pub const PORTAL_ACCESS_TOKEN_COOKIE: &str = "portal_access";
pub const PORTAL_REFRESH_TOKEN_COOKIE: &str = "portal_refresh";
pub const PORTAL_CSRF_TOKEN_COOKIE: &str = "portal_csrf";

/// httpOnly portal access-token cookie (15 minutes).
pub fn create_portal_access_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(cookie_name(PORTAL_ACCESS_TOKEN_COOKIE), token.to_string())
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::minutes(15))
        .finish()
}

/// httpOnly portal refresh-token cookie (7 days).
pub fn create_portal_refresh_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(cookie_name(PORTAL_REFRESH_TOKEN_COOKIE), token.to_string())
        .path("/")
        .http_only(true)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::days(7))
        .finish()
}

/// Portal CSRF cookie (NOT httpOnly so the portal SPA can echo it in a header).
pub fn create_portal_csrf_cookie(token: &str) -> Cookie<'static> {
    Cookie::build(cookie_name(PORTAL_CSRF_TOKEN_COOKIE), token.to_string())
        .path("/")
        .http_only(false)
        .secure(auth_cookies_use_secure_flag())
        .same_site(SameSite::Strict)
        .max_age(actix_web::cookie::time::Duration::minutes(15))
        .finish()
}

/// Whether auth cookies receive the `Secure` attribute. Delegates to the
/// shared fail-closed [`crate::config_utils::assume_production`] so cookies,
/// CSP, and HSTS all decide "hardened posture" from one place: an unset,
/// empty, or unrecognised `ENVIRONMENT` still emits `Secure` cookies (never
/// valid over plaintext HTTP). Set `ENVIRONMENT=development` (or `dev`) for
/// intentional HTTP local setups (Docker Compose on localhost, etc.).
fn auth_cookies_use_secure_flag() -> bool {
    crate::config_utils::assume_production()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every test that reads `ENVIRONMENT` takes this lock: the posture is
    // process-global and cargo runs tests in parallel.
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn production() -> std::sync::MutexGuard<'static, ()> {
        let g = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ENVIRONMENT");
        g
    }

    #[test]
    fn cookie_name_is_host_prefixed_only_when_secure() {
        let _g = production();
        assert_eq!(cookie_name(ACCESS_TOKEN_COOKIE), "__Host-access_token");
        assert_eq!(cookie_name(PORTAL_CSRF_TOKEN_COOKIE), "__Host-portal_csrf");
        assert!(create_access_token_cookie("t").secure().unwrap_or(false));

        std::env::set_var("ENVIRONMENT", "development");
        assert_eq!(cookie_name(ACCESS_TOKEN_COOKIE), "access_token");
        assert_eq!(create_access_token_cookie("t").name(), "access_token");
        assert!(!create_access_token_cookie("t").secure().unwrap_or(false));
        std::env::remove_var("ENVIRONMENT");
    }

    /// `__Host-` cookies must be Secure, `Path=/` and carry no `Domain`, or
    /// the browser drops them silently. Pin every builder to that shape.
    #[test]
    fn every_host_prefixed_cookie_satisfies_the_prefix_rules() {
        let _g = production();
        let cookies = [
            create_access_token_cookie("t"),
            create_refresh_token_cookie("t"),
            create_csrf_token_cookie("t"),
            delete_access_token_cookie(),
            delete_refresh_token_cookie(),
            delete_csrf_token_cookie(),
            create_oauth_state_cookie("t"),
            delete_oauth_state_cookie(),
            create_portal_access_cookie("t"),
            create_portal_refresh_cookie("t"),
            create_portal_csrf_cookie("t"),
        ];
        for c in cookies {
            assert!(
                c.name().starts_with(HOST_PREFIX),
                "{} lacks the prefix",
                c.name()
            );
            assert!(c.secure().unwrap_or(false), "{} is not Secure", c.name());
            assert_eq!(c.path(), Some("/"), "{} is not Path=/", c.name());
            assert!(c.domain().is_none(), "{} carries a Domain", c.name());
        }
    }

    #[test]
    fn access_token_cookie_is_http_only() {
        let _g = production();
        let cookie = create_access_token_cookie("tok123");
        assert_eq!(cookie.name(), cookie_name(ACCESS_TOKEN_COOKIE));
        assert_eq!(cookie.value(), "tok123");
        assert!(cookie.http_only().unwrap_or(false));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
        assert_eq!(cookie.path(), Some("/"));
    }

    #[test]
    fn refresh_token_cookie_is_http_only() {
        let _g = production();
        let cookie = create_refresh_token_cookie("ref456");
        assert_eq!(cookie.name(), cookie_name(REFRESH_TOKEN_COOKIE));
        assert!(cookie.http_only().unwrap_or(false));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
    }

    #[test]
    fn csrf_cookie_is_not_http_only() {
        let _g = production();
        let cookie = create_csrf_token_cookie("csrf789");
        assert_eq!(cookie.name(), cookie_name(CSRF_TOKEN_COOKIE));
        assert_eq!(cookie.value(), "csrf789");
        // CSRF cookie must be readable by JavaScript
        assert!(!cookie.http_only().unwrap_or(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
    }

    #[test]
    fn oauth_state_cookie_is_lax_and_http_only() {
        let _g = production();
        let cookie = create_oauth_state_cookie("bind-abc");
        assert_eq!(cookie.name(), cookie_name(OAUTH_STATE_COOKIE));
        assert_eq!(cookie.value(), "bind-abc");
        assert!(cookie.http_only().unwrap_or(false));
        // MUST be Lax, not Strict: the IdP redirects the browser back to the
        // callback cross-site, and a Strict cookie would not be sent.
        assert_eq!(cookie.same_site(), Some(SameSite::Lax));
    }

    #[test]
    fn delete_cookies_have_zero_max_age() {
        let _g = production();
        for c in [
            delete_access_token_cookie(),
            delete_refresh_token_cookie(),
            delete_csrf_token_cookie(),
            delete_oauth_state_cookie(),
        ] {
            assert_eq!(c.value(), "");
            assert_eq!(
                c.max_age(),
                Some(actix_web::cookie::time::Duration::seconds(0))
            );
        }
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
        let _g = production();
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
