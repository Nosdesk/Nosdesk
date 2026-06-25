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
    DirectoryContact, NewUserAddress, NewUserPhoneNumber, NewUserProfile, UserAddress,
    UserAddressInput, UserPhoneInput, UserPhoneNumber, UserProfile, UserProfileInput,
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
            "gender": { "type": "string", "title": "Gender", "enum": ["Male", "Female"], "x-allow-custom": true },
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

/// (user_uuid, custom_fields) for every profile in the workspace. Used to
/// revalidate stored values before applying a schema change.
pub fn list_profile_custom_fields(conn: &mut DbConnection) -> QueryResult<Vec<(Uuid, Value)>> {
    user_profiles::table
        .select((user_profiles::user_uuid, user_profiles::custom_fields))
        .load(conn)
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

// ---- Directory sync surfacing ----------------------------------------------

// sync-audit-only: directory sync writes contact data (audited); folds into the user sync payload in a later phase
/// Apply directory-imported contact fields for a user: the standard profile
/// columns + `office_location` (merged into custom_fields, preserving manual
/// keys) with `directory_synced=true`, and the source='microsoft' phone/address
/// rows (replaced wholesale; manual rows are left untouched). One transaction.
pub fn apply_directory_contact(
    conn: &mut DbConnection,
    user_uuid: Uuid,
    contact: &DirectoryContact,
    actor: Option<Uuid>,
) -> QueryResult<()> {
    conn.transaction(|conn| {
        // Profile: standard cols + office_location into custom_fields (preserve
        // manual keys), flagged directory_synced.
        let existing = get_profile(conn, user_uuid)?;
        let mut cf = existing
            .as_ref()
            .map(|p| p.custom_fields.clone())
            .unwrap_or_else(|| json!({}));
        if let Some(obj) = cf.as_object_mut() {
            match &contact.office_location {
                Some(v) => {
                    obj.insert("office_location".to_string(), json!(v));
                }
                None => {
                    obj.remove("office_location");
                }
            }
        }
        diesel::insert_into(user_profiles::table)
            .values(NewUserProfile {
                user_uuid,
                job_title: contact.job_title.clone(),
                organization: contact.organization.clone(),
                department: contact.department.clone(),
                custom_fields: cf.clone(),
                directory_synced: true,
                created_by: actor,
            })
            .on_conflict((user_profiles::workspace_id, user_profiles::user_uuid))
            .do_update()
            .set((
                user_profiles::job_title.eq(contact.job_title.clone()),
                user_profiles::organization.eq(contact.organization.clone()),
                user_profiles::department.eq(contact.department.clone()),
                user_profiles::custom_fields.eq(cf),
                user_profiles::directory_synced.eq(true),
            ))
            .execute(conn)?;

        // Phones: replace the microsoft-sourced rows, leave manual ones.
        diesel::delete(
            user_phone_numbers::table
                .filter(user_phone_numbers::user_uuid.eq(user_uuid))
                .filter(user_phone_numbers::source.eq("microsoft")),
        )
        .execute(conn)?;
        for (phone, phone_type) in &contact.phones {
            diesel::insert_into(user_phone_numbers::table)
                .values(NewUserPhoneNumber {
                    user_uuid,
                    phone: phone.clone(),
                    phone_type: phone_type.clone(),
                    is_primary: false,
                    source: Some("microsoft".to_string()),
                    label: None,
                    created_by: actor,
                })
                .execute(conn)?;
        }

        // Address: replace the microsoft-sourced row, leave manual ones.
        diesel::delete(
            user_addresses::table
                .filter(user_addresses::user_uuid.eq(user_uuid))
                .filter(user_addresses::source.eq("microsoft")),
        )
        .execute(conn)?;
        if let Some(addr) = &contact.address {
            diesel::insert_into(user_addresses::table)
                .values(NewUserAddress {
                    user_uuid,
                    address_type: "work".to_string(),
                    is_primary: false,
                    street: addr.street.clone(),
                    city: addr.city.clone(),
                    region: addr.region.clone(),
                    postal_code: addr.postal_code.clone(),
                    country: addr.country.clone(),
                    source: Some("microsoft".to_string()),
                    label: None,
                    created_by: actor,
                })
                .execute(conn)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DirectoryAddress, UserPhoneInput};
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn field_schema_defaults_then_overrides() {
        let mut conn = setup_test_connection();
        // Default applies until an admin stores an override.
        let def = get_field_schema(&mut conn).unwrap();
        assert_eq!(def["properties"]["office_location"]["synced"], json!(true));

        let custom = json!({
            "type": "object",
            "properties": { "year_level": { "type": "string", "title": "Year level" } }
        });
        let stored = set_field_schema(&mut conn, 1, &custom, None).unwrap();
        assert_eq!(stored["properties"]["year_level"]["title"], "Year level");
        assert_eq!(get_field_schema(&mut conn).unwrap(), custom);
    }

    #[test]
    fn profile_upsert_roundtrip() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Profile User", "user");
        assert!(get_profile(&mut conn, user.uuid).unwrap().is_none());

        let saved = upsert_profile(
            &mut conn,
            user.uuid,
            &UserProfileInput {
                job_title: Some("Tech".into()),
                organization: Some("Acme".into()),
                department: Some("IT".into()),
                custom_fields: json!({ "gender": "x" }),
            },
            Some(user.uuid),
        )
        .unwrap();
        assert_eq!(saved.job_title.as_deref(), Some("Tech"));
        assert!(!saved.directory_synced);

        let updated = upsert_profile(
            &mut conn,
            user.uuid,
            &UserProfileInput {
                job_title: Some("Lead".into()),
                organization: None,
                department: Some("IT".into()),
                custom_fields: json!({ "gender": "y" }),
            },
            Some(user.uuid),
        )
        .unwrap();
        assert_eq!(updated.job_title.as_deref(), Some("Lead"));
        assert_eq!(updated.organization, None);
        assert_eq!(updated.custom_fields["gender"], "y");
    }

    fn phone(value: &str, ty: &str, primary: bool) -> UserPhoneInput {
        UserPhoneInput {
            phone: value.into(),
            phone_type: ty.into(),
            is_primary: primary,
            label: None,
        }
    }

    #[test]
    fn phone_create_enforces_single_primary() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Phone User", "user");

        create_phone(
            &mut conn,
            user.uuid,
            &phone("111", "work", true),
            None,
            None,
        )
        .unwrap();
        create_phone(
            &mut conn,
            user.uuid,
            &phone("222", "mobile", true),
            None,
            None,
        )
        .unwrap();

        let phones = list_phones(&mut conn, user.uuid).unwrap();
        assert_eq!(phones.len(), 2);
        assert_eq!(
            phones.iter().filter(|p| p.is_primary).count(),
            1,
            "the partial-unique index + clear_primary keep exactly one primary"
        );
        assert_eq!(phones.iter().find(|p| p.is_primary).unwrap().phone, "222");
    }

    #[test]
    fn directory_contact_replaces_synced_keeps_manual() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Sync User", "user");

        // A manually-added phone the sync must never touch.
        create_phone(
            &mut conn,
            user.uuid,
            &phone("manual", "mobile", false),
            None,
            None,
        )
        .unwrap();

        let contact = DirectoryContact {
            job_title: Some("Engineer".into()),
            organization: Some("Acme".into()),
            department: Some("R&D".into()),
            office_location: Some("B12".into()),
            phones: vec![("555".into(), "work".into())],
            address: Some(DirectoryAddress {
                street: Some("1 St".into()),
                city: Some("Town".into()),
                region: None,
                postal_code: None,
                country: None,
            }),
        };
        apply_directory_contact(&mut conn, user.uuid, &contact, None).unwrap();

        let prof = get_profile(&mut conn, user.uuid).unwrap().unwrap();
        assert!(prof.directory_synced);
        assert_eq!(prof.job_title.as_deref(), Some("Engineer"));
        assert_eq!(prof.custom_fields["office_location"], "B12");

        let phones = list_phones(&mut conn, user.uuid).unwrap();
        assert_eq!(phones.len(), 2);
        assert!(phones
            .iter()
            .any(|p| p.phone == "manual" && p.source.is_none()));
        assert!(phones
            .iter()
            .any(|p| p.phone == "555" && p.source.as_deref() == Some("microsoft")));
        assert_eq!(list_addresses(&mut conn, user.uuid).unwrap().len(), 1);

        // Re-sync: the microsoft phone is replaced, the manual one stays, and a
        // now-absent office_location is cleared.
        let contact2 = DirectoryContact {
            phones: vec![("777".into(), "work".into())],
            ..Default::default()
        };
        apply_directory_contact(&mut conn, user.uuid, &contact2, None).unwrap();

        let phones2 = list_phones(&mut conn, user.uuid).unwrap();
        assert_eq!(phones2.len(), 2);
        assert!(phones2.iter().any(|p| p.phone == "manual"));
        assert!(phones2.iter().any(|p| p.phone == "777"));
        assert!(!phones2.iter().any(|p| p.phone == "555"));

        let prof2 = get_profile(&mut conn, user.uuid).unwrap().unwrap();
        assert!(prof2
            .custom_fields
            .get("office_location")
            .map(|v| v.is_null())
            .unwrap_or(true));
    }

    #[test]
    fn list_profile_custom_fields_returns_stored() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "CF User", "user");
        upsert_profile(
            &mut conn,
            user.uuid,
            &UserProfileInput {
                job_title: None,
                organization: None,
                department: None,
                custom_fields: json!({ "gender": "z" }),
            },
            None,
        )
        .unwrap();

        let rows = list_profile_custom_fields(&mut conn).unwrap();
        assert!(rows
            .iter()
            .any(|(u, cf)| *u == user.uuid && cf["gender"] == "z"));
    }
}
