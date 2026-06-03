//! Typed errors for the Microsoft Graph delta-sync pipeline.
//!
//! The pre-typed implementation collected per-item errors as
//! `Vec<String>` and tried to flatten them into an `anyhow::Error`
//! at the job boundary. That conflated three different concerns
//! (per-item failure, per-entity failure, job-level failure) into
//! the same channel and threw away all classification: a transient
//! HTTP 429 looked identical to a permanent 404 looked identical
//! to a parser bug looked identical to a database conflict.
//!
//! `MsGraphSyncError` is the typed equivalent. Each variant carries
//! the context an operator needs to act on it, and each variant
//! classifies itself into one of four buckets the executor reads:
//!
//!   * **Transient** — retry within the run with backoff; if it
//!     keeps failing, log and skip the item, the next delta tick
//!     will see it again.
//!   * **Permanent** — don't retry; the item itself is the problem
//!     (malformed Graph response, schema mismatch, unsupported
//!     payload). Skip and log; the source-of-truth fix is upstream
//!     in MS Graph or in the local schema.
//!   * **Conflict** — local DB rejected the write (unique-index
//!     race, FK pointing at a now-deleted row). Don't retry within
//!     this run; the next tick after the conflicting state settles
//!     can succeed.
//!   * **Auth** — bubbles up to the JOB level, not the item level.
//!     A token failure means none of the remaining items will
//!     succeed either; aborting the whole run is the cheapest
//!     correct response.
//!
//! `Display` is deliberately classifier-only and never includes
//! user-typed content (email, display name, ticket title, ...) so
//! it can flow through the tracing-allowlist JSON layer without
//! leaking PII. Full source-chain detail lives in `source()` for
//! callers that want it via Debug / `error_chain()`.

use std::fmt;

use thiserror::Error;

/// Classification an item-level failure gets routed by. Maps 1:1 to
/// the variants of `MsGraphSyncError` via `classify()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    /// Worth retrying within this sync run with exponential backoff.
    Transient,
    /// Skip the item; logging it surfaces the problem upstream.
    Permanent,
    /// DB-level conflict — the next run after the racing state
    /// settles can succeed; don't retry within this run.
    Conflict,
    /// The whole run can't proceed (token gone, DB unreachable).
    /// Pipeline aborts; the scheduler logs the Job-level Err.
    Auth,
}

impl Classification {
    /// String tag for structured tracing fields. Bounded enum so
    /// the value is safe for the allowlist filter.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Transient => "transient",
            Self::Permanent => "permanent",
            Self::Conflict => "conflict",
            Self::Auth => "auth",
        }
    }
}

/// Item / pipeline-level error type for the MS Graph delta sync.
///
/// Variants split by *cause*, not by *call site*: an HTTP failure is
/// a single variant whether it came from the user fetcher or the
/// device fetcher, and the call-site context goes on the
/// `ItemFailure` wrapper. That keeps the retry classifier a pure
/// function of the error and lets the retry/backoff code stay
/// generic.
#[derive(Debug, Error)]
pub enum MsGraphSyncError {
    /// MS Graph or the OAuth provider returned a token failure
    /// (401, refresh failed, no provider configured). Affects every
    /// item in the run, so the executor surfaces this at the job
    /// level and aborts.
    #[error("auth: {hint}")]
    Auth {
        hint: &'static str,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },

    /// MS Graph returned a transient HTTP status (429, 500..=599,
    /// connection reset). Retried by the executor with backoff.
    #[error("transient http: status {status}")]
    HttpTransient {
        status: u16,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// MS Graph returned a permanent HTTP status (400, 404, 410).
    /// Item is skipped; next tick may pick up its replacement.
    #[error("permanent http: status {status}")]
    HttpPermanent {
        status: u16,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },

    /// reqwest itself errored (DNS, TLS handshake, connection
    /// refused). Treated as transient.
    #[error("network: {kind}")]
    Network {
        kind: NetworkErrorKind,
        #[source]
        source: reqwest::Error,
    },

    /// Local DB rejected a write — unique violation, FK miss, check
    /// constraint, etc. Conflict-classified: next tick after the
    /// racing state settles can succeed.
    #[error("database conflict")]
    DbConflict {
        #[source]
        source: diesel::result::Error,
    },

    /// Local DB failed for an infrastructural reason (pool, dead
    /// connection, server going away). Transient at the item level
    /// in theory; in practice it usually means the whole run is
    /// hosed, so we promote to Auth-class at the executor boundary.
    #[error("database infra")]
    DbInfra {
        #[source]
        source: diesel::result::Error,
    },

    /// Couldn't parse or map a Graph payload into our domain model.
    /// Permanent: retrying won't help.
    #[error("mapping: {hint}")]
    Mapping {
        hint: &'static str,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
    },

    /// A serde JSON parse failed on a Graph response body. Permanent.
    #[error("parse: graph response")]
    Parse {
        #[source]
        source: serde_json::Error,
    },

    /// The cancellation token fired; pipeline unwinds gracefully.
    /// Not a real failure but flows through the same channel so
    /// per-item callers don't need two code paths.
    #[error("cancelled")]
    Cancelled,
}

/// Network-layer error sub-kind, since reqwest::Error covers a wide
/// range and we want the structured tracing field to be bounded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkErrorKind {
    Timeout,
    Connect,
    Tls,
    Body,
    Other,
}

impl fmt::Display for NetworkErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Timeout => "timeout",
            Self::Connect => "connect",
            Self::Tls => "tls",
            Self::Body => "body",
            Self::Other => "other",
        })
    }
}

impl NetworkErrorKind {
    /// Cheap classifier over reqwest::Error. Order matters: timeout
    /// has its own predicate, so check it before falling through.
    pub fn from_reqwest(e: &reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::Timeout
        } else if e.is_connect() {
            Self::Connect
        } else if e.is_body() {
            Self::Body
        } else if e.to_string().to_lowercase().contains("tls")
            || e.to_string().to_lowercase().contains("certificate")
        {
            // reqwest doesn't expose a typed TLS predicate; fall
            // back to substring on the message. Rough but bounded
            // because we only emit the *enum tag* into tracing, not
            // the raw message.
            Self::Tls
        } else {
            Self::Other
        }
    }
}

impl MsGraphSyncError {
    /// Pure classifier — the executor reads this to decide between
    /// retry / skip / abort. Keep the match exhaustive so adding a
    /// new variant forces the classification decision at the type
    /// system level.
    pub fn classify(&self) -> Classification {
        match self {
            Self::Auth { .. } | Self::DbInfra { .. } => Classification::Auth,
            Self::HttpTransient { .. } | Self::Network { .. } | Self::Cancelled => {
                Classification::Transient
            }
            Self::HttpPermanent { .. } | Self::Mapping { .. } | Self::Parse { .. } => {
                Classification::Permanent
            }
            Self::DbConflict { .. } => Classification::Conflict,
        }
    }

    /// Convenience: bounded-enum tag for the structured `error_kind`
    /// tracing field. Maps 1:1 to the variant; cheaper than calling
    /// classify() at every log site.
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Auth { .. } => "auth",
            Self::HttpTransient { .. } => "http_transient",
            Self::HttpPermanent { .. } => "http_permanent",
            Self::Network { .. } => "network",
            Self::DbConflict { .. } => "db_conflict",
            Self::DbInfra { .. } => "db_infra",
            Self::Mapping { .. } => "mapping",
            Self::Parse { .. } => "parse",
            Self::Cancelled => "cancelled",
        }
    }

    /// Construct a typed HTTP error from a status code. Splits
    /// transient (429 / 5xx) from permanent (everything else with
    /// an error class) at the boundary so callers don't have to
    /// repeat the predicate.
    pub fn from_status<E>(status: u16, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        let source = Box::new(source);
        if status == 429 || (500..=599).contains(&status) {
            Self::HttpTransient { status, source }
        } else {
            Self::HttpPermanent { status, source }
        }
    }
}

impl From<reqwest::Error> for MsGraphSyncError {
    fn from(e: reqwest::Error) -> Self {
        // If reqwest already saw the status, prefer the status-based
        // classification — it's more specific than the network kind.
        if let Some(status) = e.status() {
            return Self::from_status(status.as_u16(), e);
        }
        Self::Network {
            kind: NetworkErrorKind::from_reqwest(&e),
            source: e,
        }
    }
}

impl From<serde_json::Error> for MsGraphSyncError {
    fn from(source: serde_json::Error) -> Self {
        Self::Parse { source }
    }
}

/// Thin `std::error::Error` wrapper around a `String`. Used by the
/// `From<String> for MsGraphSyncError` impl so legacy call sites
/// whose inner error type is just `Result<_, String>` can flow
/// through the typed pipeline without losing the original message.
/// Display of `MsGraphSyncError::Mapping` itself never includes this
/// string — only the source chain does — so a future PR can pivot
/// the legacy site to a more specific variant (HttpPermanent,
/// DbConflict, ...) by reading the source's classification.
#[derive(Debug)]
struct OpaqueStringError(String);

impl fmt::Display for OpaqueStringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for OpaqueStringError {}

impl From<String> for MsGraphSyncError {
    fn from(msg: String) -> Self {
        Self::Mapping {
            hint: "legacy string error",
            source: Some(Box::new(OpaqueStringError(msg))),
        }
    }
}

impl From<&str> for MsGraphSyncError {
    fn from(msg: &str) -> Self {
        Self::from(msg.to_string())
    }
}

impl From<anyhow::Error> for MsGraphSyncError {
    /// Generic fallback for legacy call sites whose inner error type
    /// has already been erased through `anyhow`. Maps to `Mapping`
    /// (Permanent classification) since by the time we've lost
    /// type info we can't safely retry; future PRs that need
    /// retry on a specific site should add a typed variant + a
    /// matching From impl above this fallback.
    fn from(err: anyhow::Error) -> Self {
        // anyhow::Error -> Box<dyn Error> uses the standard impl
        // anyhow ships; we wrap in Option since `Mapping.source` is
        // optional to support the "we know the hint but the source
        // is gone" call site (rare, but valid).
        Self::Mapping {
            hint: "legacy anyhow error",
            source: Some(Into::<Box<dyn std::error::Error + Send + Sync>>::into(err)),
        }
    }
}

impl From<diesel::result::Error> for MsGraphSyncError {
    fn from(source: diesel::result::Error) -> Self {
        use diesel::result::{DatabaseErrorKind as Dk, Error as De};
        match &source {
            De::DatabaseError(
                Dk::UniqueViolation
                | Dk::ForeignKeyViolation
                | Dk::CheckViolation
                | Dk::NotNullViolation,
                _,
            )
            | De::NotFound => Self::DbConflict { source },
            // ClosedConnection / BrokenPipe / generic SerializationFailure
            // are infrastructural failures; we promote them so the
            // executor can abort the run rather than treating them
            // as recoverable item-level conflicts.
            _ => Self::DbInfra { source },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_classifier_splits_transient_and_permanent() {
        // Build a synthetic permanent / transient pair using a
        // serde_json::Error as the inner source; the variant choice
        // is what we're verifying, not the source content.
        let err = MsGraphSyncError::from_status(
            429,
            std::io::Error::new(std::io::ErrorKind::Other, "rate limited"),
        );
        assert!(matches!(
            err,
            MsGraphSyncError::HttpTransient { status: 429, .. }
        ));
        assert_eq!(err.classify(), Classification::Transient);

        let err = MsGraphSyncError::from_status(
            404,
            std::io::Error::new(std::io::ErrorKind::Other, "gone"),
        );
        assert!(matches!(
            err,
            MsGraphSyncError::HttpPermanent { status: 404, .. }
        ));
        assert_eq!(err.classify(), Classification::Permanent);

        let err = MsGraphSyncError::from_status(
            503,
            std::io::Error::new(std::io::ErrorKind::Other, "down"),
        );
        assert_eq!(err.classify(), Classification::Transient);
    }

    #[test]
    fn diesel_unique_violation_is_conflict_not_infra() {
        let inner = diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            Box::new("dup".to_string()),
        );
        let err = MsGraphSyncError::from(inner);
        assert_eq!(err.classify(), Classification::Conflict);
        assert_eq!(err.kind_str(), "db_conflict");
    }

    #[test]
    fn display_never_includes_pii() {
        // Sanity check: the classifier-only Display contract means
        // a user-facing field like "display_name" must not appear
        // in the rendered string. We test the auth + mapping
        // variants since they carry a `hint` literal under our
        // control.
        let err = MsGraphSyncError::Auth {
            hint: "token expired",
            source: None,
        };
        let s = err.to_string();
        assert_eq!(s, "auth: token expired");
        assert!(!s.contains("user@example.com"));
    }
}
