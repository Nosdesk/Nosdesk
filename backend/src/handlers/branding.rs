use actix_multipart::Multipart;
use actix_web::{web, HttpMessage, HttpRequest, HttpResponse, Responder};
use futures::{StreamExt, TryStreamExt};
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, error, info, warn};

use crate::db::Pool;
use crate::models::{SiteSettingsResponse, UpdateSiteSettings};
use crate::repository::site_settings;
use crate::utils;

#[derive(Debug, Deserialize)]
pub struct BrandingImageTypeQuery {
    #[serde(rename = "type")]
    pub type_: String, // "logo", "logo_light", or "favicon"
}

#[derive(Debug, Deserialize)]
pub struct UpdateBrandingRequest {
    pub app_name: Option<String>,
    pub primary_color: Option<String>,
}

// GET /api/admin/branding/config - Get branding settings (public for initial load)
pub async fn get_branding_config(pool: web::Data<Pool>) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Database connection error"
            }));
        }
    };

    match site_settings::get_site_settings(&mut conn) {
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
                "updated_at": null
            }))
        }
    }
}

// GET /api/branding - Public endpoint for branding (no auth required)
pub async fn get_public_branding(pool: web::Data<Pool>) -> impl Responder {
    get_branding_config(pool).await
}

// PATCH /api/admin/branding/config - Update branding settings
pub async fn update_branding_config(
    pool: web::Data<Pool>,
    req: HttpRequest,
    body: web::Json<UpdateBrandingRequest>,
) -> impl Responder {
    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Database connection error"
            }));
        }
    };

    // Get authenticated user from request
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Authentication required"
            }));
        }
    };

    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid user UUID"
            }));
        }
    };

    // Validate primary_color if provided (must be valid hex color)
    if let Some(ref color) = body.primary_color {
        if !is_valid_hex_color(color) {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid color format. Must be a valid hex color (e.g., #2C80FF)"
            }));
        }
    }

    let update = UpdateSiteSettings {
        app_name: body.app_name.clone(),
        logo_url: None,
        logo_light_url: None,
        favicon_url: None,
        primary_color: body.primary_color.as_ref().map(|c| Some(c.clone())),
        updated_by: Some(user_uuid),
    };

    match site_settings::update_site_settings(&mut conn, update) {
        Ok(settings) => {
            let response: SiteSettingsResponse = settings.into();
            HttpResponse::Ok().json(response)
        }
        Err(e) => {
            error!(error = ?e, "Error updating site settings");
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to update branding settings"
            }))
        }
    }
}

// POST /api/admin/branding/image - Upload branding image (logo or favicon)
pub async fn upload_branding_image(
    mut payload: Multipart,
    pool: web::Data<Pool>,
    req: HttpRequest,
    type_query: web::Query<BrandingImageTypeQuery>,
) -> impl Responder {
    let image_type = &type_query.type_;

    // Validate image type
    if !["logo", "logo_light", "favicon"].contains(&image_type.as_str()) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid image type. Must be 'logo', 'logo_light', or 'favicon'"
        }));
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Database connection error"
            }));
        }
    };

    // Get authenticated user from request
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Authentication required"
            }));
        }
    };

    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid user UUID"
            }));
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
        let valid_types: &[&str] = if image_type == "favicon" {
            &[
                "image/x-icon",
                "image/vnd.microsoft.icon",
                "image/png",
                "image/svg+xml",
            ]
        } else {
            &["image/png", "image/svg+xml", "image/jpeg", "image/webp"]
        };

        if !valid_types.iter().any(|t| content_type.starts_with(t)) {
            let allowed = if image_type == "favicon" {
                "ICO, PNG, or SVG"
            } else {
                "PNG, SVG, JPEG, or WebP"
            };
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": format!("Invalid file type for {}. Allowed: {}", image_type, allowed)
            }));
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
                    return HttpResponse::InternalServerError().json(json!({
                        "status": "error",
                        "message": "Error reading uploaded file"
                    }));
                }
            };
            file_data.extend_from_slice(&data);
        }

        // Check file size (max 2MB for branding images)
        if file_data.len() > 2 * 1024 * 1024 {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "File too large. Maximum size is 2MB"
            }));
        }

        // Create storage path with timestamp for cache busting
        let storage_dir = "branding";
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let filename = format!("{image_type}_{timestamp}.{file_ext}");
        let storage_path = format!("{storage_dir}/{filename}");
        let url = format!("/uploads/{storage_path}");

        // Ensure directory exists
        let dir_path = format!("uploads/{storage_dir}");
        if let Err(e) = std::fs::create_dir_all(&dir_path) {
            error!(error = ?e, dir_path = %dir_path, "Error creating branding directory");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to create storage directory"
            }));
        }

        // Clean up old files of the same type
        cleanup_old_branding_images(&dir_path, image_type).await;

        // Save the file
        let file_path = format!("uploads/{storage_path}");
        if let Err(e) = std::fs::write(&file_path, &file_data) {
            error!(error = ?e, file_path = %file_path, "Error writing file");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to save file"
            }));
        }

        info!(file_path = %file_path, "Saved branding image");

        // Update the database with the new URL
        let result = match image_type.as_str() {
            "logo" => site_settings::update_logo_url(&mut conn, Some(url.clone()), user_uuid),
            "logo_light" => {
                site_settings::update_logo_light_url(&mut conn, Some(url.clone()), user_uuid)
            }
            "favicon" => site_settings::update_favicon_url(&mut conn, Some(url.clone()), user_uuid),
            _ => unreachable!(),
        };

        match result {
            Ok(settings) => {
                let response: SiteSettingsResponse = settings.into();
                return HttpResponse::Ok().json(json!({
                    "status": "success",
                    "url": url,
                    "settings": response
                }));
            }
            Err(e) => {
                error!(error = ?e, image_type = %image_type, "Error updating site settings");
                return HttpResponse::InternalServerError().json(json!({
                    "status": "error",
                    "message": "Failed to update branding settings"
                }));
            }
        }
    }

    HttpResponse::BadRequest().json(json!({
        "status": "error",
        "message": "No file uploaded"
    }))
}

// DELETE /api/admin/branding/image - Remove branding image
pub async fn delete_branding_image(
    pool: web::Data<Pool>,
    req: HttpRequest,
    type_query: web::Query<BrandingImageTypeQuery>,
) -> impl Responder {
    let image_type = &type_query.type_;

    if !["logo", "logo_light", "favicon"].contains(&image_type.as_str()) {
        return HttpResponse::BadRequest().json(json!({
            "status": "error",
            "message": "Invalid image type. Must be 'logo', 'logo_light', or 'favicon'"
        }));
    }

    let mut conn = match pool.get() {
        Ok(conn) => conn,
        Err(e) => {
            error!(error = ?e, "Database connection error");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Database connection error"
            }));
        }
    };

    // Get authenticated user
    let claims = match req.extensions().get::<crate::models::Claims>() {
        Some(claims) => claims.clone(),
        None => {
            return HttpResponse::Unauthorized().json(json!({
                "status": "error",
                "message": "Authentication required"
            }));
        }
    };

    let user_uuid = match utils::parse_uuid(&claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => {
            return HttpResponse::BadRequest().json(json!({
                "status": "error",
                "message": "Invalid user UUID"
            }));
        }
    };

    // Get current settings to find the file to delete
    let current_settings = match site_settings::get_site_settings(&mut conn) {
        Ok(settings) => settings,
        Err(e) => {
            error!(error = ?e, "Error fetching current settings");
            return HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to fetch current settings"
            }));
        }
    };

    // Get the URL to delete
    let url_to_delete = match image_type.as_str() {
        "logo" => current_settings.logo_url,
        "logo_light" => current_settings.logo_light_url,
        "favicon" => current_settings.favicon_url,
        _ => None,
    };

    // Delete the file if it exists
    if let Some(url) = url_to_delete {
        let file_path = format!("uploads{}", url.trim_start_matches("/uploads"));
        if let Err(e) = std::fs::remove_file(&file_path) {
            warn!(error = ?e, file_path = %file_path, "Failed to delete file");
        }
    }

    // Update the database to remove the URL
    let result = match image_type.as_str() {
        "logo" => site_settings::update_logo_url(&mut conn, None, user_uuid),
        "logo_light" => site_settings::update_logo_light_url(&mut conn, None, user_uuid),
        "favicon" => site_settings::update_favicon_url(&mut conn, None, user_uuid),
        _ => unreachable!(),
    };

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
            HttpResponse::InternalServerError().json(json!({
                "status": "error",
                "message": "Failed to update branding settings"
            }))
        }
    }
}

// Helper function to validate hex color
fn is_valid_hex_color(color: &str) -> bool {
    if !color.starts_with('#') {
        return false;
    }
    let hex = &color[1..];
    if hex.len() != 6 && hex.len() != 3 {
        return false;
    }
    hex.chars().all(|c| c.is_ascii_hexdigit())
}

// Helper function to clean up old branding images
async fn cleanup_old_branding_images(dir: &str, image_type: &str) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(stem) = path.file_stem() {
                let stem_str = stem.to_string_lossy();
                // Match files that start with the image type (e.g., "logo_1234567890")
                if stem_str == image_type || stem_str.starts_with(&format!("{image_type}_")) {
                    if let Err(e) = std::fs::remove_file(&path) {
                        warn!(error = ?e, path = ?path, "Failed to cleanup old file");
                    } else {
                        debug!(path = ?path, "Cleaned up old file");
                    }
                }
            }
        }
    }
}

// Serve branding files publicly (no auth required)
pub async fn serve_branding_file(
    filename: web::Path<String>,
    _req: HttpRequest,
) -> impl Responder {
    let filename = filename.into_inner();

    // Only allow specific branding files
    let allowed_prefixes = ["logo", "logo_light", "favicon"];
    let is_allowed = allowed_prefixes
        .iter()
        .any(|prefix| filename.starts_with(prefix));

    if !is_allowed {
        return HttpResponse::Forbidden().json(json!({
            "status": "error",
            "message": "Access denied"
        }));
    }

    let file_path = format!("uploads/branding/{filename}");

    match std::fs::read(&file_path) {
        Ok(content) => {
            // Determine content type from extension
            let content_type = if filename.ends_with(".svg") {
                "image/svg+xml"
            } else if filename.ends_with(".png") {
                "image/png"
            } else if filename.ends_with(".ico") {
                "image/x-icon"
            } else if filename.ends_with(".jpg") || filename.ends_with(".jpeg") {
                "image/jpeg"
            } else if filename.ends_with(".webp") {
                "image/webp"
            } else {
                "application/octet-stream"
            };

            HttpResponse::Ok()
                .content_type(content_type)
                .insert_header(("Cache-Control", "public, max-age=86400"))
                .body(content)
        }
        Err(_) => HttpResponse::NotFound().finish(),
    }
}
