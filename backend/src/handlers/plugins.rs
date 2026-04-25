//! Plugin Handlers
//!
//! Admin endpoints for managing plugins, settings, storage, and activity.

use actix_multipart::Multipart;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error as DieselError;
use futures::StreamExt;
use serde::Deserialize;
use std::path::PathBuf;
use tokio::fs;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::handlers::helpers;
use crate::models::{
    Claims, InstallPluginRequest, NewPlugin, PluginActivityResponse,
    PluginResponse, PluginSettingResponse, PluginStorageResponse, PluginUpdate,
    SetPluginDataRequest, UpdatePluginRequest,
};
use crate::repository::plugins as plugin_repo;
use crate::services::plugins::{install, registry, signing, trust};
use crate::utils::encryption;
use crate::utils::rbac::require_admin;

/// Query parameters for pagination
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

// =============================================================================
// Helper Functions
// =============================================================================


/// Map the canonical plugin-name validator (used everywhere the
/// install pipeline runs) to a 400 response for the legacy
/// JSON-install handler. The rules themselves live in
/// `services::plugins::manifest_validate::validate_name` so the
/// JSON path can't drift from what the signed-zip / registry /
/// CLI paths accept.
fn validate_plugin_name(name: &str) -> Result<String, HttpResponse> {
    let trimmed = name.trim();
    crate::services::plugins::manifest_validate::validate_name(trimmed)
        .map_err(|e| HttpResponse::BadRequest().json(e.to_string()))?;
    Ok(trimmed.to_string())
}

/// Get a plugin by UUID or return a 404/500 error response
fn get_plugin_or_error(
    conn: &mut DbConnection,
    plugin_uuid: Uuid,
) -> Result<crate::models::Plugin, HttpResponse> {
    match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
        Ok(p) => Ok(p),
        Err(DieselError::NotFound) => Err(HttpResponse::NotFound().json("Plugin not found")),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            Err(HttpResponse::InternalServerError().json("Failed to get plugin"))
        }
    }
}

// =============================================================================
// Plugin CRUD Handlers
// =============================================================================

/// List all plugins (admin only)
pub async fn list_plugins(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match plugin_repo::list_all_plugins(&mut conn) {
        Ok(plugins) => {
            let response: Vec<_> = plugins
                .into_iter()
                .filter_map(|p| PluginResponse::try_from(p).ok())
                .collect();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Failed to list plugins: {}", e);
            HttpResponse::InternalServerError().json("Failed to list plugins")
        }
    }
}

/// List enabled plugins (for frontend plugin loader - authenticated users)
pub async fn list_enabled_plugins(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    // Any authenticated user can get enabled plugins
    if req.extensions().get::<Claims>().is_none() {
        return HttpResponse::Unauthorized().json("Authentication required");
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match plugin_repo::list_enabled_plugins(&mut conn) {
        Ok(plugins) => {
            let response: Vec<_> = plugins
                .into_iter()
                .filter_map(|p| PluginResponse::try_from(p).ok())
                .collect();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Failed to list enabled plugins: {}", e);
            HttpResponse::InternalServerError().json("Failed to list plugins")
        }
    }
}

/// Install a new plugin (admin only)
pub async fn install_plugin(
    req: HttpRequest,
    pool: web::Data<Pool>,
    body: web::Json<InstallPluginRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let installed_by = Uuid::parse_str(&claims.sub).ok();

    // Validate plugin name
    let name = match validate_plugin_name(&body.manifest.name) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Check if plugin with same name already exists
    if plugin_repo::get_plugin_by_name(&mut conn, &name).is_ok() {
        return HttpResponse::Conflict().json("Plugin with this name already exists");
    }

    let manifest_json = match serde_json::to_value(&body.manifest) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to serialize manifest: {}", e);
            return HttpResponse::BadRequest().json("Invalid manifest format");
        }
    };

    // Manifest-only installs cannot establish a trust chain: there's
    // no bundle to verify. Ignore any client-supplied `trust_level`
    // (it would let any admin mint an "official" label) and pin these
    // rows to `community` with no signer metadata. New plugins with
    // bundles must go through the signed zip upload or registry path.
    if body.trust_level.is_some() {
        warn!(
            "Ignoring client-supplied trust_level on manifest-only install of plugin '{}'",
            name
        );
    }
    let trust_level = "community".to_string();

    let new_plugin = NewPlugin {
        name,
        display_name: body.manifest.display_name.clone(),
        version: body.manifest.version.clone(),
        description: body.manifest.description.clone(),
        manifest: manifest_json,
        state: crate::models::PluginState::Installed,
        trust_level,
        installed_by,
        source: "uploaded".to_string(),
        signer_pubkey: None,
        signer_source: None,
        signature_metadata: None,
        icon_svg: None,
    };

    match plugin_repo::create_plugin(&mut conn, new_plugin) {
        Ok(plugin) => {
            info!(
                "Plugin installed: {} ({}) by {:?}",
                plugin.uuid, plugin.name, installed_by
            );

            // Sync collection schemas from manifest
            if !body.manifest.collections.is_empty() {
                if let Err(e) = crate::services::plugins::validation::sync_collection_schemas(
                    &mut conn,
                    plugin.id,
                    &body.manifest,
                ) {
                    warn!("Failed to sync collection schemas for plugin {}: {}", plugin.name, e);
                }
            }

            // Log the installation activity
            let _ = plugin_repo::log_plugin_activity(
                &mut conn,
                plugin.id,
                "installed".to_string(),
                Some(serde_json::json!({
                    "version": plugin.version,
                    "installed_by": installed_by,
                })),
                installed_by,
            );

            match PluginResponse::try_from(plugin) {
                Ok(response) => HttpResponse::Created().json(response),
                Err(e) => {
                    error!("Failed to serialize plugin response: {}", e);
                    HttpResponse::InternalServerError().json("Plugin installed but response failed")
                }
            }
        }
        Err(e) => {
            error!("Failed to install plugin: {}", e);
            HttpResponse::InternalServerError().json("Failed to install plugin")
        }
    }
}

/// Get a single plugin by UUID (admin only)
pub async fn get_plugin(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let plugin_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match PluginResponse::try_from(plugin) {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => {
            error!("Failed to parse plugin manifest: {}", e);
            HttpResponse::InternalServerError().json("Invalid plugin manifest")
        }
    }
}

/// Update a plugin (admin only)
pub async fn update_plugin(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: web::Json<UpdatePluginRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let user_uuid = Uuid::parse_str(&claims.sub).ok();
    let plugin_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get existing plugin
    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Enable/disable goes through `lifecycle::apply` so the state
    // transition + activity log are atomic, the (state, action)
    // pair is exhaustively legality-checked, and a quarantined
    // plugin can't be silently un-quarantined via this endpoint.
    if let Some(enabled) = body.enabled {
        let action = if enabled {
            crate::services::plugins::lifecycle::PluginAction::Enable
        } else {
            crate::services::plugins::lifecycle::PluginAction::Disable
        };
        match crate::services::plugins::lifecycle::apply(
            &mut conn,
            plugin_uuid,
            action,
            user_uuid,
        ) {
            Ok(_) => {}
            Err(crate::services::plugins::lifecycle::ActionError::NoSuchPlugin) => {
                return HttpResponse::NotFound().json("Plugin not found");
            }
            Err(crate::services::plugins::lifecycle::ActionError::InvalidTransition {
                from,
                action,
            }) => {
                return HttpResponse::Conflict().json(format!(
                    "Cannot {action} a plugin in state {from}"
                ));
            }
            Err(e) => {
                error!("Failed to toggle plugin state: {}", e);
                return HttpResponse::InternalServerError().json("Failed to toggle plugin");
            }
        }
    }

    // Manifest update (separate from state toggle). Skipped if
    // body.manifest is absent, in which case we just return the
    // current row reflecting whatever lifecycle changes happened
    // above.
    let updated_plugin = if let Some(ref manifest) = body.manifest {
        let mut update = PluginUpdate::default();
        update.display_name = Some(manifest.display_name.clone());
        update.version = Some(manifest.version.clone());
        update.description = manifest.description.clone();
        if let Ok(v) = serde_json::to_value(manifest) {
            update.manifest = Some(v);
        }

        match plugin_repo::update_plugin_by_uuid(&mut conn, plugin_uuid, update) {
            Ok(updated) => {
                info!("Plugin manifest updated: {} ({})", updated.uuid, updated.name);
                if let Err(e) = crate::services::plugins::validation::sync_collection_schemas(
                    &mut conn,
                    plugin.id,
                    manifest,
                ) {
                    warn!(
                        "Failed to sync collection schemas for plugin {}: {}",
                        updated.name, e
                    );
                }
                let _ = plugin_repo::log_plugin_activity(
                    &mut conn,
                    plugin.id,
                    "manifest_updated".to_string(),
                    Some(serde_json::json!({ "version": manifest.version })),
                    user_uuid,
                );
                updated
            }
            Err(DieselError::NotFound) => {
                return HttpResponse::NotFound().json("Plugin not found");
            }
            Err(e) => {
                error!("Failed to update plugin manifest: {}", e);
                return HttpResponse::InternalServerError().json("Failed to update plugin");
            }
        }
    } else {
        // No manifest update; re-fetch to capture any state change
        // from the lifecycle dispatch above.
        match plugin_repo::get_plugin_by_uuid(&mut conn, plugin_uuid) {
            Ok(p) => p,
            Err(DieselError::NotFound) => {
                return HttpResponse::NotFound().json("Plugin not found");
            }
            Err(e) => {
                error!("Failed to re-fetch plugin: {}", e);
                return HttpResponse::InternalServerError().json("Failed to load plugin");
            }
        }
    };

    match PluginResponse::try_from(updated_plugin) {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => {
            error!("Failed to serialize plugin response: {}", e);
            HttpResponse::InternalServerError().json("Plugin updated but response failed")
        }
    }
}

/// Uninstall a plugin (admin only).
///
/// Behaviour depends on the manifest's
/// `lifecycle.on_uninstall`:
///
///   - `cascade` (default): delete the `plugins` row. Postgres
///     ON DELETE CASCADE clears every dependent row
///     (plugin_data, plugin_collection_rows, plugin_activity).
///   - `preserve`: flip `state` to `Uninstalled`, remove the
///     staged bundle file, but keep the row + plugin_data so a
///     future reinstall of the same plugin name reattaches the
///     data automatically.
///
/// Both paths dispatch through `lifecycle::apply`, which writes
/// the state change and the activity log inside one transaction
/// and rejects illegal transitions exhaustively.
pub async fn uninstall_plugin(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let claims = match require_admin(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let actor = Uuid::parse_str(&claims.sub).ok();

    let plugin_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    // Read the policy from the stored manifest. If parsing fails
    // (corrupt row, manifest schema drift), fall back to Preserve:
    // the destructive default would silently delete user data
    // tied to a row we can't introspect, which is a one-way street
    // an admin can't undo. Preserve keeps the row and lets the
    // admin inspect, fix, or hard-delete via cascade explicitly.
    let policy = plugin
        .parse_manifest()
        .ok()
        .map(|m| m.lifecycle.on_uninstall)
        .unwrap_or(crate::models::PluginUninstallPolicy::Preserve);

    let action = match policy {
        crate::models::PluginUninstallPolicy::Cascade => {
            crate::services::plugins::lifecycle::PluginAction::UninstallCascade
        }
        crate::models::PluginUninstallPolicy::Preserve => {
            crate::services::plugins::lifecycle::PluginAction::UninstallPreserve
        }
    };

    match crate::services::plugins::lifecycle::apply(&mut conn, plugin_uuid, action, actor) {
        Ok(outcome) => {
            if crate::services::plugins::lifecycle::outcome_removes_bundle(&outcome) {
                remove_bundle_file_or_warn(plugin_uuid).await;
            }
            match outcome {
                crate::services::plugins::lifecycle::ActionOutcome::Deleted { .. } => {
                    info!("Plugin uninstalled (cascade): {}", plugin_uuid);
                }
                crate::services::plugins::lifecycle::ActionOutcome::StateChanged { .. } => {
                    info!(
                        "Plugin uninstalled (preserve), data retained: {}",
                        plugin_uuid
                    );
                }
            }
            HttpResponse::NoContent().finish()
        }
        Err(crate::services::plugins::lifecycle::ActionError::NoSuchPlugin) => {
            HttpResponse::NotFound().json("Plugin not found")
        }
        Err(crate::services::plugins::lifecycle::ActionError::InvalidTransition {
            from,
            action,
        }) => HttpResponse::Conflict().json(format!(
            "Cannot {action} a plugin in state {from}"
        )),
        Err(e) => {
            error!("Failed to uninstall plugin: {}", e);
            HttpResponse::InternalServerError().json("Failed to uninstall plugin")
        }
    }
}

/// Remove the staged bundle from the uploads volume, swallowing
/// `NotFound` (no bundle was ever staged for this plugin) and
/// logging anything else as a warn — failure to clean up the
/// bundle isn't worth failing the whole uninstall over, but it
/// IS worth knowing about.
async fn remove_bundle_file_or_warn(plugin_uuid: Uuid) {
    let bundle_path = get_bundle_path(plugin_uuid);
    if let Err(e) = fs::remove_file(&bundle_path).await {
        if e.kind() != std::io::ErrorKind::NotFound {
            warn!(
                error = %e,
                path = %bundle_path.display(),
                "Failed to remove plugin bundle on uninstall"
            );
        }
    }
}

// =============================================================================
// Plugin Settings Handlers
// =============================================================================

/// Get all settings for a plugin (admin only)
pub async fn get_plugin_settings(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let plugin_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match plugin_repo::get_plugin_settings(&mut conn, plugin.id) {
        Ok(settings) => {
            let response: Vec<PluginSettingResponse> =
                settings.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Failed to get plugin settings: {}", e);
            HttpResponse::InternalServerError().json("Failed to get settings")
        }
    }
}

/// Set a plugin setting (admin only)
pub async fn set_plugin_setting(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: web::Json<SetPluginDataRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let plugin_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Check if this is a secret setting from the manifest
    let is_secret = plugin
        .parse_manifest()
        .ok()
        .and_then(|m| {
            m.settings
                .iter()
                .find(|s| s.key == body.key)
                .map(|s| s.setting_type == "secret")
        })
        .unwrap_or(false);

    // Encrypt secret values before storing
    let value_to_store = if is_secret {
        match body.value.as_str() {
            Some(plaintext) => {
                match encryption::encrypt(plaintext) {
                    Ok(encrypted) => serde_json::Value::String(encrypted),
                    Err(e) => {
                        error!("Failed to encrypt plugin secret: {}", e);
                        return HttpResponse::InternalServerError()
                            .json("Failed to encrypt secret. Ensure ENCRYPTION_KEY is configured.");
                    }
                }
            }
            None => {
                return HttpResponse::BadRequest()
                    .json("Secret settings must be string values");
            }
        }
    } else {
        body.value.clone()
    };

    match plugin_repo::set_plugin_setting(
        &mut conn,
        plugin.id,
        body.key.clone(),
        Some(value_to_store),
        is_secret,
    ) {
        Ok(setting) => {
            info!("Plugin setting updated: {} / {}", plugin.name, body.key);
            HttpResponse::Ok().json(PluginSettingResponse::from(setting))
        }
        Err(e) => {
            error!("Failed to set plugin setting: {}", e);
            HttpResponse::InternalServerError().json("Failed to set setting")
        }
    }
}

/// Delete a plugin setting (admin only)
pub async fn delete_plugin_setting(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(Uuid, String)>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let (plugin_uuid, key) = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match plugin_repo::delete_plugin_setting(&mut conn, plugin.id, &key) {
        Ok(count) if count > 0 => HttpResponse::NoContent().finish(),
        Ok(_) => HttpResponse::NotFound().json("Setting not found"),
        Err(e) => {
            error!("Failed to delete plugin setting: {}", e);
            HttpResponse::InternalServerError().json("Failed to delete setting")
        }
    }
}

// =============================================================================
// Plugin Storage Handlers (for plugin runtime use)
// =============================================================================

/// Get storage value for a plugin (authenticated users - for plugin use)
pub async fn get_plugin_storage(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(Uuid, String)>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return HttpResponse::Unauthorized().json("Authentication required");
    }

    let (plugin_uuid, key) = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match plugin_repo::get_plugin_storage_entry(&mut conn, plugin.id, &key) {
        Ok(entry) => HttpResponse::Ok().json(PluginStorageResponse::from(entry)),
        Err(DieselError::NotFound) => HttpResponse::Ok().json(serde_json::json!({
            "key": key,
            "value": null
        })),
        Err(e) => {
            error!("Failed to get plugin storage: {}", e);
            HttpResponse::InternalServerError().json("Failed to get storage")
        }
    }
}

/// Set storage value for a plugin (authenticated users - for plugin use)
pub async fn set_plugin_storage(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    body: web::Json<SetPluginDataRequest>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return HttpResponse::Unauthorized().json("Authentication required");
    }

    let plugin_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match plugin_repo::set_plugin_storage(
        &mut conn,
        plugin.id,
        body.key.clone(),
        Some(body.value.clone()),
    ) {
        Ok(entry) => HttpResponse::Ok().json(PluginStorageResponse::from(entry)),
        Err(e) => {
            error!("Failed to set plugin storage: {}", e);
            HttpResponse::InternalServerError().json("Failed to set storage")
        }
    }
}

/// Delete storage value for a plugin (authenticated users - for plugin use)
pub async fn delete_plugin_storage(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<(Uuid, String)>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return HttpResponse::Unauthorized().json("Authentication required");
    }

    let (plugin_uuid, key) = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match plugin_repo::delete_plugin_storage_entry(&mut conn, plugin.id, &key) {
        Ok(_) => HttpResponse::NoContent().finish(),
        Err(e) => {
            error!("Failed to delete plugin storage: {}", e);
            HttpResponse::InternalServerError().json("Failed to delete storage")
        }
    }
}

// =============================================================================
// Plugin Activity Handlers
// =============================================================================

/// Get activity log for a plugin (admin only)
pub async fn get_plugin_activity(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    query: web::Query<PaginationQuery>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let plugin_uuid = path.into_inner();
    let limit = query.limit.unwrap_or(50).min(100);
    let offset = query.offset.unwrap_or(0);

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    match plugin_repo::get_plugin_activity(&mut conn, plugin.id, limit, offset) {
        Ok(activity) => {
            let response: Vec<PluginActivityResponse> =
                activity.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Failed to get plugin activity: {}", e);
            HttpResponse::InternalServerError().json("Failed to get activity")
        }
    }
}

// =============================================================================
// Plugin Proxy Handler
// =============================================================================

/// Proxy an external request for a plugin (authenticated users)
pub async fn proxy_plugin_request(
    req: HttpRequest,
    pool: web::Data<Pool>,
    proxy_service: web::Data<crate::services::plugins::PluginProxyService>,
    path: web::Path<Uuid>,
    body: web::Json<crate::models::PluginProxyRequest>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return HttpResponse::Unauthorized().json("Authentication required");
    }

    let plugin_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Check if plugin is enabled
    if !plugin.is_active() {
        return HttpResponse::Forbidden().json("Plugin is disabled");
    }

    // Parse the manifest
    let manifest = match plugin.parse_manifest() {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to parse plugin manifest: {}", e);
            return HttpResponse::InternalServerError().json("Invalid plugin manifest");
        }
    };

    // Fetch plugin settings for auth injection
    let settings = match crate::repository::plugins::get_plugin_settings(&mut conn, plugin.id) {
        Ok(s) => s,
        Err(e) => {
            error!("Failed to get plugin settings: {}", e);
            vec![]
        }
    };

    // Build secrets map for auth injection (decrypt encrypted secrets)
    let mut secrets = std::collections::HashMap::new();
    for setting in settings {
        if setting.is_secret {
            if let Some(value) = setting.value {
                if let Some(encrypted) = value.as_str() {
                    match encryption::decrypt(encrypted) {
                        Ok(decrypted) => {
                            secrets.insert(setting.key, decrypted);
                        }
                        Err(e) => {
                            error!(
                                "Failed to decrypt secret '{}' for plugin '{}': {}",
                                setting.key, plugin.name, e
                            );
                            // Fail closed - don't use potentially compromised data
                        }
                    }
                }
            }
        }
    }

    // Execute the proxied request with secrets for auth injection
    match proxy_service.proxy_request(&plugin.name, &manifest, body.into_inner(), &secrets).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => {
            error!("Proxy request failed: {}", e);
            HttpResponse::BadRequest().json(e)
        }
    }
}

// =============================================================================
// Plugin Bundle Handlers
// =============================================================================

/// Get the bundle storage path for a plugin
fn get_bundle_path(plugin_uuid: Uuid) -> PathBuf {
    PathBuf::from("/app/uploads/plugins")
        .join(plugin_uuid.to_string())
        .join("bundle.js")
}

/// Serve a plugin's `icon.svg` bytes. No auth required — icons are
/// shown in plugin lists that any logged-in user might see, and
/// they carry no secrets. Cache freely; the URL doesn't change
/// when the icon does, but the contents do, so we send a weak
/// `ETag` derived from the plugin's `updated_at` via the route's
/// `Last-Modified` semantics. For simplicity we just cache for 5
/// minutes and let the next install bust it via row update.
pub async fn serve_plugin_icon(
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    let plugin_uuid = path.into_inner();
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match plugin_repo::get_plugin_icon(&mut conn, plugin_uuid) {
        Ok(Some(bytes)) => HttpResponse::Ok()
            .content_type("image/svg+xml")
            .insert_header(("Cache-Control", "public, max-age=300"))
            .body(bytes),
        Ok(None) => HttpResponse::NotFound().finish(),
        Err(DieselError::NotFound) => HttpResponse::NotFound().finish(),
        Err(e) => {
            error!("Failed to load plugin icon: {}", e);
            HttpResponse::InternalServerError().finish()
        }
    }
}

/// Serve a plugin bundle (authenticated users)
pub async fn serve_plugin_bundle(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    // Any authenticated user can request plugin bundles
    if req.extensions().get::<Claims>().is_none() {
        return HttpResponse::Unauthorized().json("Authentication required");
    }

    let plugin_uuid = path.into_inner();

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Verify plugin exists and is enabled
    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    if !plugin.is_active() {
        return HttpResponse::Forbidden().json("Plugin is disabled");
    }

    // Check if bundle has been uploaded
    if plugin.bundle_uploaded_at.is_none() {
        return HttpResponse::NotFound().json("Plugin bundle not found");
    }

    // Read and serve the bundle
    let bundle_path = get_bundle_path(plugin_uuid);

    match fs::read(&bundle_path).await {
        Ok(data) => HttpResponse::Ok()
            .content_type("application/javascript")
            .insert_header(("Cache-Control", "private, max-age=3600"))
            .insert_header((
                "ETag",
                plugin.bundle_hash.as_deref().unwrap_or("unknown"),
            ))
            .body(data),
        Err(e) => {
            error!("Failed to read plugin bundle: {}", e);
            HttpResponse::NotFound().json("Plugin bundle not found")
        }
    }
}

// =============================================================================
// Plugin Zip Upload Handler
// =============================================================================

// Outer zip-file size ceiling is shared with the provisioner via
// `signing::MAX_ARCHIVE_SIZE`, so both install entry points enforce
// the same ceiling.

/// Install a plugin from a zip file (admin only)
///
/// The zip file should contain:
/// - manifest.json (required)
/// - bundle.js (optional)
pub async fn install_plugin_from_zip(
    req: HttpRequest,
    pool: web::Data<Pool>,
    mut payload: Multipart,
) -> impl Responder {
    // Check admin permission
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>().cloned() {
        Some(c) => c,
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Read the zip file from multipart
    let mut zip_data = Vec::new();

    while let Some(field) = payload.next().await {
        let mut field = match field {
            Ok(f) => f,
            Err(e) => {
                error!("Multipart field error: {}", e);
                return HttpResponse::BadRequest().json("Invalid multipart data");
            }
        };

        // Check content type
        let content_type = field.content_type().map(|m| m.to_string()).unwrap_or_default();
        if !content_type.contains("zip") && !content_type.contains("octet-stream") {
            continue;
        }

        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to read multipart chunk: {}", e);
                    return HttpResponse::BadRequest().json("Failed to read upload");
                }
            };

            if zip_data.len() + data.len() > signing::MAX_ARCHIVE_SIZE {
                return HttpResponse::BadRequest().json(format!(
                    "Zip file too large. Maximum size is {} MB",
                    signing::MAX_ARCHIVE_SIZE / (1024 * 1024)
                ));
            }

            zip_data.extend_from_slice(&data);
        }
    }

    if zip_data.is_empty() {
        return HttpResponse::BadRequest().json("No zip file received");
    }

    // Verify plugin signature. Web uploads must resolve to a public-
    // chain signer (verified or community publisher, or the Nosdesk
    // root key). Unsigned uploads and `local`-tier signatures are
    // refused here: the local tier is CLI-only by design, so shelling
    // onto the host remains the one path for admin-minted plugins.
    let verified = match signing::verify_archive(&zip_data) {
        Ok(v) => v,
        Err(signing::SigningError::MissingSignature) => {
            return HttpResponse::BadRequest().json(
                "This plugin isn't signed. Unsigned plugins must be installed via the nosdesk-plugin CLI.",
            );
        }
        Err(e) => {
            warn!("Plugin zip signature rejected: {}", e);
            return HttpResponse::BadRequest().json(format!("Plugin signature rejected: {e}"));
        }
    };

    let resolved_tier = match trust::resolve(&mut conn, &verified.envelope) {
        Ok(t) => t,
        Err(e) => {
            warn!("Plugin publisher not trusted: {}", e);
            return HttpResponse::BadRequest().json(format!("Plugin publisher not trusted: {e}"));
        }
    };

    if matches!(resolved_tier, trust::ResolvedTier::Local) {
        return HttpResponse::BadRequest().json(
            "Locally-signed plugins must be installed via the nosdesk-plugin CLI, not the admin upload form.",
        );
    }

    let signer = trust::PluginSignerFields::from_verified(&verified, &resolved_tier);
    let options = install::InstallOptions {
        source: "uploaded",
        installed_by: Uuid::parse_str(&claims.sub).ok(),
        log_activity: true,
        provision_settings: false,
        skip_if_unchanged: false,
    };

    let outcome = match install::install_verified(
        &mut conn,
        &verified.files,
        signer,
        resolved_tier,
        options,
    ) {
        Ok(o) => o,
        Err(e) => return install_error_to_response(e),
    };

    let was_create = matches!(outcome, install::InstallOutcome::Created(_));
    let plugin = match outcome {
        install::InstallOutcome::Created(p) | install::InstallOutcome::Updated(p) => p,
        install::InstallOutcome::Unchanged(p) => p,
    };

    info!(
        "Plugin installed from zip: {} v{} by {}",
        plugin.name, plugin.version, claims.sub
    );

    match PluginResponse::try_from(plugin) {
        Ok(response) => {
            if was_create {
                HttpResponse::Created().json(response)
            } else {
                HttpResponse::Ok().json(response)
            }
        }
        Err(e) => {
            error!("Failed to create plugin response: {}", e);
            HttpResponse::InternalServerError().json("Plugin created but response failed")
        }
    }
}

fn install_error_to_response(err: install::InstallError) -> HttpResponse {
    match err {
        install::InstallError::MissingManifest
        | install::InstallError::InvalidManifest(_)
        | install::InstallError::BundleTooLarge(_)
        | install::InstallError::InvalidIcon(_)
        | install::InstallError::InvalidManifestSchema(_) => {
            HttpResponse::BadRequest().json(err.to_string())
        }
        install::InstallError::ReinstallSignerMismatch { .. } => {
            // Conflict, not BadRequest: the request is structurally
            // fine, but it conflicts with the existing row's
            // ownership claim. Admin needs to hard-uninstall first.
            HttpResponse::Conflict().json(err.to_string())
        }
        install::InstallError::BundleWriteFailed(_) | install::InstallError::Db(_) => {
            error!("Plugin install failed: {}", err);
            HttpResponse::InternalServerError().json(err.to_string())
        }
    }
}

// =============================================================================
// Registry handlers
// =============================================================================

/// Serve the cached registry snapshot to the admin UI. Returns 503
/// if the background sync has never completed successfully this
/// process (no snapshot to show).
pub async fn get_registry(
    req: HttpRequest,
    cache: web::Data<registry::SharedCache>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }
    let guard = cache.read().await;
    match &*guard {
        Some(snapshot) => HttpResponse::Ok().json(serde_json::json!({
            "fetched_at": snapshot.fetched_at,
            "publishers": snapshot.publishers,
            "index": snapshot.index,
        })),
        None => HttpResponse::ServiceUnavailable()
            .json("Registry snapshot not available yet; background sync has not completed"),
    }
}

#[derive(Deserialize)]
pub struct InstallFromRegistryRequest {
    pub plugin_name: String,
    /// Omit for the latest published version.
    pub version: Option<String>,
}

/// Resolve a plugin from the cached registry, download its zip,
/// verify sha256 + signature, and install through the shared
/// install pipeline. Admin-only.
pub async fn install_from_registry(
    req: HttpRequest,
    pool: web::Data<Pool>,
    cache: web::Data<registry::SharedCache>,
    body: web::Json<InstallFromRegistryRequest>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }
    let claims = match req.extensions().get::<Claims>().cloned() {
        Some(c) => c,
        None => return HttpResponse::Unauthorized().json("Authentication required"),
    };

    // Snapshot the registry entry we intend to install so we can
    // drop the read guard before doing any network or DB work.
    // Captured fields are also what we'll cross-check against the
    // downloaded zip's signature envelope + manifest before commit.
    let (download_url, expected_sha256, claimed_publisher_pubkey, claimed_tier) = {
        let guard = cache.read().await;
        let snapshot = match &*guard {
            Some(s) => s,
            None => {
                return HttpResponse::ServiceUnavailable().json(
                    "Registry snapshot not available yet; wait for background sync",
                );
            }
        };
        let entry = match snapshot.find_plugin(&body.plugin_name) {
            Some(e) => e,
            None => {
                return HttpResponse::NotFound()
                    .json(format!("plugin {:?} not in registry", body.plugin_name));
            }
        };
        let version = match entry.resolve_version(body.version.as_deref()) {
            Some(v) => v,
            None => {
                return HttpResponse::NotFound().json(format!(
                    "plugin {:?} has no version {:?}",
                    body.plugin_name, body.version
                ));
            }
        };
        (
            version.download_url.clone(),
            version.sha256.clone(),
            entry.publisher_pubkey.clone(),
            entry.tier.clone(),
        )
    };

    let http = match registry::build_http_client() {
        Ok(c) => c,
        Err(e) => {
            error!("HTTP client build failed: {}", e);
            return HttpResponse::InternalServerError().json("HTTP client unavailable");
        }
    };

    let bytes = match download_bundle(&http, &download_url).await {
        Ok(b) => b,
        Err(e) => return HttpResponse::BadGateway().json(format!("download failed: {e}")),
    };

    // Independent content check BEFORE the signature verifier
    // touches the bytes. The signature would catch tampering too,
    // but the registry's published sha256 is a second witness and
    // surfaces a clearer error if the CDN is serving stale files.
    let actual_sha = sha256_hex(&bytes);
    if actual_sha != expected_sha256 {
        warn!(
            plugin = %body.plugin_name,
            expected = %expected_sha256,
            actual = %actual_sha,
            "Registry SHA-256 mismatch",
        );
        return HttpResponse::BadGateway().json(
            "downloaded bundle does not match registry-published sha256",
        );
    }

    let verified = match signing::verify_archive(&bytes) {
        Ok(v) => v,
        Err(e) => {
            return HttpResponse::BadRequest().json(format!("signature rejected: {e}"));
        }
    };

    // Cross-check the zip against what the registry claimed.
    // Without these asserts, the signed registry could point at a
    // zip by a different (still-trusted) publisher and we'd install
    // it under mis-attributed provenance. The registry is signed by
    // the Nosdesk root so these fields are tamper-proof in transit;
    // the check catches CDN tampering, lingering mis-wired URLs,
    // and registry-side mistakes with a clear 400 rather than a
    // silent discrepancy.
    if verified.envelope.signer_pubkey != claimed_publisher_pubkey {
        warn!(
            plugin = %body.plugin_name,
            registry_pubkey = %claimed_publisher_pubkey,
            actual_pubkey = %verified.envelope.signer_pubkey,
            "Registry / zip publisher mismatch",
        );
        return HttpResponse::BadRequest().json(
            "zip signer does not match the publisher the registry advertised",
        );
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let tier = match trust::resolve(&mut conn, &verified.envelope) {
        Ok(t) => t,
        Err(e) => {
            return HttpResponse::BadRequest().json(format!("publisher not trusted: {e}"));
        }
    };
    if tier.trust_level() != claimed_tier {
        warn!(
            plugin = %body.plugin_name,
            registry_tier = %claimed_tier,
            resolved_tier = tier.trust_level(),
            "Registry / resolved-tier mismatch",
        );
        return HttpResponse::BadRequest().json(
            "zip's resolved trust tier does not match the tier the registry advertised",
        );
    }

    // Verify the zip's manifest.name matches the registry key
    // before install so we can't be tricked into installing a
    // different plugin under the registry-claimed identity.
    if let Some(manifest_bytes) = signing::find_entry(&verified.files, "manifest.json") {
        if let Ok(manifest_value) = serde_json::from_slice::<serde_json::Value>(manifest_bytes) {
            let zip_name = manifest_value
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if zip_name != body.plugin_name {
                warn!(
                    plugin = %body.plugin_name,
                    zip_name,
                    "Registry / manifest name mismatch",
                );
                return HttpResponse::BadRequest().json(
                    "zip manifest name does not match the plugin the registry advertised",
                );
            }
        }
    }

    let signer = trust::PluginSignerFields::from_verified(&verified, &tier);

    let options = install::InstallOptions {
        source: "registry",
        installed_by: Uuid::parse_str(&claims.sub).ok(),
        log_activity: true,
        provision_settings: false,
        skip_if_unchanged: false,
    };
    let outcome = match install::install_verified(
        &mut conn,
        &verified.files,
        signer,
        tier,
        options,
    ) {
        Ok(o) => o,
        Err(e) => return install_error_to_response(e),
    };

    let was_create = matches!(outcome, install::InstallOutcome::Created(_));
    let plugin = match outcome {
        install::InstallOutcome::Created(p)
        | install::InstallOutcome::Updated(p)
        | install::InstallOutcome::Unchanged(p) => p,
    };
    info!(
        "Plugin installed from registry: {} v{} by {}",
        plugin.name, plugin.version, claims.sub
    );

    match PluginResponse::try_from(plugin) {
        Ok(response) => {
            if was_create {
                HttpResponse::Created().json(response)
            } else {
                HttpResponse::Ok().json(response)
            }
        }
        Err(e) => {
            error!("Failed to build plugin response: {}", e);
            HttpResponse::InternalServerError().json("Plugin installed but response failed")
        }
    }
}

async fn download_bundle(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    // Enforce https:// on registry-supplied download URLs. The URL
    // is root-signed, so the attacker would need root-key access to
    // set a malicious http:// value in a signed index — but a
    // well-meaning maintainer could also slip one in by mistake
    // and trigger SSRF to the instance's internal services. Hard
    // refuse anything that isn't https.
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid download_url {url}: {e}"))?;
    if parsed.scheme() != "https" {
        return Err(format!(
            "registry download_url must be https://; got scheme {:?}",
            parsed.scheme()
        ));
    }

    let resp = http
        .get(url)
        .send()
        .await
        .map_err(|e| format!("fetch {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("fetch {url}: HTTP {}", resp.status()));
    }
    if let Some(len) = resp.content_length() {
        if len as usize > signing::MAX_ARCHIVE_SIZE {
            return Err(format!("bundle exceeds {} bytes", signing::MAX_ARCHIVE_SIZE));
        }
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read {url}: {e}"))?;
    if bytes.len() > signing::MAX_ARCHIVE_SIZE {
        return Err(format!("bundle exceeds {} bytes", signing::MAX_ARCHIVE_SIZE));
    }
    Ok(bytes.to_vec())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use ring::digest::{Context, SHA256};
    let mut ctx = Context::new(&SHA256);
    ctx.update(bytes);
    hex::encode(ctx.finish().as_ref())
}
