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
use crate::services::plugins::{signing, trust};
use crate::utils::encryption;

/// Env var name that bypasses signature verification for filesystem
/// provisioning. Debug builds honour it; release builds ignore it so
/// a production deployment can't be tricked into running unsigned
/// code by setting an env var.
const DEV_MODE_ENV: &str = "NOSDESK_DEV_MODE";

fn dev_mode_enabled() -> bool {
    if !cfg!(debug_assertions) {
        return false;
    }
    matches!(env::var(DEV_MODE_ENV), Ok(v) if v == "1" || v.eq_ignore_ascii_case("true"))
}

/// Walk a plugin directory into `ArchiveEntry` values so it can be
/// fed through `signing::verify_entries`. Only top-level files are
/// considered since that's all the filesystem-provisioned plugin
/// layout uses; if a plugin ever needs subdirectories we'd recurse.
///
/// Symlinks are skipped: `DirEntry::file_type()` does not follow
/// links (unlike `Path::is_file()`), so a malicious
/// `bundle.js -> /etc/passwd` never makes it into the verified set.
/// Also enforces the same per-entry + total-archive size budgets as
/// `signing::read_archive` so an unbounded `fs::read` can't be turned
/// into a DoS or memory probe.
fn read_directory_entries(dir: &Path) -> Result<Vec<signing::ArchiveEntry>, String> {
    use std::io::Read;
    let mut out = Vec::new();
    let mut total: u64 = 0;
    for entry in fs::read_dir(dir).map_err(|e| format!("read_dir: {e}"))? {
        let entry = entry.map_err(|e| format!("dir entry: {e}"))?;
        let file_type = entry
            .file_type()
            .map_err(|e| format!("file_type for {:?}: {e}", entry.path()))?;
        if !file_type.is_file() {
            // Skip directories AND symlinks. `file_type` does not
            // traverse links; `is_file()` returns false for a link
            // even when its target is a regular file.
            continue;
        }
        let name = match entry.file_name().to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };

        // Size-gate before we allocate. We use `metadata()` on the
        // DirEntry which (like `file_type`) avoids the symlink
        // traversal that `Path::metadata` would do.
        let metadata = entry
            .metadata()
            .map_err(|e| format!("metadata for {name}: {e}"))?;
        if metadata.len() > signing::MAX_ENTRY_SIZE {
            return Err(format!(
                "{name} exceeds per-entry size limit ({} bytes)",
                signing::MAX_ENTRY_SIZE
            ));
        }

        let path = entry.path();
        let file = fs::File::open(&path).map_err(|e| format!("open {name}: {e}"))?;
        let mut buf = Vec::with_capacity(metadata.len().min(signing::MAX_ENTRY_SIZE) as usize);
        let read = file
            .take(signing::MAX_ENTRY_SIZE + 1)
            .read_to_end(&mut buf)
            .map_err(|e| format!("read {name}: {e}"))?;
        if read as u64 > signing::MAX_ENTRY_SIZE {
            return Err(format!(
                "{name} exceeds per-entry size limit ({} bytes)",
                signing::MAX_ENTRY_SIZE
            ));
        }

        total = total
            .checked_add(read as u64)
            .ok_or_else(|| "total directory size overflow".to_string())?;
        if total > signing::MAX_TOTAL_SIZE {
            return Err(format!(
                "plugin directory exceeds total size limit ({} bytes)",
                signing::MAX_TOTAL_SIZE
            ));
        }

        out.push(signing::ArchiveEntry { name, bytes: buf });
    }
    Ok(out)
}

/// Outcome of running signature resolution against a plugin
/// directory. Packages both the DB-row fields and the verified file
/// bytes so update paths can stage bundle contents from memory
/// rather than re-reading from disk (which would reopen a TOCTOU on
/// symlink-racing attackers).
struct ResolvedDirectory {
    fields: trust::PluginSignerFields,
    files: Vec<signing::ArchiveEntry>,
}

/// Run the signature check against a plugin directory and resolve
/// the trust tier. Returns the verified entries alongside the DB
/// fields so the caller can use `signing::find_entry` to pull bytes
/// that actually passed verification.
fn resolve_signer_fields(
    conn: &mut DbConnection,
    plugin_dir: &Path,
    manifest_name: &str,
) -> Result<ResolvedDirectory, String> {
    let entries = read_directory_entries(plugin_dir)
        .map_err(|e| format!("Failed to read plugin directory: {e}"))?;

    match signing::verify_entries(entries.clone()) {
        Ok(verified) => {
            let tier = trust::resolve(conn, &verified.envelope)
                .map_err(|e| format!("Plugin publisher not trusted: {e}"))?;
            Ok(ResolvedDirectory {
                fields: trust::PluginSignerFields::from_verified(&verified, &tier),
                files: verified.files,
            })
        }
        Err(signing::SigningError::MissingSignature) => {
            if dev_mode_enabled() {
                warn!(
                    plugin = %manifest_name,
                    "Provisioning unsigned plugin because {}=1 is set. Never enable this in production.",
                    DEV_MODE_ENV
                );
                Ok(ResolvedDirectory {
                    fields: trust::PluginSignerFields::dev_mode(),
                    files: entries,
                })
            } else {
                Err(format!(
                    "Plugin is not signed. Add a {} file or set {}=1 in a debug build.",
                    signing::SIGNATURE_FILE,
                    DEV_MODE_ENV
                ))
            }
        }
        Err(e) => Err(format!("Plugin signature rejected: {e}")),
    }
}

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

    // Always resolve + verify up-front so the rest of the function
    // works against an in-memory, signature-checked byte set.
    // Re-reading from disk later would reopen a TOCTOU window where a
    // symlink-swapping attacker stages different content between
    // verification and bundle write.
    let resolved = match resolve_signer_fields(conn, plugin_dir, &manifest.name) {
        Ok(r) => r,
        Err(msg) => return ProvisionResult::Failed(manifest.name.clone(), msg),
    };
    let bundle_bytes = signing::find_entry(&resolved.files, "bundle.js");

    match existing {
        Ok(plugin) => {
            let existing_manifest: PluginManifest = match plugin.parse_manifest() {
                Ok(m) => m,
                Err(_) => {
                    return ProvisionResult::Failed(
                        manifest.name.clone(),
                        "Failed to parse existing manifest".to_string(),
                    );
                }
            };

            let version_changed = existing_manifest.version != manifest.version;
            let bundle_changed = bundle_hash_differs(&plugin, bundle_bytes);

            if !version_changed && !bundle_changed {
                provision_settings_from_env(conn, &plugin, &manifest);
                return ProvisionResult::Unchanged(manifest.name);
            }

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
                trust_level: Some(resolved.fields.trust_level.clone()),
                signer_pubkey: Some(resolved.fields.signer_pubkey.clone()),
                signer_source: Some(resolved.fields.signer_source.clone()),
                signature_metadata: Some(resolved.fields.signature_metadata.clone()),
            };

            if let Err(e) = plugin_repo::update_plugin_by_uuid(conn, plugin.uuid, update) {
                return ProvisionResult::Failed(
                    manifest.name.clone(),
                    format!("Failed to update plugin: {e}"),
                );
            }

            if let Some(bytes) = bundle_bytes {
                let _ = update_bundle_from_bytes(conn, &plugin, bytes);
            }

            if !manifest.collections.is_empty() {
                if let Err(e) = super::validation::sync_collection_schemas(conn, plugin.id, &manifest) {
                    warn!("Failed to sync collection schemas for plugin {}: {}", manifest.name, e);
                }
            }

            provision_settings_from_env(conn, &plugin, &manifest);

            info!("Updated provisioned plugin: {} v{}", manifest.name, manifest.version);
            ProvisionResult::Updated(manifest.name)
        }
        Err(diesel::result::Error::NotFound) => {
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
                trust_level: resolved.fields.trust_level.clone(),
                installed_by: None,
                source: "provisioned".to_string(),
                signer_pubkey: resolved.fields.signer_pubkey.clone(),
                signer_source: resolved.fields.signer_source.clone(),
                signature_metadata: resolved.fields.signature_metadata.clone(),
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

            if let Some(bytes) = bundle_bytes {
                let _ = update_bundle_from_bytes(conn, &plugin, bytes);
            }

            // Sync collection schemas
            if !manifest.collections.is_empty() {
                if let Err(e) = super::validation::sync_collection_schemas(conn, plugin.id, &manifest) {
                    warn!("Failed to sync collection schemas for plugin {}: {}", manifest.name, e);
                }
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

/// Stage bundle bytes for a plugin. Takes pre-verified bytes (not a
/// path) so the provisioner never re-reads from disk after signature
/// verification: a TOCTOU swap of `bundle.js` between verify and
/// write would otherwise install different bytes than we vouched for.
fn update_bundle_from_bytes(
    conn: &mut DbConnection,
    plugin: &crate::models::Plugin,
    content: &[u8],
) -> Result<(), String> {
    let mut context = Context::new(&SHA256);
    context.update(content);
    let hash = hex::encode(context.finish().as_ref());

    let upload_dir = PathBuf::from("/app/uploads/plugins").join(plugin.uuid.to_string());
    fs::create_dir_all(&upload_dir).map_err(|e| format!("Failed to create upload dir: {e}"))?;

    let dest_path = upload_dir.join("bundle.js");
    fs::write(&dest_path, content).map_err(|e| format!("Failed to write bundle: {e}"))?;

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

/// Returns true when the new (already-verified) bundle bytes differ
/// from what the plugin row knows about, or when the staged copy
/// under `/app/uploads/plugins/<uuid>/bundle.js` is missing (a stale
/// row after a backup restore). Either case means re-staging.
fn bundle_hash_differs(
    plugin: &crate::models::Plugin,
    new_bytes: Option<&[u8]>,
) -> bool {
    let Some(new_bytes) = new_bytes else {
        // No bundle on disk means nothing to restage; the row either
        // never had a bundle or the operator removed it on purpose.
        return false;
    };
    let mut context = Context::new(&SHA256);
    context.update(new_bytes);
    let new_hash = hex::encode(context.finish().as_ref());

    let dest_path = PathBuf::from("/app/uploads/plugins")
        .join(plugin.uuid.to_string())
        .join("bundle.js");
    let dest_exists = dest_path.exists();

    plugin.bundle_hash.as_ref() != Some(&new_hash) || !dest_exists
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
