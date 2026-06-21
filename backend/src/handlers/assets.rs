use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::utils;
use crate::utils::i18n;
use crate::utils::locale::request_locale;
use actix_web::{http::header, web, HttpRequest, HttpResponse, Responder};
use diesel::result::Error;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error};
use uuid::Uuid;

use crate::models::{Asset, AssetUpdate, Group, NewAsset, User};
use crate::repository;
use crate::repository::groups as groups_repo;
use crate::services::assets::{validate_for_kind, AssetValidationError, SYNC_OWNED_ATTRIBUTE_KEYS};
use crate::services::imports::assets::write_assets_csv;
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
    /// Comma-separated lifecycle statuses (`in_service`, `in_repair`, …).
    status: Option<String>,
    warranty: Option<String>,
    location: Option<String>,
    /// Restrict the page to assets whose on-hand quantity is at
    /// or below their `low_stock_threshold`. Accepts the strings
    /// `"true"` / `"1"`; anything else is treated as off so the
    /// param can be omitted without an explicit `false`.
    #[serde(rename = "lowStock")]
    low_stock: Option<String>,
}

/// Query params for `GET /api/assets/export`. Accepts the same
/// filter keys as the paginated list so "export what I am
/// looking at" works without pagination/sort fields.
#[derive(Deserialize)]
pub struct AssetExportQuery {
    format: Option<String>,
    search: Option<String>,
    status: Option<String>,
    warranty: Option<String>,
    location: Option<String>,
    #[serde(rename = "lowStock")]
    low_stock: Option<String>,
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

#[derive(Debug, Serialize)]
pub struct AssetLocationResponse {
    pub location: String,
    pub asset_count: i64,
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
    pub status: String,
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
    /// Linked catalog model, or null for a model-less asset.
    pub model_id: Option<i32>,
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
        let model_id = device.model_id;

        Self {
            id: device.id,
            name: device.name,
            kind: device.kind,
            attributes: device.attributes,
            serial_number: device.serial_number.unwrap_or_default(),
            model: device.model.unwrap_or_default(),
            manufacturer: device.manufacturer,
            location: device.location,
            status: device.status,
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
            model_id,
            primary_user: user.map(|u| {
                let name = u.name.clone();
                let role = repository::user_helpers::workspace_role(conn, u.uuid)
                    .map(|r| r.as_str().to_string())
                    .unwrap_or_else(|| crate::models::WorkspaceRole::Member.as_str().to_string());

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
    mut tc: TenantConn,
    _auth: AuthContext,
    query: web::Query<CalendarOverlayParams>,
) -> impl Responder {
    use chrono::NaiveDate;

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
    let rows: Result<Vec<WarrantyOverlayRow>, Error> = tc.run(|conn| {
        use diesel::RunQueryDsl;
        diesel::sql_query(
            "SELECT id, name, NULLIF(attributes->>'warranty_end_date','')::date AS warranty_end_date \
             FROM assets \
             WHERE NULLIF(attributes->>'warranty_end_date','')::date BETWEEN $1 AND $2",
        )
        .bind::<diesel::sql_types::Date, _>(start)
        .bind::<diesel::sql_types::Date, _>(end)
        .load(conn)
    });

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
pub async fn asset_planner(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    use crate::services::assets::it_attrs;

    // The planner's axes (OS family, warranty bucket, compliance)
    // only make sense for IT-managed hardware. Filter the asset
    // set to kinds whose category is 'it' so non-IT workspaces
    // (vehicles, licenses, materials) stay out of the planner
    // view entirely instead of cluttering it with empty buckets.
    let it_slugs: Vec<String> = match tc.run(|conn| {
        use crate::schema::asset_kinds;
        use diesel::prelude::*;
        asset_kinds::table
            .filter(asset_kinds::category.eq("it"))
            .select(asset_kinds::slug)
            .load(conn)
    }) {
        Ok(s) => s,
        Err(e) => {
            error!(error = ?e, "asset planner: failed to load IT-kind slugs");
            return errors::internal("Failed to load assets");
        }
    };

    let rows: Result<Vec<Asset>, Error> = tc.run(|conn| {
        use crate::schema::assets;
        use diesel::prelude::*;
        assets::table
            .filter(assets::kind.eq_any(&it_slugs))
            .order(assets::name.asc())
            .load(conn)
    });

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
pub async fn get_all_devices(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    let result = tc.run(|conn| {
        let devices = repository::get_all_devices(conn)?;
        Ok(devices_to_responses(conn, devices))
    });

    match result {
        Ok(device_responses) => HttpResponse::Ok().json(device_responses),
        Err(e) => {
            error!(error = ?e, "Database error getting all devices");
            errors::internal("Failed to get devices")
        }
    }
}

/// `GET /api/assets/export` — CSV download of workspace assets.
/// Honors the same filters as the paginated list.
///
/// **Workspace isolation.** Row scoping does NOT come from a
/// handler-level `WorkspaceContext` parameter — it comes from the
/// same path as `get_paginated_devices`: auth middleware pins the
/// request actor to the resolved workspace (`RequestContext.actor
/// .workspace_id`), `TenantConn::run` sets the `app.workspace_id`
/// Postgres GUC inside a transaction, and the `assets_workspace_
/// isolation` RLS policy filters rows. No manual `WHERE workspace_id
/// = …` in the repository; see `list_for_export`'s doc comment.
/// If the actor is not workspace-pinned we fail closed (403) rather
/// than running an unscoped query (RLS would return zero rows, but
/// an explicit gate makes the contract obvious).
///
/// Access: any authenticated workspace member, not technician-
/// gated. Deliberate product choice: assets are workspace-readable
/// (same gate as the list/detail views), and export is a bulk read
/// of data the caller can already page through. Tighten here only
/// if assets later become role-restricted at the list layer.
pub async fn export_assets(
    mut tc: TenantConn,
    _auth: AuthContext,
    query: web::Query<AssetExportQuery>,
) -> impl Responder {
    let Some(workspace_id) = tc.workspace_id() else {
        return errors::forbidden("Export requires a resolved workspace context for this request");
    };

    let format = query.format.as_deref().unwrap_or("csv");
    if format != "csv" {
        return errors::bad_request("Only format=csv is supported");
    }

    let filters = repository::assets::AssetListFilters {
        search: query.search.as_deref(),
        status: query.status.as_deref(),
        warranty: query.warranty.as_deref(),
        location: query.location.as_deref(),
        low_stock_only: matches!(query.low_stock.as_deref(), Some("true") | Some("1")),
    };

    let export_result = tc.run(|conn| {
        let rows = repository::assets::list_for_export(conn, filters)?;
        let slug = repository::workspaces::find_by_id(conn, workspace_id)
            .ok()
            .flatten()
            .map(|w| w.slug)
            .unwrap_or_else(|| "workspace".to_string());
        Ok((rows, slug))
    });

    let (rows, slug) = match export_result {
        Ok(v) => v,
        Err(e) => {
            error!(error = ?e, "Database error exporting assets");
            return errors::internal("Failed to export assets");
        }
    };

    let body = match write_assets_csv(&rows) {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(error = ?e, "Failed to serialize asset export CSV");
            return errors::internal("Failed to build asset export");
        }
    };

    let date = chrono::Utc::now().format("%Y%m%d");
    let filename = format!("assets-{}-{}.csv", slug, date);
    HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "text/csv; charset=utf-8"))
        .insert_header((
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ))
        .body(body)
}

/// Get distinct non-empty asset locations for filters and form suggestions.
pub async fn get_asset_locations(mut tc: TenantConn, _auth: AuthContext) -> impl Responder {
    match tc.run(crate::repository::assets::list_asset_locations) {
        Ok(locations) => HttpResponse::Ok().json(
            locations
                .into_iter()
                .map(|row| AssetLocationResponse {
                    location: row.location,
                    asset_count: row.asset_count,
                })
                .collect::<Vec<_>>(),
        ),
        Err(e) => {
            error!(error = ?e, "Database error getting asset locations");
            errors::internal("Failed to get asset locations")
        }
    }
}

// Get paginated devices
pub async fn get_paginated_devices(
    mut tc: TenantConn,
    _auth: AuthContext,
    query: web::Query<PaginationParams>,
) -> impl Responder {
    let page = query.page.unwrap_or(1).max(1);
    let page_size = query.page_size.unwrap_or(25).clamp(1, 100);
    let sort_field = query.sort_field.clone();
    let sort_direction = query.sort_direction.clone();
    let search = query.search.clone();
    let status = query.status.clone();
    let warranty = query.warranty.clone();
    let location = query.location.clone();
    let low_stock = matches!(query.low_stock.as_deref(), Some("true") | Some("1"));

    let result = tc.run(|conn| {
        let (devices, total) = repository::get_paginated_devices(
            conn,
            page,
            page_size,
            sort_field,
            sort_direction,
            search,
            status,
            warranty,
            location,
            low_stock,
        )?;
        let device_responses = devices_to_responses(conn, devices);
        Ok((device_responses, total))
    });

    match result {
        Ok((device_responses, total)) => {
            let total_pages = (total as f64 / page_size as f64).ceil() as i64;
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
pub async fn get_device_by_id(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let device_id = path.into_inner();

    let result = tc.run(|conn| {
        let device = repository::get_device_by_id(conn, device_id)?;
        let user = device
            .primary_user_uuid
            .as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        let groups = groups_repo::get_groups_for_device(conn, device_id).unwrap_or_default();
        debug!(
            device_id,
            group_count = groups.len(),
            "Fetched groups for device"
        );
        Ok(AssetResponse::from_device_and_user(
            device, user, groups, conn,
        ))
    });

    match result {
        Ok(device_response) => HttpResponse::Ok().json(device_response),
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
pub async fn get_user_devices(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<String>,
) -> impl Responder {
    let user_uuid_str = path.into_inner();

    // Parse UUID from string
    let user_uuid = match utils::parse_uuid(&user_uuid_str) {
        Ok(uuid) => uuid,
        Err(_) => return errors::bad_request("Invalid UUID format"),
    };

    let result = tc.run(|conn| {
        let devices = crate::repository::assets::get_devices_for_user(conn, &user_uuid)?;
        Ok(devices_to_responses(conn, devices))
    });

    match result {
        Ok(device_responses) => HttpResponse::Ok().json(device_responses),
        Err(e) => {
            error!(user_uuid = %user_uuid_str, error = ?e, "Error getting devices for user");
            errors::internal(format!("Failed to get devices for user {user_uuid_str}"))
        }
    }
}

/// Create a new device (technician or admin only)
pub async fn create_device(
    mut tc: TenantConn,
    auth: AuthContext,
    device: web::Json<NewAsset>,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can create devices",
        );
    }

    let new_device = device.into_inner();

    // Validate kind/attributes coherence. Mirror the tickets-style
    // nested Result pattern so the inner validator's HttpResponse
    // bubbles cleanly while still running inside the txn.
    let kind = new_device.kind.clone();
    let attributes = new_device.attributes.clone();
    let validation: Result<Result<(), AssetValidationError>, diesel::result::Error> =
        tc.run(|conn| Ok(validate_for_kind(conn, &kind, &attributes)));
    match validation {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return asset_validation_response(e),
        Err(_) => return errors::internal("Failed to validate asset"),
    }

    let result = tc.run(|conn| {
        let device = repository::create_device(conn, new_device)?;
        let user = device
            .primary_user_uuid
            .as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        // Newly created device has no groups yet
        let device_response =
            AssetResponse::from_device_and_user(device.clone(), user, vec![], conn);
        Ok((device, device_response))
    });

    match result {
        Ok((device, device_response)) => {
            // Index the new device in search
            indexing_tasks::spawn_index_device(search_service.get_ref().clone(), device);

            // The new asset reaches clients through the sync pool (the
            // repository write emits `asset.created`); no discrete SSE.
            HttpResponse::Created().json(device_response)
        }
        Err(e) => {
            error!(error = ?e, "Database error creating device");
            errors::internal("Failed to create device")
        }
    }
}

/// Mint an empty asset and return it (technician or admin only).
///
/// Mirrors `POST /tickets/empty`: creation is a one-click action that
/// drops the user onto the asset's detail page, where they fill in the
/// name, type, and any optional properties inline. The row starts as a
/// `generic` kind with a placeholder name; everything else is added on
/// the detail surface, so there is no separate create form.
pub async fn create_empty_device(
    mut tc: TenantConn,
    auth: AuthContext,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can create assets",
        );
    }

    let new_device = NewAsset {
        name: "New asset".to_string(),
        serial_number: None,
        manufacturer: None,
        model: None,
        location: None,
        notes: None,
        primary_user_uuid: None,
        purchase_date: None,
        asset_tag: None,
        kind: "generic".to_string(),
        attributes: serde_json::json!({}),
        quantity: None,
        unit: None,
        external_sync_source: None,
        low_stock_threshold: None,
    };

    let result = tc.run(|conn| {
        let device = repository::create_device(conn, new_device)?;
        let device_response =
            AssetResponse::from_device_and_user(device.clone(), None, vec![], conn);
        Ok((device, device_response))
    });

    match result {
        Ok((device, device_response)) => {
            indexing_tasks::spawn_index_device(search_service.get_ref().clone(), device);
            HttpResponse::Created().json(device_response)
        }
        Err(e) => {
            error!(error = ?e, "Database error creating empty asset");
            errors::internal("Failed to create asset")
        }
    }
}

/// Rebuild a sync-owned asset's attributes from the existing row,
/// overlaying only the user-owned keys from `incoming`. Sync-owned keys
/// keep their existing values no matter what the client sent, so a
/// synced asset can never have its Graph-managed fields changed here.
fn overlay_user_attributes(
    existing: &serde_json::Value,
    incoming: Option<&serde_json::Value>,
) -> serde_json::Value {
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    if let Some(obj) = incoming.and_then(|v| v.as_object()) {
        for (key, value) in obj {
            if !SYNC_OWNED_ATTRIBUTE_KEYS.contains(&key.as_str()) {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    serde_json::Value::Object(merged)
}

/// Update a device (technician or admin only)
pub async fn update_device(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    device_update: web::Json<AssetUpdate>,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    let device_id = path.into_inner();

    // Check role - only technicians and admins can update devices
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can update devices",
        );
    }

    // Check if device is editable (not synced from Microsoft Graph)
    let existing_device = match tc.run(|conn| repository::get_device_by_id(conn, device_id)) {
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

    // Assets owned by an external sync (Intune / Entra) are managed by
    // Microsoft Graph: their columns and sync-owned attribute keys are
    // read-only here. We still allow edits to user-owned attribute keys
    // (e.g. warranty) that the sync never writes. To keep that safe no
    // matter what the client sends, re-scope a synced asset's update to
    // attributes only, rebuilt from the existing row with just the
    // non-sync keys overlaid.
    let mut update_data = if existing_device.external_sync_source.is_some() {
        let incoming = device_update.into_inner();
        AssetUpdate {
            attributes: Some(overlay_user_attributes(
                &existing_device.attributes,
                incoming.attributes.as_ref(),
            )),
            ..Default::default()
        }
    } else {
        device_update.into_inner()
    };

    // Model assignment goes through POST /assets/{id}/model, which stamps
    // the manufacturer/model/kind/specs. Never let model_id ride the
    // generic update un-stamped.
    update_data.model_id = None;

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
        let validation: Result<Result<(), AssetValidationError>, diesel::result::Error> =
            tc.run(|conn| {
                Ok(validate_for_kind(
                    conn,
                    &effective_kind,
                    &effective_attributes,
                ))
            });
        match validation {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return asset_validation_response(e),
            Err(_) => return errors::internal("Failed to validate asset"),
        }
    }

    let result = tc.run(|conn| {
        let device = repository::update_device(conn, device_id, update_data)?;
        let user = device
            .primary_user_uuid
            .as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        let groups = groups_repo::get_groups_for_device(conn, device_id).unwrap_or_default();
        let device_response =
            AssetResponse::from_device_and_user(device.clone(), user, groups, conn);
        Ok((device, device_response))
    });

    match result {
        Ok((device, device_response)) => {
            // Re-index the updated device in search
            indexing_tasks::spawn_index_device(search_service.get_ref().clone(), device);

            // The update reaches clients through the sync pool (the
            // repository write emits `asset.updated`); no discrete SSE.

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
    mut tc: TenantConn,
    auth: AuthContext,
    search_service: web::Data<Arc<SearchService>>,
    path: web::Path<i32>,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Forbidden: Only administrators can delete devices");
    }

    let device_id = path.into_inner();

    match tc.run(|conn| repository::delete_device(conn, device_id, Some(search_service.get_ref())))
    {
        Ok(rows_affected) => {
            if rows_affected > 0 {
                // Search index removal is fired by the
                // AssetDeletedObserver inside `delete_device`. The
                // deletion reaches clients through the sync pool (the
                // repository write emits `asset.deleted`); no discrete
                // SSE.

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
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let device_id = path.into_inner();

    // Only admins can unmanage devices
    if !auth.is_workspace_admin() {
        return errors::forbidden("Forbidden: Only administrators can unmanage devices");
    }

    // Check if device exists
    let existing_device = match tc.run(|conn| repository::get_device_by_id(conn, device_id)) {
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

    let result = tc.run(|conn| {
        let device = repository::update_device(conn, device_id, update_data)?;
        let user = device
            .primary_user_uuid
            .as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        let groups = groups_repo::get_groups_for_device(conn, device_id).unwrap_or_default();
        Ok(AssetResponse::from_device_and_user(
            device, user, groups, conn,
        ))
    });

    match result {
        Ok(device_response) => HttpResponse::Ok().json(device_response),
        Err(e) => {
            error!(device_id, error = ?e, "Database error unmanaging device");
            errors::internal(format!("Failed to unmanage device {device_id}"))
        }
    }
}

/// Get paginated devices excluding specific IDs
pub async fn get_paginated_devices_excluding(
    mut tc: TenantConn,
    _auth: AuthContext,
    query: web::Query<PaginationParams>,
    exclude_query: web::Query<HashMap<String, String>>,
) -> impl Responder {
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

    let search = query.search.clone();

    let result = tc.run(|conn| {
        let (devices, total_count) =
            crate::repository::assets::get_paginated_devices_excluding_ids(
                conn,
                page,
                page_size,
                search.as_deref(),
                &exclude_ids,
            )?;
        let device_responses = devices_to_responses(conn, devices);
        Ok((device_responses, total_count))
    });

    match result {
        Ok((device_responses, total_count)) => {
            let total_pages = ((total_count as f64) / (page_size as f64)).ceil() as i64;
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
    mut tc: TenantConn,
    auth: AuthContext,
    search_service: web::Data<Arc<SearchService>>,
    body: web::Json<BulkDeviceActionRequest>,
) -> impl Responder {
    // Only admins can perform bulk operations
    if !auth.is_workspace_admin() {
        return errors::forbidden(
            "Forbidden: Only administrators can perform bulk device operations",
        );
    }

    let action = body.action.as_str();
    let ids = &body.ids;

    if ids.is_empty() {
        return errors::bad_request("Bad Request: No device IDs provided");
    }

    match action {
        "delete" => {
            let mut deleted = 0;
            for id in ids {
                let id = *id;
                let search = search_service.get_ref().clone();
                match tc.run(|conn| repository::delete_device(conn, id, Some(&search))) {
                    Ok(rows) => {
                        deleted += rows;
                        // Search index removal is fired by the
                        // AssetDeletedObserver inside `delete_device`. The
                        // deletion reaches clients through the sync pool
                        // (`asset.deleted`); no discrete SSE.
                    }
                    Err(e) => {
                        error!(device_id = id, error = ?e, "Error deleting device in bulk operation");
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

// === Asset model catalog: stamp-on-assignment ================

#[derive(Debug, serde::Deserialize)]
pub struct SetModelBody {
    pub model_id: i32,
}

/// Merge a model's `default_attributes` onto an asset's attributes
/// without clobbering values the asset already carries (per-unit data
/// wins over a model default). A key counts as empty when it is absent,
/// null, or an empty string.
fn stamp_default_attributes(
    existing: &serde_json::Value,
    defaults: &serde_json::Value,
) -> serde_json::Value {
    let mut merged = existing.as_object().cloned().unwrap_or_default();
    if let Some(obj) = defaults.as_object() {
        for (key, value) in obj {
            let occupied = merged
                .get(key)
                .map(|v| !v.is_null() && v.as_str() != Some(""))
                .unwrap_or(false);
            if !occupied {
                merged.insert(key.clone(), value.clone());
            }
        }
    }
    serde_json::Value::Object(merged)
}

/// `POST /api/assets/{id}/model` — stamp a catalog model onto an asset:
/// copy the model's manufacturer, model name, kind, and default specs
/// (no-clobber) onto the row and link `model_id`. Copy-at-assignment, so
/// later edits to the model never rewrite this asset. Refused on synced
/// assets (Graph owns those fields).
pub async fn set_asset_model(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    body: web::Json<SetModelBody>,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: Only technicians and administrators can edit assets");
    }
    let asset_id = path.into_inner();
    let model_id = body.into_inner().model_id;

    let existing = match tc.run(|conn| repository::get_device_by_id(conn, asset_id)) {
        Ok(d) => d,
        Err(Error::NotFound) => {
            return errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "load asset for model stamp");
            return errors::internal("Failed to load asset");
        }
    };
    if existing.external_sync_source.is_some() {
        return errors::forbidden("Cannot set a model on an asset synced from Microsoft Graph.");
    }

    let model = match tc.run(move |conn| crate::repository::asset_models::get(conn, model_id)) {
        Ok(m) => m,
        Err(Error::NotFound) => {
            return errors::unprocessable_entity(format!("Unknown asset model: {model_id}"))
        }
        Err(e) => {
            error!(model_id, error = ?e, "load model for stamp");
            return errors::internal("Failed to load asset model");
        }
    };
    let manufacturer_id = model.manufacturer_id;
    let manufacturer =
        match tc.run(move |conn| crate::repository::manufacturers::get(conn, manufacturer_id)) {
            Ok(m) => m,
            Err(e) => {
                error!(manufacturer_id, error = ?e, "load manufacturer for stamp");
                return errors::internal("Failed to load manufacturer");
            }
        };

    let merged_attrs = stamp_default_attributes(&existing.attributes, &model.default_attributes);

    // The stamped (kind, attributes) pair must be valid, or we'd write an
    // asset that fails its own kind's schema.
    let kind = model.kind.clone();
    let attrs_for_validation = merged_attrs.clone();
    match tc.run(move |conn| Ok::<_, Error>(validate_for_kind(conn, &kind, &attrs_for_validation)))
    {
        Ok(Ok(())) => {}
        Ok(Err(e)) => return asset_validation_response(e),
        Err(_) => return errors::internal("Failed to validate asset"),
    }

    let update = AssetUpdate {
        manufacturer: Some(manufacturer.name),
        model: Some(model.name.clone()),
        kind: Some(model.kind.clone()),
        attributes: Some(merged_attrs),
        model_id: Some(Some(model_id)),
        updated_at: Some(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };

    apply_asset_update_response(&mut tc, asset_id, update, &search_service)
}

/// `DELETE /api/assets/{id}/model` — unlink the catalog model. The
/// stamped manufacturer/model/kind snapshot stays on the asset; only the
/// link is cleared, so it becomes a model-less one-off.
pub async fn clear_asset_model(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    search_service: web::Data<Arc<SearchService>>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: Only technicians and administrators can edit assets");
    }
    let asset_id = path.into_inner();

    let existing = match tc.run(|conn| repository::get_device_by_id(conn, asset_id)) {
        Ok(d) => d,
        Err(Error::NotFound) => {
            return errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "load asset for model clear");
            return errors::internal("Failed to load asset");
        }
    };
    if existing.external_sync_source.is_some() {
        return errors::forbidden("Cannot edit an asset synced from Microsoft Graph.");
    }

    let update = AssetUpdate {
        model_id: Some(None),
        updated_at: Some(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };
    apply_asset_update_response(&mut tc, asset_id, update, &search_service)
}

/// Apply an `AssetUpdate` and render the full asset response, reindexing
/// for search. Shared by the model stamp/clear endpoints.
fn apply_asset_update_response(
    tc: &mut TenantConn,
    asset_id: i32,
    update: AssetUpdate,
    search_service: &web::Data<Arc<SearchService>>,
) -> HttpResponse {
    let result = tc.run(|conn| {
        let device = repository::update_device(conn, asset_id, update)?;
        let user = device
            .primary_user_uuid
            .as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        let groups = groups_repo::get_groups_for_device(conn, asset_id).unwrap_or_default();
        let response = AssetResponse::from_device_and_user(device.clone(), user, groups, conn);
        Ok((device, response))
    });
    match result {
        Ok((device, response)) => {
            indexing_tasks::spawn_index_device(search_service.get_ref().clone(), device);
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(asset_id, error = ?e, "apply asset update");
            errors::internal("Failed to update asset")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::overlay_user_attributes;
    use serde_json::json;

    #[test]
    fn overlay_keeps_sync_keys_and_applies_user_keys() {
        // A synced device: sync owns intune_device_id / hostname; the
        // user is editing warranty_status and trying to spoof hostname.
        let existing = json!({
            "intune_device_id": "abc-123",
            "hostname": "LAPTOP-1",
            "warranty_status": "Unknown",
        });
        let incoming = json!({
            "intune_device_id": "HACKED",   // sync-owned -> must be ignored
            "hostname": "HACKED",           // sync-owned -> must be ignored
            "warranty_status": "Active",    // user-owned -> applied
            "warranty_end_date": "2027-01-01", // new user key -> applied
        });

        let merged = overlay_user_attributes(&existing, Some(&incoming));

        assert_eq!(merged["intune_device_id"], "abc-123");
        assert_eq!(merged["hostname"], "LAPTOP-1");
        assert_eq!(merged["warranty_status"], "Active");
        assert_eq!(merged["warranty_end_date"], "2027-01-01");
    }

    #[test]
    fn overlay_with_no_incoming_is_identity() {
        let existing = json!({ "intune_device_id": "abc-123", "warranty_status": "Active" });
        let merged = overlay_user_attributes(&existing, None);
        assert_eq!(merged, existing);
    }

    use super::stamp_default_attributes;

    #[test]
    fn stamp_fills_empty_keys_without_clobbering() {
        // Asset already has a hostname and a blank warranty; the model
        // defaults a warranty and an OS.
        let existing = json!({
            "hostname": "LAPTOP-1",
            "warranty_status": "",
            "os_version": null,
        });
        let defaults = json!({
            "hostname": "MODEL-DEFAULT",   // occupied -> keep the asset's
            "warranty_status": "Active",   // empty string -> fill
            "os_version": "14.0",          // null -> fill
            "operating_system": "macOS",   // absent -> fill
        });

        let merged = stamp_default_attributes(&existing, &defaults);

        assert_eq!(merged["hostname"], "LAPTOP-1");
        assert_eq!(merged["warranty_status"], "Active");
        assert_eq!(merged["os_version"], "14.0");
        assert_eq!(merged["operating_system"], "macOS");
    }
}
