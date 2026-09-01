//! Selection-based workspace resolution (Model C, increment 1).
//!
//! Under `NOSDESK_WORKSPACE_SELECTION` (hosted only) the single-origin agent
//! app names its workspace in the `X-Nosdesk-Workspace` header instead of the
//! Host subdomain. The auth gate (`enforce_workspace_membership`) resolves the
//! header, membership-gates it, and publishes the selection-derived
//! `WorkspaceContext`. This is the security-critical glue: the workspace becomes
//! client-supplied, so the gate is the tenant boundary.
//!
//! Everything here runs in one `#[test]` because it mutates process-wide env;
//! keeping it single means the toggles can't race a sibling test.

#![allow(clippy::expect_used)]

use actix_web::test::TestRequest;
use actix_web::HttpMessage as _;

use backend::extractors::WorkspaceContext;
use backend::middleware::cookie_auth::enforce_workspace_membership;
use backend::middleware::workspace_context::WORKSPACE_SELECTION_HEADER;
use backend::models::Claims;
use backend::repository::workspaces::{add_membership, find_by_id, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn claims_for(user_uuid: uuid::Uuid) -> Claims {
    claims_scoped(user_uuid, "full")
}

fn claims_scoped(user_uuid: uuid::Uuid, scope: &str) -> Claims {
    Claims {
        sub: user_uuid.to_string(),
        name: "Selector".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: scope.to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

fn context_of(conn: &mut backend::db::DbConnection, workspace_id: i32) -> WorkspaceContext {
    let ws = find_by_id(conn, workspace_id)
        .expect("workspace lookup")
        .expect("workspace exists");
    WorkspaceContext {
        workspace_id: ws.id,
        workspace_uuid: ws.uuid,
        slug: ws.slug,
        name: ws.name,
        custom_domain: ws.custom_domain,
        organisation_id: ws.organisation_id,
    }
}

fn status_of(err: &actix_web::Error) -> u16 {
    err.as_response_error().status_code().as_u16()
}

#[test]
fn selection_header_resolves_and_gates() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    // Two workspaces; a user who is a member of `acme` only. The header carries
    // the slug, the way the agent app URL does.
    let (acme_id, member_uuid, stranger_uuid) = {
        let mut conn = pool.get().expect("conn");
        let acme = common::mint_workspace(&mut conn, "acme-sel", "Acme Sel");
        let _other = common::mint_workspace(&mut conn, "other-sel", "Other Sel");
        let member = common::insert_user(&mut conn, "Sel Member");
        let stranger = common::insert_user(&mut conn, "Sel Stranger");
        (acme, member.uuid, stranger.uuid)
    };
    let acme_slug = "acme-sel";
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(member_uuid, None).with_workspace(acme_id);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                acme_id,
                member_uuid,
                "member",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add acme membership");
    }

    // --- Selection mode ON ---
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("NOSDESK_WORKSPACE_SELECTION", "1");

    // Member selecting acme via header: allowed, and the selection-derived
    // context is published for downstream handlers.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, acme_slug))
            .to_srv_request();
        enforce_workspace_membership(&req, &mut conn, &claims_for(member_uuid))
            .expect("member selecting their workspace must pass");
        let published = req
            .extensions()
            .get::<WorkspaceContext>()
            .map(|w| w.workspace_id);
        assert_eq!(
            published,
            Some(acme_id),
            "selection-derived context must be published"
        );
    }

    // Non-member selecting acme: 403.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, acme_slug))
            .to_srv_request();
        let err = enforce_workspace_membership(&req, &mut conn, &claims_for(stranger_uuid))
            .expect_err("non-member selection must be denied");
        assert_eq!(status_of(&err), 403, "non-member selection must be 403");
    }

    // Unknown slug: 403, indistinguishable from non-member (no existence leak).
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, "no-such-workspace"))
            .to_srv_request();
        let err = enforce_workspace_membership(&req, &mut conn, &claims_for(member_uuid))
            .expect_err("unknown workspace must be denied");
        assert_eq!(status_of(&err), 403, "unknown workspace must be 403");
    }

    // Blank header: treated as no selection (not an error). With no Host context
    // either, there is nothing to authorize, so it passes and publishes nothing.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, "   "))
            .to_srv_request();
        enforce_workspace_membership(&req, &mut conn, &claims_for(member_uuid))
            .expect("blank selection header is ignored, nothing to gate");
        assert!(
            req.extensions().get::<WorkspaceContext>().is_none(),
            "blank header must not publish a context"
        );
    }

    // No header in selection mode: falls back to the Host-derived context the
    // middleware would have inserted. Member passes against it.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut()
            .insert(context_of(&mut pool.get().expect("conn"), acme_id));
        enforce_workspace_membership(&req, &mut conn, &claims_for(member_uuid))
            .expect("host-derived fallback still gates members through");
    }

    // Origin wins over a stray selection header. A tenant-origin request (Host
    // resolved to acme, the member's workspace) carries a header naming a
    // DIFFERENT workspace the user is not a member of. If selection were
    // consulted here it would 403 (non-member of other-sel); instead the
    // origin is authoritative, the member passes against acme, and the header
    // is ignored. This is the cross-tenant-confinement invariant: a client
    // cannot override its own origin's tenant via a header.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, "other-sel"))
            .to_srv_request();
        req.extensions_mut()
            .insert(context_of(&mut pool.get().expect("conn"), acme_id));
        enforce_workspace_membership(&req, &mut conn, &claims_for(member_uuid))
            .expect("origin-derived workspace wins; stray selection header is ignored");
        let published = req
            .extensions()
            .get::<WorkspaceContext>()
            .map(|w| w.workspace_id);
        assert_eq!(
            published,
            Some(acme_id),
            "origin-derived context must remain authoritative, not the header"
        );
    }

    // A portal-scope token is refused on the agent surface outright, even for a
    // real member on a resolved origin. The portal is a separate principal
    // realm; its session must never authenticate an agent request.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut()
            .insert(context_of(&mut pool.get().expect("conn"), acme_id));
        let err =
            enforce_workspace_membership(&req, &mut conn, &claims_scoped(member_uuid, "portal"))
                .expect_err("portal-scope token must be denied on the agent surface");
        assert_eq!(
            status_of(&err),
            403,
            "portal-scope token on the agent surface must be 403"
        );
    }

    // --- Selection mode OFF: header is ignored entirely ---
    std::env::remove_var("NOSDESK_WORKSPACE_SELECTION");
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, acme_slug))
            .to_srv_request();
        // No Host-derived context either, so there is nothing to authorize: Ok,
        // and crucially no selection-derived context is published from the header.
        enforce_workspace_membership(&req, &mut conn, &claims_for(stranger_uuid))
            .expect("flag off: header is ignored, no tenant scope to gate");
        assert!(
            req.extensions().get::<WorkspaceContext>().is_none(),
            "flag off must not publish a context from the header"
        );
    }

    std::env::remove_var("NOSDESK_DEPLOYMENT_MODE");
}
