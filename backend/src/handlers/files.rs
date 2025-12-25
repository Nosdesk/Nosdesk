use actix_web::{web, HttpResponse};
use actix_multipart::Multipart;
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use std::sync::Arc;

use crate::db::DbConnection;
use crate::models::NewAttachment;
use crate::utils::storage::Storage;
use crate::utils::file_validation::FileValidator;

// Upload files using the storage abstraction
pub async fn upload_files(
    mut payload: Multipart,
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    println!("Received file upload request");
    
    let mut conn = pool.get().map_err(|e| {
        eprintln!("Database connection error: {:?}", e);
        actix_web::error::ErrorInternalServerError("Database connection error")
    })?;

    let mut uploaded_attachments = Vec::new();

    // Process each field in the multipart form
    while let Some(mut field) = payload.try_next().await? {
        // Check if the field name is "files"
        let field_name = field.name();
        if field_name != "files" {
            println!("Skipping non-file field: {}", field_name);
            continue;
        }
        
        // Get the filename from the field
        let content_disposition = field.content_disposition();
        let original_filename = content_disposition
            .get_filename()
            .ok_or_else(|| actix_web::error::ErrorBadRequest("Filename is required"))?;

        // SECURITY: Sanitize filename to prevent path traversal attacks
        let sanitized_filename = FileValidator::sanitize_filename(original_filename)
            .map_err(|e| {
                eprintln!("Filename sanitization failed: {:?}", e);
                actix_web::error::ErrorBadRequest(format!("Invalid filename: {}", e))
            })?;

        println!("Processing uploaded file: {} (sanitized: {})", original_filename, sanitized_filename);

        // Read the field data with incremental size validation
        let mut file_data = Vec::new();
        let mut total_size = 0usize;

        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| {
                eprintln!("Error reading chunk: {:?}", e);
                actix_web::error::ErrorInternalServerError("Error reading chunk")
            })?;

            // SECURITY: Validate chunk doesn't cause file to exceed max size
            // This prevents memory exhaustion attacks
            FileValidator::validate_chunk_size(total_size, data.len())?;

            total_size += data.len();
            file_data.extend_from_slice(&data);
        }

        println!("Read {} bytes of data for file {}", total_size, sanitized_filename);

        // SECURITY: Validate file type using magic number detection AND extension check
        // This uses a blocklist approach - blocking dangerous types while allowing most files
        let detected_mime = FileValidator::validate_file(&file_data, Some(&sanitized_filename))
            .map_err(|e| {
                eprintln!("File validation failed: {:?}", e);
                actix_web::error::ErrorBadRequest(format!("Invalid file: {}", e))
            })?;

        println!("Validated MIME type: {}", detected_mime);

        // SECURITY: Compute SHA-256 checksum for file integrity verification
        use ring::digest;
        let checksum_bytes = digest::digest(&digest::SHA256, &file_data);
        let checksum = checksum_bytes.as_ref().iter().map(|b| format!("{:02x}", b)).collect::<String>();

        // Store the file using the storage abstraction with validated MIME type
        let stored_file = storage.store_file(&file_data, &sanitized_filename, &detected_mime, "temp")
            .await
            .map_err(|e| {
                eprintln!("Failed to store file: {:?}", e);
                actix_web::error::ErrorInternalServerError("Failed to store file")
            })?;

        // Create a new attachment record in the database
        let new_attachment = NewAttachment {
            url: stored_file.url.clone(),
            name: sanitized_filename.clone(),
            file_size: Some(total_size as i64),
            mime_type: Some(detected_mime.clone()),
            checksum: Some(checksum),
            comment_id: None, // Not linked to a comment yet
            uploaded_by: None, // Will be set when attached to a comment
        };
        
        println!("Creating attachment record in database: {:?}", new_attachment);
        
        // Save the attachment to the database
        match crate::repository::create_attachment(&mut conn, new_attachment) {
            Ok(attachment) => {
                let attachment_json = json!({
                    "id": attachment.id,
                    "url": stored_file.url,
                    "name": sanitized_filename // Use sanitized filename
                });
                println!("Attachment created successfully: {:?}", attachment_json);
                uploaded_attachments.push(attachment_json);
            },
            Err(e) => {
                eprintln!("Error creating attachment record: {:?}", e);
                return Err(actix_web::error::ErrorInternalServerError("Error creating attachment record"));
            }
        }
    }
    
    println!("Upload complete. Returning {} attachments", uploaded_attachments.len());
    Ok(HttpResponse::Ok().json(uploaded_attachments))
}

// Serve ticket files with token-based authentication
pub async fn serve_ticket_file(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let filename = path.into_inner();
    
    // Extract token from query parameter or Authorization header
    let token = extract_token_from_request(&req)?;
    
    // Validate the token
    let mut conn = pool.get().map_err(|e| {
        eprintln!("Database connection error: {:?}", e);
        actix_web::error::ErrorInternalServerError("Database connection error")
    })?;
    
    // Validate token using existing auth logic
    validate_file_access_token(&token, &mut conn).await?;
    
    // Use our centralized storage method instead of hardcoded paths
    let file_path = format!("tickets/{}", filename);
    match crate::utils::storage::serve_file_from_storage(storage.as_ref().clone(), &file_path, &req).await {
        Ok(response) => Ok(response),
        Err(e) => {
            eprintln!("Error serving ticket file {}: {:?}", file_path, e);
            Err(actix_web::error::ErrorNotFound("File not found"))
        }
    }
}

// Serve temp files with token-based authentication
pub async fn serve_temp_file(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let filename = path.into_inner();
    
    // Extract token from query parameter or Authorization header
    let token = extract_token_from_request(&req)?;
    
    // Validate the token
    let mut conn = pool.get().map_err(|e| {
        eprintln!("Database connection error: {:?}", e);
        actix_web::error::ErrorInternalServerError("Database connection error")
    })?;
    
    // Validate token using existing auth logic
    validate_file_access_token(&token, &mut conn).await?;
    
    // Use our centralized storage method instead of hardcoded paths
    let file_path = format!("temp/{}", filename);
    match crate::utils::storage::serve_file_from_storage(storage.as_ref().clone(), &file_path, &req).await {
        Ok(response) => Ok(response),
        Err(e) => {
            eprintln!("Error serving temp file {}: {:?}", file_path, e);
            Err(actix_web::error::ErrorNotFound("File not found"))
        }
    }
}

// Helper function to extract token from request
// SECURITY: Only accepts tokens via secure channels (cookies or Authorization header)
// Query parameter tokens are NOT supported to prevent token exposure in URLs/logs
fn extract_token_from_request(req: &actix_web::HttpRequest) -> Result<String, actix_web::Error> {
    // First try httpOnly cookie (preferred, most secure method)
    if let Some(cookie) = req.cookie(crate::utils::cookies::ACCESS_TOKEN_COOKIE) {
        return Ok(cookie.value().to_string());
    }

    // Fallback to Authorization header (for API clients)
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Ok(token.to_string());
            }
        }
    }

    Err(actix_web::error::ErrorUnauthorized("No authentication token provided. Use httpOnly cookie or Authorization header."))
}

// Helper function to validate token for file access
async fn validate_file_access_token(
    token: &str,
    conn: &mut DbConnection,
) -> Result<(), actix_web::Error> {
    // Use JWT validation logic directly instead of creating BearerAuth
    use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
    use crate::models::Claims;
    use crate::utils::jwt::JWT_SECRET;
    
    // Create validation with same requirements as auth handler
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    validation.validate_nbf = true;
    validation.leeway = 30;
    
    // Decode the token
    let token_data = match decode::<Claims>(
        token,
        &DecodingKey::from_secret(JWT_SECRET.as_bytes()),
        &validation,
    ) {
        Ok(data) => data,
        Err(_) => return Err(actix_web::error::ErrorUnauthorized("Invalid or expired token")),
    };
    
    // Verify user still exists in database (same as auth handler)
    let user_uuid = match crate::utils::parse_uuid(&token_data.claims.sub) {
        Ok(uuid) => uuid,
        Err(_) => return Err(actix_web::error::ErrorUnauthorized("Invalid user UUID in token")),
    };

    match crate::repository::users::get_user_by_uuid(&user_uuid, conn) {
        Ok(_) => Ok(()),
        Err(_) => Err(actix_web::error::ErrorUnauthorized("User not found")),
    }
}

/// Upload images for ticket notes (collaborative editor)
/// Images are stored in tickets/{ticket_id}/notes/ folder
pub async fn upload_ticket_note_image(
    path: web::Path<i32>,
    mut payload: Multipart,
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let ticket_id = path.into_inner();
    println!("Received ticket note image upload for ticket {}", ticket_id);

    // Verify ticket exists
    let mut conn = pool.get().map_err(|e| {
        eprintln!("Database connection error: {:?}", e);
        actix_web::error::ErrorInternalServerError("Database connection error")
    })?;

    // Check if ticket exists
    crate::repository::tickets::get_ticket_by_id(&mut conn, ticket_id)
        .map_err(|_| actix_web::error::ErrorNotFound("Ticket not found"))?;

    let mut uploaded_files = Vec::new();

    // Process each field in the multipart form
    while let Some(mut field) = payload.try_next().await? {
        let field_name = field.name();
        if field_name != "files" {
            println!("Skipping non-file field: {}", field_name);
            continue;
        }

        // Get the filename from the field
        let content_disposition = field.content_disposition();
        let original_filename = content_disposition
            .get_filename()
            .ok_or_else(|| actix_web::error::ErrorBadRequest("Filename is required"))?;

        // SECURITY: Sanitize filename to prevent path traversal attacks
        let sanitized_filename = FileValidator::sanitize_filename(original_filename)
            .map_err(|e| {
                eprintln!("Filename sanitization failed: {:?}", e);
                actix_web::error::ErrorBadRequest(format!("Invalid filename: {}", e))
            })?;

        println!("Processing ticket note image: {} (sanitized: {})", original_filename, sanitized_filename);

        // Read the field data with incremental size validation
        let mut file_data = Vec::new();
        let mut total_size = 0usize;

        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| {
                eprintln!("Error reading chunk: {:?}", e);
                actix_web::error::ErrorInternalServerError("Error reading chunk")
            })?;

            // SECURITY: Validate chunk doesn't cause file to exceed max size (10MB for images)
            const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;
            if total_size + data.len() > MAX_IMAGE_SIZE {
                return Err(actix_web::error::ErrorBadRequest("File too large (max 10MB)"));
            }

            total_size += data.len();
            file_data.extend_from_slice(&data);
        }

        println!("Read {} bytes of data for file {}", total_size, sanitized_filename);

        // SECURITY: Validate file type with extension check
        let detected_mime = FileValidator::validate_file(&file_data, Some(&sanitized_filename))
            .map_err(|e| {
                eprintln!("File validation failed: {:?}", e);
                actix_web::error::ErrorBadRequest(format!("Invalid file: {}", e))
            })?;

        // Only allow image types for ticket note images
        if !detected_mime.starts_with("image/") {
            return Err(actix_web::error::ErrorBadRequest("Only image files are allowed"));
        }

        println!("Validated MIME type: {}", detected_mime);

        // Store in tickets/{ticket_id}/notes/ folder
        let folder = format!("tickets/{}/notes", ticket_id);
        let stored_file = storage.store_file(&file_data, &sanitized_filename, &detected_mime, &folder)
            .await
            .map_err(|e| {
                eprintln!("Failed to store file: {:?}", e);
                actix_web::error::ErrorInternalServerError("Failed to store file")
            })?;

        println!("Stored ticket note image: {}", stored_file.url);

        uploaded_files.push(json!({
            "url": stored_file.url,
            "name": sanitized_filename,
            "size": total_size
        }));
    }

    println!("Ticket note image upload complete. Returning {} files", uploaded_files.len());
    Ok(HttpResponse::Ok().json(uploaded_files))
}

/// Serve ticket note images
/// Path format: tickets/{ticket_id}/notes/{filename}
pub async fn serve_ticket_note_image(
    path: web::Path<(i32, String)>,
    req: actix_web::HttpRequest,
    pool: web::Data<crate::db::Pool>,
    storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let (ticket_id, filename) = path.into_inner();

    // Extract token from request
    let token = extract_token_from_request(&req)?;

    // Validate the token
    let mut conn = pool.get().map_err(|e| {
        eprintln!("Database connection error: {:?}", e);
        actix_web::error::ErrorInternalServerError("Database connection error")
    })?;

    validate_file_access_token(&token, &mut conn).await?;

    // Serve from tickets/{ticket_id}/notes/ folder
    let file_path = format!("tickets/{}/notes/{}", ticket_id, filename);
    match crate::utils::storage::serve_file_from_storage(storage.as_ref().clone(), &file_path, &req).await {
        Ok(response) => Ok(response),
        Err(e) => {
            eprintln!("Error serving ticket note image {}: {:?}", file_path, e);
            Err(actix_web::error::ErrorNotFound("File not found"))
        }
    }
} 