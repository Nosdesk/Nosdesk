//! `/api/admin/license` and `/api/admin/push-mode`: the stored licence and
//! push mode behind the Licence & Cloud admin page.
//!
//! No valid licence can be minted here (the signing key is not in the repo),
//! so the licence half covers rejection, removal and the page payload; the
//! verification itself is unit-tested in `license.rs` with an ephemeral key.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::{web, App};
use diesel::prelude::*;
use serde_json::{json, Value};

use backend::handlers::admin_license;
use backend::middleware::dual_auth_middleware;
use backend::models::{NewUser, User};
use backend::services::notifications::channels::push_mode::{self, SwitchablePushSender};

mod common;

fn mint_user(conn: &mut diesel::pg::PgConnection, name: &str, platform_role: &str) -> User {
    use backend::schema::users;
    diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: uuid::Uuid::new_v4(),
            name: name.to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: Some(platform_role.to_string()),
        })
        .get_result(conn)
        .expect("insert user")
}

#[actix_web::test]
async fn licence_and_push_mode_round_trip() {
    std::env::remove_var("NOSDESK_DEPLOYMENT_MODE");
    std::env::remove_var("NOSDESK_LICENSE_KEY");
    std::env::remove_var("NOSDESK_PUSH_MODE");
    common::ensure_test_keyring();

    let test_db = common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    // Boot mints this; the test template does not boot.
    backend::sync::system_meta::ensure_instance_id(&mut pool.get().expect("conn"))
        .expect("instance id");
    let admin = mint_user(&mut pool.get().expect("conn"), "Admin", "platform_admin");
    let admin_token = common::mint_api_token(&mut pool.get().expect("conn"), &admin, "admin");
    let member = mint_user(&mut pool.get().expect("conn"), "Member", "user");
    let member_token = common::mint_api_token(&mut pool.get().expect("conn"), &member, "member");

    let resolved = push_mode::resolve(None, None).expect("resolve");
    let push = Arc::new(SwitchablePushSender::new(resolved, "inst-1".into()).expect("sender"));

    let pool_for_app = pool.clone();
    let srv = actix_test::start(move || {
        App::new()
            .app_data(web::Data::new(pool_for_app.clone()))
            .app_data(web::Data::new(push.clone()))
            .service(
                web::scope("/api")
                    .wrap(actix_web::middleware::from_fn(dual_auth_middleware))
                    .configure(admin_license::config),
            )
    });
    let client = awc::Client::new();
    let bearer = |t: &str| ("Authorization", format!("Bearer {t}"));

    // Platform admins only.
    let resp = client
        .get(srv.url("/api/admin/license"))
        .insert_header(bearer(&member_token))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 403);

    // The page payload on a fresh Community instance.
    let mut resp = client
        .get(srv.url("/api/admin/license"))
        .insert_header(bearer(&admin_token))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["edition"], "community");
    assert_eq!(body["license"]["source"], "none");
    assert_eq!(body["license"]["env_managed"], false);
    assert!(body["license"]["details"].is_null());
    assert_eq!(body["push"]["mode"], "default");
    assert!(!body["instance_id"].as_str().unwrap_or_default().is_empty());

    // A key that is not a licence is refused with a reason, and nothing is
    // stored.
    let mut resp = client
        .put(srv.url("/api/admin/license"))
        .insert_header(bearer(&admin_token))
        .send_json(&json!({ "key": "nsk_lic_not.a.jwt" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 400);
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["code"], "license_malformed");
    let stored: i64 = backend::schema::instance_settings::table
        .filter(backend::schema::instance_settings::license_key_encrypted.is_not_null())
        .count()
        .get_result(&mut pool.get().expect("conn"))
        .expect("count");
    assert_eq!(stored, 0);

    // Push mode: stored, applied, reported.
    let mut resp = client
        .put(srv.url("/api/admin/push-mode"))
        .insert_header(bearer(&admin_token))
        .send_json(&json!({ "mode": "relay" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["push"]["mode"], "relay");
    assert_eq!(body["push"]["source"], "stored");
    assert_eq!(body["push"]["sender"], "relay");
    // Relay mode with no licence is inert, and says why.
    assert_eq!(body["push"]["configured"], false);
    assert_eq!(body["push"]["relay"]["last_outcome"], "no_license");

    let mode: Option<String> = backend::schema::instance_settings::table
        .select(backend::schema::instance_settings::push_mode)
        .first(&mut pool.get().expect("conn"))
        .expect("row");
    assert_eq!(mode.as_deref(), Some("relay"));

    // Back to the default clears the stored value.
    let mut resp = client
        .put(srv.url("/api/admin/push-mode"))
        .insert_header(bearer(&admin_token))
        .send_json(&json!({ "mode": "default" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["push"]["mode"], "default");
    assert_eq!(body["push"]["source"], "default");

    let resp = client
        .put(srv.url("/api/admin/push-mode"))
        .insert_header(bearer(&admin_token))
        .send_json(&json!({ "mode": "carrier-pigeon" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 400);

    // Removing when nothing is stored is a harmless no-op.
    let mut resp = client
        .delete(srv.url("/api/admin/license"))
        .insert_header(bearer(&admin_token))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["edition"], "community");

    // The stored path end to end: encrypt, store, reload, decrypt. A token
    // that decrypts but does not verify must read as stored-and-invalid,
    // never as a licence, and a second reload must see nothing new.
    // Appended here rather than in its own test: the licence state is
    // process-global and tests in one binary run in parallel.
    use backend::license::{self, LicenseError, LicenseSource};
    use backend::repository::instance_settings;
    let mut conn = pool.get().expect("conn");
    instance_settings::set_license(&mut conn, "not.a.jwt", "linked", None).expect("store");
    assert!(
        license::reload(&mut conn),
        "a newly stored licence is picked up"
    );
    let state = license::state();
    assert_eq!(state.source, LicenseSource::Linked);
    assert_eq!(state.error(), Some(LicenseError::Malformed));
    assert!(!state.edition().is_enterprise());
    assert!(state.relay_credential().is_none());
    assert!(
        !license::reload(&mut conn),
        "an unchanged row is not a change"
    );

    // A blob the keyring cannot read (here: a sidecar that disagrees) shows
    // as unreadable instead of quietly becoming Community.
    diesel::sql_query("UPDATE instance_settings SET license_key_kek_id = license_key_kek_id + 1")
        .execute(&mut conn)
        .expect("tamper sidecar");
    assert!(license::reload(&mut conn));
    assert_eq!(license::state().error(), Some(LicenseError::Unreadable));
    assert_eq!(license::state().source, LicenseSource::Linked);

    let mut resp = client
        .get(srv.url("/api/admin/license"))
        .insert_header(bearer(&admin_token))
        .send()
        .await
        .expect("send");
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["license"]["error"], "unreadable");
    assert_eq!(body["license"]["source"], "linked");
    assert_eq!(body["edition"], "community");

    license::remove(&mut conn).expect("remove");
    assert_eq!(license::state().source, LicenseSource::None);
}
