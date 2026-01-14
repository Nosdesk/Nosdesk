//! Plugin Handlers
//!
//! Admin endpoints for managing plugins, settings, storage, and activity.

use actix_multipart::Multipart;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use chrono::Utc;
use diesel::result::Error as DieselError;
use futures::StreamExt;
use hex;
use ring::digest::{Context, SHA256};
use serde::Deserialize;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{error, info, warn};
use uuid::Uuid;
use zip;

use crate::db::{DbConnection, Pool};
use crate::models::{
    Claims, InstallPluginRequest, NewPlugin, PluginActivityResponse, PluginBundleUpdate,
    PluginResponse, PluginSettingResponse, PluginStorageResponse, PluginUpdate,
    SetPluginDataRequest, UpdatePluginRequest,
};
use crate::repository::plugins as plugin_repo;
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

/// Get a database connection or return an error response
fn get_connection(pool: &web::Data<Pool>) -> Result<DbConnection, HttpResponse> {
    pool.get().map_err(|e| {
        error!("Database connection error: {}", e);
        HttpResponse::InternalServerError().json("Database connection error")
    })
}

/// Validate plugin name
fn validate_plugin_name(name: &str) -> Result<String, HttpResponse> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(HttpResponse::BadRequest().json("Plugin name is required"));
    }
    if trimmed.len() > 100 {
        return Err(HttpResponse::BadRequest().json("Plugin name must be 100 characters or less"));
    }
    // Plugin names should be lowercase with dashes
    if !trimmed
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(HttpResponse::BadRequest()
            .json("Plugin name must be lowercase with dashes only (e.g., 'my-plugin')"));
    }
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
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

    let trust_level = body
        .trust_level
        .clone()
        .unwrap_or_else(|| "community".to_string());

    let new_plugin = NewPlugin {
        name,
        display_name: body.manifest.display_name.clone(),
        version: body.manifest.version.clone(),
        description: body.manifest.description.clone(),
        manifest: manifest_json,
        enabled: true,
        trust_level,
        installed_by,
        source: "uploaded".to_string(),
    };

    match plugin_repo::create_plugin(&mut conn, new_plugin) {
        Ok(plugin) => {
            info!(
                "Plugin installed: {} ({}) by {:?}",
                plugin.uuid, plugin.name, installed_by
            );

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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Get existing plugin
    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let mut update = PluginUpdate::default();

    if let Some(enabled) = body.enabled {
        update.enabled = Some(enabled);
    }

    if let Some(ref manifest) = body.manifest {
        update.display_name = Some(manifest.display_name.clone());
        update.version = Some(manifest.version.clone());
        update.description = manifest.description.clone();
        if let Ok(v) = serde_json::to_value(manifest) {
            update.manifest = Some(v);
        }
    }

    match plugin_repo::update_plugin_by_uuid(&mut conn, plugin_uuid, update) {
        Ok(updated) => {
            info!("Plugin updated: {} ({})", updated.uuid, updated.name);

            // Log the update activity
            let _ = plugin_repo::log_plugin_activity(
                &mut conn,
                plugin.id,
                "updated".to_string(),
                Some(serde_json::json!({
                    "enabled": body.enabled,
                    "version_updated": body.manifest.is_some(),
                })),
                user_uuid,
            );

            match PluginResponse::try_from(updated) {
                Ok(response) => HttpResponse::Ok().json(response),
                Err(e) => {
                    error!("Failed to serialize plugin response: {}", e);
                    HttpResponse::InternalServerError().json("Plugin updated but response failed")
                }
            }
        }
        Err(DieselError::NotFound) => HttpResponse::NotFound().json("Plugin not found"),
        Err(e) => {
            error!("Failed to update plugin: {}", e);
            HttpResponse::InternalServerError().json("Failed to update plugin")
        }
    }
}

/// Uninstall a plugin (admin only)
pub async fn uninstall_plugin(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let plugin_uuid = path.into_inner();

    let mut conn = match get_connection(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    match plugin_repo::delete_plugin_by_uuid(&mut conn, plugin_uuid) {
        Ok(count) if count > 0 => {
            info!("Plugin uninstalled: {}", plugin_uuid);
            HttpResponse::NoContent().finish()
        }
        Ok(_) => HttpResponse::NotFound().json("Plugin not found"),
        Err(e) => {
            error!("Failed to uninstall plugin: {}", e);
            HttpResponse::InternalServerError().json("Failed to uninstall plugin")
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
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

    let mut conn = match get_connection(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Check if plugin is enabled
    if !plugin.enabled {
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

/// Maximum bundle size (500 KB)
const MAX_BUNDLE_SIZE: usize = 500 * 1024;

/// Upload a plugin bundle (admin only)
pub async fn upload_plugin_bundle(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
    mut payload: Multipart,
) -> impl Responder {
    if let Err(e) = require_admin(&req) {
        return e;
    }

    let plugin_uuid = path.into_inner();

    let mut conn = match get_connection(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Verify plugin exists
    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    // Read the bundle from multipart
    let mut bundle_data: Vec<u8> = Vec::new();

    while let Some(item) = payload.next().await {
        let mut field = match item {
            Ok(f) => f,
            Err(e) => {
                error!("Multipart error: {}", e);
                return HttpResponse::BadRequest().json("Invalid multipart data");
            }
        };

        // Only accept "file" field
        if field.name() != "file" {
            continue;
        }

        // Check content type
        let content_type = field.content_type().map(|m| m.to_string());
        if !matches!(
            content_type.as_deref(),
            Some("application/javascript") | Some("text/javascript") | Some("application/octet-stream")
        ) {
            warn!("Invalid content type for plugin bundle: {:?}", content_type);
            // Still accept it, just log the warning
        }

        // Read field data
        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(d) => d,
                Err(e) => {
                    error!("Error reading multipart chunk: {}", e);
                    return HttpResponse::BadRequest().json("Error reading file data");
                }
            };

            if bundle_data.len() + data.len() > MAX_BUNDLE_SIZE {
                return HttpResponse::BadRequest().json(format!(
                    "Bundle too large. Maximum size is {} KB",
                    MAX_BUNDLE_SIZE / 1024
                ));
            }

            bundle_data.extend_from_slice(&data);
        }
    }

    if bundle_data.is_empty() {
        return HttpResponse::BadRequest().json("No file data received");
    }

    // Basic JavaScript validation - check for export
    let content = String::from_utf8_lossy(&bundle_data);
    if !content.contains("export") {
        return HttpResponse::BadRequest()
            .json("Invalid bundle: must be an ES module with exports");
    }

    // Calculate hash
    let mut context = Context::new(&SHA256);
    context.update(&bundle_data);
    let digest = context.finish();
    let hash = hex::encode(digest.as_ref());

    // Create directory and write file
    let bundle_path = get_bundle_path(plugin_uuid);
    if let Some(parent) = bundle_path.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            error!("Failed to create plugin directory: {}", e);
            return HttpResponse::InternalServerError().json("Failed to store bundle");
        }
    }

    let mut file = match fs::File::create(&bundle_path).await {
        Ok(f) => f,
        Err(e) => {
            error!("Failed to create bundle file: {}", e);
            return HttpResponse::InternalServerError().json("Failed to store bundle");
        }
    };

    if let Err(e) = file.write_all(&bundle_data).await {
        error!("Failed to write bundle file: {}", e);
        return HttpResponse::InternalServerError().json("Failed to store bundle");
    }

    // Update plugin record
    let bundle_update = PluginBundleUpdate {
        bundle_hash: Some(hash.clone()),
        bundle_size: Some(bundle_data.len() as i32),
        bundle_uploaded_at: Some(Utc::now().naive_utc()),
    };

    match plugin_repo::update_plugin_bundle(&mut conn, plugin_uuid, bundle_update) {
        Ok(_) => {
            info!(
                "Plugin bundle uploaded: {} ({} bytes, hash: {})",
                plugin.name,
                bundle_data.len(),
                &hash[..8]
            );
            HttpResponse::Ok().json(serde_json::json!({
                "message": "Bundle uploaded successfully",
                "size": bundle_data.len(),
                "hash": hash
            }))
        }
        Err(e) => {
            error!("Failed to update plugin bundle record: {}", e);
            HttpResponse::InternalServerError().json("Failed to update plugin record")
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

    let mut conn = match get_connection(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Verify plugin exists and is enabled
    let plugin = match get_plugin_or_error(&mut conn, plugin_uuid) {
        Ok(p) => p,
        Err(e) => return e,
    };

    if !plugin.enabled {
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

/// Maximum zip file size (2MB)
const MAX_ZIP_SIZE: usize = 2 * 1024 * 1024;

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

    let mut conn = match get_connection(&pool) {
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

            if zip_data.len() + data.len() > MAX_ZIP_SIZE {
                return HttpResponse::BadRequest().json(format!(
                    "Zip file too large. Maximum size is {} MB",
                    MAX_ZIP_SIZE / (1024 * 1024)
                ));
            }

            zip_data.extend_from_slice(&data);
        }
    }

    if zip_data.is_empty() {
        return HttpResponse::BadRequest().json("No zip file received");
    }

    // Extract the zip file
    let cursor = std::io::Cursor::new(&zip_data);
    let mut archive = match zip::ZipArchive::new(cursor) {
        Ok(a) => a,
        Err(e) => {
            error!("Failed to read zip archive: {}", e);
            return HttpResponse::BadRequest().json("Invalid zip file");
        }
    };

    // Read manifest.json
    let manifest_content = match archive.by_name("manifest.json") {
        Ok(mut file) => {
            let mut content = String::new();
            if let Err(e) = std::io::Read::read_to_string(&mut file, &mut content) {
                error!("Failed to read manifest.json: {}", e);
                return HttpResponse::BadRequest().json("Failed to read manifest.json");
            }
            content
        }
        Err(_) => {
            return HttpResponse::BadRequest().json("Zip file must contain manifest.json");
        }
    };

    // Parse manifest
    let manifest: crate::models::PluginManifest = match serde_json::from_str(&manifest_content) {
        Ok(m) => m,
        Err(e) => {
            error!("Invalid manifest.json: {}", e);
            return HttpResponse::BadRequest().json(format!("Invalid manifest.json: {}", e));
        }
    };

    // Validate plugin name
    let name = match validate_plugin_name(&manifest.name) {
        Ok(n) => n,
        Err(e) => return e,
    };

    // Check if plugin already exists
    if plugin_repo::get_plugin_by_name(&mut conn, &name).is_ok() {
        return HttpResponse::Conflict().json(format!(
            "Plugin '{}' already exists. Uninstall it first or use the update endpoint.",
            name
        ));
    }

    // Read bundle.js if present
    let bundle_data = match archive.by_name("bundle.js") {
        Ok(mut file) => {
            let mut data = Vec::new();
            if let Err(e) = std::io::Read::read_to_end(&mut file, &mut data) {
                error!("Failed to read bundle.js: {}", e);
                return HttpResponse::BadRequest().json("Failed to read bundle.js");
            }
            Some(data)
        }
        Err(_) => None,
    };

    // Create the plugin
    let manifest_json = match serde_json::to_value(&manifest) {
        Ok(v) => v,
        Err(e) => {
            error!("Failed to serialize manifest: {}", e);
            return HttpResponse::InternalServerError().json("Failed to process manifest");
        }
    };

    let new_plugin = NewPlugin {
        name,
        display_name: manifest.display_name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        manifest: manifest_json,
        enabled: true,
        trust_level: "community".to_string(), // Uploaded plugins start as community
        installed_by: Uuid::parse_str(&claims.sub).ok(),
        source: "uploaded".to_string(),
    };

    let plugin = match plugin_repo::create_plugin(&mut conn, new_plugin) {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to create plugin: {}", e);
            return HttpResponse::InternalServerError().json("Failed to create plugin");
        }
    };

    // Store bundle if present
    let has_bundle = bundle_data.is_some();
    if let Some(data) = bundle_data {
        // Validate bundle
        let content = String::from_utf8_lossy(&data);
        if !content.contains("export") {
            warn!("Bundle doesn't contain exports - may not work correctly");
        }

        // Calculate hash
        let mut context = Context::new(&SHA256);
        context.update(&data);
        let hash = hex::encode(context.finish().as_ref());

        // Store bundle
        let bundle_path = get_bundle_path(plugin.uuid);
        if let Some(parent) = bundle_path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                error!("Failed to create plugin directory: {}", e);
            }
        }

        if let Err(e) = fs::write(&bundle_path, &data).await {
            error!("Failed to write bundle: {}", e);
        } else {
            // Update bundle metadata
            let update = PluginBundleUpdate {
                bundle_hash: Some(hash),
                bundle_size: Some(data.len() as i32),
                bundle_uploaded_at: Some(Utc::now().naive_utc()),
            };
            let _ = plugin_repo::update_plugin_bundle(&mut conn, plugin.uuid, update);
        }
    }

    // Log activity
    let user_uuid = Uuid::parse_str(&claims.sub).ok();
    let _ = plugin_repo::log_plugin_activity(
        &mut conn,
        plugin.id,
        "installed".to_string(),
        Some(serde_json::json!({
            "version": manifest.version,
            "source": "zip_upload",
            "has_bundle": has_bundle,
        })),
        user_uuid,
    );

    info!(
        "Plugin installed from zip: {} v{} by {}",
        manifest.name, manifest.version, claims.sub
    );

    // Return the created plugin
    match PluginResponse::try_from(plugin) {
        Ok(response) => HttpResponse::Created().json(response),
        Err(e) => {
            error!("Failed to create plugin response: {}", e);
            HttpResponse::InternalServerError().json("Plugin created but response failed")
        }
    }
}
