//! Feature flag resolution.
//!
//! Two layers compose at request time:
//!   1. `site_settings.feature_flags` — workspace defaults.
//!   2. `users.feature_flag_overrides` — per-user overrides merged on top.
//!
//! A flag the application doesn't know about is implicitly `false`,
//! and the application is the source of truth for the schema of each
//! flag's value. The resolution layer treats values as opaque JSON.
//!
//! Flag names follow `<feature>_<scope>` convention: `projects_v2`,
//! `triage_inbox`, `ai_summary`. Names that map to a tier-1
//! sync_aggregate use the aggregate's name as the prefix.

use diesel::prelude::*;
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::schema::{site_settings, users};

/// Resolve every flag for a given user, merging workspace defaults
/// with per-user overrides. Returns a flat `{ flag_name: value }`
/// object suitable for shipping to the client.
pub fn resolve_for_user(conn: &mut DbConnection, user_uuid: &Uuid) -> QueryResult<Value> {
    let workspace: Value = site_settings::table
        .find(1)
        .select(site_settings::feature_flags)
        .first(conn)?;

    let user_overrides: Value = users::table
        .find(user_uuid)
        .select(users::feature_flag_overrides)
        .first(conn)?;

    Ok(merge_flags(&workspace, &user_overrides))
}

/// Set the workspace-level value for a single flag. Setting `null`
/// removes the flag from the workspace defaults (clients fall back
/// to the application's code default).
pub fn set_workspace_flag(
    conn: &mut DbConnection,
    flag_name: &str,
    value: Option<Value>,
) -> QueryResult<Value> {
    let mut current: Value = site_settings::table
        .find(1)
        .select(site_settings::feature_flags)
        .first(conn)?;

    apply_patch(&mut current, flag_name, value);

    diesel::update(site_settings::table.find(1))
        .set(site_settings::feature_flags.eq(&current))
        .execute(conn)?;

    Ok(current)
}

/// Set or clear a per-user override for a single flag.
pub fn set_user_override(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    flag_name: &str,
    value: Option<Value>,
) -> QueryResult<Value> {
    let mut current: Value = users::table
        .find(user_uuid)
        .select(users::feature_flag_overrides)
        .first(conn)?;

    apply_patch(&mut current, flag_name, value);

    diesel::update(users::table.find(user_uuid))
        .set(users::feature_flag_overrides.eq(&current))
        .execute(conn)?;

    Ok(current)
}

/// Replace the entire workspace flag map. Used by the admin UI's
/// bulk-edit path; setters above are the per-flag path.
pub fn set_all_workspace_flags(conn: &mut DbConnection, flags: Value) -> QueryResult<Value> {
    if !flags.is_object() {
        return Err(diesel::result::Error::DeserializationError(
            "feature_flags must be a JSON object".into(),
        ));
    }
    diesel::update(site_settings::table.find(1))
        .set(site_settings::feature_flags.eq(&flags))
        .execute(conn)?;
    Ok(flags)
}

fn merge_flags(workspace: &Value, user: &Value) -> Value {
    let mut merged = match workspace {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    if let Value::Object(user_map) = user {
        for (k, v) in user_map {
            merged.insert(k.clone(), v.clone());
        }
    }
    Value::Object(merged)
}

fn apply_patch(current: &mut Value, flag_name: &str, value: Option<Value>) {
    if !current.is_object() {
        *current = Value::Object(Map::new());
    }
    let map = current.as_object_mut().expect("ensured above");
    match value {
        Some(v) => {
            map.insert(flag_name.to_string(), v);
        }
        None => {
            map.remove(flag_name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use serde_json::json;

    #[test]
    fn resolve_returns_workspace_defaults_when_no_user_override() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ff_user", UserRole::User);

        set_workspace_flag(&mut conn, "projects_v2", Some(json!(false))).unwrap();

        let resolved = resolve_for_user(&mut conn, &user.uuid).unwrap();
        assert_eq!(resolved.get("projects_v2"), Some(&json!(false)));
    }

    #[test]
    fn user_override_wins_over_workspace_default() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ff_override", UserRole::User);

        set_workspace_flag(&mut conn, "projects_v2", Some(json!(false))).unwrap();
        set_user_override(&mut conn, &user.uuid, "projects_v2", Some(json!(true))).unwrap();

        let resolved = resolve_for_user(&mut conn, &user.uuid).unwrap();
        assert_eq!(resolved.get("projects_v2"), Some(&json!(true)));
    }

    #[test]
    fn clearing_user_override_falls_back_to_workspace() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ff_clear", UserRole::User);

        set_workspace_flag(&mut conn, "projects_v2", Some(json!(false))).unwrap();
        set_user_override(&mut conn, &user.uuid, "projects_v2", Some(json!(true))).unwrap();
        set_user_override(&mut conn, &user.uuid, "projects_v2", None).unwrap();

        let resolved = resolve_for_user(&mut conn, &user.uuid).unwrap();
        assert_eq!(resolved.get("projects_v2"), Some(&json!(false)));
    }

    #[test]
    fn unknown_flags_are_absent() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ff_absent", UserRole::User);

        let resolved = resolve_for_user(&mut conn, &user.uuid).unwrap();
        assert!(resolved.get("never_set_flag").is_none());
    }

    #[test]
    fn set_all_workspace_flags_replaces_map() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ff_bulk", UserRole::User);

        set_workspace_flag(&mut conn, "old_flag", Some(json!(true))).unwrap();
        set_all_workspace_flags(&mut conn, json!({ "new_flag": false })).unwrap();

        let resolved = resolve_for_user(&mut conn, &user.uuid).unwrap();
        assert!(resolved.get("old_flag").is_none());
        assert_eq!(resolved.get("new_flag"), Some(&json!(false)));
    }
}
