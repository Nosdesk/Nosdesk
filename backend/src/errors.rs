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
//! Returns an `HttpResponse` directly. For handlers that stay
//! `impl Responder` because nothing in them is fallible, and for the
//! bespoke shapes the enum doesn't cover (`*_with_code`,
//! `too_many_requests`, `externally_managed`, ...).
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
//! `r2d2::Error`, `actix_web::Error`). This is the shape of every
//! handler that calls a fallible helper, and the one new handlers
//! should take.
//!
//! Every builder stamps an [`ErrorKind`] on the response it returns, so the
//! canonical request event reports `error_kind` whichever pattern produced
//! the response. Only a raw `HttpResponse::BadRequest()` built by hand in a
//! handler carries no kind; use the builders.
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
//! Fallible helpers return `Result<T, ApiError>`, or `actix_web::Result<T>`
//! when the failure is a bespoke response (see [`from_response`]); never
//! `Result<T, HttpResponse>`, which is too large an error type for `?`
//! (clippy `result_large_err`). Both propagate with `?` from a
//! `Result<HttpResponse, ApiError>` handler.
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
// Error kind stamp
// =================================================================

/// Stable, low-cardinality machine code for an error response, attached to
/// the response extensions by every builder below so the canonical wide
/// event can report `error_kind` without any handler restating it. Distinct
/// from `status_code`: it separates a unique-violation 409 from a plain
/// conflict, and a pool outage 503 from a service-unavailable 503, so
/// `group by error_kind` is possible. Read by
/// `middleware::request_context::emit_canonical_event`.
#[derive(Clone, Copy)]
pub struct ErrorKind(pub &'static str);

/// Attach `kind` to `resp`. Every builder ends with this; a later stamp
/// replaces an earlier one, which is how `db_error` overrides the kind of
/// the builder it delegates to.
fn stamp(mut resp: HttpResponse, kind: &'static str) -> HttpResponse {
    resp.extensions_mut().insert(ErrorKind(kind));
    resp
}

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
    stamp(
        HttpResponse::BadRequest().json(json!({
            "error": message.into(),
            "code": code,
        })),
        "bad_request",
    )
}

/// 401 Unauthorized — caller is unauthenticated.
pub fn unauthorized(message: impl Into<String>) -> HttpResponse {
    unauthorized_with_code(message, "AUTH_REQUIRED")
}

/// 401 Unauthorized with a specific machine-readable code.
pub fn unauthorized_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    // RFC 7235 requires a challenge on every 401. Sessions are bearer
    // tokens (cookie or header), so the scheme is `Bearer`; browsers only
    // prompt for Basic/Digest, so this never raises a native dialog.
    stamp(
        HttpResponse::Unauthorized()
            .insert_header(("WWW-Authenticate", "Bearer"))
            .json(json!({
                "error": message.into(),
                "code": code,
            })),
        "unauthorized",
    )
}

/// 403 Forbidden — caller is authenticated but lacks permission.
pub fn forbidden(message: impl Into<String>) -> HttpResponse {
    forbidden_with_code(message, "FORBIDDEN")
}

/// 403 Forbidden with a specific machine-readable code (e.g.
/// `seat_limit_reached`, which the control plane branches on).
pub fn forbidden_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    stamp(
        HttpResponse::Forbidden().json(json!({
            "error": message.into(),
            "code": code,
        })),
        "forbidden",
    )
}

/// 404 Not Found — the named entity doesn't exist or isn't visible.
/// The response body reads "{entity} not found"; clients also get
/// a structured `entity` field for programmatic dispatch.
pub fn not_found(entity: impl Into<String>) -> HttpResponse {
    let entity = entity.into();
    stamp(
        HttpResponse::NotFound().json(json!({
            "error": format!("{} not found", entity),
            "code": "RESOURCE_NOT_FOUND",
            "entity": entity,
        })),
        "not_found",
    )
}

/// 404 Not Found with a verbatim message — for cases where the
/// existing copy doesn't fit the "{entity} not found" template.
pub fn not_found_msg(message: impl Into<String>) -> HttpResponse {
    not_found_with_code(message, "RESOURCE_NOT_FOUND")
}

/// 404 Not Found with a specific machine-readable code.
pub fn not_found_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    stamp(
        HttpResponse::NotFound().json(json!({
            "error": message.into(),
            "code": code,
        })),
        "not_found",
    )
}

/// 409 Conflict — request violates a uniqueness or state constraint.
/// Uses the generic `CONFLICT` code; callers needing a more
/// specific code use [`conflict_with_code`].
pub fn conflict(message: impl Into<String>) -> HttpResponse {
    conflict_with_code(message, "CONFLICT")
}

/// 409 Conflict with a specific machine-readable code.
pub fn conflict_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    stamp(
        HttpResponse::Conflict().json(json!({
            "error": message.into(),
            "code": code,
        })),
        "conflict",
    )
}

/// 409 for a staff-seat action refused because the control plane owns the
/// identity in hosted mode. `code: "externally_managed"` is the stable code the
/// SPA reflects; the product must hand the caller off to the control plane.
pub fn externally_managed() -> HttpResponse {
    let resp = conflict_with_code(
        "Team members are managed in the Nosdesk control plane. Add, re-role, or remove seats there.",
        "externally_managed",
    );
    stamp(resp, "externally_managed")
}

/// 409 for a change to part of a hosted staff member's identity (name, avatar,
/// email addresses) that their Nosdesk account owns. Same code as
/// [`externally_managed`] so clients handle both alike.
pub fn identity_managed_in_account() -> HttpResponse {
    let resp = conflict_with_code(
        "This is managed in their Nosdesk account. Change it there.",
        "externally_managed",
    );
    stamp(resp, "externally_managed")
}

/// 409 for a local-credential action refused because local password auth is
/// disabled (hosted mode), where identity is SSO/portal-owned. Distinct from
/// [`externally_managed`]: this is not staff-specific (no one has a local
/// password in hosted), so it stays a plain "not available here".
pub fn local_auth_disabled() -> HttpResponse {
    let resp = conflict_with_code(
        "Local password authentication is disabled on this instance.",
        "local_auth_disabled",
    );
    stamp(resp, "local_auth_disabled")
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
    stamp(
        HttpResponse::Gone().json(json!({
            "error": message.into(),
            "code": code,
        })),
        "gone",
    )
}

/// 400 for a payload that failed several field checks at once. `errors`
/// lists them, one human-readable line per field, so a form can show all
/// of them in one round trip.
pub fn validation_failed(errors: Vec<String>) -> HttpResponse {
    stamp(
        HttpResponse::BadRequest().json(json!({
            "error": "Validation failed",
            "code": "VALIDATION_FAILED",
            "errors": errors,
        })),
        "bad_request",
    )
}

/// 413 Payload Too Large — the upload exceeds the configured limit.
pub fn payload_too_large(message: impl Into<String>) -> HttpResponse {
    stamp(
        HttpResponse::PayloadTooLarge().json(json!({
            "error": message.into(),
            "code": "PAYLOAD_TOO_LARGE",
        })),
        "payload_too_large",
    )
}

/// 422 Unprocessable Entity — request was syntactically valid but
/// semantically invalid (e.g. validation failure on a well-formed
/// payload). Use 400 for malformed input, 422 for "we understood it
/// but it can't be applied."
pub fn unprocessable_entity(message: impl Into<String>) -> HttpResponse {
    stamp(
        HttpResponse::UnprocessableEntity().json(json!({
            "error": message.into(),
            "code": "UNPROCESSABLE_ENTITY",
        })),
        "unprocessable_entity",
    )
}

/// 429 Too Many Requests — caller hit a rate limit. `retry_after` is
/// the suggested backoff in seconds; the value rides both the
/// `Retry-After` header (per RFC 6585) and the JSON body for clients
/// that read either.
pub fn too_many_requests(message: impl Into<String>, retry_after_secs: u64) -> HttpResponse {
    stamp(
        HttpResponse::TooManyRequests()
            .insert_header(("Retry-After", retry_after_secs.to_string()))
            .json(json!({
                "error": message.into(),
                "code": "RATE_LIMITED",
                "retry_after": retry_after_secs,
            })),
        "rate_limited",
    )
}

/// 500 Internal Server Error — generic server-side failure. Prefer
/// `db_error` or a more specific helper when applicable; the raw
/// 500 should be a last resort.
pub fn internal(message: impl Into<String>) -> HttpResponse {
    internal_with_code(message, "INTERNAL_ERROR")
}

/// 500 Internal Server Error with a specific machine-readable code.
pub fn internal_with_code(message: impl Into<String>, code: &str) -> HttpResponse {
    stamp(
        HttpResponse::InternalServerError().json(json!({
            "error": message.into(),
            "code": code,
        })),
        "internal",
    )
}

/// 502 Bad Gateway — an upstream this request depends on (a plugin
/// registry, a CDN) answered badly. Distinct from 503 so the client
/// knows retrying here will not help until the upstream does.
pub fn bad_gateway(message: impl Into<String>) -> HttpResponse {
    stamp(
        HttpResponse::BadGateway().json(json!({
            "error": message.into(),
            "code": "BAD_GATEWAY",
        })),
        "bad_gateway",
    )
}

/// 503 Service Unavailable — server is alive but a dependency is
/// temporarily down (DB pool exhausted, Redis unreachable, etc.).
/// Clients can retry with backoff.
pub fn service_unavailable(message: impl Into<String>) -> HttpResponse {
    stamp(
        HttpResponse::ServiceUnavailable()
            .insert_header(("Retry-After", "5"))
            .json(json!({
                "error": message.into(),
                "code": "SERVICE_UNAVAILABLE",
            })),
        "service_unavailable",
    )
}

/// The `{error, code}` body with extra diagnostic fields: a sample of the
/// rows a change would break, the limit the caller hit, a URL to retry
/// against. `extra` must be a JSON object; its `error` and `code` keys, if
/// any, are overwritten so the contract holds. The kind stamp follows the
/// status.
pub fn with_fields(
    status: StatusCode,
    code: &str,
    message: impl Into<String>,
    extra: serde_json::Value,
) -> HttpResponse {
    let mut body = match extra {
        serde_json::Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    body.insert("error".into(), json!(message.into()));
    body.insert("code".into(), json!(code));
    stamp(
        HttpResponse::build(status).json(body),
        kind_for_status(status),
    )
}

/// The kind a builder stamps for a status, for bodies built by
/// [`with_fields`] rather than a status-specific builder.
fn kind_for_status(status: StatusCode) -> &'static str {
    match status {
        StatusCode::BAD_REQUEST => "bad_request",
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::NOT_FOUND => "not_found",
        StatusCode::CONFLICT => "conflict",
        StatusCode::GONE => "gone",
        StatusCode::PAYLOAD_TOO_LARGE => "payload_too_large",
        StatusCode::UNPROCESSABLE_ENTITY => "unprocessable_entity",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        StatusCode::PAYMENT_REQUIRED => "payment_required",
        StatusCode::BAD_GATEWAY => "bad_gateway",
        StatusCode::SERVICE_UNAVAILABLE => "service_unavailable",
        s if s.is_server_error() => "internal",
        _ => "other",
    }
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

    let (resp, kind) = match err {
        Error::NotFound => (
            HttpResponse::NotFound().json(json!({
                "error": "Resource not found",
                "code": "RESOURCE_NOT_FOUND",
            })),
            "not_found",
        ),
        Error::DatabaseError(kind, info) => {
            error!(error = ?err, ?kind, message = info.message(), "DB error");
            match kind {
                Kind::UniqueViolation => (
                    HttpResponse::Conflict().json(json!({
                        "error": "A record with these values already exists",
                        "code": "DB_UNIQUE_VIOLATION",
                    })),
                    "db_unique_violation",
                ),
                Kind::ForeignKeyViolation => (
                    HttpResponse::BadRequest().json(json!({
                        "error": "Referenced record does not exist",
                        "code": "DB_FOREIGN_KEY_VIOLATION",
                    })),
                    "db_constraint_violation",
                ),
                Kind::NotNullViolation => (
                    HttpResponse::BadRequest().json(json!({
                        "error": "A required field was missing",
                        "code": "DB_NOT_NULL_VIOLATION",
                    })),
                    "db_constraint_violation",
                ),
                Kind::CheckViolation => (
                    HttpResponse::BadRequest().json(json!({
                        "error": "A field value violated a database constraint",
                        "code": "DB_CHECK_VIOLATION",
                    })),
                    "db_constraint_violation",
                ),
                _ => (
                    HttpResponse::InternalServerError().json(json!({
                        "error": "Database operation failed",
                        "code": "DB_ERROR",
                    })),
                    "db_error",
                ),
            }
        }
        // Connection-level errors — most likely a transient
        // infrastructure problem rather than a request problem.
        Error::BrokenTransactionManager | Error::AlreadyInTransaction => {
            error!(error = ?err, "DB transaction state error");
            (
                service_unavailable("Database transaction error"),
                "db_error",
            )
        }
        _ => {
            error!(error = ?err, "Unhandled DB error");
            (
                HttpResponse::InternalServerError().json(json!({
                    "error": "Database operation failed",
                    "code": "DB_ERROR",
                })),
                "db_error",
            )
        }
    };
    stamp(resp, kind)
}

// =================================================================
// Pool acquisition
// =================================================================

/// Acquire a DB connection from the pool, returning a 503 response
/// on exhaustion or timeout. Replaces the panic-on-failure
/// `pool.get().unwrap()` pattern that's scattered through the
/// older handlers, pool exhaustion is a normal runtime condition
/// under load, not a programming error.
pub fn db_conn(pool: &web::Data<Pool>) -> Result<DbConnection, ApiError> {
    Ok(pool.get()?)
}

/// Carry a fully built response (bespoke body, headers, a redirect) as an
/// `actix_web::Error`, for helpers whose failure isn't one of the plain
/// [`ApiError`] shapes. `cause` is what logs see, so keep it a fixed
/// literal, never request content.
pub fn from_response(cause: &'static str, resp: HttpResponse) -> actix_web::Error {
    actix_web::error::InternalError::from_response(cause, resp).into()
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

    /// 404 with the message verbatim, for copy the entity template
    /// doesn't fit (see [`not_found_msg`]).
    #[error("{0}")]
    NotFoundMsg(String),

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

    /// A ready actix error (a bespoke response from [`from_response`], or
    /// an `actix_web::Result` helper), passed through as is. Renders and
    /// stamps whatever it already carries; nothing is added here. This
    /// makes `ApiError` `!Send`: it is the handler-side type, so don't
    /// return it from `web::block` or a spawned task.
    #[error(transparent)]
    Actix(#[from] actix_web::Error),
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden(_) => StatusCode::FORBIDDEN,
            ApiError::NotFound(_) | ApiError::NotFoundMsg(_) => StatusCode::NOT_FOUND,
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
            ApiError::Actix(e) => e.as_response_error().status_code(),
        }
    }

    fn error_response(&self) -> HttpResponse {
        // Each builder stamps its own `ErrorKind`; nothing is added here.
        match self {
            ApiError::Actix(e) => e.error_response(),
            ApiError::BadRequest(m) => bad_request(m.clone()),
            ApiError::Unauthorized(m) => unauthorized(m.clone()),
            ApiError::Forbidden(m) => forbidden(m.clone()),
            ApiError::NotFound(entity) => not_found(entity.clone()),
            ApiError::NotFoundMsg(m) => not_found_msg(m.clone()),
            ApiError::Conflict(m) => conflict(m.clone()),
            ApiError::Internal(m) => internal(m.clone()),
            ApiError::ServiceUnavailable(m) => service_unavailable(m.clone()),
            ApiError::Database(e) => db_error(e),
            ApiError::Pool(e) => {
                error!(error = ?e, "DB pool acquire failed");
                // Same body as a dependency outage, distinct kind so a pool
                // exhaustion is separable from a generic 503 in the log.
                stamp(
                    service_unavailable("Database connection unavailable, please retry"),
                    "pool_unavailable",
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::body::to_bytes;

    /// A handler's `Err(ApiError)` must render the same body as the
    /// free-function builder it delegates to, plus the `ErrorKind` stamp
    /// the canonical event reads.
    #[actix_web::test]
    async fn api_error_renders_the_builder_body_and_stamps_the_kind() {
        let rendered = HttpResponse::from_error(ApiError::Forbidden("Admin required".into()));
        assert_eq!(rendered.status(), StatusCode::FORBIDDEN);
        assert_eq!(
            rendered.extensions().get::<ErrorKind>().map(|k| k.0),
            Some("forbidden")
        );

        let expected = to_bytes(forbidden("Admin required").into_body())
            .await
            .unwrap();
        let actual = to_bytes(rendered.into_body()).await.unwrap();
        assert_eq!(actual, expected);
    }

    /// A wrapped actix error keeps its own response, stamp included when
    /// it came from a builder and absent when it did not.
    #[actix_web::test]
    async fn actix_variant_passes_the_response_through() {
        let bespoke = from_response("unit", HttpResponse::ImATeapot().body("brew"));
        let rendered = HttpResponse::from_error(ApiError::Actix(bespoke));
        assert_eq!(rendered.status(), StatusCode::IM_A_TEAPOT);
        assert!(rendered.extensions().get::<ErrorKind>().is_none());
        assert_eq!(to_bytes(rendered.into_body()).await.unwrap(), "brew");

        let built = from_response("unit", too_many_requests("slow down", 7));
        let rendered = HttpResponse::from_error(ApiError::Actix(built));
        assert_eq!(rendered.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            rendered.extensions().get::<ErrorKind>().map(|k| k.0),
            Some("rate_limited")
        );
    }

    fn kind_of(resp: &HttpResponse) -> Option<&'static str> {
        resp.extensions().get::<ErrorKind>().map(|k| k.0)
    }

    /// Every builder stamps the kind the canonical event reports, so a
    /// bare `errors::x(..)` return from an `impl Responder` handler is as
    /// observable as an `Err(ApiError)`.
    #[test]
    fn every_builder_stamps_its_kind() {
        let cases: Vec<(HttpResponse, &str)> = vec![
            (bad_request("x"), "bad_request"),
            (bad_request_with_code("x", "INVALID_EMAIL"), "bad_request"),
            (unauthorized("x"), "unauthorized"),
            (unauthorized_with_code("x", "TOKEN_EXPIRED"), "unauthorized"),
            (forbidden("x"), "forbidden"),
            (forbidden_with_code("x", "seat_limit_reached"), "forbidden"),
            (not_found("Ticket"), "not_found"),
            (not_found_msg("x"), "not_found"),
            (not_found_with_code("x", "workspace_not_found"), "not_found"),
            (
                validation_failed(vec!["email: required".into()]),
                "bad_request",
            ),
            (payload_too_large("x"), "payload_too_large"),
            (
                with_fields(
                    StatusCode::CONFLICT,
                    "last_owner",
                    "x",
                    json!({ "sample": [1] }),
                ),
                "conflict",
            ),
            (conflict("x"), "conflict"),
            (conflict_with_code("x", "SLUG_TAKEN"), "conflict"),
            (externally_managed(), "externally_managed"),
            (local_auth_disabled(), "local_auth_disabled"),
            (gone("x"), "gone"),
            (gone_with_code("x", "SETUP_DONE"), "gone"),
            (unprocessable_entity("x"), "unprocessable_entity"),
            (too_many_requests("x", 1), "rate_limited"),
            (internal("x"), "internal"),
            (internal_with_code("x", "BOOM"), "internal"),
            (service_unavailable("x"), "service_unavailable"),
            (bad_gateway("x"), "bad_gateway"),
        ];
        for (resp, expected) in &cases {
            assert_eq!(kind_of(resp), Some(*expected), "kind for {expected}");
        }
    }

    /// Extra fields ride alongside the contract keys and cannot replace them.
    #[actix_web::test]
    async fn with_fields_keeps_error_and_code() {
        let resp = with_fields(
            StatusCode::BAD_REQUEST,
            "UNKNOWN_KEY",
            "unknown include key",
            json!({ "key": "x", "allowed": ["a"], "error": "overwritten" }),
        );
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body: serde_json::Value =
            serde_json::from_slice(&to_bytes(resp.into_body()).await.unwrap()).unwrap();
        assert_eq!(body["error"], "unknown include key");
        assert_eq!(body["code"], "UNKNOWN_KEY");
        assert_eq!(body["allowed"], json!(["a"]));
    }

    /// `db_error` classifies Diesel failures finely enough to separate a
    /// unique violation from a constraint violation from an outage.
    #[test]
    fn db_error_stamps_by_diesel_kind() {
        use diesel::result::{DatabaseErrorInformation, DatabaseErrorKind, Error};

        struct Info;
        impl DatabaseErrorInformation for Info {
            fn message(&self) -> &str {
                "unit"
            }
            fn details(&self) -> Option<&str> {
                None
            }
            fn hint(&self) -> Option<&str> {
                None
            }
            fn table_name(&self) -> Option<&str> {
                None
            }
            fn column_name(&self) -> Option<&str> {
                None
            }
            fn constraint_name(&self) -> Option<&str> {
                None
            }
            fn statement_position(&self) -> Option<i32> {
                None
            }
        }
        let db = |kind| Error::DatabaseError(kind, Box::new(Info));

        assert_eq!(kind_of(&db_error(&Error::NotFound)), Some("not_found"));
        assert_eq!(
            kind_of(&db_error(&db(DatabaseErrorKind::UniqueViolation))),
            Some("db_unique_violation")
        );
        assert_eq!(
            kind_of(&db_error(&db(DatabaseErrorKind::ForeignKeyViolation))),
            Some("db_constraint_violation")
        );
        assert_eq!(
            kind_of(&db_error(&db(DatabaseErrorKind::Unknown))),
            Some("db_error")
        );
        assert_eq!(
            kind_of(&db_error(&Error::BrokenTransactionManager)),
            Some("db_error")
        );
    }
}
