//! Saved-view endpoints.
//!
//! - `GET    /api/saved-views?project_id=<n>` — list every view
//!   the caller can see for the given context (workspace +
//!   project if `project_id` is set + the caller's private views).
//! - `GET    /api/saved-views/{uuid}` — fetch one.
//! - `POST   /api/saved-views` — create.
//! - `PATCH  /api/saved-views/{uuid}` — rename / re-shape /
//!   re-filter / promote-to-default.
//! - `DELETE /api/saved-views/{uuid}` — soft archive.
//!
//! Permission model:
//! - `workspace` scope writes require admin; reads open to any
//!   authenticated user.
//! - `project` scope writes require technician/admin (project-
//!   member ACLs land alongside the per-project permission
//!   rework in a later phase).
//! - `private` scope writes are owner-only; reads same.

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
    /// When provided, the response includes views with
    /// `scope = 'project' AND scope_id = project_id`. Without it,
    /// only `workspace` and `private` views are returned.
    pub project_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub scope: String,
    pub scope_id: Option<String>,
    pub name: String,
    pub shape: Value,
    pub filter: Value,
    #[serde(default)]
    pub is_default: bool,
}

#[derive(Debug, Deserialize)]
pub struct PatchBody {
    pub name: Option<String>,
    pub shape: Option<Value>,
    pub filter: Option<Value>,
    /// `Some(true)` promotes this view to default for its scope
    /// (demoting the previous default in the same transaction).
    /// `Some(false)` is rejected: per-scope defaults are positive
    /// statements only — to "unset" the default, promote a
    /// different view instead.
    pub is_default: Option<bool>,
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
        is_default: body.is_default,
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
    if matches!(body.is_default, Some(false)) {
        return errors::bad_request(
            "Setting is_default to false directly is not allowed; promote a different view instead",
        );
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
        is_default: body.is_default,
        archived_at: None,
    };
    match repo::update(&mut conn, uuid, patch) {
        Ok(updated) => HttpResponse::Ok().json(updated),
        Err(e) => {
            error!(error = %e, %uuid, "failed to update saved view");
            errors::internal("Failed to update saved view")
        }
    }
}

pub async fn archive(
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
            error!(error = %e, %uuid, "failed to look up saved view for archive");
            return errors::internal("Failed to archive saved view");
        }
    };
    let claims = req.extensions().get::<Claims>().cloned();
    if !user_can_write_view(&view, &claims, &auth) {
        return errors::forbidden(write_denied_message(&view.scope));
    }
    match repo::archive(&mut conn, uuid) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(error = %e, %uuid, "failed to archive saved view");
            errors::internal("Failed to archive saved view")
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
