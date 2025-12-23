use std::path::Path;

/// Maximum file size in bytes (configurable via environment)
/// Default: 50MB matches MAX_FILE_SIZE_MB in main.rs
pub fn get_max_file_size() -> usize {
    std::env::var("MAX_FILE_SIZE_MB")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(50)
        * 1024
        * 1024
}

/// Allowed MIME types for general uploads
/// Combines images, documents, archives, and audio for maximum flexibility
const ALLOWED_MIME_TYPES: &[&str] = &[
    // Images
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/gif",
    "image/bmp",
    "image/svg+xml",
    // Documents
    "application/pdf",
    "text/plain",
    "text/markdown",
    "application/json",
    "text/csv",
    "application/msword",
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    "application/vnd.ms-excel",
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    // Archives
    "application/zip",
    "application/x-tar",
    "application/gzip",
    "application/x-7z-compressed",
    // Audio (for voice notes)
    "audio/webm",
    "audio/ogg",
    "audio/mpeg",
    "audio/mp4",
    "audio/aac",
    "audio/wav",
    "audio/x-wav",
    "audio/m4a",      // infer crate returns this for m4a files
    "audio/x-m4a",    // alternative m4a MIME type
    // Video/WebM (browser may detect voice notes as video/webm)
    "video/webm",
];

/// Custom error type for file validation
#[derive(Debug)]
pub enum FileValidationError {
    FileTooLarge { size: usize, max_size: usize },
    InvalidMimeType { detected: String, allowed_category: String },
    InvalidFilename(String),
    MimeDetectionFailed,
}

impl std::fmt::Display for FileValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FileTooLarge { size, max_size } => {
                write!(
                    f,
                    "File too large: {} bytes exceeds maximum of {} bytes ({} MB)",
                    size,
                    max_size,
                    max_size / (1024 * 1024)
                )
            }
            Self::InvalidMimeType {
                detected,
                allowed_category,
            } => {
                write!(
                    f,
                    "Invalid file type: detected '{}', but only {} files are allowed",
                    detected, allowed_category
                )
            }
            Self::InvalidFilename(msg) => write!(f, "Invalid filename: {}", msg),
            Self::MimeDetectionFailed => write!(f, "Could not detect file type"),
        }
    }
}

impl std::error::Error for FileValidationError {}

/// Convert FileValidationError to Actix error
impl From<FileValidationError> for actix_web::Error {
    fn from(error: FileValidationError) -> Self {
        match error {
            FileValidationError::FileTooLarge { .. } => {
                actix_web::error::ErrorPayloadTooLarge(error.to_string())
            }
            FileValidationError::InvalidMimeType { .. } => {
                actix_web::error::ErrorBadRequest(error.to_string())
            }
            FileValidationError::InvalidFilename(_) => {
                actix_web::error::ErrorBadRequest(error.to_string())
            }
            FileValidationError::MimeDetectionFailed => {
                actix_web::error::ErrorBadRequest(error.to_string())
            }
        }
    }
}

/// File validator with security-focused validation
pub struct FileValidator;

impl FileValidator {
    /// Validate that accumulated file size doesn't exceed maximum
    /// This should be called incrementally as chunks are received
    pub fn validate_chunk_size(
        current_size: usize,
        chunk_len: usize,
    ) -> Result<(), FileValidationError> {
        let max_size = get_max_file_size();
        let new_size = current_size + chunk_len;

        if new_size > max_size {
            return Err(FileValidationError::FileTooLarge {
                size: new_size,
                max_size,
            });
        }

        Ok(())
    }

    /// Validate MIME type using magic number detection (via infer crate)
    /// This is more secure than trusting Content-Type headers or file extensions
    ///
    /// # Arguments
    /// * `bytes` - First few bytes of the file (at least 512 bytes recommended)
    ///
    /// # Returns
    /// The detected MIME type if valid, or an error if invalid/undetectable
    pub fn validate_mime_type(bytes: &[u8]) -> Result<String, FileValidationError> {
        // Detect MIME type from magic number
        let detected_type = infer::get(bytes)
            .map(|kind| kind.mime_type())
            .ok_or(FileValidationError::MimeDetectionFailed)?;

        // Check if detected type is in allowed list
        if !ALLOWED_MIME_TYPES.contains(&detected_type) {
            return Err(FileValidationError::InvalidMimeType {
                detected: detected_type.to_string(),
                allowed_category: "allowed file types (images, documents, archives, audio)".to_string(),
            });
        }

        Ok(detected_type.to_string())
    }

    /// Sanitize filename to prevent path traversal and other attacks
    ///
    /// Security measures:
    /// - Remove path separators (/, \)
    /// - Remove null bytes
    /// - Remove parent directory references (..)
    /// - Keep only alphanumeric, dash, underscore, and dot
    /// - Trim leading/trailing dots (prevent hidden files)
    /// - Limit length to 255 characters
    ///
    /// # Arguments
    /// * `filename` - Original filename from client
    ///
    /// # Returns
    /// Sanitized filename safe for filesystem use
    pub fn sanitize_filename(filename: &str) -> Result<String, FileValidationError> {
        // Reject empty filenames
        if filename.is_empty() {
            return Err(FileValidationError::InvalidFilename(
                "Filename cannot be empty".to_string(),
            ));
        }

        // Get just the filename part (remove any path components)
        let filename = Path::new(filename)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(filename);

        // Filter to safe characters: alphanumeric, dash, underscore, dot
        let sanitized: String = filename
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_' || *c == '.')
            .collect();

        // Trim leading/trailing dots and whitespace
        let sanitized = sanitized.trim_matches(|c: char| c == '.' || c.is_whitespace());

        // Reject if sanitization removed everything
        if sanitized.is_empty() {
            return Err(FileValidationError::InvalidFilename(
                "Filename contains only invalid characters".to_string(),
            ));
        }

        // Limit length to 255 characters (filesystem limit)
        let sanitized = if sanitized.len() > 255 {
            &sanitized[..255]
        } else {
            sanitized
        };

        // Final check for parent directory references
        if sanitized.contains("..") {
            return Err(FileValidationError::InvalidFilename(
                "Filename cannot contain parent directory references".to_string(),
            ));
        }

        Ok(sanitized.to_string())
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_filename() {
        // Valid filenames
        assert_eq!(
            FileValidator::sanitize_filename("document.pdf").unwrap(),
            "document.pdf"
        );
        assert_eq!(
            FileValidator::sanitize_filename("my-file_v2.txt").unwrap(),
            "my-file_v2.txt"
        );

        // Path traversal attempts
        assert!(FileValidator::sanitize_filename("../../etc/passwd").is_ok());
        assert!(FileValidator::sanitize_filename("..\\..\\windows\\system32").is_ok());

        // Null bytes and special characters
        assert!(FileValidator::sanitize_filename("file\0.txt").is_ok());
        assert!(FileValidator::sanitize_filename("file<>:\"|?*.txt").is_ok());

        // Empty and invalid
        assert!(FileValidator::sanitize_filename("").is_err());
        assert!(FileValidator::sanitize_filename("...").is_err());
        assert!(FileValidator::sanitize_filename("<<<>>>").is_err());
    }

    #[test]
    fn test_validate_chunk_size() {
        // Within limit
        assert!(FileValidator::validate_chunk_size(1000, 500).is_ok());

        // Would exceed limit (assuming default 50MB)
        let max = get_max_file_size();
        assert!(FileValidator::validate_chunk_size(max - 100, 200).is_err());
    }

}
