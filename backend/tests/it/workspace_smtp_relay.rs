//! A workspace's own SMTP server through `/api/admin/email/outbound/relay`:
//! validation codes the form maps to fields, the write-only password, the
//! stored-password guard on tests, and relay settings surviving a switch to a
//! verified domain.
//!
//! Tests aim relays at 127.0.0.1, which the egress guard refuses before any
//! connection, so nothing here touches the network.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewUser, User};
use backend::services::outbound_email::OutboundEmailResolver;
use backend::sync::actor::ActorContext;

const WS: i32 = 1;

fn admin(conn: &mut PgConnection) -> User {
    use backend::schema::{user_emails, users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: "Relay Admin".to_string(),
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
    diesel::insert_into(user_emails::table)
        .values((
            user_emails::user_uuid.eq(user.uuid),
            user_emails::email.eq(format!(
                "admin-{}@example.com",
                &user.uuid.simple().to_string()[..8]
            )),
            user_emails::email_type.eq("personal"),
            user_emails::is_primary.eq(true),
            user_emails::is_verified.eq(true),
        ))
        .execute(conn)
        .expect("insert email");
    user
}

fn spawn(pool: &crate::common::TestPool, user: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: "admin@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let user_uuid = user.uuid;
    actix_test::start(move || {
        let pool = pool.clone();
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
        let resolver = Arc::new(OutboundEmailResolver::new(pool.clone(), None));
        App::new()
            .app_data(web::Data::new(pool))
            .app_data(web::Data::new(resolver))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").configure(backend::handlers::email::config))
    })
}

fn relay(host: &str, port: u16, security: &str, username: &str, password: Option<&str>) -> Value {
    json!({
        "from_name": "Acme Support",
        "from_email": "support@acme.example",
        "smtp_host": host,
        "smtp_port": port,
        "smtp_security": security,
        "smtp_username": username,
        "password": password,
    })
}

#[actix_web::test]
async fn relay_save_validate_test_and_keep_across_modes() {
    crate::common::ensure_test_keyring();
    let test_db = crate::common::TestDb::new();
    let pool = test_db.pool_with_size(4);
    let user = admin(&mut pool.get().expect("conn"));
    let srv = spawn(&pool, &user);
    let client = awc::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .finish();
    let put = |body: Value| {
        client
            .put(srv.url("/api/admin/email/outbound/relay"))
            .send_json(&body)
    };

    // Field-level refusals, each with a code the form maps to a field.
    for (body, code) in [
        (
            relay("https://smtp.acme.example", 587, "starttls", "u", Some("p")),
            "RELAY_HOST_INVALID",
        ),
        (
            relay("smtp.acme.example", 465, "starttls", "u", Some("p")),
            "SMTP_CONFIG_MISMATCH",
        ),
        (
            relay("smtp.acme.example", 587, "ssl", "u", Some("p")),
            "RELAY_SECURITY_INVALID",
        ),
    ] {
        let mut resp = put(body).await.expect("send");
        assert_eq!(resp.status(), 400);
        let err: Value = resp.json().await.expect("json");
        assert_eq!(err["code"], code);
    }
    let mut resp = put(json!({
        "from_name": "Acme", "from_email": "not-an-address", "smtp_host": "smtp.acme.example",
        "smtp_port": 587, "smtp_security": "starttls"
    }))
    .await
    .expect("send");
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<Value>().await.expect("json")["code"],
        "FROM_EMAIL_INVALID"
    );

    // Saved: the workspace now sends through its own server; the password is
    // stored but never returned.
    let mut resp = put(relay(
        "127.0.0.1",
        587,
        "starttls",
        "mailer",
        Some("s3cret"),
    ))
    .await
    .expect("send");
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["sending_mode"], "smtp_relay");
    assert_eq!(body["smtp_host"], "127.0.0.1");
    assert_eq!(body["smtp_username"], "mailer");
    assert_eq!(body["password_configured"], true);
    assert_eq!(body["port_security"]["level"], "ok");
    assert!(
        !body.to_string().contains("s3cret"),
        "password must never be returned"
    );

    // Saving again with no password keeps the stored one.
    let mut resp = put(relay("127.0.0.1", 587, "starttls", "mailer", None))
        .await
        .expect("send");
    assert_eq!(
        resp.json::<Value>().await.expect("json")["password_configured"],
        true
    );

    // Testing the saved server with a blank password reuses the stored one:
    // it gets as far as the egress guard, which refuses a loopback host.
    let test = |body: Value| {
        client
            .post(srv.url("/api/admin/email/outbound/relay/test"))
            .send_json(&body)
    };
    let mut resp = test(relay("127.0.0.1", 587, "starttls", "mailer", None))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    let result: Value = resp.json().await.expect("json");
    assert_eq!(result["ok"], false);
    assert_eq!(result["code"], "egress_blocked");
    assert!(result["to"]
        .as_str()
        .unwrap_or_default()
        .starts_with("admin-"));

    // A different host with a blank password must not borrow the stored one:
    // that would send the saved password to whatever server the form names.
    let mut resp = test(relay(
        "smtp.attacker.example",
        587,
        "starttls",
        "mailer",
        None,
    ))
    .await
    .expect("send");
    assert_eq!(resp.status(), 400);
    assert_eq!(
        resp.json::<Value>().await.expect("json")["code"],
        "RELAY_PASSWORD_REQUIRED"
    );
    // Nor a different username on the same host.
    let mut resp = test(relay("127.0.0.1", 587, "starttls", "someone-else", None))
        .await
        .expect("send");
    assert_eq!(
        resp.json::<Value>().await.expect("json")["code"],
        "RELAY_PASSWORD_REQUIRED"
    );

    // No username: an IP-allowlisted relay, tested without any password.
    let mut resp = test(relay("127.0.0.1", 25, "starttls", "", None))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.json::<Value>().await.expect("json")["code"],
        "egress_blocked"
    );

    // Forgetting the password keeps the server.
    let mut resp = client
        .delete(srv.url("/api/admin/email/outbound/relay/password"))
        .send()
        .await
        .expect("send");
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["password_configured"], false);
    assert_eq!(body["smtp_host"], "127.0.0.1");

    // Switching to a verified domain keeps the relay settings for later.
    let mut resp = client
        .put(srv.url("/api/admin/email/outbound/domain"))
        .send_json(&json!({ "from_name": "Acme", "from_email": "support@acme.example" }))
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "{:?}", resp.body().await);
    let mut resp = client
        .get(srv.url("/api/admin/email/outbound"))
        .send()
        .await
        .expect("send");
    let body: Value = resp.json().await.expect("json");
    assert_eq!(body["sending_mode"], "verified_domain");
    assert_eq!(body["smtp_host"], "127.0.0.1");
    assert_eq!(body["smtp_username"], "mailer");
}
