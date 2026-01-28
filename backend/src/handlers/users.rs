use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
use bcrypt::DEFAULT_COST;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;
use futures::{StreamExt, TryStreamExt};
use actix_multipart::Multipart;
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{debug, info, warn, error};

use crate::models::{UserResponse, UserUpdate, UserUpdateWithPassword};
use crate::repository;
use crate::repository::user_emails as user_emails_repo;
use crate::utils;
use crate::utils::email_branding::get_email_branding;
use crate::db::DbConnection;
use crate::services::search::SearchService;
use crate::services::search::indexing_tasks;

/// Result type for invitation sending operations
pub enum SendInvitationResult {
    Success,
    EmailServiceError(String),
    TokenStorageError(String),
    EmailSendError(String),
}

/// Helper function to create and send an invitation to a user.
/// This consolidates the duplicated invitation logic between create_user and resend_invitation.
async fn send_user_invitation(
    conn: &mut DbConnection,
    req: &HttpRequest,
    user_uuid: Uuid,
    user_email: &str,
    user_name: &str,
    admin_name: &str,
) -> SendInvitationResult {
    // Create invitation token
    let invitation_token = crate::utils::reset_tokens::ResetTokenUtils::create_reset_token(
        user_uuid,
        crate::utils::reset_tokens::TokenType::Invitation,
    );

    // Get request metadata
    let ip_address = req.connection_info().realip_remote_addr().map(|s| s.to_string());
    let user_agent = req.headers()
        .get("User-Agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Store the invitation token
    if let Err(e) = repository::reset_tokens::create_reset_token(
        conn,
        &invitation_token.token_hash,
        user_uuid,
        invitation_token.token_type.as_str(),
        ip_address.as_deref(),
        user_agent.as_deref(),
        invitation_token.expires_at,
        None,
    ) {
        return SendInvitationResult::TokenStorageError(format!("{e:?}"));
    }

    // Get frontend base URL
    let base_url = std::env::var("FRONTEND_URL").unwrap_or_else(|_| {
        let conn_info = req.connection_info();
        format!("{}://{}", conn_info.scheme(), conn_info.host())
    });

    // Initialize email service
    let email_service = match crate::utils::email::EmailService::from_env() {
        Ok(service) => service,
        Err(e) => return SendInvitationResult::EmailServiceError(format!("{e:?}")),
    };

    // Get branding for email
    let branding = get_email_branding(conn, &base_url);

    // Send invitation email
    if let Err(e) = email_service.send_invitation_email(
        user_email,
        user_name,
        &invitation_token.raw_token,
        &branding,
        admin_name,
    ).await {
        return SendInvitationResult::EmailSendError(format!("{e:?}"));
    }

    SendInvitationResult::Success
}

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
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json("Database connection error");
        },
    };

    match repository::get_users(&mut conn) {
        Ok(users) => {
            // Convert users to UserResponse with emails (batch fetch for efficiency)
            let user_responses = repository::user_helpers::get_users_with_primary_emails(users, &mut conn);
            HttpResponse::Ok().json(user_responses)
        },
        Err(e) => {
            error!(error = ?e, "Error fetching users");
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

    // Validate sort_field against allowed columns
    let allowed_sort_fields = ["first_name", "last_name", "email", "role", "created_at", "updated_at"];
    let sort_field = query.sort_field.as_ref().and_then(|f| {
        let f_lower = f.to_lowercase();
        if allowed_sort_fields.contains(&f_lower.as_str()) {
            Some(f_lower)
        } else {
            None
        }
    });

    // Validate sort_direction
    let sort_direction = query.sort_direction.as_ref().and_then(|d| {
        let d_lower = d.to_lowercase();
        if d_lower == "asc" || d_lower == "desc" {
            Some(d_lower)
        } else {
            None
        }
    });

    // Limit search string length to prevent DoS
    let search = query.search.as_ref().map(|s| {
        if s.len() > 100 { s[..100].to_string() } else { s.clone() }
    });

    // Validate role filter
    let allowed_roles = ["admin", "agent", "user"];
    let role = query.role.as_ref().and_then(|r| {
        let r_lower = r.to_lowercase();
        if allowed_roles.contains(&r_lower.as_str()) {
            Some(r_lower)
        } else {
            None
        }
    });

    match repository::get_paginated_users(
        &mut conn,
        page,
        page_size,
        sort_field,
        sort_direction,
        search,
        role,
    ) {
        Ok((users, total)) => {
            // Calculate total pages
            let total_pages = (total as f64 / page_size as f64).ceil() as i64;

            // Convert users to UserResponse with emails (batch fetch for efficiency)
            let user_responses = repository::user_helpers::get_users_with_primary_emails(users, &mut conn);

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
            error!(error = ?e, "Error fetching paginated users");
            HttpResponse::InternalServerError().json("Failed to get paginated users")
        },
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
        Ok(user) => {
            // Use helper function to fetch primary email from user_emails table
            let user_response = repository::user_helpers::get_user_with_primary_email(user, &mut conn);
            HttpResponse::Ok().json(user_response)
        },
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
            // Convert users to UserResponse with emails (batch fetch for efficiency)
            let user_responses = repository::user_helpers::get_users_with_primary_emails(users, &mut conn);
            HttpResponse::Ok().json(user_responses)
        },
        Err(e) => {
            error!(error = ?e, "Error fetching users batch");
            HttpResponse::InternalServerError().json("Failed to get users")
        },
    }
}

// API request model for user creation (includes email which goes in user_emails table)
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    /// Optional UUID - if not provided, a new UUIDv7 will be generated
    uuid: Option<Uuid>,
    name: String,
    email: String,
    role: crate::models::UserRole,
    pronouns: Option<String>,
    avatar_url: Option<String>,
    banner_url: Option<String>,
    avatar_thumb: Option<String>,
    microsoft_uuid: Option<Uuid>,
    /// Optional password - if provided, sets the password directly.
    /// If not provided and SMTP is configured, sends an invitation email.
    /// If not provided and SMTP is not configured, returns an error.
    password: Option<String>,
    /// Whether to send an invitation email (only used when SMTP is configured and no password provided)
    send_invitation: Option<bool>,
}

pub async fn create_user(
    db_pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    user_data: web::Json<CreateUserRequest>,
    req: HttpRequest,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Get the admin user who is creating this user (for invitation email)
    let admin_name = req.extensions()
        .get::<crate::models::Claims>()
        .and_then(|claims| {
            uuid::Uuid::parse_str(&claims.sub)
                .ok()
                .and_then(|uuid| repository::get_user_by_uuid(&uuid, &mut conn).ok())
                .map(|u| u.name)
        })
        .unwrap_or_else(|| "An administrator".to_string());

    // Check email configuration
    let email_config = crate::utils::email::EmailConfig::from_env().ok();
    let smtp_configured = email_config.as_ref().map(|c| c.is_configured()).unwrap_or(false);

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

    // Validate password if provided
    if let Some(ref password) = user_data.password {
        if password.len() < 8 {
            validation_errors.push("password: Password must be at least 8 characters".to_string());
        } else if password.len() > 128 {
            validation_errors.push("password: Password must be less than 128 characters".to_string());
        }
    }

    // If no password provided and SMTP not configured, require password
    if user_data.password.is_none() && !smtp_configured {
        validation_errors.push("password: Password is required when email is not configured".to_string());
    }

    if !validation_errors.is_empty() {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Validation failed",
            "errors": validation_errors
        }));
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
    if repository::get_user_by_email(&user_data.email, &mut conn).is_ok() {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "User with this email already exists"
        }));
    }

    // Use provided UUID or generate a new UUIDv7
    let user_uuid = user_data.uuid.unwrap_or_else(uuid::Uuid::now_v7);

    // Create new user with normalized data using builder
    let (normalized_name, normalized_email) = utils::normalization::normalize_user_data(&user_data.name, &user_data.email);
    let (new_user, email) = utils::NewUserBuilder::new(normalized_name.clone(), normalized_email, user_data.role)
        .with_uuid(user_uuid)
        .with_pronouns(user_data.pronouns.as_ref().map(|p| p.trim().to_string()))
        .with_avatar(
            user_data.avatar_url.as_ref().map(|u| u.trim().to_string()),
            user_data.avatar_thumb.as_ref().map(|u| u.trim().to_string())
        )
        .with_banner(user_data.banner_url.as_ref().map(|u| u.trim().to_string()))
        .with_microsoft_uuid(user_data.microsoft_uuid)
        .build_with_email();

    // Email starts as unverified - will be verified when user accepts invitation or verifies email
    match repository::user_helpers::create_user_with_email(new_user, email.clone(), false, Some("manual".to_string()), &mut conn) {
        Ok((user, _email_entry)) => {
            use bcrypt::hash;
            use crate::models::NewUserAuthIdentity;

            // Determine how to handle authentication setup
            let (password_hash, invitation_sent) = if let Some(ref password) = user_data.password {
                // Password provided - hash it directly
                let hash = match hash(password, DEFAULT_COST) {
                    Ok(h) => h,
                    Err(e) => {
                        error!(error = ?e, "Error hashing password");
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Error setting password"
                        }));
                    }
                };
                (Some(hash), false)
            } else if smtp_configured && user_data.send_invitation.unwrap_or(true) {
                // No password, SMTP configured - send invitation email
                match send_user_invitation(
                    &mut conn,
                    &req,
                    user.uuid,
                    &email,
                    &normalized_name,
                    &admin_name,
                ).await {
                    SendInvitationResult::Success => {
                        // Invitation sent successfully
                    },
                    SendInvitationResult::TokenStorageError(e) => {
                        error!(error = %e, "Error storing invitation token");
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Error creating invitation"
                        }));
                    },
                    SendInvitationResult::EmailServiceError(e) => {
                        error!(error = %e, "Error initializing email service");
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Error sending invitation email"
                        }));
                    },
                    SendInvitationResult::EmailSendError(e) => {
                        error!(error = %e, "Error sending invitation email");
                        // Don't fail - user was created, just couldn't send email
                        warn!("User created but invitation email failed to send");
                    },
                }

                (None, true) // No password hash - user will set via invitation
            } else {
                // No password and no SMTP - this should have been caught in validation
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": "Password is required when email is not configured"
                }));
            };

            debug!(user_uuid = %user.uuid, "Created user");

            // Create local auth identity (with or without password)
            let new_identity = NewUserAuthIdentity {
                user_uuid: user.uuid,
                provider_type: "local".to_string(),
                external_id: utils::uuid_to_string(&user.uuid),
                email: Some(email.clone()),
                metadata: None,
                password_hash,
            };

            match repository::user_auth_identities::create_identity(new_identity, &mut conn) {
                Ok(_) => {
                    if invitation_sent {
                        info!(user_name = %user.name, "New user created (invitation email sent)");
                    } else {
                        info!(user_name = %user.name, "New user created (password set)");
                    }
                    let user_uuid_str = user.uuid.to_string();
                    let response = repository::user_helpers::get_user_with_primary_email(user.clone(), &mut conn);

                    // Index the new user in search
                    indexing_tasks::spawn_index_user(
                        search_service.get_ref().clone(),
                        user,
                        Some(email.clone()),
                    );

                    // Broadcast user creation via SSE
                    crate::utils::sse::SseBroadcaster::broadcast_user_created(
                        &sse_state,
                        &user_uuid_str,
                        serde_json::to_value(&response).unwrap_or_default(),
                    ).await;

                    // Add invitation_sent flag to response
                    if let serde_json::Value::Object(mut map) = serde_json::to_value(&response).unwrap_or_default() {
                        map.insert("invitation_sent".to_string(), serde_json::Value::Bool(invitation_sent));
                        return HttpResponse::Created().json(map);
                    }
                    HttpResponse::Created().json(response)
                },
                Err(e) => {
                    error!(error = ?e, "Error creating auth identity");
                    // If identity creation fails, still return the user (with primary email)
                    let user_response = repository::user_helpers::get_user_with_primary_email(user, &mut conn);
                    HttpResponse::Created().json(user_response)
                }
            }
        },
        Err(e) => {
            error!(error = ?e, "Error creating user");

            // Provide more specific error messages for common issues
            let error_message = if format!("{e:?}").contains("duplicate") || format!("{e:?}").contains("unique") {
                if format!("{e:?}").contains("email") {
                    "Email address already exists in the system"
                } else if format!("{e:?}").contains("uuid") {
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

/// Request body for delete user endpoint
/// Accepts either mfa_code (when MFA is required/enabled) or password (when MFA is not required)
#[derive(Deserialize)]
pub struct DeleteUserRequest {
    pub mfa_code: Option<String>,
    pub password: Option<String>,
}

pub async fn delete_user(
    uuid: web::Path<String>,
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
    body: web::Json<DeleteUserRequest>,
) -> impl Responder {
    let user_uuid = uuid.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection error"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Only admins can delete users
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Only administrators can delete users"
        }));
    }

    // Get the admin user (the one making the request) to verify their credentials
    let admin_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid admin UUID"
        })),
    };

    let admin_user = match repository::get_user_by_uuid(&admin_uuid, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Failed to get admin user"
        })),
    };

    // Check if MFA is required for admins
    let mfa_required = crate::utils::mfa::should_require_mfa(&admin_user.role);
    let admin_has_mfa = admin_user.mfa_enabled && admin_user.mfa_secret.is_some();

    // Verify credentials based on MFA requirement
    if mfa_required && admin_has_mfa {
        // MFA is required and enabled - verify MFA code
        let mfa_code = match &body.mfa_code {
            Some(code) if !code.is_empty() => code,
            _ => return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "MFA code is required to delete users"
            })),
        };

        let mfa_result = match crate::utils::mfa::verify_mfa_token(&admin_uuid, mfa_code, &mut conn).await {
            Ok(result) => result,
            Err(e) => {
                return HttpResponse::BadRequest().json(json!({
                    "status": "error",
                    "message": format!("MFA verification failed: {}", e)
                }));
            }
        };

        if !mfa_result.is_valid {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid MFA code"
            }));
        }
    } else {
        // MFA not required or not enabled - verify password
        let password = match &body.password {
            Some(pwd) if !pwd.is_empty() => pwd,
            _ => return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Password is required to delete users"
            })),
        };

        // Get admin's auth identity to verify password
        let auth_identities = match repository::user_auth_identities::get_user_identities(&admin_uuid, &mut conn) {
            Ok(identities) => identities,
            Err(_) => return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to get authentication data"
            })),
        };

        // Find local auth identity with password
        let local_identity = auth_identities.iter()
            .find(|id| id.provider_type == "local" && id.password_hash.is_some());

        let password_hash = match local_identity {
            Some(identity) => identity.password_hash.as_ref().unwrap(),
            None => return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "No password set for this account"
            })),
        };

        // Verify password
        let password_valid = match bcrypt::verify(password, password_hash) {
            Ok(valid) => valid,
            Err(_) => return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Password verification failed"
            })),
        };

        if !password_valid {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid password"
            }));
        }
    }

    // Parse the target user UUID
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid UUID format"
        })),
    };

    // Get the target user
    let target_user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Prevent self-deletion
    if claims.sub == user_uuid {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "You cannot delete your own account while logged in"
        }));
    }

    // Prevent deletion of admin users (safety measure)
    if target_user.role == crate::models::UserRole::Admin {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Administrator accounts cannot be deleted for security reasons"
        }));
    }

    // Delete the user
    match repository::delete_user(&target_user.uuid, &mut conn) {
        Ok(count) if count > 0 => {
            info!("User deleted successfully: {} (uuid={})", target_user.name, target_user.uuid);

            // Remove user from search index
            indexing_tasks::spawn_delete_user(search_service.get_ref().clone(), user_uuid.clone());

            // Broadcast user deletion via SSE
            crate::utils::sse::SseBroadcaster::broadcast_user_deleted(
                &sse_state,
                &user_uuid,
            ).await;

            HttpResponse::NoContent().finish()
        },
        Ok(_) => HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
        Err(e) => {
            error!("Failed to delete user {} (uuid={}): {:?}", target_user.name, target_user.uuid, e);
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to delete user. The user may have associated data that prevents deletion."
            }))
        },
    }
}

// Get user's authentication identities
pub async fn get_user_auth_identities(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
) -> impl Responder {
    // Get database connection
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Authentication required"
            }));
        }
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
            error!(error = ?e, "Error getting user by UUID");
            return HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": "User not found"
            }));
        }
    };

    // Get auth identities for the user
    match repository::user_auth_identities::get_user_identities_display(&user.uuid, &mut conn) {
        Ok(identities) => HttpResponse::Ok().json(identities),
        Err(e) => {
            error!(error = ?e, "Error fetching auth identities");
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
    req: HttpRequest,
    path: web::Path<String>, // User UUID
) -> impl Responder {
    // Get database connection
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Could not get database connection"
            }));
        }
    };

    let user_uuid = path.into_inner();

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Authentication required"
            }));
        }
    };

    // Ensure the user is authorized (either accessing their own identities or is an admin)
    if claims.sub != user_uuid && claims.role != "admin" {
        warn!(requesting_user = %claims.sub, target_user = %user_uuid, "Authorization failed: user tried to access identities of another user");
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

    match repository::user_auth_identities::get_user_identities_display(&user_uuid_parsed, &mut conn) {
        Ok(identities) => HttpResponse::Ok().json(identities),
        Err(e) => {
            error!(user_uuid = %user_uuid, error = ?e, "Error fetching auth identities for UUID");
            HttpResponse::NotFound().json(json!({
                "status": "error",
                "message": "User not found or no auth identities"
            }))
        }
    }
}

// Delete a user authentication identity
pub async fn delete_user_auth_identity(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
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

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
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
            error!(error = ?e, "Error getting user by UUID");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to get user data"
            }));
        }
    };

    // Ensure the user has at least one other auth method before deleting
    // (to prevent locking themselves out)
    let identities = match repository::user_auth_identities::get_user_identities(&user.uuid, &mut conn) {
        Ok(identities) => identities,
        Err(e) => {
            error!(error = ?e, "Error getting user auth identities");
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
    match repository::user_auth_identities::delete_identity(identity_id, &user.uuid, &mut conn) {
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
            error!(error = ?e, "Error deleting user auth identity");
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
    req: HttpRequest,
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

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
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

    let identities = match repository::user_auth_identities::get_user_identities(&user_uuid_parsed, &mut conn) {
        Ok(identities) => identities,
        Err(e) => {
            error!(error = ?e, "Error getting user auth identities");
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
    match repository::user_auth_identities::delete_identity(identity_id, &user_uuid_parsed, &mut conn) {
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
            error!(error = ?e, "Error deleting user auth identity");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to delete authentication identity"
            }))
        }
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
            error!(error = ?e, "Database connection error");
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
    let _full_storage_path = format!("{storage_path}/{user_uuid}");

    debug!(image_type = %image_type, user_uuid = %user_uuid, "Processing image upload");

    // Process the uploaded image (we only handle the first field)
    if let Ok(Some(mut field)) = payload.try_next().await {
        debug!(field_name = ?field.name(), "Received multipart field");

        // Get content type
        let content_type = field.content_type().map(|ct| ct.to_string()).unwrap_or_else(|| "application/octet-stream".to_string());
        debug!(content_type = %content_type, "Content type");
        
        // Validate content type (only allow images)
        if !content_type.starts_with("image/") {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Only image files are allowed"
            }));
        }
        
        // Check for HEIC/HEIF - these should be converted on the client side
        if content_type.as_str() == "image/heic" || content_type.as_str() == "image/heif" {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "HEIC/HEIF format should be converted to JPEG on the client side before upload"
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
        cleanup_old_user_images(storage_path, &user_uuid, image_type).await
            .map_err(|e| {
                warn!(error = %e, "Failed to cleanup old images");
                // Continue even if cleanup fails
            }).ok();
        
        let filename = format!("{user_uuid}_{image_type}.{file_ext}");
        let _file_path = format!("{storage_path}/{filename}");
        
        // Read file data
        let mut file_data = Vec::new();
        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(e) => {
                    error!(error = ?e, "Error reading chunk");
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
                    debug!(user_uuid = %user_uuid, avatar_url = %avatar_url, "Successfully processed avatar");

                    // Generate thumbnail from the processed avatar
                    let thumb_url = match crate::utils::image::generate_user_avatar_thumbnail(&avatar_url, &user_uuid).await {
                        Ok(Some(thumb_url)) => {
                            debug!(user_uuid = %user_uuid, thumb_url = %thumb_url, "Successfully generated thumbnail");
                            Some(thumb_url)
                        },
                        Ok(None) => {
                            warn!(user_uuid = %user_uuid, "Failed to generate thumbnail");
                            None
                        },
                        Err(e) => {
                            error!(user_uuid = %user_uuid, error = %e, "Error generating thumbnail");
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
                    error!(user_uuid = %user_uuid, error = %e, "Error processing avatar");
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": format!("Failed to process avatar image: {}", e)
                    }));
                }
            }
        } else {
            // For banners, process and resize to WebP format with banner dimensions (1200x400 max)
            match crate::utils::image::process_banner_image(&file_data, &user_uuid, 1200, 400).await {
                Ok(Some(banner_url)) => {
                    debug!(user_uuid = %user_uuid, banner_url = %banner_url, "Successfully processed banner");
                    (banner_url, None)
                },
                Ok(None) => {
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Failed to process banner image"
                    }));
                },
                Err(e) => {
                    error!(user_uuid = %user_uuid, error = %e, "Error processing banner");
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": format!("Failed to process banner image: {}", e)
                    }));
                }
            }
        };
        
        // Update the user record with the new image URL
        
        let user_update = UserUpdate {
            name: None,
            role: None,
            pronouns: None,
            avatar_url: if image_type == "avatar" { Some(final_url.clone()) } else { None },
            banner_url: if image_type == "banner" { Some(final_url.clone()) } else { None },
            avatar_thumb: thumbnail_url,
            theme: None, // Don't update theme when uploading images
            microsoft_uuid: None, // Don't update Microsoft UUID in regular user updates
            updated_at: Some(chrono::Utc::now().naive_utc()),
        };
        
        match repository::update_user(&user.uuid, user_update, &mut conn) {
            Ok(updated_user) => {
                return HttpResponse::Ok().json(json!({
                    "status": "success",
                    "message": format!("User {} updated successfully", image_type),
                    "url": final_url,
                    "user": UserResponse::from(updated_user)
                }));
            },
            Err(e) => {
                error!(error = ?e, "Error updating user");
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
    let pattern_prefix = format!("{user_uuid}_{image_type}");
    
    while let Ok(Some(entry)) = dir.next_entry().await {
        if let Some(filename) = entry.file_name().to_str() {
            // Check if this file matches our pattern (user_uuid_type.ext)
            if filename.starts_with(&pattern_prefix) && filename.contains('.') {
                let file_path = entry.path();
                debug!(file_path = ?file_path, "Cleaning up old image file");
                
                if let Err(e) = fs::remove_file(&file_path).await {
                    warn!(file_path = ?file_path, error = %e, "Failed to remove old image file");
                    // Continue with cleanup even if one file fails
                }
            }
        }
    }
    
    Ok(())
}

/// Clean up all stale user images (admin endpoint)
pub async fn cleanup_stale_images(
    req: HttpRequest,
    db_pool: web::Data<crate::db::Pool>,
) -> impl Responder {
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Could not get database connection"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
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
            error!(error = ?e, "Error fetching users");
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
        cleanup_stats.errors.push(format!("Avatar cleanup error: {e}"));
    }

    // Clean up banner directory  
    if let Err(e) = cleanup_directory_stale_files(
        "uploads/users/banners", 
        &users, 
        &["banner"], 
        &mut cleanup_stats
    ).await {
        cleanup_stats.errors.push(format!("Banner cleanup error: {e}"));
    }

    // Clean up thumbnail directory
    if let Err(e) = cleanup_directory_stale_files(
        "uploads/users/thumbs", 
        &users, 
        &["thumb"], 
        &mut cleanup_stats
    ).await {
        cleanup_stats.errors.push(format!("Thumbnail cleanup error: {e}"));
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
                debug!(file_path = ?file_path, "Removing stale image file");
                
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
                        let error_msg = format!("Failed to remove {file_path:?}: {e}");
                        warn!(file_path = ?file_path, error = %e, "Failed to remove file");
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
            debug!(filename = %filename, "Keeping new format file");
            return true; // Keep this file
        }

        // Check for old format patterns that should be removed
        // Old patterns: {uuid}_120x120.jpg, {uuid}_48x48.jpg, {uuid}_{random-uuid}_banner.jpg
        if valid_uuids.contains(uuid_part) {
            // This is for a valid user but in old format - remove it
            if suffix_part.contains("x") || suffix_part.len() > 20 { // Likely old format
                debug!(filename = %filename, "Removing old format file");
                return false;
            }
        }
    }

    // Check for files that don't start with a valid UUID - these are definitely stale
    let parts: Vec<&str> = base_name.split('_').collect();
    if !parts.is_empty() && !valid_uuids.contains(parts[0]) {
        debug!(filename = %filename, "Removing file with invalid UUID");
        return false;
    }

    // If the pattern cannot be determined clearly, keep the file to be safe
    debug!(filename = %filename, "Keeping unknown pattern file");
    true
}

pub async fn update_user_by_uuid(
    db_pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
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

    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // First get the user by UUID to get the ID
    let user_uuid_parsed = match utils::parse_uuid(&user_uuid) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid UUID format"),
    };

    // Authorization: Users can only update their own profile, admins can update anyone
    if claims.sub != user_uuid && claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "You can only update your own profile"
        }));
    }

    let user = match repository::get_user_by_uuid(&user_uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Check if email is being updated and if it's already in use
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
        let auth_identities = match repository::user_auth_identities::get_user_identities(&user.uuid, &mut conn) {
            Ok(identities) => identities,
            Err(e) => {
                error!(error = ?e, "Error fetching auth identities");
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
                        error!(error = ?e, "Error deleting auth identity");
                        return HttpResponse::InternalServerError().json(json!({
                            "status": "error",
                            "message": "Error updating password"
                        }));
                    }
                }
                
                // Create new identity with updated password
                let new_auth_identity = NewUserAuthIdentity {
                    user_uuid: user.uuid,
                    provider_type: identity.provider_type.clone(),
                    external_id: identity.external_id.clone(),
                    email: identity.email.clone(),
                    metadata: identity.metadata.clone(),
                    password_hash: Some(password_hash),
                };

                if let Err(e) = repository::user_auth_identities::create_identity(new_auth_identity, &mut conn) {
                    error!(error = ?e, "Error creating updated auth identity");
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error updating password"
                    }));
                }
            },
            None => {
                // Create new local identity if none exists
                let new_auth_identity = NewUserAuthIdentity {
                    user_uuid: user.uuid,
                    provider_type: "local".to_string(),
                    external_id: Uuid::now_v7().to_string(), // Generate a new provider user ID
                    email: None, // Email in user_emails table
                    metadata: None,
                    password_hash: Some(password_hash),
                };
                
                if let Err(e) = repository::user_auth_identities::create_identity(new_auth_identity, &mut conn) {
                    error!(error = ?e, "Error creating auth identity");
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error setting password"
                    }));
                }
            }
        }
    }

    // Validate theme if provided (allow any non-empty string as theme ID)
    if let Some(ref theme) = user_data.theme {
        if theme.is_empty() || theme.len() > 50 {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Theme must be a non-empty string up to 50 characters"
            }));
        }
    }

    // Update user
    let user_update = UserUpdate {
        name: user_data.name.clone(),
        // Email removed - use dedicated email endpoints
        role: user_data.role.as_ref().and_then(|r| utils::parse_role(r).ok()),
        pronouns: user_data.pronouns.clone(),
        avatar_url: user_data.avatar_url.clone(),
        banner_url: user_data.banner_url.clone(),
        avatar_thumb: user_data.avatar_thumb.clone(),
        theme: user_data.theme.clone(),
        microsoft_uuid: None, // Don't update Microsoft UUID in regular user updates
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    match repository::update_user(&user.uuid, user_update, &mut conn) {
        Ok(updated_user) => {
            // Broadcast SSE events for changed fields
            let updated_by = claims.sub.clone();

            if user_data.name.is_some() {
                crate::utils::sse::SseBroadcaster::broadcast_user_updated(
                    &sse_state,
                    &user_uuid,
                    "name",
                    json!(updated_user.name.clone()),
                    &updated_by,
                ).await;
            }
            if user_data.role.is_some() {
                let role_str = match updated_user.role {
                    crate::models::UserRole::Admin => "admin",
                    crate::models::UserRole::Technician => "technician",
                    crate::models::UserRole::User => "user",
                };
                crate::utils::sse::SseBroadcaster::broadcast_user_updated(
                    &sse_state,
                    &user_uuid,
                    "role",
                    json!(role_str),
                    &updated_by,
                ).await;
            }
            if user_data.pronouns.is_some() {
                crate::utils::sse::SseBroadcaster::broadcast_user_updated(
                    &sse_state,
                    &user_uuid,
                    "pronouns",
                    json!(updated_user.pronouns.clone()),
                    &updated_by,
                ).await;
            }
            if user_data.avatar_url.is_some() {
                crate::utils::sse::SseBroadcaster::broadcast_user_updated(
                    &sse_state,
                    &user_uuid,
                    "avatar_url",
                    json!(updated_user.avatar_url.clone()),
                    &updated_by,
                ).await;
            }
            if user_data.avatar_thumb.is_some() {
                crate::utils::sse::SseBroadcaster::broadcast_user_updated(
                    &sse_state,
                    &user_uuid,
                    "avatar_thumb",
                    json!(updated_user.avatar_thumb.clone()),
                    &updated_by,
                ).await;
            }

            // Re-index the updated user in search
            let primary_email = repository::user_helpers::get_primary_email(&updated_user.uuid, &mut conn);
            indexing_tasks::spawn_index_user(
                search_service.get_ref().clone(),
                updated_user.clone(),
                primary_email,
            );

            // Use helper function to fetch primary email from user_emails table
            let user_response = repository::user_helpers::get_user_with_primary_email(updated_user, &mut conn);
            HttpResponse::Ok().json(user_response)
        },
        Err(_) => HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Error updating user"
        })),
    }
}

/// Get user email addresses
pub async fn get_user_emails(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
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

    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
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

    // Get user first to ensure they exist
    let _user = match repository::get_user_by_uuid(&uuid_parsed, &mut conn) {
        Ok(user) => user,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "User not found"
        })),
    };

    // Get emails from user_emails table (single source of truth)
    let emails = user_emails_repo::get_user_emails_by_uuid(&mut conn, &uuid_parsed)
        .unwrap_or_else(|_| Vec::new());

    HttpResponse::Ok().json(json!({
        "status": "success",
        "emails": emails
    }))
}

/// Add a new email address for a user
pub async fn add_user_email(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>,
    email_data: web::Json<serde_json::Value>,
) -> impl Responder {
    let user_uuid = path.into_inner();
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };

    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Authorization: Users can only add emails to their own account, admins can add to anyone
    if claims.sub != user_uuid && claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Not authorized"
        }));
    }

    // Extract email from request
    let email = match email_data.get("email").and_then(|e| e.as_str()) {
        Some(e) => e.trim().to_lowercase(),
        None => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Email is required"
        })),
    };

    // Validate email format
    if !email.contains('@') || !email.contains('.') {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid email format"
        }));
    }

    // Get user ID
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

    // Check if email already exists
    if user_emails_repo::find_user_by_any_email(&mut conn, &email).is_ok() {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Email address already in use"
        }));
    }

    // Create new email
    let new_email = crate::models::NewUserEmail {
        user_uuid: user.uuid,
        email: email.clone(),
        email_type: "personal".to_string(),
        is_primary: false,
        is_verified: false,
        source: Some("manual".to_string()),
    };

    use diesel::prelude::*;
    match diesel::insert_into(crate::schema::user_emails::table)
        .values(&new_email)
        .get_result::<crate::models::UserEmail>(&mut conn)
    {
        Ok(created_email) => HttpResponse::Created().json(json!({
            "status": "success",
            "message": "Email added successfully",
            "email": created_email
        })),
        Err(e) => {
            error!(error = ?e, "Error adding email");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to add email"
            }))
        }
    }
}

/// Update an email address (set as primary)
pub async fn update_user_email(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<(String, i32)>,
    update_data: web::Json<serde_json::Value>,
) -> impl Responder {
    let (user_uuid, email_id) = path.into_inner();
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };

    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Authorization
    if claims.sub != user_uuid && claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Not authorized"
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

    // If setting as primary, unset other primary emails first
    if update_data.get("is_primary").and_then(|p| p.as_bool()).unwrap_or(false) {
        use diesel::prelude::*;
        let _ = diesel::update(crate::schema::user_emails::table)
            .filter(crate::schema::user_emails::user_uuid.eq(&user.uuid))
            .set(crate::schema::user_emails::is_primary.eq(false))
            .execute(&mut conn);
    }

    // Update the email
    let email_update = crate::models::UserEmailUpdate {
        is_primary: update_data.get("is_primary").and_then(|p| p.as_bool()),
        is_verified: update_data.get("is_verified").and_then(|v| v.as_bool()),
        updated_at: Some(chrono::Utc::now().naive_utc()),
    };

    use diesel::prelude::*;
    match diesel::update(crate::schema::user_emails::table.find(email_id))
        .set(&email_update)
        .get_result::<crate::models::UserEmail>(&mut conn)
    {
        Ok(updated_email) => HttpResponse::Ok().json(json!({
            "status": "success",
            "message": "Email updated successfully",
            "email": updated_email
        })),
        Err(e) => {
            error!(error = ?e, "Error updating email");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to update email"
            }))
        }
    }
}

/// Delete an email address
pub async fn delete_user_email(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<(String, i32)>,
) -> impl Responder {
    let (user_uuid, email_id) = path.into_inner();
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };

    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Authorization
    if claims.sub != user_uuid && claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Not authorized"
        }));
    }

    // Get user and verify email belongs to them
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

    // Check if email is primary
    use diesel::prelude::*;
    let email: crate::models::UserEmail = match crate::schema::user_emails::table
        .find(email_id)
        .first(&mut conn)
    {
        Ok(email) => email,
        Err(_) => return HttpResponse::NotFound().json(json!({
            "status": "error",
            "message": "Email not found"
        })),
    };

    if email.user_uuid != user.uuid {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Email does not belong to this user"
        }));
    }

    if email.is_primary {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Cannot delete primary email address"
        }));
    }

    // Delete the email
    match diesel::delete(crate::schema::user_emails::table.find(email_id))
        .execute(&mut conn)
    {
        Ok(_) => HttpResponse::Ok().json(json!({
            "status": "success",
            "message": "Email deleted successfully"
        })),
        Err(e) => {
            error!(error = ?e, "Error deleting email");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to delete email"
            }))
        }
    }
}

/// Resend invitation email to a user who hasn't set up their account yet
pub async fn resend_invitation(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
    path: web::Path<String>, // User UUID
) -> impl Responder {
    let user_uuid = path.into_inner();
    let mut conn = match db_pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Database connection failed"
        })),
    };

    // Extract claims from cookie auth middleware
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
        })),
    };

    // Only admins can resend invitations
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Only administrators can resend invitations"
        }));
    }

    // Check email configuration
    let email_config = crate::utils::email::EmailConfig::from_env().ok();
    let smtp_configured = email_config.as_ref().map(|c| c.is_configured()).unwrap_or(false);

    if !smtp_configured {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Email is not configured. Cannot send invitation."
        }));
    }

    // Get the user
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

    // Check if user already has a password set (completed setup)
    let auth_identities = repository::user_auth_identities::get_user_identities(&user.uuid, &mut conn).unwrap_or_default();

    let has_password = auth_identities.iter().any(|identity| {
        identity.provider_type == "local" && identity.password_hash.is_some()
    });

    if has_password {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "User has already completed account setup. Cannot resend invitation."
        }));
    }

    // Get user's primary email - try to find one marked as primary first
    let emails = match user_emails_repo::get_user_emails_by_uuid(&mut conn, &user.uuid) {
        Ok(emails) if !emails.is_empty() => emails,
        _ => return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "User has no email address"
        })),
    };

    // Find primary email or use first one
    let user_email = emails
        .iter()
        .find(|e| e.is_primary)
        .map(|e| e.email.clone())
        .unwrap_or_else(|| emails[0].email.clone());

    // Get the admin user's name for the invitation email
    let admin_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "status": "error",
            "message": "Invalid admin UUID"
        })),
    };

    let admin_name = match repository::get_user_by_uuid(&admin_uuid, &mut conn) {
        Ok(admin) => admin.name,
        Err(_) => "An administrator".to_string(),
    };

    // Invalidate any existing invitation tokens for this user
    if let Err(e) = repository::reset_tokens::invalidate_tokens_by_type(
        &mut conn,
        user.uuid,
        "invitation",
    ) {
        warn!(user_uuid = %user.uuid, error = ?e, "Failed to invalidate old invitation tokens");
        // Continue anyway - old tokens will expire naturally
    }

    // Send invitation using shared helper
    match send_user_invitation(
        &mut conn,
        &req,
        user.uuid,
        &user_email,
        &user.name,
        &admin_name,
    ).await {
        SendInvitationResult::Success => {
            info!(email = %user_email, user_name = %user.name, "Invitation email resent");
            HttpResponse::Ok().json(json!({
                "status": "success",
                "message": "Invitation email sent successfully",
                "email": user_email
            }))
        },
        SendInvitationResult::TokenStorageError(e) => {
            error!(error = %e, "Error storing invitation token");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error creating invitation"
            }))
        },
        SendInvitationResult::EmailServiceError(e) => {
            error!(error = %e, "Error initializing email service");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Error sending invitation email"
            }))
        },
        SendInvitationResult::EmailSendError(e) => {
            error!(error = %e, "Error sending invitation email");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to send invitation email"
            }))
        },
    }
}

/// Get user with all email addresses
pub async fn get_user_with_emails(
    db_pool: web::Data<crate::db::Pool>,
    req: HttpRequest,
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

    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "status": "error",
            "message": "Authentication required"
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
    let emails = match user_emails_repo::get_user_emails_by_uuid(&mut conn, &user.uuid) {
        Ok(emails) => emails,
        Err(e) => {
            error!(user_uuid = %user.uuid, error = ?e, "Error fetching emails for user");
            Vec::new() // Return empty vec if error fetching emails
        }
    };

    // Get user response with primary email populated
    let user_response = repository::user_helpers::get_user_with_primary_email(user, &mut conn);

    let user_with_emails = crate::models::UserWithEmails {
        user: user_response,
        emails,
    };

    HttpResponse::Ok().json(user_with_emails)
}

// Bulk user operations request
#[derive(Debug, Deserialize)]
pub struct BulkUserActionRequest {
    action: String,
    ids: Vec<String>, // UUIDs as strings
    value: Option<String>,
}

/// Perform bulk operations on users (admin only)
pub async fn bulk_users(
    req: HttpRequest,
    pool: web::Data<crate::db::Pool>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    body: web::Json<BulkUserActionRequest>,
) -> impl Responder {
    // Extract claims and check authentication
    let claims = match crate::utils::jwt::JwtUtils::extract_claims(&req) {
        Ok(claims) => claims,
        Err(_) => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    // Only admins can perform bulk operations
    if claims.role != "admin" {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can perform bulk user operations"
        }));
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json(json!({
            "error": "Database connection failed"
        })),
    };

    let action = body.action.as_str();
    let ids = &body.ids;

    if ids.is_empty() {
        return HttpResponse::BadRequest().json(json!({
            "error": "Bad Request",
            "message": "No user IDs provided"
        }));
    }

    // Prevent self-deletion/modification
    if ids.contains(&claims.sub) {
        return HttpResponse::BadRequest().json(json!({
            "error": "Bad Request",
            "message": "Cannot perform bulk operations on your own account"
        }));
    }

    match action {
        "delete" => {
            let mut deleted = 0;
            for id in ids {
                let uuid = match Uuid::parse_str(id) {
                    Ok(u) => u,
                    Err(_) => continue,
                };

                match repository::users::delete_user(&uuid, &mut conn) {
                    Ok(_) => {
                        deleted += 1;
                        // Remove from search index
                        indexing_tasks::spawn_delete_user(search_service.get_ref().clone(), id.clone());
                        // Broadcast SSE event
                        crate::utils::sse::SseBroadcaster::broadcast_user_deleted(
                            &sse_state,
                            id,
                        ).await;
                    }
                    Err(e) => {
                        error!(user_id = %id, error = ?e, "Error deleting user");
                    }
                }
            }

            HttpResponse::Ok().json(json!({ "affected": deleted }))
        }

        "set-role" => {
            let role_str = match &body.value {
                Some(v) => v.as_str(),
                None => return HttpResponse::BadRequest().json(json!({
                    "error": "Bad Request",
                    "message": "Role value required"
                })),
            };

            let role = match role_str {
                "admin" => crate::models::UserRole::Admin,
                "technician" => crate::models::UserRole::Technician,
                "user" => crate::models::UserRole::User,
                _ => return HttpResponse::BadRequest().json(json!({
                    "error": "Bad Request",
                    "message": "Invalid role value"
                })),
            };

            let mut updated = 0;
            for id in ids {
                let uuid = match Uuid::parse_str(id) {
                    Ok(u) => u,
                    Err(_) => continue,
                };

                let user_update = crate::models::UserUpdate {
                    name: None,
                    role: Some(role),
                    pronouns: None,
                    avatar_url: None,
                    banner_url: None,
                    avatar_thumb: None,
                    theme: None,
                    microsoft_uuid: None,
                    updated_at: Some(chrono::Utc::now().naive_utc()),
                };

                if repository::update_user(&uuid, user_update, &mut conn).is_ok() {
                    updated += 1;
                    // Broadcast SSE event
                    crate::utils::sse::SseBroadcaster::broadcast_user_updated(
                        &sse_state,
                        id,
                        "role",
                        json!(role_str),
                        &claims.sub,
                    ).await;
                }
            }

            HttpResponse::Ok().json(json!({ "affected": updated }))
        }

        _ => HttpResponse::BadRequest().json(json!({
            "error": "Bad Request",
            "message": format!("Unknown action: {}", action)
        })),
    }
} 