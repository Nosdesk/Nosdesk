//! An API token is one workspace's automation. Its holder holds a seat in the
//! workspace it was minted in, or it authenticates nothing; a request that
//! names no workspace runs in that one, pinned and gated, never as an
//! unscoped principal. (That it never reaches an account is
//! `tests/api_token_account_routes.rs`, on the real route tree.)

use std::sync::Arc;

use actix_web::http::StatusCode;
use actix_web::middleware::from_fn;
use actix_web::test as http_test;
use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::Value;
use uuid::Uuid;

use backend::middleware::api_token::dual_auth_middleware;
use backend::models::{NewApiToken, NewTicket};
use backend::repository::api_tokens::{get_token_prefix, hash_token};
use backend::repository::workflow_states;
use backend::schema::{api_tokens, tickets};
use backend::services::search::SearchService;

use crate::channel_backlog::as_admin;
use crate::common::{self, TestPool, WorkspaceSeed};

struct Fixture {
    _db: common::TestDb,
    _search_dir: tempfile::TempDir,
    pool: TestPool,
    a: WorkspaceSeed,
    b: WorkspaceSeed,
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
            b: two.b,
            search,
        }
    }

    /// A full-scope token for `holder`, made by `maker`, minted in A.
    fn token(&self, holder: Uuid, maker: Uuid) -> String {
        let raw = format!("nsk_test_{}", Uuid::new_v4().simple());
        as_admin(&self.pool, &self.a, |c| {
            diesel::insert_into(api_tokens::table)
                .values(&NewApiToken {
                    token_hash: hash_token(&raw),
                    token_prefix: get_token_prefix(&raw),
                    user_uuid: holder,
                    name: "automation".to_string(),
                    scopes: Some(vec![Some("full".to_string())]),
                    created_by: maker,
                    expires_at: None,
                    role_ceiling: Some("admin".to_string()),
                    platform_role_ceiling: None,
                })
                .execute(c)
        });
        raw
    }

    /// Call `/api` behind the real dual-auth middleware with `token`, on a
    /// request that names no workspace (no Host context, no selection).
    async fn call(&self, token: &str, req: http_test::TestRequest) -> (StatusCode, Value) {
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(self.search.clone()))
                .service(
                    web::scope("/api")
                        .wrap(from_fn(dual_auth_middleware))
                        .configure(backend::handlers::tickets::config),
                ),
        )
        .await;
        let req = req
            .insert_header(("Authorization", format!("Bearer {token}")))
            .to_request();
        match http_test::try_call_service(&app, req).await {
            Ok(resp) => {
                let status = resp.status();
                let bytes = http_test::read_body(resp).await;
                (
                    status,
                    serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                )
            }
            Err(e) => (e.error_response().status(), Value::Null),
        }
    }
}

/// A token minted in A for someone with no seat there (here, a member of B
/// only, made by A's admin before minting checked) authenticates nothing.
#[actix_web::test]
async fn a_token_whose_holder_has_no_seat_in_its_workspace_is_refused() {
    let f = Fixture::new();
    for maker in [f.a.admin_uuid, f.b.member_uuid] {
        let token = f.token(f.b.member_uuid, maker);
        let (status, body) = f
            .call(&token, http_test::TestRequest::get().uri("/api/tickets"))
            .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED, "made by {maker}: {body}");
    }
}

/// A request that names no workspace runs in the token's own: an agent's
/// token sees its workspace's tickets, and a requester's token is held to the
/// agent surface's gate like any request scoped there.
#[actix_web::test]
async fn a_token_naming_no_workspace_runs_in_its_own() {
    let f = Fixture::new();
    as_admin(&f.pool, &f.a, |c| {
        let open = workflow_states::default_state(c)?.id;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Only in workspace A".to_string(),
                workflow_state_id: open,
                requester_uuid: Some(f.a.member_uuid),
                ..Default::default()
            })
            .execute(c)
    });

    let agent = f.token(f.a.admin_uuid, f.a.admin_uuid);
    let (status, body) = f
        .call(&agent, http_test::TestRequest::get().uri("/api/tickets"))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.to_string().contains("Only in workspace A"),
        "scoped to A: {body}"
    );

    let requester = f.token(f.a.member_uuid, f.a.admin_uuid);
    let (status, body) = f
        .call(
            &requester,
            http_test::TestRequest::get().uri("/api/tickets"),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
}
