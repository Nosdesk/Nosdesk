//! Asset-kinds repository.
//!
//! The `asset_kinds` table is the runtime registry that drives
//! the `assets.kind` discriminator. Built-ins ship with the
//! product (slug + label + is_builtin = true); admins can add
//! their own kinds at runtime with a constrained JSON Schema
//! attribute spec validated by `services::assets::kinds`.
//!
//! Kinds are NOT a sync aggregate today. The admin picker
//! re-fetches on demand and asset rows store the slug as a
//! plain string, so the registry can be reloaded lazily without
//! a fanout. If asset-kind chips ever need real-time updates we
//! can wire them in then.

use chrono::Utc;
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{AssetKind, AssetKindUpdate, NewAssetKind};
use crate::schema::asset_kinds;

pub fn list_kinds(conn: &mut DbConnection) -> QueryResult<Vec<AssetKind>> {
    asset_kinds::table
        .order((asset_kinds::sort_order.asc(), asset_kinds::label.asc()))
        .load(conn)
}

/// Seed the built-in kinds for a fresh workspace, if it has none. Mirrors the
/// kinds the initial migration seeds into the bootstrap workspace, so a new
/// workspace's model/asset forms have kinds to pick. The IT kinds share the
/// device/warranty attribute schema; the rest start with an empty schema the
/// admin can extend.
///
/// Caller must run inside an actor context pinned to the target workspace;
/// `workspace_id` comes from the column default reading `app.workspace_id`.
// sync-audit-only: provisioning seed, not a user-driven write
pub fn seed_defaults_if_empty(
    conn: &mut DbConnection,
    created_by: Option<Uuid>,
) -> QueryResult<usize> {
    use diesel::dsl::count_star;

    let existing: i64 = asset_kinds::table.select(count_star()).first(conn)?;
    if existing > 0 {
        return Ok(0);
    }

    let device_schema = json!({
        "type": "object",
        "properties": {
            "hostname": {"type": "string", "title": "Hostname"},
            "is_managed": {"type": "boolean", "title": "Managed"},
            "os_version": {"type": "string", "title": "OS version"},
            "last_sync_time": {"type": "string", "title": "Last sync time", "format": "date-time"},
            "enrollment_date": {"type": "string", "title": "Enrollment date", "format": "date-time"},
            "entra_device_id": {"type": "string", "title": "Entra device ID"},
            "warranty_status": {"enum": ["Active", "Warning", "Expired", "Unknown"], "type": "string", "title": "Warranty status"},
            "compliance_state": {"type": "string", "title": "Compliance state"},
            "intune_device_id": {"type": "string", "title": "Intune device ID"},
            "operating_system": {"type": "string", "title": "Operating system"},
            "warranty_end_date": {"type": "string", "title": "Warranty end", "format": "date"},
            "microsoft_device_id": {"type": "string", "title": "Microsoft device ID"},
            "warranty_start_date": {"type": "string", "title": "Warranty start", "format": "date"}
        }
    });
    let empty_schema = json!({"type": "object", "properties": {}});

    // (slug, label, description, icon, category, sort_order, is_it)
    let defaults: [(&str, &str, &str, &str, &str, i32, bool); 13] = [
        (
            "device",
            "Device",
            "Generic IT device.",
            "device",
            "it",
            10,
            true,
        ),
        (
            "laptop",
            "Laptop",
            "Portable computer assigned to a user.",
            "laptop",
            "it",
            20,
            true,
        ),
        (
            "desktop",
            "Desktop",
            "Workstation computer at a fixed location.",
            "desktop",
            "it",
            30,
            true,
        ),
        (
            "server",
            "Server",
            "Server hardware in a data centre or office.",
            "server",
            "it",
            40,
            true,
        ),
        (
            "phone",
            "Phone",
            "Mobile phone or VoIP handset.",
            "phone",
            "it",
            50,
            true,
        ),
        (
            "monitor",
            "Monitor",
            "External display.",
            "monitor",
            "it",
            60,
            true,
        ),
        (
            "network_device",
            "Network device",
            "Switch, router, access point, firewall.",
            "network",
            "it",
            70,
            true,
        ),
        (
            "license",
            "License",
            "Software license with optional seat tracking.",
            "license",
            "logical",
            80,
            false,
        ),
        (
            "vehicle",
            "Vehicle",
            "Car, van, truck, trailer.",
            "vehicle",
            "physical",
            90,
            false,
        ),
        (
            "equipment",
            "Equipment",
            "Tools, machinery, instruments.",
            "equipment",
            "physical",
            100,
            false,
        ),
        (
            "consumable",
            "Consumable",
            "Items consumed during work (uses quantity + unit).",
            "consumable",
            "bulk",
            110,
            false,
        ),
        (
            "material",
            "Material",
            "Bulk material tracked by quantity (pipe lengths, cable rolls).",
            "material",
            "bulk",
            120,
            false,
        ),
        (
            "generic",
            "Generic asset",
            "A workspace-neutral asset. Use for anything that does not fit a more specific kind.",
            "asset",
            "generic",
            5,
            false,
        ),
    ];

    let rows: Vec<NewAssetKind> = defaults
        .iter()
        .map(
            |&(slug, label, description, icon, category, sort_order, is_it)| NewAssetKind {
                slug: slug.to_string(),
                label: label.to_string(),
                description: Some(description.to_string()),
                icon: Some(icon.to_string()),
                attribute_schema: if is_it {
                    device_schema.clone()
                } else {
                    empty_schema.clone()
                },
                sort_order,
                is_builtin: true,
                created_by,
                category: category.to_string(),
            },
        )
        .collect();

    diesel::insert_into(asset_kinds::table)
        .values(&rows)
        .execute(conn)
}

pub fn get_kind(conn: &mut DbConnection, id: i32) -> QueryResult<AssetKind> {
    asset_kinds::table.find(id).first(conn)
}

pub fn get_kind_by_slug(conn: &mut DbConnection, slug: &str) -> QueryResult<AssetKind> {
    asset_kinds::table
        .filter(asset_kinds::slug.eq(slug))
        .first(conn)
}

// sync-audit-only: kind registry is workspace config, not a sync aggregate. Admin pickers re-fetch on demand; coverage lives in the audit_log trigger attached in 2026-05-20-110000_attach_audit_assets
pub fn create_kind(conn: &mut DbConnection, new_kind: NewAssetKind) -> QueryResult<AssetKind> {
    diesel::insert_into(asset_kinds::table)
        .values(&new_kind)
        .get_result(conn)
}

// sync-audit-only: kind registry is workspace config, see create_kind
pub fn update_kind(
    conn: &mut DbConnection,
    id: i32,
    mut update: AssetKindUpdate,
) -> QueryResult<AssetKind> {
    update.updated_at = Some(Utc::now());
    diesel::update(asset_kinds::table.find(id))
        .set(&update)
        .get_result(conn)
}

// sync-audit-only: kind registry is workspace config, see create_kind
/// Delete an asset kind. The caller is responsible for refusing
/// builtin slugs first (see `handlers::asset_kinds::delete`),
/// which gives the admin UI a distinct "can't delete a builtin"
/// error instead of the generic "not found" you'd get from a
/// post-filter here.
pub fn delete_kind(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(asset_kinds::table.find(id)).execute(conn)
}

/// Count how many asset rows currently carry `slug` as their kind
/// discriminator. Drives the delete-usage guard on the admin page
/// (the ConfirmModal shows "N assets currently use this kind"
/// rather than silently orphaning rows) and the per-kind usage
/// stat in the list view. RLS is already pinned by the calling
/// TenantConn so the count is workspace-local automatically.
pub fn count_assets_using_kind(conn: &mut DbConnection, slug: &str) -> QueryResult<i64> {
    use crate::schema::assets;
    assets::table
        .filter(assets::kind.eq(slug))
        .count()
        .get_result(conn)
}
