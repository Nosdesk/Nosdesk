//! User contact profiles + the per-workspace user custom-field schema.
//!
//! The schema is override-only: a `user_field_schema` row exists once an admin
//! customises it, otherwise reads fall back to `default_user_field_schema()`.
//! Profile rows hold the SCIM-Enterprise standard columns + the custom-field
//! values, validated against the effective schema at the handler boundary.
//!
//! These are NOT a sync aggregate today: the user DTO re-fetches, and contact
//! fields fold into the user sync payload in a later phase. Writes are audited
//! via each table's audit trigger.

use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    NewUserAddress, NewUserPhoneNumber, NewUserProfile, UserAddress, UserAddressInput,
    UserPhoneInput, UserPhoneNumber, UserProfile, UserProfileInput,
};
use crate::schema::{user_addresses, user_field_schema, user_phone_numbers, user_profiles};

/// The built-in user custom-field schema applied when a workspace hasn't
/// customised one. vCard-aligned keys (office_location→ORG unit, gender→GENDER,
/// birthday→BDAY). `office_location` is `synced`: the directory sync feeds it
/// read-only. All are optional and admin-removable.
pub fn default_user_field_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "office_location": { "type": "string", "title": "Office location", "synced": true },
            "gender": { "type": "string", "title": "Gender" },
            "birthday": { "type": "string", "format": "date", "title": "Birthday" }
        }
    })
}

/// The workspace's effective user custom-field schema (stored override or the
/// code default). RLS scopes the row to the active workspace.
pub fn get_field_schema(conn: &mut DbConnection) -> QueryResult<Value> {
    let stored: Option<Value> = user_field_schema::table
        .select(user_field_schema::schema)
        .first(conn)
        .optional()?;
    Ok(stored.unwrap_or_else(default_user_field_schema))
}

// sync-audit-only: user custom-field schema is workspace config (audited, picker re-fetches), not a sync aggregate
/// Upsert the workspace's user custom-field schema. Caller validates the schema
/// shape first. Returns the stored schema.
pub fn set_field_schema(
    conn: &mut DbConnection,
    workspace_id: i32,
    schema: &Value,
    actor: Option<Uuid>,
) -> QueryResult<Value> {
    diesel::insert_into(user_field_schema::table)
        .values((
            user_field_schema::workspace_id.eq(workspace_id),
            user_field_schema::schema.eq(schema),
            user_field_schema::created_by.eq(actor),
        ))
        .on_conflict(user_field_schema::workspace_id)
        .do_update()
        .set(user_field_schema::schema.eq(schema))
        .returning(user_field_schema::schema)
        .get_result(conn)
}

/// The active workspace's profile row for a user, if any.
pub fn get_profile(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<Option<UserProfile>> {
    user_profiles::table
        .filter(user_profiles::user_uuid.eq(user_uuid))
        .first(conn)
        .optional()
}

// sync-audit-only: user profile is per-(user,workspace) contact data (audited); contact fields fold into the user sync payload in a later phase
/// Upsert the manual side of a user's profile. `directory_synced` is never
/// changed here (Graph owns it); the caller preserves synced standard columns.
pub fn upsert_profile(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    input: &UserProfileInput,
    actor: Option<Uuid>,
) -> QueryResult<UserProfile> {
    diesel::insert_into(user_profiles::table)
        .values(NewUserProfile {
            user_uuid,
            job_title: input.job_title.clone(),
            organization: input.organization.clone(),
            department: input.department.clone(),
            custom_fields: input.custom_fields.clone(),
            directory_synced: false,
            created_by: actor,
        })
        .on_conflict((user_profiles::workspace_id, user_profiles::user_uuid))
        .do_update()
        .set((
            user_profiles::job_title.eq(input.job_title.clone()),
            user_profiles::organization.eq(input.organization.clone()),
            user_profiles::department.eq(input.department.clone()),
            user_profiles::custom_fields.eq(input.custom_fields.clone()),
        ))
        .get_result(conn)
}

// ---- Phones (multi-valued, typed, workspace-scoped) ------------------------

pub fn list_phones(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<Vec<UserPhoneNumber>> {
    user_phone_numbers::table
        .filter(user_phone_numbers::user_uuid.eq(user_uuid))
        .order((
            user_phone_numbers::is_primary.desc(),
            user_phone_numbers::id.asc(),
        ))
        .load(conn)
}

pub fn get_phone(conn: &mut DbConnection, id: i32) -> QueryResult<Option<UserPhoneNumber>> {
    user_phone_numbers::table.find(id).first(conn).optional()
}

fn clear_primary_phones(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<usize> {
    diesel::update(
        user_phone_numbers::table
            .filter(user_phone_numbers::user_uuid.eq(user_uuid))
            .filter(user_phone_numbers::is_primary.eq(true)),
    )
    .set(user_phone_numbers::is_primary.eq(false))
    .execute(conn)
}

// sync-audit-only: user phone is per-(user,workspace) contact data (audited); folds into the user sync payload in a later phase
pub fn create_phone(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    input: &UserPhoneInput,
    source: Option<String>,
    actor: Option<Uuid>,
) -> QueryResult<UserPhoneNumber> {
    conn.transaction(|conn| {
        if input.is_primary {
            clear_primary_phones(conn, user_uuid)?;
        }
        diesel::insert_into(user_phone_numbers::table)
            .values(NewUserPhoneNumber {
                user_uuid,
                phone: input.phone.clone(),
                phone_type: input.phone_type.clone(),
                is_primary: input.is_primary,
                source,
                label: input.label.clone(),
                created_by: actor,
            })
            .get_result(conn)
    })
}

// sync-audit-only: user phone is per-(user,workspace) contact data (audited); folds into the user sync payload in a later phase
pub fn update_phone(
    conn: &mut DbConnection,
    id: i32,
    user_uuid: Uuid,
    input: &UserPhoneInput,
) -> QueryResult<UserPhoneNumber> {
    conn.transaction(|conn| {
        if input.is_primary {
            clear_primary_phones(conn, user_uuid)?;
        }
        diesel::update(user_phone_numbers::table.find(id))
            .set((
                user_phone_numbers::phone.eq(input.phone.clone()),
                user_phone_numbers::phone_type.eq(input.phone_type.clone()),
                user_phone_numbers::is_primary.eq(input.is_primary),
                user_phone_numbers::label.eq(input.label.clone()),
            ))
            .get_result(conn)
    })
}

// sync-audit-only: user phone is per-(user,workspace) contact data (audited); folds into the user sync payload in a later phase
pub fn delete_phone(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(user_phone_numbers::table.find(id)).execute(conn)
}

// ---- Addresses (multi-valued, typed, workspace-scoped) ---------------------

pub fn list_addresses(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<Vec<UserAddress>> {
    user_addresses::table
        .filter(user_addresses::user_uuid.eq(user_uuid))
        .order((user_addresses::is_primary.desc(), user_addresses::id.asc()))
        .load(conn)
}

pub fn get_address(conn: &mut DbConnection, id: i32) -> QueryResult<Option<UserAddress>> {
    user_addresses::table.find(id).first(conn).optional()
}

fn clear_primary_addresses(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<usize> {
    diesel::update(
        user_addresses::table
            .filter(user_addresses::user_uuid.eq(user_uuid))
            .filter(user_addresses::is_primary.eq(true)),
    )
    .set(user_addresses::is_primary.eq(false))
    .execute(conn)
}

// sync-audit-only: user address is per-(user,workspace) contact data (audited); folds into the user sync payload in a later phase
pub fn create_address(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    input: &UserAddressInput,
    source: Option<String>,
    actor: Option<Uuid>,
) -> QueryResult<UserAddress> {
    conn.transaction(|conn| {
        if input.is_primary {
            clear_primary_addresses(conn, user_uuid)?;
        }
        diesel::insert_into(user_addresses::table)
            .values(NewUserAddress {
                user_uuid,
                address_type: input.address_type.clone(),
                is_primary: input.is_primary,
                street: input.street.clone(),
                city: input.city.clone(),
                region: input.region.clone(),
                postal_code: input.postal_code.clone(),
                country: input.country.clone(),
                source,
                label: input.label.clone(),
                created_by: actor,
            })
            .get_result(conn)
    })
}

// sync-audit-only: user address is per-(user,workspace) contact data (audited); folds into the user sync payload in a later phase
pub fn update_address(
    conn: &mut DbConnection,
    id: i32,
    user_uuid: Uuid,
    input: &UserAddressInput,
) -> QueryResult<UserAddress> {
    conn.transaction(|conn| {
        if input.is_primary {
            clear_primary_addresses(conn, user_uuid)?;
        }
        diesel::update(user_addresses::table.find(id))
            .set((
                user_addresses::address_type.eq(input.address_type.clone()),
                user_addresses::is_primary.eq(input.is_primary),
                user_addresses::street.eq(input.street.clone()),
                user_addresses::city.eq(input.city.clone()),
                user_addresses::region.eq(input.region.clone()),
                user_addresses::postal_code.eq(input.postal_code.clone()),
                user_addresses::country.eq(input.country.clone()),
                user_addresses::label.eq(input.label.clone()),
            ))
            .get_result(conn)
    })
}

// sync-audit-only: user address is per-(user,workspace) contact data (audited); folds into the user sync payload in a later phase
pub fn delete_address(conn: &mut DbConnection, id: i32) -> QueryResult<usize> {
    diesel::delete(user_addresses::table.find(id)).execute(conn)
}
