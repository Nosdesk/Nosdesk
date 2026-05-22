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

/// Outcome of a successful ledger event. Carries the inserted
/// row plus enough post-write context for the handler to decide
/// whether to emit an `asset.low_stock` SSE event without having
/// to re-query the asset.
#[derive(Debug, Clone)]
pub struct RecordUsageOutcome {
    pub row: AssetUsage,
    pub asset_name: String,
    pub new_quantity: BigDecimal,
    pub threshold: Option<BigDecimal>,
    /// True iff this was a `usage` event, the asset has a
    /// configured `low_stock_threshold`, and the decrement
    /// edge-crossed it (pre-write quantity above the threshold,
    /// post-write quantity at or below). Restock events never
    /// cross downward into the alert zone.
    pub crossed_low_stock: bool,
}

/// Insert a ledger row and adjust the asset's on-hand quantity
/// in the same transaction. Direction is taken from
/// `new_usage.event_kind`: `"usage"` decrements, `"restock"`
/// increments. Returns the inserted row plus low-stock crossing
/// telemetry (only meaningful for the usage path).
///
/// Caller is responsible for verifying that `assets.quantity IS
/// NOT NULL` before calling; this fn assumes the row is stock-
/// tracked. The usage decrement is allowed to drive `quantity`
/// below zero (we don't refuse on overdraw, the admin needs to
/// see the negative to know they have an inventory discrepancy).
// sync-audit-only: ledger rows are append-only events, not entities with identity, so they ride the SseEvent::AssetUsageRecorded channel from the handler (see handlers::asset_usage::record) rather than the sync_actions stream that's for cache-pool-tracked aggregates.
pub fn record_event(
    conn: &mut DbConnection,
    new_usage: NewAssetUsage,
) -> QueryResult<RecordUsageOutcome> {
    let magnitude = new_usage.quantity_used.clone();
    let asset_id = new_usage.asset_id;
    let is_usage = new_usage.event_kind == "usage";
    conn.transaction::<RecordUsageOutcome, Error, _>(|conn| {
        // Snapshot the pre-write quantity + name + threshold
        // inside the transaction so the crossing check sees a
        // consistent view of the row. Locking unnecessary at
        // READ COMMITTED here because Postgres serialises the
        // subsequent UPDATE through row-level locks anyway.
        let (name, pre_qty, threshold): (String, Option<BigDecimal>, Option<BigDecimal>) =
            assets::table
                .find(asset_id)
                .select((assets::name, assets::quantity, assets::low_stock_threshold))
                .first(conn)?;
        // Caller contract: only stock-tracked assets. We still
        // tolerate the NULL case as a defensive default of zero
        // for the crossing calc rather than panicking.
        let pre = pre_qty.unwrap_or_else(|| BigDecimal::from(0));
        let new_qty = if is_usage {
            &pre - &magnitude
        } else {
            &pre + &magnitude
        };

        let row: AssetUsage = diesel::insert_into(asset_usage_log::table)
            .values(&new_usage)
            .get_result(conn)?;
        diesel::update(assets::table.find(asset_id))
            .set(assets::quantity.eq(&new_qty))
            .execute(conn)?;

        // Edge-detect crossing only on the usage path. Restock
        // never trips the low-stock alert downward; a "stock
        // recovered" event might be useful in the future but
        // isn't on the wire yet.
        let crossed = if is_usage {
            match threshold.as_ref() {
                Some(t) => &pre > t && &new_qty <= t,
                None => false,
            }
        } else {
            false
        };

        Ok(RecordUsageOutcome {
            row,
            asset_name: name,
            new_quantity: new_qty,
            threshold,
            crossed_low_stock: crossed,
        })
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
