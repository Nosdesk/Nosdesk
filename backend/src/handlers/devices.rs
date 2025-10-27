use actix_web::{web, HttpResponse, HttpRequest, HttpMessage, Responder};
use serde::{Deserialize, Serialize};
use serde_json::json;
use diesel::result::Error;
use std::collections::HashMap;
use uuid::Uuid;
use crate::utils;

use crate::db::Pool;
use crate::models::{NewDevice, DeviceUpdate, Device, User};
use crate::repository;

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
    pub primary_user: Option<UserInfo>,
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

impl DeviceResponse {
    pub fn from_device_and_user(device: Device, user: Option<User>, conn: &mut crate::db::DbConnection) -> Self {
        Self {
            id: device.id,
            name: device.name,
            hostname: device.hostname.unwrap_or_default(),
            serial_number: device.serial_number.unwrap_or_default(),
            model: device.model.unwrap_or_default(),
            warranty_status: device.warranty_status.unwrap_or_default(),
            manufacturer: device.manufacturer,
            primary_user_uuid: device.primary_user_uuid.map(|uuid| utils::uuid_to_string(&uuid)),
            intune_device_id: device.intune_device_id,
            entra_device_id: device.entra_device_id,
            created_at: device.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            updated_at: device.updated_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
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
        DeviceResponse::from_device_and_user(device, user, conn)
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
            eprintln!("Database error getting all devices: {:?}", e);
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
            eprintln!("Database error getting paginated devices: {:?}", e);
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

            let device_response = DeviceResponse::from_device_and_user(device, user, &mut conn);
            HttpResponse::Ok().json(device_response)
        },
        Err(e) => {
            match e {
                Error::NotFound => HttpResponse::NotFound().json(format!("Device {} not found", device_id)),
                _ => {
                    eprintln!("Database error getting device {}: {:?}", device_id, e);
                    HttpResponse::InternalServerError().json(format!("Failed to get device {}", device_id))
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
            eprintln!("Error getting devices for user {}: {:?}", user_uuid_str, e);
            HttpResponse::InternalServerError().json(format!("Failed to get devices for user {}", user_uuid_str))
        }
    }
}

/// Create a new device
pub async fn create_device(
    pool: web::Data<Pool>,
    device: web::Json<NewDevice>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(_) => return HttpResponse::InternalServerError().json("Database connection error"),
    };
    
    match repository::create_device(&mut conn, device.into_inner()) {
        Ok(device) => {
            // Get user data if device has a primary user
            let user = device.primary_user_uuid.as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            let device_response = DeviceResponse::from_device_and_user(device, user, &mut conn);
            HttpResponse::Created().json(device_response)
        },
        Err(e) => {
            eprintln!("Database error creating device: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to create device")
        }
    }
}

/// Update a device
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

    // Extract claims from cookie auth middleware for SSE events
    let user_info = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };
    
    let update_data = device_update.into_inner();
    
    // Convert to JSON before the move for SSE broadcasting
    let update_json = serde_json::to_value(&update_data).unwrap_or_default();
    
    match repository::update_device(&mut conn, device_id, update_data) {
        Ok(device) => {
            // Broadcast SSE events for each field that was updated
            if let Some(update_obj) = update_json.as_object() {
                for (key, value) in update_obj {
                    if !value.is_null() {
                        println!("Broadcasting SSE event for device {}: {} = {:?}", device_id, key, value);
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

            let device_response = DeviceResponse::from_device_and_user(device, user, &mut conn);
            HttpResponse::Ok().json(device_response)
        },
        Err(e) => {
            match e {
                Error::NotFound => HttpResponse::NotFound().json(format!("Device {} not found", device_id)),
                _ => {
                    eprintln!("Database error updating device {}: {:?}", device_id, e);
                    HttpResponse::InternalServerError().json(format!("Failed to update device {}", device_id))
                }
            }
        }
    }
}

/// Delete a device
pub async fn delete_device(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
) -> impl Responder {
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
                HttpResponse::NotFound().json(format!("Device {} not found", device_id))
            }
        }
        Err(e) => {
            eprintln!("Database error deleting device {}: {:?}", device_id, e);
            HttpResponse::InternalServerError().json(format!("Failed to delete device {}", device_id))
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
            eprintln!("Error getting paginated devices: {:?}", e);
            HttpResponse::InternalServerError().json("Failed to get devices")
        }
    }
} 