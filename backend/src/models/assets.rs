use chrono::{NaiveDate, NaiveDateTime};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::assets)]
pub struct Asset {
    pub id: i32,
    pub name: String,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub location: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
    /// Discriminator into the `asset_kinds` registry. Existing
    /// rows default to `'device'` so the IT-desk lens keeps
    /// rendering them unchanged; non-device kinds (vehicle,
    /// license, material, ...) opt in by setting this field
    /// when created from the typed asset flow.
    pub kind: String,
    /// JSONB blob holding kind-specific attributes validated
    /// against the kind's `attribute_schema` at write time. Empty
    /// object for IT-desk devices that only use the structured
    /// columns above.
    pub attributes: serde_json::Value,
    /// Quantity for bulk materials / consumables (cable length
    /// in metres, ink-cartridge stock, screws by the hundred).
    /// Null on assets that are "one row per physical thing"
    /// like a laptop.
    pub quantity: Option<bigdecimal::BigDecimal>,
    /// Unit label paired with `quantity`. Free-text ('m', 'L',
    /// 'pcs', etc.) so we don't impose a unit ontology.
    pub unit: Option<String>,
    /// Identifier of the external system that owns this row,
    /// when it's synced from one. `Some("intune")` /
    /// `Some("entra")` for Microsoft-managed assets; `None`
    /// for assets managed inside Nosdesk. Drives the
    /// is_editable predicate that hides edit UI for external
    /// rows so admins don't make changes that the next sync
    /// will overwrite.
    pub external_sync_source: Option<String>,
    /// Optional low-stock threshold. When set on a
    /// stock-tracked asset (i.e. `quantity` is also Some), a
    /// current `quantity` at or below this value flags the
    /// asset as low-stock in the UI and emits an
    /// `asset.low_stock` SSE event after each usage decrement
    /// that crosses the threshold. NULL means "not configured"
    /// (no alerting).
    pub low_stock_threshold: Option<bigdecimal::BigDecimal>,
    pub workspace_id: i32,
    /// Lifecycle state, one of the `AssetStatus` values (defaults to
    /// `in_service`). Status only changes through the lifecycle
    /// transition flow, which records an `asset_lifecycle_events`
    /// row, so `AssetUpdate` deliberately omits it.
    pub status: String,
    /// Optional link to the `asset_models` catalog row this asset was
    /// stamped from. NULL for model-less / hand-entered assets.
    pub model_id: Option<i32>,
    /// The accountable "managed by" custodian, distinct from the holder
    /// (`primary_user_uuid`, "used by"). NULL when unassigned.
    pub managed_by_user_uuid: Option<Uuid>,
}

/// Canonical asset lifecycle states. Stored as snake_case strings in
/// `assets.status` and validated here rather than by a DB CHECK, so
/// adding a state is a code change, not a migration. State-specific
/// data (repair vendor / RMA / offsite, loan recipient / due-back)
/// lives in `asset_lifecycle_events.metadata`, never in new columns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetStatus {
    InService,
    InStock,
    InRepair,
    OnLoan,
    Retired,
    Lost,
    Disposed,
    OnOrder,
    InTransit,
}

impl AssetStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InService => "in_service",
            Self::InStock => "in_stock",
            Self::InRepair => "in_repair",
            Self::OnLoan => "on_loan",
            Self::Retired => "retired",
            Self::Lost => "lost",
            Self::Disposed => "disposed",
            Self::OnOrder => "on_order",
            Self::InTransit => "in_transit",
        }
    }

    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s {
            "in_service" => Some(Self::InService),
            "in_stock" => Some(Self::InStock),
            "in_repair" => Some(Self::InRepair),
            "on_loan" => Some(Self::OnLoan),
            "retired" => Some(Self::Retired),
            "lost" => Some(Self::Lost),
            "disposed" => Some(Self::Disposed),
            "on_order" => Some(Self::OnOrder),
            "in_transit" => Some(Self::InTransit),
            _ => None,
        }
    }

    /// The canonical default for a freshly created asset. Mirrors the
    /// DB-level `DEFAULT 'in_service'` on `assets.status`.
    pub fn default_str() -> &'static str {
        Self::InService.as_str()
    }

    pub fn is_valid(s: &str) -> bool {
        Self::from_str_opt(s).is_some()
    }
}

/// Default kind for callers that omit `kind` from the JSON
/// payload. Mirrors the DB-level default on `assets.kind`
/// (`'generic'` as of 2026-05-20-130000) so a workspace-neutral
/// asset is the no-effort outcome. The IT-desk flow sets
/// `kind = 'device'` explicitly through the picker.
fn default_asset_kind() -> String {
    "generic".to_string()
}

/// Default `attributes` blob for legacy callers. JSON Schema
/// validation against the `device` builtin kind's empty
/// attribute_schema accepts an empty object.
fn default_asset_attributes() -> serde_json::Value {
    serde_json::json!({})
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::assets)]
pub struct NewAsset {
    pub name: String,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
    #[serde(default = "default_asset_kind")]
    pub kind: String,
    #[serde(default = "default_asset_attributes")]
    pub attributes: serde_json::Value,
    #[serde(default)]
    pub quantity: Option<bigdecimal::BigDecimal>,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub external_sync_source: Option<String>,
    #[serde(default)]
    pub low_stock_threshold: Option<bigdecimal::BigDecimal>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::assets)]
pub struct AssetUpdate {
    pub name: Option<String>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    /// The accountable "managed by" custodian. `None` leaves it unchanged.
    pub managed_by_user_uuid: Option<Uuid>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
    pub kind: Option<String>,
    pub attributes: Option<serde_json::Value>,
    pub quantity: Option<Option<bigdecimal::BigDecimal>>,
    pub unit: Option<Option<String>>,
    pub external_sync_source: Option<Option<String>>,
    pub low_stock_threshold: Option<Option<bigdecimal::BigDecimal>>,
    /// Link to a catalog model. `Some(Some(id))` stamps a model,
    /// `Some(None)` clears it, `None` leaves it unchanged.
    pub model_id: Option<Option<i32>>,
}

/// Runtime-extensible asset-kind registry. `slug` is the value
/// stored on `assets.kind`; `attribute_schema` is a constrained
/// JSON Schema subset validated by `services::assets::kinds`.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_kinds)]
pub struct AssetKind {
    pub id: i32,
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub attribute_schema: serde_json::Value,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<Uuid>,
    /// One of `it`, `logical`, `physical`, `bulk`, `generic`.
    /// The frontend toggles which IT-flavoured form fields and
    /// planner UI to render off this; the DB CHECK constraint
    /// enforces the closed set.
    pub category: String,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_kinds)]
pub struct NewAssetKind {
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub attribute_schema: serde_json::Value,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub created_by: Option<Uuid>,
    pub category: String,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::asset_kinds)]
pub struct AssetKindUpdate {
    pub label: Option<String>,
    pub description: Option<Option<String>>,
    pub icon: Option<Option<String>>,
    pub attribute_schema: Option<serde_json::Value>,
    pub sort_order: Option<i32>,
    pub category: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

// === Asset model catalog (NetBox-style) ======================
//
// `manufacturers` (a make) -> `asset_models` (a real make+model, the
// "device type") -> `assets` (instances stamped from a model).

/// A manufacturer / make (Apple, Dell) in the asset model catalog.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::manufacturers)]
pub struct Manufacturer {
    pub id: i32,
    pub name: String,
    pub workspace_id: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::manufacturers)]
pub struct NewManufacturer {
    pub name: String,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::manufacturers)]
pub struct ManufacturerChange {
    pub name: Option<String>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// An asset model ("device type"): a real make+model that assets are
/// stamped from. Carries the kind and a default-attributes spec blob
/// that copies onto new assets at assignment (copy-at-assignment).
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_models)]
#[diesel(belongs_to(Manufacturer))]
pub struct AssetModel {
    pub id: i32,
    pub manufacturer_id: i32,
    pub name: String,
    pub kind: String,
    pub part_number: Option<String>,
    pub default_attributes: serde_json::Value,
    pub notes: Option<String>,
    pub workspace_id: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_models)]
pub struct NewAssetModel {
    pub manufacturer_id: i32,
    pub name: String,
    pub kind: String,
    pub part_number: Option<String>,
    pub default_attributes: serde_json::Value,
    pub notes: Option<String>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::asset_models)]
pub struct AssetModelChange {
    pub manufacturer_id: Option<i32>,
    pub name: Option<String>,
    pub kind: Option<String>,
    pub part_number: Option<Option<String>>,
    pub default_attributes: Option<serde_json::Value>,
    pub notes: Option<Option<String>>,
    pub updated_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// One row of the asset usage ledger. Records how much of a
/// stock-tracked asset was consumed and when, optionally tied
/// to a ticket. Example: "5 m of cable consumed on ticket
/// #142". `unit` is stored on the row (not derived from
/// asset.unit at read time) so a later unit change on the
/// asset doesn't rewrite history.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_usage_log)]
pub struct AssetUsage {
    pub id: i64,
    pub asset_id: i32,
    pub ticket_id: Option<i32>,
    pub quantity_used: bigdecimal::BigDecimal,
    pub unit: String,
    pub recorded_by: Option<Uuid>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub notes: Option<String>,
    /// Direction discriminator. `"usage"` decrements the asset's
    /// quantity; `"restock"` increments it. Both kinds keep
    /// `quantity_used > 0`; the magnitude lives in
    /// `quantity_used`, the direction here. The DB CHECK
    /// constraint pins the enum to this closed set.
    pub event_kind: String,
    pub workspace_id: i32,
}

/// One row of the asset audit ledger. Records a physical-
/// count assertion: the admin counted `counted_quantity` units
/// on hand at `recorded_at`; the system held `previous_quantity`
/// at that moment. `delta` = counted - previous (signed). The
/// assets.quantity column is set to counted_quantity in the
/// same transaction, so this row is also the audit trail for
/// the corresponding correction.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_audits)]
pub struct AssetAudit {
    pub id: i64,
    pub asset_id: i32,
    pub counted_quantity: bigdecimal::BigDecimal,
    pub previous_quantity: bigdecimal::BigDecimal,
    pub delta: bigdecimal::BigDecimal,
    pub notes: Option<String>,
    pub recorded_by: Option<Uuid>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_audits)]
pub struct NewAssetAudit {
    pub asset_id: i32,
    pub counted_quantity: bigdecimal::BigDecimal,
    pub previous_quantity: bigdecimal::BigDecimal,
    pub delta: bigdecimal::BigDecimal,
    pub notes: Option<String>,
    pub recorded_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_usage_log)]
pub struct NewAssetUsage {
    pub asset_id: i32,
    pub ticket_id: Option<i32>,
    pub quantity_used: bigdecimal::BigDecimal,
    pub unit: String,
    pub recorded_by: Option<Uuid>,
    pub notes: Option<String>,
    pub event_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_media)]
#[diesel(belongs_to(Asset))]
pub struct AssetMedia {
    pub id: i32,
    pub asset_id: i32,
    pub url: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub kind: String,
    pub sort_order: i32,
    pub caption: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
    // Field order mirrors `schema.rs`: the column was added by a later
    // migration, so it lands last. Diesel `Queryable` maps positionally.
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_media)]
pub struct NewAssetMedia {
    pub asset_id: i32,
    pub url: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub kind: String,
    pub sort_order: i32,
    pub caption: Option<String>,
    pub uploaded_by: Option<Uuid>,
    pub thumbnail_url: Option<String>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::asset_media)]
pub struct AssetMediaUpdate {
    pub sort_order: Option<i32>,
    pub caption: Option<Option<String>>,
}

/// One entry in an asset's append-only lifecycle log. Each row is a
/// status transition; `ticket_id` links it to the ticket that
/// captured the context (e.g. the repair), and `metadata` carries
/// state-specific fields without dedicated columns.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_lifecycle_events)]
#[diesel(belongs_to(Asset))]
pub struct AssetLifecycleEvent {
    pub id: i32,
    pub asset_id: i32,
    pub from_status: Option<String>,
    pub to_status: String,
    pub reason: Option<String>,
    pub ticket_id: Option<i32>,
    pub metadata: serde_json::Value,
    pub actor_uuid: Option<Uuid>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_lifecycle_events)]
pub struct NewAssetLifecycleEvent {
    pub asset_id: i32,
    pub from_status: Option<String>,
    pub to_status: String,
    pub reason: Option<String>,
    pub ticket_id: Option<i32>,
    pub metadata: serde_json::Value,
    pub actor_uuid: Option<Uuid>,
}

/// A disposal record (design doc: asset-lifecycle-export; NIST SP 800-88
/// aligned). Written once when an asset is disposed, for compliance /
/// chain-of-custody export. `lifecycle_event_id` links to the `disposed`
/// transition that created it; `sanitization_method` is a NIST 800-88 category
/// (clear / purge / destroy / none); `certificate_file_id` is a soft reference
/// for now (the v1.2 compliance pack wires the attachment fully).
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_disposals)]
#[diesel(belongs_to(Asset))]
pub struct AssetDisposal {
    pub id: i32,
    pub asset_id: i32,
    pub lifecycle_event_id: Option<i32>,
    pub sanitization_method: String,
    pub data_bearing: bool,
    pub certificate_file_id: Option<i32>,
    pub itad_vendor: Option<String>,
    pub notes: Option<String>,
    pub actor_uuid: Option<Uuid>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_disposals)]
pub struct NewAssetDisposal {
    pub asset_id: i32,
    pub lifecycle_event_id: Option<i32>,
    pub sanitization_method: String,
    pub data_bearing: bool,
    pub certificate_file_id: Option<i32>,
    pub itad_vendor: Option<String>,
    pub notes: Option<String>,
    pub actor_uuid: Option<Uuid>,
}

/// A device loan: an asset in a borrower's custody for a span. The
/// `asset_loans` ledger is the source of truth for who holds what until
/// when; `assets.status = 'on_loan'` mirrors "has an active loan". A loan
/// is active while `returned_at` is None; overdue while active and
/// `due_back` is in the past. `status_before` is the asset's status at
/// issue, restored on return.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_loans)]
#[diesel(belongs_to(Asset))]
pub struct AssetLoan {
    pub id: i32,
    pub asset_id: i32,
    pub borrower_user_uuid: Uuid,
    pub loaned_at: chrono::DateTime<chrono::Utc>,
    pub due_back: Option<NaiveDate>,
    pub returned_at: Option<chrono::DateTime<chrono::Utc>>,
    pub ticket_id: Option<i32>,
    pub status_before: String,
    pub notes: Option<String>,
    pub actor_uuid: Option<Uuid>,
    pub returned_by_uuid: Option<Uuid>,
    pub due_soon_notified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub overdue_notified_at: Option<chrono::DateTime<chrono::Utc>>,
    pub workspace_id: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::asset_loans)]
pub struct NewAssetLoan {
    pub asset_id: i32,
    pub borrower_user_uuid: Uuid,
    pub loaned_at: chrono::DateTime<chrono::Utc>,
    pub due_back: Option<NaiveDate>,
    pub ticket_id: Option<i32>,
    pub status_before: String,
    pub notes: Option<String>,
    pub actor_uuid: Option<Uuid>,
}

/// Partial update for a loan. Outer `Option` = "leave unchanged"; inner
/// (for nullable columns) distinguishes "set to NULL" from "unchanged".
/// Covers the edit (due_back / notes) and return (returned_at /
/// returned_by_uuid) paths.
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = crate::schema::asset_loans)]
pub struct AssetLoanChange {
    pub due_back: Option<Option<NaiveDate>>,
    pub notes: Option<Option<String>>,
    pub returned_at: Option<Option<chrono::DateTime<chrono::Utc>>>,
    pub returned_by_uuid: Option<Option<Uuid>>,
}
