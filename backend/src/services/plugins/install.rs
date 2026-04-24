//! Shared install pipeline for already-verified plugin archives.
//!
//! Both the HTTP upload handler and the filesystem provisioner
//! verify signatures and resolve trust chains themselves, then pass
//! the verified bytes plus resolved signer fields into this core.
//! Keeping the upsert + bundle-staging + settings + activity-log
//! sequence in one place ensures every entry point lands the same
//! row shape, stages bundles the same way, and logs the same audit
//! trail.

use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use ring::digest::{Context, SHA256};
use tracing::{debug, error, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    NewPlugin, Plugin, PluginBundleUpdate, PluginManifest, PluginUpdate,
};
use crate::repository::plugins as plugin_repo;
use crate::services::plugins::{signing, trust, validation};

/// Cap on the staged bundle.js size. The outer archive cap is
/// enforced by `signing::MAX_ENTRY_SIZE` at verification time; this
/// is a tighter practical limit specifically for plugin JS bundles.
pub const MAX_BUNDLE_SIZE: usize = 500 * 1024;

/// Caller policy knobs. The install core is entry-point-agnostic;
/// these fields describe what an install means from the invoking
/// path's perspective.
#[derive(Debug, Clone, Copy)]
pub struct InstallOptions {
    /// Stamped into `plugins.source`. Use `"uploaded"` for HTTP
    /// admin installs, `"provisioned"` for filesystem installs.
    pub source: &'static str,
    /// Admin UUID for audit; `None` for system-initiated installs.
    pub installed_by: Option<Uuid>,
    /// Emit a `plugin_activity` row describing the install/update.
    /// Provisioned installs skip this since they aren't user actions.
    pub log_activity: bool,
    /// Walk manifest settings and pull values from
    /// `PLUGIN_<NAME>_<KEY>` env vars. Only meaningful for
    /// provisioned installs where env is the secret surface.
    pub provision_settings: bool,
    /// If the DB already has the plugin at the same version with a
    /// matching staged bundle hash, return `Unchanged` without
    /// touching any rows. The provisioner runs on every boot and
    /// wants this; the HTTP handler always upserts.
    pub skip_if_unchanged: bool,
}

#[derive(Debug)]
pub enum InstallOutcome {
    Created(Plugin),
    Updated(Plugin),
    Unchanged(Plugin),
}

impl InstallOutcome {
    pub fn plugin(&self) -> &Plugin {
        match self {
            Self::Created(p) | Self::Updated(p) | Self::Unchanged(p) => p,
        }
    }
}

#[derive(Debug)]
pub enum InstallError {
    MissingManifest,
    InvalidManifest(String),
    InvalidName(String),
    BundleTooLarge(usize),
    BundleWriteFailed(String),
    Db(diesel::result::Error),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingManifest => write!(f, "zip does not contain manifest.json"),
            Self::InvalidManifest(m) => write!(f, "invalid manifest.json: {m}"),
            Self::InvalidName(m) => write!(f, "invalid plugin name: {m}"),
            Self::BundleTooLarge(sz) => {
                write!(f, "bundle is {sz} bytes, exceeds {MAX_BUNDLE_SIZE}")
            }
            Self::BundleWriteFailed(m) => write!(f, "failed to stage bundle: {m}"),
            // Raw Diesel error stays in the log, not the response.
            Self::Db(_) => write!(f, "database error"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<diesel::result::Error> for InstallError {
    fn from(value: diesel::result::Error) -> Self {
        error!(error = %value, "Plugin install DB error");
        InstallError::Db(value)
    }
}

/// Run a verified archive through the upsert + bundle-staging
/// pipeline. Signature verification + trust resolution are the
/// caller's job; by the time the bytes get here, they've been
/// vouched for.
pub fn install_verified(
    conn: &mut DbConnection,
    files: &[signing::ArchiveEntry],
    signer: trust::PluginSignerFields,
    options: InstallOptions,
) -> Result<InstallOutcome, InstallError> {
    let manifest_bytes =
        signing::find_entry(files, "manifest.json").ok_or(InstallError::MissingManifest)?;
    let manifest: PluginManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|e| InstallError::InvalidManifest(e.to_string()))?;

    validate_plugin_name(&manifest.name)?;

    let bundle_bytes = signing::find_entry(files, "bundle.js");
    if let Some(b) = bundle_bytes {
        if b.len() > MAX_BUNDLE_SIZE {
            return Err(InstallError::BundleTooLarge(b.len()));
        }
    }

    let manifest_json = serde_json::to_value(&manifest)
        .map_err(|e| InstallError::InvalidManifest(e.to_string()))?;

    let existing = plugin_repo::get_plugin_by_name(conn, &manifest.name).ok();

    let outcome = match existing {
        Some(existing) if options.skip_if_unchanged => {
            let existing_manifest: PluginManifest = existing
                .parse_manifest()
                .map_err(|e| InstallError::InvalidManifest(e.to_string()))?;
            let version_same = existing_manifest.version == manifest.version;
            let bundle_same = !bundle_hash_differs(&existing, bundle_bytes);
            if version_same && bundle_same {
                if options.provision_settings {
                    provision_settings_from_env(conn, &existing, &manifest);
                }
                return Ok(InstallOutcome::Unchanged(existing));
            }
            InstallOutcome::Updated(update_row(
                conn, existing, &manifest, manifest_json, &signer,
            )?)
        }
        Some(existing) => InstallOutcome::Updated(update_row(
            conn, existing, &manifest, manifest_json, &signer,
        )?),
        None => InstallOutcome::Created(create_row(
            conn, &manifest, manifest_json, signer.clone(), &options,
        )?),
    };

    let plugin = outcome.plugin();

    if let Some(bytes) = bundle_bytes {
        stage_bundle(conn, plugin, bytes)?;
    }

    if !manifest.collections.is_empty() {
        if let Err(e) = validation::sync_collection_schemas(conn, plugin.id, &manifest) {
            warn!(
                "Failed to sync collection schemas for plugin {}: {}",
                manifest.name, e
            );
        }
    }

    if options.provision_settings {
        provision_settings_from_env(conn, plugin, &manifest);
    }

    if options.log_activity {
        let action = match &outcome {
            InstallOutcome::Created(_) => "installed",
            InstallOutcome::Updated(_) => "updated",
            InstallOutcome::Unchanged(_) => "unchanged",
        };
        let _ = plugin_repo::log_plugin_activity(
            conn,
            plugin.id,
            action.to_string(),
            Some(serde_json::json!({
                "version": manifest.version,
                "source": options.source,
                "has_bundle": bundle_bytes.is_some(),
                "signer_source": signer.signer_source,
                "trust_level": signer.trust_level,
            })),
            options.installed_by,
        );
    }

    Ok(outcome)
}

fn update_row(
    conn: &mut DbConnection,
    existing: Plugin,
    manifest: &PluginManifest,
    manifest_json: serde_json::Value,
    signer: &trust::PluginSignerFields,
) -> Result<Plugin, InstallError> {
    let update = PluginUpdate {
        display_name: Some(manifest.display_name.clone()),
        version: Some(manifest.version.clone()),
        description: manifest.description.clone(),
        manifest: Some(manifest_json),
        enabled: None,
        trust_level: Some(signer.trust_level.clone()),
        signer_pubkey: signer.signer_pubkey.clone(),
        signer_source: signer.signer_source.clone(),
        signature_metadata: signer.signature_metadata.clone(),
    };
    Ok(plugin_repo::update_plugin_by_uuid(conn, existing.uuid, update)?)
}

fn create_row(
    conn: &mut DbConnection,
    manifest: &PluginManifest,
    manifest_json: serde_json::Value,
    signer: trust::PluginSignerFields,
    options: &InstallOptions,
) -> Result<Plugin, InstallError> {
    let new_plugin = NewPlugin {
        name: manifest.name.clone(),
        display_name: manifest.display_name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        manifest: manifest_json,
        enabled: true,
        trust_level: signer.trust_level,
        installed_by: options.installed_by,
        source: options.source.to_string(),
        signer_pubkey: signer.signer_pubkey,
        signer_source: signer.signer_source,
        signature_metadata: signer.signature_metadata,
    };
    Ok(plugin_repo::create_plugin(conn, new_plugin)?)
}

/// Stage pre-verified bundle bytes to the uploads volume. Takes
/// `&[u8]` rather than a path so we never re-source the bytes from
/// disk after verification, which would reopen a TOCTOU window.
fn stage_bundle(
    conn: &mut DbConnection,
    plugin: &Plugin,
    content: &[u8],
) -> Result<(), InstallError> {
    let mut ctx = Context::new(&SHA256);
    ctx.update(content);
    let hash = hex::encode(ctx.finish().as_ref());

    let upload_dir = PathBuf::from("/app/uploads/plugins").join(plugin.uuid.to_string());
    fs::create_dir_all(&upload_dir)
        .map_err(|e| InstallError::BundleWriteFailed(format!("create_dir_all: {e}")))?;

    let dest_path = upload_dir.join("bundle.js");
    fs::write(&dest_path, content)
        .map_err(|e| InstallError::BundleWriteFailed(format!("write: {e}")))?;

    let update = PluginBundleUpdate {
        bundle_hash: Some(hash),
        bundle_size: Some(content.len() as i32),
        bundle_uploaded_at: Some(Utc::now().naive_utc()),
    };
    plugin_repo::update_plugin_bundle(conn, plugin.uuid, update)?;

    debug!("Staged bundle for plugin: {}", plugin.name);
    Ok(())
}

/// True when new bundle bytes differ from what the `plugins` row
/// records OR the staged copy is missing (stale row after a backup
/// restore). Applied by both entry points so a restored backup
/// doesn't get stuck serving missing bundles.
fn bundle_hash_differs(plugin: &Plugin, new_bytes: Option<&[u8]>) -> bool {
    let Some(new_bytes) = new_bytes else {
        return false;
    };
    let mut ctx = Context::new(&SHA256);
    ctx.update(new_bytes);
    let new_hash = hex::encode(ctx.finish().as_ref());

    let dest_path = PathBuf::from("/app/uploads/plugins")
        .join(plugin.uuid.to_string())
        .join("bundle.js");
    let dest_exists = dest_path.exists();

    plugin.bundle_hash.as_ref() != Some(&new_hash) || !dest_exists
}

/// Names must be 1-100 chars, lowercase ASCII letters / digits /
/// hyphens. Shared by HTTP and filesystem installs so neither can
/// smuggle in a name the other would reject.
fn validate_plugin_name(name: &str) -> Result<(), InstallError> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(InstallError::InvalidName("name is required".into()));
    }
    if trimmed.len() > 100 {
        return Err(InstallError::InvalidName(
            "must be 100 characters or fewer".into(),
        ));
    }
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(InstallError::InvalidName(
            "only lowercase ASCII letters, digits, and hyphens".into(),
        ));
    }
    if trimmed != name {
        return Err(InstallError::InvalidName(
            "leading / trailing whitespace is not allowed".into(),
        ));
    }
    Ok(())
}

/// Provision plugin settings from environment variables using the
/// pattern `PLUGIN_{PLUGIN_NAME}_{SETTING_KEY}=value`. Plugin name
/// and setting key are uppercased with hyphens converted to
/// underscores. Secret settings are encrypted before storage.
fn provision_settings_from_env(
    conn: &mut DbConnection,
    plugin: &Plugin,
    manifest: &PluginManifest,
) {
    use crate::utils::encryption;

    let env_prefix = format!("PLUGIN_{}_", plugin.name.to_uppercase().replace('-', "_"));
    let mut settings_count = 0;

    for setting_def in &manifest.settings {
        let env_key = format!(
            "{}{}",
            env_prefix,
            setting_def.key.to_uppercase().replace('-', "_")
        );
        let Ok(value) = std::env::var(&env_key) else {
            continue;
        };

        let is_secret = setting_def.setting_type == "secret";
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
                debug!(
                    "Provisioned {}setting from env: {} -> {}",
                    if is_secret { "secret " } else { "" },
                    env_key,
                    setting_def.key
                );
            }
            Err(e) => {
                warn!(
                    "Failed to provision setting {} for plugin {}: {}",
                    setting_def.key, plugin.name, e
                );
            }
        }
    }

    if settings_count > 0 {
        tracing::info!(
            "Provisioned {} settings from environment for plugin: {}",
            settings_count,
            plugin.name
        );
    }
}
