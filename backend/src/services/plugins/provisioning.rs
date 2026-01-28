//! Plugin Provisioning Service
//!
//! Scans the /app/plugins/ directory on startup and syncs plugins to the database.
//! This enables infrastructure-as-code plugin management where plugins can be
//! provisioned via volume mounts.
//!
//! Expected directory structure:
//! /app/plugins/
//! ├── my-plugin/
//! │   ├── manifest.json
//! │   └── bundle.js (optional)
//! └── another-plugin/
//!     ├── manifest.json
//!     └── bundle.js
//!
//! Plugin settings can be provisioned via environment variables:
//! PLUGIN_{PLUGIN_NAME}_{SETTING_KEY}=value
//!
//! Example:
//! PLUGIN_GITHUB_INTEGRATION_GITHUB_TOKEN=ghp_xxxx
//! PLUGIN_GITHUB_INTEGRATION_DEFAULT_OWNER=myorg

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Utc;
use ring::digest::{Context, SHA256};
use tracing::{debug, error, info, warn};

use crate::db::DbConnection;
use crate::models::{NewPlugin, PluginBundleUpdate, PluginManifest};
use crate::repository::plugins as plugin_repo;
use crate::utils::encryption;

/// Default plugins directory path
const PLUGINS_DIR: &str = "/app/plugins";

/// Result of provisioning a single plugin
#[derive(Debug)]
pub enum ProvisionResult {
    Created(String),
    Updated(String),
    #[allow(dead_code)]
    Unchanged(String),
    Failed(String, String),
}

/// Provision all plugins from the plugins directory
pub fn provision_plugins(conn: &mut DbConnection) -> Vec<ProvisionResult> {
    let plugins_path = Path::new(PLUGINS_DIR);

    if !plugins_path.exists() {
        info!("Plugins directory does not exist, skipping provisioning");
        return vec![];
    }

    if !plugins_path.is_dir() {
        warn!("Plugins path is not a directory: {}", PLUGINS_DIR);
        return vec![];
    }

    let entries = match fs::read_dir(plugins_path) {
        Ok(e) => e,
        Err(e) => {
            error!("Failed to read plugins directory: {}", e);
            return vec![];
        }
    };

    let mut results = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let result = provision_plugin(conn, &path);
            results.push(result);
        }
    }

    // Log summary
    let created = results.iter().filter(|r| matches!(r, ProvisionResult::Created(_))).count();
    let updated = results.iter().filter(|r| matches!(r, ProvisionResult::Updated(_))).count();
    let unchanged = results.iter().filter(|r| matches!(r, ProvisionResult::Unchanged(_))).count();
    let failed = results.iter().filter(|r| matches!(r, ProvisionResult::Failed(_, _))).count();

    info!(
        "Plugin provisioning complete: {} created, {} updated, {} unchanged, {} failed",
        created, updated, unchanged, failed
    );

    results
}

/// Provision a single plugin from a directory
fn provision_plugin(conn: &mut DbConnection, plugin_dir: &Path) -> ProvisionResult {
    let dir_name = plugin_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    debug!("Provisioning plugin from: {}", plugin_dir.display());

    // Read manifest.json
    let manifest_path = plugin_dir.join("manifest.json");
    if !manifest_path.exists() {
        return ProvisionResult::Failed(
            dir_name.to_string(),
            "manifest.json not found".to_string(),
        );
    }

    let manifest_content = match fs::read_to_string(&manifest_path) {
        Ok(c) => c,
        Err(e) => {
            return ProvisionResult::Failed(
                dir_name.to_string(),
                format!("Failed to read manifest.json: {e}"),
            );
        }
    };

    let manifest: PluginManifest = match serde_json::from_str(&manifest_content) {
        Ok(m) => m,
        Err(e) => {
            return ProvisionResult::Failed(
                dir_name.to_string(),
                format!("Invalid manifest.json: {e}"),
            );
        }
    };

    // Check if plugin already exists
    let existing = plugin_repo::get_plugin_by_name(conn, &manifest.name);

    match existing {
        Ok(plugin) => {
            // Plugin exists - check if it needs updating
            let existing_manifest: PluginManifest = match plugin.parse_manifest() {
                Ok(m) => m,
                Err(_) => {
                    return ProvisionResult::Failed(
                        manifest.name.clone(),
                        "Failed to parse existing manifest".to_string(),
                    );
                }
            };

            // Check if manifest changed (compare versions for simplicity)
            if existing_manifest.version == manifest.version {
                // Check if bundle needs updating
                let bundle_path = plugin_dir.join("bundle.js");
                if bundle_path.exists() {
                    if let Some(result) = update_bundle_if_changed(conn, &plugin, &bundle_path) {
                        // Still provision settings even if bundle changed
                        provision_settings_from_env(conn, &plugin, &manifest);
                        return result;
                    }
                }
                // Always check for settings from environment, even if plugin unchanged
                provision_settings_from_env(conn, &plugin, &manifest);
                return ProvisionResult::Unchanged(manifest.name);
            }

            // Update the plugin
            let manifest_json = match serde_json::to_value(&manifest) {
                Ok(v) => v,
                Err(e) => {
                    return ProvisionResult::Failed(
                        manifest.name.clone(),
                        format!("Failed to serialize manifest: {e}"),
                    );
                }
            };

            let update = crate::models::PluginUpdate {
                display_name: Some(manifest.display_name.clone()),
                version: Some(manifest.version.clone()),
                description: manifest.description.clone(),
                manifest: Some(manifest_json),
                enabled: None,
                trust_level: None,
            };

            if let Err(e) = plugin_repo::update_plugin_by_uuid(conn, plugin.uuid, update) {
                return ProvisionResult::Failed(
                    manifest.name.clone(),
                    format!("Failed to update plugin: {e}"),
                );
            }

            // Update bundle if present
            let bundle_path = plugin_dir.join("bundle.js");
            if bundle_path.exists() {
                let _ = update_bundle(conn, &plugin, &bundle_path);
            }

            // Provision settings from environment variables
            provision_settings_from_env(conn, &plugin, &manifest);

            info!("Updated provisioned plugin: {} v{}", manifest.name, manifest.version);
            ProvisionResult::Updated(manifest.name)
        }
        Err(diesel::result::Error::NotFound) => {
            // Plugin doesn't exist - create it
            let manifest_json = match serde_json::to_value(&manifest) {
                Ok(v) => v,
                Err(e) => {
                    return ProvisionResult::Failed(
                        manifest.name.clone(),
                        format!("Failed to serialize manifest: {e}"),
                    );
                }
            };

            let new_plugin = NewPlugin {
                name: manifest.name.clone(),
                display_name: manifest.display_name.clone(),
                version: manifest.version.clone(),
                description: manifest.description.clone(),
                manifest: manifest_json,
                enabled: true,
                trust_level: "official".to_string(), // Provisioned plugins are trusted
                installed_by: None,
                source: "provisioned".to_string(),
            };

            let plugin = match plugin_repo::create_plugin(conn, new_plugin) {
                Ok(p) => p,
                Err(e) => {
                    return ProvisionResult::Failed(
                        manifest.name.clone(),
                        format!("Failed to create plugin: {e}"),
                    );
                }
            };

            // Upload bundle if present
            let bundle_path = plugin_dir.join("bundle.js");
            if bundle_path.exists() {
                let _ = update_bundle(conn, &plugin, &bundle_path);
            }

            // Provision settings from environment variables
            provision_settings_from_env(conn, &plugin, &manifest);

            info!("Created provisioned plugin: {} v{}", manifest.name, manifest.version);
            ProvisionResult::Created(manifest.name)
        }
        Err(e) => {
            ProvisionResult::Failed(
                manifest.name.clone(),
                format!("Database error: {e}"),
            )
        }
    }
}

/// Update bundle for a plugin
fn update_bundle(
    conn: &mut DbConnection,
    plugin: &crate::models::Plugin,
    bundle_path: &Path,
) -> Result<(), String> {
    let content = fs::read(bundle_path).map_err(|e| format!("Failed to read bundle: {e}"))?;

    // Calculate hash
    let mut context = Context::new(&SHA256);
    context.update(&content);
    let hash = hex::encode(context.finish().as_ref());

    // Copy bundle to uploads directory
    let upload_dir = PathBuf::from("/app/uploads/plugins").join(plugin.uuid.to_string());
    fs::create_dir_all(&upload_dir).map_err(|e| format!("Failed to create upload dir: {e}"))?;

    let dest_path = upload_dir.join("bundle.js");
    fs::copy(bundle_path, &dest_path).map_err(|e| format!("Failed to copy bundle: {e}"))?;

    // Update database
    let update = PluginBundleUpdate {
        bundle_hash: Some(hash),
        bundle_size: Some(content.len() as i32),
        bundle_uploaded_at: Some(Utc::now().naive_utc()),
    };

    plugin_repo::update_plugin_bundle(conn, plugin.uuid, update)
        .map_err(|e| format!("Failed to update bundle metadata: {e}"))?;

    debug!("Updated bundle for plugin: {}", plugin.name);
    Ok(())
}

/// Check if bundle changed and update if needed
fn update_bundle_if_changed(
    conn: &mut DbConnection,
    plugin: &crate::models::Plugin,
    bundle_path: &Path,
) -> Option<ProvisionResult> {
    let content = match fs::read(bundle_path) {
        Ok(c) => c,
        Err(e) => {
            return Some(ProvisionResult::Failed(
                plugin.name.clone(),
                format!("Failed to read bundle: {e}"),
            ));
        }
    };

    // Calculate hash
    let mut context = Context::new(&SHA256);
    context.update(&content);
    let new_hash = hex::encode(context.finish().as_ref());

    // Compare with existing hash
    if plugin.bundle_hash.as_ref() == Some(&new_hash) {
        return None; // Bundle unchanged
    }

    // Bundle changed - update it
    if let Err(e) = update_bundle(conn, plugin, bundle_path) {
        return Some(ProvisionResult::Failed(plugin.name.clone(), e));
    }

    Some(ProvisionResult::Updated(plugin.name.clone()))
}

/// Provision plugin settings from environment variables.
///
/// Looks for environment variables matching the pattern:
/// PLUGIN_{PLUGIN_NAME}_{SETTING_KEY}=value
///
/// Where PLUGIN_NAME and SETTING_KEY are uppercase with hyphens replaced by underscores.
/// Example: PLUGIN_GITHUB_INTEGRATION_GITHUB_TOKEN=ghp_xxxx
fn provision_settings_from_env(
    conn: &mut DbConnection,
    plugin: &crate::models::Plugin,
    manifest: &PluginManifest,
) {
    // Convert plugin name to env prefix: "github-integration" -> "PLUGIN_GITHUB_INTEGRATION_"
    let env_prefix = format!(
        "PLUGIN_{}_",
        plugin.name.to_uppercase().replace('-', "_")
    );

    let mut settings_count = 0;

    for setting_def in &manifest.settings {
        // Convert setting key to env var name: "github_token" -> "GITHUB_TOKEN"
        let env_key = format!(
            "{}{}",
            env_prefix,
            setting_def.key.to_uppercase().replace('-', "_")
        );

        if let Ok(value) = env::var(&env_key) {
            // Determine if this is a secret setting
            let is_secret = setting_def.setting_type == "secret";

            // Convert to JSON value, encrypting secrets
            let json_value = if is_secret {
                match encryption::encrypt(&value) {
                    Ok(encrypted) => serde_json::Value::String(encrypted),
                    Err(e) => {
                        warn!(
                            "Failed to encrypt secret setting {} for plugin {}: {}. Ensure ENCRYPTION_KEY is set.",
                            setting_def.key, plugin.name, e
                        );
                        continue;
                    }
                }
            } else {
                serde_json::Value::String(value)
            };

            match plugin_repo::set_plugin_setting(
                conn,
                plugin.id,
                setting_def.key.clone(),
                Some(json_value),
                is_secret,
            ) {
                Ok(_) => {
                    settings_count += 1;
                    if is_secret {
                        debug!(
                            "Provisioned secret setting from env: {} -> {}",
                            env_key, setting_def.key
                        );
                    } else {
                        debug!(
                            "Provisioned setting from env: {} -> {}",
                            env_key, setting_def.key
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to provision setting {} for plugin {}: {}",
                        setting_def.key, plugin.name, e
                    );
                }
            }
        }
    }

    if settings_count > 0 {
        info!(
            "Provisioned {} settings from environment for plugin: {}",
            settings_count, plugin.name
        );
    }
}
