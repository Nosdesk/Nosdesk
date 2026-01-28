use async_trait::async_trait;
use std::sync::Arc;
use std::io;
use std::path::Path;
use uuid::Uuid;
use actix_web::{HttpResponse, HttpRequest};
use actix_web::http::header::{CONTENT_TYPE, CACHE_CONTROL, ACCEPT_RANGES};
use tracing::error;

/// Storage configuration for different backends
#[derive(Debug, Clone)]
pub enum StorageConfig {
    Local {
        base_path: String,
    },
    #[allow(dead_code)]
    S3 {
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
        endpoint: Option<String>, // For S3-compatible services like MinIO
    },
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
        format!("{}/{}", self.base_path.trim_end_matches('/'), path.trim_start_matches('/'))
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
        format!("{}/{}", self.public_url_base.trim_end_matches('/'), path.trim_start_matches('/'))
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

/// Future S3 storage implementation (placeholder)
pub struct S3Storage {
    _bucket: String,
    _region: String,
    _access_key: String,
    _secret_key: String,
    _endpoint: Option<String>,
}

impl S3Storage {
    pub fn new(
        bucket: String,
        region: String,
        access_key: String,
        secret_key: String,
        endpoint: Option<String>,
    ) -> Self {
        Self {
            _bucket: bucket,
            _region: region,
            _access_key: access_key,
            _secret_key: secret_key,
            _endpoint: endpoint,
        }
    }
}

#[async_trait]
impl Storage for S3Storage {
    async fn store_file(
        &self,
        _data: &[u8],
        _filename: &str,
        _content_type: &str,
        _folder: &str,
    ) -> Result<StoredFile, StorageError> {
        // TODO: Implement S3 upload
        Err(StorageError::ConfigError("S3 storage not implemented yet".to_string()))
    }

    async fn get_file(&self, _path: &str) -> Result<Vec<u8>, StorageError> {
        // TODO: Implement S3 download
        Err(StorageError::ConfigError("S3 storage not implemented yet".to_string()))
    }

    async fn delete_file(&self, _path: &str) -> Result<(), StorageError> {
        // TODO: Implement S3 delete
        Err(StorageError::ConfigError("S3 storage not implemented yet".to_string()))
    }

    async fn file_exists(&self, _path: &str) -> Result<bool, StorageError> {
        // TODO: Implement S3 exists check
        Err(StorageError::ConfigError("S3 storage not implemented yet".to_string()))
    }

    fn get_public_url(&self, _path: &str) -> String {
        // TODO: Implement S3 public URL generation
        String::new()
    }

    async fn move_file(&self, _from_path: &str, _to_path: &str) -> Result<(), StorageError> {
        // TODO: Implement S3 move (copy + delete)
        Err(StorageError::ConfigError("S3 storage not implemented yet".to_string()))
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
        StorageConfig::S3 {
            bucket,
            region,
            access_key,
            secret_key,
            endpoint,
        } => Arc::new(S3Storage::new(bucket, region, access_key, secret_key, endpoint)),
    }
}

/// Get storage configuration from environment variables
pub fn get_storage_config() -> StorageConfig {
    // For now, always use local storage
    // Later, check env vars like STORAGE_TYPE, S3_BUCKET, etc.
    StorageConfig::Local {
        base_path: "/app/uploads".to_string(), // Use Docker volume mount point
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
        .insert_header(("Access-Control-Allow-Headers", "Range, Content-Type, Authorization"))
        .insert_header(("Access-Control-Expose-Headers", "Content-Range, Content-Length, Accept-Ranges"));
    
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
                        end_str.parse::<usize>().unwrap_or(file_data.len() - 1).min(file_data.len() - 1)
                    };
                    
                    if start <= end && start < file_data.len() {
                        let content_length = end - start + 1;
                        let range_data = file_data[start..=end].to_vec();
                        
                        // Return partial content response
                        return Ok(response_builder
                            .status(actix_web::http::StatusCode::PARTIAL_CONTENT)
                            .insert_header(("Content-Length", content_length.to_string()))
                            .insert_header(("Content-Range", format!("bytes {}-{}/{}", start, end, file_data.len())))
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
