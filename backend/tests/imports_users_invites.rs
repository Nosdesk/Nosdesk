//! Send invites (`POST /admin/import/{id}/invites`) with email set up: an
//! invitation goes to each person a users import created who can't sign in
//! yet, once, and Done's sign-in line then says they have one.
//!
//! A standalone binary rather than part of `tests/it`: email configuration
//! is process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test::{self as http_test, TestRequest};
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use std::sync::Arc;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, ImportJobUpdate, NewImportJob};
use backend::services::imports::{self, csv_parser, ImportContext, ImportType, RecordIds};
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

fn email_set_up() {
    std::env::set_var("SMTP_ENABLED", "true");
    std::env::set_var("SMTP_HOST", "localhost");
    std::env::set_var("SMTP_PORT", "2525");
    std::env::set_var("SMTP_FROM_EMAIL", "help@example.com");
    std::env::set_var("FRONTEND_URL", "https://help.example.com");
}

async fn call(
    pool: &common::TestPool,
    ws: &common::WorkspaceSeed,
    req: TestRequest,
) -> (StatusCode, serde_json::Value) {
    let user = ws.admin_uuid;
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: user.to_string(),
        name: "Alex".to_string(),
        email: "alex@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (now + 3600) as usize,
        iat: now as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(user, Some(corr)).with_workspace(ws.workspace_id);
    let search_dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(search_dir.path(), pool).expect("init search"));
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            .app_data(web::Data::new(common::test_storage()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::imports::config)),
    )
    .await;
    let resp = http_test::call_service(&app, req.to_request()).await;
    let status = resp.status();
    let body = http_test::read_body(resp).await;
    (
        status,
        serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
    )
}

/// A committed users job in `ws` that created three people.
fn committed_users_job(conn: &mut common::TestPooledConn, ws: &common::WorkspaceSeed) -> Uuid {
    let parsed = csv_parser::parse_bytes(
        b"email,name,role,pronouns\n\
          ada@example.com,Ada Lovelace,user,\n\
          grace@example.com,Grace Hopper,user,\n\
          alan@example.com,Alan Turing,user,\n",
    )
    .expect("parse");
    let pin = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    with_actor_context::<_, imports::CommitError>(conn, &pin, |c| {
        let job = backend::repository::imports::create(
            c,
            NewImportJob {
                id: Uuid::now_v7(),
                job_type: "users".to_string(),
                filename: "people.csv".to_string(),
                file_path: "imports/none".to_string(),
                created_by: Some(ws.admin_uuid),
                upload_of: None,
            },
        )?;
        let ctx = ImportContext {
            actor: Some(ws.admin_uuid),
            job_id: job.id,
            options: Default::default(),
        };
        let committed = imports::apply(c, ImportType::Users, &parsed, &ctx)?;
        backend::repository::imports::update(
            c,
            job.id,
            ImportJobUpdate {
                status: Some("done".to_string()),
                completed_at: Some(Some(chrono::Utc::now())),
                record_ids: Some(serde_json::to_value(RecordIds::from_rows(&committed.rows)).ok()),
                ..Default::default()
            },
        )?;
        Ok(job.id)
    })
    .expect("committed users job")
}

fn invitations_to(conn: &mut common::TestPooledConn, email: &str) -> i64 {
    #[derive(diesel::QueryableByName)]
    struct Count {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        n: i64,
    }
    diesel::sql_query(
        "SELECT COUNT(*)::bigint AS n FROM reset_tokens t \
         JOIN user_emails e ON e.user_uuid = t.user_uuid AND e.is_primary \
         WHERE t.token_type = 'invitation' AND e.email = $1",
    )
    .bind::<diesel::sql_types::Text, _>(email)
    .get_result::<Count>(conn)
    .expect("count invitations")
    .n
}

fn queued_to(conn: &mut common::TestPooledConn, email: &str) -> i64 {
    use backend::schema::outbound_emails::dsl as o;
    o::outbound_emails
        .filter(o::recipient.eq(email))
        .count()
        .get_result(conn)
        .expect("count queued mail")
}

#[actix_web::test]
async fn send_invites_mails_each_person_who_cannot_sign_in_once() {
    email_set_up();
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let mut conn = pool.get().expect("conn");
    let job = committed_users_job(&mut conn, &ws);

    // Grace already set a password; she isn't waiting.
    diesel::sql_query(
        "INSERT INTO user_auth_identities (user_uuid, provider_type, external_id, password_hash) \
         SELECT user_uuid, 'local', user_uuid::text, 'x' FROM user_emails WHERE email = 'grace@example.com'",
    )
    .execute(&mut conn)
    .expect("grace's password");

    let uri = format!("/api/admin/import/{job}");
    let (status, before) = call(&pool, &ws, TestRequest::get().uri(&uri)).await;
    assert_eq!(status, StatusCode::OK, "{before}");
    assert_eq!(
        before["sign_in"],
        serde_json::json!({ "pending": 2, "invited": 0, "method": "invite", "email_ready": true }),
        "{before}"
    );

    let invites = || TestRequest::post().uri(&format!("/api/admin/import/{job}/invites"));
    let (status, body) = call(&pool, &ws, invites()).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["sent"], 2, "{body}");
    assert_eq!(body["failed"], 0, "{body}");
    assert_eq!(body["sign_in"]["pending"], 2, "{body}");
    assert_eq!(body["sign_in"]["invited"], 2, "{body}");

    for email in ["ada@example.com", "alan@example.com"] {
        assert_eq!(invitations_to(&mut conn, email), 1, "{email}");
        assert_eq!(queued_to(&mut conn, email), 1, "{email}");
    }
    assert_eq!(invitations_to(&mut conn, "grace@example.com"), 0);
    assert_eq!(queued_to(&mut conn, "grace@example.com"), 0);

    // Everyone waiting has an open invitation: nothing more to send.
    let (status, body) = call(&pool, &ws, invites()).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert_eq!(body["code"], "import_nothing_to_invite", "{body}");
    assert_eq!(queued_to(&mut conn, "ada@example.com"), 1);

    let (_, after) = call(&pool, &ws, TestRequest::get().uri(&uri)).await;
    assert_eq!(after["sign_in"]["invited"], 2, "{after}");
}
