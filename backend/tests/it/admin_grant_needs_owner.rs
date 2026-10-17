//! Adding a user as an admin, or mapping a directory group to admin, takes the
//! workspace owner, as changing a member's role to admin does. Both refuse with
//! the member-role endpoint's 403 and message.

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::workspace_members::FORBIDDEN_TIER_MESSAGE;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::sync::actor::ActorContext;

use crate::add_requester::post_user_as;
use crate::common;
use crate::imports_users::{member_role, new_member};

fn staff_body(email: &str, role: &str) -> Value {
    json!({
        "name": email.split('@').next().unwrap_or("someone"),
        "email": email,
        "role": role,
        "password": "correct-horse-battery",
        "send_invitation": false,
    })
}

/// The workspace role held by whoever has `email` as their primary address.
fn role_of(pool: &common::TestPool, ws: i32, email: &str) -> Option<String> {
    use backend::schema::user_emails;
    let mut conn = pool.get().expect("conn");
    let user: Option<Uuid> = user_emails::table
        .filter(user_emails::email.eq(email))
        .filter(user_emails::is_primary.eq(true))
        .select(user_emails::user_uuid)
        .first(&mut *conn)
        .optional()
        .expect("read email");
    user.and_then(|u| member_role(&mut conn, ws, u))
}

#[actix_web::test]
async fn an_admin_cannot_add_an_admin() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let (status, body) = post_user_as(
        &pool,
        &ws,
        ws.admin_uuid,
        staff_body("new.admin@example.com", "admin"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains(FORBIDDEN_TIER_MESSAGE), "{body}");
    assert_eq!(
        role_of(&pool, ws.workspace_id, "new.admin@example.com"),
        None
    );
}

#[actix_web::test]
async fn an_owner_can_add_an_admin() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let owner = new_member(&mut pool.get().expect("conn"), ws.workspace_id, "owner");

    let (status, body) = post_user_as(
        &pool,
        &ws,
        owner,
        staff_body("new.admin@example.com", "admin"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        role_of(&pool, ws.workspace_id, "new.admin@example.com").as_deref(),
        Some("admin")
    );
}

#[actix_web::test]
async fn an_admin_can_add_agents_and_requesters() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let (status, body) = post_user_as(
        &pool,
        &ws,
        ws.admin_uuid,
        staff_body("new.agent@example.com", "technician"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        role_of(&pool, ws.workspace_id, "new.agent@example.com").as_deref(),
        Some("agent")
    );

    let (status, body) = post_user_as(
        &pool,
        &ws,
        ws.admin_uuid,
        staff_body("new.requester@example.com", "user"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    assert_eq!(
        role_of(&pool, ws.workspace_id, "new.requester@example.com").as_deref(),
        Some("member")
    );
}

/// `PUT /api/ldap/settings` in `ws` as `caller`, with `group_config` on an
/// otherwise valid, disabled configuration.
async fn put_ldap_as(
    pool: &common::TestPool,
    ws: &common::WorkspaceSeed,
    caller: Uuid,
    group_config: Value,
) -> (StatusCode, Value) {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: caller.to_string(),
        name: "Caller".to_string(),
        email: "caller@example.com".to_string(),
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
    let actor = ActorContext::user(caller, Some(corr)).with_workspace(ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(web::scope("/api").route(
                "/ldap/settings",
                web::put().to(backend::handlers::ldap_integration::set_ldap_settings),
            )),
    )
    .await;
    let body = json!({
        "settings": {
            "enabled": false,
            "host": "dc01.acme.test",
            "port": 636,
            "tls_mode": "ldaps",
            "verify_certs": true,
            "ca_cert_pem": null,
            "follow_referrals": false,
            "connect_timeout_secs": 5,
            "auth_mode": "simple_bind",
            "bind_dn": "cn=svc,dc=acme,dc=test",
            "user_base_dn": "ou=people,dc=acme,dc=test",
            "username_attribute": "sAMAccountName",
            "user_filter": "(&(objectClass=user)(sAMAccountName={username}))",
            "page_size": 500,
            "attribute_map": {},
            "group_config": group_config,
            "provisioning": {},
        }
    });
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::put()
            .uri("/api/ldap/settings")
            .set_json(body)
            .to_request(),
    )
    .await;
    let status = resp.status();
    let bytes = http_test::read_body(resp).await;
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn mappings(rules: &[(&str, &str)]) -> Value {
    json!({
        "default_role": "member",
        "role_mappings": rules
            .iter()
            .map(|(group, role)| json!({ "group": group, "role": role }))
            .collect::<Vec<_>>(),
    })
}

#[actix_web::test]
async fn an_admin_cannot_map_a_directory_group_to_admin() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    let (status, body) = put_ldap_as(
        &pool,
        &ws,
        ws.admin_uuid,
        mappings(&[("Helpdesk", "agent"), ("IT-Admins", "admin")]),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains(FORBIDDEN_TIER_MESSAGE), "{body}");

    // Agent and member mappings are the admin's to make.
    let (status, body) = put_ldap_as(
        &pool,
        &ws,
        ws.admin_uuid,
        mappings(&[("Helpdesk", "agent"), ("Staff", "member")]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// An owner maps a group to admin; an admin can then edit the configuration
/// with that mapping kept, but not add another group to admin.
#[actix_web::test]
async fn an_owner_maps_admin_and_an_admin_keeps_it() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let owner = new_member(&mut pool.get().expect("conn"), ws.workspace_id, "owner");

    let (status, body) = put_ldap_as(&pool, &ws, owner, mappings(&[("IT-Admins", "admin")])).await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = put_ldap_as(
        &pool,
        &ws,
        ws.admin_uuid,
        mappings(&[("IT-Admins", "admin"), ("Helpdesk", "agent")]),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    let (status, body) = put_ldap_as(
        &pool,
        &ws,
        ws.admin_uuid,
        mappings(&[("IT-Admins", "admin"), ("Helpdesk", "admin")]),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "{body}");
    assert!(body.to_string().contains(FORBIDDEN_TIER_MESSAGE), "{body}");
}
