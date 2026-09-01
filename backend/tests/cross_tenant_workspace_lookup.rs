//! Cross-tenant workspace resolution under the production role model.
//!
//! The pre-auth email flows (password reset, invitation accept) resolve a
//! recipient's workspace from `workspace_members`, which is RLS-isolated. The
//! runtime role is NOBYPASSRLS and the request carries no workspace pin at
//! that point, so the lookup must run elevated (`background_run` / BYPASSRLS)
//! or it finds no membership and the flow silently drops the email / 500s.
//!
//! This proves the elevated lookup resolves the workspace while a same-role
//! read scoped to a different workspace cannot see the membership, which is
//! exactly why the previously-unpinned lookups failed in hosted mode.

#![allow(clippy::expect_used)]

use backend::repository::workspaces::{
    add_membership, primary_workspace_for_user, SeatWriteAuthority,
};
use backend::sync::actor::ActorContext;
use backend::sync::session::{background_run, with_actor_context};

mod common;

#[test]
fn cross_tenant_workspace_lookup_requires_elevation() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let (ws2, user_uuid) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "acme-xtenant", "Acme XTenant");
        let user = common::insert_user(&mut conn, "XTenant User");
        (ws, user.uuid)
    };
    assert_ne!(ws2, 1, "test needs a non-bootstrap workspace");

    // Give the user a membership in ws2 only.
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(user_uuid, None).with_workspace(ws2);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                ws2,
                user_uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add ws2 membership");
    }

    // The elevated (BYPASSRLS) lookup resolves the recipient's workspace.
    let resolved = background_run(&pool, "test:xtenant_lookup", |c| {
        primary_workspace_for_user(c, user_uuid)
    })
    .expect("elevated cross-tenant lookup must succeed");
    assert_eq!(resolved, ws2, "elevated lookup finds the ws2 membership");

    // A same-role read scoped to workspace 1 cannot see the ws2 membership;
    // this is the failure mode the plain unpinned lookup hit in hosted mode.
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(user_uuid, None).with_workspace(1);
        let scoped = with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            Ok(primary_workspace_for_user(c, user_uuid).ok())
        })
        .expect("scoped lookup runs");
        assert_eq!(
            scoped, None,
            "ws2 membership is invisible when scoped to ws1"
        );
    }
}
