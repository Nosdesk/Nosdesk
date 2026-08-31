//! The membership gate is the primary tenant-authorization boundary once the
//! workspace stops being Host-derived (the v1.1 single-origin agent app, where
//! the workspace is a client selection). Before then it was defense-in-depth.
//!
//! `require_workspace_membership` is the shared fail-closed check the request
//! middleware AND the out-of-band auth paths (SSE stream, collab WS) all call,
//! since those authenticate outside the middleware and the RLS GUC is pinned to
//! the selected workspace *before* any check runs. So a missed gate is a
//! cross-tenant breach, not a degraded query.
//!
//! These prove the invariants the gate must hold:
//!  - a member of the pinned workspace passes,
//!  - a non-member is denied with 403 (not 500, not a silent pass),
//!  - the check reads correctly even on a raw pooled connection whose GUC is
//!    cleared (the SSE/collab entry condition), because it pins via the actor,
//!  - a member of a *different* workspace is still denied (the cross-tenant
//!    case the single-origin app exposes).

#![allow(clippy::expect_used)]

use backend::middleware::cookie_auth::require_workspace_membership;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn status_of(err: &actix_web::Error) -> u16 {
    err.as_response_error().status_code().as_u16()
}

#[test]
fn member_passes_non_member_gets_403() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    let (ws, member_uuid, stranger_uuid) = {
        let mut conn = pool.get().expect("conn");
        let ws = common::mint_workspace(&mut conn, "acme-gate", "Acme Gate");
        let member = common::insert_user(&mut conn, "Gate Member");
        let stranger = common::insert_user(&mut conn, "Gate Stranger");
        (ws, member.uuid, stranger.uuid)
    };
    assert_ne!(ws, 1, "test needs a non-bootstrap workspace");

    // Grant the member a row in `ws` only. The stranger gets none.
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(member_uuid, None).with_workspace(ws);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                ws,
                member_uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add membership");
    }

    // A raw pooled connection with the GUC cleared (the SSE/collab entry
    // condition): the member still resolves because the check pins via the
    // actor, not the ambient request GUC.
    {
        let mut conn = pool.get().expect("conn");
        require_workspace_membership(&mut conn, ws, member_uuid)
            .expect("member of the pinned workspace must pass the gate");
    }

    // The stranger is denied, and specifically with 403 (a tenant-boundary
    // refusal), not 500 (a lookup failure that could be retried or ignored).
    {
        let mut conn = pool.get().expect("conn");
        let err = require_workspace_membership(&mut conn, ws, stranger_uuid)
            .expect_err("non-member must be denied");
        assert_eq!(status_of(&err), 403, "non-member must get 403, not {err}");
    }
}

#[test]
fn member_of_another_workspace_is_denied() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    // A user who is a legitimate member of ws_a, attempting to select ws_b.
    // This is the exact cross-tenant case the single-origin app exposes: the
    // session is valid, the workspace selection is not.
    let (ws_a, ws_b, user_uuid) = {
        let mut conn = pool.get().expect("conn");
        let ws_a = common::mint_workspace(&mut conn, "tenant-a", "Tenant A");
        let ws_b = common::mint_workspace(&mut conn, "tenant-b", "Tenant B");
        let user = common::insert_user(&mut conn, "A Member");
        (ws_a, ws_b, user.uuid)
    };

    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(user_uuid, None).with_workspace(ws_a);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                ws_a,
                user_uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add ws_a membership");
    }

    {
        let mut conn = pool.get().expect("conn");
        require_workspace_membership(&mut conn, ws_a, user_uuid)
            .expect("member passes for their own workspace");
    }

    {
        let mut conn = pool.get().expect("conn");
        let err = require_workspace_membership(&mut conn, ws_b, user_uuid)
            .expect_err("a member of ws_a must not pass for ws_b");
        assert_eq!(status_of(&err), 403, "cross-tenant selection must get 403");
    }
}
