//! Smoke tests for the two-workspace isolation fixture (C0).
//!
//! Proves the fixture seeds two independent workspaces with their own
//! members, an overlapping-event webhook, and a plugin — and that the
//! seeded rows are RLS-isolated: a connection pinned to workspace A
//! cannot see workspace B's tenant rows and vice versa. This is the
//! baseline isolation guarantee the C1/C2/C3 fixes build on.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use uuid::Uuid;

use backend::sync::actor::ActorContext;
use backend::sync::session::{background_run, with_actor_context};

mod common;

/// Webhook uuids visible from a connection pinned to `workspace_id`.
/// Reads under the workspace-pinned actor context, so RLS scopes the
/// result to that workspace exactly as a real request would.
fn visible_webhook_uuids(conn: &mut backend::db::DbConnection, workspace_id: i32) -> Vec<Uuid> {
    use backend::schema::webhooks;
    let actor = ActorContext::system("test:fixture_read").with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        webhooks::table.select(webhooks::uuid).load::<Uuid>(c)
    })
    .expect("read webhooks pinned to workspace")
}

/// Plugin uuids visible from a connection pinned to `workspace_id`.
fn visible_plugin_uuids(conn: &mut backend::db::DbConnection, workspace_id: i32) -> Vec<Uuid> {
    use backend::schema::plugins;
    let actor = ActorContext::system("test:fixture_read").with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        plugins::table.select(plugins::uuid).load::<Uuid>(c)
    })
    .expect("read plugins pinned to workspace")
}

/// Member user_uuids visible from a connection pinned to `workspace_id`.
fn visible_member_uuids(conn: &mut backend::db::DbConnection, workspace_id: i32) -> Vec<Uuid> {
    use backend::schema::workspace_members;
    let actor = ActorContext::system("test:fixture_read").with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        workspace_members::table
            .select(workspace_members::user_uuid)
            .load::<Uuid>(c)
    })
    .expect("read members pinned to workspace")
}

#[test]
fn fixture_seeds_two_distinct_workspaces() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();

    let ws = common::seed_two_workspaces(&mut conn);

    // Two distinct, non-bootstrap workspaces.
    assert_ne!(ws.a.workspace_id, ws.b.workspace_id);
    assert_ne!(ws.a.workspace_id, 1);
    assert_ne!(ws.b.workspace_id, 1);
    assert_ne!(ws.a.workspace_uuid, ws.b.workspace_uuid);

    // Each carries its own admin + member + webhook + plugin.
    assert_ne!(ws.a.admin_uuid, ws.b.admin_uuid);
    assert_ne!(ws.a.member_uuid, ws.b.member_uuid);
    assert_ne!(ws.a.admin_uuid, ws.a.member_uuid);
    assert_ne!(ws.a.webhook_id, ws.b.webhook_id);
    assert_ne!(ws.a.plugin_id, ws.b.plugin_id);

    // Both workspaces exist as active rows (global-table read).
    for seed in [&ws.a, &ws.b] {
        let found = backend::repository::workspaces::find_by_id(&mut conn, seed.workspace_id)
            .expect("find workspace")
            .expect("workspace row present");
        assert_eq!(found.uuid, seed.workspace_uuid);
    }
}

#[test]
fn members_are_scoped_to_their_workspace() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();

    let ws = common::seed_two_workspaces(&mut conn);

    let a_members = visible_member_uuids(&mut conn, ws.a.workspace_id);
    assert!(a_members.contains(&ws.a.admin_uuid), "A sees its admin");
    assert!(a_members.contains(&ws.a.member_uuid), "A sees its member");
    assert!(
        !a_members.contains(&ws.b.admin_uuid),
        "A must NOT see B's admin"
    );
    assert!(
        !a_members.contains(&ws.b.member_uuid),
        "A must NOT see B's member"
    );

    let b_members = visible_member_uuids(&mut conn, ws.b.workspace_id);
    assert!(b_members.contains(&ws.b.admin_uuid), "B sees its admin");
    assert!(
        !b_members.contains(&ws.a.member_uuid),
        "B must NOT see A's member"
    );
}

#[test]
fn webhooks_and_plugins_are_rls_isolated() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();

    let ws = common::seed_two_workspaces(&mut conn);

    // Overlapping event subscription: both webhooks target the same
    // event type. Only the workspace predicate keeps them apart.
    // (Sanity: the fixture wired the shared event.)
    assert_eq!(common::FIXTURE_WEBHOOK_EVENT, "ticket.created");

    // Webhooks: A sees only A's, B sees only B's.
    let a_hooks = visible_webhook_uuids(&mut conn, ws.a.workspace_id);
    assert!(a_hooks.contains(&ws.a.webhook_uuid), "A sees its webhook");
    assert!(
        !a_hooks.contains(&ws.b.webhook_uuid),
        "A must NOT see B's webhook (cross-tenant fan-out guard)"
    );

    let b_hooks = visible_webhook_uuids(&mut conn, ws.b.workspace_id);
    assert!(b_hooks.contains(&ws.b.webhook_uuid), "B sees its webhook");
    assert!(
        !b_hooks.contains(&ws.a.webhook_uuid),
        "B must NOT see A's webhook"
    );

    // Plugins: same isolation.
    let a_plugins = visible_plugin_uuids(&mut conn, ws.a.workspace_id);
    assert!(a_plugins.contains(&ws.a.plugin_uuid), "A sees its plugin");
    assert!(
        !a_plugins.contains(&ws.b.plugin_uuid),
        "A must NOT see B's plugin"
    );

    let b_plugins = visible_plugin_uuids(&mut conn, ws.b.workspace_id);
    assert!(b_plugins.contains(&ws.b.plugin_uuid), "B sees its plugin");
    assert!(
        !b_plugins.contains(&ws.a.plugin_uuid),
        "B must NOT see A's plugin"
    );
}

/// C1 (Critical): the outbox dispatcher resolves webhook subscribers under a
/// BYPASSRLS background session, so RLS gives it no cover. The `workspace_id`
/// predicate in `get_webhooks_for_event` is the only thing stopping an event in
/// one workspace from fanning out to another workspace's webhooks subscribed to
/// the same event type.
#[test]
fn webhook_fanout_is_workspace_scoped_under_bypass() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);

    let ws = {
        let mut conn = pool.get().expect("conn");
        common::seed_two_workspaces(&mut conn)
    };

    // Sanity: a BYPASSRLS session with no predicate sees BOTH workspaces'
    // webhooks — proving RLS does not scope this path, so the predicate must.
    let all_ids: Vec<i32> = background_run(&pool, "test:webhook_all", |c| {
        use backend::schema::webhooks;
        webhooks::table.select(webhooks::id).load::<i32>(c)
    })
    .expect("bypass read of all webhooks");
    assert!(
        all_ids.contains(&ws.a.webhook_id) && all_ids.contains(&ws.b.webhook_id),
        "bypass session sees both workspaces' webhooks (no RLS cover on the drain path)"
    );

    // A's event resolves ONLY A's webhook.
    let a_ids: Vec<i32> = {
        let wsid = ws.a.workspace_id;
        background_run(&pool, "test:webhook_a", move |c| {
            backend::repository::webhooks::get_webhooks_for_event(
                c,
                wsid,
                common::FIXTURE_WEBHOOK_EVENT,
            )
        })
        .expect("lookup A")
        .iter()
        .map(|w| w.id)
        .collect()
    };
    assert!(
        a_ids.contains(&ws.a.webhook_id),
        "A's event finds A's webhook"
    );
    assert!(
        !a_ids.contains(&ws.b.webhook_id),
        "A's event must NOT fan out to B's webhook (cross-tenant leak)"
    );

    // B's event resolves ONLY B's webhook.
    let b_ids: Vec<i32> = {
        let wsid = ws.b.workspace_id;
        background_run(&pool, "test:webhook_b", move |c| {
            backend::repository::webhooks::get_webhooks_for_event(
                c,
                wsid,
                common::FIXTURE_WEBHOOK_EVENT,
            )
        })
        .expect("lookup B")
        .iter()
        .map(|w| w.id)
        .collect()
    };
    assert!(
        b_ids.contains(&ws.b.webhook_id),
        "B's event finds B's webhook"
    );
    assert!(
        !b_ids.contains(&ws.a.webhook_id),
        "B's event must NOT fan out to A's webhook"
    );
}
