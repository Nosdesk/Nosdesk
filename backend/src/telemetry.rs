//! Tracing / logging initialisation for the backend.
//!
//! Kept as a distinct phase of startup so `main` stays a thin composition
//! root.

use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialise the global tracing subscriber. Idempotent: uses `try_init`, so
/// a second call (e.g. from a test that also boots the app) is a no-op rather
/// than a panic.
///
/// Third-party crates that emit per-operation log lines are pinned below the
/// default level so our own code stays grep-able. Tantivy in particular logs
/// every segment open / commit at INFO + DEBUG, which floods the dev backend
/// log (saw ~90% of recent lines from tantivy alone). h2 / hyper / rustls /
/// mio / want similarly emit connection-lifecycle chatter that obscures real
/// signal at debug. Operators can still bump any of these by setting
/// `RUST_LOG` explicitly.
///
/// Production (anything but an explicit `ENVIRONMENT=development` / `dev`,
/// unset included) emits via `RedactingJsonLayer`, a
/// field-allowlist JSON serializer that drops anything outside the policy in
/// `utils::tracing_redact`. Local dev keeps the pretty formatter because
/// developer laptops aren't sub-processors of anyone's data. See the "Log
/// redaction" section of `SECURITY.md` for the policy.
///
/// `EnvFilter` is attached as a per-layer filter via `Layer::with_filter`, not
/// on the registry. This keeps the registry's span store unfiltered, so
/// `LookupSpan` / `ctx.event_scope` always see `tracing_actix_web`'s
/// per-request span (carrying `request_id`) — even when the filter would drop
/// the "served" event. The bare event is suppressed inside the layer by target
/// check.
pub fn init() {
    let settings = LogSettings::from_env(
        std::env::var("ENVIRONMENT").ok().as_deref(),
        std::env::var("LOG_FORMAT").ok().as_deref(),
    );
    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| settings.default_filter());
    let json_logs = settings.json;
    let env_filter = || EnvFilter::new(&log_level);
    let registry = tracing_subscriber::registry();
    let _ = if json_logs {
        registry
            .with(crate::utils::tracing_redact::RedactingJsonLayer.with_filter(env_filter()))
            .try_init()
    } else {
        registry
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_line_number(true)
                    .with_writer(std::io::stdout)
                    .with_filter(env_filter()),
            )
            .try_init()
    };

    info!(log_level = %log_level, "Log level configured");
}

/// How the backend logs, from `ENVIRONMENT` and `LOG_FORMAT`.
#[derive(Debug, PartialEq, Eq)]
struct LogSettings {
    /// The redacting JSON layer rather than the pretty formatter.
    json: bool,
    /// `info` rather than `debug` when `RUST_LOG` is unset.
    production: bool,
}

impl LogSettings {
    /// Production follows the same rule as the rest of the security posture
    /// ([`crate::config_utils::assume_production_from`]): anything but an
    /// explicit `development` / `dev`, an unset `ENVIRONMENT` included. So an
    /// install that never sets it gets redacted JSON logs, and the dev stack,
    /// which sets `ENVIRONMENT=development`, keeps the pretty formatter. An
    /// explicit `LOG_FORMAT` wins either way: `json` forces JSON, anything else
    /// the pretty formatter (a debugging session on a production host).
    fn from_env(environment: Option<&str>, log_format: Option<&str>) -> Self {
        let production = crate::config_utils::assume_production_from(environment);
        let json = match log_format.map(str::trim) {
            Some(f) if f.eq_ignore_ascii_case("json") => true,
            Some(_) => false,
            None => production,
        };
        Self { json, production }
    }

    fn default_filter(&self) -> String {
        let base = if self.production { "info" } else { "debug" };
        format!(
            "{base},tantivy=warn,h2=info,hyper=info,hyper_util=info,rustls=info,mio=info,want=info"
        )
    }
}

#[cfg(test)]
mod tests {
    use super::LogSettings;

    #[test]
    fn an_unset_environment_logs_like_production() {
        let s = LogSettings::from_env(None, None);
        assert!(s.json, "unset ENVIRONMENT must get the redacting layer");
        assert!(s.default_filter().starts_with("info,"));
        for env in ["", "  ", "production", "Production", "staging", "prod"] {
            assert!(LogSettings::from_env(Some(env), None).json, "{env:?}");
        }
    }

    #[test]
    fn an_explicit_dev_environment_keeps_the_pretty_formatter() {
        for env in ["development", "dev", "DEV", " Development "] {
            let s = LogSettings::from_env(Some(env), None);
            assert!(!s.json, "{env:?}");
            assert!(s.default_filter().starts_with("debug,"));
        }
    }

    #[test]
    fn log_format_wins_either_way() {
        assert!(LogSettings::from_env(Some("development"), Some("json")).json);
        assert!(LogSettings::from_env(Some("development"), Some("JSON")).json);
        assert!(!LogSettings::from_env(None, Some("pretty")).json);
        assert!(!LogSettings::from_env(Some("production"), Some("pretty")).json);
        // The level still follows the environment.
        assert!(LogSettings::from_env(None, Some("pretty"))
            .default_filter()
            .starts_with("info,"));
    }
}
