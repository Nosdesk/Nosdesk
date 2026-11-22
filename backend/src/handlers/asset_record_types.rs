//! Asset record types over REST.
//!
//! For staff, under `/api/assets` so `assets:*` tokens reach them:
//!
//! - `GET  /api/assets/record-types`: the workspace's types for the record
//!   form, with the workspace currency the form's cost field offers.
//! - `GET  /api/assets/record-types/catalogue`: the built-in types, each
//!   marked as added to the workspace or not.
//! - `POST /api/assets/record-types/catalogue/{slug}`: add a built-in. Any
//!   staff member may, because the record form offers the built-ins the first
//!   time someone adds a record; adding is idempotent and configures nothing
//!   of the workspace's own.
//!
//! For workspace admins, under `/api/admin/asset-record-types`: list (with
//! archived types and record counts), get, create, edit (`?force=true` saves
//! a field or outcome change that some records no longer fit), archive,
//! restore, and delete an unused type of the workspace's own.

use std::collections::HashMap;

use actix_web::http::StatusCode;
use actix_web::{web, HttpResponse};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use crate::errors;
use crate::extractors::{AuthContext, TenantConn};
use crate::models::{AssetRecordType, RecordOutcome};
use crate::repository::asset_record_types::{
    self as repo, NewRecordTypeInput, RecordTypeChanges, RecordTypeError,
};
use crate::repository::site_settings as settings_repo;
use crate::services::assets::record_catalogue;
use crate::utils::money;

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/asset-record-types", web::get().to(admin_list))
        .route("/admin/asset-record-types", web::post().to(create))
        .route("/admin/asset-record-types/{id:\\d+}", web::get().to(get))
        .route("/admin/asset-record-types/{id:\\d+}", web::put().to(update))
        .route(
            "/admin/asset-record-types/{id:\\d+}",
            web::delete().to(delete),
        )
        .route(
            "/admin/asset-record-types/{id:\\d+}/archive",
            web::post().to(archive),
        )
        .route(
            "/admin/asset-record-types/{id:\\d+}/restore",
            web::post().to(restore),
        );
}

#[derive(Debug, Deserialize)]
pub struct PickerQuery {
    /// Archived types too, to name the type of an old record.
    #[serde(default)]
    pub include_archived: bool,
}

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub slug: String,
    pub label: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default = "empty_schema")]
    pub field_schema: Value,
    #[serde(default)]
    pub field_order: Vec<String>,
    #[serde(default = "empty_object")]
    pub core_fields: Value,
    #[serde(default)]
    pub outcomes: Vec<RecordOutcome>,
    /// Kind slugs offered the type; absent or `null` for every kind.
    #[serde(default)]
    pub applies_to_kinds: Option<Vec<String>>,
    #[serde(default)]
    pub opens_with_status: Option<String>,
    #[serde(default = "default_sort_order")]
    pub sort_order: i32,
}

/// An edit: a field left out keeps its value, and `null` clears a field
/// that may be empty. `"label": null` puts a built-in back to its catalogue
/// name.
#[derive(Debug, Deserialize)]
pub struct UpdateBody {
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub label: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub description: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub icon: Option<Option<String>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub color: Option<Option<String>>,
    #[serde(default)]
    pub field_schema: Option<Value>,
    #[serde(default)]
    pub field_order: Option<Vec<String>>,
    #[serde(default)]
    pub core_fields: Option<Value>,
    #[serde(default)]
    pub outcomes: Option<Vec<RecordOutcome>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub applies_to_kinds: Option<Option<Vec<String>>>,
    #[serde(default, deserialize_with = "crate::utils::double_option::deserialize")]
    pub opens_with_status: Option<Option<String>>,
    #[serde(default)]
    pub sort_order: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateQuery {
    /// Save a field or outcome change even when some records no longer fit.
    #[serde(default)]
    pub force: bool,
}

fn empty_schema() -> Value {
    json!({ "type": "object", "properties": {} })
}

fn empty_object() -> Value {
    json!({})
}

fn default_sort_order() -> i32 {
    100
}

fn unprocessable(code: &str, message: impl Into<String>, extra: Value) -> HttpResponse {
    errors::with_fields(StatusCode::UNPROCESSABLE_ENTITY, code, message, extra)
}

fn conflict(code: &str, message: impl Into<String>, extra: Value) -> HttpResponse {
    errors::with_fields(StatusCode::CONFLICT, code, message, extra)
}

/// The status and body for each of the repository's refusals.
fn type_error(e: RecordTypeError) -> HttpResponse {
    match e {
        RecordTypeError::NotFound => errors::not_found_msg("Record type not found"),
        RecordTypeError::InvalidKey(key) => unprocessable(
            "INVALID_KEY",
            format!("`{key}` must be lower case letters, digits and underscores, starting with a letter"),
            json!({ "key": key }),
        ),
        RecordTypeError::SlugTaken => conflict(
            "SLUG_TAKEN",
            "Another record type already uses that slug",
            json!({}),
        ),
        RecordTypeError::SlugReserved => conflict(
            "SLUG_RESERVED",
            "That slug is a built-in type's; add it from the catalogue",
            json!({}),
        ),
        RecordTypeError::LabelRequired => unprocessable(
            "LABEL_REQUIRED",
            "A record type of the workspace's own needs a name",
            json!({}),
        ),
        RecordTypeError::Schema(e) => unprocessable(
            "INVALID_SCHEMA",
            format!("Invalid field_schema: {e}"),
            json!({}),
        ),
        RecordTypeError::CoreFields(message) => unprocessable(
            "INVALID_CORE_FIELDS",
            format!("Invalid core_fields: {message}"),
            json!({}),
        ),
        RecordTypeError::Outcomes(message) => unprocessable(
            "INVALID_OUTCOMES",
            format!("Invalid outcomes: {message}"),
            json!({}),
        ),
        RecordTypeError::UnknownKind(kind) => unprocessable(
            "UNKNOWN_KIND",
            format!("Unknown asset kind: {kind}"),
            json!({ "kind": kind }),
        ),
        RecordTypeError::UnknownStatus(status) => unprocessable(
            "UNKNOWN_STATUS",
            format!("Unknown asset status: {status}"),
            json!({ "status": status }),
        ),
        RecordTypeError::StatusTaken => conflict(
            "STATUS_TAKEN",
            "Another record type already opens with that status",
            json!({}),
        ),
        RecordTypeError::InvalidRecords { count, samples } => conflict(
            "RECORDS_WOULD_NOT_FIT",
            format!(
                "{count} existing record(s) of this type would no longer fit. \
                 Pass ?force=true to save anyway, then fix the listed records."
            ),
            json!({ "invalid_count": count, "sample": samples }),
        ),
        RecordTypeError::Builtin => conflict(
            "BUILTIN",
            "Built-in record types are archived, never deleted",
            json!({}),
        ),
        RecordTypeError::InUse { count } => conflict(
            "IN_USE",
            format!("{count} record(s) use this type; archive it instead"),
            json!({ "record_count": count }),
        ),
        RecordTypeError::NotInCatalogue => {
            errors::not_found_msg("No built-in record type has that slug")
        }
        RecordTypeError::TooLong { field, max } => unprocessable(
            "TOO_LONG",
            format!("{field} is longer than {max} characters"),
            json!({ "field": field, "max": max }),
        ),
        RecordTypeError::InvalidColor(color) => unprocessable(
            "INVALID_COLOR",
            "A colour is #rrggbb",
            json!({ "color": color }),
        ),
        RecordTypeError::Db(diesel::result::Error::DatabaseError(
            diesel::result::DatabaseErrorKind::UniqueViolation,
            _,
        )) => conflict(
            "SLUG_TAKEN",
            "Another record type already uses that slug",
            json!({}),
        ),
        RecordTypeError::Db(e) => {
            error!(error = ?e, "asset record type request failed");
            errors::internal("Failed to process the record type")
        }
    }
}

/// A type with how many records use it.
fn with_count(t: &AssetRecordType, count: i64) -> Value {
    let mut out = json!(t);
    if let Value::Object(map) = &mut out {
        map.insert("record_count".into(), json!(count));
    }
    out
}

/// `GET /api/assets/record-types`: the workspace's types for the record
/// form, and the currency its cost field offers: the workspace currency,
/// and the one the default locale suggests while that is unset.
pub async fn list_for_picker(
    mut tc: TenantConn,
    auth: AuthContext,
    query: web::Query<PickerQuery>,
) -> HttpResponse {
    if !auth.can_handle_tickets() {
        return errors::not_found_msg("Not found");
    }
    let include_archived = query.include_archived;
    match tc.run(|conn| {
        let types = repo::list(conn, include_archived)?;
        let settings = settings_repo::get_site_settings(conn)?;
        Ok((types, settings))
    }) {
        Ok((types, settings)) => HttpResponse::Ok().json(json!({
            "types": types,
            "currency": settings.currency_code,
            "suggested_currency": money::suggested_currency(&settings.default_locale),
        })),
        Err(e) => {
            error!(error = ?e, "failed to list asset record types");
            errors::internal("Failed to load record types")
        }
    }
}

/// `GET /api/assets/record-types/catalogue`: every built-in type, with
/// `added` (the workspace has it, archived or not) and the workspace's type.
pub async fn catalogue(mut tc: TenantConn, auth: AuthContext) -> HttpResponse {
    if !auth.can_handle_tickets() {
        return errors::not_found_msg("Not found");
    }
    match tc.run(|conn| repo::list(conn, true)) {
        Ok(types) => {
            let by_slug: HashMap<&str, &AssetRecordType> =
                types.iter().map(|t| (t.slug.as_str(), t)).collect();
            let entries: Vec<Value> = record_catalogue::CATALOGUE
                .iter()
                .map(|entry| {
                    let added = by_slug.get(entry.slug).copied();
                    json!({
                        "slug": entry.slug,
                        "icon": entry.icon,
                        "sort_order": entry.sort_order,
                        "opens_with_status": entry.opens_with_status.map(|s| s.as_str()),
                        "field_schema": entry.field_schema(),
                        "field_order": entry.field_order(),
                        "core_fields": entry.core_fields(),
                        "outcomes": entry.outcomes(),
                        "added": added.is_some(),
                        "type": added,
                    })
                })
                .collect();
            HttpResponse::Ok().json(entries)
        }
        Err(e) => {
            error!(error = ?e, "failed to list the record type catalogue");
            errors::internal("Failed to load the record type catalogue")
        }
    }
}

/// `POST /api/assets/record-types/catalogue/{slug}`: add a built-in to the
/// workspace. 201 with the new type, or 200 with the one already added (as
/// it stands: renamed, edited or archived).
pub async fn add_from_catalogue(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<String>,
) -> HttpResponse {
    if !auth.can_handle_tickets() {
        return errors::not_found_msg("Not found");
    }
    let slug = path.into_inner();
    let created_by = Some(auth.user_uuid);
    match tc.run_result(|conn| {
        let existed = repo::get_by_slug(conn, &slug)?.is_some();
        let added = repo::add_from_catalogue(conn, &slug, created_by)?;
        Ok::<_, RecordTypeError>((existed, added))
    }) {
        Ok((false, added)) => HttpResponse::Created().json(added),
        Ok((true, added)) => HttpResponse::Ok().json(added),
        Err(e) => type_error(e),
    }
}

fn require_admin(auth: &AuthContext) -> Option<HttpResponse> {
    (!auth.is_workspace_admin()).then(|| errors::forbidden("Admin required"))
}

/// `GET /api/admin/asset-record-types`: every type, archived ones too, each
/// with its `record_count`.
pub async fn admin_list(mut tc: TenantConn, auth: AuthContext) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    match tc.run(|conn| Ok((repo::list(conn, true)?, repo::record_counts(conn)?))) {
        Ok((types, counts)) => {
            let counts: HashMap<i32, i64> = counts.into_iter().collect();
            let out: Vec<Value> = types
                .iter()
                .map(|t| with_count(t, counts.get(&t.id).copied().unwrap_or(0)))
                .collect();
            HttpResponse::Ok().json(out)
        }
        Err(e) => {
            error!(error = ?e, "failed to list asset record types");
            errors::internal("Failed to load record types")
        }
    }
}

/// `GET /api/admin/asset-record-types/{id}`, with its `record_count`.
pub async fn get(mut tc: TenantConn, auth: AuthContext, path: web::Path<i32>) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    match tc.run_result(|conn| {
        let t = repo::get(conn, id)?.ok_or(RecordTypeError::NotFound)?;
        let count = repo::count_records(conn, id)?;
        Ok::<_, RecordTypeError>(with_count(&t, count))
    }) {
        Ok(body) => HttpResponse::Ok().json(body),
        Err(e) => type_error(e),
    }
}

/// `POST /api/admin/asset-record-types`: a type of the workspace's own.
pub async fn create(
    mut tc: TenantConn,
    auth: AuthContext,
    body: web::Json<CreateBody>,
) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let body = body.into_inner();
    let input = NewRecordTypeInput {
        slug: body.slug,
        label: body.label,
        description: body.description,
        icon: body.icon,
        color: body.color,
        field_schema: body.field_schema,
        field_order: body.field_order,
        core_fields: body.core_fields,
        outcomes: body.outcomes,
        applies_to_kinds: body.applies_to_kinds,
        opens_with_status: body.opens_with_status,
        sort_order: body.sort_order,
        created_by: Some(auth.user_uuid),
    };
    match tc.run_result(|conn| repo::create(conn, input)) {
        Ok(t) => HttpResponse::Created().json(t),
        Err(e) => type_error(e),
    }
}

/// `PUT /api/admin/asset-record-types/{id}`.
pub async fn update(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    query: web::Query<UpdateQuery>,
    body: web::Json<UpdateBody>,
) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    let body = body.into_inner();
    let changes = RecordTypeChanges {
        label: body.label,
        description: body.description,
        icon: body.icon,
        color: body.color,
        field_schema: body.field_schema,
        field_order: body.field_order,
        core_fields: body.core_fields,
        outcomes: body.outcomes,
        applies_to_kinds: body.applies_to_kinds,
        opens_with_status: body.opens_with_status,
        sort_order: body.sort_order,
    };
    match tc.run_result(|conn| repo::update(conn, id, changes, query.force)) {
        Ok(t) => HttpResponse::Ok().json(t),
        Err(e) => type_error(e),
    }
}

/// `POST /api/admin/asset-record-types/{id}/archive`: its records keep it,
/// and pickers leave it out.
pub async fn archive(mut tc: TenantConn, auth: AuthContext, path: web::Path<i32>) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    match tc.run_result(|conn| repo::archive(conn, id)) {
        Ok(t) => HttpResponse::Ok().json(t),
        Err(e) => type_error(e),
    }
}

/// `POST /api/admin/asset-record-types/{id}/restore`.
pub async fn restore(mut tc: TenantConn, auth: AuthContext, path: web::Path<i32>) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    match tc.run_result(|conn| repo::restore(conn, id)) {
        Ok(t) => HttpResponse::Ok().json(t),
        Err(e) => type_error(e),
    }
}

/// `DELETE /api/admin/asset-record-types/{id}`: only a type of the
/// workspace's own that no record uses; archive the others.
pub async fn delete(mut tc: TenantConn, auth: AuthContext, path: web::Path<i32>) -> HttpResponse {
    if let Some(refused) = require_admin(&auth) {
        return refused;
    }
    let id = path.into_inner();
    match tc.run_result(|conn| repo::delete(conn, id)) {
        Ok(()) => HttpResponse::NoContent().finish(),
        Err(e) => type_error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_null_label_puts_a_built_in_back_and_an_absent_one_leaves_it() {
        let body: UpdateBody = serde_json::from_value(json!({ "label": null })).unwrap();
        assert_eq!(body.label, Some(None));
        let body: UpdateBody = serde_json::from_value(json!({ "sort_order": 5 })).unwrap();
        assert_eq!(body.label, None);
        assert_eq!(body.applies_to_kinds, None);
        let body: UpdateBody = serde_json::from_value(json!({ "applies_to_kinds": null })).unwrap();
        assert_eq!(body.applies_to_kinds, Some(None));
    }
}
