//! Plugin Handlers
//!
//! Admin endpoints for managing plugins, settings, storage, and activity.

use actix_multipart::Multipart;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use diesel::result::Error as DieselError;
use futures::StreamExt;
use serde::Deserialize;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::db::Pool;
use crate::extractors::TenantConn;
use crate::handlers::errors;
use crate::handlers::helpers;
use crate::middleware::RequestContext;
use crate::models::{
    Claims, PluginActivityResponse, PluginResponse, PluginSettingResponse, PluginStorageResponse,
    SetPluginDataRequest, UpdatePluginRequest, WorkspaceRole,
};
use crate::repository::plugin_publishers;
use crate::repository::plugins as plugin_repo;
use crate::services::plugins::{install, registry, signing, trust};
use crate::sync::actor::ActorContext;
use crate::sync::session as actor_session;
use crate::utils::encryption;
use crate::utils::rbac::{require_auth, require_workspace_role};

pub fn config(cfg: &mut web::ServiceConfig) {
    // Literal paths MUST be registered before the
    // `{uuid}` paths: actix matches in registration
    // order and a `web::Path<Uuid>` extractor on the
    // generic route would 400 trying to parse
    // "registry" or "install" as a UUID, never
    // falling through to the literal handlers.
    cfg.route(
        "/admin/plugins",
        web::get().to(crate::handlers::plugins::list_plugins),
    )
    .route(
        "/admin/plugins/config",
        web::get().to(crate::handlers::plugins::get_admin_config),
    )
    .route(
        "/admin/plugins/signing-overview",
        web::get().to(crate::handlers::plugins::get_signing_overview),
    )
    .route(
        "/admin/plugins/install",
        web::post().to(crate::handlers::plugins::install_plugin_from_zip),
    )
    .route(
        "/admin/plugins/registry",
        web::get().to(crate::handlers::plugins::get_registry),
    )
    .route(
        "/admin/plugins/registry/refresh",
        web::post().to(crate::handlers::plugins::refresh_registry),
    )
    .route(
        "/admin/plugins/registry/install",
        web::post().to(crate::handlers::plugins::install_from_registry),
    )
    .route(
        "/admin/plugins/{uuid}",
        web::get().to(crate::handlers::plugins::get_plugin),
    )
    .route(
        "/admin/plugins/{uuid}",
        web::put().to(crate::handlers::plugins::update_plugin),
    )
    .route(
        "/admin/plugins/{uuid}",
        web::delete().to(crate::handlers::plugins::uninstall_plugin),
    )
    .route(
        "/admin/plugins/{uuid}/consent",
        web::post().to(crate::handlers::plugins::consent_to_plugin),
    )
    .route(
        "/admin/plugins/{uuid}/settings",
        web::get().to(crate::handlers::plugins::get_plugin_settings),
    )
    .route(
        "/admin/plugins/{uuid}/settings",
        web::post().to(crate::handlers::plugins::set_plugin_setting),
    )
    .route(
        "/admin/plugins/{uuid}/settings/{key}",
        web::delete().to(crate::handlers::plugins::delete_plugin_setting),
    )
    .route(
        "/admin/plugins/{uuid}/activity",
        web::get().to(crate::handlers::plugins::get_plugin_activity),
    )
    // ===== PLUGIN API (For plugins to use) =====
    .route(
        "/plugins/enabled",
        web::get().to(crate::handlers::plugins::list_enabled_plugins),
    )
    .route(
        "/plugins/{uuid}/bundle",
        web::get().to(crate::handlers::plugins::serve_plugin_bundle),
    )
    // Mint a short-lived token the sandbox iframe uses to fetch this plugin's
    // bundle cross-origin (it can't send the session cookie). The sandbox
    // runtime + token-gated /bundle live at the top-level /__plugin-sandbox scope.
    .route(
        "/plugins/{uuid}/bundle-token",
        web::get().to(crate::handlers::plugin_sandbox::mint_bundle_token),
    )
    .route(
        "/plugins/{uuid}/icon",
        web::get().to(crate::handlers::plugins::serve_plugin_icon),
    )
    .route(
        "/plugins/{uuid}/storage/{key}",
        web::get().to(crate::handlers::plugins::get_plugin_storage),
    )
    .route(
        "/plugins/{uuid}/storage",
        web::post().to(crate::handlers::plugins::set_plugin_storage),
    )
    .route(
        "/plugins/{uuid}/storage/{key}",
        web::delete().to(crate::handlers::plugins::delete_plugin_storage),
    )
    .route(
        "/plugins/{uuid}/proxy",
        web::post().to(crate::handlers::plugins::proxy_plugin_request),
    )
    // ===== PLUGIN EVENT EMISSION =====
    // Authenticated user iframes can call this to record a
    // plugin-emitted event in sync_actions with
    // actor_kind = 'plugin'. Aggregate must be a registered
    // variant; plugins extend behaviour through event_type
    // strings, not by inventing new aggregates.
    .route(
        "/plugins/{uuid}/events",
        web::post().to(crate::handlers::plugin_events::emit_plugin_event),
    )
    // ===== PLUGIN COLLECTIONS =====
    .route(
        "/plugins/{uuid}/collections",
        web::get().to(crate::handlers::plugin_collections::list_collections),
    )
    .route(
        "/plugins/{uuid}/collections/{name}",
        web::get().to(crate::handlers::plugin_collections::get_collection_schema),
    )
    .route(
        "/plugins/{uuid}/collections/{name}/rows",
        web::get().to(crate::handlers::plugin_collections::list_collection_rows),
    )
    .route(
        "/plugins/{uuid}/collections/{name}/rows",
        web::post().to(crate::handlers::plugin_collections::create_collection_row),
    )
    .route(
        "/plugins/{uuid}/collections/{name}/rows/{row_uuid}",
        web::get().to(crate::handlers::plugin_collections::get_collection_row),
    )
    .route(
        "/plugins/{uuid}/collections/{name}/rows/{row_uuid}",
        web::put().to(crate::handlers::plugin_collections::update_collection_row),
    )
    .route(
        "/plugins/{uuid}/collections/{name}/rows/{row_uuid}",
        web::delete().to(crate::handlers::plugin_collections::delete_collection_row),
    );
}

/// Query parameters for pagination
#[derive(Debug, Deserialize)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

/// AAD for plugin secret settings. Binds the ciphertext to
/// `(plugin.uuid, setting_key)` so a row swap (between plugins or
/// between setting keys) fails the AEAD tag check. We use the plugin
/// UUID rather than the autoincrement `id` because the UUID survives
/// export/reimport and re-installs while `id` does not.
const PLUGIN_SECRET_AAD_TAG: &[u8] = b".nosdesk.plugin.setting.v1";

pub(crate) fn plugin_secret_aad(plugin_uuid: &Uuid, setting_key: &str) -> Vec<u8> {
    let k = setting_key.as_bytes();
    let mut buf = Vec::with_capacity(16 + 1 + k.len() + PLUGIN_SECRET_AAD_TAG.len());
    buf.extend_from_slice(plugin_uuid.as_bytes());
    buf.push(b':');
    buf.extend_from_slice(k);
    buf.extend_from_slice(PLUGIN_SECRET_AAD_TAG);
    buf
}

/// Encrypt a plugin secret setting and return its hex-encoded framed
/// blob, suitable for storing inside `plugin_data.value` (JSONB string).
/// We hex-encode rather than promoting the column to BYTEA because
/// `plugin_data.value` is polymorphic across plugins (booleans, ints,
/// JSON, secrets), and a sidecar BYTEA column would be NULL for every
/// non-secret row.
pub(crate) fn encrypt_plugin_secret(
    plaintext: &str,
    plugin_uuid: &Uuid,
    setting_key: &str,
) -> Result<String, encryption::CryptoError> {
    let blob = encryption::keyring().encrypt(
        plaintext.as_bytes(),
        &plugin_secret_aad(plugin_uuid, setting_key),
    )?;
    Ok(hex::encode(blob))
}

/// Inverse of `encrypt_plugin_secret`. Returns the plaintext string.
pub(crate) fn decrypt_plugin_secret(
    hex_blob: &str,
    plugin_uuid: &Uuid,
    setting_key: &str,
) -> anyhow::Result<String> {
    let blob = hex::decode(hex_blob)
        .map_err(|e| anyhow::anyhow!("plugin secret is not valid hex: {e}"))?;
    let plaintext = encryption::keyring()
        .decrypt(&blob, &plugin_secret_aad(plugin_uuid, setting_key))
        .map_err(|e| anyhow::anyhow!("plugin secret decrypt failed: {e}"))?;
    String::from_utf8(plaintext.to_vec())
        .map_err(|_| anyhow::anyhow!("plugin secret is not valid UTF-8"))
}

// =============================================================================
// Helper Functions
// =============================================================================

/// Web sideload (admin uploads a signed zip via the browser) is
/// off by default. The CLI path stays available regardless: an
/// operator on the host can always install local-tier plugins
/// through `nosdesk-cli`, which is the supported escape hatch.
/// Admins on the web UI shouldn't be able to upload arbitrary
/// signed bundles unless the operator has explicitly opted in,
/// because a compromised admin account would otherwise inherit
/// "install any plugin a registered publisher has signed" as a
/// capability.
///
/// Set `NOSDESK_ALLOW_WEB_SIDELOAD=1` (or `true`) to opt in.
pub fn web_sideload_enabled() -> bool {
    matches!(
        std::env::var("NOSDESK_ALLOW_WEB_SIDELOAD")
            .ok()
            .as_deref()
            .map(str::trim),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES"),
    )
}

/// Pull the workspace-pinned actor from the request's `RequestContext`
/// for the lifecycle / install dispatchers that need to drive
/// `with_actor_context` themselves (their error types aren't
/// `diesel::result::Error`, so they can't go through `tc.run`). The
/// `RequestContext` actor is populated by the auth middleware with
/// the workspace pin already attached, so writes inside the
/// resulting txn pass the RLS WITH CHECK.
///
/// Falls back to `helpers::actor_for(...)` for the no-RequestContext
/// path (e.g. handler-level unit tests that bypass middleware).
fn workspace_pinned_actor(req: &HttpRequest, system_ref: &'static str) -> ActorContext {
    if let Some(ctx) = req.extensions().get::<RequestContext>().cloned() {
        return ctx.actor;
    }
    helpers::actor_for(req, system_ref)
}

// =============================================================================
// Plugin CRUD Handlers
// =============================================================================

/// List all plugins (admin only)
pub async fn list_plugins(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let result = tc.run(|conn| {
        let plugins = plugin_repo::list_all_plugins(conn)?;
        // Single round-trip to learn which signing pubkeys are
        // revoked; the map lookup per plugin is O(1). On error
        // we degrade to "no revocation info" rather than 500ing
        // the list, since the list is more important than the
        // badge.
        let revocations = plugin_publishers::revoked_publisher_map(conn).unwrap_or_else(|e| {
            warn!(
                "Failed to load publisher revocation map; list will omit badges: {}",
                e
            );
            Default::default()
        });
        let response: Vec<_> = plugins
            .into_iter()
            .filter_map(|p| {
                let pubkey = p.signer_pubkey.clone();
                PluginResponse::try_from(p).ok().map(|mut r| {
                    r.signer_revoked_at =
                        pubkey.as_deref().and_then(|k| revocations.get(k).copied());
                    r
                })
            })
            .collect();
        Ok::<_, DieselError>(response)
    });

    match result {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => {
            error!("Failed to list plugins: {}", e);
            errors::internal("Failed to list plugins")
        }
    }
}

/// List enabled plugins (for frontend plugin loader - authenticated users)
pub async fn list_enabled_plugins(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    // Any authenticated user can get enabled plugins
    if req.extensions().get::<Claims>().is_none() {
        return errors::unauthorized("Authentication required");
    }

    match tc.run(plugin_repo::list_enabled_plugins) {
        Ok(plugins) => {
            let response: Vec<_> = plugins
                .into_iter()
                .filter_map(|p| PluginResponse::try_from(p).ok())
                .collect();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!("Failed to list enabled plugins: {}", e);
            errors::internal("Failed to list plugins")
        }
    }
}

/// Get a single plugin by UUID (admin only)
pub async fn get_plugin(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let plugin_uuid = path.into_inner();

    enum GetOutcome {
        Ok(crate::models::Plugin, Option<chrono::NaiveDateTime>),
        NotFound,
    }

    let outcome = tc.run(|conn| {
        let plugin = match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
            Ok(p) => p,
            Err(DieselError::NotFound) => return Ok(GetOutcome::NotFound),
            Err(e) => return Err(e),
        };

        let revoked_at = plugin.signer_pubkey.as_deref().and_then(|pk| {
            match plugin_publishers::find_publisher_by_pubkey(conn, pk) {
                Ok(Some(pub_row)) => pub_row.revoked_at,
                _ => None,
            }
        });
        Ok::<_, DieselError>(GetOutcome::Ok(plugin, revoked_at))
    });

    match outcome {
        Ok(GetOutcome::Ok(plugin, revoked_at)) => match PluginResponse::try_from(plugin) {
            Ok(mut response) => {
                response.signer_revoked_at = revoked_at;
                HttpResponse::Ok().json(response)
            }
            Err(e) => {
                error!("Failed to parse plugin manifest: {}", e);
                errors::internal("Invalid plugin manifest")
            }
        },
        Ok(GetOutcome::NotFound) => errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            errors::internal("Failed to get plugin")
        }
    }
}

/// Update a plugin (admin only)
pub async fn update_plugin(
    req: HttpRequest,
    pool: web::Data<Pool>,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    body: web::Json<UpdatePluginRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let claims = match req.extensions().get::<Claims>() {
        Some(claims) => claims.clone(),
        None => return errors::unauthorized("Authentication required"),
    };

    let user_uuid = Uuid::parse_str(&claims.sub).ok();
    let plugin_uuid = path.into_inner();

    // Confirm the plugin exists; the lifecycle dispatch below
    // also looks it up, but failing fast here gives a 404 instead
    // of a generic InternalServerError if it's missing.
    let exists = tc.run(
        |conn| match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
            Ok(_) => Ok::<bool, DieselError>(true),
            Err(DieselError::NotFound) => Ok(false),
            Err(e) => Err(e),
        },
    );
    match exists {
        Ok(true) => {}
        Ok(false) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to look up plugin: {}", e);
            return errors::internal("Failed to load plugin");
        }
    }

    // Enable/disable goes through `lifecycle::apply` so the state
    // transition + activity log are atomic, the (state, action)
    // pair is exhaustively legality-checked, and a quarantined
    // plugin can't be silently un-quarantined via this endpoint.
    //
    // `lifecycle::apply` returns `ActionError` (not DieselError) so
    // we can't drive it through `tc.run`; instead we acquire a pool
    // connection and call `with_actor_context` directly, using the
    // workspace-pinned actor from `RequestContext` so the inner
    // writes pass the RLS WITH CHECK.
    if let Some(enabled) = body.enabled {
        let action = if enabled {
            crate::services::plugins::lifecycle::PluginAction::Enable
        } else {
            crate::services::plugins::lifecycle::PluginAction::Disable
        };
        let actor = workspace_pinned_actor(&req, "plugins_admin");
        let mut conn = match helpers::db_conn(&pool) {
            Ok(c) => c,
            Err(e) => return e,
        };
        let result = actor_session::with_actor_context::<
            _,
            crate::services::plugins::lifecycle::ActionError,
        >(&mut conn, &actor, |conn| {
            crate::services::plugins::lifecycle::apply(conn, plugin_uuid, action, user_uuid)
        });
        match result {
            Ok(_) => {}
            Err(crate::services::plugins::lifecycle::ActionError::NoSuchPlugin) => {
                return errors::not_found_msg("Plugin not found");
            }
            Err(crate::services::plugins::lifecycle::ActionError::InvalidTransition {
                from,
                action,
            }) => {
                return errors::conflict(format!("Cannot {action} a plugin in state {from}"));
            }
            Err(e) => {
                error!("Failed to toggle plugin state: {}", e);
                return errors::internal("Failed to toggle plugin");
            }
        }
    }

    // Manifest edits used to be allowed here; that branch was
    // removed because it bypassed signature reverification. Any
    // change to the stored manifest now flows through the signed
    // install paths (zip upload, registry install) which
    // re-verify end-to-end.
    let updated_plugin = match tc.run(|conn| plugin_repo::get_plugin_by_uuid(conn, plugin_uuid)) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to re-fetch plugin: {}", e);
            return errors::internal("Failed to load plugin");
        }
    };

    match PluginResponse::try_from(updated_plugin) {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => {
            error!("Failed to serialize plugin response: {}", e);
            errors::internal("Plugin updated but response failed")
        }
    }
}

enum ConsentResult {
    Consented(Box<crate::models::Plugin>),
    NotPending(crate::models::PluginState),
}

/// Consent to a plugin's requested permission scope (admin only), advancing it
/// from `AwaitingConsent` to `Installed`. Records the consented scope + who/when
/// so a later version that widens scope can require re-consent. The consented set
/// is the plugin's currently-requested manifest permissions (what the admin saw).
pub async fn consent_to_plugin(
    req: HttpRequest,
    pool: web::Data<Pool>,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let claims = match req.extensions().get::<Claims>() {
        Some(c) => c.clone(),
        None => return errors::unauthorized("Authentication required"),
    };
    let Some(user_uuid) = Uuid::parse_str(&claims.sub).ok() else {
        return errors::unauthorized("Authentication required");
    };
    let plugin_uuid = path.into_inner();
    let actor = workspace_pinned_actor(&req, "plugins_admin");
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let result = actor_session::with_actor_context::<_, DieselError>(&mut conn, &actor, |conn| {
        let plugin = plugin_repo::get_plugin_by_uuid(conn, plugin_uuid)?;
        if !matches!(plugin.state, crate::models::PluginState::AwaitingConsent) {
            return Ok(ConsentResult::NotPending(plugin.state));
        }
        let perms: Vec<String> = plugin
            .parse_manifest()
            .map(|m| m.permissions.iter().map(|p| p.as_string()).collect())
            .unwrap_or_default();
        let updated =
            plugin_repo::consent_plugin(conn, plugin_uuid, serde_json::json!(perms), user_uuid)?;
        Ok(ConsentResult::Consented(Box::new(updated)))
    });

    match result {
        Ok(ConsentResult::Consented(plugin)) => match PluginResponse::try_from(*plugin) {
            Ok(response) => HttpResponse::Ok().json(response),
            Err(e) => {
                error!("Failed to serialize consented plugin: {}", e);
                errors::internal("Consented but response failed")
            }
        },
        Ok(ConsentResult::NotPending(state)) => {
            errors::conflict(format!("Plugin is not awaiting consent (state: {state})"))
        }
        Err(DieselError::NotFound) => errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to consent to plugin: {}", e);
            errors::internal("Failed to consent to plugin")
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
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let claims = match require_auth(&req) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let actor = Uuid::parse_str(&claims.sub).ok();

    let plugin_uuid = path.into_inner();

    let plugin = match tc.run(|conn| plugin_repo::get_plugin_by_uuid(conn, plugin_uuid)) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            return errors::internal("Failed to get plugin");
        }
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

    // Same actor-context pattern as update_plugin: lifecycle::apply
    // returns ActionError, so we drive it through with_actor_context
    // on a freshly checked-out connection rather than tc.run. The
    // workspace pin comes from the RequestContext-derived actor.
    let actor_ctx = workspace_pinned_actor(&req, "plugins_admin");
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let outcome_result = actor_session::with_actor_context::<
        _,
        crate::services::plugins::lifecycle::ActionError,
    >(&mut conn, &actor_ctx, |conn| {
        crate::services::plugins::lifecycle::apply(conn, plugin_uuid, action, actor)
    });
    match outcome_result {
        Ok(outcome) => {
            // Bundle bytes live inline on the plugin row, so cascade
            // uninstall removes them via FK cascade and preserve
            // uninstall leaves them on the row (cheap; the next
            // reinstall overwrites). No filesystem side effect.
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
            errors::not_found_msg("Plugin not found")
        }
        Err(crate::services::plugins::lifecycle::ActionError::InvalidTransition {
            from,
            action,
        }) => errors::conflict(format!("Cannot {action} a plugin in state {from}")),
        Err(e) => {
            error!("Failed to uninstall plugin: {}", e);
            errors::internal("Failed to uninstall plugin")
        }
    }
}

// =============================================================================
// Plugin Settings Handlers
// =============================================================================

/// Get all settings for a plugin (admin only)
pub async fn get_plugin_settings(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let plugin_uuid = path.into_inner();

    enum SettingsOutcome {
        Ok(Vec<crate::models::PluginData>),
        NotFound,
    }

    let outcome = tc.run(|conn| {
        let plugin = match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
            Ok(p) => p,
            Err(DieselError::NotFound) => return Ok(SettingsOutcome::NotFound),
            Err(e) => return Err(e),
        };
        let settings = plugin_repo::get_plugin_settings(conn, plugin.id)?;
        Ok::<_, DieselError>(SettingsOutcome::Ok(settings))
    });

    match outcome {
        Ok(SettingsOutcome::Ok(settings)) => {
            let response: Vec<PluginSettingResponse> =
                settings.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(response)
        }
        Ok(SettingsOutcome::NotFound) => errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin settings: {}", e);
            errors::internal("Failed to get settings")
        }
    }
}

/// Set a plugin setting (admin only)
pub async fn set_plugin_setting(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    body: web::Json<SetPluginDataRequest>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let plugin_uuid = path.into_inner();
    let body = body.into_inner();

    enum SetOutcome {
        Ok(crate::models::PluginData, String),
        NotFound,
        EncryptionFailed,
        NonStringSecret,
    }

    let key = body.key.clone();
    let value = body.value.clone();
    let outcome = tc.run(|conn| {
        let plugin = match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
            Ok(p) => p,
            Err(DieselError::NotFound) => return Ok(SetOutcome::NotFound),
            Err(e) => return Err(e),
        };

        // Check if this is a secret setting from the manifest
        let is_secret = plugin
            .parse_manifest()
            .ok()
            .and_then(|m| {
                m.settings
                    .iter()
                    .find(|s| s.key == key)
                    .map(|s| s.setting_type == "secret")
            })
            .unwrap_or(false);

        // Encrypt secret values before storing. AAD binds the
        // ciphertext to (plugin_uuid, key); see `plugin_secret_aad`.
        let value_to_store = if is_secret {
            match value.as_str() {
                Some(plaintext) => match encrypt_plugin_secret(plaintext, &plugin.uuid, &key) {
                    Ok(hex_blob) => serde_json::Value::String(hex_blob),
                    Err(e) => {
                        error!("Failed to encrypt plugin secret: {}", e);
                        return Ok(SetOutcome::EncryptionFailed);
                    }
                },
                None => return Ok(SetOutcome::NonStringSecret),
            }
        } else {
            value.clone()
        };

        let setting = plugin_repo::set_plugin_setting(
            conn,
            plugin.id,
            key.clone(),
            Some(value_to_store),
            is_secret,
        )?;
        Ok::<_, DieselError>(SetOutcome::Ok(setting, plugin.name))
    });

    match outcome {
        Ok(SetOutcome::Ok(setting, plugin_name)) => {
            info!("Plugin setting updated: {} / {}", plugin_name, body.key);
            HttpResponse::Ok().json(PluginSettingResponse::from(setting))
        }
        Ok(SetOutcome::NotFound) => errors::not_found_msg("Plugin not found"),
        Ok(SetOutcome::EncryptionFailed) => errors::internal("Failed to encrypt plugin secret"),
        Ok(SetOutcome::NonStringSecret) => {
            errors::bad_request("Secret settings must be string values")
        }
        Err(e) => {
            error!("Failed to set plugin setting: {}", e);
            errors::internal("Failed to set setting")
        }
    }
}

/// Delete a plugin setting (admin only)
pub async fn delete_plugin_setting(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<(Uuid, String)>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let (plugin_uuid, key) = path.into_inner();

    enum DeleteOutcome {
        Deleted,
        PluginNotFound,
        SettingNotFound,
    }

    let outcome = tc.run(|conn| {
        let plugin = match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
            Ok(p) => p,
            Err(DieselError::NotFound) => return Ok(DeleteOutcome::PluginNotFound),
            Err(e) => return Err(e),
        };
        match plugin_repo::delete_plugin_setting(conn, plugin.id, &key)? {
            n if n > 0 => Ok::<_, DieselError>(DeleteOutcome::Deleted),
            _ => Ok(DeleteOutcome::SettingNotFound),
        }
    });

    match outcome {
        Ok(DeleteOutcome::Deleted) => HttpResponse::NoContent().finish(),
        Ok(DeleteOutcome::PluginNotFound) => errors::not_found_msg("Plugin not found"),
        Ok(DeleteOutcome::SettingNotFound) => errors::not_found_msg("Setting not found"),
        Err(e) => {
            error!("Failed to delete plugin setting: {}", e);
            errors::internal("Failed to delete setting")
        }
    }
}

// =============================================================================
// Plugin-owned data authorization gate
// =============================================================================

/// Refusal from [`authorize_plugin_data_request`], each mapping to the right
/// HTTP status. Keeps the load + Installed + consented-permission gate in one
/// place for the plugin-owned data endpoints (storage, collections).
pub enum PluginGate {
    /// No such plugin in this workspace (RLS-scoped).
    NotFound,
    /// The plugin exists but isn't Installed (disabled / quarantined / awaiting
    /// consent) — its data surface is closed.
    Inactive,
    /// The plugin is active but its consented grant doesn't include the needed
    /// permission.
    Forbidden(&'static str),
}

impl PluginGate {
    pub fn into_response(self) -> HttpResponse {
        match self {
            PluginGate::NotFound => errors::not_found_msg("Plugin not found"),
            PluginGate::Inactive => errors::forbidden("Plugin is not active"),
            PluginGate::Forbidden(msg) => errors::forbidden(msg),
        }
    }
}

/// Load a plugin and authorize a plugin-owned data request: it must exist (under
/// workspace RLS), be Installed, and have `needed` in its effective (consented)
/// grant. The outer `Result` is the DB error channel (propagate with `?`); the
/// inner is the authorization decision — callers fold `Err(gate)` into their own
/// outcome enum and render it with `gate.into_response()`. This is the
/// server-side boundary: `api.ts`'s check is defense-in-depth, not the gate.
pub fn authorize_plugin_data_request(
    conn: &mut crate::db::DbConnection,
    plugin_uuid: Uuid,
    needed: &str,
    denied_msg: &'static str,
) -> Result<Result<crate::models::Plugin, PluginGate>, DieselError> {
    let plugin = match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return Ok(Err(PluginGate::NotFound)),
        Err(e) => return Err(e),
    };
    if !plugin.is_active() {
        return Ok(Err(PluginGate::Inactive));
    }
    if !plugin.has_effective_permission(needed) {
        return Ok(Err(PluginGate::Forbidden(denied_msg)));
    }
    Ok(Ok(plugin))
}

// =============================================================================
// Plugin Storage Handlers (for plugin runtime use)
// =============================================================================

/// Get storage value for a plugin (authenticated users - for plugin use)
pub async fn get_plugin_storage(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<(Uuid, String)>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return errors::unauthorized("Authentication required");
    }

    let (plugin_uuid, key) = path.into_inner();

    enum StorageOutcome {
        Ok(crate::models::PluginData),
        Empty,
        Gate(PluginGate),
    }

    let key_for_closure = key.clone();
    let outcome = tc.run(|conn| {
        let plugin = match authorize_plugin_data_request(
            conn,
            plugin_uuid,
            "storage:plugin",
            "Plugin has not been granted storage access",
        )? {
            Ok(p) => p,
            Err(gate) => return Ok(StorageOutcome::Gate(gate)),
        };
        match plugin_repo::get_plugin_storage_entry(conn, plugin.id, &key_for_closure) {
            Ok(entry) => Ok::<_, DieselError>(StorageOutcome::Ok(entry)),
            Err(DieselError::NotFound) => Ok(StorageOutcome::Empty),
            Err(e) => Err(e),
        }
    });

    match outcome {
        Ok(StorageOutcome::Ok(entry)) => {
            HttpResponse::Ok().json(PluginStorageResponse::from(entry))
        }
        Ok(StorageOutcome::Empty) => HttpResponse::Ok().json(serde_json::json!({
            "key": key,
            "value": null
        })),
        Ok(StorageOutcome::Gate(gate)) => gate.into_response(),
        Err(e) => {
            error!("Failed to get plugin storage: {}", e);
            errors::internal("Failed to get storage")
        }
    }
}

/// Set storage value for a plugin (authenticated users - for plugin use)
pub async fn set_plugin_storage(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    body: web::Json<SetPluginDataRequest>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return errors::unauthorized("Authentication required");
    }

    let plugin_uuid = path.into_inner();
    let body = body.into_inner();

    enum StorageOutcome {
        Ok(crate::models::PluginData),
        Gate(PluginGate),
    }

    let key = body.key.clone();
    let value = body.value.clone();
    let outcome = tc.run(|conn| {
        let plugin = match authorize_plugin_data_request(
            conn,
            plugin_uuid,
            "storage:plugin",
            "Plugin has not been granted storage access",
        )? {
            Ok(p) => p,
            Err(gate) => return Ok(StorageOutcome::Gate(gate)),
        };
        let entry = plugin_repo::set_plugin_storage(conn, plugin.id, key, Some(value))?;
        Ok::<_, DieselError>(StorageOutcome::Ok(entry))
    });

    match outcome {
        Ok(StorageOutcome::Ok(entry)) => {
            HttpResponse::Ok().json(PluginStorageResponse::from(entry))
        }
        Ok(StorageOutcome::Gate(gate)) => gate.into_response(),
        Err(e) => {
            error!("Failed to set plugin storage: {}", e);
            errors::internal("Failed to set storage")
        }
    }
}

/// Delete storage value for a plugin (authenticated users - for plugin use)
pub async fn delete_plugin_storage(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<(Uuid, String)>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return errors::unauthorized("Authentication required");
    }

    let (plugin_uuid, key) = path.into_inner();

    enum StorageOutcome {
        Deleted,
        Gate(PluginGate),
    }

    let outcome = tc.run(|conn| {
        let plugin = match authorize_plugin_data_request(
            conn,
            plugin_uuid,
            "storage:plugin",
            "Plugin has not been granted storage access",
        )? {
            Ok(p) => p,
            Err(gate) => return Ok(StorageOutcome::Gate(gate)),
        };
        plugin_repo::delete_plugin_storage_entry(conn, plugin.id, &key)?;
        Ok::<_, DieselError>(StorageOutcome::Deleted)
    });

    match outcome {
        Ok(StorageOutcome::Deleted) => HttpResponse::NoContent().finish(),
        Ok(StorageOutcome::Gate(gate)) => gate.into_response(),
        Err(e) => {
            error!("Failed to delete plugin storage: {}", e);
            errors::internal("Failed to delete storage")
        }
    }
}

// =============================================================================
// Plugin Activity Handlers
// =============================================================================

/// Get activity log for a plugin (admin only)
pub async fn get_plugin_activity(
    req: HttpRequest,
    mut tc: TenantConn,
    path: web::Path<Uuid>,
    query: web::Query<PaginationQuery>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    let plugin_uuid = path.into_inner();
    let limit = helpers::clamp_limit(query.limit);
    let offset = helpers::clamp_offset(query.offset);

    enum ActivityOutcome {
        Ok(Vec<crate::models::PluginActivity>),
        NotFound,
    }

    let outcome = tc.run(|conn| {
        let plugin = match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
            Ok(p) => p,
            Err(DieselError::NotFound) => return Ok(ActivityOutcome::NotFound),
            Err(e) => return Err(e),
        };
        let activity = plugin_repo::get_plugin_activity(conn, plugin.id, limit, offset)?;
        Ok::<_, DieselError>(ActivityOutcome::Ok(activity))
    });

    match outcome {
        Ok(ActivityOutcome::Ok(activity)) => {
            let response: Vec<PluginActivityResponse> =
                activity.into_iter().map(Into::into).collect();
            HttpResponse::Ok().json(response)
        }
        Ok(ActivityOutcome::NotFound) => errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin activity: {}", e);
            errors::internal("Failed to get activity")
        }
    }
}

// =============================================================================
// Plugin Proxy Handler
// =============================================================================

/// Per-(workspace, plugin) budget on credentialed egress. The proxy injects the
/// plugin's admin-configured secrets into an outbound request whose path/body the
/// caller controls; any workspace member can drive it (inherent to a client-side
/// plugin holding a server-side credential — see docs/plugin-enforcement-design.md
/// Decision 4). The host allowlist + these caps + the audit trail are the
/// mitigations. Mirrors the plugin-event emitter's budget.
const PLUGIN_PROXY_RATE_MAX: u32 = 60;
const PLUGIN_PROXY_RATE_WINDOW_SECS: u64 = 60;

/// Proxy an external request for a plugin (authenticated users)
pub async fn proxy_plugin_request(
    req: HttpRequest,
    mut tc: TenantConn,
    proxy_service: web::Data<crate::services::plugins::PluginProxyService>,
    path: web::Path<Uuid>,
    body: web::Json<crate::models::PluginProxyRequest>,
) -> impl Responder {
    if req.extensions().get::<Claims>().is_none() {
        return errors::unauthorized("Authentication required");
    }

    let plugin_uuid = path.into_inner();

    enum ProxyOutcome {
        Ok(crate::models::Plugin, Vec<crate::models::PluginData>),
        NotFound,
        Disabled,
    }

    let outcome = tc.run(|conn| {
        let plugin = match plugin_repo::get_plugin_by_uuid(conn, plugin_uuid) {
            Ok(p) => p,
            Err(DieselError::NotFound) => return Ok(ProxyOutcome::NotFound),
            Err(e) => return Err(e),
        };
        if !plugin.is_active() {
            return Ok(ProxyOutcome::Disabled);
        }
        // Fetch plugin settings for auth injection. We squash any error
        // to an empty vec rather than failing the proxy call — same
        // degraded-fallback behaviour as the legacy path.
        let settings = match crate::repository::plugins::get_plugin_settings(conn, plugin.id) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to get plugin settings: {}", e);
                vec![]
            }
        };
        Ok::<_, DieselError>(ProxyOutcome::Ok(plugin, settings))
    });

    let (plugin, settings) = match outcome {
        Ok(ProxyOutcome::Ok(p, s)) => (p, s),
        Ok(ProxyOutcome::NotFound) => return errors::not_found_msg("Plugin not found"),
        Ok(ProxyOutcome::Disabled) => return errors::forbidden("Plugin is disabled"),
        Err(e) => {
            error!("Failed to load plugin for proxy: {}", e);
            return errors::internal("Failed to get plugin");
        }
    };

    let workspace_id = req
        .extensions()
        .get::<RequestContext>()
        .and_then(|ctx| ctx.actor.workspace_id);

    // Bound how fast any member can drive this plugin's credentialed egress.
    // Fail open on a Redis outage (abuse-limiting, not an auth gate), but log.
    {
        let redis_url = crate::utils::rate_limit::get_redis_url();
        let key = format!("plugin_proxy:{}:{}", workspace_id.unwrap_or(0), plugin_uuid);
        match crate::utils::rate_limit::RateLimiter::check_rate_limit(
            &redis_url,
            &key,
            PLUGIN_PROXY_RATE_MAX,
            PLUGIN_PROXY_RATE_WINDOW_SECS,
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => {
                warn!(plugin_uuid = %plugin_uuid, "plugin proxy rate limit exceeded");
                return errors::too_many_requests(
                    "Too many plugin proxy requests",
                    PLUGIN_PROXY_RATE_WINDOW_SECS,
                );
            }
            Err(e) => warn!(error = %e, "plugin proxy rate limiter unavailable; allowing"),
        }
    }

    // Audit the credentialed egress: record who drove it + the target host (never
    // the injected secret or the body). Ops grep on `target=plugin_audit`.
    let actor_ref = req
        .extensions()
        .get::<Claims>()
        .map(|c| c.sub.clone())
        .unwrap_or_default();
    let target_host = url::Url::parse(&body.url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
        .unwrap_or_else(|| "<unparseable>".to_string());
    info!(
        target: "plugin_audit",
        plugin_uuid = %plugin.uuid,
        plugin_name = %plugin.name,
        actor = %actor_ref,
        method = %body.method,
        target_host = %target_host,
        workspace_id = ?workspace_id,
        "plugin proxy egress"
    );

    // Parse the manifest
    let manifest = match plugin.parse_manifest() {
        Ok(m) => m,
        Err(e) => {
            error!("Failed to parse plugin manifest: {}", e);
            return errors::internal("Invalid plugin manifest");
        }
    };

    // Build secrets map for auth injection (decrypt encrypted secrets)
    let mut secrets = std::collections::HashMap::new();
    for setting in settings {
        if setting.is_secret {
            if let Some(value) = setting.value {
                if let Some(encrypted) = value.as_str() {
                    match decrypt_plugin_secret(encrypted, &plugin.uuid, &setting.key) {
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
    match proxy_service
        .proxy_request(&plugin.name, &manifest, body.into_inner(), &secrets)
        .await
    {
        Ok(response) => HttpResponse::Ok().json(response),
        Err(e) => {
            // Structured status per failure mode: 403 permission/SSRF, 400 bad
            // method, 504 timeout, 502 other network faults (was a flat 400).
            error!("Proxy request failed: {}", e);
            HttpResponse::build(e.status_code()).json(serde_json::json!({ "error": e.to_string() }))
        }
    }
}

// =============================================================================
// Plugin Bundle Handlers
// =============================================================================

/// Serve a plugin's `icon.svg` bytes. No auth required: icons are
/// shown in plugin lists that any logged-in user might see, and
/// they carry no secrets. Cache freely; the URL doesn't change
/// when the icon does, but the contents do, so we send a weak
/// `ETag` derived from the plugin's `updated_at` via the route's
/// `Last-Modified` semantics. For simplicity we just cache for 5
/// minutes and let the next install bust it via row update.
pub async fn serve_plugin_icon(mut tc: TenantConn, path: web::Path<Uuid>) -> impl Responder {
    let plugin_uuid = path.into_inner();
    match tc.run(|conn| plugin_repo::get_plugin_icon(conn, plugin_uuid)) {
        Ok((state, _)) if !matches!(state, crate::models::PluginState::Installed) => {
            // Quarantined / disabled / uninstalled plugins do not
            // serve their icon. Mirrors the bundle handler's
            // is_active() gate so an inactive plugin's bytes never
            // leak through any serving endpoint.
            HttpResponse::NotFound().finish()
        }
        Ok((_, Some(bytes))) => HttpResponse::Ok()
            .content_type("image/svg+xml")
            .insert_header(("Cache-Control", "public, max-age=300"))
            .body(bytes),
        Ok((_, None)) => HttpResponse::NotFound().finish(),
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
    mut tc: TenantConn,
    path: web::Path<Uuid>,
) -> impl Responder {
    // Any authenticated user can request plugin bundles
    if req.extensions().get::<Claims>().is_none() {
        return errors::unauthorized("Authentication required");
    }

    let plugin_uuid = path.into_inner();

    // Verify plugin exists and is enabled
    let plugin = match tc.run(|conn| plugin_repo::get_plugin_by_uuid(conn, plugin_uuid)) {
        Ok(p) => p,
        Err(DieselError::NotFound) => return errors::not_found_msg("Plugin not found"),
        Err(e) => {
            error!("Failed to get plugin: {}", e);
            return errors::internal("Failed to get plugin");
        }
    };

    if !plugin.is_active() {
        return errors::forbidden("Plugin is disabled");
    }

    let Some(bytes) = plugin.bundle_js else {
        return errors::not_found_msg("Plugin bundle not found");
    };
    HttpResponse::Ok()
        .content_type("application/javascript")
        .insert_header(("Cache-Control", "private, max-age=3600"))
        .insert_header(("ETag", plugin.bundle_hash.as_deref().unwrap_or("unknown")))
        .body(bytes)
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
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    if !web_sideload_enabled() {
        warn!("Web sideload attempt while disabled; set NOSDESK_ALLOW_WEB_SIDELOAD=1 to enable");
        return errors::forbidden(
            "Web sideload is disabled on this instance. Use the CLI \
             (`nosdesk-cli plugin install`) or set NOSDESK_ALLOW_WEB_SIDELOAD=1 to enable.",
        );
    }

    let claims = match req.extensions().get::<Claims>().cloned() {
        Some(c) => c,
        None => return errors::unauthorized("Authentication required"),
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
                return errors::bad_request("Invalid multipart data");
            }
        };

        // Check content type
        let content_type = field
            .content_type()
            .map(|m| m.to_string())
            .unwrap_or_default();
        if !content_type.contains("zip") && !content_type.contains("octet-stream") {
            continue;
        }

        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(d) => d,
                Err(e) => {
                    error!("Failed to read multipart chunk: {}", e);
                    return errors::bad_request("Failed to read upload");
                }
            };

            if zip_data.len() + data.len() > signing::MAX_ARCHIVE_SIZE {
                return errors::bad_request(format!(
                    "Zip file too large. Maximum size is {} MB",
                    signing::MAX_ARCHIVE_SIZE / (1024 * 1024)
                ));
            }

            zip_data.extend_from_slice(&data);
        }
    }

    if zip_data.is_empty() {
        return errors::bad_request("No zip file received");
    }

    // Verify plugin signature. Web uploads must resolve to a public-
    // chain signer (verified or community publisher, or the Nosdesk
    // root key). Unsigned uploads and `local`-tier signatures are
    // refused here: the local tier is CLI-only by design, so shelling
    // onto the host remains the one path for admin-minted plugins.
    let verified = match signing::verify_archive(&zip_data) {
        Ok(v) => v,
        Err(signing::SigningError::MissingSignature) => {
            return errors::bad_request("This plugin isn't signed. Unsigned plugins must be installed via the nosdesk-plugin CLI.");
        }
        Err(e) => {
            warn!("Plugin zip signature rejected: {}", e);
            return errors::bad_request(format!("Plugin signature rejected: {e}"));
        }
    };

    let resolved_tier = match trust::resolve(&mut conn, &verified.envelope) {
        Ok(t) => t,
        Err(e) => {
            warn!("Plugin publisher not trusted: {}", e);
            return errors::bad_request(format!("Plugin publisher not trusted: {e}"));
        }
    };

    if matches!(resolved_tier, trust::ResolvedTier::Local) {
        return errors::bad_request("Locally-signed plugins must be installed via the nosdesk-plugin CLI, not the admin upload form.");
    }

    let signer = trust::PluginSignerFields::from_verified(&verified, &resolved_tier);
    let options = install::InstallOptions {
        source: "uploaded",
        installed_by: Uuid::parse_str(&claims.sub).ok(),
        log_activity: true,
        provision_settings: false,
        skip_if_unchanged: false,
    };

    // Attribute the install in audit_log via the actor GUCs. We pin
    // the workspace from RequestContext so the inner writes pass the
    // RLS WITH CHECK on plugins / plugin_activity. install_verified
    // runs its own transaction; the GUCs set here propagate down
    // through SET LOCAL.
    let actor = workspace_pinned_actor(&req, "plugins_admin");
    let outcome = match actor_session::with_actor_context::<_, install::InstallError>(
        &mut conn,
        &actor,
        |conn| install::install_verified(conn, &verified.files, signer, resolved_tier, options),
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
            errors::internal("Plugin created but response failed")
        }
    }
}

fn install_error_to_response(err: install::InstallError) -> HttpResponse {
    match err {
        install::InstallError::MissingManifest
        | install::InstallError::InvalidManifest(_)
        | install::InstallError::BundleTooLarge(_)
        | install::InstallError::InvalidIcon(_)
        | install::InstallError::InvalidManifestSchema(_) => errors::bad_request(err.to_string()),
        install::InstallError::ReinstallSignerMismatch { .. }
        | install::InstallError::RefusedQuarantined => {
            // Conflict, not BadRequest: the request is structurally
            // fine, but it conflicts with the existing row's state
            // (quarantined, or signer ownership claim). Admin must
            // resolve the conflict via lifecycle action first.
            errors::conflict(err.to_string())
        }
        install::InstallError::CollectionSchemaSync(_) => {
            // 422: the manifest is structurally valid but its
            // declared collection schema couldn't be applied
            // against the current DB. Operator-fixable via
            // schema inspection / corrective migration.
            error!("Plugin install failed: {}", err);
            HttpResponse::UnprocessableEntity().json(err.to_string())
        }
        install::InstallError::BundleWriteFailed(_) | install::InstallError::Db(_) => {
            error!("Plugin install failed: {}", err);
            errors::internal(err.to_string())
        }
    }
}

// =============================================================================
// Config
// =============================================================================

/// Surface the operator-controlled admin-UI flags so the FE can
/// render the right surface without trial-and-erroring against
/// each gated endpoint. Admin-only because the flags hint at the
/// instance's threat-model posture.
pub async fn get_admin_config(req: HttpRequest) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    HttpResponse::Ok().json(serde_json::json!({
        "web_sideload_enabled": web_sideload_enabled(),
        "registry_enabled": registry::configured_url().is_some(),
    }))
}

/// Aggregate trust-state inventory of installed plugins for the
/// admin panel. Surfaces tier distribution, dev-mode installs (a
/// production red flag), legacy unsigned rows (migration straggler
/// detector), and the top-5 publishers by install count for
/// revocation-blast-radius visibility.
pub async fn get_signing_overview(req: HttpRequest, mut tc: TenantConn) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }

    match tc.run(plugin_repo::signing_overview) {
        Ok(overview) => HttpResponse::Ok().json(overview),
        Err(e) => {
            error!("Failed to compute plugin signing overview: {}", e);
            errors::internal("Failed to compute signing overview")
        }
    }
}

// =============================================================================
// Registry handlers
// =============================================================================

/// Serve the registry state to the admin UI as a tagged status:
///   - `available`  - snapshot ready, included
///   - `disabled`   - operator opted out (NOSDESK_REGISTRY_URL empty)
///   - `failed`     - sync attempted and errored, reason included
///   - `pending`    - boot warm-up, sync not yet completed
///
/// Always returns 200 so the FE doesn't have to special-case the
/// "no data yet" path as an HTTP error. The status string carries
/// the operator intent.
pub async fn get_registry(
    req: HttpRequest,
    cache: web::Data<registry::SharedCache>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    if registry::configured_url().is_none() {
        return HttpResponse::Ok().json(serde_json::json!({ "status": "disabled" }));
    }
    let guard = cache.read().await;
    if let Some(snapshot) = &guard.snapshot {
        return HttpResponse::Ok().json(serde_json::json!({
            "status": "available",
            "snapshot": {
                "fetched_at": snapshot.fetched_at,
                "publishers": snapshot.publishers,
                "index": snapshot.index,
            },
        }));
    }
    if let Some(reason) = &guard.last_error {
        return HttpResponse::Ok().json(serde_json::json!({
            "status": "failed",
            "reason": reason,
        }));
    }
    HttpResponse::Ok().json(serde_json::json!({ "status": "pending" }))
}

/// Force an immediate registry sync and return the resulting state.
/// Backs the admin "Retry" button so it actually retries the
/// upstream fetch instead of just re-reading the cached error from
/// the previous attempt. Same response shape as `get_registry`.
pub async fn refresh_registry(
    req: HttpRequest,
    pool: web::Data<Pool>,
    cache: web::Data<registry::SharedCache>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let base_url = match registry::configured_url() {
        Some(u) => u,
        None => return HttpResponse::Ok().json(serde_json::json!({ "status": "disabled" })),
    };
    let http = match registry::build_http_client() {
        Ok(c) => c,
        Err(e) => {
            error!("HTTP client build failed: {}", e);
            return errors::internal("HTTP client unavailable");
        }
    };
    if let Err(e) = registry::sync_once(&http, &base_url, pool.get_ref(), cache.get_ref()).await {
        let msg = e.to_string();
        warn!(error = %msg, "Manual registry refresh failed");
        cache.write().await.last_error = Some(msg.clone());
        return HttpResponse::Ok().json(serde_json::json!({
            "status": "failed",
            "reason": msg,
        }));
    }
    // sync_once writes snapshot=Some on success unconditionally, so
    // a successful refresh always has a snapshot to render.
    let guard = cache.read().await;
    let snapshot = guard
        .snapshot
        .as_ref()
        .expect("sync_once Ok but cache snapshot is None");
    HttpResponse::Ok().json(serde_json::json!({
        "status": "available",
        "snapshot": {
            "fetched_at": snapshot.fetched_at,
            "publishers": snapshot.publishers,
            "index": snapshot.index,
        },
    }))
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
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let claims = match req.extensions().get::<Claims>().cloned() {
        Some(c) => c,
        None => return errors::unauthorized("Authentication required"),
    };

    // Snapshot the registry entry we intend to install so we can
    // drop the read guard before doing any network or DB work.
    // Captured fields are also what we'll cross-check against the
    // downloaded zip's signature envelope + manifest before commit.
    let (download_url, expected_sha256, claimed_publisher_pubkey, claimed_tier) = {
        let guard = cache.read().await;
        let snapshot = match guard.snapshot.as_ref() {
            Some(s) => s,
            None => {
                return errors::service_unavailable(
                    "Registry snapshot not available yet; wait for background sync",
                );
            }
        };
        let entry = match snapshot.find_plugin(&body.plugin_name) {
            Some(e) => e,
            None => {
                return errors::not_found_msg(format!(
                    "plugin {:?} not in registry",
                    body.plugin_name
                ));
            }
        };
        let version = match entry.resolve_version(body.version.as_deref()) {
            Some(v) => v,
            None => {
                return errors::not_found_msg(format!(
                    "plugin {:?} has no version {:?}",
                    body.plugin_name, body.version
                ));
            }
        };
        (
            version.download_url.clone(),
            version.sha256.clone(),
            entry.publisher_pubkey.clone(),
            entry.tier,
        )
    };

    let http = match registry::build_http_client() {
        Ok(c) => c,
        Err(e) => {
            error!("HTTP client build failed: {}", e);
            return errors::internal("HTTP client unavailable");
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
        return HttpResponse::BadGateway()
            .json("downloaded bundle does not match registry-published sha256");
    }

    let verified = match signing::verify_archive(&bytes) {
        Ok(v) => v,
        Err(e) => {
            return errors::bad_request(format!("signature rejected: {e}"));
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
        return errors::bad_request(
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
            return errors::bad_request(format!("publisher not trusted: {e}"));
        }
    };
    if tier.trust_level() != claimed_tier.as_str() {
        warn!(
            plugin = %body.plugin_name,
            registry_tier = %claimed_tier,
            resolved_tier = tier.trust_level(),
            "Registry / resolved-tier mismatch",
        );
        return errors::bad_request(
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
                return errors::bad_request(
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
    // Same actor-context wrap as the zip-upload path: pull the
    // workspace-pinned actor from RequestContext so the inner
    // writes pass RLS WITH CHECK on plugins / plugin_activity.
    let actor = workspace_pinned_actor(&req, "plugins_admin");
    let outcome = match actor_session::with_actor_context::<_, install::InstallError>(
        &mut conn,
        &actor,
        |conn| install::install_verified(conn, &verified.files, signer, tier, options),
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
            errors::internal("Plugin installed but response failed")
        }
    }
}

async fn download_bundle(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    // Enforce https:// on registry-supplied download URLs. The URL
    // is root-signed, so the attacker would need root-key access to
    // set a malicious http:// value in a signed index, but a
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
            return Err(format!(
                "bundle exceeds {} bytes",
                signing::MAX_ARCHIVE_SIZE
            ));
        }
    }
    let bytes = resp.bytes().await.map_err(|e| format!("read {url}: {e}"))?;
    if bytes.len() > signing::MAX_ARCHIVE_SIZE {
        return Err(format!(
            "bundle exceeds {} bytes",
            signing::MAX_ARCHIVE_SIZE
        ));
    }
    Ok(bytes.to_vec())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use ring::digest::{Context, SHA256};
    let mut ctx = Context::new(&SHA256);
    ctx.update(bytes);
    hex::encode(ctx.finish().as_ref())
}

#[cfg(test)]
mod tests {
    //! Permission-boundary tests. Plugins execute backend code and
    //! can read/write arbitrary plugin_data — the gate is critical.
    use super::*;
    use crate::test_helpers::{claims_for, setup_test_pool};
    use actix_web::test as actix_test;
    use actix_web::{http::StatusCode, App, HttpMessage};

    fn test_app(
        pool: crate::db::Pool,
    ) -> App<
        impl actix_web::dev::ServiceFactory<
            actix_web::dev::ServiceRequest,
            Config = (),
            Response = actix_web::dev::ServiceResponse<impl actix_web::body::MessageBody>,
            Error = actix_web::Error,
            InitError = (),
        >,
    > {
        App::new()
            .app_data(web::Data::new(pool))
            .route("/admin/plugins", web::get().to(list_plugins))
    }

    #[actix_web::test]
    async fn list_requires_authentication() {
        let pool = setup_test_pool();
        let app = actix_test::init_service(test_app(pool)).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/plugins")
            .to_request();
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[actix_web::test]
    async fn list_rejects_user_role() {
        let pool = setup_test_pool();
        let claims = claims_for(&pool, "user");
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/plugins")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn list_rejects_technician_role() {
        // Plugins are admin-only — running custom code requires more
        // trust than the technician role implies.
        let pool = setup_test_pool();
        let claims = claims_for(&pool, "technician");
        let app = actix_test::init_service(test_app(pool.clone())).await;
        let req = actix_test::TestRequest::get()
            .uri("/admin/plugins")
            .to_request();
        req.extensions_mut().insert(claims);
        let resp = actix_test::call_service(&app, req).await;
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }
}
