use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::Deserialize;
use uuid::Uuid;

use crate::db::Pool;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::handlers::helpers::{actor_for as helper_actor_for, with_actor};
use crate::models::{Claims, GroupUpdate, NewGroup};
use crate::repository;
use crate::sync::actor::ActorContext;
use crate::utils::i18n;
use crate::utils::locale::request_locale;
use crate::utils::rbac::require_admin;

#[inline]
fn actor_for(req: &HttpRequest) -> ActorContext {
    helper_actor_for(req, "handler:groups")
}

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
pub async fn get_all_groups(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
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

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::groups::create_group(conn, new_group)
    }) {
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

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::groups::update_group(conn, group_id, body.into_inner())
    }) {
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

    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::groups::delete_group(conn, group_id)
    }) {
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
    let actor = actor_for(&req);
    match with_actor(&mut conn, &actor, |conn| {
        repository::groups::unmanage_group(conn, group_id)
    }) {
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

    let actor = actor_for(&req);
    let member_uuids = body.member_uuids.clone();
    match with_actor(&mut conn, &actor, |conn| {
        repository::groups::set_group_members(conn, group_id, member_uuids, created_by)
    }) {
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

    let actor = actor_for(&req);
    let group_ids = body.group_ids.clone();
    match with_actor(&mut conn, &actor, |conn| {
        repository::groups::set_user_groups(conn, user_uuid, group_ids, created_by)
    }) {
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

    let actor = actor_for(&req);
    let child_group_ids = body.child_group_ids.clone();
    match with_actor(&mut conn, &actor, |conn| {
        repository::groups::set_group_includes(conn, group_id, child_group_ids, created_by)
    }) {
        Ok(_) => {
            // Return updated group details
            match repository::groups::get_group_details(&mut conn, &group.uuid) {
                Ok(details) => HttpResponse::Ok().json(details),
                Err(_) => errors::internal("Failed to get updated group details"),
            }
        }
        Err(Error::DatabaseError(diesel::result::DatabaseErrorKind::CheckViolation, info)) => {
            HttpResponse::BadRequest().json(serde_json::json!({
                "error": i18n::tr(&request_locale(&req), "backend-error-validation"),
                "code": "backend-error-validation",
                "message": info.message().to_string()
            }))
        }
        Err(_) => errors::internal("Failed to set group includes"),
    }
}

// ============================================================================
// Asset-Group Membership Endpoints
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
                return errors::bad_request("Cannot modify membership: This group is synced from an external source. Asset membership is managed externally and updated during sync.");
            }
        }
        Err(Error::NotFound) => return errors::not_found_msg("Group not found"),
        Err(_) => return errors::internal("Failed to get group"),
    }

    let actor = actor_for(&req);
    let device_ids = body.device_ids.clone();
    match with_actor(&mut conn, &actor, |conn| {
        repository::groups::set_group_devices(conn, group_id, device_ids, created_by)
    }) {
        Ok(_) => {
            // Return the updated group details
            match repository::groups::get_group_by_id(&mut conn, group_id) {
                Ok(group) => match repository::groups::get_group_details(&mut conn, &group.uuid) {
                    Ok(details) => HttpResponse::Ok().json(details),
                    Err(_) => errors::internal("Failed to get updated group details"),
                },
                Err(_) => errors::internal("Failed to get group"),
            }
        }
        Err(_) => errors::internal("Failed to set group devices"),
    }
}

#[cfg(test)]
mod tests {
    //! Permission-boundary tests. Group membership drives visibility
    //! of restricted categories + documentation, so unauthorised
    //! changes to groups would silently widen access elsewhere.
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/admin/groups", web::get().to(get_all_groups))
            .route("/admin/groups/{id}", web::delete().to(delete_group))
    }

    #[actix_web::test]
    async fn list_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/groups")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn list_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/groups")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn delete_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, UserRole::User);
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::delete()
            .uri("/admin/groups/1")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
