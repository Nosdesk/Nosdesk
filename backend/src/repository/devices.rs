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
pub trait DeviceDeletedObserver: Send + Sync {
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
fn asset_sync_payload(device: &Device) -> serde_json::Value {
    json!({
        "id": device.id,
        "name": device.name,
        "kind": device.kind,
        "hostname": device.hostname,
        "serial_number": device.serial_number,
        "manufacturer": device.manufacturer,
        "model": device.model,
        "asset_tag": device.asset_tag,
        "location": device.location,
        "primary_user_uuid": device.primary_user_uuid,
        "attributes": device.attributes,
        "quantity": device.quantity,
        "unit": device.unit,
    })
}

fn emit_asset_event(
    conn: &mut DbConnection,
    device: &Device,
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

// Device operations
pub fn get_all_devices(conn: &mut DbConnection) -> QueryResult<Vec<Device>> {
    devices::table
        .order_by(devices::id.asc())
        .load::<Device>(conn)
}

type DeviceBoxedQuery<'a> = devices::BoxedQuery<'a, diesel::pg::Pg>;

/// Apply search, warranty, and manufacturer filters to a device query.
/// Shared between data and count queries to avoid duplicating filter logic.
fn apply_device_filters<'a>(
    mut query: DeviceBoxedQuery<'a>,
    search: Option<&'a str>,
    warranty: Option<&'a str>,
    device_type: Option<&'a str>,
) -> DeviceBoxedQuery<'a> {
    if let Some(search_term) = search {
        if !search_term.is_empty() {
            let pattern = format!("%{}%", search_term.to_lowercase());
            query = query.filter(
                devices::name
                    .ilike(pattern.clone())
                    .or(devices::hostname.ilike(pattern.clone()))
                    .or(devices::serial_number.ilike(pattern.clone()))
                    .or(devices::model.ilike(pattern.clone()))
                    .or(devices::manufacturer.ilike(pattern.clone()))
                    .or(devices::id.eq_any(
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
        if w != "all" {
            query = query.filter(devices::warranty_status.eq(w));
        }
    }
    if let Some(m) = device_type {
        if m != "all" {
            query = query.filter(devices::manufacturer.eq(m));
        }
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
    device_type: Option<String>,
    warranty: Option<String>,
) -> Result<(Vec<Device>, i64), Error> {
    let total: i64 = apply_device_filters(
        devices::table.into_boxed(),
        search.as_deref(),
        warranty.as_deref(),
        device_type.as_deref(),
    )
    .count()
    .get_result(conn)?;

    let mut query = apply_device_filters(
        devices::table.into_boxed(),
        search.as_deref(),
        warranty.as_deref(),
        device_type.as_deref(),
    );

    // Apply sorting
    match (sort_field.as_deref(), sort_direction.as_deref()) {
        (Some("id"), Some("asc")) => query = query.order(devices::id.asc()),
        (Some("id"), _) => query = query.order(devices::id.desc()),
        (Some("name"), Some("asc")) => query = query.order(devices::name.asc()),
        (Some("name"), _) => query = query.order(devices::name.desc()),
        (Some("hostname"), Some("asc")) => query = query.order(devices::hostname.asc()),
        (Some("hostname"), _) => query = query.order(devices::hostname.desc()),
        (Some("model"), Some("asc")) => query = query.order(devices::model.asc()),
        (Some("model"), _) => query = query.order(devices::model.desc()),
        (Some("manufacturer"), Some("asc")) => query = query.order(devices::manufacturer.asc()),
        (Some("manufacturer"), _) => query = query.order(devices::manufacturer.desc()),
        (Some("warranty_status"), Some("asc")) => {
            query = query.order(devices::warranty_status.asc())
        }
        (Some("warranty_status"), _) => query = query.order(devices::warranty_status.desc()),
        (Some("serial_number"), Some("asc")) => query = query.order(devices::serial_number.asc()),
        (Some("serial_number"), _) => query = query.order(devices::serial_number.desc()),
        (Some("created_at"), Some("asc")) => query = query.order(devices::created_at.asc()),
        (Some("created_at"), _) => query = query.order(devices::created_at.desc()),
        (Some("updated_at"), Some("asc")) => query = query.order(devices::updated_at.asc()),
        (Some("updated_at"), _) => query = query.order(devices::updated_at.desc()),
        _ => query = query.order(devices::name.asc()),
    }

    let offset = (page - 1) * page_size;
    let results = query.offset(offset).limit(page_size).load::<Device>(conn)?;

    Ok((results, total))
}

pub fn get_device_by_id(conn: &mut DbConnection, device_id: i32) -> QueryResult<Device> {
    devices::table.find(device_id).first(conn)
}

pub fn get_device_by_entra_id(
    conn: &mut DbConnection,
    entra_device_id: &str,
) -> QueryResult<Device> {
    devices::table
        .filter(devices::entra_device_id.eq(entra_device_id))
        .first(conn)
}

pub fn get_device_by_microsoft_id(
    conn: &mut DbConnection,
    microsoft_device_id: &str,
) -> QueryResult<Device> {
    devices::table
        .filter(devices::microsoft_device_id.eq(microsoft_device_id))
        .first(conn)
}

pub fn create_device(conn: &mut DbConnection, new_device: NewDevice) -> QueryResult<Device> {
    // Wrap the INSERT + sync emit in a single transaction so a
    // crash between the two never leaves the row inserted
    // without a corresponding sync_actions event.
    // emit::record fires inside emit_asset_event.
    conn.transaction::<Device, Error, _>(|conn| {
        let device: Device = diesel::insert_into(devices::table)
            .values(&new_device)
            .get_result(conn)?;
        emit_asset_event(conn, &device, SyncOp::Insert, "asset.created")?;
        Ok(device)
    })
}

pub fn update_device(
    conn: &mut DbConnection,
    device_id: i32,
    device_update: DeviceUpdate,
) -> QueryResult<Device> {
    let mut update = device_update;
    update.updated_at = Some(Utc::now().naive_utc());

    // emit::record fires inside emit_asset_event.
    conn.transaction::<Device, Error, _>(|conn| {
        let device: Device = diesel::update(devices::table.find(device_id))
            .set(&update)
            .get_result(conn)?;
        emit_asset_event(conn, &device, SyncOp::Update, "asset.updated")?;
        Ok(device)
    })
}

pub fn delete_device(
    conn: &mut DbConnection,
    device_id: i32,
    observer: Option<&dyn DeviceDeletedObserver>,
) -> QueryResult<usize> {
    // emit::record fires inside emit_asset_event.
    let count = conn.transaction::<usize, Error, _>(|conn| {
        // Capture the row before deletion so the sync payload can
        // carry the final state to subscribers that joined after
        // the row was already gone from `assets`.
        let device: Option<Device> = devices::table.find(device_id).first(conn).optional()?;
        let removed = diesel::delete(devices::table.find(device_id)).execute(conn)?;
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

pub fn get_devices_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<Vec<Device>> {
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
) -> QueryResult<(Vec<Device>, i64)> {
    let mut count_query = apply_device_filters(devices::table.into_boxed(), search, None, None);
    if !exclude_ids.is_empty() {
        count_query = count_query.filter(devices::id.ne_all(exclude_ids));
    }
    let total_count = count_query.count().get_result::<i64>(conn)?;

    let mut data_query = apply_device_filters(devices::table.into_boxed(), search, None, None);
    if !exclude_ids.is_empty() {
        data_query = data_query.filter(devices::id.ne_all(exclude_ids));
    }
    let results = data_query
        .order(devices::name.asc())
        .limit(page_size)
        .offset((page - 1) * page_size)
        .load(conn)?;

    Ok((results, total_count))
}

/// Get multiple devices by their Entra device IDs (batch lookup for efficiency)
/// Used for mapping Microsoft Graph device members to local device IDs
pub fn get_devices_by_entra_ids(
    conn: &mut DbConnection,
    entra_ids: &[&str],
) -> QueryResult<Vec<(String, i32)>> {
    devices::table
        .filter(devices::entra_device_id.eq_any(entra_ids))
        .filter(devices::entra_device_id.is_not_null())
        .select((devices::entra_device_id.assume_not_null(), devices::id))
        .load::<(String, i32)>(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewDevice;
    use crate::test_helpers::setup_test_connection;

    fn minimal_device(name: &str) -> NewDevice {
        NewDevice {
            name: name.to_string(),
            hostname: None,
            device_type: None,
            serial_number: None,
            manufacturer: None,
            model: None,
            warranty_status: None,
            location: None,
            notes: None,
            primary_user_uuid: None,
            microsoft_device_id: None,
            intune_device_id: None,
            entra_device_id: None,
            compliance_state: None,
            last_sync_time: None,
            operating_system: None,
            os_version: None,
            is_managed: None,
            enrollment_date: None,
            warranty_start_date: None,
            warranty_end_date: None,
            purchase_date: None,
            asset_tag: None,
            kind: "device".to_string(),
            attributes: serde_json::json!({}),
            quantity: None,
            unit: None,
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

        let upd = DeviceUpdate {
            name: Some("NewName".to_string()),
            hostname: None,
            device_type: None,
            serial_number: None,
            manufacturer: None,
            model: None,
            warranty_status: None,
            location: None,
            notes: None,
            primary_user_uuid: None,
            microsoft_device_id: None,
            intune_device_id: None,
            entra_device_id: None,
            compliance_state: None,
            last_sync_time: None,
            operating_system: None,
            os_version: None,
            is_managed: None,
            enrollment_date: None,
            warranty_start_date: None,
            warranty_end_date: None,
            purchase_date: None,
            asset_tag: None,
            updated_at: None,
            kind: None,
            attributes: None,
            quantity: None,
            unit: None,
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
