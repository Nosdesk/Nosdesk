/// Configuration utilities for the application
use std::env;

#[derive(Debug)]
pub enum ConfigError {
    Missing(String),
    /// The variable is set but its value is not acceptable.
    Invalid(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Missing(key) => write!(f, "Missing environment variable: {key}"),
            ConfigError::Invalid(msg) => write!(f, "Invalid configuration: {msg}"),
        }
    }
}

// This allows ConfigError to be used with `?` in functions returning Result<_, Box<dyn std::error::Error>>
impl std::error::Error for ConfigError {}

// Helper to get an environment variable or return a ConfigError
fn get_env_var(name: &str) -> Result<String, ConfigError> {
    match env::var(name) {
        Ok(val) if !val.trim().is_empty() => Ok(val),
        _ => Err(ConfigError::Missing(name.to_string())),
    }
}

/// Whether the deployment is running in production, per the canonical
/// `ENVIRONMENT` var (the same one cookies / CSP / system-info read).
/// Anything other than `production` (including unset) is treated as
/// non-production, so dev/test default to the more permissive behaviour.
///
/// **Fail-open.** Use this only for enabling *extra* production-only behaviour
/// that is safe to skip in dev. For security posture that must NOT weaken when
/// the var is forgotten (CSP, HSTS, Secure cookies), use [`assume_production`].
pub fn is_production() -> bool {
    env::var("ENVIRONMENT")
        .map(|v| v.trim().eq_ignore_ascii_case("production"))
        .unwrap_or(false)
}

/// Fail-closed counterpart of [`is_production`]: the single source of truth for
/// "apply hardened (production) security posture." Returns `true` unless
/// `ENVIRONMENT` is an *explicit* local-dev label (`development` / `dev`), so an
/// unset, empty, or unrecognised value (a prod deploy that forgot to set it, or
/// a staging env) still gets the strict CSP, HSTS, and `Secure` cookies rather
/// than the permissive dev defaults. Set `ENVIRONMENT=development` (or `dev`)
/// for intentional plaintext-HTTP local setups.
pub fn assume_production() -> bool {
    assume_production_from(env::var("ENVIRONMENT").ok().as_deref())
}

/// Pure, injectable form of [`assume_production`] for callers that read
/// `ENVIRONMENT` through their own getter (e.g. `Config::from_source`, which is
/// unit-tested with a mock env). Same fail-closed rule: production unless an
/// explicit `development` / `dev` label. `None` (unset), empty, and unrecognised
/// values all assume production. This is the single source of truth for
/// "apply hardened security posture" — do not re-derive it inline.
pub fn assume_production_from(value: Option<&str>) -> bool {
    match value {
        Some(v) => {
            let v = v.trim().to_ascii_lowercase();
            v.is_empty() || !(v == "development" || v == "dev")
        }
        None => true,
    }
}

pub fn get_microsoft_client_id() -> Result<String, ConfigError> {
    get_env_var("MICROSOFT_CLIENT_ID")
}

/// Azure multi-tenant authority values that must NOT be used as
/// `MICROSOFT_TENANT_ID`. They let users from ANY Azure/MSA tenant
/// authenticate, which breaks the single-tenant trust the Microsoft
/// email-based account link relies on (and defeats openidconnect's strict
/// issuer check, which needs a concrete per-tenant issuer). A specific tenant
/// GUID or verified domain is required.
pub fn is_microsoft_multitenant_authority(tenant: &str) -> bool {
    matches!(
        tenant.trim().to_ascii_lowercase().as_str(),
        "common" | "organizations" | "consumers"
    )
}

pub fn get_microsoft_tenant_id() -> Result<String, ConfigError> {
    let tenant = get_env_var("MICROSOFT_TENANT_ID")?;
    if is_microsoft_multitenant_authority(&tenant) {
        return Err(ConfigError::Invalid(format!(
            "MICROSOFT_TENANT_ID must be a specific tenant (GUID or verified domain), \
             not the multi-tenant authority '{}'. A multi-tenant authority lets any \
             Azure/MSA account sign in, which is unsafe with email-based account linking.",
            tenant.trim()
        )));
    }
    Ok(tenant)
}

pub fn get_microsoft_client_secret() -> Result<String, ConfigError> {
    get_env_var("MICROSOFT_CLIENT_SECRET")
}

pub fn get_microsoft_redirect_uri() -> Result<String, ConfigError> {
    get_env_var("MICROSOFT_REDIRECT_URI")
}

// ===== OIDC Configuration =====

/// Check if OIDC is enabled (minimum required vars are set)
pub fn is_oidc_enabled() -> bool {
    get_env_var("OIDC_CLIENT_ID").is_ok() && get_env_var("OIDC_CLIENT_SECRET").is_ok()
}

/// Get OIDC client ID
pub fn get_oidc_client_id() -> Result<String, ConfigError> {
    get_env_var("OIDC_CLIENT_ID")
}

/// Get the native-app OIDC client id: the *public* client the mobile app uses
/// for its own Authorization-Code + PKCE flow against the IdP, distinct from the
/// confidential web `OIDC_CLIENT_ID`. Defaults to `nosdesk-app`.
pub fn get_oidc_native_client_id() -> String {
    env::var("OIDC_NATIVE_CLIENT_ID").unwrap_or_else(|_| "nosdesk-app".to_string())
}

/// Get OIDC client secret
pub fn get_oidc_client_secret() -> Result<String, ConfigError> {
    get_env_var("OIDC_CLIENT_SECRET")
}

/// Get OIDC issuer URL (for auto-discovery)
pub fn get_oidc_issuer_url() -> Result<String, ConfigError> {
    get_env_var("OIDC_ISSUER_URL")
}

/// Get OIDC redirect URI
pub fn get_oidc_redirect_uri() -> Result<String, ConfigError> {
    get_env_var("OIDC_REDIRECT_URI")
}

/// Get OIDC display name (defaults to "OpenID")
pub fn get_oidc_display_name() -> String {
    env::var("OIDC_DISPLAY_NAME").unwrap_or_else(|_| "OpenID".to_string())
}

/// Get OIDC scopes (defaults to "openid profile email")
pub fn get_oidc_scopes() -> String {
    env::var("OIDC_SCOPES").unwrap_or_else(|_| "openid profile email".to_string())
}

/// Get OIDC username claim (defaults to "preferred_username")
pub fn get_oidc_username_claim() -> String {
    env::var("OIDC_USERNAME_CLAIM").unwrap_or_else(|_| "preferred_username".to_string())
}

/// Get OIDC logout URI (optional)
pub fn get_oidc_logout_uri() -> Option<String> {
    get_env_var("OIDC_LOGOUT_URI").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multitenant_authorities_are_rejected() {
        // Case- and whitespace-insensitive.
        for t in [
            "common",
            "organizations",
            "consumers",
            "COMMON",
            " Common ",
            "Organizations",
        ] {
            assert!(
                is_microsoft_multitenant_authority(t),
                "{t:?} should be a multi-tenant authority"
            );
        }
    }

    #[test]
    fn specific_tenants_are_accepted() {
        for t in [
            "72f988bf-86f1-41af-91ab-2d7cd011db47", // tenant GUID
            "contoso.com",                          // verified domain
            "contoso.onmicrosoft.com",
        ] {
            assert!(
                !is_microsoft_multitenant_authority(t),
                "{t:?} should be a specific tenant"
            );
        }
        // Empty is a missing-config case (handled by get_env_var), not a
        // multi-tenant authority.
        assert!(!is_microsoft_multitenant_authority(""));
    }
}
