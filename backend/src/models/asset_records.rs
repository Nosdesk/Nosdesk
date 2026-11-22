//! Asset records: work done on an asset (a repair, an inspection,
//! preventative maintenance, a firmware update or a workspace's own type),
//! and the record types that shape them.

use chrono::{DateTime, NaiveDate, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A record type: a workspace's kind of work, with its own fields and
/// outcomes. A built-in type's `label` is null until someone renames it, so
/// clients show its catalogue name (`asset-record-type-<slug>`).
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_record_types)]
pub struct AssetRecordType {
    pub id: i32,
    pub workspace_id: i32,
    pub slug: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    /// The type's own fields, in the custom-fields schema subset.
    pub field_schema: serde_json::Value,
    /// `field_schema`'s property names in display order.
    pub field_order: Vec<Option<String>>,
    /// Per shared field, its [`CoreFieldMode`]; absent means optional.
    pub core_fields: serde_json::Value,
    /// Ordered [`RecordOutcome`]s.
    pub outcomes: serde_json::Value,
    /// Asset kind slugs offered this type; `None` means every kind.
    pub applies_to_kinds: Option<Vec<Option<String>>>,
    /// The asset status whose transition opens a record of this type.
    pub opens_with_status: Option<String>,
    pub is_builtin: bool,
    pub sort_order: i32,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

impl AssetRecordType {
    /// The type's outcomes, in order. A malformed entry is left out.
    pub fn outcome_list(&self) -> Vec<RecordOutcome> {
        serde_json::from_value(self.outcomes.clone()).unwrap_or_default()
    }

    /// The outcome named `key`, if the type has one.
    pub fn outcome(&self, key: &str) -> Option<RecordOutcome> {
        self.outcome_list().into_iter().find(|o| o.key == key)
    }

    /// How the type treats shared field `field` (one of [`CORE_FIELDS`]).
    pub fn core_field(&self, field: &str) -> CoreFieldMode {
        self.core_fields
            .get(field)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or(CoreFieldMode::Optional)
    }

    /// Whether the type is offered for assets of kind `kind`.
    pub fn applies_to(&self, kind: &str) -> bool {
        self.applies_to_kinds
            .as_ref()
            .is_none_or(|kinds| kinds.iter().flatten().any(|k| k == kind))
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::asset_record_types)]
pub struct NewAssetRecordType {
    pub slug: String,
    pub label: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub field_schema: serde_json::Value,
    pub field_order: Vec<String>,
    pub core_fields: serde_json::Value,
    pub outcomes: serde_json::Value,
    pub applies_to_kinds: Option<Vec<String>>,
    pub opens_with_status: Option<String>,
    pub is_builtin: bool,
    pub sort_order: i32,
    pub created_by: Option<Uuid>,
}

/// An edit to a record type. Outer `None` leaves a column as it is; for a
/// nullable column, `Some(None)` clears it. The slug and `is_builtin` never
/// change.
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = crate::schema::asset_record_types)]
pub struct AssetRecordTypeChange {
    pub label: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub icon: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub field_schema: Option<serde_json::Value>,
    pub field_order: Option<Vec<String>>,
    pub core_fields: Option<serde_json::Value>,
    pub outcomes: Option<serde_json::Value>,
    pub applies_to_kinds: Option<Option<Vec<String>>>,
    pub opens_with_status: Option<Option<String>>,
    pub sort_order: Option<i32>,
    pub archived_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}

/// What an outcome means, whatever a type calls it: `replaced` allows a
/// replacement asset, `done` completes a service schedule, and reports group
/// by category across types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeCategory {
    Done,
    NotDone,
    NoFault,
    Replaced,
}

/// One of a type's outcomes. `label` is absent on a built-in outcome nobody
/// renamed: clients show `asset-record-outcome-<type slug>-<key>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecordOutcome {
    pub key: String,
    pub category: OutcomeCategory,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// The fields every record type shares, which reports can sum across types.
/// `vendor` covers an internal technician too.
pub const CORE_FIELDS: &[&str] = &[
    "vendor",
    "reference",
    "cost",
    "outcome",
    "ticket",
    "holder",
    "location",
    "summary",
];

/// How a record type treats one of the [`CORE_FIELDS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreFieldMode {
    Hidden,
    Optional,
    Required,
}

/// What wrote a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordSource {
    Manual,
    Api,
    Import,
    Rule,
    Transition,
    Schedule,
    Backfill,
    Plugin,
}

impl RecordSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Api => "api",
            Self::Import => "import",
            Self::Rule => "rule",
            Self::Transition => "transition",
            Self::Schedule => "schedule",
            Self::Backfill => "backfill",
            Self::Plugin => "plugin",
        }
    }
}

/// Where a record's holder and location came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotSource {
    /// Sent with the record.
    Entered,
    /// The loan whose span covers the day of the work.
    Loan,
    /// The asset as it is now, for work done today or yesterday.
    Current,
    /// Nothing to go on.
    None,
}

impl SnapshotSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Entered => "entered",
            Self::Loan => "loan",
            Self::Current => "current",
            Self::None => "none",
        }
    }
}

/// A record of work done on an asset. Writing one never changes the asset's
/// status. `recorded_at` and `recorded_by` are when and by whom it was
/// entered, and never change; `occurred_on` is when the work was done.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_records)]
pub struct AssetRecord {
    pub id: i32,
    pub workspace_id: i32,
    pub asset_id: i32,
    pub record_type_id: i32,
    pub summary: Option<String>,
    pub notes: Option<String>,
    pub occurred_on: NaiveDate,
    pub occurred_at: Option<DateTime<Utc>>,
    pub completed_on: Option<NaiveDate>,
    pub completed_at: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
    pub vendor: Option<String>,
    pub performed_by_uuid: Option<Uuid>,
    pub reference: Option<String>,
    pub cost_minor: Option<i64>,
    pub cost_currency: Option<String>,
    pub ticket_id: Option<i32>,
    pub replacement_asset_id: Option<i32>,
    pub holder_uuid: Option<Uuid>,
    pub location: Option<String>,
    pub snapshot_source: String,
    pub attributes: serde_json::Value,
    pub plugin_fields: serde_json::Value,
    pub lifecycle_event_id: Option<i32>,
    pub schedule_id: Option<i32>,
    pub source: String,
    pub source_ref: Option<String>,
    pub import_job_id: Option<Uuid>,
    pub rule_id: Option<i32>,
    pub recorded_at: DateTime<Utc>,
    pub recorded_by: Option<Uuid>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
    pub version: i32,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::asset_records)]
pub struct NewAssetRecord {
    pub asset_id: i32,
    pub record_type_id: i32,
    pub summary: Option<String>,
    pub notes: Option<String>,
    pub occurred_on: NaiveDate,
    pub occurred_at: Option<DateTime<Utc>>,
    pub completed_on: Option<NaiveDate>,
    pub completed_at: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
    pub vendor: Option<String>,
    pub performed_by_uuid: Option<Uuid>,
    pub reference: Option<String>,
    pub cost_minor: Option<i64>,
    pub cost_currency: Option<String>,
    pub ticket_id: Option<i32>,
    pub replacement_asset_id: Option<i32>,
    pub holder_uuid: Option<Uuid>,
    pub location: Option<String>,
    pub snapshot_source: String,
    pub attributes: serde_json::Value,
    pub lifecycle_event_id: Option<i32>,
    pub source: String,
    pub source_ref: Option<String>,
    pub import_job_id: Option<Uuid>,
    pub rule_id: Option<i32>,
    pub recorded_by: Option<Uuid>,
    pub updated_by: Option<Uuid>,
}

/// Every column an edit writes, as the record stands after it. These are
/// the columns the app role may update.
#[derive(Debug, Clone, AsChangeset)]
#[diesel(table_name = crate::schema::asset_records, treat_none_as_null = true)]
pub struct AssetRecordEdit {
    pub record_type_id: i32,
    pub summary: Option<String>,
    pub notes: Option<String>,
    pub occurred_on: NaiveDate,
    pub occurred_at: Option<DateTime<Utc>>,
    pub completed_on: Option<NaiveDate>,
    pub completed_at: Option<DateTime<Utc>>,
    pub outcome: Option<String>,
    pub vendor: Option<String>,
    pub performed_by_uuid: Option<Uuid>,
    pub reference: Option<String>,
    pub cost_minor: Option<i64>,
    pub cost_currency: Option<String>,
    pub ticket_id: Option<i32>,
    pub replacement_asset_id: Option<i32>,
    pub holder_uuid: Option<Uuid>,
    pub location: Option<String>,
    pub snapshot_source: String,
    pub attributes: serde_json::Value,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
    pub version: i32,
}
