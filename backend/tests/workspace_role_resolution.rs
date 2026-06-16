//! Per-workspace role resolution under the production role model.
//!
//! `workspace_role()` previously read a hardcoded `workspace_id = 1`, so a
//! member of any non-bootstrap workspace resolved to `None` and the
//! derived admin/agent gates failed open-or-shut in hosted multi-tenancy.
//! It now reads the workspace the connection is scoped to (`app.workspace_id`).
//! This proves a member's role resolves in their own workspace and does not
//! leak into another, exercised under `nosdesk_app` (RLS enforced via
//! `with_actor_context`) with real commits.

#![allow(clippy::expect_used)]

use backend::models::WorkspaceRole;
use backend::repository::user_helpers::workspace_role;
use backend::repository::workspaces::add_membership;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn role_in(
    pool: &backend::db::Pool,
    workspace_id: i32,
    user_uuid: uuid::Uuid,
) -> Option<WorkspaceRole> {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::user(user_uuid, None).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        Ok(workspace_role(c, user_uuid))
    })
    .expect("workspace_role must not error")
}

#[test]
fn workspace_role_resolves_in_scoped_workspace_and_isolates() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let (ws2, user_uuid) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "acme-roles", "Acme Roles");
        let user = common::insert_user(&mut conn, "Role User");
        (ws, user.uuid)
    };
    assert_ne!(ws2, 1, "test needs a non-bootstrap workspace");

    // Grant an admin membership in ws2, under ws2's actor context so the
    // write passes RLS and the audit trigger gets the right workspace.
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(user_uuid, None).with_workspace(ws2);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(c, ws2, user_uuid, "admin")?;
            Ok(())
        })
        .expect("add ws2 membership");
    }

    // Resolves as Admin in ws2 (the reported bug: this returned None).
    assert_eq!(
        role_in(&pool, ws2, user_uuid),
        Some(WorkspaceRole::Admin),
        "the ws2 admin membership resolves in ws2"
    );

    // Isolation: the membership does not leak into the bootstrap workspace.
    assert_eq!(
        role_in(&pool, 1, user_uuid),
        None,
        "ws2 membership must not resolve in workspace 1"
    );
}
