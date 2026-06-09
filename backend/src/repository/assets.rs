use chrono::Utc;
use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// Observer fired after a device is deleted. Implementor removes
/// the device from the search index so the row doesn't haunt
/// search results after removal.
pub trait AssetDeletedObserver: Send + Sync {
    fn device_deleted(&self, device_id: i32);
}

/// Construct the sync-emit payload for an asset row. Shape
/// must stay in lockstep with `sync-models/asset.json`; the
/// frontend pool deserialises this directly into its Asset
/// cache entry. The deviation from the wire device shape is
/// intentional: the sync stream is the trimmed
/// reference-cache projection (slug, name, kind, attributes),
/// while the full REST device DTO carries Microsoft Graph and
/// warranty columns that aren't needed for picker / chip
/// rendering.
fn asset_sync_payload(device: &Asset) -> serde_json::Value {
    json!({
        "id": device.id,
        "name": device.name,
        "kind": device.kind,
        "serial_number": device.serial_number,
        "manufacturer": device.manufacturer,
        "model": device.model,
        "asset_tag": device.asset_tag,
        "location": device.location,
        "status": device.status,
        "primary_user_uuid": device.primary_user_uuid,
        "attributes": device.attributes,
        "quantity": device.quantity,
        "unit": device.unit,
        "external_sync_source": device.external_sync_source,
        "low_stock_threshold": device.low_stock_threshold,
    })
}

pub(crate) fn emit_asset_event(
    conn: &mut DbConnection,
    device: &Asset,
    op: SyncOp,
    event_type: &'static str,
) -> QueryResult<()> {
    emit::record(
        conn,
        SyncEmit {
            aggregate: SyncAggregate::Asset,
            aggregate_id: device.id.to_string(),
            op,
            event_type,
            data: asset_sync_payload(device),
            groups: groups::workspace(),
            causation_id: None,
        },
    )?;
    Ok(())
}

// Asset operations
pub fn get_all_devices(conn: &mut DbConnection) -> QueryResult<Vec<Asset>> {
    assets::table.order_by(assets::id.asc()).load::<Asset>(conn)
}

type AssetBoxedQuery<'a> = assets::BoxedQuery<'a, diesel::pg::Pg>;

/// Apply search, warranty, manufacturer, and stock filters to
/// a device query. Shared between data and count queries to
/// avoid duplicating filter logic.
fn apply_device_filters<'a>(
    mut query: AssetBoxedQuery<'a>,
    search: Option<&'a str>,
    warranty: Option<&'a str>,
    manufacturer_filter: Option<&'a str>,
    location_filter: Option<&'a str>,
    status_filter: Option<&'a str>,
    low_stock_only: bool,
) -> AssetBoxedQuery<'a> {
    if let Some(search_term) = search {
        if !search_term.is_empty() {
            let pattern = format!("%{}%", search_term.to_lowercase());
            // Hostname, OS, and other IT-flavoured columns moved
            // into the attributes JSONB in Pass B. Tantivy is the
            // index for fuzzy/cross-field search now; this SQL
            // fallback only covers the universal columns that
            // still live on `assets`.
            query = query.filter(
                assets::name
                    .ilike(pattern.clone())
                    .or(assets::serial_number.ilike(pattern.clone()))
                    .or(assets::model.ilike(pattern.clone()))
                    .or(assets::manufacturer.ilike(pattern.clone()))
                    .or(assets::id.eq_any(
                        search_term
                            .parse::<i32>()
                            .ok()
                            .map(|id| vec![id])
                            .unwrap_or_default(),
                    )),
            );
        }
    }
    if let Some(w) = warranty {
        // warranty_status moved into attributes JSONB. JSON path
        // comparison stays inside the boxed query so the filter
        // stays composable with the universal-column filters.
        // Accepts a comma-separated list so the chip UI can
        // multi-select (Active + Warning, etc); a single value
        // still works as before.
        if w != "all" && !w.is_empty() {
            let values: Vec<String> = w
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty() && *s != "all")
                .map(|s| s.to_string())
                .collect();
            if !values.is_empty() {
                // Bind via ANY(...) over a text array so the value
                // count is dynamic without dropping out of the
                // boxed query. Diesel's `eq_any` doesn't compose
                // with `sql::<Bool>` fragments, so this stays raw.
                // Compare case-insensitively because the request
                // pipeline lowercases filter values while the
                // stored JSON uses capitalised buckets ("Active",
                // "Warning", "Expired", "Unknown").
                let values_lower: Vec<String> = values.iter().map(|v| v.to_lowercase()).collect();
                query = query.filter(
                    diesel::dsl::sql::<diesel::sql_types::Bool>(
                        "LOWER(attributes->>'warranty_status') = ANY(",
                    )
                    .bind::<diesel::sql_types::Array<diesel::sql_types::Text>, _>(values_lower)
                    .sql(")"),
                );
            }
        }
    }
    if let Some(m) = manufacturer_filter {
        if m != "all" {
            query = query.filter(assets::manufacturer.eq(m));
        }
    }
    if let Some(l) = location_filter {
        if l != "all" && !l.is_empty() {
            let values: Vec<String> = l
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty() && *s != "all")
                .map(|s| s.to_lowercase())
                .collect();
            if !values.is_empty() {
                query = query.filter(
                    diesel::dsl::sql::<diesel::sql_types::Bool>(
                        "LOWER(COALESCE(location, '')) = ANY(",
                    )
                    .bind::<diesel::sql_types::Array<diesel::sql_types::Text>, _>(values)
                    .sql(")"),
                );
            }
        }
    }
    if let Some(s) = status_filter {
        // Lifecycle status is a real column with snake_case values
        // (`in_service`, `in_repair`, …). CSV multi-select from the
        // chip UI; no case folding needed unlike warranty/location.
        if s != "all" && !s.is_empty() {
            let values: Vec<String> = s
                .split(',')
                .map(|v| v.trim())
                .filter(|v| !v.is_empty() && *v != "all")
                .map(|v| v.to_string())
                .collect();
            if !values.is_empty() {
                query = query.filter(assets::status.eq_any(values));
            }
        }
    }
    if low_stock_only {
        // Both columns must be set, and current quantity must be
        // at or below the threshold. NUMERIC comparison is exact.
        query = query.filter(
            assets::quantity
                .is_not_null()
                .and(assets::low_stock_threshold.is_not_null())
                .and(assets::quantity.le(assets::low_stock_threshold)),
        );
    }
    query
}

// Get paginated devices with filtering and sorting
pub fn get_paginated_devices(
    conn: &mut DbConnection,
    page: i64,
    page_size: i64,
    sort_field: Option<String>,
    sort_direction: Option<String>,
    search: Option<String>,
    status: Option<String>,
    warranty: Option<String>,
    location: Option<String>,
    low_stock_only: bool,
) -> Result<(Vec<Asset>, i64), Error> {
    // manufacturer slot is `None`: there is no manufacturer-filter
    // consumer yet. The former `?type=` param fed this slot, which
    // silently filtered on `manufacturer`; that dead pathway is gone.
    let total: i64 = apply_device_filters(
        assets::table.into_boxed(),
        search.as_deref(),
        warranty.as_deref(),
        None,
        location.as_deref(),
        status.as_deref(),
        low_stock_only,
    )
    .count()
    .get_result(conn)?;

    let mut query = apply_device_filters(
        assets::table.into_boxed(),
        search.as_deref(),
        warranty.as_deref(),
        None,
        location.as_deref(),
        status.as_deref(),
        low_stock_only,
    );

    // Apply sorting
    match (sort_field.as_deref(), sort_direction.as_deref()) {
        (Some("id"), Some("asc")) => query = query.order(assets::id.asc()),
        (Some("id"), _) => query = query.order(assets::id.desc()),
        (Some("name"), Some("asc")) => query = query.order(assets::name.asc()),
        (Some("name"), _) => query = query.order(assets::name.desc()),
        (Some("model"), Some("asc")) => query = query.order(assets::model.asc()),
        (Some("model"), _) => query = query.order(assets::model.desc()),
        (Some("manufacturer"), Some("asc")) => query = query.order(assets::manufacturer.asc()),
        (Some("manufacturer"), _) => query = query.order(assets::manufacturer.desc()),
        (Some("location"), Some("asc")) => query = query.order(assets::location.asc().nulls_last()),
        (Some("location"), _) => query = query.order(assets::location.desc().nulls_last()),
        (Some("status"), Some("asc")) => query = query.order(assets::status.asc()),
        (Some("status"), _) => query = query.order(assets::status.desc()),
        (Some("serial_number"), Some("asc")) => query = query.order(assets::serial_number.asc()),
        (Some("serial_number"), _) => query = query.order(assets::serial_number.desc()),
        (Some("created_at"), Some("asc")) => query = query.order(assets::created_at.asc()),
        (Some("created_at"), _) => query = query.order(assets::created_at.desc()),
        (Some("updated_at"), Some("asc")) => query = query.order(assets::updated_at.asc()),
        (Some("updated_at"), _) => query = query.order(assets::updated_at.desc()),
        // Stock-tracked rows sort by on-hand quantity. NULL is
        // ordered last in both directions so non-stock-tracked
        // assets cluster at the bottom regardless of asc/desc.
        (Some("quantity"), Some("asc")) => query = query.order(assets::quantity.asc().nulls_last()),
        (Some("quantity"), _) => query = query.order(assets::quantity.desc().nulls_last()),
        _ => query = query.order(assets::name.asc()),
    }

    let offset = (page - 1) * page_size;
    let results = query.offset(offset).limit(page_size).load::<Asset>(conn)?;

    Ok((results, total))
}

#[derive(Debug, QueryableByName)]
pub struct AssetLocationSummary {
    #[diesel(sql_type = diesel::sql_types::Text)]
    pub location: String,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub asset_count: i64,
}

// sync-audit-only: read-only SELECT aggregate (distinct locations), no row write to emit
pub fn list_asset_locations(conn: &mut DbConnection) -> QueryResult<Vec<AssetLocationSummary>> {
    diesel::sql_query(
        "SELECT BTRIM(location) AS location, COUNT(*)::bigint AS asset_count \
         FROM assets \
         WHERE NULLIF(BTRIM(location), '') IS NOT NULL \
         GROUP BY BTRIM(location) \
         ORDER BY LOWER(BTRIM(location)) ASC \
         LIMIT 200",
    )
    .load(conn)
}

pub fn get_device_by_id(conn: &mut DbConnection, device_id: i32) -> QueryResult<Asset> {
    assets::table.find(device_id).first(conn)
}

/// Look up an asset by the `entra_device_id` attribute key.
/// The ID moved out of its own column in Pass B; this helper
/// hides the JSONB path so Intune sync handlers stay readable.
pub fn get_device_by_entra_id(
    conn: &mut DbConnection,
    entra_device_id: &str,
) -> QueryResult<Asset> {
    assets::table
        .filter(
            diesel::dsl::sql::<diesel::sql_types::Bool>("attributes->>'entra_device_id' = ")
                .bind::<diesel::sql_types::Text, _>(entra_device_id.to_string()),
        )
        .first(conn)
}

pub fn get_device_by_microsoft_id(
    conn: &mut DbConnection,
    microsoft_device_id: &str,
) -> QueryResult<Asset> {
    assets::table
        .filter(
            diesel::dsl::sql::<diesel::sql_types::Bool>("attributes->>'microsoft_device_id' = ")
                .bind::<diesel::sql_types::Text, _>(microsoft_device_id.to_string()),
        )
        .first(conn)
}

pub fn create_device(conn: &mut DbConnection, new_device: NewAsset) -> QueryResult<Asset> {
    // Wrap the INSERT + sync emit in a single transaction so a
    // crash between the two never leaves the row inserted
    // without a corresponding sync_actions event.
    // emit::record fires inside emit_asset_event.
    conn.transaction::<Asset, Error, _>(|conn| {
        let device: Asset = diesel::insert_into(assets::table)
            .values(&new_device)
            .get_result(conn)?;
        emit_asset_event(conn, &device, SyncOp::Insert, "asset.created")?;
        Ok(device)
    })
}

pub fn update_device(
    conn: &mut DbConnection,
    device_id: i32,
    device_update: AssetUpdate,
) -> QueryResult<Asset> {
    let mut update = device_update;
    update.updated_at = Some(Utc::now().naive_utc());

    // emit::record fires inside emit_asset_event.
    conn.transaction::<Asset, Error, _>(|conn| {
        let device: Asset = diesel::update(assets::table.find(device_id))
            .set(&update)
            .get_result(conn)?;
        emit_asset_event(conn, &device, SyncOp::Update, "asset.updated")?;
        Ok(device)
    })
}

pub fn delete_device(
    conn: &mut DbConnection,
    device_id: i32,
    observer: Option<&dyn AssetDeletedObserver>,
) -> QueryResult<usize> {
    // emit::record fires inside emit_asset_event.
    let count = conn.transaction::<usize, Error, _>(|conn| {
        // Capture the row before deletion so the sync payload can
        // carry the final state to subscribers that joined after
        // the row was already gone from `assets`.
        let device: Option<Asset> = assets::table.find(device_id).first(conn).optional()?;
        let removed = diesel::delete(assets::table.find(device_id)).execute(conn)?;
        if removed > 0 {
            if let Some(device) = device.as_ref() {
                emit_asset_event(conn, device, SyncOp::Delete, "asset.deleted")?;
            }
        }
        Ok(removed)
    })?;
    if count > 0 {
        if let Some(observer) = observer {
            observer.device_deleted(device_id);
        }
    }
    Ok(count)
}

pub fn get_devices_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<Vec<Asset>> {
    use crate::schema::assets::dsl::*;

    assets
        .filter(primary_user_uuid.eq(user_uuid))
        .order(name.asc())
        .load(conn)
}

pub fn get_paginated_devices_excluding_ids(
    conn: &mut DbConnection,
    page: i64,
    page_size: i64,
    search: Option<&str>,
    exclude_ids: &[i32],
) -> QueryResult<(Vec<Asset>, i64)> {
    let mut count_query = apply_device_filters(
        assets::table.into_boxed(),
        search,
        None,
        None,
        None,
        None,
        false,
    );
    if !exclude_ids.is_empty() {
        count_query = count_query.filter(assets::id.ne_all(exclude_ids));
    }
    let total_count = count_query.count().get_result::<i64>(conn)?;

    let mut data_query = apply_device_filters(
        assets::table.into_boxed(),
        search,
        None,
        None,
        None,
        None,
        false,
    );
    if !exclude_ids.is_empty() {
        data_query = data_query.filter(assets::id.ne_all(exclude_ids));
    }
    let results = data_query
        .order(assets::name.asc())
        .limit(page_size)
        .offset((page - 1) * page_size)
        .load(conn)?;

    Ok((results, total_count))
}

/// Map a batch of Entra device IDs (now attribute keys, not
/// columns) to local asset ids. Returns `(entra_id, asset_id)`
/// pairs for the rows whose `attributes->>'entra_device_id'` is
/// in the set. The Intune sync uses this to resolve group
/// memberships against the local roster.
// sync-audit-only: read-only lookup used by Intune sync to map external IDs to local asset IDs
pub fn get_devices_by_entra_ids(
    conn: &mut DbConnection,
    entra_ids: &[&str],
) -> QueryResult<Vec<(String, i32)>> {
    use diesel::sql_types::{Array, Text};
    let owned: Vec<String> = entra_ids.iter().map(|s| s.to_string()).collect();
    diesel::sql_query(
        "SELECT attributes->>'entra_device_id' AS entra_id, id \
         FROM assets \
         WHERE attributes->>'entra_device_id' = ANY($1)",
    )
    .bind::<Array<Text>, _>(owned)
    .load::<EntraIdRow>(conn)
    .map(|rows| rows.into_iter().map(|r| (r.entra_id, r.id)).collect())
}

#[derive(diesel::QueryableByName)]
struct EntraIdRow {
    #[diesel(sql_type = diesel::sql_types::Text)]
    entra_id: String,
    #[diesel(sql_type = diesel::sql_types::Int4)]
    id: i32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAsset;
    use crate::test_helpers::setup_test_connection;

    fn minimal_device(name: &str) -> NewAsset {
        NewAsset {
            name: name.to_string(),
            serial_number: None,
            manufacturer: None,
            model: None,
            location: None,
            notes: None,
            primary_user_uuid: None,
            purchase_date: None,
            asset_tag: None,
            kind: "device".to_string(),
            attributes: serde_json::json!({}),
            quantity: None,
            unit: None,
            external_sync_source: None,
            low_stock_threshold: None,
        }
    }

    #[test]
    fn create_and_get_device() {
        let mut conn = setup_test_connection();
        let dev = create_device(&mut conn, minimal_device("TestDev")).unwrap();

        let fetched = get_device_by_id(&mut conn, dev.id).unwrap();
        assert_eq!(fetched.name, "TestDev");
    }

    #[test]
    fn get_all_devices_test() {
        let mut conn = setup_test_connection();
        let d1 = create_device(&mut conn, minimal_device("Dev1")).unwrap();
        let d2 = create_device(&mut conn, minimal_device("Dev2")).unwrap();

        let all = get_all_devices(&mut conn).unwrap();
        let ids: Vec<i32> = all.iter().map(|d| d.id).collect();
        assert!(ids.contains(&d1.id));
        assert!(ids.contains(&d2.id));
    }

    #[test]
    fn update_device_test() {
        let mut conn = setup_test_connection();
        let dev = create_device(&mut conn, minimal_device("OldName")).unwrap();

        let upd = AssetUpdate {
            name: Some("NewName".to_string()),
            ..Default::default()
        };

        let updated = update_device(&mut conn, dev.id, upd).unwrap();
        assert_eq!(updated.name, "NewName");
    }

    #[test]
    fn delete_device_test() {
        let mut conn = setup_test_connection();
        let dev = create_device(&mut conn, minimal_device("Gone")).unwrap();

        let count = delete_device(&mut conn, dev.id, None).unwrap();
        assert_eq!(count, 1);

        let result = get_device_by_id(&mut conn, dev.id);
        assert!(result.is_err());
    }
}
