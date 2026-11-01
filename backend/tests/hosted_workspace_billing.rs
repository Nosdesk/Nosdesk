//! The hosted billing projection end to end:
//! `POST /api/internal/v1/workspaces/{slug}/billing` from the control plane,
//! and the owner-only `billing` object on `GET /api/me/workspaces`.
//!
//! The push runs through the production internal scope (platform auth,
//! idempotency, `internal_workspaces::config`) on the runtime role, so a write
//! without the bypass fails here. Verifies:
//!
//!   1. A push applies (204) and lands on the row.
//!   2. A push with the same or an older `as_of` is ignored (204, unchanged).
//!   3. A newer push with a null status clears the state.
//!   4. Unknown slug: 404. Missing key or bad timestamp: 400. No token: 401.
//!   5. `/me/workspaces` carries `billing` for the workspace's owner only;
//!      admin and agent rows have no `billing` key.
//!
//! A standalone binary rather than part of `tests/it`: hosted mode is
//! process-wide environment. The self-hosted case is in
//! `tests/it/admin_workspace_members.rs`.

#![allow(clippy::expect_used)]

mod common;

use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use actix_web::middleware::from_fn;
use actix_web::{test, web, App};
use chrono::{DateTime, Duration, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::middleware::api_token::dual_auth_middleware;
use backend::models::NewActiveSession;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::jwt::JwtUtils;

type Billing = (
    Option<String>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
    Option<DateTime<Utc>>,
);

fn stored_billing(pool: &backend::db::Pool, workspace_id: i32) -> Billing {
    use backend::schema::workspaces;
    let mut conn = pool.get().expect("conn");
    workspaces::table
        .find(workspace_id)
        .select((
            workspaces::billing_status,
            workspaces::billing_trial_end,
            workspaces::billing_cancel_at,
            workspaces::billing_as_of,
        ))
        .first(&mut conn)
        .expect("load billing state")
}

fn ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .expect("rfc3339")
        .with_timezone(&Utc)
}

fn session_token(pool: &backend::db::Pool, user_uuid: Uuid) -> String {
    let mut conn = pool.get().expect("conn");
    let user =
        backend::repository::users::get_user_by_uuid(&user_uuid, &mut conn).expect("load user");
    let session = backend::repository::active_sessions::create_session(
        &mut conn,
        NewActiveSession {
            user_uuid,
            device_name: Some("hosted-workspace-billing-test".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (Utc::now() + Duration::hours(1)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: None,
            provider_amr: None,
        },
    )
    .expect("create session");
    JwtUtils::create_token(&user, &session.session_id).expect("mint session JWT")
}

fn add_seat(pool: &backend::db::Pool, workspace_id: i32, user_uuid: Uuid, role: &str) {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::user(user_uuid, None).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
        add_membership(
            c,
            workspace_id,
            user_uuid,
            role,
            SeatWriteAuthority::ControlPlane,
        )
        .map(|_| ())
    })
    .expect("add membership");
}

async fn body_json<B: MessageBody>(resp: ServiceResponse<B>) -> Value
where
    B::Error: std::fmt::Debug,
{
    let bytes = test::read_body(resp).await;
    serde_json::from_slice(&bytes).expect("json body")
}

#[actix_web::test]
async fn control_plane_pushes_billing_state_behind_the_as_of_fence() {
    common::ensure_test_keyring();
    common::enable_platform_auth();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let acme_id = common::mint_workspace(&mut pool.get().expect("conn"), "acme-bill", "Acme");
    let token = common::mint_platform_jwt("platform:provision", 300);

    let pool_for_app = test_db.runtime_pool(4);
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .service(
                web::scope("/api/internal/v1")
                    .wrap(from_fn(backend::middleware::idempotency_middleware))
                    .wrap(from_fn(backend::extractors::platform_auth_middleware))
                    .configure(backend::handlers::internal_workspaces::config),
            )
    });
    let client = awc::Client::new();
    let url = srv.url("/api/internal/v1/workspaces/acme-bill/billing");
    let push = |body: Value| {
        let client = client.clone();
        let url = url.clone();
        let token = token.clone();
        async move {
            client
                .post(&url)
                .insert_header(("Authorization", format!("Bearer {token}")))
                .send_json(&body)
                .await
                .expect("send billing push")
                .status()
                .as_u16()
        }
    };

    // --- 1: a push applies ---
    let status = push(json!({
        "status": "trialing",
        "trial_end": "2026-11-04T09:00:00Z",
        "cancel_at": null,
        "as_of": "2026-10-22T03:00:00Z",
    }))
    .await;
    assert_eq!(status, 204, "first push applies");
    let trialing = (
        Some("trialing".to_string()),
        Some(ts("2026-11-04T09:00:00Z")),
        None,
        Some(ts("2026-10-22T03:00:00Z")),
    );
    assert_eq!(stored_billing(&pool, acme_id), trialing);

    // --- 2: the same as_of again, then an older one, are ignored ---
    for as_of in ["2026-10-22T03:00:00Z", "2026-10-22T02:59:00Z"] {
        let status = push(json!({
            "status": "canceled",
            "trial_end": null,
            "cancel_at": "2026-11-04T09:00:00Z",
            "as_of": as_of,
        }))
        .await;
        assert_eq!(status, 204, "a stale push at {as_of} still answers 204");
        assert_eq!(
            stored_billing(&pool, acme_id),
            trialing,
            "a stale push at {as_of} changes nothing"
        );
    }

    // --- 2b: a newer push replaces it; an unknown status is stored verbatim ---
    let status = push(json!({
        "status": "some_future_status",
        "trial_end": "2026-11-04T09:00:00Z",
        "cancel_at": "2026-11-04T09:00:00Z",
        "as_of": "2026-10-22T04:00:00Z",
    }))
    .await;
    assert_eq!(status, 204);
    assert_eq!(
        stored_billing(&pool, acme_id),
        (
            Some("some_future_status".to_string()),
            Some(ts("2026-11-04T09:00:00Z")),
            Some(ts("2026-11-04T09:00:00Z")),
            Some(ts("2026-10-22T04:00:00Z")),
        )
    );

    // --- 3: a newer push with no subscription clears it ---
    let status = push(json!({
        "status": null,
        "trial_end": null,
        "cancel_at": null,
        "as_of": "2026-10-22T05:00:00Z",
    }))
    .await;
    assert_eq!(status, 204);
    assert_eq!(
        stored_billing(&pool, acme_id),
        (None, None, None, Some(ts("2026-10-22T05:00:00Z")))
    );

    // --- 4: unknown slug, malformed bodies, no token ---
    let resp = client
        .post(srv.url("/api/internal/v1/workspaces/no-such-ws/billing"))
        .insert_header(("Authorization", format!("Bearer {token}")))
        .send_json(&json!({
            "status": "active",
            "trial_end": null,
            "cancel_at": null,
            "as_of": "2026-10-22T06:00:00Z",
        }))
        .await
        .expect("send unknown slug");
    assert_eq!(resp.status().as_u16(), 404, "unknown slug");

    let malformed = [
        (
            "missing status key",
            json!({"trial_end": null, "cancel_at": null, "as_of": "2026-10-22T06:00:00Z"}),
        ),
        (
            "missing cancel_at key",
            json!({"status": "active", "trial_end": null, "as_of": "2026-10-22T06:00:00Z"}),
        ),
        (
            "null as_of",
            json!({"status": "active", "trial_end": null, "cancel_at": null, "as_of": null}),
        ),
        (
            "bad timestamp",
            json!({"status": "active", "trial_end": "next tuesday", "cancel_at": null,
                   "as_of": "2026-10-22T06:00:00Z"}),
        ),
    ];
    for (what, body) in malformed {
        assert_eq!(push(body).await, 400, "{what}");
    }
    assert_eq!(
        stored_billing(&pool, acme_id),
        (None, None, None, Some(ts("2026-10-22T05:00:00Z"))),
        "a refused push changes nothing"
    );

    let resp = client
        .post(&url)
        .send_json(&json!({
            "status": "active",
            "trial_end": null,
            "cancel_at": null,
            "as_of": "2026-10-22T06:00:00Z",
        }))
        .await
        .expect("send unauthenticated");
    assert_eq!(resp.status().as_u16(), 401, "no platform token");
}

#[actix_web::test]
async fn only_a_hosted_owner_sees_billing_on_me_workspaces() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("NOSDESK_WORKSPACE_SELECTION", "1");
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    let (acme_id, owner, admin, agent) = {
        let mut conn = pool.get().expect("conn");
        let acme = common::mint_workspace(&mut conn, "acme-billing-me", "Acme");
        let owner = common::insert_plain_user(&mut conn, "Acme Owner");
        let admin = common::insert_plain_user(&mut conn, "Acme Admin");
        let agent = common::insert_plain_user(&mut conn, "Acme Agent");
        (acme, owner, admin, agent)
    };
    add_seat(&pool, acme_id, owner, "owner");
    add_seat(&pool, acme_id, admin, "admin");
    add_seat(&pool, acme_id, agent, "agent");
    {
        use backend::schema::workspaces;
        let mut conn = pool.get().expect("conn");
        diesel::update(workspaces::table.find(acme_id))
            .set((
                workspaces::billing_status.eq("trialing"),
                workspaces::billing_trial_end.eq(ts("2026-11-04T09:00:00Z")),
                workspaces::billing_as_of.eq(ts("2026-10-22T03:00:00Z")),
            ))
            .execute(&mut conn)
            .expect("seed billing state");
    }

    let app = test::init_service(
        App::new().app_data(web::Data::new(pool.clone())).service(
            web::scope("/api")
                .wrap(from_fn(dual_auth_middleware))
                .configure(backend::handlers::admin_workspaces::config),
        ),
    )
    .await;

    for (who, user, sees_billing) in [
        ("owner", owner, true),
        ("admin", admin, false),
        ("agent", agent, false),
    ] {
        let token = session_token(&pool, user);
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/me/workspaces")
                .insert_header(("Authorization", format!("Bearer {token}")))
                .insert_header(("X-Nosdesk-Workspace", "acme-billing-me"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 200, "{who}: /me/workspaces");
        let body = body_json(resp).await;
        let rows = body.as_array().expect("array");
        assert_eq!(rows.len(), 1, "{who} belongs to Acme alone");
        let row = &rows[0];
        assert_eq!(row["role"], who);
        if sees_billing {
            assert_eq!(
                row["billing"],
                json!({
                    "status": "trialing",
                    "trial_end": "2026-11-04T09:00:00Z",
                    "cancel_at": null,
                }),
                "the owner sees the pushed state, without as_of"
            );
        } else {
            assert!(
                row.get("billing").is_none(),
                "{who} gets no billing key: {row}"
            );
        }
    }
}
