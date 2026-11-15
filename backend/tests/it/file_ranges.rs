//! A stored file streams through the file routes with byte ranges: a 200 for
//! the whole file, a 206 for one range (a suffix included), a 416 for a start
//! past the end, and the whole file for a multi-range or malformed request.
//! Bodies are checked byte for byte against what was uploaded.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::header::{
    ACCEPT_RANGES, CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE,
};
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewUser, User};
use backend::sync::actor::ActorContext;
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
        file_path: None,
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

/// A QuickTime file as an iPhone writes one: an `ftyp qt` box, then bytes
/// that differ at every offset a range might land on.
fn quicktime(len: usize) -> Vec<u8> {
    let mut data = b"\x00\x00\x00\x14ftypqt  \x00\x00\x00\x00qt  ".to_vec();
    data.extend((0..len - data.len()).map(|i| (i * 31 % 251) as u8));
    data
}

#[actix_web::test]
async fn a_stored_file_is_served_whole_or_by_range() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let tech = agent(&mut pool.get().expect("conn"));
    let dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    let srv = spawn(&pool, &tech, storage);

    let data = quicktime(5000);
    let len = data.len();
    let mut body = b"--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"IMG_0001.MOV\"\r\nContent-Type: video/quicktime\r\n\r\n".to_vec();
    body.extend_from_slice(&data);
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
    let url = json[0]["url"].as_str().expect("url").to_string();

    let get = |range: Option<&'static str>| {
        let mut req = awc::Client::new().get(srv.url(&url));
        if let Some(range) = range {
            req = req.insert_header(("Range", range));
        }
        async move {
            let mut resp = req.send().await.expect("get");
            let body = resp.body().await.expect("body").to_vec();
            (resp, body)
        }
    };
    let header = |resp: &awc::ClientResponse<_>, name| {
        resp.headers()
            .get(name)
            .map(|v: &actix_web::http::header::HeaderValue| v.to_str().expect("ascii").to_string())
    };

    // The whole file: streamed with one Content-Length, and the caching,
    // disposition and type the route always sent.
    let (resp, bytes) = get(None).await;
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(bytes, data);
    assert_eq!(resp.headers().get_all(CONTENT_LENGTH).count(), 1);
    assert_eq!(header(&resp, CONTENT_LENGTH), Some(len.to_string()));
    assert_eq!(header(&resp, ACCEPT_RANGES).as_deref(), Some("bytes"));
    assert_eq!(
        header(&resp, CONTENT_TYPE).as_deref(),
        Some("video/quicktime")
    );
    assert_eq!(
        header(&resp, CACHE_CONTROL).as_deref(),
        Some("private, max-age=3600")
    );
    assert_eq!(
        header(&resp, CONTENT_DISPOSITION).as_deref(),
        Some("inline; filename=\"IMG_0001.MOV\"")
    );
    assert_eq!(header(&resp, CONTENT_RANGE), None);

    // One range: a 206 holding exactly those bytes.
    for (range, start, end) in [
        ("bytes=0-1", 0, 1),
        ("bytes=100-", 100, len - 1),
        ("bytes=-500", len - 500, len - 1),
        ("bytes=4990-9999", 4990, len - 1),
    ] {
        let (resp, bytes) = get(Some(range)).await;
        assert_eq!(resp.status().as_u16(), 206, "{range}");
        assert_eq!(bytes, data[start..=end], "{range}");
        assert_eq!(
            header(&resp, CONTENT_RANGE),
            Some(format!("bytes {start}-{end}/{len}")),
            "{range}"
        );
        assert_eq!(resp.headers().get_all(CONTENT_LENGTH).count(), 1);
        assert_eq!(
            header(&resp, CONTENT_LENGTH),
            Some((end - start + 1).to_string())
        );
        assert_eq!(header(&resp, ACCEPT_RANGES).as_deref(), Some("bytes"));
        assert_eq!(
            header(&resp, CONTENT_TYPE).as_deref(),
            Some("video/quicktime")
        );
    }

    // A start past the end: 416, naming the length.
    for range in ["bytes=5000-", "bytes=6000-7000"] {
        let (resp, bytes) = get(Some(range)).await;
        assert_eq!(resp.status().as_u16(), 416, "{range}");
        assert_eq!(header(&resp, CONTENT_RANGE), Some(format!("bytes */{len}")));
        assert!(bytes.is_empty());
    }

    // Several ranges, or one that can't be read: the whole file.
    for range in ["bytes=0-1,3-4", "bytes=5-2", "bytes=abc", "items=0-1"] {
        let (resp, bytes) = get(Some(range)).await;
        assert_eq!(resp.status().as_u16(), 200, "{range}");
        assert_eq!(bytes, data, "{range}");
        assert_eq!(header(&resp, CONTENT_RANGE), None);
    }
}
