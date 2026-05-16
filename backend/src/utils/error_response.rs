//! Localised JSON error responses for HTTP handlers.
//!
//! Every error body carries both:
//! - `error`: the human-readable message resolved against the
//!   request's effective locale (so curl users, integration
//!   tests, and API clients without a translation catalogue still
//!   get something sensible).
//! - `code`: the stable FTL key. Tests assert on this. Frontends
//!   can re-translate when the user changes locale mid-session
//!   without waiting for the next round-trip.
//!
//! Use `json_error` for static keys and `json_error_with` when a
//! Fluent message needs interpolation arguments (`Plugin { $name }
//! not found`, etc.).

use actix_web::{http::StatusCode, HttpResponse};
use fluent_bundle::FluentValue;
use serde_json::json;
use unic_langid::LanguageIdentifier;

use crate::utils::i18n;

/// Build a JSON error response with both the translated message
/// and the stable FTL key. Convention: `code` is in the
/// `backend-error-*` namespace.
pub fn json_error(locale: &LanguageIdentifier, code: &str, status: StatusCode) -> HttpResponse {
    let message = i18n::tr(locale, code);
    HttpResponse::build(status).json(json!({
        "error": message,
        "code": code,
    }))
}

/// Variant that lets the caller pass Fluent interpolation args
/// for dynamic error strings.
pub fn json_error_with(
    locale: &LanguageIdentifier,
    code: &str,
    args: &[(&str, FluentValue<'static>)],
    status: StatusCode,
) -> HttpResponse {
    let message = i18n::tr_with(locale, code, args);
    HttpResponse::build(status).json(json!({
        "error": message,
        "code": code,
    }))
}
