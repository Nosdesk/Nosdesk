use actix_web::http::header::{ACCEPT_RANGES, CACHE_CONTROL, CONTENT_TYPE};
use actix_web::{HttpRequest, HttpResponse};
use async_trait::async_trait;
use std::io;
use std::path::Path;
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

/// Storage configuration for different backends.
///
/// Only `Local` exists today. An S3 variant lived here as a stub
/// for several months and was removed because every operation
/// returned "S3 storage not implemented yet" — a footgun for any
/// self-hoster who configured `STORAGE_TYPE=s3` and watched their
/// uploads silently fail. When S3 is implemented for real, it
/// returns as a new variant on a dedicated branch with tests.
#[derive(Debug, Clone)]
pub enum StorageConfig {
    Local { base_path: String },
}

/// File metadata returned after upload
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct StoredFile {
    pub id: String,
    pub url: String,
    pub path: String,
    pub size: u64,
    pub content_type: String,
}

/// Error types for storage operations
#[derive(Debug)]
pub enum StorageError {
    #[allow(dead_code)]
    Io(io::Error),
    #[allow(dead_code)]
    InvalidPath(String),
    #[allow(dead_code)]
    NotFound(String),
    #[allow(dead_code)]
    UploadFailed(String),
    #[allow(dead_code)]
    ConfigError(String),
}

impl From<io::Error> for StorageError {
    fn from(error: io::Error) -> Self {
        StorageError::Io(error)
    }
}

/// Storage trait that all storage backends must implement
#[async_trait]
pub trait Storage: Send + Sync {
    /// Store a file and return metadata
    async fn store_file(
        &self,
        data: &[u8],
        filename: &str,
        content_type: &str,
        folder: &str,
    ) -> Result<StoredFile, StorageError>;

    /// Retrieve a file by path
    async fn get_file(&self, path: &str) -> Result<Vec<u8>, StorageError>;

    /// Delete a file by path
    async fn delete_file(&self, path: &str) -> Result<(), StorageError>;

    /// Check if a file exists
    #[allow(dead_code)]
    async fn file_exists(&self, path: &str) -> Result<bool, StorageError>;

    /// Get a public URL for a file (for serving/downloads)
    fn get_public_url(&self, path: &str) -> String;

    /// Move a file from one location to another (e.g., temp to permanent)
    async fn move_file(&self, from_path: &str, to_path: &str) -> Result<(), StorageError>;
}

/// Local filesystem storage implementation
pub struct LocalStorage {
    base_path: String,
    public_url_base: String,
}

impl LocalStorage {
    pub fn new(base_path: String, public_url_base: String) -> Self {
        Self {
            base_path,
            public_url_base,
        }
    }

    fn get_full_path(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.base_path.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    fn ensure_directory_exists(&self, file_path: &str) -> Result<(), StorageError> {
        if let Some(parent) = Path::new(file_path).parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn store_file(
        &self,
        data: &[u8],
        filename: &str,
        content_type: &str,
        folder: &str,
    ) -> Result<StoredFile, StorageError> {
        // Generate unique filename to prevent collisions
        let unique_filename = format!("{}_{}", Uuid::now_v7(), filename);
        let relative_path = format!("{}/{}", folder.trim_end_matches('/'), unique_filename);
        let full_path = self.get_full_path(&relative_path);

        // Ensure directory exists
        self.ensure_directory_exists(&full_path)?;

        // Write file
        std::fs::write(&full_path, data)?;

        Ok(StoredFile {
            id: unique_filename.clone(),
            url: self.get_public_url(&relative_path),
            path: relative_path,
            size: data.len() as u64,
            content_type: content_type.to_string(),
        })
    }

    async fn get_file(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        let full_path = self.get_full_path(path);
        match std::fs::read(&full_path) {
            Ok(data) => Ok(data),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Err(StorageError::NotFound(format!("File not found: {path}")))
            }
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn delete_file(&self, path: &str) -> Result<(), StorageError> {
        let full_path = self.get_full_path(path);
        match std::fs::remove_file(&full_path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                // File doesn't exist, consider it already deleted
                Ok(())
            }
            Err(e) => Err(StorageError::Io(e)),
        }
    }

    async fn file_exists(&self, path: &str) -> Result<bool, StorageError> {
        let full_path = self.get_full_path(path);
        Ok(Path::new(&full_path).exists())
    }

    fn get_public_url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.public_url_base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn move_file(&self, from_path: &str, to_path: &str) -> Result<(), StorageError> {
        let from_full = self.get_full_path(from_path);
        let to_full = self.get_full_path(to_path);

        // Ensure destination directory exists
        self.ensure_directory_exists(&to_full)?;

        std::fs::rename(&from_full, &to_full)?;
        Ok(())
    }
}

/// Storage factory to create storage instances based on configuration
pub fn create_storage(config: StorageConfig) -> Arc<dyn Storage> {
    match config {
        StorageConfig::Local { base_path } => {
            // In Docker, uploads are mounted at /app/uploads via the backend_uploads volume
            // The public_url_base should match the route pattern used in main.rs: /uploads/users/{path:.*}
            Arc::new(LocalStorage::new(base_path, "/uploads".to_string()))
        }
    }
}

/// Get storage configuration from environment variables.
///
/// Today only `local` is supported. We refuse to boot if the
/// caller asked for anything else — a misconfigured `STORAGE_TYPE`
/// should fail loudly at startup, not silently swallow uploads
/// later.
pub fn get_storage_config() -> StorageConfig {
    match std::env::var("STORAGE_TYPE").as_deref() {
        Ok("local") | Err(_) => StorageConfig::Local {
            base_path: "/app/uploads".to_string(),
        },
        Ok(other) => panic!(
            "STORAGE_TYPE='{other}' is not supported. Only 'local' is implemented. \
             Set STORAGE_TYPE=local or unset it.",
        ),
    }
}

/// Centralized file serving function that works with any storage backend
pub async fn serve_file_from_storage(
    storage: Arc<dyn Storage>,
    path: &str,
    req: &HttpRequest,
) -> Result<HttpResponse, actix_web::Error> {
    // Extract filename from path for content type detection
    let filename = path.split('/').next_back().unwrap_or("file");

    // Get file data from storage
    let file_data = storage.get_file(path).await.map_err(|e| {
        error!("Failed to get file from storage: {:?}", e);
        actix_web::error::ErrorNotFound("File not found")
    })?;

    // Determine content type based on file extension
    let content_type = get_content_type(filename);

    // Build response with proper headers
    let mut response_builder = HttpResponse::Ok();

    response_builder
        .insert_header((CONTENT_TYPE, content_type))
        .insert_header((ACCEPT_RANGES, "bytes"))
        .insert_header((CACHE_CONTROL, "public, max-age=3600"))
        .insert_header(("Access-Control-Allow-Origin", "*"))
        .insert_header(("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS"))
        .insert_header((
            "Access-Control-Allow-Headers",
            "Range, Content-Type, Authorization",
        ))
        .insert_header((
            "Access-Control-Expose-Headers",
            "Content-Range, Content-Length, Accept-Ranges",
        ));

    // Handle range requests for PDF.js and other file types
    let range_header = req.headers().get("Range");
    if let Some(range_value) = range_header {
        if let Ok(range_str) = range_value.to_str() {
            if let Some(range_spec) = range_str.strip_prefix("bytes=") {
                // Remove "bytes="

                // Parse range like "0-1023" or "1024-"
                if let Some((start_str, end_str)) = range_spec.split_once('-') {
                    let start = start_str.parse::<usize>().unwrap_or(0);
                    let end = if end_str.is_empty() {
                        file_data.len() - 1
                    } else {
                        end_str
                            .parse::<usize>()
                            .unwrap_or(file_data.len() - 1)
                            .min(file_data.len() - 1)
                    };

                    if start <= end && start < file_data.len() {
                        let content_length = end - start + 1;
                        let range_data = file_data[start..=end].to_vec();

                        // Return partial content response
                        return Ok(response_builder
                            .status(actix_web::http::StatusCode::PARTIAL_CONTENT)
                            .insert_header(("Content-Length", content_length.to_string()))
                            .insert_header((
                                "Content-Range",
                                format!("bytes {}-{}/{}", start, end, file_data.len()),
                            ))
                            .body(range_data));
                    }
                }
            }
        }
    }

    // Full file response (no range request or invalid range)
    Ok(response_builder
        .insert_header(("Content-Length", file_data.len().to_string()))
        .body(file_data))
}

/// Helper function to determine content type based on file extension
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── get_content_type ─────────────────────────────────────────

    #[test]
    fn content_type_for_common_formats() {
        assert_eq!(get_content_type("photo.jpg"), "image/jpeg");
        assert_eq!(get_content_type("photo.jpeg"), "image/jpeg");
        assert_eq!(get_content_type("photo.png"), "image/png");
        assert_eq!(get_content_type("photo.gif"), "image/gif");
        assert_eq!(get_content_type("photo.webp"), "image/webp");
        assert_eq!(get_content_type("photo.svg"), "image/svg+xml");
        assert_eq!(get_content_type("doc.pdf"), "application/pdf");
        assert_eq!(get_content_type("data.json"), "application/json");
        assert_eq!(get_content_type("archive.zip"), "application/zip");
    }

    #[test]
    fn content_type_case_insensitive() {
        assert_eq!(get_content_type("photo.JPG"), "image/jpeg");
        assert_eq!(get_content_type("doc.PDF"), "application/pdf");
    }

    #[test]
    fn content_type_unknown_returns_octet_stream() {
        assert_eq!(get_content_type("file.xyz"), "application/octet-stream");
        assert_eq!(get_content_type("noext"), "application/octet-stream");
    }

    // ── LocalStorage path handling ───────────────────────────────

    #[test]
    fn get_full_path_joins_correctly() {
        let storage = LocalStorage::new("/app/uploads".into(), "/uploads".into());
        assert_eq!(
            storage.get_full_path("tickets/file.pdf"),
            "/app/uploads/tickets/file.pdf"
        );
    }

    #[test]
    fn get_full_path_handles_extra_slashes() {
        let storage = LocalStorage::new("/app/uploads/".into(), "/uploads".into());
        assert_eq!(
            storage.get_full_path("/tickets/file.pdf"),
            "/app/uploads/tickets/file.pdf"
        );
    }

    #[test]
    fn get_public_url_joins_correctly() {
        let storage = LocalStorage::new("/app/uploads".into(), "/uploads".into());
        assert_eq!(
            storage.get_public_url("tickets/file.pdf"),
            "/uploads/tickets/file.pdf"
        );
    }

    #[test]
    fn get_public_url_handles_extra_slashes() {
        let storage = LocalStorage::new("/app/uploads".into(), "/uploads/".into());
        assert_eq!(
            storage.get_public_url("/tickets/file.pdf"),
            "/uploads/tickets/file.pdf"
        );
    }
}
