use actix_web::{web, HttpResponse, Responder};
use actix_web_httpauth::extractors::bearer::BearerAuth;
use bcrypt::DEFAULT_COST;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use futures::{StreamExt, TryStreamExt};
use actix_multipart::Multipart;
use std::fs;
use std::path::Path;
use std::collections::HashSet;
use chrono::NaiveDateTime;

use crate::models::{NewUser, UserResponse, UserUpdate, UserUpdateWithPassword, UserProfileUpdate};
use crate::repository;
use crate::repository::user_emails as user_emails_repo;
use crate::handlers::auth::validate_token_internal;
use crate::utils;

// Pagination query parameters
#[derive(Deserialize)]
pub struct PaginationParams {
    page: Option<i64>,
    #[serde(rename = "pageSize")]
    page_size: Option<i64>,
    #[serde(rename = "sortField")]
    sort_field: Option<String>,
    #[serde(rename = "sortDirection")]
    sort_direction: Option<String>,
    search: Option<String>,
    role: Option<String>,
}

// Paginated response
#[derive(Serialize)]
pub struct PaginatedResponse<T> {
    data: Vec<T>,
    total: i64,
    page: i64,
    #[serde(rename = "pageSize")]
    page_size: i64,
    #[serde(rename = "totalPages")]
    total_pages: i64,
}

// User handlers
pub async fn get_users(
    pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    
    let mut conn = match pool.get() {
        Ok(conn) => {
            conn
        },
        Err(e) => {
            eprintln!("Database connection error: {:?}", e);
            return HttpResponse::InternalServerError().json("Database connection error");
        },
    };

    match repository::get_users(&mut conn) {
        Ok(users) => {
            let user_responses: Vec<UserResponse> = users.into_iter().map(UserResponse::from).collect();
            HttpResponse::Ok().json(user_responses)
        },
        Err(e) => {
            eprintln!("Error fetching users: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to fetch users")
        },
    }
}

// Get paginated users
pub async fn get_paginated_users(
    pool: web::Data<crate::db::Pool>,
    query: web::Query<PaginationParams>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract and validate pagination parameters
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(25).clamp(1, 100);

    match repository::get_paginated_users(
        &mut conn,
        page,
        page_size,
        query.sort_field.clone(),
        query.sort_direction.clone(),
        query.search.clone(),
        query.role.clone(),
    ) {
        Ok((users, total)) => {
            // Calculate total pages
            let total_pages = (total as f64 / page_size as f64).ceil() as i64;
            
            // Convert users to UserResponse
            let user_responses: Vec<UserResponse> = users.into_iter().map(UserResponse::from).collect();
            
            // Create paginated response
            let response = PaginatedResponse {
                data: user_responses,
                total,
                page,
                page_size,
                total_pages,
            };
            
            HttpResponse::Ok().json(response)
        },
        Err(e) => {
            eprintln!("Error fetching paginated users: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to get paginated users")
        },
    }
}

#[allow(dead_code)]
pub async fn get_user_by_id(
    id: web::Path<i32>,
    pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    let user_id = id.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_user_by_id(user_id, &mut conn) {
        Ok(user) => HttpResponse::Ok().json(UserResponse::from(user)),
        Err(_) => HttpResponse::NotFound().json("User not found"),
    }
}

pub async fn get_user_by_uuid(
    uuid_path: web::Path<String>,
    pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    let uuid_str = uuid_path.into_inner();
    
    // Parse the UUID string into a proper UUID type
    let user_uuid_parsed = match utils::parse_uuid(&uuid_str) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid UUID format"),
    };
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => HttpResponse::Ok().json(UserResponse::from(user)),
        Err(_) => HttpResponse::NotFound().json("User not found"),
    }
}

// Batch users request
#[derive(Deserialize)]
pub struct BatchUsersRequest {
    uuids: Vec<String>,
}

pub async fn get_users_batch(
    batch_request: web::Json<BatchUsersRequest>,
    pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Validate UUIDs and remove duplicates
    let mut valid_uuids = HashSet::new();
    for uuid_str in &batch_request.uuids {
        if let Ok(uuid) = Uuid::parse_str(uuid_str) {
            valid_uuids.insert(uuid);
        }
    }

    if valid_uuids.is_empty() {
        return HttpResponse::BadRequest().json("No valid UUIDs provided");
    }

    // Convert to Vec for the repository function
    let uuids_vec: Vec<Uuid> = valid_uuids.into_iter().collect();

    match repository::get_users_by_uuids(&uuids_vec, &mut conn) {
        Ok(users) => {
            let user_responses: Vec<UserResponse> = users.into_iter().map(UserResponse::from).collect();
            HttpResponse::Ok().json(user_responses)
        },
        Err(e) => {
            eprintln!("Error fetching users batch: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to get users")
        },
    }
}

pub async fn create_user(
    db_pool: web::Data<crate::db::Pool>,
    user_data: web::Json<NewUser>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Comprehensive input validation using our validation utilities
    let mut validation_errors = Vec::new();

    // Validate name
    let trimmed_name = user_data.name.trim();
    if trimmed_name.is_empty() {
        validation_errors.push("name: Name is required".to_string());
    } else if trimmed_name.len() > 255 {
        validation_errors.push("name: Name must be less than 255 characters".to_string());
    }

    // Validate email
    if user_data.email.is_empty() {
        validation_errors.push("email: Email is required".to_string());
    } else if !user_data.email.contains('@') {
        validation_errors.push("email: Invalid email format".to_string());
    }

    // Validate role
    let _role_enum = match user_data.role {
        crate::models::UserRole::Admin => "admin",
        crate::models::UserRole::Technician => "technician", 
        crate::models::UserRole::User => "user",
    };
    // Role is already validated by the enum type

    // Validate optional fields
    if let Some(ref pronouns) = user_data.pronouns {
        if pronouns.len() > 50 {
            validation_errors.push("pronouns: Pronouns must be less than 50 characters".to_string());
        }
    }

    if let Some(ref avatar_url) = user_data.avatar_url {
        if avatar_url.len() > 500 {
            validation_errors.push("avatar_url: URL must be less than 500 characters".to_string());
        }
    }

    if let Some(ref banner_url) = user_data.banner_url {
        if banner_url.len() > 500 {
            validation_errors.push("banner_url: URL must be less than 500 characters".to_string());
        }
    }

    if let Some(ref avatar_thumb) = user_data.avatar_thumb {
        if avatar_thumb.len() > 500 {
            validation_errors.push("avatar_thumb: URL must be less than 500 characters".to_string());
        }
    }

    // Check if user with this email already exists
    if let Ok(_) = repository::get_user_by_email(&user_data.email, &mut conn) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "User with this email already exists"
        }));
    }

    // Generate UUID if provided
    let user_uuid = user_data.uuid;

    // Create new user with normalized data using builder
    let (normalized_name, normalized_email) = utils::normalization::normalize_user_data(&user_data.name, &user_data.email);
    let new_user = utils::NewUserBuilder::new(normalized_name, normalized_email, user_data.role)
        .with_uuid(user_uuid)
        .with_pronouns(user_data.pronouns.as_ref().map(|p| p.trim().to_string()))
        .with_avatar(
            user_data.avatar_url.as_ref().map(|u| u.trim().to_string()),
            user_data.avatar_thumb.as_ref().map(|u| u.trim().to_string())
        )
        .with_banner(user_data.banner_url.as_ref().map(|u| u.trim().to_string()))
        .with_microsoft_uuid(user_data.microsoft_uuid)
        .build();

    match repository::create_user(new_user, &mut conn) {
        Ok(user) => {
            // Create default password hash for this user
            use bcrypt::hash;
            use crate::models::NewUserAuthIdentity;
            
            let password_hash = match hash("changeme", DEFAULT_COST) {
                Ok(hash) => hash,
                Err(e) => {
                    eprintln!("Error hashing password: {:?}", e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error setting default password"
                    }));
                }
            };
            
            println!("Created user with ID: {}", user.id);
            
            // Create local auth identity with default password
            let new_identity = NewUserAuthIdentity {
                user_id: user.id,
                provider_type: "local".to_string(),
                external_id: utils::uuid_to_string(&user.uuid),
                email: Some(user.email.clone()),
                metadata: None,
                password_hash: Some(password_hash),
            };
            
            match repository::user_auth_identities::create_identity(new_identity, &mut conn) {
                Ok(_) => {
                    println!("✅ New user created successfully: {} (default password: changeme)", user.email);
                    HttpResponse::Created().json(UserResponse::from(user))
                },
                Err(e) => {
                    eprintln!("Error creating auth identity: {:?}", e);
                    // If identity creation fails, still return the user
                    HttpResponse::Created().json(UserResponse::from(user))
                }
            }
        },
        Err(e) => {
            eprintln!("Error creating user: {:?}", e);
            
            // Provide more specific error messages for common issues
            let error_message = if format!("{:?}", e).contains("duplicate") || format!("{:?}", e).contains("unique") {
                if format!("{:?}", e).contains("email") {
                    "Email address already exists in the system"
                } else if format!("{:?}", e).contains("uuid") {
                    "UUID already exists in the system"
                } else {
                    "Duplicate entry detected"
                }
            } else {
                "Error creating user"
            };
            
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": error_message
            }))
        }
    }
}

#[allow(dead_code)]
pub async fn update_user(
    path: web::Path<String>,
    user_data: web::Json<UserUpdate>,
    _auth: BearerAuth,
    db_pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    let user_id_str = path.into_inner();
    let user_id = match user_id_str.parse::<i32>() {
        Ok(id) => id,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid user ID"
        })),
    };

    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };

    // Check if user exists
    if let Err(_) = repository::get_user_by_id(user_id, &mut conn) {
        return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        }));
    }

    // Check if email is being updated and if it's already in use
    if let Some(email) = &user_data.email {
        if let Ok(existing_user) = repository::get_user_by_email(email, &mut conn) {
            if existing_user.id != user_id {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "Email already exists"
                }));
            }
        }
    }

    // Update user
    match repository::update_user(user_id, user_data.into_inner(), &mut conn) {
        Ok(updated_user) => {
            HttpResponse::Ok().json(json!({
                "status": "success",
                "user": UserResponse::from(updated_user)
            }))
        },
        Err(e) => {
            eprintln!("Error updating user: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error", 
                "message": "Error updating user"
            }))
        }
    }
}

pub async fn delete_user(
    uuid: web::Path<String>,
    pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
) -> impl Responder {
    let user_uuid = uuid.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Validate token and get authenticated user
    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json("Invalid or expired token"),
    };

    // Only admins can delete users
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can delete users"
        }));
    }

    // Parse the target user UUID
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid UUID format"),
    };

    // Get the target user
    let target_user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json("User not found"),
    };

    // Prevent self-deletion
    if claims.sub == user_uuid {
        return HttpResponse::BadRequest().json(json!({
            "error": "Cannot delete self",
            "message": "You cannot delete your own account while logged in"
        }));
    }

    // Prevent deletion of admin users (safety measure)
    if target_user.role == crate::models::UserRole::Admin {
        return HttpResponse::BadRequest().json(json!({
            "error": "Cannot delete admin",
            "message": "Administrator accounts cannot be deleted for security reasons"
        }));
    }

    // Delete the user
    match repository::delete_user(target_user.id, &mut conn) {
        Ok(count) if count > 0 => HttpResponse::NoContent().finish(),
        Ok(_) => HttpResponse::NotFound().json("User not found"),
        Err(_) => HttpResponse::InternalServerError().json("Failed to delete user"),
    }
}

// Get user's authentication identities
pub async fn get_user_auth_identities(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
) -> impl Responder {
    // Get database connection
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate token and get user UUID
    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(e) => {
            eprintln!("Token validation error: {:?}", e);
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid or expired token"
            }));
        },
    };

    // Get the user ID
    let user_uuid_parsed = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid UUID in token"
        })),
    };

    let user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            eprintln!("Error getting user by UUID: {:?}", e);
            return HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": "User not found"
            }));
        }
    };

    // Get auth identities for the user
    match repository::user_auth_identities::get_user_identities_display(user.id, &mut conn) {
        Ok(identities) => HttpResponse::Ok().json(identities),
        Err(e) => {
            eprintln!("Error fetching auth identities: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to retrieve auth identities"
            }))
        }
    }
}

// Get user's authentication identities by UUID
pub async fn get_user_auth_identities_by_uuid(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    path: web::Path<String>, // User UUID
) -> impl Responder {
    // Get database connection
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Database connection error: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Could not get database connection"
            }));
        }
    };

    let user_uuid = path.into_inner();

    // Validate token and get user UUID from token
    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(e) => {
            eprintln!("Token validation error: {:?}", e);
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Invalid or expired token"
            }));
        }
    };

    // Ensure the user is authorized (either accessing their own identities or is an admin)
    if claims.sub != user_uuid && claims.role != "admin" {
        eprintln!("Authorization failed: user {} tried to access identities of {}", claims.sub, user_uuid);
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Not authorized to access this resource"
        }));
    }

    // Get auth identities for the user by UUID
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid UUID format"
        })),
    };

    match repository::user_auth_identities::get_user_identities_display_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(identities) => HttpResponse::Ok().json(identities),
        Err(e) => {
            eprintln!("Error fetching auth identities for UUID {}: {:?}", user_uuid, e);
            return HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": "User not found or no auth identities"
            }));
        }
    }
}

// Delete a user authentication identity
pub async fn delete_user_auth_identity(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    path: web::Path<i32>, // Auth identity ID
) -> impl Responder {
    let identity_id = path.into_inner();
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate the token
    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
        })),
    };

    // Get the user ID
    let user_uuid_parsed = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid UUID in token"
        })),
    };

    let user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(e) => {
            eprintln!("Error getting user by UUID: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to get user data"
            }));
        }
    };

    // Ensure the user has at least one other auth method before deleting
    // (to prevent locking themselves out)
    let identities = match repository::user_auth_identities::get_user_identities(user.id, &mut conn) {
        Ok(identities) => identities,
        Err(e) => {
            eprintln!("Error getting user auth identities: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to get authentication identities"
            }));
        }
    };

    if identities.len() <= 1 {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Cannot delete the only authentication method. Add another method first."
        }));
    }

    // Delete the identity
    match repository::user_auth_identities::delete_identity(identity_id, user.id, &mut conn) {
        Ok(count) => {
            if count == 0 {
                HttpResponse::NotFound().json(json!({
                    "status": "error",
                    "message": "Authentication identity not found or doesn't belong to you"
                }))
            } else {
                HttpResponse::Ok().json(json!({
                    "status": "success",
                    "message": "Authentication identity deleted successfully"
                }))
            }
        },
        Err(e) => {
            eprintln!("Error deleting user auth identity: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to delete authentication identity"
            }))
        }
    }
}

// Delete a user authentication identity by UUID
pub async fn delete_user_auth_identity_by_uuid(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    path: web::Path<(String, i32)>, // (User UUID, Auth identity ID)
) -> impl Responder {
    let (user_uuid, identity_id) = path.into_inner();
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate the token
    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
        })),
    };

    // Ensure the user is authorized (either accessing their own identities or is an admin)
    if claims.sub != user_uuid && claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Not authorized to access this resource"
        }));
    }

    // Ensure the user has at least one other auth method before deleting
    // (to prevent locking themselves out)
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid UUID format"
        })),
    };

    let identities = match repository::user_auth_identities::get_user_identities_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(identities) => identities,
        Err(e) => {
            eprintln!("Error getting user auth identities: {:?}", e);
            return HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": "User not found"
            }));
        }
    };

    if identities.len() <= 1 {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Cannot delete the only authentication method. Add another method first."
        }));
    }

    // Delete the identity
    match repository::user_auth_identities::delete_identity_by_uuid(identity_id, &user_uuid_parsed, &mut conn) {
        Ok(count) => {
            if count == 0 {
                HttpResponse::NotFound().json(json!({
                    "status": "error",
                    "message": "Authentication identity not found or doesn't belong to this user"
                }))
            } else {
                HttpResponse::Ok().json(json!({
                    "status": "success",
                    "message": "Authentication identity deleted successfully"
                }))
            }
        },
        Err(e) => {
            eprintln!("Error deleting user auth identity: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to delete authentication identity"
            }))
        }
    }
}

#[allow(dead_code)]
pub async fn update_user_profile(
    _path: web::Path<String>,
    profile_data: web::Json<UserProfileUpdate>,
    _auth: BearerAuth,
    db_pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    let user_id = profile_data.id;
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    if let Err(_) = repository::get_user_by_id(user_id, &mut conn) {
        return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        }));
    }

    // Check if email is being updated and if it's already in use
    if let Some(email) = &profile_data.email {
        if let Ok(existing_user) = repository::get_user_by_email(email, &mut conn) {
            if existing_user.id != user_id {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "Email already in use by another user"
                }));
            }
        }
    }

    // Create password hash if password is provided
    if let Some(password) = &profile_data.password {
        use bcrypt::hash;
        use crate::models::NewUserAuthIdentity;
        
        // Hash the new password
        let password_hash = match hash(password, DEFAULT_COST) {
            Ok(hash) => hash,
            Err(_) => return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error hashing password"
            })),
        };
        
        // Find existing local auth identity
        let auth_identities = match repository::user_auth_identities::get_user_identities(user_id, &mut conn) {
            Ok(identities) => identities,
            Err(e) => {
                eprintln!("Error fetching auth identities: {:?}", e);
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Error processing user identities"
                }));
            }
        };
        
        // Find local identity
        let local_identity = auth_identities.iter().find(|identity| identity.provider_type == "local");
        
        match local_identity {
            Some(identity) => {
                // Update existing local identity
                // Delete the old identity 
                match diesel::delete(
                    crate::schema::user_auth_identities::table.filter(crate::schema::user_auth_identities::id.eq(identity.id))
                ).execute(&mut conn) {
                    Ok(_) => {},
                    Err(e) => {
                        eprintln!("Error deleting auth identity: {:?}", e);
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Error updating password"
                        }));
                    }
                }
                
                // Create new identity with updated password
                let new_auth_identity = NewUserAuthIdentity {
                    user_id,
                    provider_type: identity.provider_type.clone(),
                    external_id: identity.external_id.clone(),
                    email: identity.email.clone(),
                    metadata: identity.metadata.clone(),
                    password_hash: Some(password_hash),
                };
                
                if let Err(e) = repository::user_auth_identities::create_identity(new_auth_identity, &mut conn) {
                    eprintln!("Error creating updated auth identity: {:?}", e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error updating password"
                    }));
                }
            },
            None => {
                // Create new local identity if none exists
                let new_auth_identity = NewUserAuthIdentity {
                    user_id,
                    provider_type: "local".to_string(),
                    external_id: Uuid::new_v4().to_string(), // Generate a new provider user ID
                    email: profile_data.email.clone(),
                    metadata: None,
                    password_hash: Some(password_hash),
                };
                
                if let Err(e) = repository::user_auth_identities::create_identity(new_auth_identity, &mut conn) {
                    eprintln!("Error creating auth identity: {:?}", e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error setting password"
                    }));
                }
            }
        }
    }

    // Update user
    let user_update = UserUpdate {
        name: profile_data.name.clone(),
        email: profile_data.email.clone(),
        role: profile_data.role.as_ref().and_then(|r| utils::parse_role(r).ok()),
        pronouns: profile_data.pronouns.clone(),
        avatar_url: profile_data.avatar_url.clone(),
        banner_url: profile_data.banner_url.clone(),
        avatar_thumb: profile_data.avatar_thumb.clone(),
        microsoft_uuid: None, // Don't update Microsoft UUID in regular user updates
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match repository::update_user(user_id, user_update, &mut conn) {
        Ok(updated_user) => HttpResponse::Ok().json(UserResponse::from(updated_user)),
        Err(e) => {
            eprintln!("Error updating user: {:?}", e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error updating user"
            }))
        },
    }
}

// Upload user profile images (avatar or banner)
pub async fn upload_user_image(
    uuid: web::Path<String>,
    mut payload: Multipart,
    pool: web::Data<crate::db::Pool>,
    type_query: web::Query<UserImageTypeQuery>,
) -> impl Responder {
    let user_uuid = uuid.into_inner();
    let image_type = &type_query.type_; // "avatar" or "banner"
    
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("Database connection error: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Database connection error"
            }));
        }
    };
    
    // Validate that the user exists
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid UUID format"),
    };
    
    let user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => {
            return HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": "User not found"
            }));
        }
    };
    
    // Determine the upload directory based on image type
    let storage_path = match image_type.as_str() {
        "avatar" => "users/avatars",
        "banner" => "users/banners",
        _ => {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid image type. Must be 'avatar' or 'banner'"
            }));
        }
    };

    // Ensure the directory exists using storage abstraction
    let full_storage_path = format!("{}/{}", storage_path, user_uuid);
    
    // Process the uploaded image
    while let Ok(Some(mut field)) = payload.try_next().await {
        // Get content type
        let content_type = field.content_type().map(|ct| ct.to_string()).unwrap_or_else(|| "application/octet-stream".to_string());
        
        // Validate content type (only allow images)
        if !content_type.starts_with("image/") {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Only image files are allowed"
            }));
        }
        
        // Extract file extension from content type
        let file_ext = match content_type.as_str() {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/webp" => "webp",
            _ => {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "Unsupported image format. Allowed: JPEG, PNG, GIF, WEBP"
                }));
            }
        };
        
        // Clean up old files for this user and image type before saving new one
        cleanup_old_user_images(&storage_path, &user_uuid, image_type).await
            .map_err(|e| {
                eprintln!("Warning: Failed to cleanup old images: {}", e);
                // Continue even if cleanup fails
            }).ok();
        
        let filename = format!("{}_{}.{}", user_uuid, image_type, file_ext);
        let file_path = format!("{}/{}", storage_path, filename);
        
        // Read file data
        let mut file_data = Vec::new();
        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("Error reading chunk: {:?}", e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error reading uploaded file"
                    }));
                }
            };
            file_data.extend_from_slice(&data);
        }
        
        // Process the image based on type
        let (final_url, thumbnail_url) = if image_type == "avatar" {
            // For avatars, process and resize to WebP format with fixed dimensions (200x200 max)
            match crate::utils::image::process_avatar_image(&file_data, &user_uuid, 200).await {
                Ok(Some(avatar_url)) => {
                    println!("Successfully processed avatar for user {}: {}", user_uuid, avatar_url);
                    
                    // Generate thumbnail from the processed avatar
                    let thumb_url = match crate::utils::image::generate_user_avatar_thumbnail(&avatar_url, &user_uuid).await {
                        Ok(Some(thumb_url)) => {
                            println!("Successfully generated thumbnail for user {}: {}", user_uuid, thumb_url);
                            Some(thumb_url)
                        },
                        Ok(None) => {
                            println!("Failed to generate thumbnail for user {}", user_uuid);
                            None
                        },
                        Err(e) => {
                            eprintln!("Error generating thumbnail: {}", e);
                            None
                        }
                    };
                    
                    (avatar_url, thumb_url)
                },
                Ok(None) => {
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Failed to process avatar image"
                    }));
                },
                Err(e) => {
                    eprintln!("Error processing avatar: {}", e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": format!("Failed to process avatar image: {}", e)
                    }));
                }
            }
        } else {
            // For banners, just save the original image
            let banner_url = format!("/uploads/{}", file_path);
            (banner_url, None)
        };
        
        // Update the user record with the new image URL
        
        let user_update = UserUpdate {
            name: None,
            email: None,
            role: None,
            pronouns: None,
            avatar_url: if image_type == "avatar" { Some(final_url.clone()) } else { None },
            banner_url: if image_type == "banner" { Some(final_url.clone()) } else { None },
            avatar_thumb: thumbnail_url,
            microsoft_uuid: None, // Don't update Microsoft UUID in regular user updates
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };
        
        match repository::update_user(user.id, user_update, &mut conn) {
            Ok(updated_user) => {
                return HttpResponse::Ok().json(json!({
                    "status": "success",
                    "message": format!("User {} updated successfully", image_type),
                    "url": final_url,
                    "user": UserResponse::from(updated_user)
                }));
            },
            Err(e) => {
                eprintln!("Error updating user: {:?}", e);
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Error updating user record"
                }));
            }
        }
    }
    
    // If we get here, no file was provided
    HttpResponse::BadRequest().json(json!({
        "status": "error",
        "message": "No image file provided"
    }))
}

#[derive(serde::Deserialize)]
pub struct UserImageTypeQuery {
    pub type_: String, // "avatar" or "banner"
}

/// Clean up old user images for a specific type (avatar or banner)
async fn cleanup_old_user_images(
    image_dir: &str,
    user_uuid: &str,
    image_type: &str,
) -> Result<(), String> {
    use tokio::fs;
    
    // Read the directory
    let mut dir = match fs::read_dir(image_dir).await {
        Ok(dir) => dir,
        Err(_) => return Ok(()) // Directory doesn't exist, nothing to clean
    };

    // Look for files matching the pattern: {user_uuid}_{image_type}.{ext}
    let pattern_prefix = format!("{}_{}", user_uuid, image_type);
    
    while let Ok(Some(entry)) = dir.next_entry().await {
        if let Some(filename) = entry.file_name().to_str() {
            // Check if this file matches our pattern (user_uuid_type.ext)
            if filename.starts_with(&pattern_prefix) && filename.contains('.') {
                let file_path = entry.path();
                println!("Cleaning up old image file: {:?}", file_path);
                
                if let Err(e) = fs::remove_file(&file_path).await {
                    eprintln!("Warning: Failed to remove old image file {:?}: {}", file_path, e);
                    // Continue with cleanup even if one file fails
                }
            }
        }
    }
    
    Ok(())
}

/// Clean up all stale user images (admin endpoint)
pub async fn cleanup_stale_images(
    _auth: BearerAuth,
    db_pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Validate token and check admin permissions
    let claims = match validate_token_internal(&_auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
        })),
    };

    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Only administrators can cleanup stale images"
        }));
    }

    // Get all users to know which files should exist
    let users = match repository::get_users(&mut conn) {
        Ok(users) => users,
        Err(e) => {
            eprintln!("Error fetching users: {:?}", e);
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to fetch users"
            }));
        }
    };

    let mut cleanup_stats = CleanupStats {
        avatars_removed: 0,
        banners_removed: 0,
        thumbnails_removed: 0,
        total_files_checked: 0,
        errors: Vec::new(),
    };

    // Clean up avatar directory
    if let Err(e) = cleanup_directory_stale_files(
        "uploads/users/avatars", 
        &users, 
        &["avatar", "48x48", "120x120", "default"], 
        &mut cleanup_stats
    ).await {
        cleanup_stats.errors.push(format!("Avatar cleanup error: {}", e));
    }

    // Clean up banner directory  
    if let Err(e) = cleanup_directory_stale_files(
        "uploads/users/banners", 
        &users, 
        &["banner"], 
        &mut cleanup_stats
    ).await {
        cleanup_stats.errors.push(format!("Banner cleanup error: {}", e));
    }

    // Clean up thumbnail directory
    if let Err(e) = cleanup_directory_stale_files(
        "uploads/users/thumbs", 
        &users, 
        &["thumb"], 
        &mut cleanup_stats
    ).await {
        cleanup_stats.errors.push(format!("Thumbnail cleanup error: {}", e));
    }

    HttpResponse::Ok().json(json!({
        "status": "success",
        "message": "Stale image cleanup completed",
        "stats": {
            "avatars_removed": cleanup_stats.avatars_removed,
            "banners_removed": cleanup_stats.banners_removed,
            "thumbnails_removed": cleanup_stats.thumbnails_removed,
            "total_files_checked": cleanup_stats.total_files_checked,
            "errors": cleanup_stats.errors
        }
    }))
}

#[derive(Debug)]
struct CleanupStats {
    avatars_removed: usize,
    banners_removed: usize,
    thumbnails_removed: usize,
    total_files_checked: usize,
    errors: Vec<String>,
}

/// Clean up stale files in a specific directory
async fn cleanup_directory_stale_files(
    dir_path: &str,
    users: &[crate::models::User],
    valid_suffixes: &[&str],
    stats: &mut CleanupStats,
) -> Result<(), String> {
    use tokio::fs;
    use std::collections::HashSet;
    
    // Create a set of valid file prefixes (user UUIDs)
    let valid_uuids: HashSet<String> = users.iter().map(|u| utils::uuid_to_string(&u.uuid)).collect();
    
    // Read the directory
    let mut dir = match fs::read_dir(dir_path).await {
        Ok(dir) => dir,
        Err(_) => return Ok(()) // Directory doesn't exist, nothing to clean
    };

    while let Ok(Some(entry)) = dir.next_entry().await {
        if let Some(filename) = entry.file_name().to_str() {
            stats.total_files_checked += 1;
            
            // Check if this file should be kept
            let should_keep = should_keep_file(filename, &valid_uuids, valid_suffixes);
            
            if !should_keep {
                let file_path = entry.path();
                println!("Removing stale image file: {:?}", file_path);
                
                match fs::remove_file(&file_path).await {
                    Ok(_) => {
                        if dir_path.contains("avatars") {
                            stats.avatars_removed += 1;
                        } else if dir_path.contains("banners") {
                            stats.banners_removed += 1;
                        } else if dir_path.contains("thumbs") {
                            stats.thumbnails_removed += 1;
                        }
                    },
                    Err(e) => {
                        let error_msg = format!("Failed to remove {:?}: {}", file_path, e);
                        eprintln!("Warning: {}", error_msg);
                        stats.errors.push(error_msg);
                    }
                }
            }
        }
    }
    
    Ok(())
}

/// Determine if a file should be kept based on naming patterns
fn should_keep_file(filename: &str, valid_uuids: &HashSet<String>, valid_suffixes: &[&str]) -> bool {
    // Skip hidden files like .DS_Store
    if filename.starts_with('.') {
        return true;
    }
    
    // Skip non-image files
    if !filename.contains('.') {
        return true;
    }
    
    // Extract the base name without extension
    let base_name = filename.split('.').next().unwrap_or("");
    
    // Check for new format: {uuid}_{suffix} (like uuid_avatar.webp or uuid_thumb.webp)
    if let Some(underscore_pos) = base_name.find('_') {
        let uuid_part = &base_name[..underscore_pos];
        let suffix_part = &base_name[underscore_pos + 1..];
        
        // Check if this matches our expected NEW pattern
        if valid_uuids.contains(uuid_part) && valid_suffixes.contains(&suffix_part) {
            println!("Keeping new format file: {}", filename);
            return true; // Keep this file
        }
        
        // Check for old format patterns that should be removed
        // Old patterns: {uuid}_120x120.jpg, {uuid}_48x48.jpg, {uuid}_{random-uuid}_banner.jpg
        if valid_uuids.contains(uuid_part) {
            // This is for a valid user but in old format - remove it
            if suffix_part.contains("x") || suffix_part.len() > 20 { // Likely old format
                println!("Removing old format file: {}", filename);
                return false;
            }
        }
    }
    
    // Check for files that don't start with a valid UUID - these are definitely stale
    let parts: Vec<&str> = base_name.split('_').collect();
    if !parts.is_empty() && !valid_uuids.contains(parts[0]) {
        println!("Removing file with invalid UUID: {}", filename);
        return false;
    }
    
    // If we can't determine the pattern clearly, keep the file to be safe
    println!("Keeping unknown pattern file: {}", filename);
    true
}

pub async fn update_user_by_uuid(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    path: web::Path<String>,
    user_data: web::Json<UserUpdateWithPassword>,
) -> impl Responder {
    let user_uuid = path.into_inner();
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // First get the user by UUID to get the ID
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid UUID format"),
    };

    let user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    let user_id = user.id;

    // Check if email is being updated and if it's already in use
    if let Some(email) = &user_data.email {
        if let Ok(existing_user) = repository::get_user_by_email(email, &mut conn) {
            if existing_user.id != user_id {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "Email already in use by another user"
                }));
            }
        }
    }

    // Create password hash if password is provided
    if let Some(password) = &user_data.password {
        use bcrypt::hash;
        use crate::models::NewUserAuthIdentity;
        
        // Hash the new password
        let password_hash = match hash(password, DEFAULT_COST) {
            Ok(hash) => hash,
            Err(_) => return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error hashing password"
            })),
        };
        
        // Find existing local auth identity
        let auth_identities = match repository::user_auth_identities::get_user_identities(user_id, &mut conn) {
            Ok(identities) => identities,
            Err(e) => {
                eprintln!("Error fetching auth identities: {:?}", e);
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Error processing user identities"
                }));
            }
        };
        
        // Find local identity
        let local_identity = auth_identities.iter().find(|identity| identity.provider_type == "local");
        
        match local_identity {
            Some(identity) => {
                // Update existing local identity
                // Delete the old identity 
                match diesel::delete(
                    crate::schema::user_auth_identities::table.filter(crate::schema::user_auth_identities::id.eq(identity.id))
                ).execute(&mut conn) {
                    Ok(_) => {},
                    Err(e) => {
                        eprintln!("Error deleting auth identity: {:?}", e);
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Error updating password"
                        }));
                    }
                }
                
                // Create new identity with updated password
                let new_auth_identity = NewUserAuthIdentity {
                    user_id,
                    provider_type: identity.provider_type.clone(),
                    external_id: identity.external_id.clone(),
                    email: identity.email.clone(),
                    metadata: identity.metadata.clone(),
                    password_hash: Some(password_hash),
                };
                
                if let Err(e) = repository::user_auth_identities::create_identity(new_auth_identity, &mut conn) {
                    eprintln!("Error creating updated auth identity: {:?}", e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error updating password"
                    }));
                }
            },
            None => {
                // Create new local identity if none exists
                let new_auth_identity = NewUserAuthIdentity {
                    user_id,
                    provider_type: "local".to_string(),
                    external_id: Uuid::new_v4().to_string(), // Generate a new provider user ID
                    email: user_data.email.clone(),
                    metadata: None,
                    password_hash: Some(password_hash),
                };
                
                if let Err(e) = repository::user_auth_identities::create_identity(new_auth_identity, &mut conn) {
                    eprintln!("Error creating auth identity: {:?}", e);
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error setting password"
                    }));
                }
            }
        }
    }

    // Update user
    let user_update = UserUpdate {
        name: user_data.name.clone(),
        email: user_data.email.clone(),
        role: user_data.role.as_ref().and_then(|r| utils::parse_role(r).ok()),
        pronouns: user_data.pronouns.clone(),
        avatar_url: user_data.avatar_url.clone(),
        banner_url: user_data.banner_url.clone(),
        avatar_thumb: user_data.avatar_thumb.clone(),
        microsoft_uuid: None, // Don't update Microsoft UUID in regular user updates
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match repository::update_user(user_id, user_update, &mut conn) {
        Ok(user) => HttpResponse::Ok().json(UserResponse::from(user)),
        Err(_) => HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Error updating user"
        })),
    }
}

/// Get user email addresses
pub async fn get_user_emails(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    path: web::Path<String>, // User UUID
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };

    let user_uuid = path.into_inner();

    // Validate token
    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
        })),
    };

    // Check authorization (user can access their own emails, admins can access any)
    if claims.sub != user_uuid && claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Not authorized to access this resource"
        }));
    }

    // Get user emails
    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid UUID format"
        })),
    };

    match user_emails_repo::get_user_emails_by_uuid(&mut conn, &uuid_parsed) {
        Ok(emails) => HttpResponse::Ok().json(json!({
            "status": "success",
            "emails": emails
        })),
        Err(e) => {
            eprintln!("Error fetching user emails for UUID {}: {:?}", user_uuid, e);
            match e {
                diesel::result::Error::NotFound => {
                    HttpResponse::NotFound().json(json!({
                        "status": "error",
                        "message": "User not found"
                    }))
                },
                _ => {
                    HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Failed to retrieve user emails"
                    }))
                }
            }
        }
    }
}

/// Get user with all email addresses
pub async fn get_user_with_emails(
    db_pool: web::Data<crate::db::Pool>,
    auth: BearerAuth,
    path: web::Path<String>, // User UUID
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };

    let user_uuid = path.into_inner();

    // Validate token
    let claims = match validate_token_internal(&auth, &mut conn).await {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Invalid or expired token"
        })),
    };

    // Check authorization
    if claims.sub != user_uuid && claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Not authorized to access this resource"
        }));
    }

    // Get user
    let uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid UUID format"
        })),
    };

    let user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Get user emails
    let emails = match user_emails_repo::get_user_emails(&mut conn, user.id) {
        Ok(emails) => emails,
        Err(e) => {
            eprintln!("Error fetching emails for user {}: {:?}", user.id, e);
            Vec::new() // Return empty vec if error fetching emails
        }
    };

    let user_with_emails = crate::models::UserWithEmails {
        user: UserResponse::from(user),
        emails,
    };

    HttpResponse::Ok().json(user_with_emails)
} 