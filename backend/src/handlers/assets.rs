use crate::handlers::errors;
use crate::handlers::helpers;
use crate::utils;
use crate::utils::i18n;
use crate::utils::locale::request_locale;
use crate::utils::rbac::{is_admin, is_technician_or_admin};
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;

use crate::db::Pool;
use crate::models::{Asset, AssetUpdate, Claims, Group, NewAsset, User};
use crate::repository;
use crate::repository::groups as groups_repo;
use crate::services::assets::{validate_for_kind, AssetValidationError};
use crate::services::search::indexing_tasks;
use crate::services::search::SearchService;

/// Map an `AssetValidationError` to a JSON HTTP response. Bad
/// kind slug or invalid attributes are 422 (the request was
/// well-formed but failed semantic validation); database
/// failures bubble up as 500.
fn asset_validation_response(err: AssetValidationError) -> HttpResponse {
    match err {
        AssetValidationError::UnknownKind(slug) => {
            errors::unprocessable_entity(format!("Unknown asset kind: {slug}"))
        }
        AssetValidationError::Attributes(inner) => {
            errors::unprocessable_entity(format!("Invalid asset attributes: {inner}"))
        }
        AssetValidationError::Database(e) => {
            error!(error = ?e, "Database error during asset kind validation");
            errors::internal("Failed to validate asset attributes")
        }
    }
}

/// Extract the SSE client ID from the request header (for echo suppression).
fn extract_sse_client_id(req: &HttpRequest) -> Option<String> {
    req.headers()
        .get("X-SSE-Client-Id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
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

// Asset response shipped over the REST API. Carries the
// universal columns plus the kind + attributes blob so the
// frontend can render kind-specific fields through the
// DynamicAttributeForm. IT-specific fields like hostname /
// warranty / Microsoft Graph IDs all live inside `attributes`
// after Pass B; no top-level keys for them here.
#[derive(Debug, Serialize)]
pub struct AssetResponse {
    pub id: i32,
    pub name: String,
    pub kind: String,
    pub attributes: serde_json::Value,
    pub serial_number: String,
    pub model: String,
    pub manufacturer: Option<String>,
    pub location: Option<String>,
    pub primary_user_uuid: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub purchase_date: Option<String>,
    pub asset_tag: Option<String>,
    pub quantity: Option<String>,
    pub unit: Option<String>,
    pub external_sync_source: Option<String>,
    pub low_stock_threshold: Option<String>,
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

impl AssetResponse {
    pub fn from_device_and_user(
        device: Asset,
        user: Option<User>,
        groups: Vec<Group>,
        conn: &mut crate::db::DbConnection,
    ) -> Self {
        // Editable when the row isn't owned by an external sync.
        // Pass B replaced the column-existence predicate with a
        // dedicated `external_sync_source` column so the answer
        // doesn't depend on a particular Microsoft Graph field.
        let is_editable = device.external_sync_source.is_none();

        Self {
            id: device.id,
            name: device.name,
            kind: device.kind,
            attributes: device.attributes,
            serial_number: device.serial_number.unwrap_or_default(),
            model: device.model.unwrap_or_default(),
            manufacturer: device.manufacturer,
            location: device.location,
            primary_user_uuid: device
                .primary_user_uuid
                .map(|uuid| utils::uuid_to_string(&uuid)),
            created_at: device
                .created_at
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            updated_at: device
                .updated_at
                .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                .to_string(),
            purchase_date: device
                .purchase_date
                .map(|d| d.format("%Y-%m-%d").to_string()),
            asset_tag: device.asset_tag,
            quantity: device.quantity.as_ref().map(|q| q.to_string()),
            unit: device.unit,
            external_sync_source: device.external_sync_source,
            low_stock_threshold: device.low_stock_threshold.as_ref().map(|q| q.to_string()),
            is_editable,
            primary_user: user.map(|u| {
                let name = u.name.clone();
                let role = match u.role {
                    crate::models::UserRole::Admin => "admin",
                    crate::models::UserRole::Technician => "technician",
                    crate::models::UserRole::User => "user",
                }
                .to_string();

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
fn devices_to_responses(
    conn: &mut crate::db::DbConnection,
    devices: Vec<Asset>,
) -> Vec<AssetResponse> {
    devices
        .into_iter()
        .map(|device| {
            let user = device
                .primary_user_uuid
                .as_ref()
                .and_then(|uuid| get_user_by_uuid(conn, uuid));
            let groups = groups_repo::get_groups_for_device(conn, device.id).unwrap_or_default();
            AssetResponse::from_device_and_user(device, user, groups, conn)
        })
        .collect()
}

/// Calendar overlay query: a date window the caller is rendering.
#[derive(Debug, Deserialize)]
pub struct CalendarOverlayParams {
    /// ISO date or datetime, inclusive. Treated as the lower bound
    /// of the visible calendar window.
    pub start: String,
    /// ISO date or datetime, inclusive. Upper bound.
    pub end: String,
}

/// One overlay entry for the calendar. Today only carries warranty
/// expiries; OS support cutoffs and scheduled maintenance get
/// their own `kind` variants once those data sources land.
#[derive(Debug, Serialize)]
pub struct CalendarOverlayEntry {
    pub kind: &'static str,
    pub date: String,
    pub device_id: i32,
    pub device_name: String,
    pub label: String,
}

/// `GET /api/devices/calendar-overlay?start=YYYY-MM-DD&end=YYYY-MM-DD`
///
/// Returns warranty-expiry overlays for the given window. The
/// calendar view fetches one window per visible month and renders
/// each entry as a badge in the day cell so device events are
/// visible alongside ticket due dates.
pub async fn calendar_overlay(
    pool: web::Data<Pool>,
    query: web::Query<CalendarOverlayParams>,
) -> impl Responder {
    use chrono::NaiveDate;
    use diesel::prelude::*;

    let parse = |s: &str| -> Option<NaiveDate> {
        // Accept YYYY-MM-DD or full RFC3339 (drop the time part).
        if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            return Some(d);
        }
        s.split('T')
            .next()
            .and_then(|head| NaiveDate::parse_from_str(head, "%Y-%m-%d").ok())
    };
    let start = match parse(&query.start) {
        Some(d) => d,
        None => return errors::bad_request("start must be an ISO date"),
    };
    let end = match parse(&query.end) {
        Some(d) => d,
        None => return errors::bad_request("end must be an ISO date"),
    };
    if end < start {
        return errors::bad_request("end must be on or after start");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // warranty_end_date moved into the attributes JSONB in
    // Pass B; the calendar overlay reads it via JSON path and
    // ::date-casts so the start/end window comparison stays in
    // SQL. NULLIF guards the empty-string case (jsonb_strip_nulls
    // would normally remove the key, but a hand-written row
    // could leave one in).
    #[derive(diesel::QueryableByName)]
    struct WarrantyOverlayRow {
        #[diesel(sql_type = diesel::sql_types::Int4)]
        id: i32,
        #[diesel(sql_type = diesel::sql_types::Text)]
        name: String,
        #[diesel(sql_type = diesel::sql_types::Date)]
        warranty_end_date: NaiveDate,
    }
    let rows: Result<Vec<WarrantyOverlayRow>, Error> = diesel::sql_query(
        "SELECT id, name, NULLIF(attributes->>'warranty_end_date','')::date AS warranty_end_date \
         FROM assets \
         WHERE NULLIF(attributes->>'warranty_end_date','')::date BETWEEN $1 AND $2",
    )
    .bind::<diesel::sql_types::Date, _>(start)
    .bind::<diesel::sql_types::Date, _>(end)
    .load(&mut conn);

    match rows {
        Ok(rows) => {
            let entries: Vec<CalendarOverlayEntry> = rows
                .into_iter()
                .map(|row| CalendarOverlayEntry {
                    kind: "warranty_expiry",
                    date: row.warranty_end_date.format("%Y-%m-%d").to_string(),
                    device_id: row.id,
                    device_name: row.name.clone(),
                    label: format!("Warranty ends: {}", row.name),
                })
                .collect();
            HttpResponse::Ok().json(entries)
        }
        Err(e) => {
            error!(error = ?e, "Database error loading calendar overlays");
            errors::internal("Failed to load calendar overlays")
        }
    }
}

/// One asset row as the rollout-planner consumes it. Derived
/// fields (os_family / warranty_bucket) are bucketed at the
/// boundary so the renderer can group / filter without a second
/// pass over every row.
#[derive(Debug, Serialize)]
pub struct AssetPlannerRow {
    pub id: i32,
    pub name: String,
    pub hostname: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub operating_system: Option<String>,
    pub os_version: Option<String>,
    /// Bucketed os: 'windows' | 'macos' | 'linux' | 'ios' | 'android' | 'other'.
    pub os_family: &'static str,
    pub warranty_end_date: Option<String>,
    /// 'expired' | 'expiring_30d' | 'expiring_90d' | 'active' | 'unknown'.
    pub warranty_bucket: &'static str,
    pub compliance_state: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub asset_tag: Option<String>,
}

fn classify_os(raw: Option<&str>) -> &'static str {
    let s = raw.unwrap_or("").to_lowercase();
    if s.contains("windows") {
        return "windows";
    }
    if s.contains("mac") || s.contains("os x") || s.contains("darwin") {
        return "macos";
    }
    if s.contains("linux") || s.contains("ubuntu") || s.contains("fedora") || s.contains("debian") {
        return "linux";
    }
    if s.contains("ios") || s.contains("iphone") || s.contains("ipad") {
        return "ios";
    }
    if s.contains("android") {
        return "android";
    }
    "other"
}

fn classify_warranty(end: Option<chrono::NaiveDate>, today: chrono::NaiveDate) -> &'static str {
    let Some(end) = end else {
        return "unknown";
    };
    let days = (end - today).num_days();
    if days < 0 {
        "expired"
    } else if days <= 30 {
        "expiring_30d"
    } else if days <= 90 {
        "expiring_90d"
    } else {
        "active"
    }
}

/// `GET /api/assets/planner` — returns every device shaped for the
/// asset rollout planner view. Bucketing happens server-side so
/// the renderer doesn't repeat the OS-string heuristics.
pub async fn asset_planner(
    pool: web::Data<Pool>,
    _auth: crate::extractors::AuthContext,
) -> impl Responder {
    use crate::schema::assets;
    use diesel::prelude::*;

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // The planner's axes (OS family, warranty bucket, compliance)
    // only make sense for IT-managed hardware. Filter the asset
    // set to kinds whose category is 'it' so non-IT workspaces
    // (vehicles, licenses, materials) stay out of the planner
    // view entirely instead of cluttering it with empty buckets.
    use crate::schema::asset_kinds;
    use crate::services::assets::it_attrs;
    let it_slugs: Vec<String> = match asset_kinds::table
        .filter(asset_kinds::category.eq("it"))
        .select(asset_kinds::slug)
        .load(&mut conn)
    {
        Ok(s) => s,
        Err(e) => {
            error!(error = ?e, "asset planner: failed to load IT-kind slugs");
            return errors::internal("Failed to load assets");
        }
    };

    let rows: Result<Vec<Asset>, Error> = assets::table
        .filter(assets::kind.eq_any(&it_slugs))
        .order(assets::name.asc())
        .load(&mut conn);

    match rows {
        Ok(devices) => {
            let today = chrono::Utc::now().date_naive();
            let payload: Vec<AssetPlannerRow> = devices
                .into_iter()
                .map(|d| {
                    let os = it_attrs::operating_system(&d.attributes).map(str::to_string);
                    let os_version = it_attrs::os_version(&d.attributes).map(str::to_string);
                    let warranty_end = it_attrs::warranty_end_date(&d.attributes);
                    let compliance = it_attrs::compliance_state(&d.attributes).map(str::to_string);
                    let hostname = it_attrs::hostname(&d.attributes).map(str::to_string);
                    AssetPlannerRow {
                        id: d.id,
                        name: d.name.clone(),
                        hostname,
                        manufacturer: d.manufacturer.clone(),
                        model: d.model.clone(),
                        os_family: classify_os(os.as_deref()),
                        warranty_bucket: classify_warranty(warranty_end, today),
                        operating_system: os,
                        os_version,
                        warranty_end_date: warranty_end.map(|dt| dt.format("%Y-%m-%d").to_string()),
                        compliance_state: compliance,
                        primary_user_uuid: d.primary_user_uuid,
                        asset_tag: d.asset_tag,
                    }
                })
                .collect();
            HttpResponse::Ok().json(payload)
        }
        Err(e) => {
            error!(error = ?e, "asset planner load failed");
            errors::internal("Failed to load assets")
        }
    }
}

/// Get all devices
pub async fn get_all_devices(pool: web::Data<Pool>) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::get_all_devices(&mut conn) {
        Ok(devices) => {
            // Convert devices to enhanced responses with user data
            let device_responses = devices_to_responses(&mut conn, devices);
            HttpResponse::Ok().json(device_responses)
        }
        Err(e) => {
            error!(error = ?e, "Database error getting all devices");
            errors::internal("Failed to get devices")
        }
    }
}

// Get paginated devices
pub async fn get_paginated_devices(
    pool: web::Data<Pool>,
    query: web::Query<PaginationParams>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
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
            errors::internal("Failed to get devices")
        }
    }
}

/// Get a single device by ID
pub async fn get_device_by_id(pool: web::Data<Pool>, path: web::Path<i32>) -> impl Responder {
    let device_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::get_device_by_id(&mut conn, device_id) {
        Ok(device) => {
            // Get user data if device has a primary user
            let user = device
                .primary_user_uuid
                .as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            // Get groups for the device
            let groups =
                groups_repo::get_groups_for_device(&mut conn, device_id).unwrap_or_default();
            debug!(
                device_id,
                group_count = groups.len(),
                "Fetched groups for device"
            );

            let device_response =
                AssetResponse::from_device_and_user(device, user, groups, &mut conn);
            HttpResponse::Ok().json(device_response)
        }
        Err(e) => match e {
            Error::NotFound => errors::not_found_msg(format!("Asset {device_id} not found")),
            _ => {
                error!(device_id, error = ?e, "Database error getting device");
                errors::internal(format!("Failed to get device {device_id}"))
            }
        },
    }
}

/// Get devices for a specific user
pub async fn get_user_devices(pool: web::Data<Pool>, path: web::Path<String>) -> impl Responder {
    let user_uuid_str = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Parse UUID from string
    let user_uuid = match utils::parse_uuid(&user_uuid_str) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid UUID format"),
    };

    match crate::repository::assets::get_devices_for_user(&mut conn, &user_uuid) {
        Ok(devices) => {
            let device_responses = devices_to_responses(&mut conn, devices);
            HttpResponse::Ok().json(device_responses)
        }
        Err(e) => {
            error!(user_uuid = %user_uuid_str, error = ?e, "Error getting devices for user");
            errors::internal(format!("Failed to get devices for user {user_uuid_str}"))
        }
    }
}

/// Create a new device (technician or admin only)
pub async fn create_device(
    req: HttpRequest,
    pool: web::Data<Pool>,
    device: web::Json<NewAsset>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_technician_or_admin(&claims) {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can create devices",
        );
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let new_device = device.into_inner();
    if let Err(e) = validate_for_kind(&mut conn, &new_device.kind, &new_device.attributes) {
        return asset_validation_response(e);
    }

    match repository::create_device(&mut conn, new_device) {
        Ok(device) => {
            let device_id = device.id;

            // Index the new device in search
            indexing_tasks::spawn_index_device(search_service.get_ref().clone(), device.clone());

            // Get user data if device has a primary user
            let user = device
                .primary_user_uuid
                .as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            // Newly created device has no groups yet
            let device_response =
                AssetResponse::from_device_and_user(device, user, vec![], &mut conn);

            // Broadcast SSE event for device creation (with echo suppression)
            let source_client_id = extract_sse_client_id(&req);
            sse_state
                .broadcast_event_from(
                    crate::handlers::sse::SseEvent::AssetCreated {
                        device_id,
                        device: serde_json::to_value(&device_response).unwrap_or_default(),
                        timestamp: chrono::Utc::now(),
                    },
                    source_client_id,
                )
                .await;

            HttpResponse::Created().json(device_response)
        }
        Err(e) => {
            error!(error = ?e, "Database error creating device");
            errors::internal("Failed to create device")
        }
    }
}

/// Update a device (technician or admin only)
pub async fn update_device(
    pool: web::Data<Pool>,
    path: web::Path<i32>,
    device_update: web::Json<AssetUpdate>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    search_service: web::Data<Arc<SearchService>>,
    req: HttpRequest,
) -> impl Responder {
    let device_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Extract claims from cookie auth middleware for SSE events and role check
    let user_info = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    // Check role - only technicians and admins can update devices
    if !is_technician_or_admin(&user_info) {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can update devices",
        );
    }

    // Check if device is editable (not synced from Microsoft Graph)
    let existing_device = match repository::get_device_by_id(&mut conn, device_id) {
        Ok(device) => device,
        Err(e) => {
            return match e {
                Error::NotFound => errors::not_found_msg(format!("Asset {device_id} not found")),
                _ => {
                    error!(device_id, error = ?e, "Database error getting device");
                    errors::internal(format!("Failed to get device {device_id}"))
                }
            }
        }
    };

    // Prevent editing assets owned by an external sync.
    if existing_device.external_sync_source.is_some() {
        return errors::forbidden("Cannot edit device synced from Microsoft Graph: This device is managed by Microsoft Intune/Entra and cannot be edited manually. Changes must be made in Microsoft Entra Admin Center or Intune.");
    }

    let update_data = device_update.into_inner();

    // Validate kind/attributes coherence if either is being
    // changed. Updates that only touch IT-desk columns don't
    // hit the registry at all.
    if update_data.kind.is_some() || update_data.attributes.is_some() {
        let effective_kind = update_data
            .kind
            .clone()
            .unwrap_or_else(|| existing_device.kind.clone());
        let effective_attributes = update_data
            .attributes
            .clone()
            .unwrap_or_else(|| existing_device.attributes.clone());
        if let Err(e) = validate_for_kind(&mut conn, &effective_kind, &effective_attributes) {
            return asset_validation_response(e);
        }
    }

    // Convert to JSON before the move for SSE broadcasting
    let update_json = serde_json::to_value(&update_data).unwrap_or_default();

    match repository::update_device(&mut conn, device_id, update_data) {
        Ok(device) => {
            // Re-index the updated device in search
            indexing_tasks::spawn_index_device(search_service.get_ref().clone(), device.clone());

            // Broadcast SSE events for each field that was updated (with echo suppression)
            let source_client_id = extract_sse_client_id(&req);
            if let Some(update_obj) = update_json.as_object() {
                for (key, value) in update_obj {
                    if !value.is_null() {
                        debug!(device_id, field = %key, value = ?value, "Broadcasting SSE event for device update");
                        sse_state
                            .broadcast_event_from(
                                crate::handlers::sse::SseEvent::AssetUpdated {
                                    device_id,
                                    field: key.to_string(),
                                    value: value.clone(),
                                    updated_by: user_info.sub.clone(),
                                    timestamp: chrono::Utc::now(),
                                },
                                source_client_id.clone(),
                            )
                            .await;
                    }
                }
            }

            // Get user data if device has a primary user
            let user = device
                .primary_user_uuid
                .as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            // Get groups for the device
            let groups =
                groups_repo::get_groups_for_device(&mut conn, device_id).unwrap_or_default();

            let device_response =
                AssetResponse::from_device_and_user(device, user, groups, &mut conn);
            HttpResponse::Ok().json(device_response)
        }
        Err(e) => match e {
            Error::NotFound => errors::not_found_msg(format!("Asset {device_id} not found")),
            _ => {
                error!(device_id, error = ?e, "Database error updating device");
                errors::internal(format!("Failed to update device {device_id}"))
            }
        },
    }
}

/// Delete a device (admin only)
pub async fn delete_device(
    req: HttpRequest,
    pool: web::Data<Pool>,
    search_service: web::Data<Arc<SearchService>>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    path: web::Path<i32>,
) -> impl Responder {
    // Extract claims and check role
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    if !is_admin(&claims) {
        return errors::forbidden("Forbidden: Only administrators can delete devices");
    }

    let device_id = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match repository::delete_device(&mut conn, device_id, Some(search_service.get_ref())) {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                // Search index removal is fired by the
                // AssetDeletedObserver inside `delete_device`.

                // Broadcast SSE event for device deletion (with echo suppression)
                let source_client_id = extract_sse_client_id(&req);
                sse_state
                    .broadcast_event_from(
                        crate::handlers::sse::SseEvent::AssetDeleted {
                            device_id,
                            timestamp: chrono::Utc::now(),
                        },
                        source_client_id,
                    )
                    .await;

                HttpResponse::Ok().json(json!({
                    "message": format!("Asset {} deleted successfully", device_id)
                }))
            } else {
                errors::not_found_msg(format!("Asset {device_id} not found"))
            }
        }
        Err(e) => {
            error!(device_id, error = ?e, "Database error deleting device");
            errors::internal(format!("Failed to delete device {device_id}"))
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
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Extract claims from cookie auth middleware and check role
    let user_info = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    // Only admins can unmanage devices
    if !is_admin(&user_info) {
        return errors::forbidden("Forbidden: Only administrators can unmanage devices");
    }

    // Check if device exists
    let existing_device = match repository::get_device_by_id(&mut conn, device_id) {
        Ok(device) => device,
        Err(e) => {
            return match e {
                Error::NotFound => errors::not_found_msg(format!("Asset {device_id} not found")),
                _ => {
                    error!(device_id, error = ?e, "Database error getting device");
                    errors::internal(format!("Failed to get device {device_id}"))
                }
            }
        }
    };

    // Asset must be sync-owned to unmanage; otherwise it's
    // already manually editable. `external_sync_source` is the
    // post-Pass-B replacement for the column-existence predicate.
    if existing_device.external_sync_source.is_none() {
        return errors::bad_request("Asset is not managed by Microsoft Graph: This device is already manually managed and doesn't need to be unmanaged.");
    }

    // Clearing `external_sync_source` flips the asset back to
    // manually-managed. The IT attribute keys (intune_device_id,
    // entra_device_id, microsoft_device_id) stay in attributes
    // as historical breadcrumbs; the admin can edit them through
    // the asset form like any other attribute.
    let update_data = crate::models::AssetUpdate {
        external_sync_source: Some(None),
        ..Default::default()
    };

    match repository::update_device(&mut conn, device_id, update_data) {
        Ok(device) => {
            // Get user data if device has a primary user
            let user = device
                .primary_user_uuid
                .as_ref()
                .and_then(|uuid| get_user_by_uuid(&mut conn, uuid));

            // Get groups for the device
            let groups =
                groups_repo::get_groups_for_device(&mut conn, device_id).unwrap_or_default();

            let device_response =
                AssetResponse::from_device_and_user(device, user, groups, &mut conn);
            HttpResponse::Ok().json(device_response)
        }
        Err(e) => {
            error!(device_id, error = ?e, "Database error unmanaging device");
            errors::internal(format!("Failed to unmanage device {device_id}"))
        }
    }
}

/// Get paginated devices excluding specific IDs
pub async fn get_paginated_devices_excluding(
    pool: web::Data<Pool>,
    query: web::Query<PaginationParams>,
    exclude_query: web::Query<HashMap<String, String>>,
) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(20).clamp(1, 100);

    // Parse exclude_ids from comma-separated string
    let exclude_ids: Vec<i32> = exclude_query
        .get("excludeIds")
        .map(|ids_str| {
            ids_str
                .split(',')
                .filter_map(|id| id.trim().parse::<i32>().ok())
                .collect()
        })
        .unwrap_or_default();

    match crate::repository::assets::get_paginated_devices_excluding_ids(
        &mut conn,
        page,
        page_size,
        query.search.as_deref(),
        &exclude_ids,
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
        }
        Err(e) => {
            error!(error = ?e, "Error getting paginated devices");
            errors::internal("Failed to get devices")
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
    search_service: web::Data<Arc<SearchService>>,
    sse_state: web::Data<crate::handlers::sse::SseState>,
    body: web::Json<BulkDeviceActionRequest>,
) -> impl Responder {
    // Extract claims and check authentication
    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Unauthorized: Authentication required"),
    };

    // Only admins can perform bulk operations
    if !is_admin(&claims) {
        return errors::forbidden(
            "Forbidden: Only administrators can perform bulk device operations",
        );
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let action = body.action.as_str();
    let ids = &body.ids;

    if ids.is_empty() {
        return errors::bad_request("Bad Request: No device IDs provided");
    }

    match action {
        "delete" => {
            let mut deleted = 0;
            let source_client_id = extract_sse_client_id(&req);
            for id in ids {
                match repository::delete_device(&mut conn, *id, Some(search_service.get_ref())) {
                    Ok(rows) => {
                        deleted += rows;
                        // Search index removal is fired by the
                        // AssetDeletedObserver inside `delete_device`.

                        // Broadcast SSE event for each deleted device
                        sse_state
                            .broadcast_event_from(
                                crate::handlers::sse::SseEvent::AssetDeleted {
                                    device_id: *id,
                                    timestamp: chrono::Utc::now(),
                                },
                                source_client_id.clone(),
                            )
                            .await;
                    }
                    Err(e) => {
                        error!(device_id = *id, error = ?e, "Error deleting device in bulk operation");
                    }
                }
            }

            HttpResponse::Ok().json(json!({ "affected": deleted }))
        }

        _ => HttpResponse::BadRequest().json(json!({
            "error": i18n::tr(&request_locale(&req), "backend-error-bad-request"),
            "code": "backend-error-bad-request",
            "message": format!("Unknown action: {}", action)
        })),
    }
}
