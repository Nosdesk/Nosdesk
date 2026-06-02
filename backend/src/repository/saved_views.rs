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

/// "Pickable" saved views for the AddWidgetModal "Your saved views"
/// tab: every chart-backed view (viz_type != 'list') the caller can
/// see. The visibility rules mirror the existing list path —
/// workspace-scope views are world-readable inside the workspace,
/// private-scope views are caller-only. Earlier revisions of this
/// function returned every workspace row regardless of scope; that
/// leaked other users' private chart views into the picker.
pub fn list_pickable(conn: &mut DbConnection, user_uuid: Uuid) -> QueryResult<Vec<SavedView>> {
    let user_id = user_uuid.to_string();
    // Two predicates ORed inside one query so the result stays
    // sorted by name once — equivalent to UNION ALL + ORDER BY, but
    // expressible in the Diesel typed builder without `union`.
    saved_views::table
        .filter(saved_views::viz_type.ne("list"))
        .filter(
            saved_views::scope.eq("workspace").or(saved_views::scope
                .eq("private")
                .and(saved_views::scope_id.eq(user_id))),
        )
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

        let pickable = list_pickable(&mut conn, user.uuid).unwrap();
        assert_eq!(pickable.len(), 1);
        assert_eq!(pickable[0].id, chart.id);
    }

    #[test]
    fn list_pickable_hides_other_users_private_views() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "sv_alice", UserRole::User);
        let bob = TestFixtures::create_user(&mut conn, "sv_bob", UserRole::User);

        // Alice's private chart view: should NOT be visible to Bob.
        let mut alice_chart = private_view_for(alice.uuid, "alice-secret");
        alice_chart.viz_type = "kpi_tile".into();
        create(&mut conn, alice_chart).unwrap();

        // Bob's own chart view: should be visible.
        let mut bob_chart = private_view_for(bob.uuid, "bob-own");
        bob_chart.viz_type = "kpi_tile".into();
        let bob_chart = create(&mut conn, bob_chart).unwrap();

        let pickable = list_pickable(&mut conn, bob.uuid).unwrap();
        assert_eq!(
            pickable.len(),
            1,
            "Bob should only see his own private chart views"
        );
        assert_eq!(pickable[0].id, bob_chart.id);
    }

    #[test]
    fn list_pickable_includes_workspace_scope_views() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "sv_admin", UserRole::Admin);
        let viewer = TestFixtures::create_user(&mut conn, "sv_viewer", UserRole::User);

        // Workspace-scope chart: visible to every workspace member.
        let workspace_chart = NewSavedView {
            scope: "workspace".into(),
            scope_id: None,
            name: "ws-chart".into(),
            shape: json!({"type": "list"}),
            filter: json!({"predicate": {"combinator": "AND", "children": []}}),
            created_by: admin.uuid,
            dataset: "tickets".into(),
            viz_type: "line".into(),
            viz_config: json!({}),
        };
        let created = create(&mut conn, workspace_chart).unwrap();

        let pickable = list_pickable(&mut conn, viewer.uuid).unwrap();
        assert!(
            pickable.iter().any(|v| v.id == created.id),
            "workspace-scope chart should be visible to any workspace member"
        );
    }
}
