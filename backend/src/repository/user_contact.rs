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
use crate::models::{NewUserProfile, UserProfile, UserProfileInput};
use crate::schema::{user_field_schema, user_profiles};

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
