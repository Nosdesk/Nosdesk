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

/// List saved views for a (scope, scope_id) tuple, restricted to
/// one dataset. The tickets surface keeps using the convenience
/// wrapper `list_for_scope` below; new datasets (assets, users)
/// pass their dataset explicitly so private views don't leak
/// across surfaces for the same user.
pub fn list_for_scope_dataset(
    conn: &mut DbConnection,
    scope: &str,
    scope_id: Option<&str>,
    dataset: &str,
) -> QueryResult<Vec<SavedView>> {
    let mut query = saved_views::table
        .filter(saved_views::scope.eq(scope))
        .filter(saved_views::dataset.eq(dataset))
        .into_boxed();
    if let Some(sid) = scope_id {
        query = query.filter(saved_views::scope_id.eq(sid));
    } else {
        query = query.filter(saved_views::scope_id.is_null());
    }
    query.order(saved_views::name.asc()).load(conn)
}

/// Tickets-only convenience wrapper. Kept so existing call sites
/// (the saved-views handler list path) don't need to thread a
/// dataset argument.
pub fn list_for_scope(
    conn: &mut DbConnection,
    scope: &str,
    scope_id: Option<&str>,
) -> QueryResult<Vec<SavedView>> {
    list_for_scope_dataset(conn, scope, scope_id, "tickets")
}

/// "Pickable" saved views — every view in the current workspace
/// whose viz_type is something other than the default list. Backs
/// the AddWidgetModal "Your saved views" tab where the operator
/// drops a chart onto the dashboard. The RLS policy on saved_views
/// already restricts the result to the active workspace_id, so the
/// query here is just `viz_type <> 'list'`.
pub fn list_pickable(conn: &mut DbConnection) -> QueryResult<Vec<SavedView>> {
    saved_views::table
        .filter(saved_views::viz_type.ne("list"))
        .order(saved_views::name.asc())
        .load(conn)
}

pub fn find_by_uuid(conn: &mut DbConnection, uuid: Uuid) -> QueryResult<Option<SavedView>> {
    saved_views::table
        .filter(saved_views::uuid.eq(uuid))
        .first(conn)
        .optional()
}

// sync-audit-only: Operational / bespoke tables
pub fn create(conn: &mut DbConnection, new: NewSavedView) -> QueryResult<SavedView> {
    diesel::insert_into(saved_views::table)
        .values(&new)
        .get_result(conn)
}

// sync-audit-only: Operational / bespoke tables
pub fn update(
    conn: &mut DbConnection,
    uuid: Uuid,
    patch: SavedViewUpdate,
) -> QueryResult<SavedView> {
    diesel::update(saved_views::table.filter(saved_views::uuid.eq(uuid)))
        .set(&patch)
        .get_result(conn)
}

// sync-audit-only: companion to saved_views::create / update; not a sync aggregate
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
            dataset: "tickets".into(),
            viz_type: "list".into(),
            viz_config: json!({}),
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
    fn list_for_scope_dataset_keeps_datasets_separate() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sv_dataset", UserRole::User);

        let mut ticket_view = private_view_for(user.uuid, "tickets-view");
        ticket_view.dataset = "tickets".into();
        let mut asset_view = private_view_for(user.uuid, "assets-view");
        asset_view.dataset = "assets".into();
        create(&mut conn, ticket_view).unwrap();
        create(&mut conn, asset_view).unwrap();

        let tickets = list_for_scope_dataset(
            &mut conn,
            "private",
            Some(&user.uuid.to_string()),
            "tickets",
        )
        .unwrap();
        let assets =
            list_for_scope_dataset(&mut conn, "private", Some(&user.uuid.to_string()), "assets")
                .unwrap();

        assert_eq!(tickets.len(), 1);
        assert_eq!(tickets[0].name, "tickets-view");
        assert_eq!(assets.len(), 1);
        assert_eq!(assets[0].name, "assets-view");
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
            viz_type: None,
            viz_config: None,
        };
        let updated = update(&mut conn, view.uuid, patch).unwrap();
        assert_eq!(updated.name, "after");
    }

    #[test]
    fn list_pickable_excludes_default_list_views() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "sv_pickable", UserRole::User);

        // Default list view: should be filtered out.
        create(&mut conn, private_view_for(user.uuid, "plain-list")).unwrap();
        // Chart view: should appear in the pickable set.
        let mut chart = private_view_for(user.uuid, "kpi-chart");
        chart.viz_type = "kpi_tile".into();
        chart.viz_config = json!({"metric": "tickets_created"});
        let chart = create(&mut conn, chart).unwrap();

        let pickable = list_pickable(&mut conn).unwrap();
        assert_eq!(pickable.len(), 1);
        assert_eq!(pickable[0].id, chart.id);
    }
}
