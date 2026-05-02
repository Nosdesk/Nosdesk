//! Plugin Repository
//!
//! Provides database operations for plugins, data (settings/storage), and activity logging.

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    NewPlugin, NewPluginActivity, NewPluginData, Plugin, PluginActivity, PluginBundleUpdate,
    PluginData, PluginUpdate, SyncAggregate, SyncOp,
};
use crate::schema::{plugin_activity, plugin_data, plugins};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

// =============================================================================
// Plugins
// =============================================================================

/// List all plugins
pub fn list_all_plugins(conn: &mut DbConnection) -> Result<Vec<Plugin>, diesel::result::Error> {
    plugins::table
        .order(plugins::installed_at.desc())
        .load::<Plugin>(conn)
}

/// List plugins in the `installed` lifecycle state. Other states
/// (`disabled`, `quarantined`, `uninstalled`) are filtered out;
/// they're rendered in admin views via `list_all_plugins` but
/// don't appear in the runtime loader.
pub fn list_enabled_plugins(conn: &mut DbConnection) -> Result<Vec<Plugin>, diesel::result::Error> {
    plugins::table
        .filter(plugins::state.eq(crate::models::PluginState::Installed))
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

/// Create a new plugin row.
///
/// The [`InstallToken`](crate::services::plugins::install::InstallToken)
/// argument is structurally how this function says "I refuse to
/// insert plugin rows that haven't been through the verified
/// install pipeline." The token's only public constructor is
/// private to `services::plugins::install`, so the only code that
/// can call this function in production is the install pipeline
/// itself. Tests use `InstallToken::for_test()` (gated by
/// `#[cfg(test)]`) to construct fixtures.
///
/// This closes the historical bypass where any handler could
/// build a `NewPlugin` and call `create_plugin` directly,
/// sidestepping signature verification, manifest validation, and
/// trust-tier resolution.
pub fn create_plugin(
    conn: &mut DbConnection,
    new_plugin: NewPlugin,
    _token: crate::services::plugins::install::InstallToken,
) -> Result<Plugin, diesel::result::Error> {
    conn.transaction(|conn| {
        let plugin: Plugin = diesel::insert_into(plugins::table)
            .values(&new_plugin)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Plugin,
                aggregate_id: plugin.uuid.to_string(),
                op: SyncOp::Insert,
                event_type: "plugin.installed",
                data: json!({
                    "uuid": plugin.uuid,
                    "name": plugin.name,
                    "version": plugin.version,
                    "trust_level": plugin.trust_level,
                    "source": plugin.source,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(plugin)
    })
}

/// Update a plugin by UUID
pub fn update_plugin_by_uuid(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
    update: PluginUpdate,
) -> Result<Plugin, diesel::result::Error> {
    conn.transaction(|conn| {
        let plugin: Plugin = diesel::update(plugins::table.filter(plugins::uuid.eq(plugin_uuid)))
            .set(&update)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Plugin,
                aggregate_id: plugin.uuid.to_string(),
                op: SyncOp::Update,
                event_type: "plugin.updated",
                data: json!({
                    "uuid": plugin.uuid,
                    "name": plugin.name,
                    "version": plugin.version,
                    "trust_level": plugin.trust_level,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(plugin)
    })
}

/// Delete a plugin by UUID
pub fn delete_plugin_by_uuid(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
) -> Result<usize, diesel::result::Error> {
    conn.transaction(|conn| {
        let result = diesel::delete(plugins::table.filter(plugins::uuid.eq(plugin_uuid)))
            .execute(conn)?;
        if result > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::Plugin,
                    aggregate_id: plugin_uuid.to_string(),
                    op: SyncOp::Delete,
                    event_type: "plugin.uninstalled",
                    data: json!({ "uuid": plugin_uuid }),
                    groups: groups::workspace(),
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })
}

/// Update a plugin's bundle metadata
pub fn update_plugin_bundle(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
    update: PluginBundleUpdate,
) -> Result<Plugin, diesel::result::Error> {
    conn.transaction(|conn| {
        let plugin: Plugin = diesel::update(plugins::table.filter(plugins::uuid.eq(plugin_uuid)))
            .set(&update)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Plugin,
                aggregate_id: plugin.uuid.to_string(),
                op: SyncOp::Update,
                event_type: "plugin.bundle_updated",
                data: json!({
                    "uuid": plugin.uuid,
                    "version": plugin.version,
                    "bundle_hash": plugin.bundle_hash,
                    "bundle_size": plugin.bundle_size,
                }),
                groups: groups::workspace(),
                causation_id: None,
            },
        )?;
        Ok(plugin)
    })
}

/// Fetch the lifecycle state plus the icon bytes for a plugin.
/// Returning both atomically lets callers gate serving on
/// `state == Installed` without a separate round-trip; selecting
/// just these two columns avoids loading the manifest JSON or
/// signer metadata on every icon request.
pub fn get_plugin_icon(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
) -> Result<(crate::models::PluginState, Option<Vec<u8>>), diesel::result::Error> {
    plugins::table
        .filter(plugins::uuid.eq(plugin_uuid))
        .select((plugins::state, plugins::icon_svg))
        .first::<(crate::models::PluginState, Option<Vec<u8>>)>(conn)
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

/// Set a plugin data entry (upsert).
///
/// Uses Postgres `ON CONFLICT (plugin_id, data_type, key) DO UPDATE`
/// so two concurrent writers can't both see a NotFound and race to
/// INSERT — one would lose with a unique-constraint violation
/// surfaced as a 500. The unique index on (plugin_id, data_type,
/// key) is defined in the consolidate_plugin_data migration.
///
/// `is_secret` is set on insert and intentionally NOT changed on
/// update: a setting's secret-ness is a manifest property and must
/// not be flipped by a runtime caller. If a plugin's manifest later
/// reclassifies a key, that flows through reinstall, not through a
/// data write.
pub fn set_plugin_data(
    conn: &mut DbConnection,
    plugin_id: i32,
    data_type: &str,
    key: String,
    value: Option<serde_json::Value>,
    is_secret: bool,
) -> Result<PluginData, diesel::result::Error> {
    let new_entry = NewPluginData {
        plugin_id,
        data_type: data_type.to_string(),
        key,
        value,
        is_secret,
    };
    diesel::insert_into(plugin_data::table)
        .values(&new_entry)
        .on_conflict((
            plugin_data::plugin_id,
            plugin_data::data_type,
            plugin_data::key,
        ))
        .do_update()
        .set(plugin_data::value.eq(diesel::upsert::excluded(plugin_data::value)))
        .get_result(conn)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::plugins::install::InstallToken;
    use crate::test_helpers::setup_test_connection;

    /// Test fixture: insert a plugin row using the test-only
    /// `InstallToken::for_test()` constructor. Production code
    /// can only build a token through the verified install
    /// pipeline; tests bypass that for fixture setup.
    fn create_test_plugin(conn: &mut DbConnection, name: &str, enabled: bool) -> Plugin {
        create_plugin(conn, make_new_plugin(name, enabled), InstallToken::for_test())
            .expect("test plugin insert must succeed")
    }

    fn make_new_plugin(name: &str, enabled: bool) -> NewPlugin {
        NewPlugin {
            name: name.to_string(),
            display_name: name.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            manifest: serde_json::json!({}),
            state: if enabled {
                crate::models::PluginState::Installed
            } else {
                crate::models::PluginState::Disabled
            },
            trust_level: "sandbox".to_string(),
            installed_by: None,
            source: "test".to_string(),
            signer_pubkey: None,
            signer_source: None,
            signature_metadata: None,
            icon_svg: None,
        }
    }

    #[test]
    fn create_and_get_plugin() {
        let mut conn = setup_test_connection();
        let plugin = create_test_plugin(&mut conn, "test-plugin", true);

        let fetched = get_plugin_by_uuid(&mut conn, plugin.uuid).unwrap();
        assert_eq!(fetched.name, "test-plugin");
        assert_eq!(fetched.id, plugin.id);
    }

    #[test]
    fn list_all_plugins_test() {
        let mut conn = setup_test_connection();
        create_test_plugin(&mut conn, "plug-a", true);
        create_test_plugin(&mut conn, "plug-b", false);

        let all = list_all_plugins(&mut conn).unwrap();
        let names: Vec<&str> = all.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"plug-a"));
        assert!(names.contains(&"plug-b"));
    }

    #[test]
    fn list_enabled_plugins_test() {
        let mut conn = setup_test_connection();
        create_test_plugin(&mut conn, "enabled-plug", true);
        create_test_plugin(&mut conn, "disabled-plug", false);

        let enabled = super::list_enabled_plugins(&mut conn).unwrap();
        let names: Vec<&str> = enabled.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"enabled-plug"));
        assert!(!names.contains(&"disabled-plug"));
    }

    #[test]
    fn delete_plugin_test() {
        let mut conn = setup_test_connection();
        let plugin = create_test_plugin(&mut conn, "doomed", true);

        delete_plugin_by_uuid(&mut conn, plugin.uuid).unwrap();
        assert!(get_plugin_by_uuid(&mut conn, plugin.uuid).is_err());
    }

    #[test]
    fn plugin_data_crud() {
        let mut conn = setup_test_connection();
        let plugin = create_test_plugin(&mut conn, "data-plug", true);

        // Set data
        let entry = set_plugin_data(
            &mut conn,
            plugin.id,
            "setting",
            "api_key".to_string(),
            Some(serde_json::json!("secret123")),
            true,
        )
        .unwrap();
        assert_eq!(entry.key, "api_key");

        // Get data entry
        let fetched = get_plugin_data_entry(&mut conn, plugin.id, "setting", "api_key").unwrap();
        assert_eq!(fetched.value, Some(serde_json::json!("secret123")));

        // Delete data entry
        let deleted = delete_plugin_data_entry(&mut conn, plugin.id, "setting", "api_key").unwrap();
        assert_eq!(deleted, 1);
        assert!(get_plugin_data_entry(&mut conn, plugin.id, "setting", "api_key").is_err());
    }
}
