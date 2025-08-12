use async_trait::async_trait;
use std::sync::Arc;
use std::io;
use std::path::Path;
use uuid::Uuid;

/// Storage configuration for different backends
#[derive(Debug, Clone)]
pub enum StorageConfig {
    Local {
        base_path: String,
    },
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
    Io(io::Error),
    InvalidPath(String),
    NotFound(String),
    UploadFailed(String),
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
        let unique_filename = format!("{}_{}", Uuid::new_v4(), filename);
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
                Err(StorageError::NotFound(format!("File not found: {}", path)))
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
