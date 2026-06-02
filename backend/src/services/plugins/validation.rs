//! Plugin Collection Validation
//!
//! Validates collection definitions from manifests and row data against schemas.

use crate::db::DbConnection;
use crate::models::{
    CollectionDefinition, CollectionFieldDefinition, NewPluginCollectionSchema,
    PluginCollectionSchemaUpdate, PluginManifest,
};
use crate::repository::plugin_collections as collection_repo;
use tracing::{info, warn};

/// Valid field types for collection definitions
const VALID_FIELD_TYPES: &[&str] = &[
    "string",
    "number",
    "boolean",
    "date",
    "datetime",
    "uuid",
    "json",
    "reference",
];

/// Validate a collection definition from a manifest
pub fn validate_collection_definition(
    name: &str,
    definition: &CollectionDefinition,
) -> Result<(), String> {
    // Validate collection name
    if name.is_empty() || name.len() > 100 {
        return Err(format!(
            "Collection name must be 1-100 characters: '{name}'"
        ));
    }

    if !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Err(format!(
            "Collection name must contain only alphanumeric characters and underscores: '{name}'"
        ));
    }

    // Validate fields
    if definition.fields.is_empty() {
        return Err(format!("Collection '{name}' must have at least one field"));
    }

    for (field_name, field_def) in &definition.fields {
        validate_field(name, field_name, field_def)?;
    }

    Ok(())
}

/// Validate a single field definition
fn validate_field(
    collection_name: &str,
    field_name: &str,
    field_def: &CollectionFieldDefinition,
) -> Result<(), String> {
    if field_name.is_empty() || field_name.len() > 100 {
        return Err(format!(
            "Field name must be 1-100 characters in collection '{collection_name}': '{field_name}'"
        ));
    }

    if !VALID_FIELD_TYPES.contains(&field_def.field_type.as_str()) {
        return Err(format!(
            "Invalid field type '{}' for field '{field_name}' in collection '{collection_name}'. \
             Valid types: {}",
            field_def.field_type,
            VALID_FIELD_TYPES.join(", ")
        ));
    }

    // Reference fields must specify a target
    if field_def.field_type == "reference" && field_def.reference.is_none() {
        return Err(format!(
            "Reference field '{field_name}' in collection '{collection_name}' must specify a 'reference' target"
        ));
    }

    Ok(())
}

/// Validate row data against a collection definition
pub fn validate_row_data(
    data: &serde_json::Value,
    definition: &CollectionDefinition,
) -> Result<(), String> {
    let obj = data
        .as_object()
        .ok_or_else(|| "Row data must be a JSON object".to_string())?;

    for (field_name, field_def) in &definition.fields {
        if field_def.required {
            match obj.get(field_name) {
                None | Some(serde_json::Value::Null) => {
                    return Err(format!("Required field '{field_name}' is missing"));
                }
                _ => {}
            }
        }

        // Type check present fields
        if let Some(value) = obj.get(field_name) {
            if !value.is_null() {
                validate_field_value(field_name, value, field_def)?;
            }
        }
    }

    Ok(())
}

/// Validate a field value against its definition
fn validate_field_value(
    field_name: &str,
    value: &serde_json::Value,
    field_def: &CollectionFieldDefinition,
) -> Result<(), String> {
    match field_def.field_type.as_str() {
        "string" | "date" | "datetime" | "uuid" | "reference" if !value.is_string() => {
            return Err(format!(
                "Field '{field_name}' must be a string, got {}",
                value_type_name(value)
            ));
        }
        "number" if !value.is_number() => {
            return Err(format!(
                "Field '{field_name}' must be a number, got {}",
                value_type_name(value)
            ));
        }
        "boolean" if !value.is_boolean() => {
            return Err(format!(
                "Field '{field_name}' must be a boolean, got {}",
                value_type_name(value)
            ));
        }
        "json" => {
            // Any JSON value is valid
        }
        _ => {}
    }

    Ok(())
}

fn value_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Sync collection schemas from a manifest to the database.
/// Creates new schemas, updates changed ones, and deletes removed ones.
pub fn sync_collection_schemas(
    conn: &mut DbConnection,
    plugin_id: i32,
    manifest: &PluginManifest,
) -> Result<(), String> {
    let existing = collection_repo::get_schemas_by_plugin(conn, plugin_id)
        .map_err(|e| format!("Failed to get existing schemas: {e}"))?;

    let _existing_names: std::collections::HashSet<String> =
        existing.iter().map(|s| s.collection_name.clone()).collect();

    let manifest_names: std::collections::HashSet<String> =
        manifest.collections.keys().cloned().collect();

    // Create or update schemas from manifest
    for (name, definition) in &manifest.collections {
        // Validate the definition
        validate_collection_definition(name, definition)?;

        let schema_json = serde_json::to_value(definition)
            .map_err(|e| format!("Failed to serialize collection definition: {e}"))?;

        if let Some(existing_schema) = existing.iter().find(|s| &s.collection_name == name) {
            // Check if schema changed
            if existing_schema.schema != schema_json {
                let update = PluginCollectionSchemaUpdate {
                    schema: Some(schema_json),
                    version: Some(existing_schema.version + 1),
                };
                collection_repo::update_schema(conn, existing_schema.id, update)
                    .map_err(|e| format!("Failed to update schema '{name}': {e}"))?;
                info!(
                    "Updated collection schema: {name} (v{})",
                    existing_schema.version + 1
                );
            }
        } else {
            // Create new schema
            let new_schema = NewPluginCollectionSchema {
                plugin_id,
                collection_name: name.clone(),
                schema: schema_json,
                version: 1,
            };
            collection_repo::create_schema(conn, new_schema)
                .map_err(|e| format!("Failed to create schema '{name}': {e}"))?;
            info!("Created collection schema: {name}");
        }
    }

    // Delete schemas removed from manifest (cascade deletes rows)
    for existing_schema in &existing {
        if !manifest_names.contains(&existing_schema.collection_name) {
            collection_repo::delete_schema(conn, existing_schema.id).map_err(|e| {
                format!(
                    "Failed to delete schema '{}': {e}",
                    existing_schema.collection_name
                )
            })?;
            warn!(
                "Deleted collection schema: {} (and all its data)",
                existing_schema.collection_name
            );
        }
    }

    Ok(())
}
