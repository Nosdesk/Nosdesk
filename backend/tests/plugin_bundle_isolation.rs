//! `serve_bundle` (the sandbox `/bundle` route) reads the plugin under the
//! token's workspace pin (RLS), exactly as `run_in_workspace` does. So a bundle
//! token scoped to workspace A cannot serve workspace B's plugin, even if its
//! `plugin_uuid` were pointed there: the pinned read returns nothing. Combined
//! with the mint endpoint only issuing tokens for plugins in the caller's own
//! (RLS-scoped) workspace, cross-tenant bundle access is closed at both ends.
//!
//! This mirrors the handler's read (`with_actor_context` is the core of
//! `run_in_workspace`) against the two-workspace fixture's seeded plugins.

#![allow(clippy::expect_used)]

use uuid::Uuid;

use backend::repository::plugins as plugin_repo;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

/// The plugin `serve_bundle` would see for a token scoped to `ws_id`.
fn read_pinned(
    conn: &mut backend::db::DbConnection,
    ws_id: i32,
    plugin_uuid: Uuid,
) -> diesel::QueryResult<backend::models::Plugin> {
    let actor = ActorContext::system("test:plugin_bundle_read").with_workspace(ws_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        plugin_repo::get_plugin_by_uuid(c, plugin_uuid)
    })
}

#[test]
fn a_token_cannot_serve_another_workspaces_bundle() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let ws = common::seed_two_workspaces(&mut conn);

    // A token scoped to workspace B serves B's plugin (the happy path).
    assert!(
        read_pinned(&mut conn, ws.b.workspace_id, ws.b.plugin_uuid).is_ok(),
        "workspace B's own token must read B's plugin"
    );

    // A token scoped to workspace A, pointed at B's plugin uuid, reads nothing:
    // RLS scopes the pinned read to A, where B's plugin does not exist.
    assert!(
        matches!(
            read_pinned(&mut conn, ws.a.workspace_id, ws.b.plugin_uuid),
            Err(diesel::result::Error::NotFound)
        ),
        "a workspace-A pin must not read workspace B's plugin"
    );

    // Sanity: the two seeded plugins are distinct.
    assert_ne!(ws.a.plugin_uuid, ws.b.plugin_uuid);
}
