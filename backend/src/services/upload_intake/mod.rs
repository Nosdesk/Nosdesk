//! One intake for uploaded files: limits, validation and hashing in one
//! place, with the file streamed to local disk instead of held in memory.
//!
//! Each upload path declares an [`UploadPolicy`] (per-file cap, file count,
//! which types it takes, where it stores) and reads its request through
//! [`multipart`], or [`stream`] for bytes that don't arrive as multipart. A
//! file comes back as an [`Intaken`]: spooled to a temp file, with its
//! names, the type sniffed from its first 8 KiB, its SHA-256 and its size.
//! [`Intaken::store`] copies it to storage from disk.
//!
//! What the intake enforces:
//!
//! - **The cap**, before each chunk is written, so a too-large file stops at
//!   the first chunk past it. A policy with a file count also refuses a
//!   request whose `Content-Length` is over `max_files x max_bytes` plus room
//!   for the multipart framing, before any of the body is read.
//! - **The type**, from the head buffer once it is full (or the file ends),
//!   so a refused file is never read back or spooled past its first chunks.
//! - **The spool budget**: a process-wide count of spooled bytes,
//!   `NOSDESK_SPOOL_BUDGET_MB`. When it is spent the upload is refused with
//!   503 and `Retry-After`, so concurrent uploads can't fill the disk. A
//!   request's files are taken one at a time, and each gives its share back
//!   when it drops, so a reply with several files needs only the largest.
//! - **Cleanup**: the temp file is removed when the [`Intaken`] drops, on
//!   success, refusal or a broken stream. A killed process skips drop, so
//!   [`SpoolArea::sweep`] removes spool files an hour old, at boot and hourly.
//!
//! The spool directory is `NOSDESK_SPOOL_DIR`, default
//! `<OS temp>/nosdesk-spool`. The archive imports use [`spool`] directly,
//! under their own caps, outside the budget and in the OS temp dir: a 1 GB
//! workspace import is not an attachment. Backup restore does too, beside
//! the file's final name under `NOSDESK_MAX_IMPORT_BYTES`.

pub mod spool;

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, SystemTime};

use actix_multipart::{Field, Multipart, MultipartError};
use bytes::{Bytes, BytesMut};
use futures::{Stream, StreamExt, TryStreamExt};
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tracing::{error, info, warn};

use crate::utils::file_validation::{
    get_max_file_size, FileValidationError, FileValidator, GUEST_MAX_FILE_SIZE_MB,
};
use crate::utils::storage::{Storage, StorageError, StoredFile};

use spool::Spooled;

const MIB: u64 = 1024 * 1024;

/// How much of each file is kept in memory for sniffing its type. `infer`
/// reads no more than this from a file itself.
pub const HEAD_BYTES: usize = 8 * 1024;

/// Room over the files themselves for multipart boundaries, part headers
/// and small text fields, in the `Content-Length` check.
const ENVELOPE_BYTES: u64 = MIB;

/// The largest text field a policy takes (a voice note's transcription).
const MAX_TEXT_FIELD_BYTES: usize = 64 * 1024;

/// The prefix of every spool file, so the sweep removes only its own files
/// even when `NOSDESK_SPOOL_DIR` is shared.
const SPOOL_PREFIX: &str = "nosdesk-upload-";

/// How old a spool file is when the sweep removes it. A file still being
/// written has a fresh modification time.
pub const SPOOL_MAX_AGE: Duration = Duration::from_secs(60 * 60);

/// Which files a policy takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UploadTypes {
    /// Anything but the blocked executable and script types
    /// ([`FileValidator::validate_file`]).
    Blocklist,
    /// The public form's short list ([`FileValidator::validate_guest_upload`]).
    GuestAllowlist,
    /// Images only, after the blocklist.
    Images,
    /// Images of the listed types only, after the blocklist: a path that
    /// decodes the image and takes only what its decoder reads.
    ImageOf(&'static [&'static str]),
    /// A zip archive.
    Zip,
    /// Any type: the path checks the content itself, or keeps what the
    /// sender declared. The type is sniffed, else `application/octet-stream`.
    Any,
}

/// What one upload path accepts.
#[derive(Debug, Clone)]
pub struct UploadPolicy {
    /// One file's cap, in bytes.
    pub max_bytes: u64,
    /// Files a request may carry. `None` takes any number, and the request
    /// has no `Content-Length` check of its own.
    pub max_files: Option<usize>,
    pub types: UploadTypes,
    /// Where [`Intaken::store`] puts a file, as a storage folder.
    pub folder: String,
    /// The multipart field files arrive in.
    pub file_field: &'static str,
    /// Text fields the path reads, each up to 64 KiB. Other fields are
    /// skipped.
    pub text_fields: &'static [&'static str],
}

impl UploadPolicy {
    /// Reply and note attachments (`/upload`): any type but the blocked ones,
    /// up to `MAX_FILE_SIZE_MB` each, staged in `temp/` until a reply claims
    /// them. No file count: the composer sends a reply's files together.
    pub fn attachments() -> Self {
        Self {
            max_bytes: get_max_file_size() as u64,
            max_files: None,
            types: UploadTypes::Blocklist,
            folder: "temp".to_string(),
            file_field: "files",
            text_fields: &["transcription"],
        }
    }

    /// Images in a ticket's notes: 10 MB each, up to 10 a request, stored
    /// beside the ticket's files.
    pub fn ticket_note_images(ticket_id: i32) -> Self {
        Self {
            max_bytes: 10 * MIB,
            max_files: Some(10),
            types: UploadTypes::Images,
            folder: format!("tickets/{ticket_id}/notes"),
            file_field: "files",
            text_fields: &[],
        }
    }

    /// Images pasted or dropped into a collaborative document: 10 MB each,
    /// up to 10 a request, stored in the document's folder.
    pub fn document_images(folder: String) -> Self {
        Self {
            max_bytes: 10 * MIB,
            max_files: Some(10),
            types: UploadTypes::Images,
            folder,
            file_field: "files",
            text_fields: &[],
        }
    }

    /// An asset's photos and documents: up to 25 MB each, within
    /// `MAX_FILE_SIZE_MB` (the handler holds images to 10 MB and checks the
    /// document list), any number a request.
    pub fn asset_media(asset_id: i32) -> Self {
        Self {
            max_bytes: (25 * MIB).min(get_max_file_size() as u64),
            max_files: None,
            types: UploadTypes::Blocklist,
            folder: format!("assets/{asset_id}/media"),
            file_field: "files",
            text_fields: &[],
        }
    }

    /// A profile photo or banner: one image of a type the decoder reads, up
    /// to 10 MB. It is decoded and re-encoded, so it never stores as sent.
    pub fn profile_image() -> Self {
        Self {
            max_bytes: 10 * MIB,
            max_files: Some(1),
            types: UploadTypes::ImageOf(&["image/jpeg", "image/png", "image/gif", "image/webp"]),
            folder: String::new(),
            file_field: "file",
            text_fields: &[],
        }
    }

    /// A workspace logo or favicon: one image, up to 2 MB, stored under a
    /// fixed key by the handler.
    pub fn branding_image(types: &'static [&'static str]) -> Self {
        Self {
            max_bytes: 2 * MIB,
            max_files: Some(1),
            types: UploadTypes::ImageOf(types),
            folder: String::new(),
            file_field: "file",
            text_fields: &[],
        }
    }

    /// The public request form and the customer portal: one file a request,
    /// from the guest allowlist, up to `GUEST_MAX_FILE_SIZE_MB`, staged in
    /// `temp/` until a submission claims it.
    pub fn guest() -> Self {
        Self {
            max_bytes: (GUEST_MAX_FILE_SIZE_MB as u64) * MIB,
            max_files: Some(1),
            types: UploadTypes::GuestAllowlist,
            folder: "temp".to_string(),
            file_field: "file",
            text_fields: &[],
        }
    }

    /// A spreadsheet import: one file up to 10 MB, read whole by its parser,
    /// which decides whether it is a workbook or CSV.
    pub fn spreadsheet_import() -> Self {
        Self {
            max_bytes: 10 * MIB,
            max_files: Some(1),
            types: UploadTypes::Any,
            folder: String::new(),
            file_field: "file",
            text_fields: &[],
        }
    }

    /// An attachment that arrived on an inbound channel (email now, chat
    /// connectors later): up to 25 MiB, any type, filed under its ticket.
    pub fn inbound_attachment(ticket_id: i32) -> Self {
        Self {
            max_bytes: 25 * MIB,
            max_files: None,
            types: UploadTypes::Any,
            folder: format!("tickets/{ticket_id}"),
            file_field: "files",
            text_fields: &[],
        }
    }

    /// The largest request body the policy can need, when it has a file
    /// count.
    pub fn max_request_bytes(&self) -> Option<u64> {
        self.max_files
            .map(|n| (n as u64).saturating_mul(self.max_bytes) + ENVELOPE_BYTES)
    }

    /// The type of a file from its head, or why it is refused.
    fn check_type(&self, head: &[u8], sanitized: &str) -> Result<String, Refusal> {
        match self.types {
            UploadTypes::Blocklist => Ok(FileValidator::validate_file(head, Some(sanitized))?),
            UploadTypes::GuestAllowlist => {
                // Its own size check sees only the head; the cap is ours.
                Ok(FileValidator::validate_guest_upload(head, sanitized)?)
            }
            UploadTypes::Images => {
                let mime = FileValidator::validate_file(head, Some(sanitized))?;
                if mime.starts_with("image/") {
                    Ok(mime)
                } else {
                    Err(Refusal::NotAnImage)
                }
            }
            UploadTypes::ImageOf(allowed) => {
                let mime = FileValidator::validate_file(head, Some(sanitized))?;
                if allowed.contains(&mime.as_str()) {
                    Ok(mime)
                } else if mime.starts_with("image/") {
                    Err(Refusal::UnsupportedImage { detected: mime })
                } else {
                    Err(Refusal::NotAnImage)
                }
            }
            UploadTypes::Any => Ok(infer::get(head)
                .map(|kind| kind.mime_type())
                .unwrap_or("application/octet-stream")
                .to_string()),
            UploadTypes::Zip => {
                let mime = FileValidator::validate_file(head, Some(sanitized))?;
                if head.starts_with(b"PK\x03\x04") {
                    Ok("application/zip".to_string())
                } else {
                    Err(Refusal::Validation(FileValidationError::BlockedMimeType {
                        detected: mime,
                    }))
                }
            }
        }
    }
}

/// Why one file was refused.
#[derive(Debug)]
pub enum Refusal {
    /// By name, size or type.
    Validation(FileValidationError),
    /// Not an image, on an images-only path.
    NotAnImage,
    /// An image of a type the path doesn't take.
    UnsupportedImage { detected: String },
}

impl From<FileValidationError> for Refusal {
    fn from(e: FileValidationError) -> Self {
        Self::Validation(e)
    }
}

/// Why an upload was refused or failed.
#[derive(Debug)]
pub enum IntakeError {
    /// The request's `Content-Length` is over the policy's request cap;
    /// nothing was read.
    RequestTooLarge { length: u64, max: u64 },
    /// More files than the policy takes.
    TooManyFiles { max: usize },
    /// A file field without a filename.
    MissingFilename,
    /// One file refused, named as the client sent it.
    Refused { file: String, reason: Refusal },
    /// A text field over its cap.
    TextTooLarge { field: String },
    /// The spool budget is spent; try again shortly.
    Busy,
    /// The multipart framing couldn't be parsed.
    Multipart(MultipartError),
    /// The client's stream failed part way (disconnect, reset).
    Unreadable(String),
    /// The spool file couldn't be written.
    Io(io::Error),
}

impl std::fmt::Display for IntakeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestTooLarge { length, max } => {
                write!(f, "upload of {length} bytes is over {max}")
            }
            Self::TooManyFiles { max } => write!(f, "more than {max} files"),
            Self::MissingFilename => write!(f, "a file has no filename"),
            Self::Refused { reason, .. } => match reason {
                Refusal::Validation(e) => write!(f, "{e}"),
                Refusal::NotAnImage => write!(f, "not an image"),
                Refusal::UnsupportedImage { detected } => {
                    write!(f, "image type {detected} not taken")
                }
            },
            Self::TextTooLarge { field } => write!(f, "{field} is over its cap"),
            Self::Busy => write!(f, "spool budget spent"),
            Self::Multipart(e) => write!(f, "{e}"),
            Self::Unreadable(e) => write!(f, "upload stream failed: {e}"),
            Self::Io(e) => write!(f, "spool write failed: {e}"),
        }
    }
}

impl std::error::Error for IntakeError {}

impl From<io::Error> for IntakeError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<MultipartError> for IntakeError {
    fn from(e: MultipartError) -> Self {
        Self::Multipart(e)
    }
}

/// The response for each refusal, the ones `/upload` gave before the intake.
impl From<IntakeError> for actix_web::Error {
    fn from(e: IntakeError) -> Self {
        match e {
            IntakeError::RequestTooLarge { length, max } => FileValidationError::FileTooLarge {
                size: usize::try_from(length).unwrap_or(usize::MAX),
                max_size: usize::try_from(max).unwrap_or(usize::MAX),
            }
            .refusal(""),
            IntakeError::TooManyFiles { max } => crate::errors::from_response(
                "too many files",
                crate::errors::bad_request_with_code(
                    format!("Too many files (max {max})"),
                    "TOO_MANY_FILES",
                ),
            ),
            IntakeError::MissingFilename => {
                crate::errors::bad_request_error("Filename is required")
            }
            IntakeError::Refused { file, reason } => match reason {
                Refusal::Validation(e) => e.refusal(&file),
                Refusal::NotAnImage | Refusal::UnsupportedImage { .. } => {
                    crate::errors::from_response(
                        "not an image",
                        crate::errors::bad_request_with_code(
                            "Only image files are allowed",
                            "FILE_TYPE_NOT_ALLOWED",
                        ),
                    )
                }
            },
            IntakeError::TextTooLarge { field } => {
                let mut name = field.chars();
                let name = name
                    .next()
                    .map(|c| c.to_uppercase().chain(name).collect::<String>())
                    .unwrap_or_default();
                crate::errors::bad_request_error(format!(
                    "{name} too large (max {}KB)",
                    MAX_TEXT_FIELD_BYTES / 1024
                ))
            }
            IntakeError::Busy => crate::errors::from_response(
                "upload spool full",
                crate::errors::service_unavailable(
                    "The server is busy with other uploads. Try again shortly.",
                ),
            ),
            IntakeError::Multipart(e) => e.into(),
            IntakeError::Unreadable(e) => {
                error!(error = %e, "Error reading upload");
                crate::errors::internal_error("Error reading chunk")
            }
            IntakeError::Io(e) => {
                error!(error = %e, "Failed to spool upload");
                crate::errors::internal_error("Failed to store file")
            }
        }
    }
}

impl IntakeError {
    /// The refusal's response, for a handler that answers with an
    /// `HttpResponse` rather than an error.
    pub fn into_response(self) -> actix_web::HttpResponse {
        actix_web::Error::from(self).error_response()
    }
}

/// One file taken in: on local disk until it drops.
#[derive(Debug)]
pub struct Intaken {
    spooled: Spooled,
    /// This file's share of the spool budget, given back on drop.
    _budget: Option<OwnedSemaphorePermit>,
    head: Bytes,
    /// The name as the client sent it, for refusals.
    pub client_name: String,
    /// The name as uploaded, for display and download.
    pub display_name: String,
    /// The name made safe for a storage key.
    pub sanitized_name: String,
    /// The type sniffed from the head.
    pub mime: String,
    /// SHA-256 of the whole file, lowercase hex.
    pub sha256: String,
    pub size: u64,
}

impl Intaken {
    /// The spooled file.
    pub fn path(&self) -> &Path {
        self.spooled.path()
    }

    /// The first [`HEAD_BYTES`] of the file (all of it when smaller).
    pub fn head(&self) -> &[u8] {
        &self.head
    }

    /// Store the file under `folder` as `{uuid7}_{sanitized name}`, streamed
    /// from disk.
    pub async fn store(
        &self,
        storage: &dyn Storage,
        folder: &str,
    ) -> Result<StoredFile, StorageError> {
        storage
            .store_file_from_path(self.path(), &self.sanitized_name, &self.mime, folder)
            .await
    }
}

/// A part of a multipart upload, in the order it arrived.
#[derive(Debug)]
pub enum Part {
    File(Intaken),
    Text { name: String, value: String },
}

/// The spool directory and its budget.
#[derive(Debug)]
pub struct SpoolArea {
    dir: PathBuf,
    budget: Arc<Semaphore>,
}

static GLOBAL: OnceLock<SpoolArea> = OnceLock::new();

/// `NOSDESK_SPOOL_DIR`, or `<OS temp>/nosdesk-spool`. Shared with `Config`.
pub fn spool_dir_from(raw: Option<&str>) -> PathBuf {
    raw.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("nosdesk-spool"))
}

/// `NOSDESK_SPOOL_BUDGET_MB`, default 4 x `MAX_FILE_SIZE_MB`, never below
/// one file at the cap (or every such upload would answer 503). Shared with
/// `Config`.
pub fn spool_budget_mb_from(raw: Option<&str>, max_file_size_mb: usize) -> u64 {
    let floor = max_file_size_mb as u64;
    raw.and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(floor.saturating_mul(4))
        .max(floor)
}

impl SpoolArea {
    pub fn new(dir: PathBuf, budget_bytes: u64) -> Self {
        // A semaphore holds at most usize::MAX >> 3 permits.
        let permits = usize::try_from(budget_bytes)
            .unwrap_or(usize::MAX)
            .min(Semaphore::MAX_PERMITS);
        Self {
            dir,
            budget: Arc::new(Semaphore::new(permits)),
        }
    }

    /// Set the process's spool area from `Config` at boot. Before (or
    /// without) it, the defaults apply, read once.
    pub fn configure(dir: PathBuf, budget_mb: u64) -> &'static SpoolArea {
        let area = GLOBAL.get_or_init(|| Self::new(dir.clone(), budget_mb.saturating_mul(MIB)));
        if area.dir != dir {
            warn!("upload spool configured after first use; keeping the first");
        }
        area
    }

    /// The process's spool area.
    pub fn global() -> &'static SpoolArea {
        GLOBAL.get_or_init(|| {
            let max_mb = get_max_file_size() as u64 / MIB;
            Self::new(
                spool_dir_from(None),
                spool_budget_mb_from(None, max_mb as usize).saturating_mul(MIB),
            )
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn temp_file(&self) -> io::Result<NamedTempFile> {
        std::fs::create_dir_all(&self.dir)?;
        tempfile::Builder::new()
            .prefix(SPOOL_PREFIX)
            .tempfile_in(&self.dir)
    }

    /// Take `bytes` more of the budget into `held`.
    fn reserve(
        &self,
        held: &mut Option<OwnedSemaphorePermit>,
        bytes: usize,
    ) -> Result<(), IntakeError> {
        if bytes == 0 {
            return Ok(());
        }
        let n = u32::try_from(bytes).map_err(|_| IntakeError::Busy)?;
        let permit = self
            .budget
            .clone()
            .try_acquire_many_owned(n)
            .map_err(|_| IntakeError::Busy)?;
        match held {
            Some(h) => h.merge(permit),
            None => *held = Some(permit),
        }
        Ok(())
    }

    /// Read a multipart upload under `policy`. With a file count, a
    /// `content_length` over the request cap is refused before anything is
    /// read.
    pub fn multipart<'a>(
        &'a self,
        content_length: Option<u64>,
        payload: Multipart,
        policy: &'a UploadPolicy,
    ) -> Result<MultipartIntake<'a>, IntakeError> {
        check_length(content_length, policy)?;
        Ok(MultipartIntake {
            area: self,
            payload,
            policy,
            files: 0,
        })
    }

    /// Take in one file that arrives as a byte stream, named `name` by
    /// whoever sent it.
    pub async fn stream<S, E>(
        &self,
        stream: S,
        name: &str,
        policy: &UploadPolicy,
    ) -> Result<Intaken, IntakeError>
    where
        S: Stream<Item = Result<Bytes, E>> + Unpin,
        E: std::fmt::Display,
    {
        let refused = |reason: Refusal| IntakeError::Refused {
            file: name.to_string(),
            reason,
        };
        let sanitized_name =
            FileValidator::sanitize_filename(name).map_err(|e| refused(e.into()))?;
        let display_name =
            FileValidator::display_filename(name).unwrap_or_else(|| sanitized_name.clone());

        let file = self.temp_file()?;
        let mut out = tokio::fs::File::from_std(file.as_file().try_clone()?);
        let mut stream = stream;
        let mut budget = None;
        let mut head = BytesMut::with_capacity(HEAD_BYTES);
        let mut mime = None;
        let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
        let mut size: u64 = 0;
        // A type refused from the head reads on to the end (or the cap)
        // without writing, so a client still sending gets the refusal rather
        // than a reset connection, and an oversized file is still a 413.
        let mut wrong_type = None;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| IntakeError::Unreadable(e.to_string()))?;
            size += chunk.len() as u64;
            if size > policy.max_bytes {
                return Err(refused(
                    FileValidationError::FileTooLarge {
                        size: usize::try_from(size).unwrap_or(usize::MAX),
                        max_size: usize::try_from(policy.max_bytes).unwrap_or(usize::MAX),
                    }
                    .into(),
                ));
            }
            if wrong_type.is_some() {
                continue;
            }
            if head.len() < HEAD_BYTES {
                let take = (HEAD_BYTES - head.len()).min(chunk.len());
                head.extend_from_slice(&chunk[..take]);
                if head.len() == HEAD_BYTES {
                    match policy.check_type(&head, &sanitized_name) {
                        Ok(checked) => mime = Some(checked),
                        Err(reason) => {
                            wrong_type = Some(reason);
                            continue;
                        }
                    }
                }
            }
            self.reserve(&mut budget, chunk.len())?;
            hash.update(&chunk);
            out.write_all(&chunk).await?;
        }
        if let Some(reason) = wrong_type {
            return Err(refused(reason));
        }
        out.flush().await?;
        let mime = match mime {
            Some(mime) => mime,
            None => policy.check_type(&head, &sanitized_name).map_err(refused)?,
        };
        Ok(Intaken {
            spooled: Spooled::new(file, Some(name.to_string()), size),
            _budget: budget,
            head: head.freeze(),
            client_name: name.to_string(),
            display_name,
            sanitized_name,
            mime,
            sha256: hex::encode(hash.finish()),
            size,
        })
    }

    /// Remove spool files last written more than `max_age` ago: what a
    /// killed process left. Returns how many were removed.
    pub fn sweep(&self, max_age: Duration) -> io::Result<usize> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e),
        };
        let now = SystemTime::now();
        let mut removed = 0;
        for entry in entries.flatten() {
            if !entry
                .file_name()
                .to_string_lossy()
                .starts_with(SPOOL_PREFIX)
            {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let old = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .is_some_and(|age| age > max_age);
            if meta.is_file() && old && std::fs::remove_file(entry.path()).is_ok() {
                removed += 1;
            }
        }
        Ok(removed)
    }
}

/// A multipart upload being read, one part at a time.
pub struct MultipartIntake<'a> {
    area: &'a SpoolArea,
    payload: Multipart,
    policy: &'a UploadPolicy,
    files: usize,
}

impl MultipartIntake<'_> {
    /// The next file or text field the policy takes. A file is spooled whole
    /// before it is returned; drop it before asking for the next to give its
    /// share of the budget back.
    pub async fn next(&mut self) -> Result<Option<Part>, IntakeError> {
        while let Some(mut field) = self.payload.try_next().await? {
            let name = field.name().to_string();
            if name == self.policy.file_field {
                let filename = field
                    .content_disposition()
                    .get_filename()
                    .map(str::to_string)
                    .ok_or(IntakeError::MissingFilename)?;
                self.files += 1;
                if let Some(max) = self.policy.max_files {
                    if self.files > max {
                        return Err(IntakeError::TooManyFiles { max });
                    }
                }
                let file = self.area.stream(&mut field, &filename, self.policy).await;
                if let Err(IntakeError::Refused { reason, .. }) = &file {
                    warn!(error = ?reason, "Upload refused");
                }
                return file.map(|f| Some(Part::File(f)));
            }
            if self.policy.text_fields.contains(&name.as_str()) {
                let value = read_text(&mut field, &name).await?;
                return Ok(Some(Part::Text { name, value }));
            }
        }
        Ok(None)
    }
}

async fn read_text(field: &mut Field, name: &str) -> Result<String, IntakeError> {
    let mut text = Vec::new();
    while let Some(chunk) = field.next().await {
        let chunk = chunk.map_err(|e| IntakeError::Unreadable(e.to_string()))?;
        if text.len() + chunk.len() > MAX_TEXT_FIELD_BYTES {
            return Err(IntakeError::TextTooLarge {
                field: name.to_string(),
            });
        }
        text.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&text).into_owned())
}

/// [`SpoolArea::multipart`] in the process's spool area.
pub fn multipart<'a>(
    content_length: Option<u64>,
    payload: Multipart,
    policy: &'a UploadPolicy,
) -> Result<MultipartIntake<'a>, IntakeError> {
    SpoolArea::global().multipart(content_length, payload, policy)
}

/// [`SpoolArea::stream`] in the process's spool area.
pub async fn stream<S, E>(
    stream: S,
    name: &str,
    policy: &UploadPolicy,
) -> Result<Intaken, IntakeError>
where
    S: Stream<Item = Result<Bytes, E>> + Unpin,
    E: std::fmt::Display,
{
    SpoolArea::global().stream(stream, name, policy).await
}

/// Refuse a `Content-Length` over a counted policy's request cap, for a
/// path that reads its multipart fields itself and hands one to
/// [`SpoolArea::stream`].
pub fn check_length(content_length: Option<u64>, policy: &UploadPolicy) -> Result<(), IntakeError> {
    if let (Some(length), Some(max)) = (content_length, policy.max_request_bytes()) {
        if length > max {
            return Err(IntakeError::RequestTooLarge { length, max });
        }
    }
    Ok(())
}

/// The spool area a request's upload goes to: the app's own when it set
/// one (a test does, to watch its directory), else the process's.
pub fn area(req: &actix_web::HttpRequest) -> &SpoolArea {
    req.app_data::<actix_web::web::Data<SpoolArea>>()
        .map(|area| area.get_ref())
        .unwrap_or_else(|| SpoolArea::global())
}

/// A request's `Content-Length`, when it sent a readable one.
pub fn content_length(req: &actix_web::HttpRequest) -> Option<u64> {
    req.headers()
        .get(actix_web::http::header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
}

/// Sweep the process's spool area of files a killed process left. Logged,
/// never fatal: run at boot and hourly.
pub fn sweep_global() {
    let area = SpoolArea::global();
    match area.sweep(SPOOL_MAX_AGE) {
        Ok(0) => {}
        Ok(count) => info!(count, "Removed stale upload spool files"),
        Err(e) => warn!(error = %e, "Upload spool sweep failed"),
    }
}

#[cfg(test)]
mod tests;
