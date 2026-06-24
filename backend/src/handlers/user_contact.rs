//! User contact fields: the workspace custom-field schema (admin) and the
//! per-user profile (self or admin).
//!
//! Schema reads are open to authenticated staff (the profile form needs them);
//! schema writes are workspace-admin. Profile reads/writes mirror the user-
//! update gate (self or admin). Custom-field values are validated against the
//! effective workspace schema; standard columns owned by the directory sync
//! are preserved (not overwritten by a manual edit).

use actix_web::{web, HttpResponse, Responder};
use serde_json::{json, Value};
use tracing::error;
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::UserProfileInput;
use crate::repository::user_contact as repo;
use crate::services::custom_fields::schema as field_schema;

// ---- Workspace user custom-field schema (admin) ----------------------------

pub async fn get_user_field_schema(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    match tc.run(|conn| repo::get_field_schema(conn)) {
        Ok(schema) => HttpResponse::Ok().json(schema),
        Err(e) => {
            error!(error = %e, "get user field schema failed");
            errors::internal("Failed to load user field schema")
        }
    }
}

pub async fn set_user_field_schema(
    mut tc: TenantConn,
    body: web::Json<Value>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can edit the user field schema");
    }
    let schema = body.into_inner();
    if let Err(e) = field_schema::validate_schema(&schema) {
        return errors::unprocessable_entity(format!("Invalid user field schema: {e}"));
    }
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::forbidden("A resolved workspace is required");
    };
    let actor = Some(auth.user_uuid);
    match tc.run(|conn| repo::set_field_schema(conn, workspace_id, &schema, actor)) {
        Ok(stored) => HttpResponse::Ok().json(stored),
        Err(e) => {
            error!(error = %e, "set user field schema failed");
            errors::internal("Failed to save user field schema")
        }
    }
}

// ---- Per-user contact profile ----------------------------------------------

fn empty_profile(user_uuid: Uuid) -> Value {
    json!({
        "user_uuid": user_uuid,
        "job_title": null,
        "organization": null,
        "department": null,
        "custom_fields": {},
        "directory_synced": false,
    })
}

pub async fn get_user_profile_fields(
    mut tc: TenantConn,
    params: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let user_uuid = params.into_inner();
    match tc.run(|conn| repo::get_profile(conn, user_uuid)) {
        Ok(Some(profile)) => HttpResponse::Ok().json(profile),
        Ok(None) => HttpResponse::Ok().json(empty_profile(user_uuid)),
        Err(e) => {
            error!(error = %e, "get user profile fields failed");
            errors::internal("Failed to load user profile")
        }
    }
}

pub async fn set_user_profile_fields(
    mut tc: TenantConn,
    params: web::Path<Uuid>,
    body: web::Json<UserProfileInput>,
    auth: AuthContext,
) -> impl Responder {
    let user_uuid = params.into_inner();
    if auth.user_uuid != user_uuid && !auth.is_workspace_admin() {
        return errors::forbidden("You can only edit your own profile");
    }
    let mut input = body.into_inner();

    let result = tc.run(|conn| {
        // Validate the custom-field values against the effective schema.
        let schema = repo::get_field_schema(conn)?;
        if let Err(e) = field_schema::validate_attributes(&schema, &input.custom_fields) {
            return Ok(Err(format!("Invalid custom fields: {e}")));
        }
        // Preserve directory-synced standard columns: a manual edit can't
        // change job_title/organization/department on a Graph-owned profile.
        if let Some(existing) = repo::get_profile(conn, user_uuid)? {
            if existing.directory_synced {
                input.job_title = existing.job_title.clone();
                input.organization = existing.organization.clone();
                input.department = existing.department.clone();
            }
        }
        let saved = repo::upsert_profile(conn, user_uuid, &input, Some(auth.user_uuid))?;
        Ok(Ok(saved))
    });

    match result {
        Ok(Ok(profile)) => HttpResponse::Ok().json(profile),
        Ok(Err(msg)) => errors::unprocessable_entity(msg),
        Err(e) => {
            error!(error = %e, "set user profile fields failed");
            errors::internal("Failed to save user profile")
        }
    }
}
