//! Asset media takes documents as well as photos.
//!
//! A PDF service report is stored as a `document` with no thumbnail; an image
//! is still a `photo`; a type outside the document list is refused.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use image::{ImageFormat, Rgba, RgbaImage};
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
            .route(
                "/assets/{id}/media",
                web::post().to(backend::handlers::asset_media::upload_for_asset),
            )
    })
}

fn photo_png() -> Vec<u8> {
    let image = RgbaImage::from_pixel(32, 32, Rgba([0x30, 0x60, 0x90, 255]));
    let mut out = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .expect("encode");
    out
}

async fn upload(
    srv: &actix_test::TestServer,
    asset_id: i32,
    filename: &str,
    bytes: &[u8],
) -> (u16, serde_json::Value) {
    let mut body = format!(
        "--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--x--\r\n");
    let mut resp = awc::Client::new()
        .post(srv.url(&format!("/assets/{asset_id}/media")))
        .insert_header(("content-type", "multipart/form-data; boundary=x"))
        .send_body(body)
        .await
        .expect("send");
    let status = resp.status().as_u16();
    let json = resp.json().await.unwrap_or(serde_json::Value::Null);
    (status, json)
}

#[actix_web::test]
async fn documents_are_stored_without_thumbnails_and_other_types_are_refused() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let tech = agent(&mut pool.get().expect("conn"));
    let asset_id = run_in_workspace(&pool, "test:asset_media", WS, |c| {
        Ok(common::insert_stock_asset(c, "Forklift"))
    })
    .expect("asset");
    let dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    let srv = spawn(&pool, &tech, storage.clone());

    let pdf = b"%PDF-1.7\n1 0 obj << /Type /Catalog >> endobj\ntrailer << >>\n%%EOF\n";
    let (status, body) = upload(&srv, asset_id, "Service Report.pdf", pdf).await;
    assert_eq!(status, 201, "{body}");
    let row = &body[0];
    assert_eq!(row["kind"], "document", "{body}");
    assert_eq!(row["mime_type"], "application/pdf", "{body}");
    assert!(row["thumbnail_url"].is_null(), "{body}");
    let url = row["url"].as_str().expect("url");
    let filename = url.rsplit('/').next().expect("filename");
    let stored = storage
        .get_file(&format!("ws/{WS}/assets/{asset_id}/media/{filename}"))
        .await
        .expect("the document is stored");
    assert_eq!(stored, pdf);

    let (status, body) = upload(&srv, asset_id, "label.png", &photo_png()).await;
    assert_eq!(status, 201, "{body}");
    assert_eq!(body[0]["kind"], "photo", "{body}");
    assert!(body[0]["thumbnail_url"].is_string(), "{body}");

    let (status, _) = upload(&srv, asset_id, "page.html", b"<html><script></script>").await;
    assert_eq!(status, 400, "an unlisted type is refused");
    let (status, _) = upload(&srv, asset_id, "invoice.pdf", b"PK\x03\x04 not a pdf").await;
    assert_eq!(
        status, 400,
        "content that doesn't match the extension is refused"
    );
}
