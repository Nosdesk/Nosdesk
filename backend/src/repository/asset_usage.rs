//! Asset usage ledger repository.
//!
//! Append-mostly table; reads dominate. The transactional
//! `record_usage` insert is the only spot that also mutates
//! `assets.quantity` (decrement) so the on-hand count stays
//! aligned with the ledger sum. If we ever need a "correction"
//! flow that reverses a previous row, it'll add an inverse-sign
//! row rather than UPDATE-ing the original; the audit_log
//! trigger on this table makes that the safer pattern.

use bigdecimal::BigDecimal;
use diesel::prelude::*;
use diesel::result::Error;

use crate::db::DbConnection;
use crate::models::{AssetUsage, NewAssetUsage};
use crate::schema::{asset_usage_log, assets};

/// Insert a usage row and decrement the asset's on-hand
/// quantity in the same transaction. Returns the inserted row.
///
/// Caller is responsible for verifying that `assets.quantity IS
/// NOT NULL` before calling; this fn assumes the row is stock-
/// tracked and unconditionally decrements. The decrement is
/// allowed to drive `quantity` below zero (we don't refuse a
/// usage event on overdraw — the admin needs to see the
/// negative to know they have an inventory discrepancy).
// sync-pending-wire: emit fires inside the handler so the sync_action carries the joined ticket + asset display fields the frontend needs; see handlers::assets::record_usage
pub fn record_usage(conn: &mut DbConnection, new_usage: NewAssetUsage) -> QueryResult<AssetUsage> {
    let qty = new_usage.quantity_used.clone();
    let asset_id = new_usage.asset_id;
    conn.transaction::<AssetUsage, Error, _>(|conn| {
        let row: AssetUsage = diesel::insert_into(asset_usage_log::table)
            .values(&new_usage)
            .get_result(conn)?;
        diesel::update(assets::table.find(asset_id))
            .set(assets::quantity.eq(assets::quantity - qty))
            .execute(conn)?;
        Ok(row)
    })
}

/// Usage history for a single asset, newest first. Used by the
/// asset detail "Usage history" panel.
// sync-audit-only: read-only ledger lookup
pub fn list_for_asset(
    conn: &mut DbConnection,
    asset_id: i32,
    limit: i64,
    offset: i64,
) -> QueryResult<Vec<AssetUsage>> {
    asset_usage_log::table
        .filter(asset_usage_log::asset_id.eq(asset_id))
        .order(asset_usage_log::recorded_at.desc())
        .limit(limit)
        .offset(offset)
        .load(conn)
}

/// Usage rows tied to a specific ticket, newest first. Used by
/// the ticket detail "Asset usage" section.
// sync-audit-only: read-only ledger lookup
pub fn list_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<AssetUsage>> {
    asset_usage_log::table
        .filter(asset_usage_log::ticket_id.eq(ticket_id))
        .order(asset_usage_log::recorded_at.desc())
        .load(conn)
}

/// Convenience: read the current on-hand quantity for an asset.
/// Returns `Ok(None)` for assets that don't track stock (i.e.
/// `assets.quantity IS NULL`), which the handler uses to refuse
/// usage writes.
// sync-audit-only: read-only quantity lookup
pub fn quantity_for_asset(
    conn: &mut DbConnection,
    asset_id: i32,
) -> QueryResult<Option<BigDecimal>> {
    assets::table
        .find(asset_id)
        .select(assets::quantity)
        .first::<Option<BigDecimal>>(conn)
}
