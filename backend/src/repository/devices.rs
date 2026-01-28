use diesel::prelude::*;
use diesel::QueryResult;
use diesel::result::Error;
use chrono::Utc;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

// Device operations
pub fn get_all_devices(conn: &mut DbConnection) -> QueryResult<Vec<Device>> {
    devices::table
        .order_by(devices::id.asc())
        .load::<Device>(conn)
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
    // Build the main query
    let mut query = devices::table.into_boxed();
    
    // Apply filters if provided
    if let Some(search_term) = search.clone() {
        if !search_term.is_empty() {
            let search_pattern = format!("%{}%", search_term.to_lowercase());
            query = query.filter(
                devices::name.ilike(search_pattern.clone())
                    .or(devices::hostname.ilike(search_pattern.clone()))
                    .or(devices::serial_number.ilike(search_pattern.clone()))
                    .or(devices::model.ilike(search_pattern.clone()))
                    .or(devices::manufacturer.ilike(search_pattern.clone()))
                    .or(devices::id.eq_any(
                        search_term.parse::<i32>().ok().map(|id| vec![id]).unwrap_or_default()
                    ))
            );
        }
    }
    
    // Handle warranty status filter
    if let Some(warranty_filter) = warranty.clone() {
        if warranty_filter != "all" {
            query = query.filter(devices::warranty_status.eq(warranty_filter));
        }
    }
    
    // Handle manufacturer filter (device_type can be used for manufacturer filtering)
    if let Some(manufacturer_filter) = device_type.clone() {
        if manufacturer_filter != "all" {
            query = query.filter(devices::manufacturer.eq(manufacturer_filter));
        }
    }
    
    // Build a separate count query with the same filters
    let mut count_query = devices::table.into_boxed();
    
    if let Some(search_term) = search {
        if !search_term.is_empty() {
            let search_pattern = format!("%{}%", search_term.to_lowercase());
            count_query = count_query.filter(
                devices::name.ilike(search_pattern.clone())
                    .or(devices::hostname.ilike(search_pattern.clone()))
                    .or(devices::serial_number.ilike(search_pattern.clone()))
                    .or(devices::model.ilike(search_pattern.clone()))
                    .or(devices::manufacturer.ilike(search_pattern.clone()))
                    .or(devices::id.eq_any(
                        search_term.parse::<i32>().ok().map(|id| vec![id]).unwrap_or_default()
                    ))
            );
        }
    }
    
    if let Some(warranty_filter) = warranty {
        if warranty_filter != "all" {
            count_query = count_query.filter(devices::warranty_status.eq(warranty_filter));
        }
    }
    
    if let Some(manufacturer_filter) = device_type {
        if manufacturer_filter != "all" {
            count_query = count_query.filter(devices::manufacturer.eq(manufacturer_filter));
        }
    }
    
    // Get total count
    let total: i64 = count_query.count().get_result(conn)?;
    
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
        (Some("warranty_status"), Some("asc")) => query = query.order(devices::warranty_status.asc()),
        (Some("warranty_status"), _) => query = query.order(devices::warranty_status.desc()),
        (Some("serial_number"), Some("asc")) => query = query.order(devices::serial_number.asc()),
        (Some("serial_number"), _) => query = query.order(devices::serial_number.desc()),
        (Some("created_at"), Some("asc")) => query = query.order(devices::created_at.asc()),
        (Some("created_at"), _) => query = query.order(devices::created_at.desc()),
        (Some("updated_at"), Some("asc")) => query = query.order(devices::updated_at.asc()),
        (Some("updated_at"), _) => query = query.order(devices::updated_at.desc()),
        _ => query = query.order(devices::name.asc()), // Default sort by name
    }
    
    // Apply pagination
    let offset = (page - 1) * page_size;
    query = query.offset(offset).limit(page_size);
    
    // Execute the query
    let results = query.load::<Device>(conn)?;
    
    Ok((results, total))
}

pub fn get_device_by_id(conn: &mut DbConnection, device_id: i32) -> QueryResult<Device> {
    devices::table
        .find(device_id)
        .first(conn)
}

#[allow(dead_code)]
pub fn get_device_by_intune_id(conn: &mut DbConnection, intune_device_id: &str) -> QueryResult<Device> {
    devices::table
        .filter(devices::intune_device_id.eq(intune_device_id))
        .first(conn)
}

pub fn get_device_by_entra_id(conn: &mut DbConnection, entra_device_id: &str) -> QueryResult<Device> {
    devices::table
        .filter(devices::entra_device_id.eq(entra_device_id))
        .first(conn)
}

pub fn get_device_by_microsoft_id(conn: &mut DbConnection, microsoft_device_id: &str) -> QueryResult<Device> {
    devices::table
        .filter(devices::microsoft_device_id.eq(microsoft_device_id))
        .first(conn)
}

#[allow(dead_code)]
pub fn get_devices_by_user(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<Vec<Device>> {
    devices::table
        .filter(devices::primary_user_uuid.eq(user_uuid))
        .order_by(devices::name.asc())
        .load::<Device>(conn)
}

pub fn create_device(conn: &mut DbConnection, new_device: NewDevice) -> QueryResult<Device> {
    diesel::insert_into(devices::table)
        .values(&new_device)
        .get_result(conn)
}

pub fn update_device(conn: &mut DbConnection, device_id: i32, device_update: DeviceUpdate) -> QueryResult<Device> {
    let mut update = device_update;
    update.updated_at = Some(Utc::now().naive_utc());
    
    diesel::update(devices::table.find(device_id))
        .set(&update)
        .get_result(conn)
}

pub fn delete_device(conn: &mut DbConnection, device_id: i32) -> QueryResult<usize> {
    diesel::delete(devices::table.find(device_id))
        .execute(conn)
}

// Microsoft Entra/Intune specific functions
#[allow(dead_code)]
pub fn upsert_device_by_intune_id(
    conn: &mut DbConnection,
    _intune_device_id: &str,
    device_data: NewDevice,
) -> QueryResult<Device> {
    use diesel::upsert::excluded;
    
    diesel::insert_into(devices::table)
        .values(&device_data)
        .on_conflict(devices::intune_device_id)
        .do_update()
        .set((
            devices::name.eq(excluded(devices::name)),
            devices::hostname.eq(excluded(devices::hostname)),
            devices::serial_number.eq(excluded(devices::serial_number)),
            devices::model.eq(excluded(devices::model)),
            devices::warranty_status.eq(excluded(devices::warranty_status)),
            devices::manufacturer.eq(excluded(devices::manufacturer)),
            devices::primary_user_uuid.eq(excluded(devices::primary_user_uuid)),
            devices::entra_device_id.eq(excluded(devices::entra_device_id)),
            devices::updated_at.eq(Utc::now().naive_utc()),
        ))
        .get_result(conn)
}

#[allow(dead_code)]
pub fn upsert_device_by_entra_id(
    conn: &mut DbConnection,
    _entra_device_id: &str,
    device_data: NewDevice,
) -> QueryResult<Device> {
    use diesel::upsert::excluded;
    
    diesel::insert_into(devices::table)
        .values(&device_data)
        .on_conflict(devices::entra_device_id)
        .do_update()
        .set((
            devices::name.eq(excluded(devices::name)),
            devices::hostname.eq(excluded(devices::hostname)),
            devices::serial_number.eq(excluded(devices::serial_number)),
            devices::model.eq(excluded(devices::model)),
            devices::warranty_status.eq(excluded(devices::warranty_status)),
            devices::manufacturer.eq(excluded(devices::manufacturer)),
            devices::primary_user_uuid.eq(excluded(devices::primary_user_uuid)),
            devices::intune_device_id.eq(excluded(devices::intune_device_id)),
            devices::updated_at.eq(Utc::now().naive_utc()),
        ))
        .get_result(conn)
}

pub fn get_devices_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<Vec<Device>> {
    use crate::schema::devices::dsl::*;
    
    devices
        .filter(primary_user_uuid.eq(user_uuid))
        .order(name.asc())
        .load(conn)
}

pub fn get_paginated_devices_excluding_ids(
    conn: &mut DbConnection, 
    page: i64, 
    page_size: i64, 
    search: Option<&str>,
    exclude_ids: &[i32]
) -> QueryResult<(Vec<Device>, i64)> {
    use crate::schema::devices::dsl::*;
    
    // Create count query
    let mut count_query = devices.into_boxed();
    
    // Exclude specific device IDs
    if !exclude_ids.is_empty() {
        count_query = count_query.filter(id.ne_all(exclude_ids));
    }
    
    // Apply search filter if provided
    if let Some(search_term) = search {
        if !search_term.trim().is_empty() {
            let search_pattern = format!("%{}%", search_term.trim());
            count_query = count_query.filter(
                name.ilike(search_pattern.clone())
                    .or(hostname.ilike(search_pattern.clone()))
                    .or(serial_number.ilike(search_pattern.clone()))
                    .or(manufacturer.ilike(search_pattern.clone()))
                    .or(model.ilike(search_pattern))
            );
        }
    }
    
    let total_count = count_query.count().get_result::<i64>(conn)?;
    
    // Create data query
    let mut data_query = devices.into_boxed();
    
    // Apply the same filters
    if !exclude_ids.is_empty() {
        data_query = data_query.filter(id.ne_all(exclude_ids));
    }
    
    if let Some(search_term) = search {
        if !search_term.trim().is_empty() {
            let search_pattern = format!("%{}%", search_term.trim());
            data_query = data_query.filter(
                name.ilike(search_pattern.clone())
                    .or(hostname.ilike(search_pattern.clone()))
                    .or(serial_number.ilike(search_pattern.clone()))
                    .or(manufacturer.ilike(search_pattern.clone()))
                    .or(model.ilike(search_pattern))
            );
        }
    }
    
    let results = data_query
        .order(name.asc())
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