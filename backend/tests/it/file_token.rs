//! File tokens: the read-only credential for one file that the app's `<video>`
//! carries in `?token=`, since a media element loads straight from the server
//! and can't send a header.
//!
//! Proves, through the real `file_auth_middleware`: a session mints one bound
//! to its workspace and to the file path it names; it opens that file whole
//! and by range (206), with a cross-origin resource policy so the app's page
//! can read it, however the path's space or non-ASCII name is percent-encoded;
//! it opens no other file; the mint refuses a missing or malformed path; a
//! token bound to another workspace, or held by a non-member, opens nothing;
//! `sse` and `collab` tokens, and a file token that names no file, don't open
//! files; a file token opens nothing but a file read (not the mint, not
//! another API, not the event stream); no token is still a 401; and signing
//! out ends it.

#![allow(clippy::expect_used)]

use actix_web::dev::Service;
use actix_web::http::header::{CONTENT_RANGE, CROSS_ORIGIN_RESOURCE_POLICY};
use actix_web::middleware::from_fn;
use actix_web::{web, App, HttpMessage, HttpResponse};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::sse::{sse_events_stream, SseState};
use backend::middleware::api_token::{dual_auth_middleware, file_auth_middleware};
use backend::middleware::RequestContext;
use backend::models::{Claims, NewActiveSession, NewAttachment, User};
use backend::repository::workspaces::{add_membership, find_by_id, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;
use backend::utils::jwt::{JwtUtils, FILES_SCOPE, FILE_TOKEN_TTL_SECS};
use backend::utils::storage::{create_storage, StorageConfig, WorkspaceScopedStorage};

use crate::common;

const WS: i32 = 1;

async fn ping() -> HttpResponse {
    HttpResponse::Ok().finish()
}

fn join(conn: &mut backend::db::DbConnection, workspace_id: i32, user: Uuid) {
    let actor = ActorContext::user(user, None).with_workspace(workspace_id);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        add_membership(
            c,
            workspace_id,
            user,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;
        Ok(())
    })
    .expect("add membership");
}

fn session_jwt(pool: &backend::db::Pool, user: &User) -> (Uuid, String) {
    let session = backend::repository::active_sessions::create_session(
        &mut pool.get().expect("conn"),
        NewActiveSession {
            user_uuid: user.uuid,
            device_name: Some("file-token-test".into()),
            ip_address: None,
            user_agent: None,
            location: None,
            expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
            oidc_id_token: None,
            confirmed_at: None,
            provider_amr: None,
        },
    )
    .expect("create session");
    let token = JwtUtils::create_token(user, &session.session_id).expect("mint JWT");
    (session.session_id, token)
}

/// A QuickTime file as an iPhone writes one, bytes differing at every offset.
fn quicktime(len: usize) -> Vec<u8> {
    let mut data = b"\x00\x00\x00\x14ftypqt  \x00\x00\x00\x00qt  ".to_vec();
    data.extend((0..len - data.len()).map(|i| (i * 31 % 251) as u8));
    data
}

#[actix_web::test]
async fn a_file_token_reads_its_file_in_its_workspace_and_nothing_else() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);

    // An agent of workspace 1 who is also an agent of `other`, and an account
    // in neither.
    let (user, ws1_uuid, other_uuid, stranger) = {
        let mut conn = pool.get().expect("conn");
        let user = common::insert_user(&mut conn, "File Token Agent");
        let stranger = common::insert_user(&mut conn, "File Token Stranger");
        let other = common::mint_workspace(&mut conn, "file-token-other", "Other");
        join(&mut conn, WS, user.uuid);
        join(&mut conn, other, user.uuid);
        let uuid_of = |conn: &mut backend::db::DbConnection, id| {
            find_by_id(conn, id)
                .expect("ws lookup")
                .expect("ws exists")
                .uuid
        };
        let ws1 = uuid_of(&mut conn, WS);
        let other = uuid_of(&mut conn, other);
        (user, ws1, other, stranger)
    };
    let (sid, session) = session_jwt(&pool, &user);

    let dir = tempfile::tempdir().expect("storage dir");
    let storage = create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    });
    let ws1_storage = WorkspaceScopedStorage::arc(storage.clone(), WS);
    let server_pool = pool.clone();
    let upload_claims = Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: String::new(),
        platform_role: user.platform_role.clone(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
        file_path: None,
    };
    let user_uuid = user.uuid;
    let srv = actix_test::start(move || {
        // Workspace 1 is the request's origin, as a Host-derived context.
        let ws = WorkspaceContext {
            workspace_id: WS,
            workspace_uuid: ws1_uuid,
            slug: "default".to_string(),
            name: "Default".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let claims = upload_claims.clone();
        App::new()
            .app_data(web::Data::new(server_pool.clone()))
            .app_data(web::Data::new(storage.clone()))
            .app_data(web::Data::new(SseState::new()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                srv.call(req)
            })
            .service(
                web::resource("/upload")
                    .wrap_fn(move |req, srv| {
                        let corr = Uuid::now_v7();
                        let actor = ActorContext::user(user_uuid, Some(corr)).with_workspace(WS);
                        req.extensions_mut().insert(claims.clone());
                        req.extensions_mut()
                            .insert(RequestContext::new(corr, actor));
                        srv.call(req)
                    })
                    .route(web::post().to(backend::handlers::upload_files)),
            )
            .route("/api/events/stream", web::get().to(sse_events_stream))
            .service(
                web::scope("/api/files")
                    .wrap(from_fn(file_auth_middleware))
                    .route(
                        "/token",
                        web::post().to(backend::handlers::files::mint_file_token),
                    )
                    .route(
                        "/temp/{filename:.*}",
                        web::get().to(backend::handlers::serve_temp_file),
                    ),
            )
            .service(
                web::scope("/api")
                    .wrap(from_fn(dual_auth_middleware))
                    .route("/ping", web::get().to(ping)),
            )
    });
    let client = awc::Client::new();

    // Videos in workspace 1, as drafts only their uploader may load.
    let upload = |name: &'static str, data: Vec<u8>| {
        let mut body = format!(
            "--x\r\nContent-Disposition: form-data; name=\"files\"; filename=\"{name}\"\r\nContent-Type: video/quicktime\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(&data);
        body.extend_from_slice(b"\r\n--x--\r\n");
        let req = client
            .post(srv.url("/upload"))
            .insert_header(("content-type", "multipart/form-data; boundary=x"));
        async move {
            let mut resp = req.send_body(body).await.expect("upload");
            let json: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
            assert_eq!(resp.status().as_u16(), 200, "{json}");
            let stored = json[0]["url"].as_str().expect("url").to_string();
            let file = stored.replacen("/uploads/temp/", "/api/files/temp/", 1);
            assert_ne!(file, stored, "an upload lands in temp: {stored}");
            file
        }
    };
    let data = quicktime(5000);
    let file = upload("IMG_0001.MOV", data.clone()).await;
    let other_file = upload("IMG_0002.MOV", quicktime(3000)).await;

    // A session mints a file token for one path.
    let mint = |body: Option<serde_json::Value>| {
        let req = client
            .post(srv.url("/api/files/token"))
            .insert_header(("Authorization", format!("Bearer {session}")));
        async move {
            let mut resp = match body {
                Some(body) => req.send_json(&body).await,
                None => req.send().await,
            }
            .expect("mint");
            let json: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
            (resp.status().as_u16(), json)
        }
    };
    let token_for = |path: String| {
        let minted = mint(Some(serde_json::json!({ "path": path })));
        async move {
            let (status, json) = minted.await;
            assert_eq!(status, 200, "{json}");
            json["token"].as_str().expect("token").to_string()
        }
    };

    // Bound to its workspace, its session and its file.
    let (status, minted) = mint(Some(serde_json::json!({ "path": file }))).await;
    assert_eq!(status, 200, "{minted}");
    assert_eq!(minted["expires_in"], FILE_TOKEN_TTL_SECS);
    let token = minted["token"].as_str().expect("token").to_string();
    let claims = JwtUtils::validate_token(&token).expect("a valid token");
    assert_eq!(claims.scope, FILES_SCOPE);
    assert_eq!(claims.workspace_uuid, Some(ws1_uuid));
    assert_eq!(claims.file_path.as_deref(), Some(file.as_str()));
    assert_eq!(
        claims.sid,
        Some(sid.to_string()),
        "bound to the session that minted it"
    );

    // A path that isn't one file read is refused at the mint.
    for (body, what) in [
        (None, "no body"),
        (Some(serde_json::json!({})), "no path"),
        (Some(serde_json::json!({ "path": "" })), "an empty path"),
        (
            Some(serde_json::json!({ "path": "/api/tickets" })),
            "not a file",
        ),
        (
            Some(serde_json::json!({ "path": "/api/files/token" })),
            "the mint",
        ),
        (
            Some(
                serde_json::json!({ "path": format!("/api/files/temp/../tickets/{}", &file["/api/files/temp/".len()..]) }),
            ),
            "a parent segment",
        ),
        (
            Some(serde_json::json!({ "path": "/api/files/temp/%2E%2E/x.mov" })),
            "an encoded parent segment",
        ),
        (
            Some(serde_json::json!({ "path": format!("{file}?v=2") })),
            "a query",
        ),
        (
            Some(serde_json::json!({ "path": format!("{file}#t=10") })),
            "a fragment",
        ),
    ] {
        let (status, json) = mint(body).await;
        assert_eq!(status, 400, "{what}: {json}");
        assert_eq!(json["code"], "INVALID_FILE_PATH", "{what}");
    }

    let get = |path: String, range: Option<&'static str>| {
        let mut req = client.get(srv.url(&path));
        if let Some(range) = range {
            req = req.insert_header(("Range", range));
        }
        async move {
            let mut resp = req.send().await.expect("get");
            let body = resp.body().limit(1 << 20).await.expect("body").to_vec();
            (resp, body)
        }
    };
    let with = |token: &str| format!("{file}?token={token}");

    // The file, whole and by range, readable from another origin.
    let (resp, bytes) = get(with(&token), None).await;
    assert_eq!(resp.status().as_u16(), 200);
    assert_eq!(bytes, data);
    assert_eq!(
        resp.headers()
            .get(CROSS_ORIGIN_RESOURCE_POLICY)
            .and_then(|v| v.to_str().ok()),
        Some("cross-origin")
    );
    let (resp, bytes) = get(with(&token), Some("bytes=100-199")).await;
    assert_eq!(resp.status().as_u16(), 206);
    assert_eq!(bytes, data[100..200]);
    assert_eq!(
        resp.headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok()),
        Some("bytes 100-199/5000")
    );

    // It opens no other file, even one the caller may read.
    assert_eq!(
        get(format!("{other_file}?token={token}"), None)
            .await
            .0
            .status()
            .as_u16(),
        401,
        "a token for one file on another"
    );
    let (resp, _) = get(
        format!("{other_file}?token={}", token_for(other_file.clone()).await),
        None,
    )
    .await;
    assert_eq!(resp.status().as_u16(), 200, "that file's own token");

    // A name with a space and a non-ASCII letter (an upload's name is
    // sanitised, but a stored file's needn't be): the page may mint for the
    // name as stored and the player load it percent-encoded, or both encode it,
    // in either case of hex.
    let spaced = format!("{}_Site visit é.mov", Uuid::now_v7());
    let spaced_data = quicktime(4000);
    ws1_storage
        .put_file(&spaced_data, &format!("temp/{spaced}"), "video/quicktime")
        .await
        .expect("store spaced file");
    {
        let mut conn = pool.get().expect("conn");
        let actor = ActorContext::user(user.uuid, None).with_workspace(WS);
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            backend::repository::comments::create_attachment(
                c,
                NewAttachment {
                    url: format!("/uploads/temp/{spaced}"),
                    name: "Site visit é.mov".to_string(),
                    file_size: Some(spaced_data.len() as i64),
                    mime_type: Some("video/quicktime".to_string()),
                    checksum: None,
                    comment_id: None,
                    uploaded_by: Some(user.uuid),
                    transcription: None,
                },
            )?;
            Ok(())
        })
        .expect("spaced attachment");
    }
    let raw = format!("/api/files/temp/{spaced}");
    let encoded = raw.replace(' ', "%20").replace('é', "%C3%A9");
    let lower = raw.replace(' ', "%20").replace('é', "%c3%a9");
    for (minted_for, loaded) in [
        (&raw, &encoded),
        (&encoded, &encoded),
        (&encoded, &lower),
        (&lower, &encoded),
    ] {
        let spaced_token = token_for(minted_for.clone()).await;
        let (resp, bytes) = get(format!("{loaded}?token={spaced_token}"), None).await;
        assert_eq!(
            resp.status().as_u16(),
            200,
            "minted for {minted_for}, loaded as {loaded}"
        );
        assert_eq!(bytes, spaced_data);
        let (resp, bytes) = get(format!("{loaded}?token={spaced_token}"), Some("bytes=0-99")).await;
        assert_eq!(resp.status().as_u16(), 206);
        assert_eq!(bytes, spaced_data[..100]);
    }

    // A file token that names no file opens nothing.
    let mut pathless = JwtUtils::validate_token(&token).expect("claims");
    pathless.file_path = None;
    let pathless = jsonwebtoken::encode(
        &jsonwebtoken::Header::default(),
        &pathless,
        &jsonwebtoken::EncodingKey::from_secret(
            std::env::var("JWT_SECRET").expect("JWT_SECRET").as_bytes(),
        ),
    )
    .expect("encode");
    assert_eq!(
        JwtUtils::validate_token(&pathless).expect("valid").scope,
        FILES_SCOPE
    );
    assert_eq!(
        get(with(&pathless), None).await.0.status().as_u16(),
        401,
        "a file token without a path"
    );

    // A token for the caller's other workspace doesn't open a workspace 1
    // file (404, as a file they can't see), nor does a stranger's token for
    // workspace 1 (403 at the membership gate).
    let other_token = JwtUtils::create_file_token(
        &user.uuid.to_string(),
        &user.platform_role,
        other_uuid,
        Some(&sid.to_string()),
        &file,
    )
    .expect("mint other");
    assert_eq!(get(with(&other_token), None).await.0.status().as_u16(), 404);
    let stranger_token = JwtUtils::create_file_token(
        &stranger.uuid.to_string(),
        &stranger.platform_role,
        ws1_uuid,
        None,
        &file,
    )
    .expect("mint stranger");
    assert_eq!(
        get(with(&stranger_token), None).await.0.status().as_u16(),
        403
    );

    // Stream tokens don't open files.
    let sse = JwtUtils::create_sse_token(
        &user.uuid.to_string(),
        &user.platform_role,
        Some(ws1_uuid),
        None,
    )
    .expect("mint sse");
    let collab = JwtUtils::create_collab_token(
        &user.uuid.to_string(),
        &user.platform_role,
        Some(ws1_uuid),
        None,
    )
    .expect("mint collab");
    assert_eq!(
        get(with(&sse), None).await.0.status().as_u16(),
        401,
        "sse token"
    );
    assert_eq!(
        get(with(&collab), None).await.0.status().as_u16(),
        401,
        "collab token"
    );
    assert_eq!(
        get(with("not-a-token"), None).await.0.status().as_u16(),
        401
    );

    // No token: still a 401.
    assert_eq!(get(file.clone(), None).await.0.status().as_u16(), 401);

    // A file token opens nothing but a file read: not the mint, by query or
    // as a bearer; not another API; not the event stream.
    let status =
        |req: awc::ClientRequest| async move { req.send().await.expect("send").status().as_u16() };
    assert_eq!(
        status(client.post(srv.url(&format!("/api/files/token?token={token}")))).await,
        401
    );
    assert_eq!(
        status(
            client
                .post(srv.url("/api/files/token"))
                .insert_header(("Authorization", format!("Bearer {token}")))
        )
        .await,
        401
    );
    assert_eq!(
        status(
            client
                .get(srv.url(&file))
                .insert_header(("Authorization", format!("Bearer {token}")))
        )
        .await,
        401,
        "a file token is not a bearer"
    );
    assert_eq!(
        status(
            client
                .get(srv.url("/api/ping"))
                .insert_header(("Authorization", format!("Bearer {token}")))
        )
        .await,
        401
    );
    assert_eq!(
        status(client.get(srv.url(&format!("/api/ping?token={token}")))).await,
        401
    );
    assert_eq!(
        status(client.get(srv.url(&format!("/api/events/stream?sse_token={token}")))).await,
        403,
        "a file token doesn't open the event stream"
    );

    // Signing out ends it.
    backend::repository::active_sessions::revoke_session_by_uuid(
        &mut pool.get().expect("conn"),
        &sid,
    )
    .expect("sign out");
    assert_eq!(get(with(&token), None).await.0.status().as_u16(), 401);
}
