//! A workspace's records name only its own people. Accounts are shared across
//! workspaces, so an admin or agent of A who knows the uuid of someone who
//! belongs only to B must not be able to put them on A's asset, loan, group,
//! contact card or API token: each write refuses them as it refuses a uuid no
//! account has, and leaves nothing behind that names them.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::Utc;
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewGroup};
use backend::schema::{api_tokens, asset_loans, assets, user_groups, user_profiles};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;

use crate::channel_backlog::as_admin;
use crate::common::{self, TestPool, WorkspaceSeed};

struct Fixture {
    _db: common::TestDb,
    _search_dir: tempfile::TempDir,
    pool: TestPool,
    a: WorkspaceSeed,
    /// Someone who belongs only to workspace B.
    stranger: Uuid,
    search: Arc<SearchService>,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let two = common::seed_two_workspaces(&mut pool.get().expect("conn"));
        let search_dir = tempfile::tempdir().expect("search dir");
        let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
        Self {
            _db: db,
            _search_dir: search_dir,
            pool,
            a: two.a,
            stranger: two.b.member_uuid,
            search,
        }
    }

    /// Rows in A naming `who`, counted by `count`.
    fn count(&self, count: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<i64>) -> i64 {
        as_admin(&self.pool, &self.a, count)
    }

    async fn send(&self, method: Method, uri: &str, body: Value) -> (StatusCode, Value) {
        let user = self.a.admin_uuid;
        let now = Utc::now().timestamp();
        let claims = Claims {
            sub: user.to_string(),
            name: "Admin".to_string(),
            email: "admin@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (now + 3600) as usize,
            iat: now as usize,
        };
        let workspace = WorkspaceContext {
            workspace_id: self.a.workspace_id,
            workspace_uuid: self.a.workspace_uuid,
            slug: self.a.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(self.a.workspace_id);
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(self.search.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(
                    web::scope("/api")
                        .configure(backend::handlers::assets::config)
                        .configure(backend::handlers::groups::config)
                        .configure(backend::handlers::api_tokens::config)
                        .configure(backend::handlers::users::config),
                ),
        )
        .await;
        let mut req = http_test::TestRequest::default().method(method).uri(uri);
        if !body.is_null() {
            req = req.set_json(body);
        }
        let resp = http_test::call_service(&app, req.to_request()).await;
        let status = resp.status();
        let bytes = http_test::read_body(resp).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// A new asset in A, with no user.
    async fn asset(&self) -> i32 {
        let (status, body) = self
            .send(Method::POST, "/api/assets", json!({ "name": "Laptop" }))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body["id"].as_i64().expect("asset id") as i32
    }
}

#[actix_web::test]
async fn an_asset_s_user_and_manager_are_the_workspace_s_people() {
    let f = Fixture::new();
    let nobody = Uuid::new_v4();
    let stranger = f.stranger;

    let create = |user: Uuid| json!({ "name": "Laptop", "primary_user_uuid": user });
    let (nobody_status, _) = f.send(Method::POST, "/api/assets", create(nobody)).await;
    let (status, body) = f.send(Method::POST, "/api/assets", create(stranger)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "create: {body}");
    assert_eq!(status, nobody_status);

    let asset = f.asset().await;
    for field in ["primary_user_uuid", "managed_by_user_uuid"] {
        let (status, body) = f
            .send(
                Method::PUT,
                &format!("/api/assets/{asset}"),
                json!({ field: stranger }),
            )
            .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{field}: {body}");
    }
    let named = f.count(|c| {
        assets::table
            .filter(
                assets::primary_user_uuid
                    .eq(stranger)
                    .or(assets::managed_by_user_uuid.eq(stranger)),
            )
            .count()
            .get_result(c)
    });
    assert_eq!(named, 0);

    // The workspace's own people still can be.
    let (status, body) = f
        .send(
            Method::PUT,
            &format!("/api/assets/{asset}"),
            json!({ "primary_user_uuid": f.a.member_uuid, "managed_by_user_uuid": f.a.admin_uuid }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[actix_web::test]
async fn a_loan_s_borrower_is_one_of_the_workspace_s_people() {
    let f = Fixture::new();
    let asset = f.asset().await;
    let lend = |user: Uuid| json!({ "borrower_user_uuid": user });
    let uri = format!("/api/assets/{asset}/loans");

    let (nobody_status, nobody_body) = f.send(Method::POST, &uri, lend(Uuid::new_v4())).await;
    let (status, body) = f.send(Method::POST, &uri, lend(f.stranger)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!((status, &body), (nobody_status, &nobody_body));
    let named = f.count(|c| {
        asset_loans::table
            .filter(asset_loans::borrower_user_uuid.eq(f.stranger))
            .count()
            .get_result(c)
    });
    assert_eq!(named, 0);

    let (status, body) = f.send(Method::POST, &uri, lend(f.a.member_uuid)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

#[actix_web::test]
async fn a_group_s_members_hold_a_seat_in_the_workspace() {
    let f = Fixture::new();
    let group = as_admin(&f.pool, &f.a, |c| {
        backend::repository::groups::create_group(
            c,
            NewGroup {
                name: "Service desk".to_string(),
                description: None,
                color: None,
                created_by: Some(f.a.admin_uuid),
            },
        )
    });

    let (status, body) = f
        .send(
            Method::PUT,
            &format!("/api/groups/{}/members", group.id),
            json!({ "member_uuids": [f.a.member_uuid, f.stranger] }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let (status, body) = f
        .send(
            Method::PUT,
            &format!("/api/users/{}/groups", f.stranger),
            json!({ "group_ids": [group.id] }),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    let named = f.count(|c| {
        user_groups::table
            .filter(user_groups::user_uuid.eq(f.stranger))
            .count()
            .get_result(c)
    });
    assert_eq!(named, 0);

    let (status, body) = f
        .send(
            Method::PUT,
            &format!("/api/groups/{}/members", group.id),
            json!({ "member_uuids": [f.a.member_uuid] }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[actix_web::test]
async fn an_admin_edits_contact_details_of_the_workspace_s_people_only() {
    let f = Fixture::new();
    let stranger = f.stranger;

    let (status, body) = f
        .send(
            Method::PUT,
            &format!("/api/users/{stranger}/profile-fields"),
            json!({ "job_title": "CEO", "custom_fields": {} }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "profile: {body}");
    let (status, body) = f
        .send(
            Method::POST,
            &format!("/api/users/{stranger}/phones"),
            json!({ "phone": "+61 2 5550 1234" }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "phone: {body}");
    let (status, body) = f
        .send(
            Method::PUT,
            &format!("/api/users/{stranger}/manager"),
            json!({ "manager_uuid": f.a.admin_uuid }),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "manager: {body}");
    let named = f.count(|c| {
        user_profiles::table
            .filter(user_profiles::user_uuid.eq(stranger))
            .count()
            .get_result(c)
    });
    assert_eq!(named, 0, "no profile row makes them one of A's people");

    let (status, body) = f
        .send(
            Method::PUT,
            &format!("/api/users/{}/profile-fields", f.a.member_uuid),
            json!({ "job_title": "Analyst", "custom_fields": {} }),
        )
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[actix_web::test]
async fn an_api_token_is_made_for_someone_with_a_seat_here() {
    let f = Fixture::new();
    let mint = |user: Uuid| json!({ "name": "ci", "user_uuid": user });

    let (nobody_status, nobody_body) = f
        .send(Method::POST, "/api/admin/api-tokens", mint(Uuid::new_v4()))
        .await;
    let (status, body) = f
        .send(Method::POST, "/api/admin/api-tokens", mint(f.stranger))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!((status, &body), (nobody_status, &nobody_body));
    let named = f.count(|c| {
        api_tokens::table
            .filter(api_tokens::user_uuid.eq(f.stranger))
            .count()
            .get_result(c)
    });
    assert_eq!(named, 0);

    let (status, body) = f
        .send(Method::POST, "/api/admin/api-tokens", mint(f.a.admin_uuid))
        .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}
