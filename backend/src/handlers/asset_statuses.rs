//! Named asset statuses over REST, for workspace admins, under
//! `/api/admin/asset-statuses`: list (archived ones too, each with how many
//! assets hold it and whether a delete would go through), get, create,
//! edit, reorder a category, archive, restore, and delete one nothing
//! holds.
//!
//! Everyone who sees assets reads them through `GET /api/assets/statuses`
//! (`handlers::asset_lifecycle::list_statuses`).

use std::collections::HashMap;

use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use crate::db::DbConnection;
use crate::errors;
use crate::extractors::{AuthContext, TenantConn};
use crate::models::AssetNamedStatus;
use crate::repository::asset_statuses::{self as repo, NewStatusInput, StatusChanges, StatusError};

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/asset-statuses", web::get().to(admin_list))
        .route("/admin/asset-statuses", web::post().to(create))
        .route("/admin/asset-statuses/order", web::put().to(reorder))
        .route("/admin/asset-statuses/{id:\\d+}", web::get().to(get))
        .route("/admin/asset-statuses/{id:\\d+}", web::put().to(update))
        .route("/admin/asset-statuses/{id:\\d+}", web::delete().to(delete))
        .route(
            "/admin/asset-statuses/{id:\\d+}/archive",
            web::post().to(archive),
        )
        .route(
            "/admin/asset-statuses/{id:\\d+}/restore",
            web::post().to(restore),
        );
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    /// One of the nine category keys.
    pub category: String,
    pub slug: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub position: i32,
    #[serde(default)]
    pub is_default: bool,
    /// Kind slugs offered the status; absent or `null` for every kind.
    #[serde(default)]
    pub applies_to_kinds: Option<Vec<String>>,
}

/// An edit: a field left out keeps its value, and `null` clears a field
/// that may be empty. The category and slug never change.
#[derive(Debug, Deserialize)]
pub struct UpdateBody {
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub color: Option<Option<String>>,
    #[serde(default)]
    pub position: Option<i32>,
    #[serde(default)]
    pub is_default: Option<bool>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub applies_to_kinds: Option<Option<Vec<String>>>,
}

/// A category's statuses in their new order.
#[derive(Debug, Deserialize)]
pub struct ReorderBody {
    pub category: String,
    pub ids: Vec<i32>,
}

fn unprocessable(code: &str, message: impl Into<String>, extra: Value) -> HttpResponse {
    errors::with_fields(StatusCode::UNPROCESSABLE_ENTITY, code, message, extra)
}

fn conflict(code: &str, message: impl Into<String>, extra: Value) -> HttpResponse {
    errors::with_fields(StatusCode::CONFLICT, code, message, extra)
}

/// The status and body for each of the repository's refusals.
fn status_error(e: StatusError) -> HttpResponse {
    match e {
        StatusError::NotFound => errors::not_found_msg("Asset status not found"),
        StatusError::UnknownCategory(category) => unprocessable(
            "UNKNOWN_CATEGORY",
            format!("Unknown asset status category: {category}"),
            json!({ "category": category }),
        ),
        StatusError::InvalidKey(key) => unprocessable(
            "INVALID_KEY",
            format!("`{key}` must be lower case letters, digits and underscores, starting with a letter"),
            json!({ "key": key }),
        ),
        StatusError::SlugTaken => conflict(
            "SLUG_TAKEN",
            "Another status already uses that slug",
            json!({}),
        ),
        StatusError::SlugReserved => conflict(
            "SLUG_RESERVED",
            "That slug is a status category's key",
            json!({}),
        ),
        StatusError::LabelReserved => conflict(
            "LABEL_RESERVED",
            "That name is a status category's; a plain asset already reads as it",
            json!({}),
        ),
        StatusError::LabelRequired => {
            unprocessable("LABEL_REQUIRED", "A status needs a name", json!({}))
        }
        StatusError::LabelTaken => conflict(
            "LABEL_TAKEN",
            "Another status already has that name",
            json!({}),
        ),
        StatusError::TooLong { field, max } => unprocessable(
            "TOO_LONG",
            format!("{field} is longer than {max} characters"),
            json!({ "field": field, "max": max }),
        ),
        StatusError::InvalidColor(color) => unprocessable(
            "INVALID_COLOR",
            "A color is #rrggbb",
            json!({ "color": color }),
        ),
        StatusError::UnknownKind(kind) => unprocessable(
            "UNKNOWN_KIND",
            format!("Unknown asset kind: {kind}"),
            json!({ "kind": kind }),
        ),
        StatusError::DefaultLimitedToKinds => unprocessable(
            "DEFAULT_LIMITED_TO_KINDS",
            "A category's default names assets of every kind, so it can't be limited to kinds",
            json!({}),
        ),
        StatusError::InUse { count } => conflict(
            "IN_USE",
            format!("{count} asset(s), status change(s) or loan(s) hold this status; archive it instead"),
            json!({ "usage_count": count }),
        ),
        StatusError::OtherCategory { category } => unprocessable(
            "WRONG_CATEGORY",
            format!("That status belongs to {category}"),
            json!({ "category": category }),
        ),
        StatusError::DefaultTaken => conflict(
            "DEFAULT_TAKEN",
            "Another status became the category's default; try again",
            json!({}),
        ),
        StatusError::Db(e) => {
            error!(error = ?e, "asset status request failed");
            errors::internal("Failed to process the asset status")
        }
    }
}

/// How many assets hold each status, and how many rows of any kind (assets,
/// lifecycle events, loans): the count a delete checks.
struct Counts {
    assets: HashMap<i32, i64>,
    usage: HashMap<i32, i64>,
}

impl Counts {
    fn load(conn: &mut DbConnection) -> diesel::QueryResult<Self> {
        Ok(Self {
            assets: repo::asset_counts(conn)?.into_iter().collect(),
            usage: repo::usage_counts(conn)?,
        })
    }

    /// `s` with `asset_count`, `usage_count` and `deletable`, which a
    /// delete answers to: a status shown deletable deletes.
    fn of(&self, s: &AssetNamedStatus) -> Value {
        let usage = self.usage.get(&s.id).copied().unwrap_or(0);
        let mut out = json!(s);
        if let Value::Object(map) = &mut out {
            map.insert(
                "asset_count".into(),
                json!(self.assets.get(&s.id).copied().unwrap_or(0)),
            );
            map.insert("usage_count".into(), json!(usage));
            map.insert("deletable".into(), json!(usage == 0));
        }
        out
    }
}

/// The written status with its counts, read in the same transaction.
fn counted(conn: &mut DbConnection, s: AssetNamedStatus) -> Result<Value, StatusError> {
    Ok(Counts::load(conn)?.of(&s))
}

fn require_admin(auth: &AuthContext) -> Option<HttpResponse> {
    (!auth.is_workspace_admin()).then(|| errors::forbidden("Admin required"))
}

/// `GET /api/admin/asset-statuses`: every status in picker order, archived
/// ones too, each with its `asset_count`, `usage_count` and `deletable`.
pub async fn admin_list(mut tc: TenantConn, auth: AuthContext) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    match tc.run(|conn| Ok((repo::list(conn, true)?, Counts::load(conn)?))) {
        Ok((statuses, counts)) => {
            let out: Vec<Value> = statuses.iter().map(|s| counts.of(s)).collect();
            HttpResponse::Ok().json(out)
        }
        Err(e) => {
            error!(error = ?e, "failed to list asset statuses");
            errors::internal("Failed to load asset statuses")
        }
    }
}

/// `GET /api/admin/asset-statuses/{id}`, with its counts.
pub async fn get(mut tc: TenantConn, auth: AuthContext, path: web::Path<i32>) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    match tc.run_result(|conn| {
        let s = repo::get(conn, id)?.ok_or(StatusError::NotFound)?;
        counted(conn, s)
    }) {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => status_error(e),
    }
}

/// `POST /api/admin/asset-statuses`.
pub async fn create(
    mut tc: TenantConn,
    auth: AuthContext,
    body: web::Json<CreateBody>,
) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let body = body.into_inner();
    let input = NewStatusInput {
        category: body.category,
        slug: body.slug,
        label: body.label,
        description: body.description,
        color: body.color,
        position: body.position,
        is_default: body.is_default,
        applies_to_kinds: body.applies_to_kinds,
        created_by: Some(auth.user_uuid),
    };
    match tc.run_result(|conn| repo::create(conn, input)) {
        Ok(s) => HttpResponse::Created().json(
            Counts {
                assets: HashMap::new(),
                usage: HashMap::new(),
            }
            .of(&s),
        ),
        Err(e) => status_error(e),
    }
}

/// `PUT /api/admin/asset-statuses/{id}`, answering with the status and its
/// counts.
pub async fn update(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    body: web::Json<UpdateBody>,
) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    let body = body.into_inner();
    let changes = StatusChanges {
        label: body.label,
        description: body.description,
        color: body.color,
        position: body.position,
        is_default: body.is_default,
        applies_to_kinds: body.applies_to_kinds,
    };
    match tc.run_result(|conn| {
        let s = repo::update(conn, id, changes)?;
        counted(conn, s)
    }) {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => status_error(e),
    }
}

/// `PUT /api/admin/asset-statuses/order`: a category's statuses in the
/// order `ids` lists them, answering with the category's statuses.
pub async fn reorder(
    mut tc: TenantConn,
    auth: AuthContext,
    body: web::Json<ReorderBody>,
) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let body = body.into_inner();
    match tc.run_result(|conn| repo::reorder(conn, &body.category, &body.ids)) {
        Ok(statuses) => HttpResponse::Ok().json(statuses),
        Err(e) => status_error(e),
    }
}

/// `POST /api/admin/asset-statuses/{id}/archive`: the rows holding it keep
/// it, and pickers leave it out.
pub async fn archive(mut tc: TenantConn, auth: AuthContext, path: web::Path<i32>) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    match tc.run_result(|conn| {
        let s = repo::archive(conn, id)?;
        counted(conn, s)
    }) {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => status_error(e),
    }
}

/// `POST /api/admin/asset-statuses/{id}/restore`.
pub async fn restore(mut tc: TenantConn, auth: AuthContext, path: web::Path<i32>) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    match tc.run_result(|conn| {
        let s = repo::restore(conn, id)?;
        counted(conn, s)
    }) {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => status_error(e),
    }
}

/// `DELETE /api/admin/asset-statuses/{id}`: only a status no asset,
/// lifecycle event or loan holds; archive the others.
pub async fn delete(mut tc: TenantConn, auth: AuthContext, path: web::Path<i32>) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    match tc.run_result(|conn| repo::delete(conn, id)) {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => status_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_null_clears_and_an_absent_field_leaves_it() {
        let body: UpdateBody =
            serde_json::from_value(json!({ "applies_to_kinds": null, "color": null })).unwrap();
        assert_eq!(body.applies_to_kinds, Some(None));
        assert_eq!(body.color, Some(None));
        let body: UpdateBody = serde_json::from_value(json!({ "position": 2 })).unwrap();
        assert_eq!(body.applies_to_kinds, None);
        assert_eq!(body.description, None);
    }
}
