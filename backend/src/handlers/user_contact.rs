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
use crate::models::{
    UserAddress, UserAddressInput, UserPhoneInput, UserPhoneNumber, UserProfileInput,
};
use crate::repository::user_contact as repo;
use crate::services::custom_fields::schema as field_schema;

/// Contact-mutation gate shared by the handlers: self, workspace-admin, OR an
/// agent editing a REQUESTER (a `member`) of this workspace. Agents log and
/// manage customers, but never edit another staff member's contact (that stays
/// self-or-admin). Returns the forbidden response to early-return, or None when
/// authorized.
fn guard_contact_edit(
    auth: &AuthContext,
    tc: &mut TenantConn,
    user_uuid: Uuid,
) -> Option<HttpResponse> {
    if auth.user_uuid == user_uuid || auth.is_workspace_admin() {
        return None;
    }
    // Agent-or-above may edit a requester (member) of their own workspace. The
    // membership read is RLS-scoped to the pinned workspace, so it can only see
    // the caller's own tenant; a staff target (agent/admin) is not a requester
    // and falls through to forbidden.
    if auth.can_handle_tickets() {
        if let Some(ws_id) = tc.workspace_id() {
            if let Ok(Some(m)) =
                tc.run(|conn| crate::repository::workspaces::membership(conn, ws_id, user_uuid))
            {
                if crate::models::WorkspaceRole::from_db(&m.role)
                    == crate::models::WorkspaceRole::Member
                {
                    return None;
                }
            }
        }
    }
    Some(errors::forbidden(
        "You can only edit your own contact details, or a requester in your workspace.",
    ))
}

/// A satellite contact row carrying ownership + sync provenance.
trait ContactRow {
    fn owner(&self) -> Uuid;
    fn source(&self) -> Option<&str>;
}
impl ContactRow for UserPhoneNumber {
    fn owner(&self) -> Uuid {
        self.user_uuid
    }
    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}
impl ContactRow for UserAddress {
    fn owner(&self) -> Uuid {
        self.user_uuid
    }
    fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }
}

/// Guard a loaded row before a mutation: it must exist, belong to the path
/// user, and be manually-owned (not directory-synced). Returns the sentinel the
/// finish_* mappers turn into a 404/403.
fn guard_editable<T: ContactRow>(row: Option<T>, user_uuid: Uuid) -> Result<(), &'static str> {
    let row = row.ok_or("not_found")?;
    if row.owner() != user_uuid {
        return Err("not_found");
    }
    if row.source().is_some() {
        return Err("sync_owned");
    }
    Ok(())
}

/// Custom-field keys a schema marks `synced` (directory-owned, read-only).
fn synced_property_keys(schema: &Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| {
            props
                .iter()
                .filter(|(_, v)| v.get("synced").and_then(Value::as_bool).unwrap_or(false))
                .map(|(k, _)| k.clone())
                .collect()
        })
        .unwrap_or_default()
}

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

#[derive(serde::Deserialize)]
pub struct SchemaWriteQuery {
    /// `true`/`1` applies a schema change even if it invalidates stored values.
    pub force: Option<String>,
}

pub async fn set_user_field_schema(
    mut tc: TenantConn,
    body: web::Json<Value>,
    query: web::Query<SchemaWriteQuery>,
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
    let force = matches!(query.force.as_deref(), Some("true") | Some("1"));

    // Unless forced, reject a schema change that would invalidate values already
    // stored on user profiles, mirroring the asset-kind conflict guard.
    if !force {
        const SAMPLE_LIMIT: usize = 5;
        let schema_clone = schema.clone();
        let counted = tc.run(|conn| {
            let mut invalid = 0usize;
            let mut sample: Vec<Value> = Vec::new();
            for (user_uuid, custom_fields) in repo::list_profile_custom_fields(conn)? {
                if let Err(e) = field_schema::validate_attributes(&schema_clone, &custom_fields) {
                    invalid += 1;
                    if sample.len() < SAMPLE_LIMIT {
                        sample.push(json!({ "user_uuid": user_uuid, "error": e.to_string() }));
                    }
                }
            }
            Ok::<_, diesel::result::Error>((invalid, sample))
        });
        match counted {
            Ok((invalid, sample)) if invalid > 0 => {
                return HttpResponse::Conflict().json(json!({
                    "error": "schema_invalidates_existing_profiles",
                    "message": format!(
                        "{invalid} user profile(s) have values that would no longer validate. \
                         Pass ?force=true to apply anyway, then fix or clear the affected values."
                    ),
                    "invalid_count": invalid,
                    "sample": sample,
                }));
            }
            Ok(_) => {}
            Err(e) => {
                error!(error = %e, "revalidate user profiles failed");
                return errors::internal("Failed to revalidate user profiles");
            }
        }
    }

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
        let schema = repo::get_field_schema(conn)?;
        let existing = repo::get_profile(conn, user_uuid)?;
        let is_directory_synced = existing
            .as_ref()
            .map(|p| p.directory_synced)
            .unwrap_or(false);

        // The directory owns `synced` custom-field keys ONLY when this profile is
        // actually directory-managed: drop manual attempts to set them and
        // restore the stored values (the UI hides them, and a direct API call
        // must not forge or clear them). For a local profile there's no directory
        // feeding them, so they're ordinary editable fields and pass through.
        if is_directory_synced {
            for key in synced_property_keys(&schema) {
                if let Some(obj) = input.custom_fields.as_object_mut() {
                    obj.remove(&key);
                    if let Some(v) = existing.as_ref().and_then(|p| p.custom_fields.get(&key)) {
                        obj.insert(key, v.clone());
                    }
                }
            }
        }

        // Validate the custom-field values against the effective schema.
        if let Err(e) = field_schema::validate_attributes(&schema, &input.custom_fields) {
            return Ok(Err(format!("Invalid custom fields: {e}")));
        }
        // Preserve directory-synced standard columns: a manual edit can't
        // change job_title/organization/department on a Graph-owned profile.
        if let Some(existing) = existing {
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
    if let Some(resp) = guard_contact_edit(&auth, &mut tc, user_uuid) {
        return resp;
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
    if let Some(resp) = guard_contact_edit(&auth, &mut tc, user_uuid) {
        return resp;
    }
    let input = body.into_inner();
    let result = tc.run(|conn| {
        if let Err(e) = guard_editable(repo::get_phone(conn, id)?, user_uuid) {
            return Ok(Err(e));
        }
        Ok(Ok(repo::update_phone(conn, id, user_uuid, &input)?))
    });
    finish(
        result,
        |row| HttpResponse::Ok().json(row),
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
    if let Some(resp) = guard_contact_edit(&auth, &mut tc, user_uuid) {
        return resp;
    }
    let result = tc.run(|conn| {
        if let Err(e) = guard_editable(repo::get_phone(conn, id)?, user_uuid) {
            return Ok(Err(e));
        }
        repo::delete_phone(conn, id)?;
        Ok(Ok(()))
    });
    finish(
        result,
        |_| HttpResponse::NoContent().finish(),
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
    if let Some(resp) = guard_contact_edit(&auth, &mut tc, user_uuid) {
        return resp;
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
    if let Some(resp) = guard_contact_edit(&auth, &mut tc, user_uuid) {
        return resp;
    }
    let input = body.into_inner();
    let result = tc.run(|conn| {
        if let Err(e) = guard_editable(repo::get_address(conn, id)?, user_uuid) {
            return Ok(Err(e));
        }
        Ok(Ok(repo::update_address(conn, id, user_uuid, &input)?))
    });
    finish(
        result,
        |row| HttpResponse::Ok().json(row),
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
    if let Some(resp) = guard_contact_edit(&auth, &mut tc, user_uuid) {
        return resp;
    }
    let result = tc.run(|conn| {
        if let Err(e) = guard_editable(repo::get_address(conn, id)?, user_uuid) {
            return Ok(Err(e));
        }
        repo::delete_address(conn, id)?;
        Ok(Ok(()))
    });
    finish(
        result,
        |_| HttpResponse::NoContent().finish(),
        "delete user address failed",
        "Failed to delete address",
    )
}

// ---- Shared result mapping for the load-guard-mutate pattern ----------------

/// Map a load-guard-mutate outcome to a response: the guard sentinels become
/// 404/403, infra errors 500, and `ok` renders the success body.
fn finish<T>(
    result: diesel::QueryResult<Result<T, &'static str>>,
    ok: impl FnOnce(T) -> HttpResponse,
    log_msg: &str,
    err_msg: &str,
) -> HttpResponse {
    match result {
        Ok(Ok(value)) => ok(value),
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
