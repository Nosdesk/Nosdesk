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

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info};
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{NewSavedView, SavedView, SavedViewUpdate};
use crate::repository::saved_views as repo;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route(
        "/saved-views",
        web::get().to(crate::handlers::saved_views::list),
    )
    .route(
        "/saved-views",
        web::post().to(crate::handlers::saved_views::create),
    )
    .route(
        "/saved-views/{uuid}",
        web::get().to(crate::handlers::saved_views::get_one),
    )
    .route(
        "/saved-views/{uuid}",
        web::patch().to(crate::handlers::saved_views::patch),
    )
    .route(
        "/saved-views/{uuid}",
        web::delete().to(crate::handlers::saved_views::delete),
    );
}

const NAME_MIN: usize = 1;
const NAME_MAX: usize = 120;

/// Viz-type allowlist. Mirrors the DB-level CHECK constraint added
/// in 2026-06-08-000000_saved_views_viz_columns so the handler
/// short-circuits with a 400 before the DB raises a constraint
/// violation. Update both lists in lockstep.
const VIZ_TYPES: &[&str] = &[
    "list",
    "kpi_tile",
    "line",
    "horizontal_bar",
    "heatmap",
    "leaderboard",
    "table",
];

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
    /// When `true`, the response is the workspace's pickable
    /// chart-backed saved views (viz_type != 'list'). Backs the
    /// AddWidgetModal "Your saved views" tab. Mutually exclusive
    /// with `project_id` / `dataset`; when set, those are ignored.
    #[serde(default)]
    pub has_viz: Option<bool>,
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
    /// Renderer for the dashboard SavedViewWidget. Optional on the
    /// wire (the ticket-list UI doesn't set it); defaults to 'list'.
    #[serde(default)]
    pub viz_type: Option<String>,
    /// Per-renderer config blob. Opaque to the create path; the
    /// frontend chart-config form validates client-side.
    #[serde(default)]
    pub viz_config: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct PatchBody {
    pub name: Option<String>,
    pub shape: Option<Value>,
    pub filter: Option<Value>,
    pub viz_type: Option<String>,
    pub viz_config: Option<Value>,
}

pub async fn list(
    mut tc: TenantConn,
    query: web::Query<ListQuery>,
    auth: AuthContext,
) -> impl Responder {
    // Pickable path: chart-backed saved views the caller can see,
    // for the AddWidgetModal "Your saved views" tab. The repo
    // enforces the same scope rules as the ticket-list path
    // (workspace visible to all, private visible only to the
    // creator), so this short-circuits the scope-merging branches
    // below without dropping any visibility guarantees.
    if query.has_viz.unwrap_or(false) {
        let user_uuid = auth.user_uuid;
        return match tc.run(|conn| repo::list_pickable(conn, user_uuid)) {
            Ok(rows) => HttpResponse::Ok().json(rows),
            Err(e) => {
                error!(error = %e, "failed to load pickable saved views");
                errors::internal("Failed to load saved views")
            }
        };
    }

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
        let dataset_owned = dataset.to_string();
        return match tc.run(|conn| {
            repo::list_for_scope_dataset(conn, "private", Some(&user_id), &dataset_owned)
        }) {
            Ok(rows) => HttpResponse::Ok().json(rows),
            Err(e) => {
                error!(error = %e, dataset, "failed to load dataset saved views");
                errors::internal("Failed to load saved views")
            }
        };
    }

    let project_id = query.project_id;
    let user_id = auth.user_uuid.to_string();
    let result = tc.run(|conn| {
        let mut out: Vec<SavedView> = Vec::new();
        // Workspace-scoped views: every authenticated user can see.
        out.extend(repo::list_for_scope(conn, "workspace", None)?);
        // Project-scoped views: only when a project context is supplied.
        if let Some(project_id) = project_id {
            let scope_id = project_id.to_string();
            out.extend(repo::list_for_scope(conn, "project", Some(&scope_id))?);
        }
        // Private views: only the caller's own.
        out.extend(repo::list_for_scope(conn, "private", Some(&user_id))?);
        Ok(out)
    });
    match result {
        Ok(out) => HttpResponse::Ok().json(out),
        Err(e) => {
            error!(error = %e, "failed to load saved views");
            errors::internal("Failed to load saved views")
        }
    }
}

pub async fn get_one(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    match tc.run(|conn| repo::find_by_uuid(conn, uuid)) {
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
    mut tc: TenantConn,
    body: web::Json<CreateBody>,
    auth: AuthContext,
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
        return errors::bad_request("Non-ticket saved views must use the 'private' scope");
    }
    if let Err(msg) = validate_scope_pair(&body.scope, &body.scope_id, &auth) {
        return errors::bad_request(msg);
    }
    if !user_can_write_scope(&body.scope, &auth) {
        return errors::forbidden(write_denied_message(&body.scope));
    }

    let viz_type = body.viz_type.as_deref().unwrap_or("list");
    if let Err(msg) = validate_viz_type(viz_type) {
        return errors::bad_request(msg);
    }

    let new = NewSavedView {
        scope: body.scope,
        scope_id: body.scope_id,
        name: body.name.trim().to_string(),
        shape: body.shape,
        filter: body.filter,
        created_by: auth.user_uuid,
        dataset: dataset.to_string(),
        viz_type: viz_type.to_string(),
        viz_config: body
            .viz_config
            .unwrap_or_else(|| Value::Object(Default::default())),
    };

    match tc.run(|conn| repo::create(conn, new)) {
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

/// Result variants for the patch flow so permission/notfound checks
/// happen inside the transaction without doubling up calls.
enum PatchOutcome {
    Ok(SavedView),
    NotFound,
    Forbidden(&'static str),
}

pub async fn patch(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    body: web::Json<PatchBody>,
    auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    let body = body.into_inner();
    if let Some(ref n) = body.name {
        if let Err(msg) = validate_name(n) {
            return errors::bad_request(msg);
        }
    }
    if let Some(ref v) = body.viz_type {
        if let Err(msg) = validate_viz_type(v) {
            return errors::bad_request(msg);
        }
    }

    let patch = SavedViewUpdate {
        name: body.name.map(|s| s.trim().to_string()),
        shape: body.shape,
        filter: body.filter,
        viz_type: body.viz_type,
        viz_config: body.viz_config,
    };

    let result = tc.run(|conn| {
        let view = match repo::find_by_uuid(conn, uuid)? {
            Some(v) => v,
            None => return Ok(PatchOutcome::NotFound),
        };
        if !user_can_write_view(&view, &auth) {
            return Ok(PatchOutcome::Forbidden(write_denied_message(&view.scope)));
        }
        let updated = repo::update(conn, uuid, patch)?;
        Ok(PatchOutcome::Ok(updated))
    });

    match result {
        Ok(PatchOutcome::Ok(updated)) => HttpResponse::Ok().json(updated),
        Ok(PatchOutcome::NotFound) => errors::not_found_msg("Saved view not found"),
        Ok(PatchOutcome::Forbidden(msg)) => errors::forbidden(msg),
        Err(e) => {
            error!(error = %e, %uuid, "failed to update saved view");
            errors::internal("Failed to update saved view")
        }
    }
}

/// Result variants for the delete flow, mirroring `PatchOutcome`.
enum DeleteOutcome {
    Ok,
    NotFound,
    Forbidden(&'static str),
}

pub async fn delete(
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    auth: AuthContext,
) -> impl Responder {
    let uuid = path.into_inner();
    let result = tc.run(|conn| {
        let view = match repo::find_by_uuid(conn, uuid)? {
            Some(v) => v,
            None => return Ok(DeleteOutcome::NotFound),
        };
        if !user_can_write_view(&view, &auth) {
            return Ok(DeleteOutcome::Forbidden(write_denied_message(&view.scope)));
        }
        repo::delete(conn, uuid)?;
        Ok(DeleteOutcome::Ok)
    });
    match result {
        Ok(DeleteOutcome::Ok) => HttpResponse::NoContent().finish(),
        Ok(DeleteOutcome::NotFound) => errors::not_found_msg("Saved view not found"),
        Ok(DeleteOutcome::Forbidden(msg)) => errors::forbidden(msg),
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

fn validate_viz_type(viz_type: &str) -> Result<(), &'static str> {
    if VIZ_TYPES.contains(&viz_type) {
        Ok(())
    } else {
        Err("viz_type must be one of: list, kpi_tile, line, horizontal_bar, heatmap, leaderboard, table")
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

fn user_can_write_scope(scope: &str, auth: &AuthContext) -> bool {
    match scope {
        "workspace" => auth.is_workspace_admin(),
        "project" => auth.can_handle_tickets(),
        "private" => true,
        _ => false,
    }
}

fn user_can_write_view(view: &SavedView, auth: &AuthContext) -> bool {
    if view.scope == "private" {
        return view.scope_id.as_deref() == Some(auth.user_uuid.to_string().as_str());
    }
    user_can_write_scope(&view.scope, auth)
}

fn write_denied_message(scope: &str) -> &'static str {
    match scope {
        "workspace" => "Only admins can edit workspace saved views",
        "project" => "Only technicians and admins can edit project saved views",
        "private" => "You can only edit your own private saved views",
        _ => "You don't have permission to edit this saved view",
    }
}
