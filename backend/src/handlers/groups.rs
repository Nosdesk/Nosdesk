use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::Deserialize;
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::helpers;
use crate::handlers::errors;
use crate::models::{NewGroup, GroupUpdate, Claims};
use crate::repository;
use crate::utils::rbac::require_admin;

// ============================================================================
// Group Detail Endpoint (All authenticated users)
// ============================================================================

/// Get group details by UUID (accessible to all authenticated users)
pub async fn get_group_details(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    // Verify user is authenticated (but not admin required)
    let _claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    let group_uuid = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid group UUID"),
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::groups::get_group_details(&mut conn, &group_uuid) {
        Ok(details) => HttpResponse::Ok().json(details),
        Err(e) => match e {
            Error::NotFound => errors::not_found_msg("Group not found"),
            _ => errors::internal("Failed to get group details"),
        },
    }
}

// ============================================================================
// Group CRUD Endpoints (Admin Only)
// ============================================================================

/// Get all groups with member counts
pub async fn get_all_groups(
    req: HttpRequest,
    pool: web::Data<Pool>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::groups::get_groups_with_member_counts(&mut conn) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(_) => errors::internal("Failed to get groups"),
    }
}

/// Get a single group by ID with members
pub async fn get_group(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let group_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::groups::get_group_with_members(&mut conn, group_id) {
        Ok(group) => HttpResponse::Ok().json(group),
        Err(e) => match e {
            Error::NotFound => errors::not_found_msg("Group not found"),
            _ => errors::internal("Failed to get group"),
        },
    }
}

/// Request body for creating a group
#[derive(Debug, Deserialize)]
pub struct CreateGroupRequest {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
}

/// Create a new group (admin only)
pub async fn create_group(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<CreateGroupRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let (_claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let created_by = Some(user_uuid);

    let new_group = NewGroup {
        name: body.name.clone(),
        description: body.description.clone(),
        color: body.color.clone(),
        created_by,
    };

    match repository::groups::create_group(&mut conn, new_group) {
        Ok(group) => HttpResponse::Created().json(group),
        Err(_) => errors::internal("Failed to create group"),
    }
}

/// Update an existing group (admin only)
pub async fn update_group(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<GroupUpdate>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let group_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::groups::update_group(&mut conn, group_id, body.into_inner()) {
        Ok(group) => HttpResponse::Ok().json(group),
        Err(e) => match e {
            Error::NotFound => errors::not_found_msg("Group not found"),
            _ => errors::internal("Failed to update group"),
        },
    }
}

/// Delete a group (admin only)
pub async fn delete_group(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let group_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::groups::delete_group(&mut conn, group_id) {
        Ok(0) => errors::not_found_msg("Group not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(_) => errors::internal("Failed to delete group"),
    }
}

/// Unmanage a group (remove external source to make it fully editable) - admin only
pub async fn unmanage_group(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let group_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Check if group exists and is externally synced
    let group = match repository::groups::get_group_by_id(&mut conn, group_id) {
        Ok(group) => group,
        Err(Error::NotFound) => return errors::not_found_msg("Group not found"),
        Err(_) => return errors::internal("Failed to get group"),
    };

    if group.external_source.is_none() {
        return errors::bad_request("Group is not externally managed: This group is already manually managed and doesn't need to be unmanaged.");
    }

    // Clear external management fields
    match repository::groups::unmanage_group(&mut conn, group_id) {
        Ok(updated_group) => HttpResponse::Ok().json(updated_group),
        Err(_) => errors::internal("Failed to unmanage group"),
    }
}

// ============================================================================
// Group Membership Endpoints
// ============================================================================

/// Request body for setting group members
#[derive(Debug, Deserialize)]
pub struct SetGroupMembersRequest {
    pub member_uuids: Vec<Uuid>,
}

/// Set members of a group (replaces existing members)
/// Note: Externally synced groups (e.g., from Microsoft) cannot have their membership modified
pub async fn set_group_members(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SetGroupMembersRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let (_claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let created_by = Some(user_uuid);
    let group_id = path.into_inner();

    // Check if group is externally synced - membership cannot be modified
    match repository::groups::get_group_by_id(&mut conn, group_id) {
        Ok(group) => {
            if group.external_source.is_some() {
                return errors::bad_request("Cannot modify membership: This group is synced from an external source. Membership is managed externally and updated during sync.");
            }
        }
        Err(Error::NotFound) => return errors::not_found_msg("Group not found"),
        Err(_) => return errors::internal("Failed to get group"),
    }

    match repository::groups::set_group_members(&mut conn, group_id, body.member_uuids.clone(), created_by) {
        Ok(_) => {
            // Return the updated group with members
            match repository::groups::get_group_with_members(&mut conn, group_id) {
                Ok(group) => HttpResponse::Ok().json(group),
                Err(_) => errors::internal("Failed to get updated group"),
            }
        }
        Err(_) => errors::internal("Failed to set group members"),
    }
}

/// Get groups for a specific user (self or admin)
pub async fn get_user_groups(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let user_uuid_str = path.into_inner();

    let user_uuid = match Uuid::parse_str(&user_uuid_str) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    // Allow self-access or admin access
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    if claims.sub != user_uuid_str && claims.role != "admin" {
        return errors::forbidden("Not authorized to access this resource");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::groups::get_groups_for_user(&mut conn, &user_uuid) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(_) => errors::internal("Failed to get user groups"),
    }
}

/// Request body for setting user's groups
#[derive(Debug, Deserialize)]
pub struct SetUserGroupsRequest {
    pub group_ids: Vec<i32>,
}

/// Set groups for a specific user (replaces existing memberships)
pub async fn set_user_groups(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<String>,
    body: web::Json<SetUserGroupsRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let (_claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let created_by = Some(user_uuid);
    let user_uuid = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    match repository::groups::set_user_groups(&mut conn, user_uuid, body.group_ids.clone(), created_by) {
        Ok(_) => {
            // Return the updated groups for this user
            match repository::groups::get_groups_for_user(&mut conn, &user_uuid) {
                Ok(groups) => HttpResponse::Ok().json(groups),
                Err(_) => errors::internal("Failed to get updated user groups"),
            }
        }
        Err(_) => errors::internal("Failed to set user groups"),
    }
}

// ============================================================================
// Group Includes (Composite Groups) Endpoints
// ============================================================================

/// Get included (child) groups for a parent group
pub async fn get_group_includes(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let group_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::groups::get_included_groups(&mut conn, group_id) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(e) => match e {
            Error::NotFound => errors::not_found_msg("Group not found"),
            _ => errors::internal("Failed to get group includes"),
        },
    }
}

/// Request body for setting group includes
#[derive(Debug, Deserialize)]
pub struct SetGroupIncludesRequest {
    pub child_group_ids: Vec<i32>,
}

/// Set included groups for a parent group (replaces existing includes)
pub async fn set_group_includes(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SetGroupIncludesRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let (_claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let created_by = Some(user_uuid);
    let group_id = path.into_inner();

    // Verify group exists
    let group = match repository::groups::get_group_by_id(&mut conn, group_id) {
        Ok(group) => group,
        Err(Error::NotFound) => return errors::not_found_msg("Group not found"),
        Err(_) => return errors::internal("Failed to get group"),
    };

    match repository::groups::set_group_includes(&mut conn, group_id, body.child_group_ids.clone(), created_by) {
        Ok(_) => {
            // Return updated group details
            match repository::groups::get_group_details(&mut conn, &group.uuid) {
                Ok(details) => HttpResponse::Ok().json(details),
                Err(_) => errors::internal("Failed to get updated group details"),
            }
        }
        Err(Error::DatabaseError(diesel::result::DatabaseErrorKind::CheckViolation, info)) => {
            HttpResponse::BadRequest().json(serde_json::json!({
                "error": "Validation error",
                "message": info.message().to_string()
            }))
        }
        Err(_) => errors::internal("Failed to set group includes"),
    }
}

// ============================================================================
// Device-Group Membership Endpoints
// ============================================================================

/// Request body for setting group devices
#[derive(Debug, Deserialize)]
pub struct SetGroupDevicesRequest {
    pub device_ids: Vec<i32>,
}

/// Set devices of a group (replaces existing manually-added devices, preserves synced ones)
/// Note: Externally synced groups (e.g., from Microsoft) cannot have their device membership modified
pub async fn set_group_devices(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SetGroupDevicesRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let (_claims, user_uuid, mut conn) = match helpers::auth_conn(&req, &pool) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let created_by = Some(user_uuid);
    let group_id = path.into_inner();

    // Check if group is externally synced - membership cannot be modified
    match repository::groups::get_group_by_id(&mut conn, group_id) {
        Ok(group) => {
            if group.external_source.is_some() {
                return errors::bad_request("Cannot modify membership: This group is synced from an external source. Device membership is managed externally and updated during sync.");
            }
        }
        Err(Error::NotFound) => return errors::not_found_msg("Group not found"),
        Err(_) => return errors::internal("Failed to get group"),
    }

    match repository::groups::set_group_devices(&mut conn, group_id, body.device_ids.clone(), created_by) {
        Ok(_) => {
            // Return the updated group details
            match repository::groups::get_group_by_id(&mut conn, group_id) {
                Ok(group) => {
                    match repository::groups::get_group_details(&mut conn, &group.uuid) {
                        Ok(details) => HttpResponse::Ok().json(details),
                        Err(_) => errors::internal("Failed to get updated group details"),
                    }
                }
                Err(_) => errors::internal("Failed to get group"),
            }
        }
        Err(_) => errors::internal("Failed to set group devices"),
    }
}
