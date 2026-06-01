//! Internal provisioning endpoints under `/api/internal/v1/...`
//! consumed by the control plane (`~/dev/nosdesk-com`).
//!
//! These are platform-scoped: the caller must hold an api_token
//! flagged `is_platform_scoped = true` (minted operator-side, NOT
//! workspace-bound). The middleware-level auth + the per-handler
//! `PlatformScope` extractor double-guard the surface so a
//! user-bound token can't reach these endpoints even on accident.
//!
//! Idempotency: every mutating endpoint here is wrapped by the
//! `idempotency_middleware`. Callers MUST supply `Idempotency-Key`
//! on POST / PUT / PATCH; the handler returns 400 if they don't
//! (enforced via an explicit check, not relying on middleware
//! semantics, so the contract is clear from the handler signature).
//!
//! See `docs/m5-product-side-handoff.md` Tasks 3-5 for the broader
//! shape of this surface.

use actix_web::{web, HttpRequest, HttpResponse, Responder};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::errors;
use crate::middleware::api_token::PlatformScope;
use crate::models::NewWorkspace;
use crate::repository::workspaces::{self, CreateWorkspaceError};

/// Application-layer slug rule (M5 handoff Task 3). Stricter than
/// the DB CHECK (`^[a-z0-9](...){0,62}[a-z0-9]$`):
///   * must start with a letter (DB allows digits)
///   * must end with letter or digit (no trailing hyphen)
///   * no consecutive hyphens (DB doesn't enforce)
///   * 3–40 chars (DB allows 1–64)
///
/// The control plane owns the reserved-word denylist; this layer is
/// purely structural. Cross-tenant collision on a reserved slug is
/// impossible because `workspaces.slug` is UNIQUE and reserved slugs
/// can't pass the control plane's gate.
static SLUG_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[a-z][a-z0-9-]*[a-z0-9]$").expect("compile slug regex"));

#[derive(Debug)]
enum SlugError {
    BadLength,
    BadShape,
    ConsecutiveHyphens,
}

impl SlugError {
    fn as_message(&self) -> &'static str {
        match self {
            Self::BadLength => "slug must be 3 to 40 characters",
            Self::BadShape => {
                "slug must be lowercase letters, digits, and hyphens; start with a letter and end with a letter or digit"
            }
            Self::ConsecutiveHyphens => "slug must not contain consecutive hyphens",
        }
    }
}

fn validate_slug(slug: &str) -> Result<(), SlugError> {
    if !(3..=40).contains(&slug.len()) {
        return Err(SlugError::BadLength);
    }
    if !SLUG_RE.is_match(slug) {
        return Err(SlugError::BadShape);
    }
    if slug.contains("--") {
        return Err(SlugError::ConsecutiveHyphens);
    }
    Ok(())
}

/// Header name the idempotency middleware looks for. Duplicated as a
/// string here so this handler can produce a useful 400 when callers
/// forget it; the middleware itself would just pass through (since
/// missing-header is its no-op condition).
const IDEMPOTENCY_HEADER: &str = "Idempotency-Key";

/// Request body for `POST /api/internal/v1/workspaces/create`.
/// `owner_user_uuid` / `owner_email` / `owner_name` are accepted so
/// the request shape matches the M5 plan, but THIS endpoint does
/// nothing with them — the eager-projection write happens in the
/// separate `upsert_projected_user` endpoint (M5 Task 4). The
/// control plane calls create first, then upsert_projected_user;
/// separating them keeps both individually retryable.
#[derive(Debug, Deserialize)]
pub struct CreateWorkspaceRequest {
    pub slug: String,
    pub name: String,
    pub owner_user_uuid: Uuid,
    pub owner_email: String,
    #[serde(default)]
    pub owner_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateWorkspaceResponse {
    workspace_uuid: Uuid,
    slug: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// `POST /api/internal/v1/workspaces/create` — see module docs.
/// Returns 201 on first call, 409 on slug collision, 400 on a
/// missing Idempotency-Key or malformed slug.
pub async fn create_workspace(
    req: HttpRequest,
    _: PlatformScope,
    pool: web::Data<Pool>,
    body: web::Json<CreateWorkspaceRequest>,
) -> impl Responder {
    // Enforce the Idempotency-Key contract here even though the
    // middleware will also see it. The middleware's "no header =
    // pass through" semantics is fine for non-critical routes but
    // wrong for provisioning, where a missing header means the
    // caller has bypassed retry safety and we'd rather refuse.
    if req.headers().get(IDEMPOTENCY_HEADER).is_none() {
        return errors::bad_request(
            "Idempotency-Key header is required for provisioning callbacks",
        );
    }

    let CreateWorkspaceRequest {
        slug,
        name,
        owner_user_uuid: _,
        owner_email: _,
        owner_name: _,
    } = body.into_inner();

    if let Err(e) = validate_slug(&slug) {
        return errors::bad_request(e.as_message());
    }
    if name.trim().is_empty() {
        return errors::bad_request("name must not be empty");
    }

    let mut conn = match pool.get() {
        Ok(c) => c,
        Err(e) => {
            error!(error = ?e, "workspaces/create: db pool exhausted");
            return errors::internal("Database connection failed");
        }
    };

    // Pre-mint the UUID so the response (and the eventual
    // control-plane mirror row) both reference the same identity.
    let workspace_uuid = Uuid::now_v7();
    let record = NewWorkspace {
        uuid: workspace_uuid,
        slug: slug.clone(),
        name: name.clone(),
    };

    match workspaces::create_workspace(&mut conn, &record) {
        Ok(ws) => {
            info!(
                workspace_uuid = %ws.uuid,
                workspace_id = ws.id,
                slug = %ws.slug,
                "workspaces/create: provisioned"
            );
            HttpResponse::Created().json(CreateWorkspaceResponse {
                workspace_uuid: ws.uuid,
                slug: ws.slug,
                created_at: ws.created_at,
            })
        }
        Err(CreateWorkspaceError::SlugTaken) => {
            // Non-enumerable wording: don't distinguish active vs
            // tombstoned slugs (per handoff doc + the W4 slug
            // never-reuse policy).
            warn!(slug = %slug, "workspaces/create: slug collision");
            HttpResponse::Conflict().json(json!({
                "error": "slug_taken",
                "message": format!("slug '{slug}' is unavailable, please choose another"),
            }))
        }
        Err(CreateWorkspaceError::Db(e)) => {
            error!(error = ?e, "workspaces/create: db insert failed");
            errors::internal("Failed to create workspace")
        }
    }
}

#[cfg(test)]
mod slug_tests {
    use super::*;

    #[test]
    fn accepts_valid_slugs() {
        for s in [
            "abc",
            "acme",
            "abc-co",
            "abc1",
            "a-1-b",
            "twelve34five-6789",
        ] {
            assert!(validate_slug(s).is_ok(), "expected {s} to be valid");
        }
    }

    #[test]
    fn rejects_bad_length() {
        assert!(matches!(validate_slug("ab"), Err(SlugError::BadLength)));
        let too_long = "a".repeat(41);
        assert!(matches!(
            validate_slug(&too_long),
            Err(SlugError::BadLength)
        ));
    }

    #[test]
    fn rejects_bad_shape() {
        for s in [
            "1abc",    // starts with digit
            "abc-",    // ends with hyphen
            "-abc",    // starts with hyphen
            "ABC",     // uppercase
            "abc_def", // underscore
            "abc def", // space
        ] {
            assert!(
                matches!(validate_slug(s), Err(SlugError::BadShape)),
                "expected {s} to fail shape rule"
            );
        }
    }

    #[test]
    fn rejects_consecutive_hyphens() {
        assert!(matches!(
            validate_slug("abc--def"),
            Err(SlugError::ConsecutiveHyphens)
        ));
    }
}
