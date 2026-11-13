//! A personal API token is bound to the workspace it was minted in.
//!
//! `api_tokens.workspace_id` records that workspace, and
//! `enforce_api_token_workspace` makes it a ceiling: membership in the workspace
//! the request names is necessary but no longer sufficient. Without this, an
//! admin of both A and B could point A's token at B, which defeats the point of
//! minting a token per workspace.
//!
//! Single `#[test]` because the selection-header cases mutate process-wide env;
//! keeping it one test means the toggles cannot race a sibling.

#![allow(clippy::expect_used)]

use actix_web::test::TestRequest;
use actix_web::HttpMessage as _;
use diesel::prelude::*;

use backend::extractors::WorkspaceContext;
use backend::middleware::cookie_auth::enforce_api_token_workspace;
use backend::middleware::workspace_context::WORKSPACE_SELECTION_HEADER;
use backend::models::Claims;
use backend::repository::workspaces::{add_membership, find_by_id, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

mod common;

/// Claims as `try_bearer_auth` builds them for an API token: full scope, no
/// session, and the token's workspace binding.
fn token_claims(user_uuid: uuid::Uuid, bound: uuid::Uuid) -> Claims {
    Claims {
        sub: user_uuid.to_string(),
        name: "Token Owner".to_string(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: Some(bound),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as usize,
        iat: chrono::Utc::now().timestamp() as usize,
        file_path: None,
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
fn api_token_is_refused_outside_the_workspace_it_was_minted_in() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(2);

    // One user who is a member of BOTH workspaces, holding a token minted in A.
    // Membership is deliberately not the thing under test here.
    let (a_id, a_uuid, b_id, outsider_uuid, owner) = {
        let mut conn = pool.get().expect("conn");
        let a = common::mint_workspace(&mut conn, "tok-a", "Token A");
        let b = common::mint_workspace(&mut conn, "tok-b", "Token B");
        let owner = common::insert_user(&mut conn, "Token Owner");
        let outsider = common::insert_user(&mut conn, "Token Outsider");
        let a_uuid = context_of(&mut conn, a).workspace_uuid;
        (a, a_uuid, b, outsider.uuid, owner)
    };
    let owner_uuid = owner.uuid;
    let pool_data = actix_web::web::Data::new(pool.clone());
    for ws_id in [a_id, b_id] {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(owner_uuid, None).with_workspace(ws_id);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(
                c,
                ws_id,
                owner_uuid,
                "admin",
                SeatWriteAuthority::ControlPlane,
            )?;
            Ok(())
        })
        .expect("add membership");
    }

    // --- Host-derived origin (no selection env needed) ---

    // Token bound to A, presented at B's origin, by a member of B. This is the
    // case that passed on membership alone.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(context_of(&mut conn, b_id));
        let err =
            enforce_api_token_workspace(&req, &mut conn, &token_claims(owner_uuid, a_uuid), a_uuid)
                .expect_err("a token bound to A must not authenticate against B");
        assert_eq!(status_of(&err), 403);
        assert!(
            err.to_string().contains("bound to a different workspace"),
            "the refusal must say why: {err}"
        );
    }

    // Same token at A's own origin: allowed.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(context_of(&mut conn, a_id));
        enforce_api_token_workspace(&req, &mut conn, &token_claims(owner_uuid, a_uuid), a_uuid)
            .expect("a token at its own workspace must pass");
        let published = req
            .extensions()
            .get::<WorkspaceContext>()
            .map(|w| w.workspace_id);
        assert_eq!(published, Some(a_id));
    }

    // A token whose holder is not a member of the bound workspace is still
    // refused by the membership gate. The binding is a ceiling, not a grant.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        req.extensions_mut().insert(context_of(&mut conn, a_id));
        let err = enforce_api_token_workspace(
            &req,
            &mut conn,
            &token_claims(outsider_uuid, a_uuid),
            a_uuid,
        )
        .expect_err("a non-member must be refused even at the bound workspace");
        assert_eq!(status_of(&err), 403);
    }

    // No origin and no selection: nothing to compare the binding against, so
    // the request stays unscoped and RLS is the backstop, exactly as it was
    // before the binding existed. The binding must NOT act as a selection here:
    // this same case is a platform admin's cross-workspace call, which has no
    // workspace to be a member of, and pinning the binding would 403 it.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        enforce_api_token_workspace(&req, &mut conn, &token_claims(owner_uuid, a_uuid), a_uuid)
            .expect("an unresolved workspace must fall through, not be gated");
        assert!(
            req.extensions().get::<WorkspaceContext>().is_none(),
            "the binding must not select a workspace the request never named"
        );
    }

    // The same unresolved call by someone who is not a member of the bound
    // workspace also falls through, which is the regression this shape avoids.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default().to_srv_request();
        enforce_api_token_workspace(
            &req,
            &mut conn,
            &token_claims(outsider_uuid, a_uuid),
            a_uuid,
        )
        .expect("a platform-admin style cross-workspace call must not be gated");
    }

    // --- Selection header (single-origin agent app) ---
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("NOSDESK_WORKSPACE_SELECTION", "1");

    // Selecting B with a token bound to A: refused, even though the holder is a
    // member of B.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, "tok-b"))
            .to_srv_request();
        let err =
            enforce_api_token_workspace(&req, &mut conn, &token_claims(owner_uuid, a_uuid), a_uuid)
                .expect_err("selecting another workspace must not widen the binding");
        assert_eq!(status_of(&err), 403);
    }

    // Selecting A: allowed.
    {
        let mut conn = pool.get().expect("conn");
        let req = TestRequest::default()
            .insert_header((WORKSPACE_SELECTION_HEADER, "tok-a"))
            .to_srv_request();
        enforce_api_token_workspace(&req, &mut conn, &token_claims(owner_uuid, a_uuid), a_uuid)
            .expect("selecting the bound workspace must pass");
    }

    std::env::remove_var("NOSDESK_WORKSPACE_SELECTION");
    std::env::remove_var("NOSDESK_DEPLOYMENT_MODE");

    // Archiving the bound workspace must not strand the token. `find_by_id`
    // hides archived workspaces, so resolving the binding through it would turn
    // an archived binding into a failed token lookup and a 401, killing every
    // token minted in a workspace the moment it is archived, including the
    // platform admin's token needed to restore it.
    {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query("UPDATE workspaces SET archived_at = now() WHERE id = $1")
            .bind::<diesel::sql_types::Integer, _>(a_id)
            .execute(&mut conn)
            .expect("archive workspace A");

        // The distinction the fix rests on: the archive-filtering finder now
        // hides the row, the binding finder does not.
        assert!(
            backend::repository::workspaces::find_by_id(&mut conn, a_id)
                .expect("lookup")
                .is_none(),
            "find_by_id must hide an archived workspace"
        );
        assert_eq!(
            backend::repository::workspaces::uuid_for_id(&mut conn, a_id).expect("lookup"),
            Some(a_uuid),
            "the binding must still resolve once the workspace is archived"
        );

        // End to end through the real credential path: mint a token in A (the
        // FK means A must exist, and it is archived by now), present it, and
        // require that it still authenticates and still carries its binding.
        let token = common::mint_api_token(&mut conn, &owner, "archived-binding");
        let req = TestRequest::default()
            .insert_header(("Authorization", format!("Bearer {token}")))
            .to_srv_request();
        let claims = backend::middleware::api_token::try_bearer_auth(&req, &pool_data)
            .expect("an archived binding must not fail authentication")
            .expect("a bearer token was presented");
        assert_eq!(
            claims.workspace_uuid,
            Some(a_uuid),
            "the token must still carry the workspace it was minted in"
        );
    }
}
