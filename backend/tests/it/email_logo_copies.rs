//! Logos and their email copies.
//!
//! An upload records the logo and its copy together and refuses an image it
//! can't read; removing the logo removes its current copy, while copies of
//! earlier logos stay for mail already sent. The scheduler job makes the copy
//! for a logo that has none, wherever the logo is stored, and records it only
//! while the logo is unchanged.

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
use backend::repository::site_settings::{self, Logo};
use backend::schema::site_settings as ss;
use backend::services::email_logos;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;
use backend::utils::email_logo::EmailLogo;
use backend::utils::storage::{create_storage, Storage, StorageConfig};

use crate::common::{self, TestPool};

/// A 400 x 100 dark wordmark inside 20 px of transparent canvas: striped,
/// so like lettering it covers about half its box.
fn logo_png() -> Vec<u8> {
    let image = RgbaImage::from_fn(440, 140, |x, y| {
        let inked =
            (20..420).contains(&x) && (20..120).contains(&y) && (x.is_multiple_of(2) || x == 419);
        if inked {
            Rgba([0x20, 0x20, 0x20, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });
    let mut out = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut out), ImageFormat::Png)
        .expect("encode");
    out
}

fn local_storage(dir: &tempfile::TempDir) -> Arc<dyn Storage> {
    create_storage(StorageConfig::Local {
        base_path: dir.path().to_string_lossy().into_owned(),
    })
}

fn in_workspace<T>(
    pool: &TestPool,
    workspace_id: i32,
    f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
) -> T {
    run_in_workspace(pool, "test:email_logo", workspace_id, f).expect("workspace query")
}

/// Point the workspace's main logo at `url`, with no email copy.
fn set_logo_url(pool: &TestPool, workspace_id: i32, url: &str) {
    in_workspace(pool, workspace_id, |c| {
        site_settings::get_site_settings(c)?;
        diesel::update(ss::table)
            .set((
                ss::logo_url.eq(url),
                ss::email_logo.eq(None::<serde_json::Value>),
            ))
            .execute(c)
    });
}

fn stored_copy(pool: &TestPool, workspace_id: i32) -> Option<EmailLogo> {
    in_workspace(pool, workspace_id, |c| {
        Ok(site_settings::get_site_settings(c)?.email_logo)
    })
    .map(|value| serde_json::from_value(value).expect("an EmailLogo"))
}

/// The storage path of a copy's URL, under the workspace's prefix.
fn copy_key(workspace_id: i32, copy: &EmailLogo) -> String {
    let filename = copy.url.rsplit('/').next().expect("filename");
    format!("ws/{workspace_id}/branding/{filename}")
}

#[actix_web::test]
async fn a_logo_without_a_copy_gets_one_once() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(3);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (ws, other) = (seeded.a, seeded.b);
    let dir = tempfile::tempdir().expect("storage dir");
    let storage = local_storage(&dir);

    storage
        .put_file(
            &logo_png(),
            &format!("ws/{}/branding/logo.png", ws.workspace_id),
            "image/png",
        )
        .await
        .expect("store logo");
    set_logo_url(
        &pool,
        ws.workspace_id,
        &format!("/uploads/branding/{}/logo.png?v=1", ws.workspace_uuid),
    );

    assert_eq!(email_logos::make_missing(&pool, storage.clone()).await, 1);
    let copy = stored_copy(&pool, ws.workspace_id).expect("a copy is recorded");
    assert!(
        copy.url.starts_with(&format!(
            "/uploads/branding/{}/email_logo_",
            ws.workspace_uuid
        )),
        "{}",
        copy.url
    );
    // Trimmed to 400 x 100, fitted to the 48 px height, drawn at 2x.
    assert_eq!((copy.width, copy.height), (192, 48));
    assert!(copy.reads_on_light && !copy.reads_on_dark);
    let png = storage
        .get_file(&copy_key(ws.workspace_id, &copy))
        .await
        .expect("the copy is stored under the workspace");
    let decoded = image::load_from_memory_with_format(&png, ImageFormat::Png).expect("a PNG");
    assert_eq!((decoded.width(), decoded.height()), (384, 96));

    assert_eq!(
        email_logos::make_missing(&pool, storage.clone()).await,
        0,
        "nothing left to make"
    );
    assert!(
        stored_copy(&pool, other.workspace_id).is_none(),
        "a workspace without a logo gets nothing"
    );
}

#[actix_web::test]
async fn a_logo_from_the_shared_folder_gets_its_copy_in_the_workspaces_own() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(3);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("storage dir");
    let storage = local_storage(&dir);

    // Uploaded before branding was stored per workspace.
    storage
        .put_file(&logo_png(), "branding/logo_1699999999.png", "image/png")
        .await
        .expect("store logo");
    set_logo_url(
        &pool,
        ws.workspace_id,
        "/uploads/branding/logo_1699999999.png",
    );

    assert_eq!(email_logos::make_missing(&pool, storage.clone()).await, 1);
    let copy = stored_copy(&pool, ws.workspace_id).expect("a copy is recorded");
    storage
        .get_file(&copy_key(ws.workspace_id, &copy))
        .await
        .expect("the copy is under the workspace's prefix");
}

#[actix_web::test]
async fn a_copy_is_recorded_only_while_its_logo_is_unchanged() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    set_logo_url(&pool, ws.workspace_id, "/uploads/branding/x/logo.png?v=2");
    let value = serde_json::json!({
        "url": "/uploads/branding/x/email_logo_1.png",
        "width": 100, "height": 20, "reads_on_light": true, "reads_on_dark": true,
    });

    let record = |source: &'static str| {
        let value = value.clone();
        in_workspace(&pool, ws.workspace_id, move |c| {
            site_settings::record_email_copy(c, Logo::Main, source, value)
        })
    };
    assert!(
        !record("/uploads/branding/x/logo.png?v=1"),
        "an upload replaced the logo since"
    );
    assert!(record("/uploads/branding/x/logo.png?v=2"));
    assert!(
        !record("/uploads/branding/x/logo.png?v=2"),
        "it already has one"
    );
}

// --- The upload and delete handlers ----------------------------------------

const WS: i32 = 1;

fn admin(conn: &mut PgConnection) -> User {
    use backend::schema::{users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: "Admin".to_string(),
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
            workspace_members::role.eq("admin"),
        ))
        .execute(conn)
        .expect("insert member");
    user
}

/// A test server running every request as `user`, a workspace-1 admin.
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
                "/admin/branding/image",
                web::post().to(backend::handlers::branding::upload_branding_image),
            )
            .route(
                "/admin/branding/image",
                web::delete().to(backend::handlers::branding::delete_branding_image),
            )
    })
}

fn multipart(bytes: &[u8]) -> Vec<u8> {
    let mut body = b"--x\r\nContent-Disposition: form-data; name=\"file\"; filename=\"logo.png\"\r\nContent-Type: image/png\r\n\r\n".to_vec();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--x--\r\n");
    body
}

async fn upload(srv: &actix_test::TestServer, bytes: &[u8]) -> u16 {
    awc::Client::new()
        .post(srv.url("/admin/branding/image?type=logo"))
        .insert_header(("content-type", "multipart/form-data; boundary=x"))
        .send_body(multipart(bytes))
        .await
        .expect("send")
        .status()
        .as_u16()
}

#[actix_web::test]
async fn an_upload_records_the_logo_and_its_copy_and_removal_takes_the_copy() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let admin = admin(&mut pool.get().expect("conn"));
    let dir = tempfile::tempdir().expect("storage dir");
    let storage = local_storage(&dir);
    let srv = spawn(&pool, &admin, storage.clone());

    assert_eq!(upload(&srv, &logo_png()).await, 200);
    let first = stored_copy(&pool, WS).expect("the upload made a copy");
    assert_eq!((first.width, first.height), (192, 48));
    let logo_url = in_workspace(&pool, WS, |c| {
        Ok(site_settings::get_site_settings(c)?.logo_url)
    })
    .expect("logo recorded");
    assert!(logo_url.starts_with(&format!("/uploads/branding/{}/logo.png?v=", Uuid::nil())));
    storage
        .get_file(&copy_key(WS, &first))
        .await
        .expect("copy stored");

    // A file that isn't an image is refused, and nothing changes.
    assert_eq!(upload(&srv, b"not a png").await, 400);
    assert_eq!(stored_copy(&pool, WS), Some(first.clone()));

    // A new logo gets a new copy; the old copy stays for mail already sent.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    assert_eq!(upload(&srv, &logo_png()).await, 200);
    let second = stored_copy(&pool, WS).expect("a new copy");
    assert_ne!(second.url, first.url);
    storage
        .get_file(&copy_key(WS, &first))
        .await
        .expect("the earlier copy stays");

    // Removing the logo removes it and its current copy.
    let status = awc::Client::new()
        .delete(srv.url("/admin/branding/image?type=logo"))
        .send()
        .await
        .expect("send")
        .status();
    assert_eq!(status, 200);
    let settings = in_workspace(&pool, WS, site_settings::get_site_settings);
    assert!(settings.logo_url.is_none() && settings.email_logo.is_none());
    assert!(storage.get_file(&copy_key(WS, &second)).await.is_err());
    assert!(storage
        .get_file(&format!("ws/{WS}/branding/logo.png"))
        .await
        .is_err());
}
