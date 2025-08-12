use actix_web::{web, HttpResponse};
use actix_multipart::Multipart;
use futures::{StreamExt, TryStreamExt};
use serde_json::json;
use std::sync::Arc;

use crate::db::DbConnection;
use crate::models::NewAttachment;
use crate::repository;
use crate::utils::storage::Storage;

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
        let filename = content_disposition
            .get_filename()
            .map_or_else(|| format!("unnamed_file_{}", uuid::Uuid::new_v4()), |f| f.to_string());
        
        // Get content type if available
        let content_type = field.content_type().map(|ct| ct.to_string()).unwrap_or_else(|| "application/octet-stream".to_string());
        
        println!("Processing uploaded file: {} ({})", filename, content_type);
        

        
        // Read the field data
        let mut file_data = Vec::new();
        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| {
                eprintln!("Error reading chunk: {:?}", e);
                actix_web::error::ErrorInternalServerError("Error reading chunk")
            })?;
            file_data.extend_from_slice(&data);
        }
        
        println!("Read {} bytes of data for file {}", file_data.len(), filename);
        
        // Store the file using the storage abstraction
        let stored_file = storage.store_file(&file_data, &filename, &content_type, "temp")
            .await
            .map_err(|e| {
                eprintln!("Failed to store file: {:?}", e);
                actix_web::error::ErrorInternalServerError("Failed to store file")
            })?;
        
        // Create a new attachment record in the database
        let new_attachment = NewAttachment {
            url: stored_file.url.clone(),
            name: stored_file.id.clone(),
            comment_id: None, // Not linked to a comment yet
        };
        
        println!("Creating attachment record in database: {:?}", new_attachment);
        
        // Save the attachment to the database
        match repository::create_attachment(&mut conn, new_attachment) {
            Ok(attachment) => {
                let attachment_json = json!({
                    "id": attachment.id,
                    "url": stored_file.url,
                    "name": filename // Use original filename for display
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
    
    // Serve the file with proper headers
    let file_path = format!("uploads/tickets/{}", filename);
    serve_file_with_headers(file_path, &req, &filename).await
}

// Serve temp files with token-based authentication
pub async fn serve_temp_file(
    path: web::Path<String>,
    req: actix_web::HttpRequest,
    pool: web::Data<crate::db::Pool>,
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
    
    // Serve the file with proper headers
    let file_path = format!("uploads/temp/{}", filename);
    serve_file_with_headers(file_path, &req, &filename).await
}

// Helper function to extract token from request
fn extract_token_from_request(req: &actix_web::HttpRequest) -> Result<String, actix_web::Error> {
    // First try query parameter ?token=...
    if let Some(token) = req.query_string().split('&')
        .find(|param| param.starts_with("token="))
        .and_then(|param| param.split('=').nth(1)) {
        return Ok(token.to_string());
    }
    
    // Then try Authorization header
    if let Some(auth_header) = req.headers().get("Authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                return Ok(token.to_string());
            }
        }
    }
    
    Err(actix_web::error::ErrorUnauthorized("No authentication token provided"))
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

// Helper function to serve files with proper headers for PDF.js and other file types
async fn serve_file_with_headers(
    file_path: String,
    req: &actix_web::HttpRequest,
    filename: &str,
) -> Result<HttpResponse, actix_web::Error> {
    use std::io::SeekFrom;
    use actix_web::http::header::{
        ACCEPT_RANGES, CACHE_CONTROL, CONTENT_LENGTH, 
        CONTENT_RANGE, CONTENT_TYPE, RANGE
    };
    use tokio::io::{AsyncReadExt, AsyncSeekExt};
    
    // Open file
    let mut file = match tokio::fs::File::open(&file_path).await {
        Ok(file) => file,
        Err(_) => return Err(actix_web::error::ErrorNotFound("File not found")),
    };
    
    // Get file metadata
    let metadata = match file.metadata().await {
        Ok(metadata) => metadata,
        Err(_) => return Err(actix_web::error::ErrorInternalServerError("Could not read file metadata")),
    };
    
    let file_size = metadata.len();
    
    // Determine content type based on file extension
    let content_type = get_content_type(filename);
    
    // Check if this is a range request
    let range_header = req.headers().get(RANGE);
    
    let mut response_builder = HttpResponse::Ok();
    
    // Set common headers
    response_builder
        .insert_header((CONTENT_TYPE, content_type))
        .insert_header((ACCEPT_RANGES, "bytes"))
        .insert_header((CACHE_CONTROL, "public, max-age=3600"))
        .insert_header(("Access-Control-Allow-Origin", "*"))
        .insert_header(("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"))
        .insert_header(("Access-Control-Allow-Headers", "Range, Content-Type, Authorization"))
        .insert_header(("Access-Control-Expose-Headers", "Content-Range, Content-Length, Accept-Ranges"));
    
    // Handle range requests for PDF.js
    if let Some(range_value) = range_header {
        if let Ok(range_str) = range_value.to_str() {
            if range_str.starts_with("bytes=") {
                let range_spec = &range_str[6..]; // Remove "bytes="
                
                // Parse range like "0-1023" or "1024-"
                if let Some((start_str, end_str)) = range_spec.split_once('-') {
                    let start = start_str.parse::<u64>().unwrap_or(0);
                    let end = if end_str.is_empty() {
                        file_size - 1
                    } else {
                        end_str.parse::<u64>().unwrap_or(file_size - 1).min(file_size - 1)
                    };
                    
                    if start <= end && start < file_size {
                        let content_length = end - start + 1;
                        
                        // Seek to start position
                        if let Err(_) = file.seek(SeekFrom::Start(start)).await {
                            return Err(actix_web::error::ErrorInternalServerError("Could not seek in file"));
                        }
                        
                        // Read the requested range
                        let mut buffer = vec![0u8; content_length as usize];
                        if let Err(_) = file.read_exact(&mut buffer).await {
                            return Err(actix_web::error::ErrorInternalServerError("Could not read file range"));
                        }
                        
                        // Return partial content response
                        return Ok(response_builder
                            .status(actix_web::http::StatusCode::PARTIAL_CONTENT)
                            .insert_header((CONTENT_LENGTH, content_length.to_string()))
                            .insert_header((CONTENT_RANGE, format!("bytes {}-{}/{}", start, end, file_size)))
                            .body(buffer));
                    }
                }
            }
        }
    }
    
    // Full file response (no range request or invalid range)
    let mut buffer = Vec::new();
    if let Err(_) = file.read_to_end(&mut buffer).await {
        return Err(actix_web::error::ErrorInternalServerError("Could not read file"));
    }
    
    Ok(response_builder
        .insert_header((CONTENT_LENGTH, file_size.to_string()))
        .body(buffer))
}

// Helper function to determine content type based on file extension
fn get_content_type(filename: &str) -> &'static str {
    let extension = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "txt" => "text/plain",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
} 