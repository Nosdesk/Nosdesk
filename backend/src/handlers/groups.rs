use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::Deserialize;
use uuid::Uuid;

use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::models::{Claims, GroupUpdate, NewGroup, WorkspaceRole};
use crate::repository;
use crate::utils::i18n;
use crate::utils::locale::request_locale;
use crate::utils::rbac::require_workspace_role;

// ============================================================================
// Group Detail Endpoint (All authenticated users)
// ============================================================================

/// Get group details by UUID (accessible to all authenticated users)
pub async fn get_group_details(
    req: HttpRequest,
    mut tc: TenantConn,
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

    match tc.run(|conn| repository::groups::get_group_details(conn, &group_uuid)) {
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
pub async fn get_all_groups(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    match tc.run(|conn| repository::groups::get_groups_with_member_counts(conn)) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(_) => errors::internal("Failed to get groups"),
    }
}

/// Get a single group by ID with members
pub async fn get_group(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let group_id = path.into_inner();

    match tc.run(|conn| repository::groups::get_group_with_members(conn, group_id)) {
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
    mut tc: TenantConn,
    body: web::Json<CreateGroupRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::internal("Invalid user UUID"),
    };

    let created_by = Some(user_uuid);

    let new_group = NewGroup {
        name: body.name.clone(),
        description: body.description.clone(),
        color: body.color.clone(),
        created_by,
    };

    match tc.run(|conn| repository::groups::create_group(conn, new_group)) {
        Ok(group) => HttpResponse::Created().json(group),
        Err(_) => errors::internal("Failed to create group"),
    }
}

/// Update an existing group (admin only)
pub async fn update_group(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<GroupUpdate>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let group_id = path.into_inner();
    let update = body.into_inner();

    match tc.run(|conn| repository::groups::update_group(conn, group_id, update)) {
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
    mut tc: TenantConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let group_id = path.into_inner();

    match tc.run(|conn| repository::groups::delete_group(conn, group_id)) {
        Ok(0) => errors::not_found_msg("Group not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(_) => errors::internal("Failed to delete group"),
    }
}

/// Outcome enum for unmanage_group so HttpResponse branches stay outside the txn closure.
enum UnmanageOutcome {
    Updated(crate::models::Group),
    NotFound,
    NotExternallyManaged,
}

/// Unmanage a group (remove external source to make it fully editable) - admin only
pub async fn unmanage_group(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let group_id = path.into_inner();

    let outcome = tc.run(|conn| {
        let group = match repository::groups::get_group_by_id(conn, group_id) {
            Ok(g) => g,
            Err(Error::NotFound) => return Ok(UnmanageOutcome::NotFound),
            Err(e) => return Err(e),
        };

        if group.external_source.is_none() {
            return Ok(UnmanageOutcome::NotExternallyManaged);
        }

        let updated = repository::groups::unmanage_group(conn, group_id)?;
        Ok::<_, diesel::result::Error>(UnmanageOutcome::Updated(updated))
    });

    match outcome {
        Ok(UnmanageOutcome::Updated(g)) => HttpResponse::Ok().json(g),
        Ok(UnmanageOutcome::NotFound) => errors::not_found_msg("Group not found"),
        Ok(UnmanageOutcome::NotExternallyManaged) => errors::bad_request(
            "Group is not externally managed: This group is already manually managed and doesn't need to be unmanaged.",
        ),
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

/// Outcome of set_group_members so HttpResponse branches stay outside the txn closure.
enum SetMembersOutcome {
    Ok(crate::models::GroupWithMembers),
    NotFound,
    ExternallyManaged,
}

/// Set members of a group (replaces existing members)
/// Note: Externally synced groups (e.g., from Microsoft) cannot have their membership modified
pub async fn set_group_members(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<SetGroupMembersRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::internal("Invalid user UUID"),
    };

    let created_by = Some(user_uuid);
    let group_id = path.into_inner();
    let member_uuids = body.member_uuids.clone();

    let outcome = tc.run(|conn| {
        let group = match repository::groups::get_group_by_id(conn, group_id) {
            Ok(g) => g,
            Err(Error::NotFound) => return Ok(SetMembersOutcome::NotFound),
            Err(e) => return Err(e),
        };

        if group.external_source.is_some() {
            return Ok(SetMembersOutcome::ExternallyManaged);
        }

        repository::groups::set_group_members(conn, group_id, member_uuids, created_by)?;
        let updated = repository::groups::get_group_with_members(conn, group_id)?;
        Ok::<_, diesel::result::Error>(SetMembersOutcome::Ok(updated))
    });

    match outcome {
        Ok(SetMembersOutcome::Ok(g)) => HttpResponse::Ok().json(g),
        Ok(SetMembersOutcome::NotFound) => errors::not_found_msg("Group not found"),
        Ok(SetMembersOutcome::ExternallyManaged) => errors::bad_request(
            "Cannot modify membership: This group is synced from an external source. Membership is managed externally and updated during sync.",
        ),
        Err(_) => errors::internal("Failed to set group members"),
    }
}

/// Get groups for a specific user (self or admin)
pub async fn get_user_groups(
    req: HttpRequest,
    mut tc: TenantConn,
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

    match tc.run(|conn| repository::groups::get_groups_for_user(conn, &user_uuid)) {
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
    mut tc: TenantConn,
    path: web::Path<String>,
    body: web::Json<SetUserGroupsRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let actor_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::internal("Invalid user UUID"),
    };

    let created_by = Some(actor_uuid);
    let user_uuid = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid user UUID"),
    };

    let group_ids = body.group_ids.clone();

    let result = tc.run(|conn| {
        repository::groups::set_user_groups(conn, user_uuid, group_ids, created_by)?;
        let updated = repository::groups::get_groups_for_user(conn, &user_uuid)?;
        Ok::<_, diesel::result::Error>(updated)
    });

    match result {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(_) => errors::internal("Failed to set user groups"),
    }
}

// ============================================================================
// Group Includes (Composite Groups) Endpoints
// ============================================================================

/// Get included (child) groups for a parent group
pub async fn get_group_includes(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let group_id = path.into_inner();

    match tc.run(|conn| repository::groups::get_included_groups(conn, group_id)) {
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

/// Outcome of set_group_includes so HttpResponse branches stay outside the txn closure.
enum SetIncludesOutcome {
    Ok(crate::models::GroupDetails),
    NotFound,
    CheckViolation(String),
}

/// Set included groups for a parent group (replaces existing includes)
pub async fn set_group_includes(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<SetGroupIncludesRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::internal("Invalid user UUID"),
    };

    let created_by = Some(user_uuid);
    let group_id = path.into_inner();
    let child_group_ids = body.child_group_ids.clone();

    let outcome = tc.run(|conn| {
        let group = match repository::groups::get_group_by_id(conn, group_id) {
            Ok(g) => g,
            Err(Error::NotFound) => return Ok(SetIncludesOutcome::NotFound),
            Err(e) => return Err(e),
        };

        match repository::groups::set_group_includes(conn, group_id, child_group_ids, created_by) {
            Ok(_) => {}
            Err(Error::DatabaseError(diesel::result::DatabaseErrorKind::CheckViolation, info)) => {
                return Ok(SetIncludesOutcome::CheckViolation(
                    info.message().to_string(),
                ));
            }
            Err(e) => return Err(e),
        }

        let details = repository::groups::get_group_details(conn, &group.uuid)?;
        Ok::<_, diesel::result::Error>(SetIncludesOutcome::Ok(details))
    });

    match outcome {
        Ok(SetIncludesOutcome::Ok(d)) => HttpResponse::Ok().json(d),
        Ok(SetIncludesOutcome::NotFound) => errors::not_found_msg("Group not found"),
        Ok(SetIncludesOutcome::CheckViolation(msg)) => {
            HttpResponse::BadRequest().json(serde_json::json!({
                "error": i18n::tr(&request_locale(&req), "backend-error-validation"),
                "code": "backend-error-validation",
                "message": msg,
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

/// Outcome of set_group_devices so HttpResponse branches stay outside the txn closure.
enum SetDevicesOutcome {
    Ok(crate::models::GroupDetails),
    NotFound,
    ExternallyManaged,
}

/// Set devices of a group (replaces existing manually-added devices, preserves synced ones)
/// Note: Externally synced groups (e.g., from Microsoft) cannot have their device membership modified
pub async fn set_group_devices(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<i32>,
    body: web::Json<SetGroupDevicesRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let user_uuid = match Uuid::parse_str(&claims.sub) {
        Ok(u) => u,
        Err(_) => return errors::internal("Invalid user UUID"),
    };

    let created_by = Some(user_uuid);
    let group_id = path.into_inner();
    let device_ids = body.device_ids.clone();

    let outcome = tc.run(|conn| {
        let group = match repository::groups::get_group_by_id(conn, group_id) {
            Ok(g) => g,
            Err(Error::NotFound) => return Ok(SetDevicesOutcome::NotFound),
            Err(e) => return Err(e),
        };

        if group.external_source.is_some() {
            return Ok(SetDevicesOutcome::ExternallyManaged);
        }

        repository::groups::set_group_devices(conn, group_id, device_ids, created_by)?;
        let details = repository::groups::get_group_details(conn, &group.uuid)?;
        Ok::<_, diesel::result::Error>(SetDevicesOutcome::Ok(details))
    });

    match outcome {
        Ok(SetDevicesOutcome::Ok(d)) => HttpResponse::Ok().json(d),
        Ok(SetDevicesOutcome::NotFound) => errors::not_found_msg("Group not found"),
        Ok(SetDevicesOutcome::ExternallyManaged) => errors::bad_request(
            "Cannot modify membership: This group is synced from an external source. Asset membership is managed externally and updated during sync.",
        ),
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
    use actix_web::{http::StatusCode, web, App, HttpMessage};

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
