//! Asset models CRUD (the "device type" catalog).
//!
//! An asset model is a real make+model that assets are stamped from.
//! Technician-gated like manufacturers and asset creation. `kind` and
//! `default_attributes` are validated against the asset-kind registry so
//! the catalog can't hold a model that wouldn't stamp a valid asset.
//!
//! - `GET    /api/asset-models[?manufacturer_id=]`  list
//! - `GET    /api/asset-models/{id}`                fetch one
//! - `POST   /api/asset-models`                     create
//! - `PUT    /api/asset-models/{id}`                update
//! - `DELETE /api/asset-models/{id}`                delete (unlinks assets)

use actix_web::{web, HttpResponse, Responder};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{AssetModelChange, NewAssetModel};
use crate::repository::asset_models as repo;
use crate::services::assets::{validate_for_kind, AssetValidationError};

const NAME_MAX_LEN: usize = 255;

#[derive(Debug, Deserialize)]
pub struct CreateBody {
    pub manufacturer_id: i32,
    pub name: String,
    pub kind: String,
    #[serde(default)]
    pub part_number: Option<String>,
    #[serde(default = "default_attrs")]
    pub default_attributes: Value,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBody {
    pub manufacturer_id: Option<i32>,
    pub name: Option<String>,
    pub kind: Option<String>,
    /// `Some(None)` clears, `None` leaves unchanged.
    #[serde(default)]
    pub part_number: Option<Option<String>>,
    pub default_attributes: Option<Value>,
    #[serde(default)]
    pub notes: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub manufacturer_id: Option<i32>,
}

fn default_attrs() -> Value {
    json!({})
}

fn validation_err_response(err: AssetValidationError) -> HttpResponse {
    match err {
        AssetValidationError::UnknownKind(slug) => {
            errors::unprocessable_entity(format!("Unknown asset kind: {slug}"))
        }
        AssetValidationError::Attributes(inner) => {
            errors::unprocessable_entity(format!("Invalid default attributes: {inner}"))
        }
        AssetValidationError::Database(e) => {
            error!(error = ?e, "asset model validation db error");
            errors::internal("Failed to validate model")
        }
    }
}

pub async fn list(
    mut tc: TenantConn,
    query: web::Query<ListQuery>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let result = match query.manufacturer_id {
        Some(mid) => tc.run(move |conn| repo::list_for_manufacturer(conn, mid)),
        None => tc.run(repo::list),
    };
    match result {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "failed to list asset models");
            errors::internal("Failed to list asset models")
        }
    }
}

pub async fn get(mut tc: TenantConn, path: web::Path<i32>, auth: AuthContext) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let id = path.into_inner();
    match tc.run(|conn| repo::get(conn, id)) {
        Ok(row) => HttpResponse::Ok().json(row),
        Err(DieselError::NotFound) => errors::not_found_msg(format!("Asset model {id} not found")),
        Err(e) => {
            error!(id, error = %e, "failed to load asset model");
            errors::internal("Failed to load asset model")
        }
    }
}

pub async fn create(
    mut tc: TenantConn,
    body: web::Json<CreateBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let body = body.into_inner();

    let name = body.name.trim().to_string();
    if name.is_empty() || name.len() > NAME_MAX_LEN {
        return errors::bad_request(format!("name must be 1 to {NAME_MAX_LEN} characters"));
    }

    // A model never defaults sync-owned keys (Intune/Entra telemetry is
    // per-instance), so strip them before validating + storing.
    let default_attributes =
        crate::services::assets::strip_sync_owned_keys(&body.default_attributes);

    // Model must reference a real kind, and its default specs must be
    // valid for that kind, or it could never stamp a valid asset.
    let kind = body.kind.clone();
    let attrs = default_attributes.clone();
    match tc.run(move |conn| Ok::<_, DieselError>(validate_for_kind(conn, &kind, &attrs))) {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return validation_err_response(e),
        Err(e) => {
            error!(error = %e, "model validation failed");
            return errors::internal("Failed to validate model");
        }
    }

    let new = NewAssetModel {
        manufacturer_id: body.manufacturer_id,
        name,
        kind: body.kind,
        part_number: body.part_number.map(|s| s.trim().to_string()),
        default_attributes,
        notes: body.notes.map(|s| s.trim().to_string()),
        created_by: Some(auth.user_uuid),
    };
    match tc.run(|conn| repo::create(conn, new)) {
        Ok(row) => HttpResponse::Created().json(row),
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            errors::bad_request("That manufacturer already has a model with this name")
        }
        Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)) => {
            errors::bad_request("Unknown manufacturer")
        }
        Err(e) => {
            error!(error = %e, "failed to create asset model");
            errors::internal("Failed to create asset model")
        }
    }
}

pub async fn update(
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<UpdateBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let id = path.into_inner();
    let body = body.into_inner();

    if let Some(ref name) = body.name {
        let trimmed = name.trim();
        if trimmed.is_empty() || trimmed.len() > NAME_MAX_LEN {
            return errors::bad_request(format!("name must be 1 to {NAME_MAX_LEN} characters"));
        }
    }

    // Strip sync-owned keys from any incoming default specs; a model never
    // defaults Intune/Entra telemetry.
    let stripped_default_attributes = body
        .default_attributes
        .as_ref()
        .map(crate::services::assets::strip_sync_owned_keys);

    // If kind or default specs change, validate the effective pair
    // against the registry before applying.
    if body.kind.is_some() || stripped_default_attributes.is_some() {
        let existing = match tc.run(|conn| repo::get(conn, id)) {
            Ok(m) => m,
            Err(DieselError::NotFound) => {
                return errors::not_found_msg(format!("Asset model {id} not found"))
            }
            Err(e) => {
                error!(id, error = %e, "failed to load model for validation");
                return errors::internal("Failed to load asset model");
            }
        };
        let kind = body.kind.clone().unwrap_or(existing.kind);
        let attrs = stripped_default_attributes
            .clone()
            .unwrap_or(existing.default_attributes);
        match tc.run(move |conn| Ok::<_, DieselError>(validate_for_kind(conn, &kind, &attrs))) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return validation_err_response(e),
            Err(e) => {
                error!(error = %e, "model validation failed");
                return errors::internal("Failed to validate model");
            }
        }
    }

    let change = AssetModelChange {
        manufacturer_id: body.manufacturer_id,
        name: body.name.map(|s| s.trim().to_string()),
        kind: body.kind,
        part_number: body
            .part_number
            .map(|opt| opt.map(|s| s.trim().to_string())),
        default_attributes: stripped_default_attributes,
        notes: body.notes.map(|opt| opt.map(|s| s.trim().to_string())),
        updated_at: None,
    };
    match tc.run(|conn| repo::update(conn, id, change)) {
        Ok(row) => HttpResponse::Ok().json(row),
        Err(DieselError::NotFound) => errors::not_found_msg(format!("Asset model {id} not found")),
        Err(DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _)) => {
            errors::bad_request("That manufacturer already has a model with this name")
        }
        Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)) => {
            errors::bad_request("Unknown manufacturer")
        }
        Err(e) => {
            error!(id, error = %e, "failed to update asset model");
            errors::internal("Failed to update asset model")
        }
    }
}

pub async fn delete(mut tc: TenantConn, path: web::Path<i32>, auth: AuthContext) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let id = path.into_inner();
    // The asset FK is SET NULL, so a delete unlinks assets rather than
    // blocking; they keep their stamped manufacturer/model snapshot.
    match tc.run(|conn| repo::delete(conn, id)) {
        Ok(0) => errors::not_found_msg(format!("Asset model {id} not found")),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!(id, error = %e, "failed to delete asset model");
            errors::internal("Failed to delete asset model")
        }
    }
}
