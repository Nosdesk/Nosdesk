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

/// Dangerous file types that are explicitly blocked
/// These are executable or script files that could be malicious
const BLOCKED_MIME_TYPES: &[&str] = &[
    // Executables
    "application/x-executable",
    "application/x-dosexec",
    "application/x-msdos-program",
    "application/x-msdownload",
    "application/vnd.microsoft.portable-executable",
    // Scripts
    "application/x-sh",
    "application/x-bash",
    "application/x-csh",
    "text/x-shellscript",
    // Java
    "application/java-archive",
    "application/x-java-class",
    // Dynamic libraries
    "application/x-sharedlib",
    "application/x-mach-binary",
];

/// Dangerous file extensions that are explicitly blocked
/// These are checked when magic number detection fails or as additional safety
const BLOCKED_EXTENSIONS: &[&str] = &[
    // Windows executables
    "exe", "dll", "scr", "cpl", "msi", "com", "bat", "cmd", "ps1", "vbs", "vbe", "js", "jse", "ws", "wsf", "wsc", "wsh",
    // Linux/Unix executables
    "sh", "bash", "csh", "ksh", "zsh", "run", "bin",
    // Mac executables
    "app", "command",
    // Java
    "jar", "class",
    // Other potentially dangerous
    "reg", "inf", "scf", "lnk", "pif", "hta", "gadget",
];

/// Custom error type for file validation
#[derive(Debug)]
pub enum FileValidationError {
    FileTooLarge { size: usize, max_size: usize },
    BlockedMimeType { detected: String },
    BlockedExtension { extension: String },
    InvalidFilename(String),
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
            Self::BlockedMimeType { detected } => {
                write!(
                    f,
                    "File type '{detected}' is not allowed for security reasons"
                )
            }
            Self::BlockedExtension { extension } => {
                write!(
                    f,
                    "File extension '.{extension}' is not allowed for security reasons"
                )
            }
            Self::InvalidFilename(msg) => write!(f, "Invalid filename: {msg}"),
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
            FileValidationError::BlockedMimeType { .. } => {
                actix_web::error::ErrorBadRequest(error.to_string())
            }
            FileValidationError::BlockedExtension { .. } => {
                actix_web::error::ErrorBadRequest(error.to_string())
            }
            FileValidationError::InvalidFilename(_) => {
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

    /// Validate file using blocklist approach (block dangerous types, allow everything else)
    /// This is more permissive than an allowlist while still maintaining security
    ///
    /// # Arguments
    /// * `bytes` - First few bytes of the file (at least 512 bytes recommended)
    /// * `filename` - Optional filename to check extension as fallback
    ///
    /// # Returns
    /// The detected MIME type if safe, or "application/octet-stream" for unknown types
    pub fn validate_mime_type(bytes: &[u8]) -> Result<String, FileValidationError> {
        Self::validate_file(bytes, None)
    }

    /// Validate file with optional filename for extension checking
    ///
    /// # Arguments
    /// * `bytes` - First few bytes of the file (at least 512 bytes recommended)
    /// * `filename` - Optional filename to check extension
    ///
    /// # Returns
    /// The detected MIME type if safe, or "application/octet-stream" for unknown types
    pub fn validate_file(bytes: &[u8], filename: Option<&str>) -> Result<String, FileValidationError> {
        // First, check filename extension if provided
        if let Some(name) = filename {
            let extension = Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_lowercase());

            if let Some(ext) = extension {
                if BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
                    return Err(FileValidationError::BlockedExtension { extension: ext });
                }
            }
        }

        // Try to detect MIME type from magic number
        if let Some(kind) = infer::get(bytes) {
            let detected_type = kind.mime_type();

            // Block dangerous MIME types
            if BLOCKED_MIME_TYPES.contains(&detected_type) {
                return Err(FileValidationError::BlockedMimeType {
                    detected: detected_type.to_string(),
                });
            }

            return Ok(detected_type.to_string());
        }

        // Magic number detection failed - this is common for text files (txt, csv, json, md, etc.)
        // These are generally safe, so allow them with a generic MIME type
        // The extension was already checked above if provided
        Ok("application/octet-stream".to_string())
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

    #[test]
    fn test_blocked_extensions() {
        // Blocked extensions should fail
        assert!(FileValidator::validate_file(b"anything", Some("malware.exe")).is_err());
        assert!(FileValidator::validate_file(b"anything", Some("script.sh")).is_err());
        assert!(FileValidator::validate_file(b"anything", Some("payload.bat")).is_err());
        assert!(FileValidator::validate_file(b"anything", Some("virus.dll")).is_err());

        // Safe extensions should pass
        assert!(FileValidator::validate_file(b"anything", Some("document.pdf")).is_ok());
        assert!(FileValidator::validate_file(b"anything", Some("image.png")).is_ok());
        assert!(FileValidator::validate_file(b"anything", Some("notes.txt")).is_ok());
    }

    #[test]
    fn test_unknown_files_allowed() {
        // Files without magic numbers (like text files) should be allowed
        // as long as they don't have blocked extensions
        let plain_text = b"Hello, world! This is plain text.";
        assert!(FileValidator::validate_mime_type(plain_text).is_ok());
        assert!(FileValidator::validate_file(plain_text, Some("readme.txt")).is_ok());
        assert!(FileValidator::validate_file(plain_text, Some("data.csv")).is_ok());
        assert!(FileValidator::validate_file(plain_text, Some("config.json")).is_ok());
    }

}
