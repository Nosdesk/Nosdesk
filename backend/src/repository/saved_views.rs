//! Saved-view CRUD.
//!
//! Three scopes:
//! - `workspace` — visible to every workspace member, edits gated
//!   to admins at the handler.
//! - `project` — visible to anyone with project read access; edits
//!   gated to project members.
//! - `private` — visible only to the creator.
//!
//! Saved views are user-configurable workspace state, not
//! collaborative content. They stay on the audit-only allowlist —
//! no `sync_actions` emit; the lint test is updated alongside.
//!
//! History: earlier revisions carried `is_default` (with a partial
//! unique index per scope) and `archived_at` (soft delete). Both
//! dropped 2026-05-09 — see `models::SavedView` for the rationale.

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NewSavedView, SavedView, SavedViewUpdate};
use crate::schema::saved_views;

/// List every saved view for the given scope tuple. The most common
/// call site — render the saved-views picker for a route.
pub fn list_for_scope(
    conn: &mut DbConnection,
    scope: &str,
    scope_id: Option<&str>,
) -> QueryResult<Vec<SavedView>> {
    let mut query = saved_views::table
        .filter(saved_views::scope.eq(scope))
        .into_boxed();
    if let Some(sid) = scope_id {
        query = query.filter(saved_views::scope_id.eq(sid));
    } else {
        query = query.filter(saved_views::scope_id.is_null());
    }
    query.order(saved_views::name.asc()).load(conn)
}

pub fn find_by_uuid(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<Option<SavedView>> {
    saved_views::table
        .filter(saved_views::uuid.eq(uuid))
        .first(conn)
        .optional()
}

pub fn create(conn: &mut DbConnection, new: NewSavedView) -> QueryResult<SavedView> {
    diesel::insert_into(saved_views::table)
        .values(&new)
        .get_result(conn)
}

pub fn update(
    conn: &mut DbConnection,
    uuid: Uuid,
    patch: SavedViewUpdate,
) -> QueryResult<SavedView> {
    diesel::update(saved_views::table.filter(saved_views::uuid.eq(uuid)))
        .set(&patch)
        .get_result(conn)
}

/// Hard delete. The view's row is removed; any in-flight `?view=<uuid>`
/// URL will fall through the resolution chain to the workspace
/// default or the built-in `MY_OPEN_VIEW`.
pub fn delete(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<usize> {
    diesel::delete(saved_views::table.filter(saved_views::uuid.eq(uuid))).execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use serde_json::json;

    fn private_view_for(user_uuid: Uuid, name: &str) -> NewSavedView {
        NewSavedView {
            scope: "private".into(),
            scope_id: Some(user_uuid.to_string()),
            name: name.into(),
            shape: json!({"type": "list"}),
            filter: json!({"predicate": {"combinator": "AND", "children": []}}),
            created_by: user_uuid,
        }
    }

    #[test]
    fn create_and_list_round_trip() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sv_create", UserRole::User);

        let one = create(&mut conn, private_view_for(user.uuid, "alpha")).unwrap();
        let two = create(&mut conn, private_view_for(user.uuid, "beta")).unwrap();

        let listed = list_for_scope(&mut conn, "private", Some(&user.uuid.to_string())).unwrap();
        assert_eq!(listed.len(), 2);
        // Alphabetical order per the repo's ORDER BY name ASC.
        assert_eq!(listed[0].id, one.id);
        assert_eq!(listed[1].id, two.id);
    }

    #[test]
    fn delete_removes_the_row() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sv_delete", UserRole::User);

        let live = create(&mut conn, private_view_for(user.uuid, "live")).unwrap();
        let dead = create(&mut conn, private_view_for(user.uuid, "dead")).unwrap();
        let removed = delete(&mut conn, dead.uuid).unwrap();
        assert_eq!(removed, 1);

        let listed = list_for_scope(&mut conn, "private", Some(&user.uuid.to_string())).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, live.id);
    }

    #[test]
    fn update_changes_the_name() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sv_update", UserRole::User);
        let view = create(&mut conn, private_view_for(user.uuid, "before")).unwrap();

        let patch = SavedViewUpdate {
            name: Some("after".into()),
            shape: None,
            filter: None,
        };
        let updated = update(&mut conn, view.uuid, patch).unwrap();
        assert_eq!(updated.name, "after");
    }
}
