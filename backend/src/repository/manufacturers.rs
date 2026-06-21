//! Manufacturers repository (asset model catalog).
//!
//! A `manufacturer` is a make (Apple, Dell). Workspace config like
//! `asset_kinds`: not a sync aggregate, pickers re-fetch on demand, and
//! audit coverage lives in the `tr_audit_manufacturers` trigger.

use chrono::Utc;
use diesel::prelude::*;

use crate::db::DbConnection;
use crate::models::{Manufacturer, ManufacturerChange, NewManufacturer};
use crate::schema::manufacturers;

pub fn list(conn: &mut DbConnection) -> QueryResult<Vec<Manufacturer>> {
    manufacturers::table
        .order(manufacturers::name.asc())
        .load(conn)
}

pub fn get(conn: &mut DbConnection, id: i32) -> QueryResult<Manufacturer> {
    manufacturers::table.find(id).first(conn)
}

// sync-audit-only: manufacturer catalog is workspace config, not a sync aggregate; coverage lives in tr_audit_manufacturers
pub fn create(conn: &mut DbConnection, new: NewManufacturer) -> QueryResult<Manufacturer> {
    diesel::insert_into(manufacturers::table)
        .values(&new)
        .get_result(conn)
}

// sync-audit-only: manufacturer catalog is workspace config, see create
pub fn update(
    conn: &mut DbConnection,
    id: i32,
    mut change: ManufacturerChange,
) -> QueryResult<Manufacturer> {
    change.updated_at = Some(Utc::now());
    diesel::update(manufacturers::table.find(id))
        .set(&change)
        .get_result(conn)
}

// sync-audit-only: manufacturer catalog is workspace config, see create
pub fn delete(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(manufacturers::table.find(id)).execute(conn)
}

/// Count models referencing this manufacturer. The FK is RESTRICT, so
/// the admin UI warns before a delete that the DB would reject.
pub fn count_models(conn: &mut DbConnection, manufacturer_id: i32) -> QueryResult<i64> {
    use crate::schema::asset_models;
    asset_models::table
        .filter(asset_models::manufacturer_id.eq(manufacturer_id))
        .count()
        .get_result(conn)
}
