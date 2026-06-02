//! Structured error responses for HTTP handlers.
//!
//! # Two patterns, one wire shape
//!
//! Handlers can produce error responses two ways. Both render the
//! same JSON shape (`{ "error": "...", "code": "..." }`); pick
//! whichever fits the surrounding code:
//!
//! ## 1. Free-function helpers (`errors::forbidden(...)` etc.)
//!
//! Returns an `HttpResponse` directly. Use when the handler signature
//! is `impl Responder` and explicit early-return reads cleaner than
//! threading through a `?`. This is the dominant pattern in the
//! existing codebase post-migration.
//!
//! ```ignore
//! pub async fn flag(req: HttpRequest) -> impl Responder {
//!     if !is_admin(&claims) {
//!         return errors::forbidden("Admin required");
//!     }
//!     HttpResponse::Ok().json(...)
//! }
//! ```
//!
//! ## 2. `ApiError` enum + `?` operator (canonical Actix shape)
//!
//! Implements [`actix_web::ResponseError`], so handlers returning
//! `Result<HttpResponse, ApiError>` get the `?` operator and
//! automatic conversion from common error types (`diesel::Error`,
//! `r2d2::Error`). Use this for new handlers where the call chain
//! is mostly fallible operations.
//!
//! ```ignore
//! pub async fn flag(req: HttpRequest, pool: web::Data<Pool>)
//!     -> Result<HttpResponse, ApiError>
//! {
//!     let mut conn = pool.get()?;        // r2d2::Error → 503
//!     let user = users::get(&mut conn)?; // diesel::Error → mapped
//!     if !is_admin(&claims) {
//!         return Err(ApiError::Forbidden("Admin required".into()));
//!     }
//!     Ok(HttpResponse::Ok().json(...))
//! }
//! ```
//!
//! Both patterns produce identical JSON. The enum delegates to the
//! free functions internally, so clients can't tell which the
//! handler used.
//!
//! # Error-code naming
//!
//! `SCREAMING_SNAKE_CASE`, domain-prefixed where useful
//! (`AUTH_REQUIRED`, `DB_UNIQUE_VIOLATION`, `RESOURCE_NOT_FOUND`).
//! Keep them stable; clients branch on them.

use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse, ResponseError};
use serde_json::json;
use tracing::error;

use crate::db::{DbConnection, Pool};

// =================================================================
// Standard error builders
// =================================================================

/// 400 Bad Request — caller sent malformed or invalid input.
/// Uses the generic `BAD_REQUEST` code; callers needing a more
/// specific error code use [`bad_request_with_code`].
pub fn bad_request(message: impl Into<String>) -> HttpResponse {
    bad_request_with_code(message, "BAD_REQUEST")
}

/// 400 Bad Request with a specific machine-readable code clients
/// can branch on (e.g. `INVALID_EMAIL`, `WEAK_PASSWORD`).
pub fn bad_request_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    HttpResponse::BadRequest().json(json!({
        "error": message.into(),
        "code": code,
    }))
}

/// 401 Unauthorized — caller is unauthenticated.
pub fn unauthorized(message: impl Into<String>) -> HttpResponse {
    unauthorized_with_code(message, "AUTH_REQUIRED")
}

/// 401 Unauthorized with a specific machine-readable code.
pub fn unauthorized_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    HttpResponse::Unauthorized().json(json!({
        "error": message.into(),
        "code": code,
    }))
}

/// 403 Forbidden — caller is authenticated but lacks permission.
pub fn forbidden(message: impl Into<String>) -> HttpResponse {
    HttpResponse::Forbidden().json(json!({
        "error": message.into(),
        "code": "FORBIDDEN",
    }))
}

/// 404 Not Found — the named entity doesn't exist or isn't visible.
/// The response body reads "{entity} not found"; clients also get
/// a structured `entity` field for programmatic dispatch.
pub fn not_found(entity: impl Into<String>) -> HttpResponse {
    let entity = entity.into();
    HttpResponse::NotFound().json(json!({
        "error": format!("{} not found", entity),
        "code": "RESOURCE_NOT_FOUND",
        "entity": entity,
    }))
}

/// 404 Not Found with a verbatim message — for cases where the
/// existing copy doesn't fit the "{entity} not found" template.
pub fn not_found_msg(message: impl Into<String>) -> HttpResponse {
    HttpResponse::NotFound().json(json!({
        "error": message.into(),
        "code": "RESOURCE_NOT_FOUND",
    }))
}

/// 409 Conflict — request violates a uniqueness or state constraint.
/// Uses the generic `CONFLICT` code; callers needing a more
/// specific code use [`conflict_with_code`].
pub fn conflict(message: impl Into<String>) -> HttpResponse {
    conflict_with_code(message, "CONFLICT")
}

/// 409 Conflict with a specific machine-readable code.
pub fn conflict_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    HttpResponse::Conflict().json(json!({
        "error": message.into(),
        "code": code,
    }))
}

/// 410 Gone — the resource existed but is permanently no longer
/// available, and no forwarding address is known. Used for one-shot
/// endpoints (e.g. initial-admin setup) once they have been consumed:
/// unlike 404, it tells the client not to retry. Uses the generic
/// `GONE` code; callers needing a specific code use [`gone_with_code`].
pub fn gone(message: impl Into<String>) -> HttpResponse {
    gone_with_code(message, "GONE")
}

/// 410 Gone with a specific machine-readable code.
pub fn gone_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    HttpResponse::Gone().json(json!({
        "error": message.into(),
        "code": code,
    }))
}

/// 422 Unprocessable Entity — request was syntactically valid but
/// semantically invalid (e.g. validation failure on a well-formed
/// payload). Use 400 for malformed input, 422 for "we understood it
/// but it can't be applied."
pub fn unprocessable_entity(message: impl Into<String>) -> HttpResponse {
    HttpResponse::UnprocessableEntity().json(json!({
        "error": message.into(),
        "code": "UNPROCESSABLE_ENTITY",
    }))
}

/// 429 Too Many Requests — caller hit a rate limit. `retry_after` is
/// the suggested backoff in seconds; the value rides both the
/// `Retry-After` header (per RFC 6585) and the JSON body for clients
/// that read either.
pub fn too_many_requests(message: impl Into<String>, retry_after_secs: u64) -> HttpResponse {
    HttpResponse::TooManyRequests()
        .insert_header(("Retry-After", retry_after_secs.to_string()))
        .json(json!({
            "error": message.into(),
            "code": "RATE_LIMITED",
            "retry_after": retry_after_secs,
        }))
}

/// 500 Internal Server Error — generic server-side failure. Prefer
/// `db_error` or a more specific helper when applicable; the raw
/// 500 should be a last resort.
pub fn internal(message: impl Into<String>) -> HttpResponse {
    internal_with_code(message, "INTERNAL_ERROR")
}

/// 500 Internal Server Error with a specific machine-readable code.
pub fn internal_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    HttpResponse::InternalServerError().json(json!({
        "error": message.into(),
        "code": code,
    }))
}

/// 503 Service Unavailable — server is alive but a dependency is
/// temporarily down (DB pool exhausted, Redis unreachable, etc.).
/// Clients can retry with backoff.
pub fn service_unavailable(message: impl Into<String>) -> HttpResponse {
    HttpResponse::ServiceUnavailable()
        .insert_header(("Retry-After", "5"))
        .json(json!({
            "error": message.into(),
            "code": "SERVICE_UNAVAILABLE",
        }))
}

// =================================================================
// Diesel error mapping
// =================================================================

/// Map a Diesel error to the appropriate HTTP status + structured
/// body. Logs the raw error so operators can find it in the server
/// log without leaking column names or input values to clients.
pub fn db_error(err: &diesel::result::Error) -> HttpResponse {
    use diesel::result::DatabaseErrorKind as Kind;
    use diesel::result::Error;

    match err {
        Error::NotFound => HttpResponse::NotFound().json(json!({
            "error": "Resource not found",
            "code": "RESOURCE_NOT_FOUND",
        })),
        Error::DatabaseError(kind, info) => {
            error!(error = ?err, ?kind, message = info.message(), "DB error");
            match kind {
                Kind::UniqueViolation => HttpResponse::Conflict().json(json!({
                    "error": "A record with these values already exists",
                    "code": "DB_UNIQUE_VIOLATION",
                })),
                Kind::ForeignKeyViolation => HttpResponse::BadRequest().json(json!({
                    "error": "Referenced record does not exist",
                    "code": "DB_FOREIGN_KEY_VIOLATION",
                })),
                Kind::NotNullViolation => HttpResponse::BadRequest().json(json!({
                    "error": "A required field was missing",
                    "code": "DB_NOT_NULL_VIOLATION",
                })),
                Kind::CheckViolation => HttpResponse::BadRequest().json(json!({
                    "error": "A field value violated a database constraint",
                    "code": "DB_CHECK_VIOLATION",
                })),
                _ => HttpResponse::InternalServerError().json(json!({
                    "error": "Database operation failed",
                    "code": "DB_ERROR",
                })),
            }
        }
        // Connection-level errors — most likely a transient
        // infrastructure problem rather than a request problem.
        Error::BrokenTransactionManager | Error::AlreadyInTransaction => {
            error!(error = ?err, "DB transaction state error");
            service_unavailable("Database transaction error")
        }
        _ => {
            error!(error = ?err, "Unhandled DB error");
            HttpResponse::InternalServerError().json(json!({
                "error": "Database operation failed",
                "code": "DB_ERROR",
            }))
        }
    }
}

// =================================================================
// Pool acquisition
// =================================================================

/// Acquire a DB connection from the pool, returning a 503 response
/// on exhaustion or timeout. Replaces the panic-on-failure
/// `pool.get().unwrap()` pattern that's scattered through the
/// older handlers, pool exhaustion is a normal runtime condition
/// under load, not a programming error.
pub fn db_conn(pool: &web::Data<Pool>) -> Result<DbConnection, HttpResponse> {
    pool.get().map_err(|e| {
        error!(error = ?e, "DB pool acquire failed");
        service_unavailable("Database connection unavailable, please retry")
    })
}

// =================================================================
// ApiError: canonical Actix error enum
// =================================================================

/// Error type for handlers that return `Result<HttpResponse, ApiError>`.
///
/// Implements [`actix_web::ResponseError`] so handlers get the `?`
/// operator and automatic conversion from common error types
/// (`diesel::result::Error`, `r2d2::Error`). Variants delegate to
/// the free-function helpers above, so the JSON shape on the wire
/// is identical regardless of which pattern a handler uses.
#[derive(thiserror::Error, Debug)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),

    #[error("{0}")]
    Unauthorized(String),

    #[error("{0}")]
    Forbidden(String),

    /// Body renders as `{entity} not found`.
    #[error("{0} not found")]
    NotFound(String),

    #[error("{0}")]
    Conflict(String),

    #[error("{0}")]
    Internal(String),

    #[error("{0}")]
    ServiceUnavailable(String),

    /// Diesel error, mapped via [`db_error`] when rendered.
    #[error(transparent)]
    Database(#[from] diesel::result::Error),

    /// Pool acquire failure, renders as 503.
    #[error(transparent)]
    Pool(#[from] r2d2::Error),
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) => StatusCode::NOT_FOUND,
            ApiError::Conflict(_) => StatusCode::CONFLICT,
            ApiError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::ServiceUnavailable(_) => StatusCode::SERVICE_UNAVAILABLE,
            ApiError::Database(diesel::result::Error::NotFound) => StatusCode::NOT_FOUND,
            ApiError::Database(diesel::result::Error::DatabaseError(kind, _)) => {
                use diesel::result::DatabaseErrorKind as Kind;
                match kind {
                    Kind::UniqueViolation => StatusCode::CONFLICT,
                    Kind::ForeignKeyViolation | Kind::NotNullViolation | Kind::CheckViolation => {
                        StatusCode::BAD_REQUEST
                    }
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                }
            }
            ApiError::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            ApiError::Pool(_) => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    fn error_response(&self) -> HttpResponse {
        match self {
            ApiError::BadRequest(m) => bad_request(m.clone()),
            ApiError::Unauthorized(m) => unauthorized(m.clone()),
            ApiError::Forbidden(m) => forbidden(m.clone()),
            ApiError::NotFound(entity) => not_found(entity.clone()),
            ApiError::Conflict(m) => conflict(m.clone()),
            ApiError::Internal(m) => internal(m.clone()),
            ApiError::ServiceUnavailable(m) => service_unavailable(m.clone()),
            ApiError::Database(e) => db_error(e),
            ApiError::Pool(e) => {
                error!(error = ?e, "DB pool acquire failed");
                service_unavailable("Database connection unavailable, please retry")
            }
        }
    }
}
