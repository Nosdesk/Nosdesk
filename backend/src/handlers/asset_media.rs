//! Asset media endpoints.
//!
//! Asset photos live outside the ticket-comment attachment model so
//! authorization, storage paths, and sync visibility can follow asset
//! ownership directly instead of borrowing ticket/comment semantics.

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
use crate::utils::file_validation::FileValidator;
use crate::utils::image::generate_asset_media_thumbnail;
use crate::utils::storage::{Caching, Storage, WorkspaceScopedStorage};

const ASSET_MEDIA_THUMB_SIZE: u32 = 320;

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
            const MAX_IMAGE_SIZE: usize = 10 * 1024 * 1024;
            if total_size + data.len() > MAX_IMAGE_SIZE {
                return Err(crate::errors::bad_request_error(
                    "File too large (max 10MB)",
                ));
            }
            total_size += data.len();
            file_data.extend_from_slice(&data);
        }

        let detected_mime = FileValidator::validate_file(&file_data, Some(&sanitized_filename))
            .map_err(|e| {
                warn!(error = ?e, filename = %sanitized_filename, "asset media validation failed");
                crate::errors::bad_request_error(format!("Invalid file: {e}"))
            })?;
        if !detected_mime.starts_with("image/") {
            return Err(crate::errors::bad_request_error(
                "Only image files are allowed",
            ));
        }

        let checksum_bytes = digest::digest(&digest::SHA256, &file_data);
        let checksum = checksum_bytes
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();

        let folder = format!("assets/{asset_id}/media");
        let stored_file = storage
            .0
            .store_file(&file_data, &sanitized_filename, &detected_mime, &folder)
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
        let thumbnail_url = if let Some(webp) =
            generate_asset_media_thumbnail(&file_data, ASSET_MEDIA_THUMB_SIZE).await
        {
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
            mime_type: Some(detected_mime),
            checksum: Some(checksum),
            kind: "photo".to_string(),
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
