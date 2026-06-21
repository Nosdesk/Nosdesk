//! Asset models repository (the "device type" catalog).
//!
//! An `asset_model` is a real make+model that assets are stamped from.
//! Workspace config like `asset_kinds`: not a sync aggregate, pickers
//! re-fetch, audit coverage via the `tr_audit_asset_models` trigger.

use chrono::Utc;
use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{AssetModel, AssetModelChange, NewAssetModel};
use crate::schema::asset_models;

pub fn list(conn: &mut DbConnection) -> QueryResult<Vec<AssetModel>> {
    asset_models::table
        .order((
            asset_models::manufacturer_id.asc(),
            asset_models::name.asc(),
        ))
        .load(conn)
}

pub fn list_for_manufacturer(
    conn: &mut DbConnection,
    manufacturer_id: i32,
) -> QueryResult<Vec<AssetModel>> {
    asset_models::table
        .filter(asset_models::manufacturer_id.eq(manufacturer_id))
        .order(asset_models::name.asc())
        .load(conn)
}

pub fn get(conn: &mut DbConnection, id: i32) -> QueryResult<AssetModel> {
    asset_models::table.find(id).first(conn)
}

// sync-audit-only: model catalog is workspace config, not a sync aggregate; coverage lives in tr_audit_asset_models
pub fn create(conn: &mut DbConnection, new: NewAssetModel) -> QueryResult<AssetModel> {
    diesel::insert_into(asset_models::table)
        .values(&new)
        .get_result(conn)
}

// sync-audit-only: model catalog is workspace config, see create
pub fn update(
    conn: &mut DbConnection,
    id: i32,
    mut change: AssetModelChange,
) -> QueryResult<AssetModel> {
    change.updated_at = Some(Utc::now());
    diesel::update(asset_models::table.find(id))
        .set(&change)
        .get_result(conn)
}

// sync-audit-only: model catalog is workspace config, see create
pub fn delete(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(asset_models::table.find(id)).execute(conn)
}

/// Count assets currently stamped from this model. Drives the delete
/// guard (the FK is SET NULL, so a delete unlinks rather than blocks,
/// but the admin should still see the impact first).
pub fn count_assets(conn: &mut DbConnection, model_id: i32) -> QueryResult<i64> {
    use crate::schema::assets;
    assets::table
        .filter(assets::model_id.eq(model_id))
        .count()
        .get_result(conn)
}
