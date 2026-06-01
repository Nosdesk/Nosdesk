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
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::handlers::errors;
use crate::middleware::api_token::PlatformScope;
use crate::models::{NewWorkspace, Workspace};
use crate::repository::workspaces::{self, CreateWorkspaceError};
use crate::services::oauth_provisioning::{find_or_create_projected_user, ProjectedUserInput};
use crate::utils::workspace_slug::validate_slug;

/// Header name the idempotency middleware looks for. Duplicated as a
/// string here so this handler can produce a useful 400 when callers
/// forget it; the middleware itself would just pass through (since
/// missing-header is its no-op condition).
const IDEMPOTENCY_HEADER: &str = "Idempotency-Key";

/// Return a 400 response if the request is missing the
/// `Idempotency-Key` header. Centralises the contract check so the
/// three M5 mutating handlers don't each spell it out.
fn require_idempotency_key(req: &HttpRequest) -> Option<HttpResponse> {
    if req.headers().get(IDEMPOTENCY_HEADER).is_none() {
        Some(errors::bad_request(
            "Idempotency-Key header is required for provisioning callbacks",
        ))
    } else {
        None
    }
}

/// Pull a connection from the pool, mapping exhaustion to a 500
/// with a consistent error log. `context` is the operation tag used
/// in the log message.
fn pool_conn(pool: &web::Data<Pool>, context: &str) -> Result<DbConnection, HttpResponse> {
    pool.get().map_err(|e| {
        error!(error = ?e, context = context, "db pool exhausted");
        errors::internal("Database connection failed")
    })
}

/// Resolve a workspace by slug or return the appropriate HTTP
/// response. 404 if the slug doesn't match, 500 on a query error.
/// `context` is the operation tag used in the error log.
fn resolve_workspace_or_respond(
    conn: &mut DbConnection,
    slug: &str,
    context: &str,
) -> Result<Workspace, HttpResponse> {
    match workspaces::find_by_slug(conn, slug) {
        Ok(Some(ws)) => Ok(ws),
        Ok(None) => {
            warn!(slug = %slug, context = context, "workspace not found");
            Err(errors::not_found_msg(format!(
                "workspace '{slug}' not found"
            )))
        }
        Err(e) => {
            error!(error = ?e, slug = %slug, context = context, "workspace lookup failed");
            Err(errors::internal("Workspace lookup failed"))
        }
    }
}

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
    if let Some(resp) = require_idempotency_key(&req) {
        return resp;
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

    let mut conn = match pool_conn(&pool, "workspaces/create") {
        Ok(c) => c,
        Err(resp) => return resp,
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

// =====================================================================
// upsert_projected_user (M5 Task 4)
// =====================================================================
//
// `POST /api/internal/v1/workspaces/{slug}/upsert_projected_user` —
// D8.4 eager owner projection. Creates the `users` row + membership
// grant ahead of the user's first OIDC login so the
// `workspace_members.user_uuid` FK has a target when the control
// plane writes the parent row at provision time.

#[derive(Debug, Deserialize)]
pub struct UpsertProjectedUserRequest {
    /// OIDC `iss` (issuer URL or stable provider identifier).
    /// Stored as `user_auth_identities.provider_type`.
    pub iss: String,
    /// OIDC `sub` (stable per-user identifier from the IdP).
    /// Stored as `user_auth_identities.external_id`.
    pub sub: String,
    pub email: String,
    #[serde(default)]
    pub name: Option<String>,
    /// One of `owner`, `admin`, `member`. First-write-wins on the
    /// `workspace_members` row — re-projecting an existing
    /// membership does NOT silently escalate or downgrade the role
    /// (handoff doc Task 4 gotcha).
    pub role: String,
}

#[derive(Debug, Serialize)]
struct UpsertProjectedUserResponse {
    user_uuid: Uuid,
    workspace_id: i32,
    role: String,
    /// `true` if this call minted the local user row; `false` if
    /// the user already existed (via `(iss, sub)` identity match
    /// or by email fallback).
    created: bool,
}

fn valid_role(role: &str) -> bool {
    matches!(role, "owner" | "admin" | "member")
}

pub async fn upsert_projected_user(
    req: HttpRequest,
    _: PlatformScope,
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<UpsertProjectedUserRequest>,
) -> impl Responder {
    if let Some(resp) = require_idempotency_key(&req) {
        return resp;
    }

    let slug = path.into_inner();
    let UpsertProjectedUserRequest {
        iss,
        sub,
        email,
        name,
        role,
    } = body.into_inner();

    if iss.trim().is_empty() || sub.trim().is_empty() {
        return errors::bad_request("iss and sub must both be non-empty");
    }
    if email.trim().is_empty() {
        return errors::bad_request("email must be non-empty");
    }
    if !valid_role(&role) {
        return errors::bad_request("role must be one of: owner, admin, member");
    }

    let mut conn = match pool_conn(&pool, "upsert_projected_user") {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    // Resolve workspace by slug. Done first so the 404 path is
    // distinct from "we tried but failed downstream"; matches the
    // handoff's "unknown workspace returns 404" acceptance.
    let workspace = match resolve_workspace_or_respond(&mut conn, &slug, "upsert_projected_user") {
        Ok(ws) => ws,
        Err(resp) => return resp,
    };

    let input = ProjectedUserInput {
        iss,
        sub,
        email,
        name,
        role: role.clone(),
        workspace_id: workspace.id,
        // Eager-projected users authenticate exclusively via OIDC
        // on first login; no fallback password is set so the
        // identity row carries NULL where the lazy path puts a
        // random hash. Loss of the OIDC config later doesn't
        // strand them — operators reset via the admin tools.
        password_hash: None,
        metadata: None,
    };

    match find_or_create_projected_user(&mut conn, input) {
        Ok(outcome) => {
            let created = outcome.is_created();
            let user = outcome.into_user();
            info!(
                user_uuid = %user.uuid,
                workspace_id = workspace.id,
                role = %role,
                created,
                "upsert_projected_user: ok"
            );
            let payload = UpsertProjectedUserResponse {
                user_uuid: user.uuid,
                workspace_id: workspace.id,
                role,
                created,
            };
            if created {
                HttpResponse::Created().json(payload)
            } else {
                HttpResponse::Ok().json(payload)
            }
        }
        Err(e) => {
            error!(error = %e, slug = %slug, "upsert_projected_user: provisioning failed");
            errors::internal("Failed to project user")
        }
    }
}

// =====================================================================
// PATCH /api/internal/v1/workspaces/{slug}/custom-domain (M5 Task 5)
// =====================================================================

#[derive(Debug, Deserialize)]
pub struct CustomDomainRequest {
    /// New custom-domain hostname, or `null` to clear. The control
    /// plane has already verified DNS + Fly Certs by the time it
    /// calls this; we only do structural validation here.
    pub hostname: Option<String>,
}

#[derive(Debug, Serialize)]
struct CustomDomainResponse {
    workspace_uuid: Uuid,
    slug: String,
    custom_domain: Option<String>,
}

/// Lightweight FQDN check: lowercase, contains a dot, no leading /
/// trailing dot or hyphen, ASCII only. Loose by intent — the
/// control plane does the heavy validation (DNS resolution,
/// certificate provisioning); this layer rejects garbage that
/// could break downstream code paths.
fn looks_like_fqdn(s: &str) -> bool {
    let len = s.len();
    if !(3..=253).contains(&len) {
        return false;
    }
    if !s.contains('.') {
        return false;
    }
    if s.starts_with('.') || s.ends_with('.') || s.starts_with('-') || s.ends_with('-') {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-')
}

pub async fn set_custom_domain(
    req: HttpRequest,
    _: PlatformScope,
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<CustomDomainRequest>,
) -> impl Responder {
    if let Some(resp) = require_idempotency_key(&req) {
        return resp;
    }

    let slug = path.into_inner();
    let hostname_normalised = match body.into_inner().hostname {
        Some(h) => {
            let trimmed = h.trim().to_ascii_lowercase();
            if trimmed.is_empty() {
                None
            } else if !looks_like_fqdn(&trimmed) {
                return errors::bad_request(
                    "hostname must be a lowercase ASCII FQDN (e.g. support.acme.com)",
                );
            } else {
                Some(trimmed)
            }
        }
        None => None,
    };

    let mut conn = match pool_conn(&pool, "custom_domain") {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    // Capture the previous value so we can invalidate its cache key
    // even when the operator is clearing or changing the hostname.
    let previous = match resolve_workspace_or_respond(&mut conn, &slug, "custom_domain") {
        Ok(ws) => ws,
        Err(resp) => return resp,
    };

    let updated = match workspaces::update_custom_domain(
        &mut conn,
        &slug,
        hostname_normalised.as_deref(),
    ) {
        Ok(Some(ws)) => ws,
        Ok(None) => {
            // Shouldn't happen — we just looked up by the same slug.
            return errors::not_found_msg(format!("workspace '{slug}' not found"));
        }
        Err(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        )) => {
            warn!(slug = %slug, hostname = ?hostname_normalised, "custom_domain: hostname already in use");
            return HttpResponse::Conflict().json(serde_json::json!({
                "error": "hostname_taken",
                "message": "this hostname is already mapped to a workspace",
            }));
        }
        Err(e) => {
            error!(error = ?e, slug = %slug, "custom_domain: update failed");
            return errors::internal("Failed to update custom domain");
        }
    };

    // Invalidate cache for both the previous and current host
    // mappings so the next request sees the change immediately.
    // Subdomain (slug) cache stays — the slug-to-workspace mapping
    // hasn't changed.
    if let Some(prev) = previous.custom_domain.as_deref() {
        crate::middleware::workspace_context::invalidate_cache_key(&format!("host:{prev}"));
    }
    if let Some(new) = updated.custom_domain.as_deref() {
        crate::middleware::workspace_context::invalidate_cache_key(&format!("host:{new}"));
    }

    info!(
        slug = %updated.slug,
        custom_domain = ?updated.custom_domain,
        "custom_domain: updated"
    );
    HttpResponse::Ok().json(CustomDomainResponse {
        workspace_uuid: updated.uuid,
        slug: updated.slug,
        custom_domain: updated.custom_domain,
    })
}
