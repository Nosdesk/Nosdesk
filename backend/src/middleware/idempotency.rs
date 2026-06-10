//! Idempotency-Key middleware (M5 product-side handoff Task 2).
//!
//! On POST / PUT / PATCH requests carrying an `Idempotency-Key`
//! header, this middleware:
//!
//!   * Checks `idempotency_keys` for a cached response keyed on
//!     `<request path>:<header value>` (per-route scoping so two
//!     handlers can't collide on the same caller-side key).
//!   * On HIT, replays the cached body + status verbatim, without
//!     invoking the inner handler. Retries are byte-identical to the
//!     first call's response.
//!   * On MISS, runs the inner handler, then caches the response
//!     before returning it to the caller — but only when the response
//!     is a success (2xx). Errors are safe to retry and may yet
//!     succeed, so they're never pinned under the key. A best-effort
//!     cache write (non-fatal if the row already exists from a
//!     concurrent racer — see `repository::idempotency_keys` for the
//!     race note).
//!
//! Non-mutating methods and missing-header requests pass through
//! unchanged so this middleware is safe to wrap broadly; it only
//! ever does extra work on the M5 internal callback surface.
//!
//! The retention sweeper lives in `services::scheduled_jobs::
//! prune_idempotency_keys` (24h horizon, configurable via
//! `IDEMPOTENCY_KEY_RETENTION_HOURS`).

use actix_web::body::{to_bytes, BoxBody, MessageBody};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::http::header::CONTENT_TYPE;
use actix_web::http::{Method, StatusCode};
use actix_web::middleware::Next;
use actix_web::{web, Error, HttpResponse};
use tracing::{debug, warn};

use crate::db::Pool;
use crate::repository::idempotency_keys;

const HEADER_NAME: &str = "Idempotency-Key";

/// Cap on response body size we will cache, in bytes. Responses
/// larger than this pass through uncached (the alternative — a
/// multi-MiB row per retry — is worse than not caching). 1 MiB is
/// well above any current M5 callback response, which sit in the
/// hundreds of bytes.
const MAX_CACHED_BODY_BYTES: usize = 1024 * 1024;

pub async fn idempotency_middleware(
    req: ServiceRequest,
    next: Next<impl MessageBody + 'static>,
) -> Result<ServiceResponse<BoxBody>, Error> {
    let is_mutating = matches!(*req.method(), Method::POST | Method::PUT | Method::PATCH);
    let header_val = req
        .headers()
        .get(HEADER_NAME)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    // Pass-through path: non-mutating methods or no header. We
    // erase the body type to BoxBody so the return type unifies
    // with the cache-hit / cache-miss branches below.
    if !is_mutating || header_val.is_none() {
        let res = next.call(req).await?;
        return Ok(res.map_into_boxed_body());
    }

    let header_val = header_val.expect("checked Some above");
    let scoped_key = format!("{}:{}", req.path(), header_val);
    let pool = req.app_data::<web::Data<Pool>>().cloned().ok_or_else(|| {
        actix_web::error::ErrorInternalServerError(
            "Idempotency middleware: DB pool not found in app data",
        )
    })?;

    // Cache lookup. Failures here are non-fatal (treat as miss);
    // we'd rather run the handler than 500 a request because the
    // cache table was briefly unreachable.
    let cached = {
        let mut conn = pool
            .get()
            .map_err(|e| actix_web::error::ErrorInternalServerError(format!("db conn: {e}")))?;
        match idempotency_keys::try_get(&mut conn, &scoped_key) {
            Ok(record) => record,
            Err(e) => {
                warn!(key = %scoped_key, error = ?e, "idempotency lookup failed; treating as miss");
                None
            }
        }
    };

    if let Some(record) = cached {
        debug!(
            key = %scoped_key,
            status = record.response_status,
            "idempotency hit; replaying cached response"
        );
        let status = StatusCode::from_u16(record.response_status as u16).unwrap_or(StatusCode::OK);
        let body_bytes = serde_json::to_vec(&record.response_body).map_err(|e| {
            actix_web::error::ErrorInternalServerError(format!(
                "serialize cached idempotency body: {e}"
            ))
        })?;
        let response = HttpResponse::build(status)
            .content_type("application/json")
            .body(body_bytes);
        let (http_req, _) = req.into_parts();
        return Ok(ServiceResponse::new(http_req, response));
    }

    // Cache miss: run the inner handler, then cache its response.
    let res = next.call(req).await?;
    let (http_req, response) = res.into_parts();
    let status = response.status();
    // Preserve original headers across the body reconstruction so
    // CORS, Content-Type, etc. survive intact.
    let headers = response.headers().clone();

    let body_bytes = to_bytes(response.into_body())
        .await
        .map_err(|_| actix_web::error::ErrorInternalServerError("read response body"))?;

    // Only successful (2xx) responses are cached. An idempotency key
    // exists to keep a request that already *succeeded* from running
    // twice; an error response (validation, conflict, auth failure,
    // transient 5xx) is safe to retry and may yet succeed, so caching it
    // would wrongly pin a stale failure under the key. Auth failures
    // don't reach this middleware anymore (auth runs ahead of it on the
    // provisioning scope), but gating on success keeps the policy
    // correct for every caller, present and future.
    if !status.is_success() {
        debug!(
            key = %scoped_key,
            status = status.as_u16(),
            "non-success response; not cached (retry is safe)"
        );
    } else if body_bytes.len() > MAX_CACHED_BODY_BYTES {
        warn!(
            key = %scoped_key,
            bytes = body_bytes.len(),
            "response body exceeds idempotency cache limit; not cached"
        );
    } else {
        // Only cache JSON responses — they're the only thing the
        // M5 internal callbacks return, and JSONB storage forces
        // the assumption. Non-JSON bodies (HTML error pages,
        // streamed downloads) pass through without caching.
        let looks_json = headers
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.to_ascii_lowercase().starts_with("application/json"));
        if looks_json {
            match serde_json::from_slice::<serde_json::Value>(&body_bytes) {
                Ok(json_body) => {
                    let mut conn = pool.get().map_err(|e| {
                        actix_web::error::ErrorInternalServerError(format!("db conn: {e}"))
                    })?;
                    if let Err(e) = idempotency_keys::upsert(
                        &mut conn,
                        &scoped_key,
                        status.as_u16() as i16,
                        &json_body,
                    ) {
                        warn!(
                            key = %scoped_key,
                            error = ?e,
                            "idempotency cache write failed (non-fatal)"
                        );
                    } else {
                        debug!(
                            key = %scoped_key,
                            status = status.as_u16(),
                            "idempotency cache stored"
                        );
                    }
                }
                Err(e) => {
                    warn!(key = %scoped_key, error = ?e, "response not parseable as JSON; skipped cache");
                }
            }
        } else {
            debug!(key = %scoped_key, "non-JSON Content-Type; skipped idempotency cache");
        }
    }

    // Rebuild the response so the caller sees the body even though
    // we consumed it for caching. Replay status + headers + body.
    let mut builder = HttpResponse::build(status);
    for (name, value) in headers.iter() {
        builder.insert_header((name.clone(), value.clone()));
    }
    let new_response = builder.body(body_bytes);
    Ok(ServiceResponse::new(http_req, new_response))
}
