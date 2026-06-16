//! Per-workspace site_settings under the production role model.
//!
//! Before this fix site_settings was capped to a single id=1 row, so a
//! non-bootstrap workspace's read was RLS-filtered to nothing and
//! /api/feature-flags 500'd. This proves a non-bootstrap workspace resolves
//! its own settings (no error), lazily creates its row on first access, and
//! is isolated from workspace 1 — exercised under `nosdesk_app` (RLS
//! enforced via with_actor_context) with real commits.

#![allow(clippy::expect_used)]

use backend::repository::feature_flags;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn resolve(
    pool: &backend::db::Pool,
    workspace_id: i32,
    user_uuid: uuid::Uuid,
) -> serde_json::Value {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::user(user_uuid, None).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        feature_flags::resolve_for_user(c, &user_uuid)
    })
    .expect("resolve_for_user must not error for any workspace")
}

#[test]
fn each_workspace_resolves_and_isolates_its_own_flags() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let (ws2, user_uuid) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "acme2", "Acme 2");
        let user = common::insert_user(&mut conn, "Flags User");
        (ws, user.uuid)
    };
    assert_ne!(ws2, 1, "test needs a non-bootstrap workspace");

    // Resolving in the fresh workspace must NOT 500 (the reported bug) — it
    // lazily creates the workspace's settings row and returns defaults.
    let initial = resolve(&pool, ws2, user_uuid);
    assert!(initial.is_object());
    assert!(initial.get("projects_v2").is_none());

    // Set a workspace flag in ws2, then it resolves there.
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(user_uuid, None).with_workspace(ws2);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            feature_flags::set_workspace_flag(c, "projects_v2", Some(serde_json::json!(true)))
        })
        .expect("set_workspace_flag");
    }
    assert_eq!(
        resolve(&pool, ws2, user_uuid).get("projects_v2"),
        Some(&serde_json::json!(true)),
        "the flag set in ws2 resolves in ws2"
    );

    // Isolation: workspace 1 must NOT see ws2's flag.
    assert!(
        resolve(&pool, 1, user_uuid).get("projects_v2").is_none(),
        "ws2's workspace flag must not leak into workspace 1"
    );
}
