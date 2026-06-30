//! Internal provisioning endpoints under `/api/internal/v1/...`
//! consumed by the control plane (`~/dev/nosdesk-com`).
//!
//! These are platform-only: the caller (the control plane) presents a
//! short-lived EdDSA JWT signed with the platform key, verified by the
//! per-handler `PlatformAuth` extractor against `PLATFORM_PUBLIC_KEY` /
//! `PLATFORM_ISSUER`. Self-hosted instances 404 the surface entirely.
//! No api_token / cookie auth is involved.
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
use crate::extractors::PlatformAuth;
use crate::handlers::errors;
use crate::models::{NewWorkspace, Workspace};
use crate::repository::workspaces::{self, CreateWorkspaceError, UpdateMembershipRoleResult};
use crate::services::oauth_provisioning::{find_or_create_projected_user, ProjectedUserInput};
use crate::services::search::{indexing_tasks, SearchService};
use crate::utils::workspace_slug::validate_slug;
use std::sync::Arc;

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
    /// Staff-seat cap for the new workspace (NULL/absent = unlimited). The
    /// control plane sets this to 5 for self-serve trials; operator and
    /// self-hosted provisions omit it.
    #[serde(default)]
    pub seat_limit: Option<i32>,
}

#[derive(Debug, Serialize)]
struct CreateWorkspaceResponse {
    workspace_uuid: Uuid,
    slug: String,
    created_at: chrono::DateTime<chrono::Utc>,
}

/// `POST /api/internal/v1/workspaces/create` — see module docs.
/// Ensure-exists: 201 the first time, 200 if a *live* workspace with this
/// slug already exists (so the control plane can call this unconditionally
/// and a re-provision self-heals a product-side loss like a reset dev DB),
/// 409 only when the slug is reserved by an archived or hard-deleted
/// workspace, 400 on a missing Idempotency-Key or malformed slug.
pub async fn create_workspace(
    req: HttpRequest,
    _: PlatformAuth,
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
        seat_limit,
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
        seat_limit,
    };

    // Insert the workspace and seed its default content in one
    // transaction, so a seed failure rolls the new workspace row back too:
    // the control plane then retries `create` cleanly rather than
    // inheriting a reachable but unusable (state-less) workspace.
    //
    // The two writes need different roles. The `workspaces` parent is a
    // BYPASSRLS-only table (`nosdesk_app` has SELECT only), so the insert
    // runs under `nosdesk_admin` via `with_actor_bypass_context`. The seed
    // then drops to `nosdesk_app` with `app.workspace_id` pinned to the
    // freshly-minted id (a second `set_actor`), so every seeded row's
    // workspace_id default and audit trigger resolves to this workspace and
    // RLS applies fail-closed. The owner doesn't exist yet (projected by a
    // separate call), so seed rows are authored as NULL.
    let provision_actor = crate::sync::actor::ActorContext::system("workspace:provision");
    let result = crate::sync::session::with_actor_bypass_context::<Workspace, CreateWorkspaceError>(
        &mut conn,
        &provision_actor,
        |c| {
            let ws = workspaces::create_workspace(c, &record)?;
            let seed_actor = crate::sync::actor::ActorContext::system("workspace:provision")
                .with_workspace(ws.id);
            crate::sync::session::set_actor(c, &seed_actor)?;
            crate::services::seed::seed_workspace_defaults(c, None)?;
            Ok(ws)
        },
    );

    match result {
        Ok(ws) => {
            info!(
                workspace_uuid = %ws.uuid,
                workspace_id = ws.id,
                slug = %ws.slug,
                "workspaces/create: provisioned + seeded"
            );
            HttpResponse::Created().json(CreateWorkspaceResponse {
                workspace_uuid: ws.uuid,
                slug: ws.slug,
                created_at: ws.created_at,
            })
        }
        Err(CreateWorkspaceError::SlugTaken) => {
            // Ensure-exists: a *live* workspace with this slug means a prior
            // provision already created it (or a concurrent create won the
            // race). Return it idempotently as 200 rather than 409, so the
            // control plane calls create unconditionally and a re-provision
            // after a product-side loss (e.g. a reset dev DB) self-heals.
            // `find_by_slug` filters archived rows, so `None` here means the
            // slug is reserved by an archived or hard-deleted workspace — that
            // stays a genuine 409 (the W4 slug never-reuse policy), with the
            // same non-enumerable wording.
            match workspaces::find_by_slug(&mut conn, &slug) {
                Ok(Some(ws)) => {
                    info!(
                        workspace_uuid = %ws.uuid,
                        slug = %ws.slug,
                        "workspaces/create: ensure-exists hit, returning existing workspace"
                    );
                    HttpResponse::Ok().json(CreateWorkspaceResponse {
                        workspace_uuid: ws.uuid,
                        slug: ws.slug,
                        created_at: ws.created_at,
                    })
                }
                Ok(None) => {
                    warn!(slug = %slug, "workspaces/create: slug reserved (archived or retired)");
                    HttpResponse::Conflict().json(json!({
                        "error": "slug_taken",
                        "message": format!("slug '{slug}' is unavailable, please choose another"),
                    }))
                }
                Err(e) => {
                    error!(error = ?e, slug = %slug, "workspaces/create: find_by_slug after SlugTaken failed");
                    errors::internal("Failed to create workspace")
                }
            }
        }
        Err(CreateWorkspaceError::Db(e)) => {
            error!(error = ?e, "workspaces/create: db insert failed");
            errors::internal("Failed to create workspace")
        }
    }
}

/// Response for the deprovision / restore lifecycle endpoints.
#[derive(Debug, Serialize)]
struct WorkspaceLifecycleResponse {
    workspace_uuid: Uuid,
    slug: String,
    /// True once the workspace is soft-archived (deprovisioned). The
    /// scheduler hard-deletes after the grace window; `restore` clears it.
    archived: bool,
    archived_at: Option<chrono::DateTime<chrono::Utc>>,
}

fn lifecycle_response(ws: &Workspace) -> HttpResponse {
    HttpResponse::Ok().json(WorkspaceLifecycleResponse {
        workspace_uuid: ws.uuid,
        slug: ws.slug.clone(),
        archived: ws.archived_at.is_some(),
        archived_at: ws.archived_at,
    })
}

/// `DELETE /api/internal/v1/workspaces/{slug}` — soft-deprovision a
/// workspace (sets `archived_at`). It drops out of routing immediately
/// but rows persist, reversible via `restore` until the grace window
/// elapses and the scheduler hard-deletes. Naturally idempotent: an
/// already-archived workspace is a 200 no-op (the timestamp is NOT
/// reset, so a repeat call can't push back the purge). 404 only if no
/// workspace ever had this slug.
pub async fn deprovision_workspace(
    _: PlatformAuth,
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let slug = path.into_inner();
    let mut conn = match pool_conn(&pool, "workspaces/deprovision") {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let ws = match workspaces::find_by_slug_any_state(&mut conn, &slug) {
        Ok(Some(ws)) => ws,
        Ok(None) => {
            warn!(slug = %slug, "workspaces/deprovision: slug not found");
            return errors::not_found_msg(format!("workspace '{slug}' not found"));
        }
        Err(e) => {
            error!(error = ?e, slug = %slug, "workspaces/deprovision: lookup failed");
            return errors::internal("Workspace lookup failed");
        }
    };

    // Already deprovisioned: no-op, so a repeat call doesn't reset the
    // archive clock and delay the scheduler's hard delete.
    if ws.archived_at.is_some() {
        info!(slug = %slug, "workspaces/deprovision: already archived, no-op");
        return lifecycle_response(&ws);
    }

    // UPDATE workspaces runs under the BYPASSRLS role (nosdesk_app has
    // SELECT only on this parent table), same as create.
    let actor = crate::sync::actor::ActorContext::system("workspace:deprovision");
    let result = crate::sync::session::with_actor_bypass_context::<
        Option<Workspace>,
        diesel::result::Error,
    >(&mut conn, &actor, |c| {
        workspaces::archive_workspace(c, ws.id)
    });
    match result {
        Ok(Some(archived)) => {
            info!(slug = %slug, workspace_id = archived.id, "workspaces/deprovision: archived");
            lifecycle_response(&archived)
        }
        Ok(None) => {
            warn!(slug = %slug, "workspaces/deprovision: row vanished mid-archive");
            errors::not_found_msg(format!("workspace '{slug}' not found"))
        }
        Err(e) => {
            error!(error = ?e, slug = %slug, "workspaces/deprovision: archive failed");
            errors::internal("Failed to deprovision workspace")
        }
    }
}

/// `POST /api/internal/v1/workspaces/{slug}/restore` — clear
/// `archived_at` so a deprovisioned workspace routes again (reactivation
/// after a cancel reversal). Idempotent: restoring an active workspace is
/// a 200 no-op. 404 if the slug never existed; a hard-deleted workspace
/// can't be restored (its row is gone and the slug is retired), which
/// also surfaces as 404.
pub async fn restore_workspace(
    _: PlatformAuth,
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let slug = path.into_inner();
    let mut conn = match pool_conn(&pool, "workspaces/restore") {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let ws = match workspaces::find_by_slug_any_state(&mut conn, &slug) {
        Ok(Some(ws)) => ws,
        Ok(None) => {
            warn!(slug = %slug, "workspaces/restore: slug not found");
            return errors::not_found_msg(format!("workspace '{slug}' not found"));
        }
        Err(e) => {
            error!(error = ?e, slug = %slug, "workspaces/restore: lookup failed");
            return errors::internal("Workspace lookup failed");
        }
    };

    if ws.archived_at.is_none() {
        info!(slug = %slug, "workspaces/restore: already active, no-op");
        return lifecycle_response(&ws);
    }

    let actor = crate::sync::actor::ActorContext::system("workspace:restore");
    let result = crate::sync::session::with_actor_bypass_context::<
        Option<Workspace>,
        diesel::result::Error,
    >(&mut conn, &actor, |c| {
        workspaces::restore_workspace(c, ws.id)
    });
    match result {
        Ok(Some(restored)) => {
            info!(slug = %slug, workspace_id = restored.id, "workspaces/restore: restored");
            lifecycle_response(&restored)
        }
        Ok(None) => {
            warn!(slug = %slug, "workspaces/restore: row vanished mid-restore");
            errors::not_found_msg(format!("workspace '{slug}' not found"))
        }
        Err(e) => {
            error!(error = ?e, slug = %slug, "workspaces/restore: restore failed");
            errors::internal("Failed to restore workspace")
        }
    }
}

/// Request body for `POST /api/internal/v1/workspaces/{slug}/seat_limit`.
/// `seat_limit: null` clears the cap (unlimited).
#[derive(Debug, Deserialize)]
pub struct SetSeatLimitRequest {
    pub seat_limit: Option<i32>,
}

/// `POST /api/internal/v1/workspaces/{slug}/seat_limit` — set or clear the
/// workspace's staff seat cap. Idempotent (setting the same value is a no-op),
/// so unlike `create` it doesn't require an Idempotency-Key. The control plane
/// calls this with `seat_limit: null` to lift the trial cap on subscription
/// activation. Returns 200 on success, 404 if the slug is unknown.
pub async fn set_seat_limit(
    _: PlatformAuth,
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<SetSeatLimitRequest>,
) -> impl Responder {
    let slug = path.into_inner();
    let seat_limit = body.into_inner().seat_limit;

    let mut conn = match pool_conn(&pool, "workspaces/seat_limit") {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    // `workspaces` is BYPASSRLS-only (`nosdesk_app` has SELECT only), so the
    // UPDATE runs under `nosdesk_admin` like the create insert.
    let actor = crate::sync::actor::ActorContext::system("workspace:seat_limit");
    let result = crate::sync::session::with_actor_bypass_context::<usize, diesel::result::Error>(
        &mut conn,
        &actor,
        |c| workspaces::set_seat_limit(c, &slug, seat_limit),
    );

    match result {
        Ok(0) => {
            warn!(slug = %slug, "workspaces/seat_limit: unknown workspace");
            HttpResponse::NotFound().json(json!({
                "error": "workspace_not_found",
                "message": format!("workspace '{slug}' not found"),
            }))
        }
        Ok(_) => {
            info!(slug = %slug, seat_limit = ?seat_limit, "workspaces/seat_limit: updated");
            HttpResponse::Ok().json(json!({ "slug": slug, "seat_limit": seat_limit }))
        }
        Err(e) => {
            error!(error = ?e, slug = %slug, "workspaces/seat_limit: update failed");
            errors::internal("Failed to update seat limit")
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
    /// One of `owner`, `admin`, `agent`, `member`. First-write-wins on
    /// the `workspace_members` row — re-projecting an existing
    /// membership does NOT silently escalate or downgrade the role
    /// (handoff doc Task 4 gotcha). To change an existing member's role
    /// the control plane calls `set_member_role` instead.
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
    matches!(role, "owner" | "admin" | "agent" | "member")
}

pub async fn upsert_projected_user(
    req: HttpRequest,
    _: PlatformAuth,
    pool: web::Data<Pool>,
    // Best-effort search reindex: optional so projection doesn't hard-depend
    // on the search subsystem (and test apps need not wire it).
    search_service: Option<web::Data<Arc<SearchService>>>,
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
        return errors::bad_request("role must be one of: owner, admin, agent, member");
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
        // OIDC seat identity is a global platform identity, not workspace-scoped.
        identity_workspace_id: None,
        email,
        // The control plane provisions verified seat emails, so the
        // email-fallback link is authorised here. The (iss, sub) is
        // usually known too, in which case the identity match wins first.
        email_verified: true,
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

    // Wrap the projection in the target workspace's actor context.
    // `find_or_create_projected_user` inserts into the audited
    // `users` table on the create branch; without the GUC pin the
    // audit trigger fires NDX01 and provisioning 500s. The actor is
    // a bootstrap-style system actor scoped to the target workspace
    // so the audit row records "system projection into <workspace>"
    // rather than attributing the write to a request user.
    //
    // The closure error type collapses the String error from
    // find_or_create_projected_user into a diesel-shaped error so
    // with_actor_context's generic E bound is satisfied; we unwrap
    // it back into a String on the match below.
    let actor = crate::sync::actor::ActorContext::bootstrap("upsert_projected_user")
        .with_workspace(workspace.id);
    let projection = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &actor,
        move |c| {
            find_or_create_projected_user(c, input).map_err(|e| {
                diesel::result::Error::QueryBuilderError(format!("projection: {e}").into())
            })
        },
    )
    .map_err(|e| e.to_string());

    match projection {
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
            // Eager projection grants the user membership in this
            // workspace. find_or_create_projected_user mints users with no
            // search observer, so this reindex is what writes the user
            // into the index with the correct multi-valued workspace tags
            // (and refreshes them when an existing user is re-projected).
            // Best-effort: seed the workspace's Getting Started docs now
            // that a real user exists to author the welcome page (its
            // author columns are NOT NULL with an FK to `users`, so the
            // create-time functional seed couldn't write it). Idempotent —
            // a no-op once the collection has a page, so re-projection and
            // subsequent member projections don't duplicate it. Runs in its
            // own actor-context txn so a docs hiccup never fails the
            // projection the control plane is retrying.
            let docs_actor = crate::sync::actor::ActorContext::system("workspace:seed-docs")
                .with_workspace(workspace.id);
            if let Err(e) = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
                &mut conn,
                &docs_actor,
                |c| crate::services::seed::seed_getting_started(c, user.uuid),
            ) {
                warn!(error = %e, workspace_id = workspace.id, "seed-docs: failed (non-fatal)");
            }

            if let Some(search_service) = &search_service {
                indexing_tasks::spawn_reindex_user(search_service.get_ref().clone(), user.uuid);
            }
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
// set_member_role
// =====================================================================
//
// `POST /api/internal/v1/workspaces/{slug}/members/set_role` — promote
// or demote an existing projected member's role (e.g. a self-registered
// `member` becomes a billed `agent`). `upsert_projected_user` is
// deliberately first-write-wins and never mutates an existing role; this
// is the sanctioned path to CHANGE one. Idempotent (setting the same
// role is a no-op at the DB), so no Idempotency-Key.

#[derive(Debug, Deserialize)]
pub struct SetMemberRoleRequest {
    /// OIDC `iss` — `user_auth_identities.provider_type`.
    pub iss: String,
    /// OIDC `sub` — `user_auth_identities.external_id`.
    pub sub: String,
    /// Target role. `owner` is rejected: ownership transfer is a
    /// separate, higher-stakes operation, and the last-owner guard
    /// lives in `update_membership_role`.
    pub role: String,
}

#[derive(Debug, Serialize)]
struct SetMemberRoleResponse {
    user_uuid: Uuid,
    workspace_id: i32,
    role: String,
}

/// Roles the control plane may set on an existing membership. Excludes
/// `owner` (see `SetMemberRoleRequest::role`).
fn valid_settable_role(role: &str) -> bool {
    matches!(role, "admin" | "agent" | "member")
}

/// Result of the membership upsert in [`set_member_role`]: the role was
/// applied (updated an existing row or created a missing one), or the
/// change was refused because it would demote the workspace's only owner.
enum SetMemberRoleOutcome {
    Applied,
    LastOwner,
}

pub async fn set_member_role(
    _: PlatformAuth,
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<SetMemberRoleRequest>,
) -> impl Responder {
    let slug = path.into_inner();
    let SetMemberRoleRequest { iss, sub, role } = body.into_inner();

    if iss.trim().is_empty() || sub.trim().is_empty() {
        return errors::bad_request("iss and sub must both be non-empty");
    }
    if !valid_settable_role(&role) {
        return errors::bad_request("role must be one of: admin, agent, member");
    }

    let mut conn = match pool_conn(&pool, "set_member_role") {
        Ok(c) => c,
        Err(resp) => return resp,
    };

    let workspace = match resolve_workspace_or_respond(&mut conn, &slug, "set_member_role") {
        Ok(ws) => ws,
        Err(resp) => return resp,
    };

    // Resolve the member from (iss, sub). A miss is a 404, same as an
    // unknown membership below — the control plane treats both as
    // "no such member to promote".
    let user_uuid =
        match crate::repository::user_auth_identities::find_user_by_identity(&iss, &sub, &mut conn)
        {
            Ok(Some(u)) => u,
            Ok(None) => {
                warn!(slug = %slug, "set_member_role: no user for (iss, sub)");
                return errors::not_found_msg("member not found");
            }
            Err(e) => {
                error!(error = ?e, slug = %slug, "set_member_role: identity lookup failed");
                return errors::internal("Failed to resolve member");
            }
        };

    // Upsert the membership. `workspace_members` is RLS-enabled with a
    // `WITH CHECK` isolation policy, and its audit trigger reads
    // `app.workspace_id`; the bypass context both satisfies the policy for
    // this control-plane write and sets the actor GUCs. The seat-limit
    // trigger surfaces as a DB error, mapped to 403 below.
    //
    // CREATE-IF-ABSENT: if the user has no membership yet (e.g. the eager
    // projection's grant didn't materialise, or the control plane promotes
    // before projecting), UPDATE alone is a silent no-op. Fall through to
    // an insert with the requested role so the promotion actually takes.
    let actor = crate::sync::actor::ActorContext::system("workspace:set_member_role")
        .with_workspace(workspace.id);
    let outcome = crate::sync::session::with_actor_bypass_context::<
        SetMemberRoleOutcome,
        diesel::result::Error,
    >(
        &mut conn,
        &actor,
        |c| match workspaces::update_membership_role(c, workspace.id, user_uuid, &role)? {
            UpdateMembershipRoleResult::Updated(_) => Ok(SetMemberRoleOutcome::Applied),
            UpdateMembershipRoleResult::LastOwner => Ok(SetMemberRoleOutcome::LastOwner),
            UpdateMembershipRoleResult::NotFound => {
                // No row to update: create it, self-verifying via RETURNING
                // so a write that produces no row errors here instead of
                // logging "applied" over a phantom membership.
                workspaces::upsert_membership_role(c, workspace.id, user_uuid, &role)?;
                Ok(SetMemberRoleOutcome::Applied)
            }
        },
    );

    match outcome {
        Ok(SetMemberRoleOutcome::Applied) => {
            info!(workspace_id = workspace.id, %user_uuid, role = %role, "set_member_role: applied");
            HttpResponse::Ok().json(SetMemberRoleResponse {
                user_uuid,
                workspace_id: workspace.id,
                role,
            })
        }
        Ok(SetMemberRoleOutcome::LastOwner) => HttpResponse::Conflict().json(json!({
            "error": "last_owner",
            "message": "cannot demote the only owner; promote another member first",
        })),
        Err(e) if workspaces::is_seat_limit_violation(&e) => {
            warn!(workspace_id = workspace.id, %user_uuid, "set_member_role: blocked by workspace seat limit");
            HttpResponse::Forbidden().json(json!({
                "error": "seat_limit_reached",
                "message": "This workspace has reached its seat limit. Contact support to add more seats.",
            }))
        }
        Err(e) => {
            error!(error = ?e, workspace_id = workspace.id, %user_uuid, "set_member_role: update failed");
            errors::internal("Failed to update member role")
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

/// Set or clear a workspace's verified custom domain.
///
/// Passkey implication (C1 / tenant-origin): WebAuthn RP ID is the workspace's
/// canonical host, so changing the custom domain changes the RP ID and
/// **invalidates every existing passkey** for the workspace (the spec binds
/// credentials to the RP ID; there is no rebind). The admin-facing flow must
/// warn before changing it, and we must not delete the old credentials until
/// users re-enrol on the new host. Slug is immutable, so this is the only
/// passkey-invalidating event. See docs/plans/tenant-origin-awareness.md.
pub async fn set_custom_domain(
    req: HttpRequest,
    _: PlatformAuth,
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

// =====================================================================
// GET /api/internal/v1/workspaces/{slug}/provisioning
// =====================================================================
//
// Provisioning-readiness check for the control plane. After the
// create + upsert_projected_user sequence the control plane polls this
// to confirm the tenant is actually usable — seeded (workflow states /
// SLA / categories) AND owned — rather than assuming the multi-call
// sequence completed. Also feeds the stuck-provisioning reconciliation
// sweeper. Named for the domain (provisioning), not "readiness", to keep
// it distinct from the instance-level `/readiness` orchestrator probe;
// the `ready` field carries the verdict. Read-only, so no Idempotency-Key.

#[derive(Debug, Serialize)]
struct ProvisioningChecks {
    /// Default workflow states exist — without them a workspace can't
    /// create or triage a ticket (the P0.1 usability blocker).
    workflow_states: bool,
    /// A default SLA policy exists.
    default_sla_policy: bool,
    /// Default ticket categories exist.
    ticket_categories: bool,
    /// At least one `owner` membership (eager-projected, or granted on
    /// first login).
    owner: bool,
}

#[derive(Debug, Serialize)]
struct ProvisioningStatus {
    workspace_uuid: Uuid,
    slug: String,
    /// True only when every check passes — the tenant is fully usable.
    ready: bool,
    checks: ProvisioningChecks,
}

pub async fn workspace_provisioning(
    _: PlatformAuth,
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let slug = path.into_inner();
    let mut conn = match pool_conn(&pool, "workspace_provisioning") {
        Ok(c) => c,
        Err(resp) => return resp,
    };
    let workspace = match resolve_workspace_or_respond(&mut conn, &slug, "workspace_provisioning") {
        Ok(ws) => ws,
        Err(resp) => return resp,
    };

    // Owner membership: workspace_members is a meta-table (no RLS), so a
    // direct workspace-id-filtered count is correct without pinning context.
    let owners = workspaces::count_workspace_owners(&mut conn, workspace.id).unwrap_or_else(|e| {
        warn!(error = ?e, slug = %slug, "workspace_provisioning: owner count failed");
        0
    });

    // Seeded functional defaults live on FORCE-RLS tenant tables, so count
    // them with the workspace pinned in the actor context — the same scope
    // the create-time seed wrote them under.
    let actor = crate::sync::actor::ActorContext::system("workspace:provisioning")
        .with_workspace(workspace.id);
    let counts = crate::sync::session::with_actor_context::<_, diesel::result::Error>(
        &mut conn,
        &actor,
        |conn| {
            use crate::schema::{sla_policies, ticket_categories, workflow_states};
            use diesel::dsl::count_star;
            use diesel::prelude::*;
            let wf: i64 = workflow_states::table.select(count_star()).first(conn)?;
            let sla: i64 = sla_policies::table
                .filter(sla_policies::is_default.eq(true))
                .select(count_star())
                .first(conn)?;
            let cat: i64 = ticket_categories::table.select(count_star()).first(conn)?;
            Ok((wf, sla, cat))
        },
    );
    let (wf, sla, cat) = match counts {
        Ok(c) => c,
        Err(e) => {
            error!(error = ?e, slug = %slug, "workspace_provisioning: seeded-defaults count failed");
            return errors::internal("Workspace provisioning check failed");
        }
    };

    let checks = ProvisioningChecks {
        workflow_states: wf > 0,
        default_sla_policy: sla > 0,
        ticket_categories: cat > 0,
        owner: owners > 0,
    };
    let ready = checks.workflow_states
        && checks.default_sla_policy
        && checks.ticket_categories
        && checks.owner;

    HttpResponse::Ok().json(ProvisioningStatus {
        workspace_uuid: workspace.uuid,
        slug: workspace.slug,
        ready,
        checks,
    })
}
