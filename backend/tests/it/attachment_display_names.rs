//! An uploaded attachment keeps the name it was uploaded under. The storage key
//! is a sanitized single path segment; the row, the upload response and the
//! download's `Content-Disposition` carry the name as written, spaces and
//! punctuation included.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewUser, User};
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool};

const WS: i32 = 1;

fn agent(conn: &mut PgConnection) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: "Tech".to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert user");
    diesel::insert_into(workspace_members::table)
        .values((
            workspace_members::workspace_id.eq(WS),
            workspace_members::user_uuid.eq(user.uuid),
            workspace_members::role.eq("agent"),
        ))
        .execute(conn)
        .expect("insert member");
    user
}

fn spawn(pool: &TestPool, user: &User, storage: Arc<dyn Storage>) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let user_uuid = user.uuid;
    actix_test::start(move || {
        let claims = claims.clone();
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user_uuid, Some(corr)).with_workspace(WS);
        let ws = WorkspaceContext {
            workspace_id: WS,
            workspace_uuid: Uuid::nil(),
            slug: "default".to_string(),
            name: "Default".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(storage.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route("/upload", web::post().to(backend::handlers::upload_files))
            .route(
                "/uploads/temp/{filename:.*}",
                web::get().to(backend::handlers::serve_temp_file),
            )
    })
}

#[actix_web::test]
async fn an_upload_keeps_its_name_and_stores_under_a_safe_key() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let tech = agent(&mut pool.get().expect("conn"));
    let dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    let srv = spawn(&pool, &tech, storage);

    let name = "Q3 report (final).pdf";
    let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << >>\n%%EOF\n";
    let mut body = format!(
        "--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{name}\"\r\nContent-Type: application/pdf\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(pdf);
    body.extend_from_slice(b"\r\n--x--\r\n");
    let mut resp = awc::Client::new()
        .post(srv.url("/upload"))
        .insert_header(("content-type", "multipart/form-data; boundary=x"))
        .send_body(body)
        .await
        .expect("send");
    let status = resp.status().as_u16();
    let json: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    assert_eq!(status, 200, "{json}");

    let uploaded = &json[0];
    assert_eq!(uploaded["name"], name, "{json}");
    let url = uploaded["url"].as_str().expect("url");
    let key = url.strip_prefix("/uploads/temp/").expect("a temp upload");
    assert!(
        key.ends_with("_Q3reportfinal.pdf") && !key.contains([' ', '(', ')', '/']),
        "the storage key is one sanitized segment: {url}"
    );

    let id = uploaded["id"].as_i64().expect("id") as i32;
    let row = run_in_workspace(&pool, "test:attachment_display_names", WS, |c| {
        backend::repository::comments::get_attachment_by_id(c, id)
    })
    .expect("attachment row");
    assert_eq!(row.name, name);
    assert_eq!(row.url, url);

    // The uploader's draft downloads under the name as uploaded.
    let mut resp = awc::Client::new()
        .get(srv.url(url))
        .send()
        .await
        .expect("get");
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(
        resp.headers()
            .get("content-disposition")
            .expect("content-disposition")
            .to_str()
            .expect("ascii header"),
        "inline; filename=\"Q3 report (final).pdf\""
    );
    assert_eq!(resp.body().await.expect("body").as_ref(), pdf);
}
