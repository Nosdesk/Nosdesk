//! Named asset statuses: a workspace's own names for its assets' statuses,
//! each inside one of the fixed [`AssetStatus`](crate::models::AssetStatus)
//! categories.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::models::AssetStatus;

/// A named status. `assets.status` holds its `category`, `assets.status_id`
/// its id; an asset with no `status_id` is plain and reads as its category's
/// default named status, or the catalogue name when there is none.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::asset_statuses)]
pub struct AssetNamedStatus {
    pub id: i32,
    pub workspace_id: i32,
    /// One of the nine category keys. Never changes.
    pub category: String,
    /// Stable key for the API, imports and plugins. Never changes.
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    /// `#rrggbb`; `None` uses the category's color.
    pub color: Option<String>,
    /// Order within the category.
    pub position: i32,
    /// The category's name in this workspace.
    pub is_default: bool,
    /// Asset kind slugs offered this status; `None` means every kind.
    pub applies_to_kinds: Option<Vec<Option<String>>>,
    pub archived_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: Option<Uuid>,
}

impl AssetNamedStatus {
    /// The category, `None` for a stored value outside the nine.
    pub fn category(&self) -> Option<AssetStatus> {
        AssetStatus::from_str_opt(&self.category)
    }

    /// Whether the status is offered for assets of kind `kind`.
    pub fn applies_to(&self, kind: &str) -> bool {
        self.applies_to_kinds
            .as_ref()
            .is_none_or(|kinds| kinds.iter().flatten().any(|k| k == kind))
    }
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::asset_statuses)]
pub struct NewAssetNamedStatus {
    pub category: String,
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub position: i32,
    pub is_default: bool,
    pub applies_to_kinds: Option<Vec<String>>,
    pub created_by: Option<Uuid>,
}

/// An edit to a named status. Outer `None` leaves a column as it is; for a
/// nullable column, `Some(None)` clears it. The category and slug never
/// change.
#[derive(Debug, Clone, Default, AsChangeset)]
#[diesel(table_name = crate::schema::asset_statuses)]
pub struct AssetNamedStatusChange {
    pub label: Option<String>,
    pub description: Option<Option<String>>,
    pub color: Option<Option<String>>,
    pub position: Option<i32>,
    pub is_default: Option<bool>,
    pub applies_to_kinds: Option<Option<Vec<String>>>,
    pub archived_at: Option<Option<DateTime<Utc>>>,
    pub updated_at: Option<DateTime<Utc>>,
}
