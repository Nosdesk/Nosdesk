//! SSE workspace binding (Model C, increment 2).
//!
//! EventSource can't send the `X-Nosdesk-Workspace` selection header, so the
//! selected workspace is bound into the SSE token at mint time and the stream
//! authorizes against it. This proves the stream derives the workspace from the
//! token and membership-gates it: a member streams, a non-member and an unknown
//! workspace are both denied (no existence leak). It's the same tenant boundary
//! the REST gate enforces, reached over a different transport.

#![allow(clippy::expect_used)]

use actix_web::{web, App};

use backend::handlers::sse::{sse_events_stream, SseState};
use backend::repository::workspaces::{add_membership, find_by_id};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::jwt::JwtUtils;

mod common;

fn sse_token_for(user_uuid: uuid::Uuid, workspace_uuid: Option<uuid::Uuid>) -> String {
    if std::env::var("JWT_SECRET").is_err() {
        std::env::set_var("JWT_SECRET", "test-secret-key-for-testing-only-32chars");
    }
    // common::insert_user seeds platform_role = platform_admin; the token's
    // platform_role must match or validation rejects it (demotion guard).
    JwtUtils::create_sse_token(&user_uuid.to_string(), "platform_admin", workspace_uuid)
        .expect("mint sse token")
}

#[actix_web::test]
async fn stream_authorizes_against_the_token_bound_workspace() {
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    // Workspace `acme`; a member and a non-member.
    let (acme_uuid, member_uuid, stranger_uuid) = {
        let mut conn = pool.get().expect("conn");
        let acme = common::mint_workspace(&mut conn, "acme-sse", "Acme SSE");
        let member = common::insert_user(&mut conn, "SSE Member");
        let stranger = common::insert_user(&mut conn, "SSE Stranger");
        let acme_uuid = find_by_id(&mut conn, acme)
            .expect("ws lookup")
            .expect("ws exists")
            .uuid;
        let actor = ActorContext::user(member.uuid, None).with_workspace(acme);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            add_membership(c, acme, member.uuid, "member")?;
            Ok(())
        })
        .expect("add membership");
        (acme_uuid, member.uuid, stranger.uuid)
    };

    let server_pool = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(server_pool.clone()))
            .app_data(web::Data::new(SseState::new()))
            .route("/api/events/stream", web::get().to(sse_events_stream))
    });

    let client = awc::Client::new();
    let get_status = |token: String| {
        let url = srv.url(&format!("/api/events/stream?sse_token={token}"));
        let client = client.clone();
        async move {
            client
                .get(url)
                .send()
                .await
                .expect("stream request")
                .status()
                .as_u16()
        }
    };

    // Member: the token's workspace resolves and they belong to it -> stream opens.
    assert_eq!(
        get_status(sse_token_for(member_uuid, Some(acme_uuid))).await,
        200,
        "member's token-bound workspace must stream"
    );

    // Non-member with a token bound to acme: denied.
    assert_eq!(
        get_status(sse_token_for(stranger_uuid, Some(acme_uuid))).await,
        403,
        "non-member's token-bound workspace must be denied"
    );

    // Token bound to an unknown workspace: denied, indistinguishable from a
    // non-member (no workspace-existence leak).
    assert_eq!(
        get_status(sse_token_for(member_uuid, Some(uuid::Uuid::new_v4()))).await,
        403,
        "unknown token-bound workspace must be denied"
    );
}
