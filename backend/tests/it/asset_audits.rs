//! Asset audit ledger integration tests.

use std::str::FromStr;

use backend::models::NewAsset;
use backend::repository::asset_audits;
use backend::repository::assets as asset_repo;
use bigdecimal::BigDecimal;
use diesel::prelude::*;

use crate::common::TestDb;

fn seed_stock_asset(
    conn: &mut backend::db::DbConnection,
    qty: &str,
    threshold: Option<&str>,
) -> i32 {
    let new = NewAsset {
        name: "Stock test asset".to_string(),
        serial_number: None,
        manufacturer: None,
        model: None,
        location: None,
        notes: None,
        primary_user_uuid: None,
        purchase_date: None,
        asset_tag: None,
        kind: "generic".to_string(),
        attributes: serde_json::json!({}),
        quantity: Some(BigDecimal::from_str(qty).unwrap()),
        unit: Some("pcs".to_string()),
        external_sync_source: None,
        low_stock_threshold: threshold.map(|t| BigDecimal::from_str(t).unwrap()),
        created_by: None,
    };
    asset_repo::create_device(conn, new)
        .expect("insert asset")
        .id
}

#[derive(diesel::QueryableByName)]
struct QtyRow {
    #[diesel(sql_type = diesel::sql_types::Numeric)]
    quantity: BigDecimal,
}

fn read_quantity(conn: &mut backend::db::DbConnection, asset_id: i32) -> BigDecimal {
    diesel::sql_query("SELECT quantity FROM assets WHERE id = $1")
        .bind::<diesel::sql_types::Integer, _>(asset_id)
        .get_result::<QtyRow>(&mut **conn)
        .expect("re-read quantity")
        .quantity
}

#[test]
fn audit_corrects_quantity_and_records_signed_delta() {
    let db = TestDb::new();
    let mut conn = db.conn();

    // Book shows 50, count finds 42. Delta = 42 - 50 = -8.
    let asset_id = seed_stock_asset(&mut conn, "50.000", None);
    let outcome = asset_audits::record_audit(
        &mut conn,
        asset_id,
        BigDecimal::from_str("42.000").unwrap(),
        Some("Found 8 missing".to_string()),
        None,
    )
    .expect("audit");

    assert_eq!(outcome.row.previous_quantity.to_string(), "50.000");
    assert_eq!(outcome.row.counted_quantity.to_string(), "42.000");
    assert_eq!(outcome.row.delta.to_string(), "-8.000");
    assert_eq!(outcome.new_quantity.to_string(), "42.000");
    assert!(!outcome.crossed_low_stock); // no threshold configured

    let live = read_quantity(&mut conn, asset_id);
    assert_eq!(live.to_string(), "42.000");
}

#[test]
fn audit_above_threshold_does_not_cross_low_stock() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let asset_id = seed_stock_asset(&mut conn, "50.000", Some("10.000"));

    // 50 -> 20, threshold 10. Both sides still above threshold.
    let outcome = asset_audits::record_audit(
        &mut conn,
        asset_id,
        BigDecimal::from_str("20.000").unwrap(),
        None,
        None,
    )
    .expect("audit");
    assert!(!outcome.crossed_low_stock);
}

#[test]
fn audit_below_threshold_edge_crosses() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let asset_id = seed_stock_asset(&mut conn, "50.000", Some("10.000"));

    // 50 (above) -> 5 (below threshold) → edge crossing.
    let outcome = asset_audits::record_audit(
        &mut conn,
        asset_id,
        BigDecimal::from_str("5.000").unwrap(),
        None,
        None,
    )
    .expect("audit");
    assert!(outcome.crossed_low_stock);
    assert_eq!(outcome.row.delta.to_string(), "-45.000");
}

#[test]
fn audit_already_below_threshold_does_not_re_cross() {
    let db = TestDb::new();
    let mut conn = db.conn();
    // Start already below the threshold.
    let asset_id = seed_stock_asset(&mut conn, "3.000", Some("10.000"));

    let outcome = asset_audits::record_audit(
        &mut conn,
        asset_id,
        BigDecimal::from_str("2.000").unwrap(),
        None,
        None,
    )
    .expect("audit");
    // Started below, ended below: no edge to detect, no
    // duplicate alert.
    assert!(!outcome.crossed_low_stock);
}

#[test]
fn audit_with_matching_count_records_zero_delta() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let asset_id = seed_stock_asset(&mut conn, "25.000", None);

    let outcome = asset_audits::record_audit(
        &mut conn,
        asset_id,
        BigDecimal::from_str("25.000").unwrap(),
        Some("Books match".to_string()),
        None,
    )
    .expect("audit");
    // 25 - 25 = 0; BigDecimal drops scale on subtraction so
    // the stored value is "0". Compare numerically rather than
    // by string.
    assert_eq!(outcome.row.delta, BigDecimal::from(0));
    assert_eq!(read_quantity(&mut conn, asset_id).to_string(), "25.000");
}

/// The on-hand quantity each `asset.updated` row for the asset carried,
/// oldest first.
fn quantities_sent(conn: &mut backend::db::DbConnection, asset_id: i32) -> Vec<BigDecimal> {
    use backend::schema::sync_actions::dsl as s;
    let rows: Vec<serde_json::Value> = s::sync_actions
        .filter(s::event_type.eq("asset.updated"))
        .filter(s::aggregate_id.eq(asset_id.to_string()))
        .order(s::sync_id.asc())
        .select(s::data)
        .load(&mut **conn)
        .expect("asset.updated rows");
    rows.iter()
        .map(|data| match &data["quantity"] {
            serde_json::Value::String(q) => BigDecimal::from_str(q).expect("quantity"),
            other => BigDecimal::from_str(&other.to_string()).expect("quantity"),
        })
        .collect()
}

/// A count corrects the on-hand quantity, and open clients hold the asset row,
/// so the correction goes out as `asset.updated` carrying the new count.
#[test]
fn audit_sends_the_new_quantity_to_open_clients() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let asset_id = seed_stock_asset(&mut conn, "50.000", None);

    asset_audits::record_audit(
        &mut conn,
        asset_id,
        BigDecimal::from_str("42.000").unwrap(),
        None,
        None,
    )
    .expect("audit");

    assert_eq!(
        quantities_sent(&mut conn, asset_id),
        vec![BigDecimal::from_str("42.000").unwrap()]
    );
}

/// Usage and restock move the on-hand quantity, and each move goes out as
/// `asset.updated` carrying the new count.
#[test]
fn usage_and_restock_send_the_new_quantity_to_open_clients() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let asset_id = seed_stock_asset(&mut conn, "10.000", None);

    let event = |kind: &str, qty: &str| backend::models::NewAssetUsage {
        asset_id,
        ticket_id: None,
        quantity_used: BigDecimal::from_str(qty).unwrap(),
        unit: "pcs".to_string(),
        recorded_by: None,
        notes: None,
        event_kind: kind.to_string(),
    };
    backend::repository::asset_usage::record_event(&mut conn, event("usage", "3")).expect("usage");
    backend::repository::asset_usage::record_event(&mut conn, event("restock", "5"))
        .expect("restock");

    assert_eq!(
        quantities_sent(&mut conn, asset_id),
        vec![BigDecimal::from(7), BigDecimal::from(12)]
    );
}
