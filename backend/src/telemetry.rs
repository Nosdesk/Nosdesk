//! Tracing / logging initialisation for the backend.
//!
//! Kept as a distinct phase of startup so `main` stays a thin composition
//! root (see `docs/plans/main-bootstrap-refactor.md`).

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
/// Production (`LOG_FORMAT=json`) emits via `RedactingJsonLayer` — a
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
    let log_level = std::env::var("RUST_LOG").unwrap_or_else(|_| {
        let base = if std::env::var("ENVIRONMENT").unwrap_or_default() == "production" {
            "info"
        } else {
            "debug"
        };
        format!(
            "{base},tantivy=warn,h2=info,hyper=info,hyper_util=info,rustls=info,mio=info,want=info"
        )
    });

    let json_logs = std::env::var("LOG_FORMAT").ok().as_deref() == Some("json");
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
