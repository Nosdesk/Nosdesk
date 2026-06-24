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
use crate::models::{UserAddressInput, UserPhoneInput, UserProfileInput};
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

// ---- Phones ----------------------------------------------------------------

pub async fn list_user_phones(
    mut tc: TenantConn,
    params: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let user_uuid = params.into_inner();
    match tc.run(|conn| repo::list_phones(conn, user_uuid)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "list user phones failed");
            errors::internal("Failed to load phone numbers")
        }
    }
}

pub async fn add_user_phone(
    mut tc: TenantConn,
    params: web::Path<Uuid>,
    body: web::Json<UserPhoneInput>,
    auth: AuthContext,
) -> impl Responder {
    let user_uuid = params.into_inner();
    if auth.user_uuid != user_uuid && !auth.is_workspace_admin() {
        return errors::forbidden("You can only edit your own contact details");
    }
    let input = body.into_inner();
    match tc.run(|conn| repo::create_phone(conn, user_uuid, &input, None, Some(auth.user_uuid))) {
        Ok(row) => HttpResponse::Ok().json(row),
        Err(e) => {
            error!(error = %e, "add user phone failed");
            errors::internal("Failed to add phone number")
        }
    }
}

pub async fn update_user_phone(
    mut tc: TenantConn,
    params: web::Path<(Uuid, i32)>,
    body: web::Json<UserPhoneInput>,
    auth: AuthContext,
) -> impl Responder {
    let (user_uuid, id) = params.into_inner();
    if auth.user_uuid != user_uuid && !auth.is_workspace_admin() {
        return errors::forbidden("You can only edit your own contact details");
    }
    let input = body.into_inner();
    let result = tc.run(|conn| {
        let Some(existing) = repo::get_phone(conn, id)? else {
            return Ok(Err("not_found"));
        };
        if existing.user_uuid != user_uuid {
            return Ok(Err("not_found"));
        }
        if existing.source.is_some() {
            return Ok(Err("sync_owned"));
        }
        Ok(Ok(repo::update_phone(conn, id, user_uuid, &input)?))
    });
    finish_row(
        result,
        "update user phone failed",
        "Failed to update phone number",
    )
}

pub async fn delete_user_phone(
    mut tc: TenantConn,
    params: web::Path<(Uuid, i32)>,
    auth: AuthContext,
) -> impl Responder {
    let (user_uuid, id) = params.into_inner();
    if auth.user_uuid != user_uuid && !auth.is_workspace_admin() {
        return errors::forbidden("You can only edit your own contact details");
    }
    let result = tc.run(|conn| {
        let Some(existing) = repo::get_phone(conn, id)? else {
            return Ok(Err("not_found"));
        };
        if existing.user_uuid != user_uuid {
            return Ok(Err("not_found"));
        }
        if existing.source.is_some() {
            return Ok(Err("sync_owned"));
        }
        repo::delete_phone(conn, id)?;
        Ok(Ok(()))
    });
    finish_unit(
        result,
        "delete user phone failed",
        "Failed to delete phone number",
    )
}

// ---- Addresses -------------------------------------------------------------

pub async fn list_user_addresses(
    mut tc: TenantConn,
    params: web::Path<Uuid>,
    _auth: AuthContext,
) -> impl Responder {
    let user_uuid = params.into_inner();
    match tc.run(|conn| repo::list_addresses(conn, user_uuid)) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = %e, "list user addresses failed");
            errors::internal("Failed to load addresses")
        }
    }
}

pub async fn add_user_address(
    mut tc: TenantConn,
    params: web::Path<Uuid>,
    body: web::Json<UserAddressInput>,
    auth: AuthContext,
) -> impl Responder {
    let user_uuid = params.into_inner();
    if auth.user_uuid != user_uuid && !auth.is_workspace_admin() {
        return errors::forbidden("You can only edit your own contact details");
    }
    let input = body.into_inner();
    match tc.run(|conn| repo::create_address(conn, user_uuid, &input, None, Some(auth.user_uuid))) {
        Ok(row) => HttpResponse::Ok().json(row),
        Err(e) => {
            error!(error = %e, "add user address failed");
            errors::internal("Failed to add address")
        }
    }
}

pub async fn update_user_address(
    mut tc: TenantConn,
    params: web::Path<(Uuid, i32)>,
    body: web::Json<UserAddressInput>,
    auth: AuthContext,
) -> impl Responder {
    let (user_uuid, id) = params.into_inner();
    if auth.user_uuid != user_uuid && !auth.is_workspace_admin() {
        return errors::forbidden("You can only edit your own contact details");
    }
    let input = body.into_inner();
    let result = tc.run(|conn| {
        let Some(existing) = repo::get_address(conn, id)? else {
            return Ok(Err("not_found"));
        };
        if existing.user_uuid != user_uuid {
            return Ok(Err("not_found"));
        }
        if existing.source.is_some() {
            return Ok(Err("sync_owned"));
        }
        Ok(Ok(repo::update_address(conn, id, user_uuid, &input)?))
    });
    finish_row(
        result,
        "update user address failed",
        "Failed to update address",
    )
}

pub async fn delete_user_address(
    mut tc: TenantConn,
    params: web::Path<(Uuid, i32)>,
    auth: AuthContext,
) -> impl Responder {
    let (user_uuid, id) = params.into_inner();
    if auth.user_uuid != user_uuid && !auth.is_workspace_admin() {
        return errors::forbidden("You can only edit your own contact details");
    }
    let result = tc.run(|conn| {
        let Some(existing) = repo::get_address(conn, id)? else {
            return Ok(Err("not_found"));
        };
        if existing.user_uuid != user_uuid {
            return Ok(Err("not_found"));
        }
        if existing.source.is_some() {
            return Ok(Err("sync_owned"));
        }
        repo::delete_address(conn, id)?;
        Ok(Ok(()))
    });
    finish_unit(
        result,
        "delete user address failed",
        "Failed to delete address",
    )
}

// ---- Shared result mapping for the load-guard-mutate pattern ----------------

fn finish_row<T: serde::Serialize>(
    result: diesel::QueryResult<Result<T, &'static str>>,
    log_msg: &str,
    err_msg: &str,
) -> HttpResponse {
    match result {
        Ok(Ok(row)) => HttpResponse::Ok().json(row),
        Ok(Err("not_found")) => errors::not_found("Contact entry"),
        Ok(Err("sync_owned")) => {
            errors::forbidden("Directory-synced contact details are read-only")
        }
        Ok(Err(_)) => errors::internal(err_msg),
        Err(e) => {
            error!(error = %e, "{log_msg}");
            errors::internal(err_msg)
        }
    }
}

fn finish_unit(
    result: diesel::QueryResult<Result<(), &'static str>>,
    log_msg: &str,
    err_msg: &str,
) -> HttpResponse {
    match result {
        Ok(Ok(())) => HttpResponse::NoContent().finish(),
        Ok(Err("not_found")) => errors::not_found("Contact entry"),
        Ok(Err("sync_owned")) => {
            errors::forbidden("Directory-synced contact details are read-only")
        }
        Ok(Err(_)) => errors::internal(err_msg),
        Err(e) => {
            error!(error = %e, "{log_msg}");
            errors::internal(err_msg)
        }
    }
}
