//! Bulk delete reports every user it did not delete.
//!
//! `POST /users/bulk` skips administrators and unknown ids rather than failing
//! the batch. It used to return 200 with only `affected`, so a list where every
//! selected user was skipped looked like delete doing nothing.
//!
//! Bulk delete steps up like `DELETE /users/{uuid}`: the caller's password or
//! 2FA code rides in the request body.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;

use crate::common;

const PASSWORD: &str = "correct horse battery staple";

fn give_local_password(conn: &mut diesel::PgConnection, user: Uuid) {
    use backend::schema::user_auth_identities;
    let hash = bcrypt::hash(PASSWORD, 4).expect("hash");
    diesel::insert_into(user_auth_identities::table)
        .values((
            user_auth_identities::user_uuid.eq(user),
            user_auth_identities::provider_type.eq("local"),
            user_auth_identities::external_id.eq(user.to_string()),
            user_auth_identities::password_hash.eq(hash),
        ))
        .execute(conn)
        .expect("insert local identity");
}

fn is_deleted(conn: &mut diesel::PgConnection, who: Uuid) -> bool {
    backend::schema::users::table
        .find(who)
        .select(backend::schema::users::deleted_at)
        .first::<Option<chrono::NaiveDateTime>>(conn)
        .expect("user row")
        .is_some()
}

/// An app running every request as a platform admin `operator` in `a`.
macro_rules! bulk_app {
    ($pool:expr, $a:expr, $operator:expr, $search:expr) => {{
        let workspace = WorkspaceContext {
            workspace_id: $a.workspace_id,
            workspace_uuid: $a.workspace_uuid,
            slug: $a.slug.clone(),
            name: $a.slug.clone(),
            organisation_id: None,
            custom_domain: None,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user($operator, Some(corr)).with_workspace($a.workspace_id);
        let claims = Claims {
            sub: $operator.to_string(),
            name: "Platform Operator".to_string(),
            email: "operator@example.com".to_string(),
            platform_role: "platform_admin".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (chrono::Utc::now().timestamp() + 3600) as usize,
            iat: chrono::Utc::now().timestamp() as usize,
        };
        http_test::init_service(
            App::new()
                .app_data(web::Data::new($pool.clone()))
                .app_data(web::Data::new($search))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(web::scope("/api").configure(backend::handlers::users::config)),
        )
        .await
    }};
}

fn bulk_delete(body: serde_json::Value) -> http_test::TestRequest {
    http_test::TestRequest::default()
        .method(Method::POST)
        .uri("/api/users/bulk")
        .set_json(body)
}

#[actix_web::test]
async fn bulk_delete_counts_skipped_and_failed_users() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = {
        let mut conn = pool.get().expect("conn");
        common::seed_two_workspaces(&mut conn)
    };
    let a = ws.a.clone();
    let operator = {
        let mut conn = pool.get().expect("conn");
        let operator = common::insert_plain_user(&mut conn, "Platform Operator");
        give_local_password(&mut conn, operator);
        operator
    };

    let search_dir = tempfile::tempdir().expect("tempdir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    let app = bulk_app!(pool, a, operator, search);

    let unknown = Uuid::new_v4();
    let resp = http_test::call_service(
        &app,
        bulk_delete(json!({
            "action": "delete",
            "ids": [
                a.member_uuid.to_string(),
                a.admin_uuid.to_string(),
                unknown.to_string(),
            ],
            "password": PASSWORD,
        }))
        .to_request(),
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body: serde_json::Value = http_test::read_body_json(resp).await;
    assert_eq!(body["affected"], 1, "the member is deleted: {body}");
    assert_eq!(body["skipped_admin"], 1, "the admin is skipped: {body}");
    assert_eq!(body["failed"], 1, "the unknown id is reported: {body}");

    let mut conn = pool.get().expect("conn");
    assert!(is_deleted(&mut conn, a.member_uuid));
    assert!(!is_deleted(&mut conn, a.admin_uuid));
}

#[actix_web::test]
async fn bulk_delete_requires_the_callers_password() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = {
        let mut conn = pool.get().expect("conn");
        common::seed_two_workspaces(&mut conn)
    };
    let a = ws.a.clone();
    let operator = {
        let mut conn = pool.get().expect("conn");
        let operator = common::insert_plain_user(&mut conn, "Platform Operator");
        give_local_password(&mut conn, operator);
        operator
    };

    let search_dir = tempfile::tempdir().expect("tempdir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    let app = bulk_app!(pool, a, operator, search);
    let ids = json!([a.member_uuid.to_string()]);

    let resp = http_test::call_service(
        &app,
        bulk_delete(json!({ "action": "delete", "ids": ids })).to_request(),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "no credential is refused"
    );
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8");
    assert!(body.contains("Confirm with your password"), "{body}");
    let mut conn = pool.get().expect("conn");
    assert!(!is_deleted(&mut conn, a.member_uuid));

    let resp = http_test::call_service(
        &app,
        bulk_delete(json!({ "action": "delete", "ids": ids, "password": "wrong password" }))
            .to_request(),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "a wrong password is refused"
    );
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8");
    assert!(body.contains("Confirm with your password"), "{body}");
    assert!(!is_deleted(&mut conn, a.member_uuid));

    let resp = http_test::call_service(
        &app,
        bulk_delete(json!({ "action": "delete", "ids": ids, "password": PASSWORD })).to_request(),
    )
    .await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "the correct password deletes"
    );
    let body: serde_json::Value = http_test::read_body_json(resp).await;
    assert_eq!(body["affected"], 1, "{body}");
    assert!(is_deleted(&mut conn, a.member_uuid));
}
