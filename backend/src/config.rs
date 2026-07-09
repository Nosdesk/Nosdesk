//! Startup configuration: environment parsing + fail-fast validation.
//!
//! `Config::from_env` is the single place boot-time environment variables are
//! read and validated. It returns `Err` (rather than calling `process::exit`)
//! so the composition root owns the exit and the logic stays unit-testable.
//! Subsystem initialisation that constructs global state (the encryption
//! keyring, GeoIP) and the rate limiters live in `main`, and read their inputs
//! off the returned `Config`.

use std::env;

use tracing::{error, info, warn};

/// A boot-time variable lookup. In production it's `|k| env::var(k).ok()`; in
/// tests it's a map, so the fatal paths are reachable without mutating process
/// env (which would race across parallel tests).
type EnvGet<'a> = &'a dyn Fn(&str) -> Option<String>;

/// Parsed, validated startup configuration.
#[derive(Clone, Debug)]
pub struct Config {
    /// Raw `ENVIRONMENT` value ("development" by default).
    pub environment: String,
    /// Whether to apply hardened production posture, precomputed for the boot
    /// gates. Fail-closed (`config_utils::assume_production_from`): true unless
    /// `ENVIRONMENT` is an explicit `development` / `dev` label.
    pub is_production: bool,
    pub host: String,
    pub port: u16,
    pub rate_limit_per_minute: u64,
    pub auth_rate_limit_per_minute: u64,
    pub redis_url: String,
    pub max_file_size_mb: usize,
    /// `max_file_size_mb` in bytes, for the actix payload/multipart caps.
    pub max_payload_size: usize,
    pub frontend_url: String,
    pub additional_origins: Vec<String>,
    /// Tenant domain suffix for hosted-mode CORS (M5 Task 6); `None` self-hosted.
    pub tenant_domain: Option<String>,
    /// Graceful-drain budget for in-flight HTTP requests on shutdown, passed to
    /// actix's `HttpServer::shutdown_timeout`. Keep it below the deploy grace
    /// period (Fly `kill_timeout`, k8s `terminationGracePeriodSeconds`) minus
    /// the readiness-drain pause + collab-flush budget, so the drain completes
    /// before SIGKILL. `NOSDESK_SHUTDOWN_TIMEOUT_SECS`, default 25.
    pub shutdown_timeout_secs: u64,
}

/// Detects values that look like docker.env.example placeholders (e.g.
/// "your-super-secret-jwt-key-change-this-in-production"). Applied to every
/// production secret check so an operator who forgets to override the example
/// file gets a fast hard failure rather than a forged-token incident.
pub fn looks_like_placeholder(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "change-this",
        "change-me",
        "your-super-secret",
        "your-64-character",
        "your-",
        "placeholder",
        "example",
    ];
    NEEDLES.iter().any(|n| lower.contains(n))
}

fn fatal(reason: &str) -> std::io::Error {
    std::io::Error::other(reason.to_string())
}

impl Config {
    /// Parse and validate the real process environment. Thin wrapper over
    /// [`Config::from_source`] that supplies `env::var` and the build-time
    /// plugin-root presence.
    pub fn from_env() -> Result<Config, std::io::Error> {
        let plugin_root_present = crate::services::plugins::signing::root_pubkey().is_some();
        Self::from_source(&|k| env::var(k).ok(), plugin_root_present)
    }

    /// The testable core: reads every value through `get` and takes the
    /// build-time plugin-root presence as an explicit flag, so unit tests can
    /// drive every fatal path without touching process env or the build
    /// constant. Logs operator-facing guidance on each fatal path (same
    /// messages as before) and returns `Err` so the caller exits non-zero.
    /// Never constructs global state.
    pub fn from_source(get: EnvGet, plugin_root_present: bool) -> Result<Config, std::io::Error> {
        let environment = get("ENVIRONMENT").unwrap_or_else(|| "development".to_string());
        // Fail-closed: an unset / empty / non-canonical `ENVIRONMENT` (a prod
        // deploy that forgot it, or `staging` / `prod` / `Production`) must still
        // enforce the hardened secret gates below, matching how CSP/HSTS/cookies
        // use `assume_production`. Only an explicit `development` / `dev` label
        // opts out. Read the RAW getter here, not `environment` above, which has
        // already defaulted an unset value to `development`.
        let is_production =
            crate::config_utils::assume_production_from(get("ENVIRONMENT").as_deref());
        info!("Environment: {}", environment);

        validate_jwt_secret(get, is_production)?;
        validate_plugin_trust_root(is_production, plugin_root_present)?;
        validate_default_credentials(get, is_production)?;
        warn_insecure_production_urls(get, is_production);

        let rate_limit_per_minute = get("RATE_LIMIT_PER_MINUTE")
            .unwrap_or_else(|| "60".to_string()) // Conservative limit for public endpoints
            .parse::<u64>()
            .unwrap_or(60)
            .clamp(30, 1000); // Reasonable limits: 30-1000 requests per minute

        let auth_rate_limit_per_minute = get("AUTH_RATE_LIMIT_PER_MINUTE")
            .unwrap_or_else(|| "600".to_string()) // Higher limit for authenticated users (10x public rate)
            .parse::<u64>()
            .unwrap_or(600)
            .clamp(120, 5000); // Higher limits for authenticated users: 120-5000 requests per minute

        // Redis is a hard dependency: HTTP + auth/MFA rate limiting, the Yjs
        // collab cache, and the `/readiness` probe all require it. Resolve ONE
        // URL for all of them (same shape as `utils::rate_limit::get_redis_url`).
        // Production requires it explicitly — in-memory rate limiting would be
        // per-machine, an N× silent bypass across the fleet — while dev defaults
        // to localhost. There is no `memory://` fallback: it only ever masked a
        // misconfigured single dev box where readiness and auth lockout were
        // already broken anyway.
        let redis_url = match get("REDIS_URL") {
            Some(url) => url,
            None => {
                if is_production {
                    error!(
                        "REDIS_URL is required in production: rate limiting, the collab cache, and the readiness probe all depend on Redis, and in-memory limiting is a per-machine (N×) silent bypass. Configure Redis."
                    );
                    return Err(fatal("REDIS_URL is required in production"));
                }
                "redis://localhost:6379".to_string()
            }
        };

        let host = get("HOST").unwrap_or_else(|| "127.0.0.1".to_string());
        let port = get("PORT")
            .unwrap_or_else(|| "8080".to_string())
            .parse::<u16>()
            .map_err(|e| {
                error!(error = %e, "Invalid PORT value");
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid PORT")
            })?;

        // Security: file upload limits from environment.
        let max_file_size_mb = get("MAX_FILE_SIZE_MB")
            .unwrap_or_else(|| "50".to_string())
            .parse::<usize>()
            .unwrap_or(50)
            .clamp(1, 500); // 1MB to 500MB limit
        let max_payload_size = max_file_size_mb * 1024 * 1024;

        // CORS: FRONTEND_URL required in production.
        let frontend_url = match get("FRONTEND_URL") {
            Some(url) => url,
            None if is_production => {
                error!("FRONTEND_URL must be set in production for CORS security");
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "FRONTEND_URL environment variable is required in production",
                ));
            }
            None => "http://localhost:3000".to_string(),
        };

        let additional_origins: Vec<String> = get("ADDITIONAL_CORS_ORIGINS")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string())
            .collect();

        // Tenant domain suffix for hosted-mode CORS (M5 Task 6). When set (e.g.
        // `nosdesk.app`), every `<slug>.<tenant_domain>` origin passes the CORS
        // check. Self-hosted leaves this unset and relies on FRONTEND_URL alone.
        // Built as an anchored regex downstream so a substring-only match
        // (`s.ends_with(".nosdesk.app")`) — the classic CORS bypass — can't happen.
        let tenant_domain = get("NOSDESK_TENANT_DOMAIN")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());

        let shutdown_timeout_secs = get("NOSDESK_SHUTDOWN_TIMEOUT_SECS")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(25);

        Ok(Config {
            environment,
            is_production,
            host,
            port,
            rate_limit_per_minute,
            auth_rate_limit_per_minute,
            redis_url,
            max_file_size_mb,
            max_payload_size,
            frontend_url,
            additional_origins,
            tenant_domain,
            shutdown_timeout_secs,
        })
    }
}

/// JWT_SECRET must be present, non-placeholder, and >= 32 chars in production.
/// Validated here; the value itself is read again by `JwtUtils` at use.
fn validate_jwt_secret(get: EnvGet, is_production: bool) -> Result<(), std::io::Error> {
    match get("JWT_SECRET") {
        Some(secret) => {
            if is_production && looks_like_placeholder(&secret) {
                error!("JWT_SECRET appears to be the docker.env.example placeholder");
                error!("Refusing to start in production with a placeholder JWT_SECRET");
                error!("Generate a secure key with: openssl rand -base64 32");
                return Err(fatal("JWT_SECRET is a placeholder"));
            }
            if secret.len() < 32 {
                if is_production {
                    error!("JWT_SECRET must be at least 32 characters in production");
                    error!("Generate a secure key with: openssl rand -base64 32");
                    return Err(fatal("JWT_SECRET is too short"));
                } else {
                    warn!("JWT_SECRET is less than 32 characters - this would be rejected in production");
                }
            }
        }
        None => {
            error!("JWT_SECRET environment variable must be set");
            error!("Generate a secure key with: openssl rand -base64 32");
            return Err(fatal("JWT_SECRET must be set"));
        }
    }
    info!("JWT_SECRET validated");
    Ok(())
}

/// NOSDESK_ROOT_PUBKEY is baked in at build time via option_env! (see
/// services/plugins/signing.rs). Without it the plugin trust chain can't verify
/// Official / Verified tiers; only `local` (CLI-installed) plugins work. That's
/// acceptable for an unconfigured fork but not for a production deployment.
fn validate_plugin_trust_root(
    is_production: bool,
    plugin_root_present: bool,
) -> Result<(), std::io::Error> {
    if is_production && !plugin_root_present {
        error!("NOSDESK_ROOT_PUBKEY was not set at build time");
        error!("Refusing to start in production without a plugin trust root");
        error!("Rebuild with: docker build --build-arg NOSDESK_ROOT_PUBKEY=<base64> ...");
        error!("(Forks running their own registry should override with their own root key.)");
        return Err(fatal("plugin trust root missing"));
    }
    Ok(())
}

/// docker.env.example default credentials must never ship to production.
fn validate_default_credentials(get: EnvGet, is_production: bool) -> Result<(), std::io::Error> {
    if !is_production {
        return Ok(());
    }
    const EX_POSTGRES_PASSWORD: &str = "nosdesk_password";
    const EX_REDIS_PASSWORD: &str = "nosdesk_redis_password";

    let insecure_defaults_allowed = matches!(
        get("ALLOW_INSECURE_DEFAULT_SECRETS").as_deref(),
        Some("1" | "true" | "yes")
    );

    if !insecure_defaults_allowed {
        if get("POSTGRES_PASSWORD").as_deref() == Some(EX_POSTGRES_PASSWORD) {
            error!("POSTGRES_PASSWORD matches docker.env.example default ({EX_POSTGRES_PASSWORD})");
            error!("Refusing to start in production with documented sample credentials");
            error!("Change POSTGRES_PASSWORD or set ALLOW_INSECURE_DEFAULT_SECRETS=1 only for isolated labs");
            return Err(fatal("POSTGRES_PASSWORD is a documented default"));
        }
        if get("REDIS_PASSWORD").as_deref() == Some(EX_REDIS_PASSWORD) {
            error!("REDIS_PASSWORD matches docker.env.example default ({EX_REDIS_PASSWORD})");
            error!("Refusing to start in production with documented sample credentials");
            error!("Change REDIS_PASSWORD or set ALLOW_INSECURE_DEFAULT_SECRETS=1 only for isolated labs");
            return Err(fatal("REDIS_PASSWORD is a documented default"));
        }
    } else {
        warn!("ALLOW_INSECURE_DEFAULT_SECRETS enabled — example Postgres/Redis passwords accepted (labs only)");
    }
    Ok(())
}

/// Non-fatal production hygiene warnings for HTTPS / DB SSL.
fn warn_insecure_production_urls(get: EnvGet, is_production: bool) {
    if !is_production {
        return;
    }
    if let Some(frontend_url) = get("FRONTEND_URL") {
        if !frontend_url.starts_with("https://") && !frontend_url.starts_with("http://localhost") {
            warn!("FRONTEND_URL should use HTTPS in production");
        }
    }
    if let Some(db_url) = get("DATABASE_URL") {
        if !db_url.contains("sslmode=require") && !db_url.contains("localhost") {
            warn!("DATABASE_URL should use sslmode=require in production");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Build a Config from an in-memory var map (no process env touched), with
    /// an explicit plugin-root-present flag. `plugin_root=true` keeps the
    /// plugin-root check from short-circuiting the production-only cases below.
    fn build(pairs: &[(&str, &str)], plugin_root: bool) -> Result<Config, std::io::Error> {
        let mut map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        // Parsing-focused tests default to an explicit dev label so they don't
        // trip the fail-closed production gates (an unset ENVIRONMENT now assumes
        // production — see `unset_environment_assumes_production`). Tests that
        // assert production pass `ENVIRONMENT=production`, which overrides this.
        map.entry("ENVIRONMENT".to_string())
            .or_insert_with(|| "development".to_string());
        let get = |k: &str| map.get(k).cloned();
        Config::from_source(&get, plugin_root)
    }

    // A valid, non-placeholder, >=32 char secret for the happy paths.
    const GOOD_JWT: &str = "0123456789abcdef0123456789abcdef01";

    #[test]
    fn dev_defaults_are_accepted() {
        let c = build(&[("JWT_SECRET", GOOD_JWT)], true).unwrap();
        assert!(!c.is_production);
        assert_eq!(c.redis_url, "redis://localhost:6379"); // dev fallback
        assert_eq!(c.port, 8080);
        assert_eq!(c.frontend_url, "http://localhost:3000");
        assert_eq!(c.max_payload_size, 50 * 1024 * 1024);
    }

    #[test]
    fn jwt_missing_is_fatal() {
        let err = build(&[], true).unwrap_err();
        assert_eq!(err.to_string(), "JWT_SECRET must be set");
    }

    #[test]
    fn unset_environment_assumes_production() {
        // Fail-closed: with ENVIRONMENT absent entirely (bypassing the test
        // helper's dev default), the hardened gates must fire rather than
        // defaulting to permissive dev behaviour. A too-short JWT is rejected.
        let get = |k: &str| match k {
            "JWT_SECRET" => Some("short".to_string()),
            _ => None,
        };
        let err = Config::from_source(&get, true).unwrap_err();
        assert_eq!(err.to_string(), "JWT_SECRET is too short");
    }

    #[test]
    fn jwt_placeholder_in_production_is_fatal() {
        let err = build(
            &[
                ("ENVIRONMENT", "production"),
                ("JWT_SECRET", "your-super-secret-change-this-in-production"),
            ],
            true,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "JWT_SECRET is a placeholder");
    }

    #[test]
    fn jwt_too_short_in_production_is_fatal() {
        let err = build(
            &[("ENVIRONMENT", "production"), ("JWT_SECRET", "short")],
            true,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "JWT_SECRET is too short");
    }

    #[test]
    fn short_jwt_in_dev_is_allowed() {
        // dev only warns; still builds.
        assert!(build(&[("JWT_SECRET", "short")], true).is_ok());
    }

    #[test]
    fn plugin_root_missing_in_production_is_fatal() {
        let err = build(
            &[("ENVIRONMENT", "production"), ("JWT_SECRET", GOOD_JWT)],
            false, // no build-time pubkey
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "plugin trust root missing");
    }

    #[test]
    fn redis_required_in_production() {
        let err = build(
            &[("ENVIRONMENT", "production"), ("JWT_SECRET", GOOD_JWT)],
            true,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "REDIS_URL is required in production");
    }

    #[test]
    fn frontend_url_required_in_production() {
        let err = build(
            &[
                ("ENVIRONMENT", "production"),
                ("JWT_SECRET", GOOD_JWT),
                ("REDIS_URL", "redis://cache:6379"),
            ],
            true,
        )
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            "FRONTEND_URL environment variable is required in production"
        );
    }

    #[test]
    fn default_postgres_password_in_production_is_fatal() {
        let err = build(
            &[
                ("ENVIRONMENT", "production"),
                ("JWT_SECRET", GOOD_JWT),
                ("REDIS_URL", "redis://cache:6379"),
                ("FRONTEND_URL", "https://app.example.com"),
                ("POSTGRES_PASSWORD", "nosdesk_password"),
            ],
            true,
        )
        .unwrap_err();
        assert_eq!(err.to_string(), "POSTGRES_PASSWORD is a documented default");
    }

    #[test]
    fn insecure_defaults_escape_hatch_allows_sample_creds() {
        let c = build(
            &[
                ("ENVIRONMENT", "production"),
                ("JWT_SECRET", GOOD_JWT),
                ("REDIS_URL", "redis://cache:6379"),
                ("FRONTEND_URL", "https://app.example.com"),
                ("POSTGRES_PASSWORD", "nosdesk_password"),
                ("ALLOW_INSECURE_DEFAULT_SECRETS", "1"),
            ],
            true,
        )
        .unwrap();
        assert!(c.is_production);
    }

    #[test]
    fn invalid_port_is_fatal() {
        let err = build(&[("JWT_SECRET", GOOD_JWT), ("PORT", "not-a-port")], true).unwrap_err();
        assert_eq!(err.to_string(), "Invalid PORT");
    }

    #[test]
    fn rate_limits_are_clamped() {
        let c = build(
            &[("JWT_SECRET", GOOD_JWT), ("RATE_LIMIT_PER_MINUTE", "5")],
            true,
        )
        .unwrap();
        assert_eq!(c.rate_limit_per_minute, 30); // clamped up to the floor
    }

    #[test]
    fn cors_origins_and_tenant_domain_parse() {
        let c = build(
            &[
                ("JWT_SECRET", GOOD_JWT),
                (
                    "ADDITIONAL_CORS_ORIGINS",
                    " https://a.com , ,https://b.com ",
                ),
                ("NOSDESK_TENANT_DOMAIN", " nosdesk.app "),
            ],
            true,
        )
        .unwrap();
        assert_eq!(c.additional_origins, vec!["https://a.com", "https://b.com"]);
        assert_eq!(c.tenant_domain.as_deref(), Some("nosdesk.app"));
    }

    #[test]
    fn shutdown_timeout_defaults_and_parses() {
        let c = build(&[("JWT_SECRET", GOOD_JWT)], true).unwrap();
        assert_eq!(c.shutdown_timeout_secs, 25); // default

        let c = build(
            &[
                ("JWT_SECRET", GOOD_JWT),
                ("NOSDESK_SHUTDOWN_TIMEOUT_SECS", "40"),
            ],
            true,
        )
        .unwrap();
        assert_eq!(c.shutdown_timeout_secs, 40);

        // Garbage falls back to the default rather than failing the boot.
        let c = build(
            &[
                ("JWT_SECRET", GOOD_JWT),
                ("NOSDESK_SHUTDOWN_TIMEOUT_SECS", "soon"),
            ],
            true,
        )
        .unwrap();
        assert_eq!(c.shutdown_timeout_secs, 25);
    }
}
