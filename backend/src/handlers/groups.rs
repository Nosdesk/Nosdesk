use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::Deserialize;
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{NewGroup, GroupUpdate, Claims};
use crate::repository;
use crate::utils::rbac::require_admin;

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

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::groups::get_groups_with_member_counts(&mut conn) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(_) => HttpResponse::InternalServerError().json("Failed to get groups"),
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::groups::get_group_with_members(&mut conn, group_id) {
        Ok(group) => HttpResponse::Ok().json(group),
        Err(e) => match e {
            Error::NotFound => HttpResponse::NotFound().json("Group not found"),
            _ => HttpResponse::InternalServerError().json("Failed to get group"),
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

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let created_by = Uuid::parse_str(&claims.sub).ok();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let new_group = NewGroup {
        name: body.name.clone(),
        description: body.description.clone(),
        color: body.color.clone(),
        created_by,
    };

    match repository::groups::create_group(&mut conn, new_group) {
        Ok(group) => HttpResponse::Created().json(group),
        Err(_) => HttpResponse::InternalServerError().json("Failed to create group"),
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::groups::update_group(&mut conn, group_id, body.into_inner()) {
        Ok(group) => HttpResponse::Ok().json(group),
        Err(e) => match e {
            Error::NotFound => HttpResponse::NotFound().json("Group not found"),
            _ => HttpResponse::InternalServerError().json("Failed to update group"),
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
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::groups::delete_group(&mut conn, group_id) {
        Ok(0) => HttpResponse::NotFound().json("Group not found"),
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(_) => HttpResponse::InternalServerError().json("Failed to delete group"),
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
pub async fn set_group_members(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    body: web::Json<SetGroupMembersRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let created_by = Uuid::parse_str(&claims.sub).ok();
    let group_id = path.into_inner();

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::groups::set_group_members(&mut conn, group_id, body.member_uuids.clone(), created_by) {
        Ok(_) => {
            // Return the updated group with members
            match repository::groups::get_group_with_members(&mut conn, group_id) {
                Ok(group) => HttpResponse::Ok().json(group),
                Err(_) => HttpResponse::InternalServerError().json("Failed to get updated group"),
            }
        }
        Err(_) => HttpResponse::InternalServerError().json("Failed to set group members"),
    }
}

/// Get groups for a specific user
pub async fn get_user_groups(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let user_uuid = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::groups::get_groups_for_user(&mut conn, &user_uuid) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(_) => HttpResponse::InternalServerError().json("Failed to get user groups"),
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

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let created_by = Uuid::parse_str(&claims.sub).ok();
    let user_uuid = match Uuid::parse_str(&path.into_inner()) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid user UUID"),
    };

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::groups::set_user_groups(&mut conn, user_uuid, body.group_ids.clone(), created_by) {
        Ok(_) => {
            // Return the updated groups for this user
            match repository::groups::get_groups_for_user(&mut conn, &user_uuid) {
                Ok(groups) => HttpResponse::Ok().json(groups),
                Err(_) => HttpResponse::InternalServerError().json("Failed to get updated user groups"),
            }
        }
        Err(_) => HttpResponse::InternalServerError().json("Failed to set user groups"),
    }
}
