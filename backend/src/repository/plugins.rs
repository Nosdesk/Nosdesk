//! Plugin Repository
//!
//! Provides database operations for plugins, data (settings/storage), and activity logging.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    NewPlugin, NewPluginActivity, NewPluginData, Plugin, PluginActivity, PluginBundleUpdate,
    PluginData, PluginDataUpdate, PluginUpdate,
};
use crate::schema::{plugin_activity, plugin_data, plugins};

// =============================================================================
// Plugins
// =============================================================================

/// List all plugins
pub fn list_all_plugins(conn: &mut DbConnection) -> Result<Vec<Plugin>, diesel::result::Error> {
    plugins::table
        .order(plugins::installed_at.desc())
        .load::<Plugin>(conn)
}

/// List enabled plugins
pub fn list_enabled_plugins(conn: &mut DbConnection) -> Result<Vec<Plugin>, diesel::result::Error> {
    plugins::table
        .filter(plugins::enabled.eq(true))
        .order(plugins::name.asc())
        .load::<Plugin>(conn)
}

/// Get a plugin by name
pub fn get_plugin_by_name(
    conn: &mut DbConnection,
    name: &str,
) -> Result<Plugin, diesel::result::Error> {
    plugins::table
        .filter(plugins::name.eq(name))
        .first::<Plugin>(conn)
}

/// Get a plugin by UUID
pub fn get_plugin_by_uuid(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
) -> Result<Plugin, diesel::result::Error> {
    plugins::table
        .filter(plugins::uuid.eq(plugin_uuid))
        .first::<Plugin>(conn)
}

/// Create a new plugin
pub fn create_plugin(
    conn: &mut DbConnection,
    new_plugin: NewPlugin,
) -> Result<Plugin, diesel::result::Error> {
    diesel::insert_into(plugins::table)
        .values(&new_plugin)
        .get_result(conn)
}

/// Update a plugin by UUID
pub fn update_plugin_by_uuid(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
    update: PluginUpdate,
) -> Result<Plugin, diesel::result::Error> {
    diesel::update(plugins::table.filter(plugins::uuid.eq(plugin_uuid)))
        .set(&update)
        .get_result(conn)
}

/// Delete a plugin by UUID
pub fn delete_plugin_by_uuid(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(plugins::table.filter(plugins::uuid.eq(plugin_uuid))).execute(conn)
}

/// Update a plugin's bundle metadata
pub fn update_plugin_bundle(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
    update: PluginBundleUpdate,
) -> Result<Plugin, diesel::result::Error> {
    diesel::update(plugins::table.filter(plugins::uuid.eq(plugin_uuid)))
        .set(&update)
        .get_result(conn)
}

// =============================================================================
// Plugin Data (Settings + Storage consolidated)
// =============================================================================

/// Get all data entries of a specific type for a plugin
pub fn get_plugin_data(
    conn: &mut DbConnection,
    plugin_id: i32,
    data_type: &str,
) -> Result<Vec<PluginData>, diesel::result::Error> {
    plugin_data::table
        .filter(plugin_data::plugin_id.eq(plugin_id))
        .filter(plugin_data::data_type.eq(data_type))
        .order(plugin_data::key.asc())
        .load::<PluginData>(conn)
}

/// Get a specific data entry for a plugin
pub fn get_plugin_data_entry(
    conn: &mut DbConnection,
    plugin_id: i32,
    data_type: &str,
    key: &str,
) -> Result<PluginData, diesel::result::Error> {
    plugin_data::table
        .filter(plugin_data::plugin_id.eq(plugin_id))
        .filter(plugin_data::data_type.eq(data_type))
        .filter(plugin_data::key.eq(key))
        .first::<PluginData>(conn)
}

/// Set a plugin data entry (upsert)
pub fn set_plugin_data(
    conn: &mut DbConnection,
    plugin_id: i32,
    data_type: &str,
    key: String,
    value: Option<serde_json::Value>,
    is_secret: bool,
) -> Result<PluginData, diesel::result::Error> {
    // Try to update existing
    let existing = plugin_data::table
        .filter(plugin_data::plugin_id.eq(plugin_id))
        .filter(plugin_data::data_type.eq(data_type))
        .filter(plugin_data::key.eq(&key))
        .first::<PluginData>(conn);

    match existing {
        Ok(entry) => {
            let update = PluginDataUpdate {
                value: Some(value),
            };
            diesel::update(plugin_data::table.filter(plugin_data::id.eq(entry.id)))
                .set(&update)
                .get_result(conn)
        }
        Err(diesel::result::Error::NotFound) => {
            // Insert new
            let new_entry = NewPluginData {
                plugin_id,
                data_type: data_type.to_string(),
                key,
                value,
                is_secret,
            };
            diesel::insert_into(plugin_data::table)
                .values(&new_entry)
                .get_result(conn)
        }
        Err(e) => Err(e),
    }
}

/// Delete a plugin data entry
pub fn delete_plugin_data_entry(
    conn: &mut DbConnection,
    plugin_id: i32,
    data_type: &str,
    key: &str,
) -> Result<usize, diesel::result::Error> {
    diesel::delete(
        plugin_data::table
            .filter(plugin_data::plugin_id.eq(plugin_id))
            .filter(plugin_data::data_type.eq(data_type))
            .filter(plugin_data::key.eq(key)),
    )
    .execute(conn)
}

// =============================================================================
// Convenience functions for Settings (data_type = 'setting')
// =============================================================================

/// Get all settings for a plugin
pub fn get_plugin_settings(
    conn: &mut DbConnection,
    plugin_id: i32,
) -> Result<Vec<PluginData>, diesel::result::Error> {
    get_plugin_data(conn, plugin_id, "setting")
}

/// Set a plugin setting (upsert)
pub fn set_plugin_setting(
    conn: &mut DbConnection,
    plugin_id: i32,
    key: String,
    value: Option<serde_json::Value>,
    is_secret: bool,
) -> Result<PluginData, diesel::result::Error> {
    set_plugin_data(conn, plugin_id, "setting", key, value, is_secret)
}

/// Delete a plugin setting
pub fn delete_plugin_setting(
    conn: &mut DbConnection,
    plugin_id: i32,
    key: &str,
) -> Result<usize, diesel::result::Error> {
    delete_plugin_data_entry(conn, plugin_id, "setting", key)
}

// =============================================================================
// Convenience functions for Storage (data_type = 'storage')
// =============================================================================

/// Get a specific storage entry for a plugin
pub fn get_plugin_storage_entry(
    conn: &mut DbConnection,
    plugin_id: i32,
    key: &str,
) -> Result<PluginData, diesel::result::Error> {
    get_plugin_data_entry(conn, plugin_id, "storage", key)
}

/// Set a plugin storage entry (upsert)
pub fn set_plugin_storage(
    conn: &mut DbConnection,
    plugin_id: i32,
    key: String,
    value: Option<serde_json::Value>,
) -> Result<PluginData, diesel::result::Error> {
    set_plugin_data(conn, plugin_id, "storage", key, value, false)
}

/// Delete a plugin storage entry
pub fn delete_plugin_storage_entry(
    conn: &mut DbConnection,
    plugin_id: i32,
    key: &str,
) -> Result<usize, diesel::result::Error> {
    delete_plugin_data_entry(conn, plugin_id, "storage", key)
}

// =============================================================================
// Plugin Activity
// =============================================================================

/// Log a plugin activity
pub fn log_plugin_activity(
    conn: &mut DbConnection,
    plugin_id: i32,
    action: String,
    details: Option<serde_json::Value>,
    user_uuid: Option<Uuid>,
) -> Result<PluginActivity, diesel::result::Error> {
    let new_activity = NewPluginActivity {
        plugin_id,
        action,
        details,
        user_uuid,
    };

    diesel::insert_into(plugin_activity::table)
        .values(&new_activity)
        .get_result(conn)
}

/// Get activity log for a plugin (paginated)
pub fn get_plugin_activity(
    conn: &mut DbConnection,
    plugin_id: i32,
    limit: i64,
    offset: i64,
) -> Result<Vec<PluginActivity>, diesel::result::Error> {
    plugin_activity::table
        .filter(plugin_activity::plugin_id.eq(plugin_id))
        .order(plugin_activity::created_at.desc())
        .limit(limit)
        .offset(offset)
        .load::<PluginActivity>(conn)
}
