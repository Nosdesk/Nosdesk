use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use diesel::result::Error;
use std::collections::HashMap;
use tracing::{debug, error};
use uuid::Uuid;
use crate::utils;
use crate::utils::rbac::{is_admin, is_technician_or_admin};

use crate::db::Pool;
use crate::models::{Claims, NewDevice, DeviceUpdate, Device, User, Group};
use crate::repository;
use crate::repository::groups as groups_repo;

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
    #[serde(rename = "type")]
    device_type: Option<String>,
    warranty: Option<String>,
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

// Enhanced device response with joined user data
#[derive(Debug, Serialize)]
pub struct DeviceResponse {
    pub id: i32,
    pub name: String,
    pub hostname: String,
    pub serial_number: String,
    pub model: String,
    pub warranty_status: String,
    pub manufacturer: Option<String>,
    pub primary_user_uuid: Option<String>,
    pub intune_device_id: Option<String>,
    pub entra_device_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub last_sync_time: Option<String>,
    pub primary_user: Option<UserInfo>,
    pub groups: Vec<GroupInfo>,
    pub is_editable: bool,
}

#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub uuid: String,
    pub name: String,
    pub email: String,
    pub role: String,
    pub avatar_url: Option<String>,
    pub avatar_thumb: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct GroupInfo {
    pub id: i32,
    pub uuid: String,
    pub name: String,
    pub color: Option<String>,
}

impl From<Group> for GroupInfo {
    fn from(group: Group) -> Self {
        Self {
            id: group.id,
            uuid: utils::uuid_to_string(&group.uuid),
            name: group.name,
            color: group.color,
        }
    }
}

impl DeviceResponse {
    pub fn from_device_and_user(device: Device, user: Option<User>, groups: Vec<Group>, conn: &mut crate::db::DbConnection) -> Self {
        // Device is editable only if it's NOT synced from Microsoft Graph
        // (i.e., it has neither intune_device_id nor entra_device_id)
        let is_editable = device.intune_device_id.is_none() && device.entra_device_id.is_none();

        Self {
            id: device.id,
            name: device.name,
            hostname: device.hostname.unwrap_or_default(),
            serial_number: device.serial_number.unwrap_or_default(),
            model: device.model.unwrap_or_default(),
            warranty_status: device.warranty_status.unwrap_or_default(),
            manufacturer: device.manufacturer,
            primary_user_uuid: device.primary_user_uuid.map(|uuid| utils::uuid_to_string(&uuid)),
            intune_device_id: device.intune_device_id.clone(),
            entra_device_id: device.entra_device_id.clone(),
            created_at: device.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            updated_at: device.updated_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            last_sync_time: device.last_sync_time.map(|t| t.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()),
            is_editable,
            primary_user: user.map(|u| {
                let name = u.name.clone();
                let role = match u.role {
                    crate::models::UserRole::Admin => "admin",
                    crate::models::UserRole::Technician => "technician",
                    crate::models::UserRole::User => "user",
                }.to_string();

                // Fetch primary email from user_emails table
                let email = repository::user_helpers::get_primary_email(&u.uuid, conn)
                    .unwrap_or_else(|| name.clone());

                UserInfo {
                    uuid: utils::uuid_to_string(&u.uuid),
                    name,
                    email,
                    role,
                    avatar_url: u.avatar_url,
                    avatar_thumb: u.avatar_thumb,
                }
            }),
            groups: groups.into_iter().map(GroupInfo::from).collect(),
        }
    }
}

// Helper function to get user by UUID
fn get_user_by_uuid(conn: &mut crate::db::DbConnection, uuid: &Uuid) -> Option<User> {
    use crate::repository;
    repository::get_user_by_uuid(uuid, conn).ok()
}

// Helper function to convert devices to device responses with user data
fn devices_to_responses(conn: &mut crate::db::DbConnection, devices: Vec<Device>) -> Vec<DeviceResponse> {
    devices.into_iter().map(|device| {
        let user = device.primary_user_uuid.as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        let groups = groups_repo::get_groups_for_device(conn, device.id).unwrap_or_default();
        DeviceResponse::from_device_and_user(device, user, groups, conn)
    }).collect()
}

/// Get all devices
pub async fn get_all_devices(pool: web::Data<Pool>) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::get_all_devices(&mut conn) {
        Ok(devices) => {
            // Convert devices to enhanced responses with user data
            let device_responses = devices_to_responses(&mut conn, devices);
            HttpResponse::Ok().json(device_responses)
        },
        Err(e) => {
            error!(error = ?e, "Database error getting all devices");
            HttpResponse::InternalServerError().json("Failed to get devices")
        }
    }
}

// Get paginated devices
pub async fn get_paginated_devices(
    pool: web::Data<Pool>,
    query: web::Query<PaginationParams>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(25).clamp(1, 100);

    match repository::get_paginated_devices(
        &mut conn,
        page,
        page_size,
        query.sort_field.clone(),
        query.sort_direction.clone(),
        query.search.clone(),
        query.device_type.clone(),
        query.warranty.clone(),
    ) {
        Ok((devices, total)) => {
            let total_pages = (total as f64 / page_size as f64).ceil() as i64;
            
            // Convert devices to enhanced responses with user data
            let device_responses = devices_to_responses(&mut conn, devices);
            
            let response = PaginatedResponse {
                data: device_responses,
                total,
                page,
                page_size,
                total_pages,
            };
            
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(error = ?e, "Database error getting paginated devices");
            HttpResponse::InternalServerError().json("Failed to get devices")
        }
    }
}

/// Get a single device by ID
pub async fn get_device_by_id(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    let device_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };
    
    match repository::get_device_by_id(&mut conn, device_id) {
        Ok(device) => {
            // Get user data if device has a primary user
            let user = device.primary_user_uuid.as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            // Get groups for the device
            let groups = groups_repo::get_groups_for_device(&mut conn, device_id).unwrap_or_default();
            debug!(device_id, group_count = groups.len(), "Fetched groups for device");

            let device_response = DeviceResponse::from_device_and_user(device, user, groups, &mut conn);
            HttpResponse::Ok().json(device_response)
        },
        Err(e) => {
            match e {
                Error::NotFound => HttpResponse::NotFound().json(format!("Device {device_id} not found")),
                _ => {
                    error!(device_id, error = ?e, "Database error getting device");
                    HttpResponse::InternalServerError().json(format!("Failed to get device {device_id}"))
                }
            }
        }
    }
}

/// Get devices for a specific user
pub async fn get_user_devices(
    pool: web::Data<Pool>,
    path: web::Path<String>,
) -> impl Responder {
    let user_uuid_str = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };
    
    // Parse UUID from string
    let user_uuid = match utils::parse_uuid(&user_uuid_str) {
        Ok(uuid) => uuid,
        Err(_) => return HttpResponse::BadRequest().json("Invalid UUID format"),
    };
    
    match crate::repository::devices::get_devices_for_user(&mut conn, &user_uuid) {
        Ok(devices) => {
            let device_responses = devices_to_responses(&mut conn, devices);
            HttpResponse::Ok().json(device_responses)
        },
        Err(e) => {
            error!(user_uuid = %user_uuid_str, error = ?e, "Error getting devices for user");
            HttpResponse::InternalServerError().json(format!("Failed to get devices for user {user_uuid_str}"))
        }
    }
}

/// Create a new device (technician or admin only)
pub async fn create_device(
    req: HttpRequest,
    pool: web::Data<Pool>,
    device: web::Json<NewDevice>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_technician_or_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can create devices"
        }));
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::create_device(&mut conn, device.into_inner()) {
        Ok(device) => {
            let device_id = device.id;

            // Get user data if device has a primary user
            let user = device.primary_user_uuid.as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            // Newly created device has no groups yet
            let device_response = DeviceResponse::from_device_and_user(device, user, vec![], &mut conn);

            // Broadcast SSE event for device creation
            use crate::utils::sse::SseBroadcaster;
            SseBroadcaster::broadcast_device_created(
                &sse_state,
                device_id,
                serde_json::to_value(&device_response).unwrap_or_default(),
            ).await;

            HttpResponse::Created().json(device_response)
        },
        Err(e) => {
            error!(error = ?e, "Database error creating device");
            HttpResponse::InternalServerError().json("Failed to create device")
        }
    }
}

/// Update a device (technician or admin only)
pub async fn update_device(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    device_update: web::Json<DeviceUpdate>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    req: HttpRequest,
) -> impl Responder {
    let device_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware for SSE events and role check
    let user_info = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    // Check role - only technicians and admins can update devices
    if !is_technician_or_admin(&user_info) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only technicians and administrators can update devices"
        }));
    }

    // Check if device is editable (not synced from Microsoft Graph)
    let existing_device = match repository::get_device_by_id(&mut conn, device_id) {
        Ok(device) => device,
        Err(e) => {
            return match e {
                Error::NotFound => HttpResponse::NotFound().json(format!("Device {device_id} not found")),
                _ => {
                    error!(device_id, error = ?e, "Database error getting device");
                    HttpResponse::InternalServerError().json(format!("Failed to get device {device_id}"))
                }
            }
        }
    };

    // Prevent editing devices synced from Microsoft Graph
    let is_synced = existing_device.intune_device_id.is_some() || existing_device.entra_device_id.is_some();
    if is_synced {
        return HttpResponse::Forbidden().json(json!({
            "error": "Cannot edit device synced from Microsoft Graph",
            "message": "This device is managed by Microsoft Intune/Entra and cannot be edited manually. Changes must be made in Microsoft Entra Admin Center or Intune."
        }));
    }

    let update_data = device_update.into_inner();
    
    // Convert to JSON before the move for SSE broadcasting
    let update_json = serde_json::to_value(&update_data).unwrap_or_default();
    
    match repository::update_device(&mut conn, device_id, update_data) {
        Ok(device) => {
            // Broadcast SSE events for each field that was updated
            if let Some(update_obj) = update_json.as_object() {
                for (key, value) in update_obj {
                    if !value.is_null() {
                        debug!(device_id, field = %key, value = ?value, "Broadcasting SSE event for device update");
                        use crate::utils::sse::SseBroadcaster;
                        SseBroadcaster::broadcast_device_updated(
                            &sse_state,
                            device_id,
                            key,
                            value.clone(),
                            &user_info.sub,
                        ).await;
                    }
                }
            }


            // Get user data if device has a primary user
            let user = device.primary_user_uuid.as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            // Get groups for the device
            let groups = groups_repo::get_groups_for_device(&mut conn, device_id).unwrap_or_default();

            let device_response = DeviceResponse::from_device_and_user(device, user, groups, &mut conn);
            HttpResponse::Ok().json(device_response)
        },
        Err(e) => {
            match e {
                Error::NotFound => HttpResponse::NotFound().json(format!("Device {device_id} not found")),
                _ => {
                    error!(device_id, error = ?e, "Database error updating device");
                    HttpResponse::InternalServerError().json(format!("Failed to update device {device_id}"))
                }
            }
        }
    }
}

/// Delete a device (admin only)
pub async fn delete_device(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    if !is_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can delete devices"
        }));
    }

    let device_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    match repository::delete_device(&mut conn, device_id) {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                HttpResponse::Ok().json(json!({
                    "message": format!("Device {} deleted successfully", device_id)
                }))
            } else {
                HttpResponse::NotFound().json(format!("Device {device_id} not found"))
            }
        }
        Err(e) => {
            error!(device_id, error = ?e, "Database error deleting device");
            HttpResponse::InternalServerError().json(format!("Failed to delete device {device_id}"))
        }
    }
}

/// Unmanage a device (remove Intune/Entra IDs to make it editable) - admin only
pub async fn unmanage_device(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    req: HttpRequest,
) -> impl Responder {
    let device_id = path.into_inner();
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    // Extract claims from cookie auth middleware and check role
    let user_info = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    // Only admins can unmanage devices
    if !is_admin(&user_info) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can unmanage devices"
        }));
    }

    // Check if device exists
    let existing_device = match repository::get_device_by_id(&mut conn, device_id) {
        Ok(device) => device,
        Err(e) => {
            return match e {
                Error::NotFound => HttpResponse::NotFound().json(format!("Device {device_id} not found")),
                _ => {
                    error!(device_id, error = ?e, "Database error getting device");
                    HttpResponse::InternalServerError().json(format!("Failed to get device {device_id}"))
                }
            }
        }
    };

    // Check if device is synced from Microsoft Graph
    let is_synced = existing_device.intune_device_id.is_some() || existing_device.entra_device_id.is_some();
    if !is_synced {
        return HttpResponse::BadRequest().json(json!({
            "error": "Device is not managed by Microsoft Graph",
            "message": "This device is already manually managed and doesn't need to be unmanaged."
        }));
    }

    // Remove Microsoft Graph IDs to make device editable by setting them to empty strings
    // Note: Empty strings will be stored in DB, but device will become editable (is_editable checks for None, not empty)
    let update_data = crate::models::DeviceUpdate {
        name: None,
        hostname: None,
        device_type: None,
        serial_number: None,
        manufacturer: None,
        model: None,
        warranty_status: None,
        location: None,
        notes: None,
        primary_user_uuid: None,
        microsoft_device_id: None,
        intune_device_id: Some(String::new()),
        entra_device_id: Some(String::new()),
        compliance_state: None,
        last_sync_time: None,
        operating_system: None,
        os_version: None,
        is_managed: None,
        enrollment_date: None,
        updated_at: None,
    };

    match repository::update_device(&mut conn, device_id, update_data) {
        Ok(device) => {
            // Get user data if device has a primary user
            let user = device.primary_user_uuid.as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            // Get groups for the device
            let groups = groups_repo::get_groups_for_device(&mut conn, device_id).unwrap_or_default();

            let device_response = DeviceResponse::from_device_and_user(device, user, groups, &mut conn);
            HttpResponse::Ok().json(device_response)
        },
        Err(e) => {
            error!(device_id, error = ?e, "Database error unmanaging device");
            HttpResponse::InternalServerError().json(format!("Failed to unmanage device {device_id}"))
        }
    }
}

/// Get paginated devices excluding specific IDs
pub async fn get_paginated_devices_excluding(
    pool: web::Data<Pool>,
    query: web::Query<PaginationParams>,
    exclude_query: web::Query<HashMap<String, String>>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);

    // Parse exclude_ids from comma-separated string
    let exclude_ids: Vec<i32> = exclude_query.get("excludeIds")
        .map(|ids_str| {
            ids_str.split(',')
                .filter_map(|id| id.trim().parse::<i32>().ok())
                .collect()
        })
        .unwrap_or_default();

    match crate::repository::devices::get_paginated_devices_excluding_ids(
        &mut conn, 
        page, 
        page_size, 
        query.search.as_deref(),
        &exclude_ids
    ) {
        Ok((devices, total_count)) => {
            let total_pages = ((total_count as f64) / (page_size as f64)).ceil() as i64;
            let device_responses = devices_to_responses(&mut conn, devices);
            
            let response = PaginatedResponse {
                data: device_responses,
                page,
                page_size,
                total: total_count,
                total_pages,
            };
            
            HttpResponse::Ok().json(response)
        },
        Err(e) => {
            error!(error = ?e, "Error getting paginated devices");
            HttpResponse::InternalServerError().json("Failed to get devices")
        }
    }
}

// Bulk device operations request
#[derive(Debug, Deserialize)]
pub struct BulkDeviceActionRequest {
    action: String,
    ids: Vec<i32>,
}

/// Perform bulk operations on devices (admin only)
pub async fn bulk_devices(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<BulkDeviceActionRequest>,
) -> impl Responder {
    // Extract claims and check authentication
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json(json!({
            "error": "Unauthorized",
            "message": "Authentication required"
        })),
    };

    // Only admins can perform bulk operations
    if !is_admin(&claims) {
        return HttpResponse::Forbidden().json(json!({
            "error": "Forbidden",
            "message": "Only administrators can perform bulk device operations"
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
            "message": "No device IDs provided"
        }));
    }

    match action {
        "delete" => {
            let mut deleted = 0;
            for id in ids {
                match repository::delete_device(&mut conn, *id) {
                    Ok(rows) => deleted += rows,
                    Err(e) => {
                        error!(device_id = *id, error = ?e, "Error deleting device in bulk operation");
                    }
                }
            }

            HttpResponse::Ok().json(json!({ "affected": deleted }))
        }

        _ => HttpResponse::BadRequest().json(json!({
            "error": "Bad Request",
            "message": format!("Unknown action: {}", action)
        })),
    }
} 