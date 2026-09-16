use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Asset Groups - native, workspace-local asset classification
// ============================================================================
//
// Tag-style UX (multi-assign, assigned from the asset, a list filter facet)
// over an entity-shaped schema so future depth stays additive. Distinct from
// the directory groups above: no Entra/security semantics.

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::asset_groups)]
pub struct AssetGroup {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub display_order: i32,
    pub archived_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_groups)]
pub struct NewAssetGroup {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    #[serde(default)]
    pub display_order: i32,
    #[serde(skip_deserializing)]
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::asset_groups)]
pub struct AssetGroupUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub display_order: Option<i32>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::asset_group_assignments)]
pub struct NewAssetGroupAssignment {
    pub group_id: i32,
    pub asset_id: i32,
    pub added_by: Option<Uuid>,
}

/// List/picker DTO: a group plus its current member count.
#[derive(Debug, Serialize)]
pub struct AssetGroupResponse {
    #[serde(flatten)]
    pub group: AssetGroup,
    pub asset_count: i64,
}

/// Compact group reference for "groups this asset is in" — matches the
/// frontend `AssetGroup` type. Lighter than the full row for per-asset
/// rendering across a list page.
#[derive(Debug, Serialize)]
pub struct AssetGroupRef {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub color: Option<String>,
}
