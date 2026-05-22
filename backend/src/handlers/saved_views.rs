//! Saved-view endpoints.
//!
//! - `GET    /api/saved-views?project_id=<n>` — list every ticket
//!   view the caller can see (workspace + project if `project_id`
//!   is set + the caller's private views). Tickets-specific.
//! - `GET    /api/saved-views?dataset=<d>` — list the caller's
//!   private views for one dataset ('assets' | 'users'). Non-
//!   ticket datasets are private-only by design.
//! - `GET    /api/saved-views/{uuid}` — fetch one.
//! - `POST   /api/saved-views` — create.
//! - `PATCH  /api/saved-views/{uuid}` — rename / re-shape / re-filter.
//! - `DELETE /api/saved-views/{uuid}` — hard delete.
//!
//! Permission model (tickets):
//! - `workspace` scope writes require admin; reads open to any
//!   authenticated user.
//! - `project` scope writes require technician/admin (project-
//!   member ACLs land alongside the per-project permission
//!   rework in a later phase).
//! - `private` scope writes are owner-only; reads same.
//!
//! Non-ticket datasets are restricted to `private` scope: the
//! handler refuses any other scope on create so the per-dataset
//! visibility story stays simple ("my saved views, only mine").

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::extractors::AuthContext;
use crate::handlers::{errors, helpers};
use crate::models::{Claims, NewSavedView, SavedView, SavedViewUpdate};
use crate::repository::saved_views as repo;
use crate::utils::rbac::is_admin;

const NAME_MIN: usize = 1;
const NAME_MAX: usize = 120;

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    /// Tickets surface only. When set, includes views with
    /// `scope = 'project' AND scope_id = project_id` alongside
    /// the workspace + private views. Without it, the response
    /// is workspace + private only.
    pub project_id: Option<i32>,
    /// When set to a non-ticket dataset ('assets' | 'users'),
    /// the response is the caller's private views for that
    /// dataset and nothing else. The ticket scope merging
    /// (workspace + project + private) is skipped because those
    /// scopes only apply to ticket views.
    pub dataset: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub scope: String,
    pub scope_id: Option<String>,
    pub name: String,
    pub shape: Value,
    pub filter: Value,
    /// Defaults to 'tickets' when absent for backwards compat
    /// with the existing ticket-view UI.
    #[serde(default)]
    pub dataset: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PatchBody {
    pub name: Option<String>,
    pub shape: Option<Value>,
    pub filter: Option<Value>,
}

pub async fn list(
    pool: web::Data<Pool>,
    query: web::Query<ListQuery>,
    auth: AuthContext,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Non-ticket dataset path: private-only, single dataset. The
    // ticket scope-merging branches below don't apply because
    // workspace / project scopes are ticket-specific.
    if let Some(dataset) = query
        .dataset
        .as_deref()
        .filter(|d| !d.is_empty() && *d != "tickets")
    {
        if let Err(msg) = validate_dataset(dataset) {
            return errors::bad_request(msg);
        }
        let user_id = auth.user_uuid.to_string();
        return match repo::list_for_scope_dataset(
            &mut conn,
            "private",
            Some(&user_id),
            dataset,
        ) {
            Ok(rows) => HttpResponse::Ok().json(rows),
            Err(e) => {
                error!(error = %e, dataset, "failed to load dataset saved views");
                errors::internal("Failed to load saved views")
            }
        };
    }

    let mut out: Vec<SavedView> = Vec::new();

    // Workspace-scoped views: every authenticated user can see.
    match repo::list_for_scope(&mut conn, "workspace", None) {
        Ok(rows) => out.extend(rows),
        Err(e) => {
            error!(error = %e, "failed to load workspace saved views");
            return errors::internal("Failed to load saved views");
        }
    }

    // Project-scoped views: only when a project context is supplied.
    if let Some(project_id) = query.project_id {
        let scope_id = project_id.to_string();
        match repo::list_for_scope(&mut conn, "project", Some(&scope_id)) {
            Ok(rows) => out.extend(rows),
            Err(e) => {
                error!(error = %e, project_id, "failed to load project saved views");
                return errors::internal("Failed to load saved views");
            }
        }
    }

    // Private views: only the caller's own.
    let user_id = auth.user_uuid.to_string();
    match repo::list_for_scope(&mut conn, "private", Some(&user_id)) {
        Ok(rows) => out.extend(rows),
        Err(e) => {
            error!(error = %e, "failed to load private saved views");
            return errors::internal("Failed to load saved views");
        }
    }

    HttpResponse::Ok().json(out)
}

pub async fn get_one(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match repo::find_by_uuid(&mut conn, uuid) {
        Ok(Some(view)) => {
            if !user_can_read(&view, &auth) {
                return errors::forbidden("You don't have access to this saved view");
            }
            HttpResponse::Ok().json(view)
        }
        Ok(None) => errors::not_found_msg("Saved view not found"),
        Err(e) => {
            error!(error = %e, %uuid, "failed to fetch saved view");
            errors::internal("Failed to fetch saved view")
        }
    }
}

pub async fn create(
    pool: web::Data<Pool>,
    body: web::Json<CreateBody>,
    auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    let body = body.into_inner();
    if let Err(msg) = validate_name(&body.name) {
        return errors::bad_request(msg);
    }
    let dataset = body
        .dataset
        .as_deref()
        .filter(|d| !d.is_empty())
        .unwrap_or("tickets");
    if let Err(msg) = validate_dataset(dataset) {
        return errors::bad_request(msg);
    }
    // Non-ticket datasets are private-only. Refuse workspace /
    // project scope on those so the access model stays simple
    // ("my saved views, only mine") until product asks for more.
    if dataset != "tickets" && body.scope != "private" {
        return errors::bad_request(
            "Non-ticket saved views must use the 'private' scope",
        );
    }
    if let Err(msg) = validate_scope_pair(&body.scope, &body.scope_id, &auth) {
        return errors::bad_request(msg);
    }
    let claims = req.extensions().get::<Claims>().cloned();
    if !user_can_write_scope(&body.scope, &claims, &auth) {
        return errors::forbidden(write_denied_message(&body.scope));
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let new = NewSavedView {
        scope: body.scope,
        scope_id: body.scope_id,
        name: body.name.trim().to_string(),
        shape: body.shape,
        filter: body.filter,
        created_by: auth.user_uuid,
        dataset: dataset.to_string(),
    };

    match repo::create(&mut conn, new) {
        Ok(view) => {
            info!(uuid = %view.uuid, scope = %view.scope, "saved view created");
            HttpResponse::Created().json(view)
        }
        Err(e) => {
            error!(error = %e, "failed to create saved view");
            errors::internal("Failed to create saved view")
        }
    }
}

pub async fn patch(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: web::Json<PatchBody>,
    auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    let uuid = path.into_inner();
    let body = body.into_inner();
    if let Some(ref n) = body.name {
        if let Err(msg) = validate_name(n) {
            return errors::bad_request(msg);
        }
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let view = match repo::find_by_uuid(&mut conn, uuid) {
        Ok(Some(v)) => v,
        Ok(None) => return errors::not_found_msg("Saved view not found"),
        Err(e) => {
            error!(error = %e, %uuid, "failed to look up saved view for patch");
            return errors::internal("Failed to update saved view");
        }
    };
    let claims = req.extensions().get::<Claims>().cloned();
    if !user_can_write_view(&view, &claims, &auth) {
        return errors::forbidden(write_denied_message(&view.scope));
    }

    let patch = SavedViewUpdate {
        name: body.name.map(|s| s.trim().to_string()),
        shape: body.shape,
        filter: body.filter,
    };
    match repo::update(&mut conn, uuid, patch) {
        Ok(updated) => HttpResponse::Ok().json(updated),
        Err(e) => {
            error!(error = %e, %uuid, "failed to update saved view");
            errors::internal("Failed to update saved view")
        }
    }
}

pub async fn delete(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    auth: AuthContext,
    req: HttpRequest,
) -> impl Responder {
    let uuid = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let view = match repo::find_by_uuid(&mut conn, uuid) {
        Ok(Some(v)) => v,
        Ok(None) => return errors::not_found_msg("Saved view not found"),
        Err(e) => {
            error!(error = %e, %uuid, "failed to look up saved view for delete");
            return errors::internal("Failed to delete saved view");
        }
    };
    let claims = req.extensions().get::<Claims>().cloned();
    if !user_can_write_view(&view, &claims, &auth) {
        return errors::forbidden(write_denied_message(&view.scope));
    }
    match repo::delete(&mut conn, uuid) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, %uuid, "failed to delete saved view");
            errors::internal("Failed to delete saved view")
        }
    }
}

// ----- helpers -----

fn validate_name(name: &str) -> Result<(), &'static str> {
    let trimmed = name.trim();
    if trimmed.len() < NAME_MIN || trimmed.len() > NAME_MAX {
        return Err("Name must be 1 to 120 characters");
    }
    Ok(())
}

/// Allowlist for the `dataset` column. Kept narrow so a typo on
/// the client doesn't quietly land an "asssets" partition that
/// nothing else ever queries.
fn validate_dataset(dataset: &str) -> Result<(), &'static str> {
    match dataset {
        "tickets" | "assets" | "users" => Ok(()),
        _ => Err("dataset must be one of: tickets, assets, users"),
    }
}

fn validate_scope_pair(
    scope: &str,
    scope_id: &Option<String>,
    auth: &AuthContext,
) -> Result<(), &'static str> {
    match scope {
        "workspace" => {
            if scope_id.is_some() {
                return Err("workspace scope must have a null scope_id");
            }
        }
        "project" => {
            let sid = scope_id.as_deref().unwrap_or("");
            if sid.is_empty() {
                return Err("project scope requires scope_id (the project id as text)");
            }
            if sid.parse::<i32>().is_err() {
                return Err("project scope_id must be an integer project id");
            }
        }
        "private" => {
            // For private views, force scope_id to the caller's
            // own UUID so a malicious client can't park their
            // saved views under another user's id.
            let expected = auth.user_uuid.to_string();
            if scope_id.as_deref() != Some(expected.as_str()) {
                return Err("private scope_id must equal your own user uuid");
            }
        }
        _ => return Err("scope must be one of: workspace, project, private"),
    }
    Ok(())
}

fn user_can_read(view: &SavedView, auth: &AuthContext) -> bool {
    match view.scope.as_str() {
        "workspace" | "project" => true,
        "private" => view.scope_id.as_deref() == Some(auth.user_uuid.to_string().as_str()),
        _ => false,
    }
}

fn user_can_write_scope(scope: &str, claims: &Option<Claims>, auth: &AuthContext) -> bool {
    match scope {
        "workspace" => claims.as_ref().map(is_admin).unwrap_or(false),
        "project" => auth.is_technician_or_admin(),
        "private" => true,
        _ => false,
    }
}

fn user_can_write_view(view: &SavedView, claims: &Option<Claims>, auth: &AuthContext) -> bool {
    if view.scope == "private" {
        return view.scope_id.as_deref() == Some(auth.user_uuid.to_string().as_str());
    }
    user_can_write_scope(&view.scope, claims, auth)
}

fn write_denied_message(scope: &str) -> &'static str {
    match scope {
        "workspace" => "Only admins can edit workspace saved views",
        "project" => "Only technicians and admins can edit project saved views",
        "private" => "You can only edit your own private saved views",
        _ => "You don't have permission to edit this saved view",
    }
}

// Suppress unused-import warnings for items only referenced inside
// permission checks.
#[allow(dead_code)]
fn _keepalive(_: &DbConnection) {}
