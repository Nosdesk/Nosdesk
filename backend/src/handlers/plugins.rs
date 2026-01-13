//! Plugin Handlers
//!
//! Admin endpoints for managing plugins, settings, storage, and activity.

use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error as DieselError;
use serde::Deserialize;
use tracing::{error, info};
use uuid::Uuid;

use crate::db::{DbConnection, Pool};
use crate::models::{
    Claims, InstallPluginRequest, NewPlugin, PluginActivityResponse, PluginResponse,
    PluginSettingResponse, PluginStorageResponse, PluginUpdate, SetPluginDataRequest,
    UpdatePluginRequest,
};
use crate::repository::plugins as plugin_repo;
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

    match plugin_repo::set_plugin_setting(
        &mut conn,
        plugin.id,
        body.key.clone(),
        Some(body.value.clone()),
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

    // Execute the proxied request
    match proxy_service.proxy_request(&plugin.name, &manifest, body.into_inner()).await {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => {
            error!("Proxy request failed: {}", e);
            HttpResponse::BadRequest().json(e)
        }
    }
}
