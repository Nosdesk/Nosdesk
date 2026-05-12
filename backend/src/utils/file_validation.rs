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
    // SVG: XML container that browsers execute as a document, with full
    // <script> and event-handler semantics. Treating it as an image is
    // the source of a long line of XSS reports across GitHub, Slack, and
    // others. We refuse uploads entirely; if a real image is wanted, the
    // caller can rasterise to PNG/JPEG client-side.
    "image/svg+xml",
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
    // SVG: see BLOCKED_MIME_TYPES comment. `.svgz` is gzipped SVG and
    // serves identically once decoded by the browser.
    "svg", "svgz",
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

/// Public guest upload: tight allowlist of MIME types. Anything not on this
/// list is rejected outright. Keep this list conservative — expanding it is
/// a security decision.
const GUEST_ALLOWED_MIME_TYPES: &[&str] = &[
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "application/pdf",
    "text/plain",
];

/// Extensions the guest upload path will accept. Must line up semantically
/// with [`GUEST_ALLOWED_MIME_TYPES`]. Files whose magic bytes don't match
/// their extension are rejected.
const GUEST_ALLOWED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "png", "gif", "webp", "pdf", "txt", "log",
];

/// Per-file cap for guest uploads — deliberately tighter than the
/// authenticated `MAX_FILE_SIZE_MB` so a spam wave can't exhaust disk.
pub const GUEST_MAX_FILE_SIZE_MB: usize = 10;

/// Hard cap on the number of attachments a single guest-submitted ticket
/// can reference.
pub const GUEST_MAX_FILES_PER_TICKET: usize = 5;

/// Upload-token freshness window. Attachments not claimed by a submission
/// within this many minutes are considered orphaned and rejected at claim
/// time (they'll be swept later by the temp-file cleanup job).
pub const GUEST_ATTACHMENT_TTL_MINUTES: i64 = 60;

/// Pull the lowercased extension out of a filename. Used by both
/// [`FileValidator::validate_file`] and
/// [`FileValidator::validate_guest_upload`] — keeps the two validators
/// agreeing on what "extension" means (filesystem path-parsed, not
/// substring-after-last-dot which would misparse e.g. `.tar.gz`).
fn file_extension(filename: &str) -> Option<String> {
    Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

/// Detect a MIME type from file magic bytes. Returns `None` if the
/// content doesn't match any signature `infer` knows — the two
/// validators handle that case differently (permissive fallback for the
/// authenticated path, reject-unless-text for the guest path).
fn detected_mime(bytes: &[u8]) -> Option<&'static str> {
    infer::get(bytes).map(|kind| kind.mime_type())
}

/// Defensive SVG sniffer for content whose magic bytes `infer` either
/// missed (leading whitespace, BOM, HTML comments before the root
/// element) or that was renamed to a non-`.svg` extension. A browser
/// will still execute `<script>` inside such content if it's served
/// with an SVG-ish Content-Type, so we refuse it at upload time
/// regardless of how the bytes are dressed up.
fn looks_like_svg(bytes: &[u8]) -> bool {
    let prefix_len = bytes.len().min(1024);
    let Ok(text) = std::str::from_utf8(&bytes[..prefix_len]) else {
        return false;
    };
    let trimmed = text
        .trim_start_matches('\u{feff}')
        .trim_start_matches(|c: char| c.is_whitespace());
    trimmed.starts_with("<?xml") && trimmed.to_ascii_lowercase().contains("<svg")
        || trimmed.starts_with("<svg")
        || trimmed.to_ascii_lowercase().starts_with("<svg")
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
    /// Validate file with optional filename for extension checking
    ///
    /// # Arguments
    /// * `bytes` - First few bytes of the file (at least 512 bytes recommended)
    /// * `filename` - Optional filename to check extension
    ///
    /// # Returns
    /// The detected MIME type if safe, or "application/octet-stream" for unknown types
    pub fn validate_file(bytes: &[u8], filename: Option<&str>) -> Result<String, FileValidationError> {
        if let Some(name) = filename {
            if let Some(ext) = file_extension(name) {
                if BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
                    return Err(FileValidationError::BlockedExtension { extension: ext });
                }
            }
        }

        // SVG check runs ahead of `infer` because `infer` classifies
        // many SVG documents as `text/xml` (matching the XML prolog
        // before the SVG-specific signature), which would slip past
        // the MIME blocklist. Refuse SVG bytes regardless of the
        // claimed extension or detected MIME.
        if looks_like_svg(bytes) {
            return Err(FileValidationError::BlockedMimeType {
                detected: "image/svg+xml".to_string(),
            });
        }

        if let Some(mime) = detected_mime(bytes) {
            if BLOCKED_MIME_TYPES.contains(&mime) {
                return Err(FileValidationError::BlockedMimeType {
                    detected: mime.to_string(),
                });
            }
            return Ok(mime.to_string());
        }

        // Magic-byte detection commonly misses text/csv/json/md. These are
        // generally safe (extension-blocked above if dangerous) so we let
        // them through with a generic MIME type.
        Ok("application/octet-stream".to_string())
    }

    /// Stricter sibling of [`validate_file`] for the public guest upload
    /// endpoint. Uses an *allowlist* (not a blocklist) — any file whose
    /// magic-byte-detected MIME type isn't in [`GUEST_ALLOWED_MIME_TYPES`]
    /// is rejected, and the extension must also appear on the matching
    /// allowlist. Text/log files, where magic-byte detection doesn't fire,
    /// are allowed only when the extension is `.txt` or `.log`.
    pub fn validate_guest_upload(
        bytes: &[u8],
        filename: &str,
    ) -> Result<String, FileValidationError> {
        // Size is enforced at chunk read time via validate_chunk_size with a
        // tighter guest cap, but double-check the total here.
        let max_size = GUEST_MAX_FILE_SIZE_MB * 1024 * 1024;
        if bytes.len() > max_size {
            return Err(FileValidationError::FileTooLarge {
                size: bytes.len(),
                max_size,
            });
        }

        let extension = file_extension(filename);
        let ext_ok = extension
            .as_deref()
            .map(|e| GUEST_ALLOWED_EXTENSIONS.contains(&e))
            .unwrap_or(false);

        if !ext_ok {
            return Err(FileValidationError::BlockedExtension {
                extension: extension.unwrap_or_else(|| "(none)".to_string()),
            });
        }

        // SVG sniffer runs ahead of the MIME branch for the same
        // reason as `validate_file`: `infer` reports many SVG docs
        // as `text/xml`, which a downstream serve path could still
        // hand back with an SVG Content-Type.
        if looks_like_svg(bytes) {
            return Err(FileValidationError::BlockedMimeType {
                detected: "image/svg+xml".to_string(),
            });
        }

        if let Some(detected) = detected_mime(bytes) {
            if !GUEST_ALLOWED_MIME_TYPES.contains(&detected) {
                return Err(FileValidationError::BlockedMimeType {
                    detected: detected.to_string(),
                });
            }
            return Ok(detected.to_string());
        }

        // Magic detection didn't match — only allowed for plaintext-ish
        // extensions where `infer` often returns nothing.
        match extension.as_deref() {
            Some("txt") | Some("log") => Ok("text/plain".to_string()),
            _ => Err(FileValidationError::BlockedMimeType {
                detected: "unknown".to_string(),
            }),
        }
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
        assert!(FileValidator::validate_file(plain_text, None).is_ok());
        assert!(FileValidator::validate_file(plain_text, Some("readme.txt")).is_ok());
        assert!(FileValidator::validate_file(plain_text, Some("data.csv")).is_ok());
        assert!(FileValidator::validate_file(plain_text, Some("config.json")).is_ok());
    }

    // ---- Guest upload allowlist tests ----
    //
    // Each fake file below starts with the real magic bytes of the claimed
    // format. `infer::get` only needs the header, so a short prefix is
    // enough to verify the allowlist pathway without reading real files
    // off disk.

    const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";
    const JPEG_HEADER: &[u8] = b"\xFF\xD8\xFF\xE0\x00\x10JFIF";
    const GIF_HEADER: &[u8] = b"GIF89a";
    const PDF_HEADER: &[u8] = b"%PDF-1.7\n";

    #[test]
    fn guest_upload_accepts_allowed_images() {
        assert!(FileValidator::validate_guest_upload(PNG_HEADER, "screenshot.png").is_ok());
        assert!(FileValidator::validate_guest_upload(JPEG_HEADER, "photo.jpg").is_ok());
        assert!(FileValidator::validate_guest_upload(GIF_HEADER, "animation.gif").is_ok());
    }

    #[test]
    fn guest_upload_accepts_pdf() {
        assert!(FileValidator::validate_guest_upload(PDF_HEADER, "report.pdf").is_ok());
    }

    #[test]
    fn guest_upload_accepts_text_and_log_by_extension() {
        // Plain text has no magic bytes, so acceptance falls through to the
        // extension check. Only the two plaintext extensions pass.
        let text = b"error: something went wrong";
        assert!(FileValidator::validate_guest_upload(text, "notes.txt").is_ok());
        assert!(FileValidator::validate_guest_upload(text, "app.log").is_ok());
    }

    #[test]
    fn guest_upload_rejects_unknown_extension() {
        // Right magic bytes, wrong extension → still rejected.
        assert!(matches!(
            FileValidator::validate_guest_upload(PNG_HEADER, "screenshot.webm"),
            Err(FileValidationError::BlockedExtension { .. })
        ));
    }

    #[test]
    fn guest_upload_rejects_executable_magic() {
        // Raw shell script — not on the MIME allowlist. infer won't detect
        // a MIME so we land in the text/log fallback, which rejects the
        // `.sh` extension.
        let sh = b"#!/bin/bash\necho pwned";
        assert!(matches!(
            FileValidator::validate_guest_upload(sh, "rogue.sh"),
            Err(FileValidationError::BlockedExtension { .. })
        ));
    }

    #[test]
    fn guest_upload_rejects_mime_spoofing() {
        // Binary claiming .png but contents are a ZIP (a common malware
        // wrapper). `infer` detects the zip MIME; it's not on the allowlist.
        let zip_header: &[u8] = b"PK\x03\x04\x14\x00\x00\x00\x08\x00";
        assert!(matches!(
            FileValidator::validate_guest_upload(zip_header, "innocent.png"),
            Err(FileValidationError::BlockedMimeType { .. })
        ));
    }

    #[test]
    fn guest_upload_rejects_oversized_file() {
        let too_big = vec![0u8; GUEST_MAX_FILE_SIZE_MB * 1024 * 1024 + 1];
        assert!(matches!(
            FileValidator::validate_guest_upload(&too_big, "payload.png"),
            Err(FileValidationError::FileTooLarge { .. })
        ));
    }

    #[test]
    fn guest_upload_rejects_missing_extension() {
        assert!(matches!(
            FileValidator::validate_guest_upload(PNG_HEADER, "noextension"),
            Err(FileValidationError::BlockedExtension { .. })
        ));
    }

    #[test]
    fn guest_upload_rejects_unknown_magic_bytes_on_image_extension() {
        // Extension says .png but the content has no recognisable header
        // and no text/log fallback applies.
        let garbage = b"not really an image";
        assert!(matches!(
            FileValidator::validate_guest_upload(garbage, "fake.png"),
            Err(FileValidationError::BlockedMimeType { .. })
        ));
    }

    // ---- SVG blocking (AUD-006) ----

    const SVG_WELL_FORMED: &[u8] =
        b"<?xml version=\"1.0\"?><svg xmlns=\"http://www.w3.org/2000/svg\"><script>alert(1)</script></svg>";
    const SVG_BARE_TAG: &[u8] = b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>";
    const SVG_WITH_BOM: &[u8] = b"\xef\xbb\xbf<?xml version=\"1.0\"?><svg></svg>";
    const SVG_LEADING_WHITESPACE: &[u8] = b"\n\n  <?xml version=\"1.0\"?><svg></svg>";

    #[test]
    fn validate_file_rejects_svg_extension() {
        assert!(matches!(
            FileValidator::validate_file(b"anything", Some("logo.svg")),
            Err(FileValidationError::BlockedExtension { .. })
        ));
        assert!(matches!(
            FileValidator::validate_file(b"anything", Some("logo.svgz")),
            Err(FileValidationError::BlockedExtension { .. })
        ));
    }

    #[test]
    fn validate_file_rejects_svg_renamed_to_png() {
        // Extension `.png` doesn't trip the extension blocklist, but
        // `infer` will see the SVG magic bytes and the MIME blocklist
        // refuses image/svg+xml.
        assert!(matches!(
            FileValidator::validate_file(SVG_WELL_FORMED, Some("innocent.png")),
            Err(FileValidationError::BlockedMimeType { .. })
        ));
    }

    #[test]
    fn validate_file_rejects_svg_with_bom_or_whitespace() {
        // `infer` may miss SVG when prefixed with a BOM or leading
        // whitespace. The looks_like_svg sniffer is the backstop.
        for buf in [SVG_WITH_BOM, SVG_LEADING_WHITESPACE] {
            assert!(matches!(
                FileValidator::validate_file(buf, Some("notes.txt")),
                Err(FileValidationError::BlockedMimeType { .. })
            ));
        }
    }

    #[test]
    fn validate_file_rejects_bare_svg_tag() {
        assert!(matches!(
            FileValidator::validate_file(SVG_BARE_TAG, Some("vector.png")),
            Err(FileValidationError::BlockedMimeType { .. })
        ));
    }

    #[test]
    fn validate_file_allows_non_svg_xml() {
        // Generic XML that isn't SVG must not be caught by the sniffer.
        let xml = b"<?xml version=\"1.0\"?><note><body>hi</body></note>";
        assert!(FileValidator::validate_file(xml, Some("note.xml")).is_ok());
    }

    #[test]
    fn guest_upload_rejects_svg_renamed_to_txt() {
        // Guest path: SVG bytes saved as `.txt` would slip past the
        // extension allowlist and fall into the text/log fallback. The
        // sniffer catches it.
        assert!(matches!(
            FileValidator::validate_guest_upload(SVG_WELL_FORMED, "notes.txt"),
            Err(FileValidationError::BlockedMimeType { .. })
        ));
    }
}
