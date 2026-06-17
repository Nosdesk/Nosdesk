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
use backend::repository::workspaces::{add_membership, find_by_id};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

fn claims_for(user_uuid: uuid::Uuid) -> Claims {
    Claims {
        sub: user_uuid.to_string(),
        name: "Selector".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
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

    // Two workspaces; a user who is a member of `acme` only.
    let (acme_id, acme_uuid, member_uuid, stranger_uuid) = {
        let mut conn = pool.get().expect("conn");
        let acme = common::mint_workspace(&mut conn, "acme-sel", "Acme Sel");
        let _other = common::mint_workspace(&mut conn, "other-sel", "Other Sel");
        let member = common::insert_user(&mut conn, "Sel Member");
        let stranger = common::insert_user(&mut conn, "Sel Stranger");
        let acme_uuid = context_of(&mut conn, acme).workspace_uuid;
        (acme, acme_uuid, member.uuid, stranger.uuid)
    };
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(member_uuid, None).with_workspace(acme_id);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(c, acme_id, member_uuid, "member")?;
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
            .insert_header((WORKSPACE_SELECTION_HEADER, acme_uuid.to_string()))
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
            .insert_header((WORKSPACE_SELECTION_HEADER, acme_uuid.to_string()))
            .to_srv_request();
        let err = enforce_workspace_membership(&req, &mut conn, &claims_for(stranger_uuid))
            .expect_err("non-member selection must be denied");
        assert_eq!(status_of(&err), 403, "non-member selection must be 403");
    }

    // Unknown workspace uuid: 403, indistinguishable from non-member (no leak).
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, uuid::Uuid::new_v4().to_string()))
            .to_srv_request();
        let err = enforce_workspace_membership(&req, &mut conn, &claims_for(member_uuid))
            .expect_err("unknown workspace must be denied");
        assert_eq!(status_of(&err), 403, "unknown workspace must be 403");
    }

    // Malformed header: 400, a client error distinct from the 403 boundary.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, "not-a-uuid"))
            .to_srv_request();
        let err = enforce_workspace_membership(&req, &mut conn, &claims_for(member_uuid))
            .expect_err("malformed selection header must be rejected");
        assert_eq!(status_of(&err), 400, "malformed header must be 400");
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

    // --- Selection mode OFF: header is ignored entirely ---
    std::env::remove_var("NOSDESK_WORKSPACE_SELECTION");
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, acme_uuid.to_string()))
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
