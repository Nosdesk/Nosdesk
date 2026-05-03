//! Saved-view CRUD.
//!
//! Three scopes:
//! - `workspace` — visible to every workspace member, edits gated
//!   to admins at the handler.
//! - `project` — visible to anyone with project read access; edits
//!   gated to project members.
//! - `private` — visible only to the creator.
//!
//! `is_default` is a per-scope flag enforced by a partial unique
//! index. Setting a new default atomically clears the previous one
//! through a transaction; the helper exposed here does the
//! demote-then-promote dance so callers can't forget.
//!
//! Saved views are user-configurable workspace state, not
//! collaborative content. They stay on the audit-only allowlist —
//! no `sync_actions` emit; the lint test is updated alongside.

use chrono::Utc;
use diesel::prelude::*;
use diesel::Connection;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewSavedView, SavedView, SavedViewUpdate};
use crate::schema::saved_views;

/// List every non-archived saved view for the given scope tuple.
/// The most common call site — render the saved-views picker for
/// a route.
pub fn list_for_scope(
    conn: &mut DbConnection,
    scope: &str,
    scope_id: Option<&str>,
) -> QueryResult<Vec<SavedView>> {
    let mut query = saved_views::table
        .filter(saved_views::archived_at.is_null())
        .filter(saved_views::scope.eq(scope))
        .into_boxed();
    if let Some(sid) = scope_id {
        query = query.filter(saved_views::scope_id.eq(sid));
    } else {
        query = query.filter(saved_views::scope_id.is_null());
    }
    query
        .order(saved_views::name.asc())
        .load(conn)
}

pub fn find_by_uuid(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<Option<SavedView>> {
    saved_views::table
        .filter(saved_views::uuid.eq(uuid))
        .first(conn)
        .optional()
}

pub fn create(conn: &mut DbConnection, new: NewSavedView) -> QueryResult<SavedView> {
    conn.transaction(|conn| {
        // If the row is being created as default, demote the
        // existing default for the same scope tuple in the same
        // transaction so the partial unique index doesn't reject
        // the insert.
        if new.is_default {
            demote_default_for_scope(conn, &new.scope, new.scope_id.as_deref())?;
        }
        diesel::insert_into(saved_views::table)
            .values(&new)
            .get_result(conn)
    })
}

pub fn update(
    conn: &mut DbConnection,
    uuid: Uuid,
    patch: SavedViewUpdate,
) -> QueryResult<SavedView> {
    conn.transaction(|conn| {
        // Default promotion goes through the same demote-old-default
        // path the create flow uses. Pull the row first so we know
        // its scope without trusting the patch.
        if matches!(patch.is_default, Some(true)) {
            let row: SavedView = saved_views::table
                .filter(saved_views::uuid.eq(uuid))
                .first(conn)?;
            demote_default_for_scope(conn, &row.scope, row.scope_id.as_deref())?;
        }
        diesel::update(saved_views::table.filter(saved_views::uuid.eq(uuid)))
            .set(&patch)
            .get_result(conn)
    })
}

/// Soft-archive — sets `archived_at = NOW()`. Existing
/// `?view=<uuid>` URLs surface a friendly "this view was archived"
/// message at the route boundary; the row stays in the DB so
/// audit and recovery paths work.
pub fn archive(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<SavedView> {
    diesel::update(saved_views::table.filter(saved_views::uuid.eq(uuid)))
        .set((
            saved_views::archived_at.eq(Some(Utc::now())),
            saved_views::is_default.eq(false),
        ))
        .get_result(conn)
}

fn demote_default_for_scope(
    conn: &mut DbConnection,
    scope: &str,
    scope_id: Option<&str>,
) -> QueryResult<()> {
    let mut query = diesel::update(
        saved_views::table
            .filter(saved_views::is_default.eq(true))
            .filter(saved_views::archived_at.is_null())
            .filter(saved_views::scope.eq(scope)),
    )
    .into_boxed();
    if let Some(sid) = scope_id {
        query = query.filter(saved_views::scope_id.eq(sid));
    } else {
        query = query.filter(saved_views::scope_id.is_null());
    }
    query.set(saved_views::is_default.eq(false)).execute(conn)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use serde_json::json;

    fn private_view_for(user_uuid: Uuid, name: &str, is_default: bool) -> NewSavedView {
        NewSavedView {
            scope: "private".into(),
            scope_id: Some(user_uuid.to_string()),
            name: name.into(),
            shape: json!({"type": "list"}),
            filter: json!({"predicate": {"combinator": "AND", "children": []}}),
            created_by: user_uuid,
            is_default,
        }
    }

    #[test]
    fn promoting_a_new_default_demotes_the_old_one() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sv_promote", UserRole::User);

        let first = create(&mut conn, private_view_for(user.uuid, "first", true)).unwrap();
        assert!(first.is_default);

        let second = create(&mut conn, private_view_for(user.uuid, "second", true)).unwrap();
        assert!(second.is_default);

        let reloaded_first: SavedView = saved_views::table.find(first.id).first(&mut conn).unwrap();
        assert!(!reloaded_first.is_default,
            "creating a new default should have demoted the previous one");
    }

    #[test]
    fn list_for_scope_excludes_archived_rows() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sv_list", UserRole::User);

        let live = create(&mut conn, private_view_for(user.uuid, "live", false)).unwrap();
        let dead = create(&mut conn, private_view_for(user.uuid, "dead", false)).unwrap();
        archive(&mut conn, dead.uuid).unwrap();

        let listed = list_for_scope(&mut conn, "private", Some(&user.uuid.to_string())).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, live.id);
    }
}
