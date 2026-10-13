//! Asset media endpoints.
//!
//! Asset photos and documents (service reports, invoices) live outside the
//! ticket-comment attachment model so authorization, storage paths, and sync
//! visibility can follow asset ownership directly instead of borrowing
//! ticket/comment semantics.

use std::sync::Arc;

use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use diesel::OptionalExtension;
use futures::{StreamExt, TryStreamExt};
use ring::digest;
use serde::Deserialize;
use serde_json::json;
use tracing::{debug, error, warn};

use crate::db::Pool;
use crate::errors::{self, ApiError};
use crate::extractors::{AuthContext, ScopedStorage, TenantConn};
use crate::handlers::files::{authorize_at_owning_workspace, serve_or_not_found};
use crate::models::{AssetMediaUpdate, NewAssetMedia};
use crate::repository::{asset_media as repo, assets as assets_repo};
use crate::utils::file_validation::{get_max_file_size, FileValidator};
use crate::utils::image::generate_asset_media_thumbnail;
use crate::utils::storage::{Caching, Storage, WorkspaceScopedStorage};

const ASSET_MEDIA_THUMB_SIZE: u32 = 320;
const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;
const MAX_DOCUMENT_SIZE: usize = 25 * 1024 * 1024;

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const PPTX: &str = "application/vnd.openxmlformats-officedocument.presentationml.presentation";
const ODT: &str = "application/vnd.oasis.opendocument.text";
const ODS: &str = "application/vnd.oasis.opendocument.spreadsheet";
const ODP: &str = "application/vnd.oasis.opendocument.presentation";
const ZIP: &str = "application/zip";
const OLE: &str = "application/x-ole-storage";
/// What `FileValidator::validate_file` reports when no signature matches.
const UNKNOWN: &str = "application/octet-stream";

/// Document types an asset accepts besides images: extension, the MIME it is
/// stored as, and the magic-byte MIMEs its content may sniff as. Office files
/// are ZIP or OLE containers, which the sniffer can report generically; text
/// formats have no signature at all.
const DOCUMENT_TYPES: &[(&str, &str, &[&str])] = &[
    ("pdf", "application/pdf", &["application/pdf"]),
    ("doc", "application/msword", &["application/msword", OLE]),
    ("docx", DOCX, &[DOCX, ZIP]),
    (
        "xls",
        "application/vnd.ms-excel",
        &["application/vnd.ms-excel", OLE],
    ),
    ("xlsx", XLSX, &[XLSX, ZIP]),
    (
        "ppt",
        "application/vnd.ms-powerpoint",
        &["application/vnd.ms-powerpoint", OLE],
    ),
    ("pptx", PPTX, &[PPTX, ZIP]),
    ("odt", ODT, &[ODT, ZIP]),
    ("ods", ODS, &[ODS, ZIP]),
    ("odp", ODP, &[ODP, ZIP]),
    ("rtf", "application/rtf", &["application/rtf"]),
    ("txt", "text/plain", &[UNKNOWN]),
    ("csv", "text/csv", &[UNKNOWN]),
];

/// The `asset_media.kind` an upload is stored as. Only photos get a thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MediaKind {
    Photo,
    Document,
}

impl MediaKind {
    fn as_str(self) -> &'static str {
        match self {
            MediaKind::Photo => "photo",
            MediaKind::Document => "document",
        }
    }
}

/// Classify an upload that already passed `FileValidator::validate_file`.
/// Images are photos whatever their name; anything else must carry a listed
/// document extension and sniff as that type. Returns the kind and the MIME
/// to store, or the refusal message.
pub(crate) fn classify_upload(
    bytes: &[u8],
    detected_mime: &str,
    filename: &str,
) -> Result<(MediaKind, String), &'static str> {
    const REFUSED: &str = "Only images, PDFs, office documents, and text files are allowed";
    if detected_mime.starts_with("image/") {
        if bytes.len() > MAX_IMAGE_SIZE {
            return Err("Image too large (max 10MB)");
        }
        return Ok((MediaKind::Photo, detected_mime.to_string()));
    }
    let ext = std::path::Path::new(filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or(REFUSED)?;
    let (_, stored_mime, sniffs_as) = DOCUMENT_TYPES
        .iter()
        .find(|(e, _, _)| *e == ext)
        .ok_or(REFUSED)?;
    if !sniffs_as.contains(&detected_mime) {
        return Err(REFUSED);
    }
    // No signature to check for text: refuse binary content wearing a .txt.
    if detected_mime == UNKNOWN && bytes.iter().take(8192).any(|b| *b == 0) {
        return Err(REFUSED);
    }
    Ok((MediaKind::Document, (*stored_mime).to_string()))
}

#[derive(Debug, Deserialize)]
pub struct UpdateAssetMediaBody {
    pub sort_order: Option<i32>,
    pub caption: Option<Option<String>>,
}

pub async fn list_for_asset(
    mut tc: TenantConn,
    _auth: AuthContext,
    path: web::Path<i32>,
) -> impl Responder {
    let asset_id = path.into_inner();
    match tc.run(|conn| {
        assets_repo::get_device_by_id(conn, asset_id)?;
        repo::list_for_asset(conn, asset_id)
    }) {
        Ok(rows) => HttpResponse::Ok().json(rows),
        Err(diesel::result::Error::NotFound) => {
            errors::not_found_msg(format!("Asset {asset_id} not found"))
        }
        Err(e) => {
            error!(asset_id, error = ?e, "failed to list asset media");
            errors::internal("Failed to load asset media")
        }
    }
}

pub async fn upload_for_asset(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<i32>,
    mut payload: Multipart,
    storage: ScopedStorage,
) -> Result<HttpResponse, actix_web::Error> {
    if !auth.can_handle_tickets() {
        return Err(ApiError::Forbidden(
            "Forbidden: Only technicians and administrators can upload asset media".into(),
        )
        .into());
    }

    let asset_id = path.into_inner();
    tc.run(|conn| assets_repo::get_device_by_id(conn, asset_id))
        .map_err(|e| match e {
            diesel::result::Error::NotFound => crate::errors::not_found_error("Asset not found"),
            other => {
                error!(asset_id, error = ?other, "failed to load asset before media upload");
                crate::errors::internal_error("Failed to load asset")
            }
        })?;

    let mut uploaded = Vec::new();

    while let Some(mut field) = payload.try_next().await? {
        let field_name = field.name();
        if field_name != "files" {
            debug!(field_name = %field_name, "skipping non-file asset media field");
            continue;
        }

        let original_filename = field
            .content_disposition()
            .get_filename()
            .ok_or_else(|| crate::errors::bad_request_error("Filename is required"))?;
        let sanitized_filename =
            FileValidator::sanitize_filename(original_filename).map_err(|e| {
                warn!(error = ?e, original_filename = %original_filename, "asset media filename sanitization failed");
                crate::errors::bad_request_error(format!("Invalid filename: {e}"))
            })?;

        let mut file_data = Vec::new();
        let mut total_size = 0usize;
        while let Some(chunk) = field.next().await {
            let data = chunk.map_err(|e| {
                error!(error = ?e, "error reading asset media chunk");
                crate::errors::internal_error("Error reading chunk")
            })?;
            // Documents up to 25MB, within the instance's MAX_FILE_SIZE_MB;
            // `classify_upload` holds images to 10MB.
            let max_size = MAX_DOCUMENT_SIZE.min(get_max_file_size());
            if total_size + data.len() > max_size {
                return Err(crate::errors::bad_request_error(format!(
                    "File too large (max {}MB)",
                    max_size / (1024 * 1024)
                )));
            }
            total_size += data.len();
            file_data.extend_from_slice(&data);
        }

        let detected_mime = FileValidator::validate_file(&file_data, Some(&sanitized_filename))
            .map_err(|e| {
                warn!(error = ?e, filename = %sanitized_filename, "asset media validation failed");
                crate::errors::bad_request_error(format!("Invalid file: {e}"))
            })?;
        let (kind, stored_mime) = classify_upload(&file_data, &detected_mime, &sanitized_filename)
            .map_err(crate::errors::bad_request_error)?;

        let checksum_bytes = digest::digest(&digest::SHA256, &file_data);
        let checksum = checksum_bytes
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();

        let folder = format!("assets/{asset_id}/media");
        let stored_file = storage
            .0
            .store_file(&file_data, &sanitized_filename, &stored_mime, &folder)
            .await
            .map_err(|e| {
                error!(asset_id, error = ?e, filename = %sanitized_filename, "failed to store asset media");
                crate::errors::internal_error("Failed to store file")
            })?;

        let url = format!("/api/files/assets/{asset_id}/media/{}", stored_file.id);
        let thumb_stem = stored_file
            .id
            .split_once('_')
            .map(|(stem, _)| stem)
            .unwrap_or(stored_file.id.as_str());
        let thumbnail = match kind {
            MediaKind::Photo => {
                generate_asset_media_thumbnail(&file_data, ASSET_MEDIA_THUMB_SIZE).await
            }
            MediaKind::Document => None,
        };
        let thumbnail_url = if let Some(webp) = thumbnail {
            let thumb_path = format!("assets/{asset_id}/media/thumb/{thumb_stem}.webp");
            match storage.0.put_file(&webp, &thumb_path, "image/webp").await {
                Ok(_) => Some(format!(
                    "/api/files/assets/{asset_id}/media/thumb/{thumb_stem}.webp"
                )),
                Err(e) => {
                    warn!(asset_id, error = ?e, thumb_path = %thumb_path, "failed to store asset media thumbnail");
                    None
                }
            }
        } else {
            None
        };
        let new_media = NewAssetMedia {
            asset_id,
            url,
            thumbnail_url,
            name: sanitized_filename,
            file_size: Some(total_size as i64),
            mime_type: Some(stored_mime),
            checksum: Some(checksum),
            kind: kind.as_str().to_string(),
            sort_order: 0,
            caption: None,
            uploaded_by: Some(auth.user_uuid),
        };

        let row = tc.run(|conn| repo::create(conn, new_media)).map_err(|e| {
            error!(asset_id, error = ?e, "failed to create asset media row");
            crate::errors::internal_error("Failed to create asset media")
        })?;
        uploaded.push(row);
    }

    Ok(HttpResponse::Created().json(uploaded))
}

pub async fn update_media(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
    body: web::Json<UpdateAssetMediaBody>,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can update asset media",
        );
    }
    let (asset_id, media_id) = path.into_inner();
    let body = body.into_inner();
    // An all-absent body would compile a column-less UPDATE, which
    // Diesel rejects at runtime ("There are no changes to save"). A
    // `caption: null` is still a change (clear it), so only guard the
    // case where neither field was supplied.
    if body.sort_order.is_none() && body.caption.is_none() {
        return errors::bad_request("No asset media fields to update");
    }
    let update = AssetMediaUpdate {
        sort_order: body.sort_order,
        caption: body.caption,
    };
    match tc.run(|conn| {
        let row = repo::get_by_id(conn, media_id)?;
        if row.asset_id != asset_id {
            return Err(diesel::result::Error::NotFound);
        }
        repo::update(conn, media_id, update)
    }) {
        Ok(row) => HttpResponse::Ok().json(row),
        Err(diesel::result::Error::NotFound) => {
            errors::not_found_msg(format!("Asset media {media_id} not found"))
        }
        Err(e) => {
            error!(asset_id, media_id, error = ?e, "failed to update asset media");
            errors::internal("Failed to update asset media")
        }
    }
}

pub async fn delete_media(
    mut tc: TenantConn,
    auth: AuthContext,
    path: web::Path<(i32, i32)>,
    storage: ScopedStorage,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden(
            "Forbidden: Only technicians and administrators can delete asset media",
        );
    }
    let (asset_id, media_id) = path.into_inner();
    let deleted = match tc.run(|conn| {
        let row = repo::get_by_id(conn, media_id)?;
        if row.asset_id != asset_id {
            return Err(diesel::result::Error::NotFound);
        }
        repo::delete(conn, media_id)
    }) {
        Ok(Some(row)) => row,
        Ok(None) | Err(diesel::result::Error::NotFound) => {
            return errors::not_found_msg(format!("Asset media {media_id} not found"));
        }
        Err(e) => {
            error!(asset_id, media_id, error = ?e, "failed to delete asset media");
            return errors::internal("Failed to delete asset media");
        }
    };

    if let Some(filename) = deleted.url.rsplit('/').next() {
        let path = format!("assets/{}/media/{filename}", deleted.asset_id);
        if let Err(e) = storage.0.delete_file(&path).await {
            warn!(asset_id, media_id, path = %path, error = ?e, "failed to delete asset media object");
        }
    }
    if let Some(thumb_url) = deleted.thumbnail_url.as_deref() {
        let prefix = format!("/api/files/assets/{}/media/", deleted.asset_id);
        if let Some(tail) = thumb_url.strip_prefix(&prefix) {
            let path = format!("assets/{}/media/{tail}", deleted.asset_id);
            if let Err(e) = storage.0.delete_file(&path).await {
                warn!(asset_id, media_id, path = %path, error = ?e, "failed to delete asset media thumbnail");
            }
        }
    }

    HttpResponse::Ok().json(json!({ "deleted": true }))
}

/// `GET /api/files/assets/{asset_id}/media/{filename}`. An `<img>` loads it
/// without the workspace selection header, so the workspace comes from the
/// asset (see `authorize_at_owning_workspace`, which also admits only agent-app
/// members). Any such member may see an asset's media, as with the media list.
pub async fn serve_asset_media_file(
    path: web::Path<(i32, String)>,
    req: HttpRequest,
    pool: web::Data<Pool>,
    auth: AuthContext,
    base_storage: web::Data<Arc<dyn Storage>>,
) -> Result<HttpResponse, actix_web::Error> {
    let (asset_id, filename) = path.into_inner();
    let (workspace_id, ()) = authorize_at_owning_workspace(
        &pool,
        &auth,
        |c| assets_repo::asset_workspace_id(c, asset_id),
        |c, _, _| {
            // Read again under the pin: the asset must be in this workspace.
            Ok(assets_repo::get_device_by_id(c, asset_id)
                .optional()?
                .map(|_| ()))
        },
    )?;
    let storage = WorkspaceScopedStorage::arc(base_storage.get_ref().clone(), workspace_id);

    let file_path = format!("assets/{asset_id}/media/{filename}");
    serve_or_not_found(storage, &file_path, &req, Caching::Private).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_are_photos_whatever_their_name() {
        let (kind, mime) = classify_upload(b"png", "image/png", "scan.pdf").unwrap();
        assert_eq!(kind, MediaKind::Photo);
        assert_eq!(mime, "image/png");
    }

    #[test]
    fn oversized_images_are_refused() {
        let big = vec![0u8; MAX_IMAGE_SIZE + 1];
        assert!(classify_upload(&big, "image/jpeg", "photo.jpg").is_err());
    }

    #[test]
    fn documents_must_sniff_as_their_extension() {
        let (kind, mime) =
            classify_upload(b"%PDF-1.7", "application/pdf", "Service-Report.PDF").unwrap();
        assert_eq!(kind, MediaKind::Document);
        assert_eq!(mime, "application/pdf");

        // A generic ZIP sniff is fine for OOXML, which is a ZIP container.
        let (kind, mime) = classify_upload(b"PK", ZIP, "invoice.docx").unwrap();
        assert_eq!(kind, MediaKind::Document);
        assert_eq!(mime, DOCX);

        // A PDF renamed .docx, or a ZIP named .pdf, is refused.
        assert!(classify_upload(b"%PDF", "application/pdf", "invoice.docx").is_err());
        assert!(classify_upload(b"PK", ZIP, "invoice.pdf").is_err());
    }

    #[test]
    fn text_files_need_text_content() {
        let (kind, mime) = classify_upload(b"serial,date\n1,2", UNKNOWN, "log.csv").unwrap();
        assert_eq!(kind, MediaKind::Document);
        assert_eq!(mime, "text/csv");
        assert!(classify_upload(b"\x7fELF\0\0", UNKNOWN, "notes.txt").is_err());
    }

    #[test]
    fn unlisted_types_are_refused() {
        assert!(classify_upload(b"PK", ZIP, "bundle.zip").is_err());
        assert!(classify_upload(b"<html>", UNKNOWN, "page.html").is_err());
        assert!(classify_upload(b"data", UNKNOWN, "no-extension").is_err());
        assert!(classify_upload(b"ID3", "audio/mpeg", "song.mp3").is_err());
    }
}
