use super::plugins::Plugin;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== PLUGIN COLLECTION TYPES =====

/// Collection field definition in plugin manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionFieldDefinition {
    #[serde(rename = "type")]
    pub field_type: String,
    pub label: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub reference: Option<String>,
}

/// Collection definition in plugin manifest. `schema_version` is
/// required so future plugin versions can express migrations
/// (rename, drop, retype a field) without losing data. v1
/// recognises only schema_version 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionDefinition {
    pub schema_version: u32,
    pub label: Option<String>,
    pub fields: std::collections::HashMap<String, CollectionFieldDefinition>,
}

/// Plugin collection schema (DB row)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
#[diesel(belongs_to(Plugin))]
pub struct PluginCollectionSchema {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

/// New collection schema for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
pub struct NewPluginCollectionSchema {
    pub plugin_id: i32,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
}

/// Collection schema update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
pub struct PluginCollectionSchemaUpdate {
    pub schema: Option<serde_json::Value>,
    pub version: Option<i32>,
}

/// Plugin collection row (DB row)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
#[diesel(belongs_to(PluginCollectionSchema, foreign_key = schema_id))]
pub struct PluginCollectionRow {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub schema_id: i32,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub workspace_id: i32,
}

/// New collection row for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
pub struct NewPluginCollectionRow {
    pub plugin_id: i32,
    pub schema_id: i32,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
}

/// Collection row update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
pub struct PluginCollectionRowUpdate {
    pub data: Option<serde_json::Value>,
}

// ===== COLLECTION API TYPES =====

/// Query params for listing collection rows
#[derive(Debug, Deserialize)]
pub struct CollectionQueryParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub filter: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

/// Request to create a collection row
#[derive(Debug, Deserialize)]
pub struct CreateCollectionRowRequest {
    pub data: serde_json::Value,
}

/// Request to update a collection row
#[derive(Debug, Deserialize)]
pub struct UpdateCollectionRowRequest {
    pub data: serde_json::Value,
}

/// Collection row API response
#[derive(Debug, Serialize)]
pub struct CollectionRowResponse {
    pub uuid: Uuid,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<PluginCollectionRow> for CollectionRowResponse {
    fn from(row: PluginCollectionRow) -> Self {
        CollectionRowResponse {
            uuid: row.uuid,
            data: row.data,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Paginated collection rows response
#[derive(Debug, Serialize)]
pub struct CollectionListResponse {
    pub rows: Vec<CollectionRowResponse>,
    pub total: i64,
}

/// Collection schema API response
#[derive(Debug, Serialize)]
pub struct CollectionSchemaResponse {
    pub uuid: Uuid,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
    pub row_count: i64,
}
