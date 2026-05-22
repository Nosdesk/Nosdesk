//! Stock audit ledger repository.
//!
//! Append-only. Each audit row is the snapshot of a physical
//! count + the delta against what the system thought it had at
//! that moment. The matching `assets.quantity` correction
//! happens in the same transaction as the insert; there is no
//! "audit without correction" path because the whole point of
//! an audit is reconciling book and physical stock.

use bigdecimal::BigDecimal;
use diesel::prelude::*;
use diesel::result::Error;

use crate::db::DbConnection;
use crate::models::{AssetAudit, NewAssetAudit};
use crate::schema::{asset_audits, assets};

/// Outcome of a successful audit insert. Carries the inserted
/// row plus the post-correction quantity so the handler can
/// emit SSE events and run the low-stock crossing check
/// without re-querying.
#[derive(Debug, Clone)]
pub struct AuditOutcome {
    pub row: AssetAudit,
    pub asset_name: String,
    pub asset_unit: String,
    pub new_quantity: BigDecimal,
    pub threshold: Option<BigDecimal>,
    /// True iff the asset has a configured `low_stock_threshold`,
    /// the previous quantity was above it, and the post-audit
    /// quantity is at or below. Edge-detected so a sequence of
    /// audits below threshold doesn't re-fire the alert.
    pub crossed_low_stock: bool,
}

/// Record a physical-count audit. Reads the asset's current
/// quantity (the "previous"), inserts a row with the count, the
/// previous, and the signed delta, then sets
/// `assets.quantity = counted_quantity`. All in one transaction.
///
/// The asset must be stock-tracked (`quantity IS NOT NULL`).
/// Caller is responsible for enforcing that gate; this fn
/// returns NotFound if the row isn't there at all.
// sync-audit-only: append-only audit event, broadcast via SseEvent::AssetAuditRecorded from handlers::asset_audits::record rather than the sync_actions aggregate stream
pub fn record_audit(
    conn: &mut DbConnection,
    asset_id: i32,
    counted_quantity: BigDecimal,
    notes: Option<String>,
    recorded_by: Option<uuid::Uuid>,
) -> QueryResult<AuditOutcome> {
    conn.transaction::<AuditOutcome, Error, _>(|conn| {
        let (name, unit, previous_qty, threshold): (
            String,
            Option<String>,
            Option<BigDecimal>,
            Option<BigDecimal>,
        ) = assets::table
            .find(asset_id)
            .select((
                assets::name,
                assets::unit,
                assets::quantity,
                assets::low_stock_threshold,
            ))
            .first(conn)?;

        let previous = previous_qty.unwrap_or_else(|| BigDecimal::from(0));
        let delta = &counted_quantity - &previous;

        let new_audit = NewAssetAudit {
            asset_id,
            counted_quantity: counted_quantity.clone(),
            previous_quantity: previous.clone(),
            delta: delta.clone(),
            notes,
            recorded_by,
        };
        let row: AssetAudit = diesel::insert_into(asset_audits::table)
            .values(&new_audit)
            .get_result(conn)?;

        diesel::update(assets::table.find(asset_id))
            .set(assets::quantity.eq(&counted_quantity))
            .execute(conn)?;

        let crossed = match threshold.as_ref() {
            Some(t) => &previous > t && &counted_quantity <= t,
            None => false,
        };

        Ok(AuditOutcome {
            row,
            asset_name: name,
            asset_unit: unit.unwrap_or_default(),
            new_quantity: counted_quantity,
            threshold,
            crossed_low_stock: crossed,
        })
    })
}

/// Paginated audit history for one asset, newest first. Mirrors
/// the shape of `asset_usage::list_for_asset` so the frontend
/// can render both ledgers with the same paging widget.
// sync-audit-only: read-only ledger lookup
pub fn list_for_asset(
    conn: &mut DbConnection,
    asset_id: i32,
    limit: i64,
    offset: i64,
) -> QueryResult<Vec<AssetAudit>> {
    asset_audits::table
        .filter(asset_audits::asset_id.eq(asset_id))
        .order(asset_audits::recorded_at.desc())
        .limit(limit)
        .offset(offset)
        .load(conn)
}
