//! Plugin Collections Repository
//!
//! Provides database operations for plugin collection schemas and rows.

use diesel::dsl::count_star;
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    NewPluginCollectionRow, NewPluginCollectionSchema, PluginCollectionRow,
    PluginCollectionRowUpdate, PluginCollectionSchema, PluginCollectionSchemaUpdate,
};
use crate::schema::{plugin_collection_rows, plugin_collection_schemas};

// =============================================================================
// Schema CRUD
// =============================================================================

/// Get all schemas for a plugin
pub fn get_schemas_by_plugin(
    conn: &mut DbConnection,
    pid: i32,
) -> Result<Vec<PluginCollectionSchema>, diesel::result::Error> {
    plugin_collection_schemas::table
        .filter(plugin_collection_schemas::plugin_id.eq(pid))
        .order(plugin_collection_schemas::collection_name.asc())
        .load::<PluginCollectionSchema>(conn)
}

/// Get a schema by plugin id + collection name
pub fn get_schema_by_name(
    conn: &mut DbConnection,
    pid: i32,
    name: &str,
) -> Result<PluginCollectionSchema, diesel::result::Error> {
    plugin_collection_schemas::table
        .filter(plugin_collection_schemas::plugin_id.eq(pid))
        .filter(plugin_collection_schemas::collection_name.eq(name))
        .first::<PluginCollectionSchema>(conn)
}

/// Create a new collection schema
pub fn create_schema(
    conn: &mut DbConnection,
    new_schema: NewPluginCollectionSchema,
) -> Result<PluginCollectionSchema, diesel::result::Error> {
    diesel::insert_into(plugin_collection_schemas::table)
        .values(&new_schema)
        .get_result(conn)
}

/// Update a collection schema
pub fn update_schema(
    conn: &mut DbConnection,
    schema_id: i32,
    update: PluginCollectionSchemaUpdate,
) -> Result<PluginCollectionSchema, diesel::result::Error> {
    diesel::update(plugin_collection_schemas::table.find(schema_id))
        .set(&update)
        .get_result(conn)
}

/// Delete a collection schema (cascades to rows)
pub fn delete_schema(
    conn: &mut DbConnection,
    schema_id: i32,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(plugin_collection_schemas::table.find(schema_id)).execute(conn)
}

// =============================================================================
// Row CRUD
// =============================================================================

/// Create a new collection row
pub fn create_row(
    conn: &mut DbConnection,
    new_row: NewPluginCollectionRow,
) -> Result<PluginCollectionRow, diesel::result::Error> {
    diesel::insert_into(plugin_collection_rows::table)
        .values(&new_row)
        .get_result(conn)
}

/// Get a row by UUID
pub fn get_row_by_uuid(
    conn: &mut DbConnection,
    row_uuid: Uuid,
) -> Result<PluginCollectionRow, diesel::result::Error> {
    plugin_collection_rows::table
        .filter(plugin_collection_rows::uuid.eq(row_uuid))
        .first::<PluginCollectionRow>(conn)
}

/// Update a row
pub fn update_row(
    conn: &mut DbConnection,
    row_uuid: Uuid,
    update: PluginCollectionRowUpdate,
) -> Result<PluginCollectionRow, diesel::result::Error> {
    diesel::update(plugin_collection_rows::table.filter(plugin_collection_rows::uuid.eq(row_uuid)))
        .set(&update)
        .get_result(conn)
}

/// Delete a row
pub fn delete_row(conn: &mut DbConnection, row_uuid: Uuid) -> Result<usize, diesel::result::Error> {
    diesel::delete(plugin_collection_rows::table.filter(plugin_collection_rows::uuid.eq(row_uuid)))
        .execute(conn)
}

/// List rows for a schema with pagination, optional JSONB filter and sort
pub fn list_rows(
    conn: &mut DbConnection,
    sid: i32,
    limit: i64,
    offset: i64,
    filter_json: Option<serde_json::Value>,
    sort_by: Option<String>,
    sort_order: Option<String>,
) -> Result<(Vec<PluginCollectionRow>, i64), diesel::result::Error> {
    use diesel::sql_types::{BigInt, Integer, Text};

    // Build the base filter predicate
    let base_filter = plugin_collection_rows::schema_id.eq(sid);

    if let Some(ref filter_val) = filter_json {
        // Use raw SQL for JSONB contains operator
        let filter_str = filter_val.to_string();

        // Count query with filter
        let total: i64 = diesel::sql_query(
            "SELECT COUNT(*) as count FROM plugin_collection_rows WHERE schema_id = $1 AND data @> $2::jsonb"
        )
        .bind::<Integer, _>(sid)
        .bind::<Text, _>(&filter_str)
        .get_result::<CountResult>(conn)?
        .count;

        // Data query with filter, sort, pagination
        let order_clause = build_order_clause(sort_by.as_deref(), sort_order.as_deref());

        let query_str = format!(
            "SELECT id, uuid, plugin_id, schema_id, data, created_by, created_at, updated_at \
             FROM plugin_collection_rows \
             WHERE schema_id = $1 AND data @> $2::jsonb \
             ORDER BY {} \
             LIMIT $3 OFFSET $4",
            order_clause
        );

        let rows = diesel::sql_query(query_str)
            .bind::<Integer, _>(sid)
            .bind::<Text, _>(&filter_str)
            .bind::<BigInt, _>(limit)
            .bind::<BigInt, _>(offset)
            .load::<PluginCollectionRowRaw>(conn)?
            .into_iter()
            .map(|r| r.into())
            .collect();

        Ok((rows, total))
    } else {
        // Count without filter
        let total = plugin_collection_rows::table
            .filter(base_filter)
            .select(count_star())
            .first::<i64>(conn)?;

        // Data query without filter
        let order_clause = build_order_clause(sort_by.as_deref(), sort_order.as_deref());

        let query_str = format!(
            "SELECT id, uuid, plugin_id, schema_id, data, created_by, created_at, updated_at \
             FROM plugin_collection_rows \
             WHERE schema_id = $1 \
             ORDER BY {} \
             LIMIT $2 OFFSET $3",
            order_clause
        );

        let rows = diesel::sql_query(query_str)
            .bind::<Integer, _>(sid)
            .bind::<BigInt, _>(limit)
            .bind::<BigInt, _>(offset)
            .load::<PluginCollectionRowRaw>(conn)?
            .into_iter()
            .map(|r| r.into())
            .collect();

        Ok((rows, total))
    }
}

/// Count rows for a schema
pub fn count_rows_by_schema(
    conn: &mut DbConnection,
    sid: i32,
) -> Result<i64, diesel::result::Error> {
    plugin_collection_rows::table
        .filter(plugin_collection_rows::schema_id.eq(sid))
        .select(count_star())
        .first::<i64>(conn)
}

// =============================================================================
// Internal helpers
// =============================================================================

fn build_order_clause(sort_by: Option<&str>, sort_order: Option<&str>) -> String {
    let direction = match sort_order {
        Some(d) if d.eq_ignore_ascii_case("desc") => "DESC",
        _ => "ASC",
    };

    match sort_by {
        Some(field) => {
            // Sanitize field name to prevent SQL injection
            let safe_field: String = field
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if safe_field.is_empty() {
                format!("created_at {direction}")
            } else {
                format!("data->>'{safe_field}' {direction}")
            }
        }
        None => format!("created_at {direction}"),
    }
}

/// Raw SQL query result for COUNT
#[derive(QueryableByName)]
struct CountResult {
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    count: i64,
}

/// Raw SQL query result for collection rows
#[derive(QueryableByName)]
struct PluginCollectionRowRaw {
    #[diesel(sql_type = diesel::sql_types::Integer)]
    id: i32,
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    uuid: Uuid,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    plugin_id: i32,
    #[diesel(sql_type = diesel::sql_types::Integer)]
    schema_id: i32,
    #[diesel(sql_type = diesel::sql_types::Jsonb)]
    data: serde_json::Value,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Uuid>)]
    created_by: Option<Uuid>,
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    created_at: chrono::NaiveDateTime,
    #[diesel(sql_type = diesel::sql_types::Timestamptz)]
    updated_at: chrono::NaiveDateTime,
}

impl From<PluginCollectionRowRaw> for PluginCollectionRow {
    fn from(raw: PluginCollectionRowRaw) -> Self {
        PluginCollectionRow {
            id: raw.id,
            uuid: raw.uuid,
            plugin_id: raw.plugin_id,
            schema_id: raw.schema_id,
            data: raw.data,
            created_by: raw.created_by,
            created_at: raw.created_at,
            updated_at: raw.updated_at,
        }
    }
}
