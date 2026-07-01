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
use tracing::{debug, error, warn};
use uuid::Uuid;

use crate::models::{Asset, AssetGroupRef, AssetUpdate, Group, NewAsset, User};
use crate::repository;
use crate::repository::groups as groups_repo;
use crate::services::assets::{validate_for_kind, AssetValidationError, SYNC_OWNED_ATTRIBUTE_KEYS};
use crate::services::imports::assets::{write_assets_csv, write_history_csv};
use crate::services::search::indexing_tasks;
use crate::services::search::SearchService;

/// Map an `AssetValidationError` to a JSON HTTP response. Bad
/// kind slug or invalid attributes are 422 (the request was
/// well-formed but failed semantic validation); database
/// failures bubble up as 500.
fn asset_validation_response(err: AssetValidationError) -> HttpResponse {
    match err {
        AssetValidationError::UnknownKind(slug) => {
            warn!(kind = %slug, "Asset write rejected: unknown asset kind");
            errors::unprocessable_entity(format!("Unknown asset kind: {slug}"))
        }
        AssetValidationError::Attributes(inner) => {
            warn!(error = %inner, "Asset write rejected: invalid attributes");
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
    /// Comma-separated native asset-group ids; matches assets in ANY of them.
    groups: Option<String>,
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
    groups: Option<String>,
    #[serde(rename = "lowStock")]
    low_stock: Option<String>,
    /// `snapshot` (default) exports current-state rows; `history` exports the
    /// lifecycle event log (one row per transition, ticket-correlated).
    scope: Option<String>,
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
    pub managed_by_user_uuid: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub purchase_date: Option<String>,
    pub asset_tag: Option<String>,
    pub quantity: Option<String>,
    pub unit: Option<String>,
    pub external_sync_source: Option<String>,
    pub low_stock_threshold: Option<String>,
    pub primary_user: Option<UserInfo>,
    /// Directory-group memberships (Intune/Entra-synced or manual).
    pub groups: Vec<GroupInfo>,
    /// Native asset groups this asset belongs to. Populated by the list and
    /// detail paths; empty elsewhere (callers that don't enrich).
    pub asset_groups: Vec<AssetGroupRef>,
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
            managed_by_user_uuid: device
                .managed_by_user_uuid
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
            // Enriched by the list / detail callers; empty by default.
            asset_groups: Vec::new(),
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
    // Batch the native-group lookup for the whole page (one query) rather than
    // per row.
    let ids: Vec<i32> = devices.iter().map(|d| d.id).collect();
    let mut native_groups =
        crate::repository::asset_groups::group_refs_for_assets(conn, &ids).unwrap_or_default();
    devices
        .into_iter()
        .map(|device| {
            let user = device
                .primary_user_uuid
                .as_ref()
                .and_then(|uuid| get_user_by_uuid(conn, uuid));
            let groups = groups_repo::get_groups_for_device(conn, device.id).unwrap_or_default();
            let asset_groups = native_groups.remove(&device.id).unwrap_or_default();
            let mut resp = AssetResponse::from_device_and_user(device, user, groups, conn);
            resp.asset_groups = asset_groups;
            resp
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

use crate::services::assets::bucketing::{classify_os, classify_warranty_window};

/// A full asset row augmented with the server-derived planning buckets.
/// Flattens `AssetResponse` so the inventory list renders the same
/// columns it always does, and adds `os_family` / `warranty_window` so
/// the client groups by a stable key rather than re-deriving the OS /
/// warranty heuristics. Compliance grouping reads
/// `attributes.compliance_state` directly (already a categorical value),
/// so it needs no derived field.
#[derive(Debug, Serialize)]
pub struct AssetGroupingRow {
    #[serde(flatten)]
    pub asset: AssetResponse,
    /// 'windows' | 'macos' | 'linux' | 'ios' | 'android' | 'other'.
    pub os_family: &'static str,
    /// 'expired' | 'expiring_30d' | 'expiring_90d' | 'active' | 'unknown'.
    pub warranty_window: &'static str,
}

/// `GET /api/assets/grouping-dataset` — the complete set of assets
/// matching the current inventory list filters, each tagged with the
/// derived planning buckets. The inventory list switches to this source
/// when a planning axis (OS family / warranty window / compliance) is
/// active so group counts and "select all in a bucket" are fleet-true
/// rather than reflecting only the rows scrolled into view. Same filter
/// keys as the paginated list; RLS scopes rows to the workspace.
pub async fn asset_grouping_dataset(
    mut tc: TenantConn,
    query: web::Query<AssetExportQuery>,
    auth: AuthContext,
) -> impl Responder {
    use crate::services::assets::it_attrs;

    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }

    let filters = repository::assets::AssetListFilters {
        search: query.search.as_deref(),
        warranty: query.warranty.as_deref(),
        location: query.location.as_deref(),
        status: query.status.as_deref(),
        groups: query.groups.as_deref(),
        low_stock_only: matches!(query.low_stock.as_deref(), Some("true") | Some("1")),
    };

    let result = tc.run(move |conn| {
        let devices = repository::assets::list_for_export(conn, filters)?;
        let today = chrono::Utc::now().date_naive();
        // Derive the buckets from each device's attributes before the
        // response builder consumes the row.
        let rows: Vec<AssetGroupingRow> = devices
            .into_iter()
            .map(|d| {
                let os_family = classify_os(it_attrs::operating_system(&d.attributes));
                let warranty_window =
                    classify_warranty_window(it_attrs::warranty_end_date(&d.attributes), today);
                let user = d
                    .primary_user_uuid
                    .as_ref()
                    .and_then(|uuid| get_user_by_uuid(conn, uuid));
                let groups = groups_repo::get_groups_for_device(conn, d.id).unwrap_or_default();
                let asset = AssetResponse::from_device_and_user(d, user, groups, conn);
                AssetGroupingRow {
                    asset,
                    os_family,
                    warranty_window,
                }
            })
            .collect();
        Ok::<_, Error>(rows)
    });

    match result {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(e) => {
            error!(error = ?e, "asset grouping dataset load failed");
            errors::internal("Failed to load assets")
        }
    }
}

/// Request body for `POST /api/assets/rollouts`. Mints a rollout project
/// and one ticket per selected device, each ticket linked to its asset.
#[derive(Debug, Deserialize)]
pub struct CreateRolloutBody {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Initial workflow state every rollout ticket starts in.
    pub workflow_state_id: i32,
    #[serde(default)]
    pub priority: Option<crate::models::TicketPriority>,
    /// Exact device ids to roll out. The client selects these from the
    /// complete grouping dataset, so the set is authoritative.
    pub asset_ids: Vec<i32>,
}

#[derive(Debug, Serialize)]
pub struct CreateRolloutResponse {
    pub project_id: i32,
    pub ticket_count: usize,
}

/// `POST /api/assets/rollouts` — the planner-to-projects handoff. Creates
/// a project, then for every selected asset a ticket (linked to the
/// project and to the asset) in one transaction, so a partial failure
/// never leaves a half-built rollout. Agent-gated like ticket creation.
pub async fn create_rollout(
    mut tc: TenantConn,
    body: web::Json<CreateRolloutBody>,
    auth: AuthContext,
) -> impl Responder {
    use crate::services::assets::rollout::{self, RolloutSpec};

    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let body = body.into_inner();

    let name = body.name.trim().to_string();
    if name.is_empty() || name.len() > 255 {
        return errors::bad_request("name must be 1 to 255 characters");
    }
    if body.asset_ids.is_empty() {
        return errors::bad_request("Select at least one device for the rollout");
    }
    // Bound the batch so a runaway selection can't open thousands of
    // tickets in one synchronous request.
    const MAX_ROLLOUT_DEVICES: usize = 500;
    if body.asset_ids.len() > MAX_ROLLOUT_DEVICES {
        return errors::bad_request(format!(
            "A rollout can cover at most {MAX_ROLLOUT_DEVICES} devices at once"
        ));
    }

    // Dedup while preserving order so a doubled id doesn't double-ticket.
    let mut seen = std::collections::HashSet::new();
    let asset_ids: Vec<i32> = body
        .asset_ids
        .into_iter()
        .filter(|id| seen.insert(*id))
        .collect();

    let spec = RolloutSpec {
        name,
        description: body
            .description
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        workflow_state_id: body.workflow_state_id,
        priority: body.priority.unwrap_or_default(),
        asset_ids,
    };

    let result = tc.run(move |conn| rollout::create_rollout(conn, spec));

    match result {
        Ok(r) => HttpResponse::Created().json(CreateRolloutResponse {
            project_id: r.project_id,
            ticket_count: r.ticket_count,
        }),
        Err(Error::DatabaseError(diesel::result::DatabaseErrorKind::ForeignKeyViolation, _)) => {
            errors::bad_request("Unknown workflow state")
        }
        Err(e) => {
            error!(error = ?e, "rollout creation failed");
            errors::internal("Failed to create rollout")
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
    let want_history = query.scope.as_deref() == Some("history");

    let filters = repository::assets::AssetListFilters {
        search: query.search.as_deref(),
        status: query.status.as_deref(),
        warranty: query.warranty.as_deref(),
        location: query.location.as_deref(),
        groups: query.groups.as_deref(),
        low_stock_only: matches!(query.low_stock.as_deref(), Some("true") | Some("1")),
    };

    let export_result = tc.run(|conn| {
        let rows = repository::assets::list_for_export(conn, filters)?;
        // History exports the lifecycle log for the same filtered assets.
        let events = if want_history {
            let ids: Vec<i32> = rows.iter().map(|a| a.id).collect();
            Some(repository::asset_lifecycle::history_for_export(conn, &ids)?)
        } else {
            None
        };
        let slug = repository::workspaces::find_by_id(conn, workspace_id)
            .ok()
            .flatten()
            .map(|w| w.slug)
            .unwrap_or_else(|| "workspace".to_string());
        Ok((rows, events, slug))
    });

    let (rows, events, slug) = match export_result {
        Ok(v) => v,
        Err(e) => {
            error!(error = ?e, "Database error exporting assets");
            return errors::internal("Failed to export assets");
        }
    };

    let (body, kind) = match events {
        Some(events) => match write_history_csv(&rows, &events) {
            Ok(bytes) => (bytes, "asset-history"),
            Err(e) => {
                error!(error = ?e, "Failed to serialize asset history CSV");
                return errors::internal("Failed to build asset export");
            }
        },
        None => match write_assets_csv(&rows) {
            Ok(bytes) => (bytes, "assets"),
            Err(e) => {
                error!(error = ?e, "Failed to serialize asset export CSV");
                return errors::internal("Failed to build asset export");
            }
        },
    };

    let date = chrono::Utc::now().format("%Y%m%d");
    let filename = format!("{kind}-{slug}-{date}.csv");
    HttpResponse::Ok()
        .insert_header((header::CONTENT_TYPE, "text/csv; charset=utf-8"))
        .insert_header((
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{filename}\""),
        ))
        .body(body)
}

/// `GET /api/assets/{id}/record-card` — the asset's full lifecycle history as a
/// CSV (offboarding / disposal / dispute evidence). Same rows as the bulk
/// history export, scoped to one asset. Any authenticated workspace member.
pub async fn record_card(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let asset_id = path.into_inner();
    let result = tc.run(|conn| {
        let asset = repository::get_device_by_id(conn, asset_id)?;
        let events = repository::asset_lifecycle::history_for_export(conn, &[asset_id])?;
        Ok((asset, events))
    });
    let (asset, events) = match result {
        Ok(v) => v,
        Err(diesel::result::Error::NotFound) => {
            return errors::not_found_msg(format!("Asset {asset_id} not found"));
        }
        Err(e) => {
            error!(asset_id, error = ?e, "Database error building asset record card");
            return errors::internal("Failed to build asset record card");
        }
    };
    let body = match write_history_csv(std::slice::from_ref(&asset), &events) {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(asset_id, error = ?e, "Failed to serialize asset record card CSV");
            return errors::internal("Failed to build asset record card");
        }
    };
    let tag = asset.asset_tag.as_deref().unwrap_or("asset");
    let date = chrono::Utc::now().format("%Y%m%d");
    let filename = format!("record-card-{tag}-{date}.csv");
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
    let groups = query.groups.clone();
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
            groups,
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
        let mut resp = AssetResponse::from_device_and_user(device, user, groups, conn);
        resp.asset_groups =
            crate::repository::asset_groups::group_refs_for_assets(conn, &[device_id])
                .unwrap_or_default()
                .remove(&device_id)
                .unwrap_or_default();
        Ok(resp)
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
            // managed_by is Nosdesk-local custody, not owned by the sync, so it
            // stays settable even on externally-synced assets.
            managed_by_user_uuid: incoming.managed_by_user_uuid,
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

    // The write runs in its OWN transaction, with nothing else inside it. A
    // failure assembling the response below (e.g. an enrichment read against a
    // drifted schema) must never be able to abort and silently roll this back:
    // an aborted transaction turns the implicit COMMIT into a ROLLBACK that
    // still looks like success, so the client's change would vanish with a 200.
    let device = match tc.run(|conn| repository::update_device(conn, device_id, update_data)) {
        Ok(device) => device,
        Err(e) => {
            return match e {
                Error::NotFound => errors::not_found_msg(format!("Asset {device_id} not found")),
                _ => {
                    error!(device_id, error = ?e, "Database error updating device");
                    errors::internal(format!("Failed to update device {device_id}"))
                }
            }
        }
    };

    // Re-index in search. The update reaches clients through the sync pool (the
    // repository write emits `asset.updated`); no discrete SSE.
    indexing_tasks::spawn_index_device(search_service.get_ref().clone(), device.clone());

    // Assemble the response from the committed row in a SEPARATE transaction.
    // Enrichment reads propagate their errors (no silent `unwrap_or_default`),
    // but because the write already committed, a failure here only costs the
    // enrichment, never the write.
    let response = tc.run(|conn| {
        let user = device
            .primary_user_uuid
            .as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        let groups = groups_repo::get_groups_for_device(conn, device_id)?;
        Ok::<_, Error>(AssetResponse::from_device_and_user(
            device.clone(),
            user,
            groups,
            conn,
        ))
    });

    match response {
        Ok(device_response) => HttpResponse::Ok().json(device_response),
        Err(e) => {
            warn!(device_id, error = ?e, "Asset updated, but enriching the response failed; returning a minimal response");
            // The write is durable. Return the saved row without the enrichment
            // reads that just failed (no owner, no groups) so the client still
            // sees its change reflected.
            match tc.run(|conn| {
                Ok::<_, Error>(AssetResponse::from_device_and_user(
                    device.clone(),
                    None,
                    Vec::new(),
                    conn,
                ))
            }) {
                Ok(minimal) => HttpResponse::Ok().json(minimal),
                Err(_) => errors::internal(format!(
                    "Asset {device_id} updated, but the response could not be built"
                )),
            }
        }
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

    // Write in its own transaction (see `update_device`: response-assembly
    // reads must not be able to abort and silently roll back the write).
    let device = match tc.run(|conn| repository::update_device(conn, device_id, update_data)) {
        Ok(device) => device,
        Err(e) => {
            error!(device_id, error = ?e, "Database error unmanaging device");
            return errors::internal(format!("Failed to unmanage device {device_id}"));
        }
    };

    let response = tc.run(|conn| {
        let user = device
            .primary_user_uuid
            .as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        let groups = groups_repo::get_groups_for_device(conn, device_id)?;
        Ok::<_, Error>(AssetResponse::from_device_and_user(
            device.clone(),
            user,
            groups,
            conn,
        ))
    });
    match response {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => {
            warn!(device_id, error = ?e, "Asset unmanaged, but enriching the response failed; returning a minimal response");
            match tc.run(|conn| {
                Ok::<_, Error>(AssetResponse::from_device_and_user(
                    device.clone(),
                    None,
                    Vec::new(),
                    conn,
                ))
            }) {
                Ok(minimal) => HttpResponse::Ok().json(minimal),
                Err(_) => errors::internal(format!(
                    "Asset {device_id} unmanaged, but the response could not be built"
                )),
            }
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
    // Write in its own transaction (see `update_device`: response-assembly
    // reads must not be able to abort and silently roll back the write).
    let device = match tc.run(|conn| repository::update_device(conn, asset_id, update)) {
        Ok(device) => device,
        Err(e) => {
            error!(asset_id, error = ?e, "apply asset update");
            return errors::internal("Failed to update asset");
        }
    };
    indexing_tasks::spawn_index_device(search_service.get_ref().clone(), device.clone());

    let response = tc.run(|conn| {
        let user = device
            .primary_user_uuid
            .as_ref()
            .and_then(|uuid| get_user_by_uuid(conn, uuid));
        let groups = groups_repo::get_groups_for_device(conn, asset_id)?;
        Ok::<_, Error>(AssetResponse::from_device_and_user(
            device.clone(),
            user,
            groups,
            conn,
        ))
    });
    match response {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => {
            warn!(asset_id, error = ?e, "Asset updated, but enriching the response failed; returning a minimal response");
            match tc.run(|conn| {
                Ok::<_, Error>(AssetResponse::from_device_and_user(
                    device.clone(),
                    None,
                    Vec::new(),
                    conn,
                ))
            }) {
                Ok(minimal) => HttpResponse::Ok().json(minimal),
                Err(_) => errors::internal("Asset updated, but the response could not be built"),
            }
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
