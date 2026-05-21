//! Admin endpoints for the runtime asset-kind registry.
//!
//! - `GET    /api/admin/asset-kinds`       lists all kinds.
//! - `GET    /api/admin/asset-kinds/{id}`  fetches one.
//! - `POST   /api/admin/asset-kinds`       creates a non-builtin kind.
//! - `PUT    /api/admin/asset-kinds/{id}`  edits a kind's
//!   label, description, icon, sort_order, or attribute_schema.
//!   Slug and is_builtin are immutable post-creation so existing
//!   asset rows keep resolving.
//! - `DELETE /api/admin/asset-kinds/{id}`  deletes a non-builtin
//!   kind (refused on builtins).
//!
//! Every write validates the attribute_schema with
//! `services::assets::kinds::validate_schema` so a malformed
//! schema can't poison the registry.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error as DieselError;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use crate::db::Pool;
use crate::handlers::{errors, helpers};
use crate::models::{AssetKind, AssetKindUpdate, NewAssetKind};
use crate::repository::asset_kinds as repo;
use crate::services::assets::kinds as schema_validator;

const SLUG_MAX_LEN: usize = 64;
const LABEL_MAX_LEN: usize = 255;

/// Closed set of categories, mirrored from the DB CHECK
/// constraint on asset_kinds.category. The frontend uses these
/// to decide which IT-flavoured surfaces to render.
const VALID_CATEGORIES: &[&str] = &["it", "logical", "physical", "bulk", "generic"];

fn is_valid_category(c: &str) -> bool {
    VALID_CATEGORIES.contains(&c)
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub slug: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default = "default_schema")]
    pub attribute_schema: Value,
    #[serde(default = "default_sort_order")]
    pub sort_order: i32,
    #[serde(default = "default_category")]
    pub category: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBody {
    pub label: Option<String>,
    /// `Some(None)` clears the field; `None` leaves it unchanged.
    /// JSON `null` deserialises to `Some(None)` because `Option`'s
    /// own visitor handles null; absent keys fall through `serde
    /// (default)` to outer `None`.
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub icon: Option<Option<String>>,
    pub attribute_schema: Option<Value>,
    pub sort_order: Option<i32>,
    pub category: Option<String>,
}

fn default_schema() -> Value {
    json!({"type": "object", "properties": {}})
}

fn default_sort_order() -> i32 {
    100
}

fn default_category() -> String {
    "generic".to_string()
}

pub async fn list(pool: web::Data<Pool>, req: HttpRequest) -> impl Responder {
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repo::list_kinds(&mut conn) {
        Ok(kinds) => HttpResponse::Ok().json(kinds),
        Err(e) => {
            error!(error = %e, "failed to list asset kinds");
            errors::internal("Failed to list asset kinds")
        }
    }
}

pub async fn get(pool: web::Data<Pool>, path: web::Path<i32>, req: HttpRequest) -> impl Responder {
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let id = path.into_inner();
    match repo::get_kind(&mut conn, id) {
        Ok(kind) => HttpResponse::Ok().json(kind),
        Err(DieselError::NotFound) => errors::not_found_msg(format!("Asset kind {id} not found")),
        Err(e) => {
            error!(id, error = %e, "failed to load asset kind");
            errors::internal("Failed to load asset kind")
        }
    }
}

pub async fn create(
    pool: web::Data<Pool>,
    body: web::Json<CreateBody>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let body = body.into_inner();

    let slug = body.slug.trim().to_string();
    if !is_valid_slug(&slug) {
        return errors::bad_request(
            "slug must be 1 to 64 chars of lowercase letters, digits, and underscores",
        );
    }
    let label = body.label.trim().to_string();
    if label.is_empty() || label.len() > LABEL_MAX_LEN {
        return errors::bad_request(format!("label must be 1 to {LABEL_MAX_LEN} characters"));
    }

    if let Err(e) = schema_validator::validate_schema(&body.attribute_schema) {
        return errors::unprocessable_entity(format!("Invalid attribute_schema: {e}"));
    }

    if !is_valid_category(&body.category) {
        return errors::bad_request(format!(
            "category must be one of: {}",
            VALID_CATEGORIES.join(", ")
        ));
    }

    // Pull the admin's UUID for created_by. admin_conn already
    // verified the role; we just need the subject claim.
    let created_by = req
        .extensions()
        .get::<crate::models::Claims>()
        .and_then(|c| uuid::Uuid::parse_str(&c.sub).ok());

    let new_kind = NewAssetKind {
        slug,
        label,
        description: body.description.map(|s| s.trim().to_string()),
        icon: body.icon.map(|s| s.trim().to_string()),
        attribute_schema: body.attribute_schema,
        sort_order: body.sort_order,
        is_builtin: false,
        created_by,
        category: body.category,
    };

    match repo::create_kind(&mut conn, new_kind) {
        Ok(kind) => HttpResponse::Created().json(kind),
        Err(DieselError::DatabaseError(diesel::result::DatabaseErrorKind::UniqueViolation, _)) => {
            errors::bad_request("Another asset kind already uses that slug")
        }
        Err(e) => {
            error!(error = %e, "failed to create asset kind");
            errors::internal("Failed to create asset kind")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateQuery {
    /// When the update changes `attribute_schema`, the handler
    /// pre-checks every existing asset of this kind against the
    /// new schema and refuses the write if any would fail. Set
    /// `force=true` to apply the schema change anyway; existing
    /// invalid rows stay in the DB but get flagged on the asset
    /// detail page until an admin fixes their attributes. This
    /// is the safety-net pattern the frontend can rely on
    /// without bolting on a separate dry-run endpoint.
    #[serde(default)]
    pub force: Option<String>,
}

pub async fn update(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<UpdateBody>,
    query: web::Query<UpdateQuery>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let id = path.into_inner();
    let body = body.into_inner();
    let force = matches!(query.force.as_deref(), Some("true") | Some("1"));

    if let Some(ref label) = body.label {
        let trimmed = label.trim();
        if trimmed.is_empty() || trimmed.len() > LABEL_MAX_LEN {
            return errors::bad_request(format!("label must be 1 to {LABEL_MAX_LEN} characters"));
        }
    }
    if let Some(ref schema) = body.attribute_schema {
        if let Err(e) = schema_validator::validate_schema(schema) {
            return errors::unprocessable_entity(format!("Invalid attribute_schema: {e}"));
        }
    }
    if let Some(ref category) = body.category {
        if !is_valid_category(category) {
            return errors::bad_request(format!(
                "category must be one of: {}",
                VALID_CATEGORIES.join(", ")
            ));
        }
    }

    // Revalidate existing assets against the new schema before
    // applying the change, unless the admin has explicitly opted
    // in via `?force=true`. Cuts off the common footgun where a
    // tightened schema silently invalidates already-stored data.
    if let Some(ref new_schema) = body.attribute_schema {
        if !force {
            let existing_kind = match repo::get_kind(&mut conn, id) {
                Ok(k) => k,
                Err(DieselError::NotFound) => {
                    return errors::not_found_msg(format!("Asset kind {id} not found"))
                }
                Err(e) => {
                    error!(id, error = %e, "failed to load kind for revalidation");
                    return errors::internal("Failed to load asset kind");
                }
            };
            const SAMPLE_LIMIT: usize = 5;
            match schema_validator::count_invalid_assets_for_kind(
                &mut conn,
                &existing_kind.slug,
                new_schema,
                SAMPLE_LIMIT,
            ) {
                Ok((invalid_count, samples)) if invalid_count > 0 => {
                    return HttpResponse::Conflict().json(json!({
                        "error": "schema_invalidates_existing_assets",
                        "message": format!(
                            "{invalid_count} existing asset(s) of kind '{}' would no longer validate. \
                             Pass ?force=true to apply anyway, then fix or remove the listed rows.",
                            existing_kind.slug
                        ),
                        "invalid_count": invalid_count,
                        "sample": samples,
                    }));
                }
                Ok(_) => {}
                Err(e) => {
                    error!(id, error = %e, "failed to revalidate existing assets");
                    return errors::internal("Failed to revalidate existing assets");
                }
            }
        }
    }

    let update = AssetKindUpdate {
        label: body.label.map(|s| s.trim().to_string()),
        description: body
            .description
            .map(|opt| opt.map(|s| s.trim().to_string())),
        icon: body.icon.map(|opt| opt.map(|s| s.trim().to_string())),
        attribute_schema: body.attribute_schema,
        sort_order: body.sort_order,
        category: body.category,
        updated_at: None,
    };

    match repo::update_kind(&mut conn, id, update) {
        Ok(kind) => HttpResponse::Ok().json(kind),
        Err(DieselError::NotFound) => errors::not_found_msg(format!("Asset kind {id} not found")),
        Err(e) => {
            error!(id, error = %e, "failed to update asset kind");
            errors::internal("Failed to update asset kind")
        }
    }
}

pub async fn delete(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match helpers::admin_conn(&req, &pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let id = path.into_inner();

    // Lookup-then-delete so we can distinguish "not found",
    // "is_builtin" (delete refused), and "deleted" with clear
    // status codes. The delete itself filters on is_builtin =
    // false so the DB guarantees the same.
    let existing: AssetKind = match repo::get_kind(&mut conn, id) {
        Ok(k) => k,
        Err(DieselError::NotFound) => {
            return errors::not_found_msg(format!("Asset kind {id} not found"))
        }
        Err(e) => {
            error!(id, error = %e, "failed to load asset kind for delete");
            return errors::internal("Failed to delete asset kind");
        }
    };
    if existing.is_builtin {
        return errors::bad_request(
            "Built-in asset kinds cannot be deleted. Edit the attribute_schema or hide the kind by setting a high sort_order instead.",
        );
    }

    match repo::delete_kind(&mut conn, id) {
        Ok(0) => errors::not_found_msg(format!("Asset kind {id} not found")),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(id, error = %e, "failed to delete asset kind");
            errors::internal("Failed to delete asset kind")
        }
    }
}

fn is_valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.len() <= SLUG_MAX_LEN
        && slug
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_accepts_lowercase_alnum_underscore() {
        assert!(is_valid_slug("license"));
        assert!(is_valid_slug("network_device"));
        assert!(is_valid_slug("vehicle_2"));
    }

    #[test]
    fn slug_rejects_empty_uppercase_and_dashes() {
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("License"));
        assert!(!is_valid_slug("network-device"));
        assert!(!is_valid_slug("space inside"));
        assert!(!is_valid_slug(&"a".repeat(SLUG_MAX_LEN + 1)));
    }
}
