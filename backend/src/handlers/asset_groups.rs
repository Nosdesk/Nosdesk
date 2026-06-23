//! Native asset-group handlers.
//!
//! Group management (create / rename / recolor / archive) is admin-only,
//! mirroring tag and ticket-category governance. Reading the picker list and
//! assigning groups to an asset are agent-tier, mirroring how asset writes
//! (`create_device` / `update_device`) gate on `can_handle_tickets`.

use actix_web::{web, HttpResponse, Responder};
use serde::Deserialize;
use tracing::error;
use uuid::Uuid;

use crate::extractors::{AuthContext, TenantConn};
use crate::handlers::errors;
use crate::models::{AssetGroupUpdate, NewAssetGroup};
use crate::repository::asset_groups as repo;

#[derive(Debug, Deserialize)]
pub struct ListGroupsQuery {
    /// Include archived groups (the management view passes `true` to offer
    /// restore). Defaults to `false` so the picker stays clean.
    #[serde(default)]
    pub include_archived: bool,
}

pub async fn list_groups(
    mut tc: TenantConn,
    query: web::Query<ListGroupsQuery>,
    _auth: AuthContext,
) -> impl Responder {
    let include_archived = query.include_archived;
    match tc.run(|conn| repo::list_groups(conn, include_archived)) {
        Ok(groups) => HttpResponse::Ok().json(groups),
        Err(e) => {
            error!(error = %e, "list asset groups failed");
            errors::internal("Failed to list asset groups")
        }
    }
}

pub async fn create_group(
    mut tc: TenantConn,
    body: web::Json<NewAssetGroup>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can create asset groups");
    }
    let mut new_group = body.into_inner();
    new_group.created_by = Some(auth.user_uuid);
    match tc.run(|conn| {
        let group = repo::create_group(conn, new_group)?;
        repo::group_response(conn, group.id)
    }) {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(e) => {
            error!(error = %e, "create asset group failed");
            errors::internal("Failed to create asset group")
        }
    }
}

pub async fn update_group(
    mut tc: TenantConn,
    params: web::Path<i32>,
    body: web::Json<AssetGroupUpdate>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can update asset groups");
    }
    let id = params.into_inner();
    match tc.run(|conn| {
        repo::update_group(conn, id, body.into_inner())?;
        repo::group_response(conn, id)
    }) {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(diesel::result::Error::NotFound) => errors::not_found("Asset group"),
        Err(e) => {
            error!(error = %e, "update asset group failed");
            errors::internal("Failed to update asset group")
        }
    }
}

pub async fn archive_group(
    mut tc: TenantConn,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can archive asset groups");
    }
    let id = params.into_inner();
    match tc.run(|conn| {
        repo::archive_group(conn, id)?;
        repo::group_response(conn, id)
    }) {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(diesel::result::Error::NotFound) => errors::not_found("Asset group"),
        Err(e) => {
            error!(error = %e, "archive asset group failed");
            errors::internal("Failed to archive asset group")
        }
    }
}

pub async fn restore_group(
    mut tc: TenantConn,
    params: web::Path<i32>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.is_workspace_admin() {
        return errors::forbidden("Only admins can restore asset groups");
    }
    let id = params.into_inner();
    match tc.run(|conn| {
        repo::restore_group(conn, id)?;
        repo::group_response(conn, id)
    }) {
        Ok(resp) => HttpResponse::Ok().json(resp),
        Err(diesel::result::Error::NotFound) => errors::not_found("Asset group"),
        Err(e) => {
            error!(error = %e, "restore asset group failed");
            errors::internal("Failed to restore asset group")
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetAssetGroupsBody {
    /// Replace the asset's group set with this list. Empty array clears all.
    /// The repository computes the diff, so the API stays the same whether the
    /// caller is adding or removing.
    pub group_ids: Vec<i32>,
}

/// `PUT /api/assets/{id}/groups` — set an asset's native groups (from the
/// asset side). Agent-tier, mirroring asset write permissions. Returns the
/// resulting group refs so the caller can render them without reconstruction.
pub async fn set_asset_groups(
    mut tc: TenantConn,
    params: web::Path<i32>,
    body: web::Json<SetAssetGroupsBody>,
    auth: AuthContext,
) -> impl Responder {
    if !auth.can_handle_tickets() {
        return errors::forbidden("Forbidden: technicians and administrators only");
    }
    let asset_id = params.into_inner();
    let group_ids = body.group_ids.clone();
    let actor: Option<Uuid> = Some(auth.user_uuid);
    match tc.run(|conn| {
        repo::set_groups_for_asset(conn, asset_id, &group_ids, actor)?;
        Ok(repo::group_refs_for_assets(conn, &[asset_id])?
            .remove(&asset_id)
            .unwrap_or_default())
    }) {
        Ok(refs) => HttpResponse::Ok().json(refs),
        Err(e) => {
            error!(error = %e, asset_id, "set asset groups failed");
            errors::internal("Failed to update asset groups")
        }
    }
}
