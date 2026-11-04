//! Hosted: a workspace's owner or admin never sees another workspace through
//! the operator console. Every `/api/admin/workspaces` route and
//! `/api/admin/edition` answer 403 to anyone without the platform-admin role,
//! however senior their seat, and change nothing in the other workspace. The
//! workspace switcher (`/api/me/workspaces`) and the workspace's own member
//! list (`/api/workspace/members`) show only the caller's own workspaces and
//! people. A platform admin on the same routes gets 200, so the 403s come from
//! the role gate and not from routing.
//!
//! A standalone binary rather than part of `tests/it`: hosted selection mode
//! is process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use actix_web::body::MessageBody;
use actix_web::dev::ServiceResponse;
use actix_web::middleware::from_fn;
use actix_web::{test, web, App};
use chrono::{Duration, Utc};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::middleware::api_token::dual_auth_middleware;
use backend::models::NewActiveSession;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::jwt::JwtUtils;

fn session_token(pool: &backend::db::Pool, user_uuid: Uuid) -> String {
    let mut conn = pool.get().expect("conn");
    let user =
        backend::repository::users::get_user_by_uuid(&user_uuid, &mut conn).expect("load user");
    let session = backend::repository::active_sessions::create_session(
        &mut conn,
        NewActiveSession {
            user_uuid,
            device_name: Some("admin-workspaces-gate-test".into()),
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
async fn hosted_workspace_admins_cannot_reach_other_workspaces() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    std::env::set_var("NOSDESK_WORKSPACE_SELECTION", "1");
    common::ensure_test_keyring();
    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);

    // Two customer workspaces. Acme's owner and admin hold seats but no
    // platform role; Globex has its own owner. The operator is a platform
    // admin with a seat in Acme, for the control requests.
    let (acme_id, globex_id, owner, admin, globex_owner, operator) = {
        let mut conn = pool.get().expect("conn");
        let acme = common::mint_workspace(&mut conn, "acme-gate", "Acme Gate");
        let globex = common::mint_workspace(&mut conn, "globex-gate", "Globex Gate");
        let owner = common::insert_plain_user(&mut conn, "Acme Owner");
        let admin = common::insert_plain_user(&mut conn, "Acme Admin");
        let globex_owner = common::insert_plain_user(&mut conn, "Globex Owner");
        let operator = common::insert_user(&mut conn, "Operator").uuid;
        (acme, globex, owner, admin, globex_owner, operator)
    };
    add_seat(&pool, acme_id, owner, "owner");
    add_seat(&pool, acme_id, admin, "admin");
    add_seat(&pool, globex_id, globex_owner, "owner");
    add_seat(&pool, acme_id, operator, "admin");

    let app = test::init_service(
        App::new().app_data(web::Data::new(pool.clone())).service(
            web::scope("/api")
                .wrap(from_fn(dual_auth_middleware))
                .configure(backend::handlers::admin_workspaces::config),
        ),
    )
    .await;

    let globex = format!("/api/admin/workspaces/{globex_id}");
    let globex_member = format!("{globex}/members/{globex_owner}");
    let operator_routes: Vec<(&str, String, Option<Value>)> = vec![
        ("GET", "/api/admin/workspaces".into(), None),
        (
            "GET",
            "/api/admin/workspaces?include_archived=true".into(),
            None,
        ),
        ("GET", "/api/admin/edition".into(), None),
        (
            "POST",
            "/api/admin/workspaces".into(),
            Some(json!({"slug": "rogue-gate", "name": "Rogue"})),
        ),
        ("PATCH", globex.clone(), Some(json!({"name": "Renamed"}))),
        ("POST", format!("{globex}/archive"), None),
        ("POST", format!("{globex}/restore"), None),
        ("DELETE", format!("{globex}?confirm=globex-gate"), None),
        ("GET", format!("{globex}/members"), None),
        (
            "POST",
            format!("{globex}/members"),
            Some(json!({"user_uuid": admin, "role": "admin"})),
        ),
        (
            "PATCH",
            globex_member.clone(),
            Some(json!({"role": "member"})),
        ),
        ("DELETE", globex_member, None),
    ];

    for (who, user) in [("owner", owner), ("admin", admin)] {
        let token = session_token(&pool, user);
        for (method, uri, body) in &operator_routes {
            let req = match *method {
                "GET" => test::TestRequest::get(),
                "POST" => test::TestRequest::post(),
                "PATCH" => test::TestRequest::patch(),
                "DELETE" => test::TestRequest::delete(),
                other => unreachable!("method {other}"),
            }
            .uri(uri)
            .insert_header(("Authorization", format!("Bearer {token}")))
            .insert_header(("X-Nosdesk-Workspace", "acme-gate"));
            let req = match body {
                Some(b) => req.set_json(b),
                None => req,
            };
            let resp = test::call_service(&app, req.to_request()).await;
            assert_eq!(
                resp.status().as_u16(),
                403,
                "Acme's {who} must get 403 on {method} {uri}"
            );
        }

        // The switcher lists Acme alone.
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/me/workspaces")
                .insert_header(("Authorization", format!("Bearer {token}")))
                .insert_header(("X-Nosdesk-Workspace", "acme-gate"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 200, "{who}: /me/workspaces");
        let ids: Vec<i64> = body_json(resp)
            .await
            .as_array()
            .expect("array")
            .iter()
            .map(|w| w["workspace_id"].as_i64().expect("workspace_id"))
            .collect();
        assert_eq!(ids, vec![i64::from(acme_id)], "{who} sees only Acme");

        // The workspace's own member list holds no one from Globex.
        let resp = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/workspace/members")
                .insert_header(("Authorization", format!("Bearer {token}")))
                .insert_header(("X-Nosdesk-Workspace", "acme-gate"))
                .to_request(),
        )
        .await;
        assert_eq!(resp.status().as_u16(), 200, "{who}: /workspace/members");
        let listed = body_json(resp).await.to_string();
        assert!(
            !listed.contains(&globex_owner.to_string()),
            "{who}: Acme's member list must not include Globex's owner"
        );
        assert!(
            listed.contains(&owner.to_string()),
            "{who}: Acme's member list includes Acme's owner"
        );
    }

    // Globex is untouched: same name, active, its owner still seated.
    {
        use backend::schema::{workspace_members, workspaces};
        let mut conn = pool.get().expect("conn");
        let (name, archived): (String, Option<chrono::DateTime<Utc>>) = workspaces::table
            .find(globex_id)
            .select((workspaces::name, workspaces::archived_at))
            .first(&mut conn)
            .expect("globex still exists");
        assert_eq!(name, "Globex Gate");
        assert!(archived.is_none(), "Globex must not be archived");
        let seated: i64 = workspace_members::table
            .filter(workspace_members::workspace_id.eq(globex_id))
            .filter(workspace_members::removed_at.is_null())
            .count()
            .get_result(&mut conn)
            .expect("count globex members");
        assert_eq!(seated, 1, "Globex keeps exactly its own owner");
    }

    // Control: the platform admin gets the cross-workspace list, Globex
    // included, on the same app and header.
    let token = session_token(&pool, operator);
    let resp = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/admin/workspaces")
            .insert_header(("Authorization", format!("Bearer {token}")))
            .insert_header(("X-Nosdesk-Workspace", "acme-gate"))
            .to_request(),
    )
    .await;
    assert_eq!(
        resp.status().as_u16(),
        200,
        "platform admin lists workspaces"
    );
    let listed = body_json(resp).await.to_string();
    assert!(listed.contains("globex-gate"), "the operator sees Globex");
}
