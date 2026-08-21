use actix_multipart::Multipart;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, error, info, warn};

use std::sync::Arc;
use uuid::Uuid;

use crate::db::Pool;
use crate::extractors::{ScopedStorage, TenantConn, WorkspaceContext};
use crate::handlers::errors;
use crate::handlers::files::serve_or_not_found;
use crate::handlers::helpers;
use crate::models::{SiteSettingsResponse, UpdateSiteSettings, WorkspaceRole};
use crate::repository::site_settings;
use crate::utils;
use crate::utils::rbac::require_workspace_role;
use crate::utils::storage::{Storage, WorkspaceScopedStorage};

/// Logical storage folder for branding objects. Physically this sits under the
/// workspace prefix `WorkspaceScopedStorage` adds, so one workspace's branding
/// is not addressable from another.
const BRANDING_DIR: &str = "branding";

/// The workspace-relative storage path for a branding image.
///
/// Deterministic per type, like avatars (`users.rs`): a re-upload of the same
/// format overwrites in place, so there is no directory scan to "clean up"
/// after. Cache busting rides on a `?v=` query on the stored URL instead of a
/// unique filename, so the object key stays stable.
fn branding_logical_path(image_type: &str, file_ext: &str) -> String {
    format!("{BRANDING_DIR}/{image_type}.{file_ext}")
}

/// The workspace-relative storage path a branding URL refers to, but only when
/// the URL belongs to `workspace_uuid`.
///
/// Returns `None` for legacy flat URLs (`/uploads/branding/logo_123.png`) by
/// design. Those objects predate workspace scoping and live in a directory
/// shared by every workspace, so deleting one could remove another tenant's
/// file. They are left to age out; the legacy route keeps serving them.
fn owned_logical_path(url: &str, workspace_uuid: Uuid) -> Option<String> {
    let rest = url.strip_prefix("/uploads/branding/")?;
    let rest = rest.split('?').next().unwrap_or(rest);
    let (uuid_segment, filename) = rest.split_once('/')?;
    if uuid_segment != workspace_uuid.to_string() || !is_allowed_branding_filename(filename) {
        return None;
    }
    Some(format!("{BRANDING_DIR}/{filename}"))
}

/// Branding routes (config + image upload), mounted inside the authenticated
/// `/api` scope in main.rs.
pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/admin/branding/config", web::get().to(get_branding_config))
        .route(
            "/admin/branding/config",
            web::patch().to(update_branding_config),
        )
        .route(
            "/admin/branding/image",
            web::post().to(upload_branding_image),
        )
        .route(
            "/admin/branding/image",
            web::delete().to(delete_branding_image),
        );
}

#[derive(Debug, Deserialize)]
pub struct BrandingImageTypeQuery {
    #[serde(rename = "type")]
    pub type_: String, // "logo", "logo_light", or "favicon"
}

#[derive(Debug, Deserialize)]
pub struct UpdateBrandingRequest {
    pub app_name: Option<String>,
    pub primary_color: Option<String>,
    /// Workspace-wide default email signature. Same omission /
    /// empty-string semantics as user-level signature: omitted =
    /// leave alone, empty string = clear back to "no org default".
    #[serde(default)]
    pub signature_default: Option<String>,
    /// Whether to send the "we got your message" auto-ack when a
    /// channel message opens a new ticket. Omitted = leave alone.
    #[serde(default)]
    pub channel_auto_ack_enabled: Option<bool>,
    /// Custom auto-ack body. Same omission / empty-string semantics
    /// as signature_default: omitted = leave alone, empty string =
    /// clear back to "use built-in FTL default for the locale".
    #[serde(default)]
    pub channel_auto_ack_template: Option<String>,
    /// Whether to render the anti-phishing security note in the email
    /// footer. Omitted = leave alone.
    #[serde(default)]
    pub email_security_note_enabled: Option<bool>,
    /// Custom security-note body. Same omission / empty-string
    /// semantics: omitted = leave alone, empty string = clear back to
    /// the built-in localized default.
    #[serde(default)]
    pub email_security_note_template: Option<String>,
}

// GET /api/admin/branding/config - Get branding settings (public for initial load)
pub async fn get_branding_config(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // site_settings is RLS-isolated by workspace; scope the read to the
    // request's workspace (resolved from the Host on every route, public
    // included) so each workspace sees its own branding.
    let actor = helpers::actor_for(&req, "branding:read");
    let loaded = crate::sync::session::with_actor_context(&mut conn, &actor, |conn| {
        site_settings::get_site_settings(conn)
    });
    match loaded {
        Ok(settings) => {
            let response: SiteSettingsResponse = settings.into();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            warn!(error = ?e, "Error fetching site settings, returning defaults");
            // Return defaults if no settings exist
            HttpResponse::Ok().json(json!({
                "app_name": "Nosdesk",
                "logo_url": null,
                "logo_light_url": null,
                "favicon_url": null,
                "primary_color": null,
                "updated_at": null,
                "signature_default": null,
                "channel_auto_ack_enabled": true,
                "channel_auto_ack_template": null,
                "email_security_note_enabled": false,
                "email_security_note_template": null
            }))
        }
    }
}

// GET /api/branding - Public endpoint for branding (no auth required)
pub async fn get_public_branding(req: HttpRequest, pool: web::Data<Pool>) -> impl Responder {
    get_branding_config(req, pool).await
}

// PATCH /api/admin/branding/config - Update branding settings
pub async fn update_branding_config(
    mut tc: TenantConn,
    req: HttpRequest,
    body: web::Json<UpdateBrandingRequest>,
) -> impl Responder {
    // Branding is workspace-wide configuration: only an admin may change it.
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    // Get authenticated user from request
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request("Invalid user UUID");
        }
    };

    // Validate primary_color if provided (must be valid hex color)
    if let Some(ref color) = body.primary_color {
        if !is_valid_hex_color(color) {
            return errors::bad_request(
                "Invalid color format. Must be a valid hex color (e.g., #2C80FF)",
            );
        }
    }

    // Validate signature template tokens up front. Same rule as the
    // per-user signature path in handlers/users.rs: empty / blank
    // skips validation (will be normalized to None below).
    if let Some(ref sig) = body.signature_default {
        if !sig.trim().is_empty() {
            let unknown = crate::utils::template_variables::unknown_variables(
                sig,
                crate::utils::template_variables::SIGNATURE_VARIABLES,
            );
            if !unknown.is_empty() {
                return errors::bad_request(format!(
                    "Unknown signature variables: {}. Supported: {}.",
                    unknown.join(", "),
                    crate::utils::template_variables::SIGNATURE_VARIABLES.join(", ")
                ));
            }
        }
    }

    // Same rule for the auto-ack template — admins shouldn't be able
    // to save `{{tech_name}}` here (auto-ack is system-authored, no
    // agent on hand). Empty / blank clears back to the built-in FTL
    // default.
    if let Some(ref tmpl) = body.channel_auto_ack_template {
        if !tmpl.trim().is_empty() {
            let unknown = crate::utils::template_variables::unknown_variables(
                tmpl,
                crate::utils::template_variables::AUTO_ACK_VARIABLES,
            );
            if !unknown.is_empty() {
                return errors::bad_request(format!(
                    "Unknown auto-ack variables: {}. Supported: {}.",
                    unknown.join(", "),
                    crate::utils::template_variables::AUTO_ACK_VARIABLES.join(", ")
                ));
            }
        }
    }

    // Same allow-list rule for the security note. Empty / blank clears
    // back to the built-in localized default.
    if let Some(ref tmpl) = body.email_security_note_template {
        if !tmpl.trim().is_empty() {
            let unknown = crate::utils::template_variables::unknown_variables(
                tmpl,
                crate::utils::template_variables::SECURITY_NOTE_VARIABLES,
            );
            if !unknown.is_empty() {
                return errors::bad_request(format!(
                    "Unknown security-note variables: {}. Supported: {}.",
                    unknown.join(", "),
                    crate::utils::template_variables::SECURITY_NOTE_VARIABLES.join(", ")
                ));
            }
        }
    }

    // Mirror the user-signature empty-string-is-clear semantic from
    // users.rs so the admin UI can revert to "no org default"
    // without a separate API call.
    let signature_default_change = body.signature_default.as_ref().map(|s| {
        if s.trim().is_empty() {
            None
        } else {
            Some(s.clone())
        }
    });
    let auto_ack_template_change = body.channel_auto_ack_template.as_ref().map(|s| {
        if s.trim().is_empty() {
            None
        } else {
            Some(s.clone())
        }
    });
    let security_note_template_change = body.email_security_note_template.as_ref().map(|s| {
        if s.trim().is_empty() {
            None
        } else {
            Some(s.clone())
        }
    });

    let update = UpdateSiteSettings {
        app_name: body.app_name.clone(),
        logo_url: None,
        logo_light_url: None,
        favicon_url: None,
        primary_color: body.primary_color.as_ref().map(|c| Some(c.clone())),
        updated_by: Some(user_uuid),
        signature_default: signature_default_change,
        channel_auto_ack_enabled: body.channel_auto_ack_enabled,
        channel_auto_ack_template: auto_ack_template_change,
        email_security_note_enabled: body.email_security_note_enabled,
        email_security_note_template: security_note_template_change,
        ..Default::default()
    };

    match tc.run(|conn| site_settings::update_site_settings(conn, update)) {
        Ok(settings) => {
            let response: SiteSettingsResponse = settings.into();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(error = ?e, "Error updating site settings");
            errors::internal("Failed to update branding settings")
        }
    }
}

// POST /api/admin/branding/image - Upload branding image (logo or favicon)
pub async fn upload_branding_image(
    mut payload: Multipart,
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
    storage: ScopedStorage,
    type_query: web::Query<BrandingImageTypeQuery>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let image_type = &type_query.type_;

    // Validate image type
    if !["logo", "logo_light", "favicon"].contains(&image_type.as_str()) {
        return errors::bad_request(
            "Invalid image type. Must be 'logo', 'logo_light', or 'favicon'",
        );
    }

    // Get authenticated user from request
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request("Invalid user UUID");
        }
    };

    info!(image_type = %image_type, user_id = %user_uuid, "Processing branding image upload");

    // Process the uploaded file (we only handle the first field)
    if let Ok(Some(mut field)) = payload.try_next().await {
        let content_type = field
            .content_type()
            .map(|ct| ct.to_string())
            .unwrap_or_else(|| "application/octet-stream".to_string());

        debug!(content_type = %content_type, "Received file upload");

        // Validate content type based on image type
        // SVG is intentionally excluded. An attacker-uploaded SVG is a
        // stored-XSS vector when served inline (it can carry <script> /
        // event handlers), and branding renders on the unauthenticated
        // login screen. The global CSP + nosniff already blunt this, but
        // not accepting SVG at all removes the vector outright. See
        // security-audit-2026-06.
        let valid_types: &[&str] = if image_type == "favicon" {
            &["image/x-icon", "image/vnd.microsoft.icon", "image/png"]
        } else {
            &["image/png", "image/jpeg", "image/webp"]
        };

        if !valid_types.iter().any(|t| content_type.starts_with(t)) {
            let allowed = if image_type == "favicon" {
                "ICO or PNG"
            } else {
                "PNG, JPEG, or WebP"
            };
            return errors::bad_request(format!(
                "Invalid file type for {}. Allowed: {}",
                image_type, allowed
            ));
        }

        // Determine file extension
        let file_ext = match content_type.as_str() {
            "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
            "image/png" => "png",
            "image/svg+xml" => "svg",
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };

        // Read file data
        let mut file_data = Vec::new();
        while let Some(chunk) = field.next().await {
            let data = match chunk {
                Ok(data) => data,
                Err(e) => {
                    error!(error = ?e, "Error reading chunk");
                    return errors::internal("Error reading uploaded file");
                }
            };
            file_data.extend_from_slice(&data);
        }

        // Check file size (max 2MB for branding images)
        if file_data.len() > 2 * 1024 * 1024 {
            return errors::bad_request("File too large. Maximum size is 2MB");
        }

        // What this type pointed at before, so a format change can have its
        // now-unreferenced object removed once the new one is recorded.
        let previous_url = tc
            .run(site_settings::get_site_settings)
            .ok()
            .and_then(|settings| match image_type.as_str() {
                "logo" => settings.logo_url,
                "logo_light" => settings.logo_light_url,
                "favicon" => settings.favicon_url,
                _ => None,
            });

        let filename = format!("{image_type}.{file_ext}");
        let logical_path = branding_logical_path(image_type, file_ext);
        // The object key is stable, so the URL carries the version instead.
        let version = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // The workspace is in the URL because branding is public and has to be
        // addressable from another workspace's context (the switcher renders
        // every workspace you belong to). Serving resolves it back to a scoped
        // storage handle, so the object still cannot be read outside its prefix.
        let url = format!(
            "/uploads/{BRANDING_DIR}/{}/{filename}?v={version}",
            ws.workspace_uuid
        );

        if let Err(e) = storage
            .get()
            .put_file(&file_data, &logical_path, &content_type)
            .await
        {
            error!(error = ?e, logical_path = %logical_path, "Error writing branding image");
            return errors::internal("Failed to save file");
        }

        info!(logical_path = %logical_path, workspace_id = %ws.workspace_id, "Saved branding image");

        // Update the database with the new URL
        let url_for_db = url.clone();
        let image_type_owned = image_type.clone();
        let result = tc.run(|conn| match image_type_owned.as_str() {
            "logo" => site_settings::update_logo_url(conn, Some(url_for_db), user_uuid),
            "logo_light" => site_settings::update_logo_light_url(conn, Some(url_for_db), user_uuid),
            "favicon" => site_settings::update_favicon_url(conn, Some(url_for_db), user_uuid),
            _ => unreachable!(),
        });

        match result {
            Ok(settings) => {
                // Only once the row points at the new object, and only when the
                // old key differs (a format change). Deleting first would risk
                // losing the image if the update failed. `storage` is scoped, so
                // this cannot reach another workspace's object even if the
                // recorded URL were wrong.
                if let Some(superseded) = previous_url
                    .as_deref()
                    .and_then(|u| owned_logical_path(u, ws.workspace_uuid))
                    .filter(|path| path != &logical_path)
                {
                    if let Err(e) = storage.get().delete_file(&superseded).await {
                        warn!(error = ?e, path = %superseded, "Failed to remove superseded branding image");
                    }
                }
                let response: SiteSettingsResponse = settings.into();
                return HttpResponse::Ok().json(json!({
                    "status": "success",
                    "url": url,
                    "settings": response
                }));
            }
            Err(e) => {
                error!(error = ?e, image_type = %image_type, "Error updating site settings");
                return errors::internal("Failed to update branding settings");
            }
        }
    }

    errors::bad_request("No file uploaded")
}

// DELETE /api/admin/branding/image - Remove branding image
pub async fn delete_branding_image(
    mut tc: TenantConn,
    req: HttpRequest,
    ws: WorkspaceContext,
    storage: ScopedStorage,
    type_query: web::Query<BrandingImageTypeQuery>,
) -> impl Responder {
    if let Err(e) = require_workspace_role(&req, WorkspaceRole::Admin) {
        return e;
    }
    let image_type = &type_query.type_;

    if !["logo", "logo_light", "favicon"].contains(&image_type.as_str()) {
        return errors::bad_request(
            "Invalid image type. Must be 'logo', 'logo_light', or 'favicon'",
        );
    }

    // Get authenticated user
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return errors::unauthorized("Authentication required");
        }
    };

    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return errors::bad_request("Invalid user UUID");
        }
    };

    // Get current settings to find the file to delete
    let current_settings = match tc.run(site_settings::get_site_settings) {
        Ok(settings) => settings,
        Err(e) => {
            error!(error = ?e, "Error fetching current settings");
            return errors::internal("Failed to fetch current settings");
        }
    };

    // Get the URL to delete
    let url_to_delete = match image_type.as_str() {
        "logo" => current_settings.logo_url,
        "logo_light" => current_settings.logo_light_url,
        "favicon" => current_settings.favicon_url,
        _ => None,
    };

    // Remove the object through scoped storage, so this works on every backend
    // and cannot address anything outside the workspace. A legacy flat URL
    // yields `None` and is left in place: those objects are shared, so removing
    // one could take another workspace's image with it.
    if let Some(path) = url_to_delete
        .as_deref()
        .and_then(|u| owned_logical_path(u, ws.workspace_uuid))
    {
        if let Err(e) = storage.get().delete_file(&path).await {
            warn!(error = ?e, path = %path, "Failed to delete branding image");
        }
    }

    // Update the database to remove the URL
    let image_type_owned = image_type.clone();
    let result = tc.run(|conn| match image_type_owned.as_str() {
        "logo" => site_settings::update_logo_url(conn, None, user_uuid),
        "logo_light" => site_settings::update_logo_light_url(conn, None, user_uuid),
        "favicon" => site_settings::update_favicon_url(conn, None, user_uuid),
        _ => unreachable!(),
    });

    match result {
        Ok(settings) => {
            let response: SiteSettingsResponse = settings.into();
            HttpResponse::Ok().json(json!({
                "status": "success",
                "settings": response
            }))
        }
        Err(e) => {
            error!(error = ?e, image_type = %image_type, "Error updating site settings");
            errors::internal("Failed to update branding settings")
        }
    }
}

// Helper function to validate hex color
fn is_valid_hex_color(color: &str) -> bool {
    let hex = match color.strip_prefix('#') {
        Some(h) => h,
        None => return false,
    };
    if hex.len() != 6 && hex.len() != 3 {
        return false;
    }
    hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// Validate a branding filename against the exact shape the upload
/// handler writes: `{type}[_{timestamp}].{ext}` (e.g.
/// `logo_1699999999.png`). A `starts_with` prefix check is not enough
/// here because this handler reads from the filesystem directly: a
/// value like `logo/../../../proc/self/environ` starts with an allowed
/// prefix yet escapes the branding directory. Pairs with the
/// `is_safe_storage_path` traversal guard in the caller.
fn is_allowed_branding_filename(filename: &str) -> bool {
    let (stem, ext) = match filename.rsplit_once('.') {
        Some(parts) => parts,
        None => return false,
    };
    if !matches!(ext, "png" | "ico" | "jpg" | "jpeg" | "webp" | "svg") {
        return false;
    }
    // stem is exactly an allowed base, or base + "_" + all-digits.
    ["logo_light", "logo", "favicon"].iter().any(|base| {
        if stem == *base {
            return true;
        }
        match stem.strip_prefix(base).and_then(|r| r.strip_prefix('_')) {
            Some(rest) => !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()),
            None => false,
        }
    })
}

/// GET `/uploads/branding/{workspace_uuid}/{filename}` (public).
///
/// Branding is deliberately public: host-mode login screens render the logo and
/// favicon before anyone authenticates. The workspace comes from the path
/// rather than the request so a page pinned to one workspace can still render
/// another's mark, which is what the workspace switcher needs. Resolution goes
/// through a scoped storage handle, so a request can only ever read objects
/// under that workspace's prefix.
pub async fn serve_workspace_branding_file(
    path: web::Path<(Uuid, String)>,
    req: HttpRequest,
    base_storage: web::Data<Arc<dyn Storage>>,
    pool: web::Data<Pool>,
) -> impl Responder {
    let (workspace_uuid, filename) = path.into_inner();

    if !is_allowed_branding_filename(&filename) {
        return HttpResponse::NotFound().finish();
    }

    let mut conn = match helpers::db_conn(&pool) {
        Ok(c) => c,
        Err(e) => return e,
    };
    // `workspaces` is the resolution table and reads without a pinned GUC. An
    // unknown uuid is a plain 404: this is a public image route, so there is
    // nothing to distinguish from a missing file.
    let workspace = match crate::middleware::workspace_context::resolve_workspace_uuid(
        &mut conn,
        workspace_uuid,
    ) {
        Ok(Some(ctx)) => ctx,
        Ok(None) => return HttpResponse::NotFound().finish(),
        Err(e) => {
            error!(error = ?e, %workspace_uuid, "Branding workspace resolution failed");
            return HttpResponse::NotFound().finish();
        }
    };

    let storage =
        WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace.workspace_id);
    let logical_path = format!("{BRANDING_DIR}/{filename}");
    match serve_or_not_found(storage, &logical_path, &req).await {
        Ok(response) => response,
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

/// GET `/uploads/branding/{filename}` (public, legacy).
///
/// Serves branding uploaded before the objects were workspace-scoped, which sat
/// in one directory shared by every workspace. Read-only and unscoped by
/// necessity: there is no workspace in these paths to scope by. New uploads
/// write scoped keys and rewrite the stored URL, so this drains on its own and
/// no migration has to move files.
pub async fn serve_branding_file(
    filename: web::Path<String>,
    req: HttpRequest,
    base_storage: web::Data<Arc<dyn Storage>>,
) -> impl Responder {
    let filename = filename.into_inner();

    // `is_safe_storage_path` rejects traversal, `is_allowed_branding_filename`
    // pins the exact shape the old upload handler wrote. Both are kept: the
    // storage backend takes a path, and a prefix check alone would let
    // `logo/../../secret` through.
    if !crate::utils::storage::is_safe_storage_path(&filename)
        || !is_allowed_branding_filename(&filename)
    {
        return HttpResponse::NotFound().finish();
    }

    let logical_path = format!("{BRANDING_DIR}/{filename}");
    match serve_or_not_found(base_storage.get_ref().clone(), &logical_path, &req).await {
        Ok(response) => response,
        Err(_) => HttpResponse::NotFound().finish(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        branding_logical_path, is_allowed_branding_filename, owned_logical_path, BRANDING_DIR,
    };
    use uuid::Uuid;

    #[test]
    fn rejects_traversal_and_unexpected_shapes() {
        // Traversal payloads that start with an allowed prefix.
        assert!(!is_allowed_branding_filename(
            "logo/../../../proc/self/environ"
        ));
        assert!(!is_allowed_branding_filename("logo/../secret.png"));
        assert!(!is_allowed_branding_filename("../favicon.ico"));
        // Subdirectories and missing separators.
        assert!(!is_allowed_branding_filename("logo/foo.png"));
        assert!(!is_allowed_branding_filename("logolight.png"));
        // No extension / disallowed extension / non-numeric suffix.
        assert!(!is_allowed_branding_filename("logo"));
        assert!(!is_allowed_branding_filename("logo_123.exe"));
        assert!(!is_allowed_branding_filename("logo_abc.png"));
        assert!(!is_allowed_branding_filename("evil.png"));
    }

    #[test]
    fn accepts_filenames_the_upload_handler_writes() {
        assert!(is_allowed_branding_filename("logo.png"));
        assert!(is_allowed_branding_filename("logo_1699999999.png"));
        assert!(is_allowed_branding_filename("logo_light_1699999999.webp"));
        assert!(is_allowed_branding_filename("favicon_123.ico"));
        assert!(is_allowed_branding_filename("favicon.ico"));
    }

    /// The object key carries no timestamp, so a re-upload overwrites in place.
    /// This is what removes the need for any "delete the old ones" pass, which
    /// is where the cross-workspace deletion came from.
    #[test]
    fn logical_path_is_deterministic_per_type() {
        assert_eq!(branding_logical_path("logo", "png"), "branding/logo.png");
        assert_eq!(
            branding_logical_path("logo", "png"),
            branding_logical_path("logo", "png")
        );
        assert_eq!(
            branding_logical_path("logo_light", "webp"),
            "branding/logo_light.webp"
        );
    }

    /// The isolation property. A URL belonging to another workspace must never
    /// resolve to a path this workspace's scoped storage would act on.
    #[test]
    fn owned_logical_path_only_claims_this_workspaces_objects() {
        let mine = Uuid::from_u128(1);
        let theirs = Uuid::from_u128(2);

        assert_eq!(
            owned_logical_path(&format!("/uploads/branding/{mine}/logo.png"), mine),
            Some("branding/logo.png".to_string())
        );
        // Another workspace's object: not ours to touch.
        assert_eq!(
            owned_logical_path(&format!("/uploads/branding/{theirs}/logo.png"), mine),
            None
        );
        // Legacy flat objects are shared, so they are never claimed.
        assert_eq!(
            owned_logical_path("/uploads/branding/logo_1699999999.png", mine),
            None
        );
        // Cache-busting query is not part of the key.
        assert_eq!(
            owned_logical_path(&format!("/uploads/branding/{mine}/favicon.ico?v=123"), mine),
            Some("branding/favicon.ico".to_string())
        );
        // Traversal and unrelated shapes.
        assert_eq!(
            owned_logical_path(&format!("/uploads/branding/{mine}/../../secret.png"), mine),
            None
        );
        assert_eq!(
            owned_logical_path("/uploads/tickets/1/file.png", mine),
            None
        );
        assert_eq!(owned_logical_path("https://evil.test/logo.png", mine), None);
    }

    /// The URL the upload records and the key the serve route reconstructs are
    /// two halves of one contract; a one-sided change breaks branding silently.
    #[test]
    fn served_key_matches_what_upload_records() {
        let ws = Uuid::from_u128(7);
        let logical = branding_logical_path("logo", "webp");
        let url = format!("/uploads/{BRANDING_DIR}/{ws}/logo.webp?v=1699999999");
        assert_eq!(owned_logical_path(&url, ws), Some(logical));
    }
}
