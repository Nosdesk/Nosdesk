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
/// `Local` writes to the filesystem; `S3` targets any S3-compatible
/// object store (fly.io Tigris is the deployed target). The app proxies
/// every file through `serve_file_from_storage`, so even the S3 backend
/// uses a private bucket and app-relative public URLs, access control
/// stays in the handlers and no presigned/public bucket URLs are needed.
#[derive(Debug, Clone)]
pub enum StorageConfig {
    Local {
        base_path: String,
    },
    S3 {
        bucket: String,
        region: String,
        endpoint: String,
        access_key_id: String,
        secret_access_key: String,
        /// Tigris serves both virtual-hosted and path-style; default
        /// virtual-hosted (false). Toggle via STORAGE_S3_FORCE_PATH_STYLE
        /// for stores that only do path-style.
        force_path_style: bool,
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
    /// A backend (S3/network) operation failed for a reason other than
    /// a missing object.
    #[allow(dead_code)]
    Backend(String),
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

/// S3-compatible object storage (fly.io Tigris is the deployed target).
///
/// Keys mirror the local layout (`{folder}/{uuid}_{filename}`). Because
/// the app proxies serving via `serve_file_from_storage`, `get_public_url`
/// returns the same app-relative `/uploads/...` path the local backend
/// does, the bucket can stay private and access control stays in the
/// handlers. No streaming: the `Storage` trait is byte-oriented, so each
/// op buffers a whole object (matches the existing local behaviour).
pub struct S3Storage {
    client: aws_sdk_s3::Client,
    bucket: String,
    public_url_base: String,
}

impl S3Storage {
    pub fn new(
        bucket: String,
        region: String,
        endpoint: String,
        access_key_id: String,
        secret_access_key: String,
        force_path_style: bool,
        public_url_base: String,
    ) -> Self {
        use aws_sdk_s3::config::{BehaviorVersion, Credentials, Region};
        let creds = Credentials::new(
            access_key_id,
            secret_access_key,
            None,
            None,
            "nosdesk-storage",
        );
        let conf = aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .region(Region::new(region))
            .endpoint_url(endpoint)
            .credentials_provider(creds)
            .force_path_style(force_path_style)
            .build();
        Self {
            client: aws_sdk_s3::Client::from_conf(conf),
            bucket,
            public_url_base,
        }
    }
}

#[async_trait]
impl Storage for S3Storage {
    async fn store_file(
        &self,
        data: &[u8],
        filename: &str,
        content_type: &str,
        folder: &str,
    ) -> Result<StoredFile, StorageError> {
        let unique_filename = format!("{}_{}", Uuid::now_v7(), filename);
        let key = format!("{}/{}", folder.trim_end_matches('/'), unique_filename);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(aws_sdk_s3::primitives::ByteStream::from(data.to_vec()))
            .content_type(content_type)
            .send()
            .await
            .map_err(|e| StorageError::UploadFailed(format!("S3 put_object failed: {e}")))?;

        let url = self.get_public_url(&key);
        Ok(StoredFile {
            id: unique_filename,
            url,
            path: key,
            size: data.len() as u64,
            content_type: content_type.to_string(),
        })
    }

    async fn get_file(&self, path: &str) -> Result<Vec<u8>, StorageError> {
        use aws_sdk_s3::operation::get_object::GetObjectError;
        let out = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .map_err(|e| match &e {
                aws_sdk_s3::error::SdkError::ServiceError(se)
                    if matches!(se.err(), GetObjectError::NoSuchKey(_)) =>
                {
                    StorageError::NotFound(format!("File not found: {path}"))
                }
                _ => StorageError::Backend(format!("S3 get_object failed: {e}")),
            })?;

        let bytes = out
            .body
            .collect()
            .await
            .map_err(|e| StorageError::Backend(format!("S3 body read failed: {e}")))?
            .into_bytes();
        Ok(bytes.to_vec())
    }

    async fn delete_file(&self, path: &str) -> Result<(), StorageError> {
        // S3 delete is idempotent: deleting a missing key succeeds, so a
        // double-delete is a no-op just like the local backend.
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
            .map_err(|e| StorageError::Backend(format!("S3 delete_object failed: {e}")))?;
        Ok(())
    }

    async fn file_exists(&self, path: &str) -> Result<bool, StorageError> {
        use aws_sdk_s3::operation::head_object::HeadObjectError;
        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(path)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(aws_sdk_s3::error::SdkError::ServiceError(se))
                if matches!(se.err(), HeadObjectError::NotFound(_)) =>
            {
                Ok(false)
            }
            Err(e) => Err(StorageError::Backend(format!("S3 head_object failed: {e}"))),
        }
    }

    fn get_public_url(&self, path: &str) -> String {
        // App-relative proxy path (same as local). Serving goes through
        // serve_file_from_storage, so the bucket stays private.
        format!(
            "{}/{}",
            self.public_url_base.trim_end_matches('/'),
            path.trim_start_matches('/')
        )
    }

    async fn move_file(&self, from_path: &str, to_path: &str) -> Result<(), StorageError> {
        // S3 has no rename: copy then delete. The copy source is
        // `{bucket}/{key}` and must be URL-encoded per the S3 API (the
        // SDK does not encode it for us).
        let encoded_key = urlencoding::encode(from_path.trim_start_matches('/'));
        let copy_source = format!("{}/{}", self.bucket, encoded_key);
        self.client
            .copy_object()
            .bucket(&self.bucket)
            .key(to_path)
            .copy_source(&copy_source)
            .send()
            .await
            .map_err(|e| StorageError::Backend(format!("S3 copy_object failed: {e}")))?;
        self.delete_file(from_path).await?;
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
        StorageConfig::S3 {
            bucket,
            region,
            endpoint,
            access_key_id,
            secret_access_key,
            force_path_style,
        } => Arc::new(S3Storage::new(
            bucket,
            region,
            endpoint,
            access_key_id,
            secret_access_key,
            force_path_style,
            // Same app-relative base as local so the serve routes + frontend
            // URLs are identical regardless of backend.
            "/uploads".to_string(),
        )),
    }
}

/// Get storage configuration from environment variables.
///
/// `local` (default) writes to `/app/uploads`. `s3` reads the env that
/// fly.io `fly storage create` (Tigris) sets, `BUCKET_NAME`,
/// `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ENDPOINT_URL_S3`,
/// `AWS_REGION`, with `STORAGE_S3_*` overrides for other S3-compatible
/// stores. A misconfigured `STORAGE_TYPE` (or `s3` missing required env)
/// fails loudly at startup rather than silently swallowing uploads later.
pub fn get_storage_config() -> StorageConfig {
    match std::env::var("STORAGE_TYPE").as_deref() {
        Ok("local") | Err(_) => StorageConfig::Local {
            base_path: "/app/uploads".to_string(),
        },
        Ok("s3") => {
            // Prefer the AWS_* / BUCKET_NAME names fly's Tigris integration
            // sets so the bucket "just works" on fly; fall back to
            // STORAGE_S3_* for self-hosters pointing at another S3 store.
            fn require(primary: &str, fallback: &str) -> String {
                std::env::var(primary)
                    .or_else(|_| std::env::var(fallback))
                    .unwrap_or_else(|_| {
                        panic!("STORAGE_TYPE=s3 requires {primary} (or {fallback}) to be set")
                    })
            }
            StorageConfig::S3 {
                bucket: require("BUCKET_NAME", "STORAGE_S3_BUCKET"),
                access_key_id: require("AWS_ACCESS_KEY_ID", "STORAGE_S3_ACCESS_KEY_ID"),
                secret_access_key: require("AWS_SECRET_ACCESS_KEY", "STORAGE_S3_SECRET_ACCESS_KEY"),
                endpoint: std::env::var("AWS_ENDPOINT_URL_S3")
                    .or_else(|_| std::env::var("STORAGE_S3_ENDPOINT"))
                    .unwrap_or_else(|_| "https://fly.storage.tigris.dev".to_string()),
                region: std::env::var("AWS_REGION")
                    .or_else(|_| std::env::var("STORAGE_S3_REGION"))
                    .unwrap_or_else(|_| "auto".to_string()),
                force_path_style: matches!(
                    std::env::var("STORAGE_S3_FORCE_PATH_STYLE").as_deref(),
                    Ok("1" | "true" | "yes")
                ),
            }
        }
        Ok(other) => panic!("STORAGE_TYPE='{other}' is not supported. Use 'local' or 's3'.",),
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

    // ── S3Storage URL invariant ──────────────────────────────────
    // The S3 backend must produce the SAME app-relative public URLs as
    // local, that is what keeps the serve routes + frontend URLs identical
    // regardless of backend (the app proxies serving via get_file). new()
    // only builds an aws client config; no network call happens here.

    #[test]
    fn s3_public_url_matches_local_proxy_path() {
        let s3 = S3Storage::new(
            "bucket".into(),
            "auto".into(),
            "https://fly.storage.tigris.dev".into(),
            "ak".into(),
            "sk".into(),
            false,
            "/uploads".into(),
        );
        let local = LocalStorage::new("/app/uploads".into(), "/uploads".into());
        for key in ["users/avatars/x.png", "/tickets/file.pdf"] {
            assert_eq!(s3.get_public_url(key), local.get_public_url(key));
        }
        assert_eq!(
            s3.get_public_url("users/avatars/x.png"),
            "/uploads/users/avatars/x.png"
        );
    }
}
