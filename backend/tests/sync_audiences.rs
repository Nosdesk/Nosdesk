//! Sync and the user routes send each viewer only what is meant for them.
//!
//! The workspace holds a webhook with a custom header, a mail channel, an
//! audit read, a knowledge gap and everyone's people rows. A workspace admin,
//! an agent and a requester-role member each read the sync delta and the
//! people list.
#![allow(clippy::expect_used)]

use std::collections::HashSet;

use actix_web::dev::Service;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, NewAsset, NewChannel, NewTicket, NewUser, NewUserEmail, SyncAggregate, SyncOp, User,
    UserUpdate,
};
use backend::sync::actor::ActorContext;
use backend::sync::emit::{self, SyncEmit};
use backend::sync::session::run_in_workspace;

mod common;

const WS: i32 = 1;
/// The webhook's custom header value: no client receives it.
const HEADER_SECRET: &str = "Bearer header-value-for-the-sink";

fn email_of(uuid: Uuid) -> String {
    format!("{}@audience.test", uuid.simple())
}

fn member(conn: &mut PgConnection, name: &str, role: &str) -> User {
    use backend::schema::{user_emails, users, workspace_members};
    let user: User = diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
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
            workspace_members::role.eq(role),
        ))
        .execute(conn)
        .expect("insert workspace member");
    diesel::insert_into(user_emails::table)
        .values(&NewUserEmail {
            user_uuid: user.uuid,
            email: email_of(user.uuid),
            email_type: "personal".to_string(),
            is_primary: true,
            is_verified: true,
            source: None,
        })
        .execute(conn)
        .expect("primary email");
    user
}

fn spawn(pool: &common::TestPool, user: &User) -> actix_test::TestServer {
    let pool = pool.clone();
    let claims = Claims {
        sub: user.uuid.to_string(),
        name: user.name.clone(),
        email: email_of(user.uuid),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
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
        };
        App::new()
            .app_data(web::Data::new(pool))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(ws.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .route(
                        "/sync/delta",
                        web::get().to(backend::handlers::sync::delta::delta),
                    )
                    .route(
                        "/users/paginated",
                        web::get().to(backend::handlers::get_paginated_users),
                    )
                    .route(
                        "/assets/paginated",
                        web::get().to(backend::handlers::get_paginated_devices),
                    )
                    .route(
                        "/assets/{id}",
                        web::get().to(backend::handlers::get_device_by_id),
                    )
                    .route(
                        "/users/{uuid}/assets",
                        web::get().to(backend::handlers::get_user_devices),
                    ),
            )
    })
}

async fn get(srv: &actix_test::TestServer, uri: &str) -> Value {
    let mut resp = awc::Client::new()
        .get(srv.url(uri))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 200, "GET {uri}");
    resp.json::<Value>()
        .limit(16 * 1024 * 1024)
        .await
        .expect("json")
}

/// The delta from the start of the feed, once it covers the newest action
/// (it serves only rows below the cluster-wide commit horizon).
async fn delta(srv: &actix_test::TestServer, viewer: Uuid, newest: i64) -> Vec<Value> {
    let uri = format!("/api/sync/delta?from=0&limit=5000&groups=workspace:{WS},user:{viewer}");
    for _ in 0..200 {
        let body = get(srv, &uri).await;
        if body["last_sync_id"].as_i64().is_some_and(|id| id >= newest) {
            return body["actions"].as_array().expect("actions").clone();
        }
        actix_web::rt::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the delta never reached the newest action ({newest})");
}

fn kinds(rows: &[Value]) -> HashSet<String> {
    rows.iter()
        .filter_map(|r| r["aggregate"].as_str().map(str::to_string))
        .collect()
}

/// The latest `user` row about `who` in a delta.
fn user_row(rows: &[Value], who: Uuid) -> Value {
    rows.iter()
        .filter(|r| r["aggregate"] == json!("user"))
        .map(|r| r["data"].clone())
        .rfind(|d| d["uuid"] == json!(who))
        .unwrap_or_else(|| panic!("no user row about {who}"))
}

fn has(row: &Value, field: &str) -> bool {
    row.get(field).is_some_and(|v| !v.is_null())
}

struct Fixture {
    pool: common::TestPool,
    admin: User,
    agent: User,
    member: User,
    newest: i64,
    /// An asset whose primary user is the admin.
    admins_asset: i32,
    _db: common::TestDb,
}

fn seed() -> Fixture {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);
    let (admin, agent, member) = {
        let mut conn = pool.get().expect("conn");
        (
            member(&mut conn, "Audience Admin", "admin"),
            member(&mut conn, "Audience Agent", "agent"),
            member(&mut conn, "Audience Member", "member"),
        )
    };
    let admins_asset = run_in_workspace(&pool, "test:sync_audiences", WS, |c| {
        use backend::repository as repo;
        for user in [&admin, &agent, &member] {
            repo::users::update_user(
                &user.uuid,
                UserUpdate {
                    name: None,
                    pronouns: Some("they/them".to_string()),
                    avatar_url: None,
                    banner_url: None,
                    avatar_thumb: None,
                    microsoft_uuid: None,
                    updated_at: None,
                },
                c,
                None,
            )?;
        }
        repo::webhooks::create_webhook(
            c,
            "Sink".to_string(),
            "https://sink.invalid/hook".to_string(),
            "signing-secret".to_string(),
            vec!["ticket.created".to_string()],
            Some(json!({ "Authorization": HEADER_SECRET })),
            Some(admin.uuid),
        )?;
        repo::channels::create(
            c,
            NewChannel {
                provider: "imap".to_string(),
                name: "Helpdesk mailbox".to_string(),
                enabled: false,
                config: json!({ "host": "imap.audience.test", "username": "helpdesk" }),
            },
        )?;
        emit::record(
            c,
            SyncEmit {
                aggregate: SyncAggregate::Data,
                aggregate_id: "audit".to_string(),
                op: SyncOp::Insert,
                event_type: "data.audit.read",
                data: json!({ "filter": {}, "rows_returned": 3 }),
                groups: backend::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        let state = repo::workflow_states::default_state(c)?.id;
        let ticket: i32 = diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Payroll export failed".to_string(),
                workflow_state_id: state,
                requester_uuid: Some(admin.uuid),
                ..Default::default()
            })
            .returning(backend::schema::tickets::id)
            .get_result(c)?;
        repo::knowledge_gaps::flag_ticket(c, ticket, "Payroll export failed", admin.uuid, None)?;
        let asset: i32 = diesel::insert_into(backend::schema::assets::table)
            .values(&NewAsset {
                name: "Admin desktop".to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: Some(admin.uuid),
                purchase_date: None,
                asset_tag: None,
                kind: "computer".to_string(),
                attributes: json!({}),
                quantity: None,
                unit: None,
                external_sync_source: None,
                low_stock_threshold: None,
            })
            .returning(backend::schema::assets::id)
            .get_result(c)?;
        Ok::<_, diesel::result::Error>(asset)
    })
    .expect("seed records");
    let newest: i64 = {
        use backend::schema::sync_actions;
        run_in_workspace(&pool, "test:sync_audiences", WS, |c| {
            sync_actions::table
                .select(diesel::dsl::max(sync_actions::sync_id))
                .first::<Option<i64>>(c)
        })
        .expect("newest action")
        .expect("some action")
    };
    Fixture {
        pool,
        admin,
        agent,
        member,
        newest,
        admins_asset,
        _db: db,
    }
}

#[actix_web::test]
async fn each_role_receives_only_its_records() {
    let f = seed();
    for (who, viewer) in [
        ("member", &f.member),
        ("agent", &f.agent),
        ("admin", &f.admin),
    ] {
        let srv = spawn(&f.pool, viewer);
        let rows = delta(&srv, viewer.uuid, f.newest).await;
        let seen = kinds(&rows);
        let is_admin = viewer.uuid == f.admin.uuid;
        let is_staff = viewer.uuid != f.member.uuid;
        for kind in ["webhook", "channel", "data"] {
            assert_eq!(seen.contains(kind), is_admin, "{who}: {kind} rows");
        }
        assert_eq!(
            seen.contains("knowledge_gap"),
            is_staff,
            "{who}: knowledge gap rows"
        );
        for row in rows.iter().filter(|r| r["aggregate"] == json!("webhook")) {
            assert!(
                !row.to_string().contains(HEADER_SECRET),
                "{who}: a webhook row carries a header value: {row}"
            );
        }

        let own = user_row(&rows, viewer.uuid);
        assert!(has(&own, "email"), "{who}: own row: {own}");
        for other in [&f.admin, &f.agent, &f.member]
            .into_iter()
            .filter(|u| u.uuid != viewer.uuid)
        {
            let row = user_row(&rows, other.uuid);
            assert!(has(&row, "name"), "{who}: another's name: {row}");
            for field in ["email", "workspace_role", "platform_role"] {
                assert_eq!(
                    has(&row, field),
                    is_staff,
                    "{who}: another's {field}: {row}"
                );
            }
        }
    }
}

#[actix_web::test]
async fn a_member_gets_names_from_the_people_list() {
    let f = seed();
    let people = |body: Value| -> Vec<Value> { body["data"].as_array().expect("data").clone() };
    let local_part = f.admin.uuid.simple().to_string();

    let member = spawn(&f.pool, &f.member);
    let rows = people(get(&member, "/api/users/paginated?page=1&pageSize=50").await);
    for row in rows.iter().filter(|r| r["uuid"] != json!(f.member.uuid)) {
        assert!(has(row, "name"), "member: another's name: {row}");
        for field in ["email", "workspace_role", "platform_role"] {
            assert!(!has(row, field), "member: another's {field}: {row}");
        }
    }
    let own = rows
        .iter()
        .find(|r| r["uuid"] == json!(f.member.uuid))
        .expect("own row");
    assert!(has(own, "email"), "member: own row: {own}");
    let found = people(
        get(
            &member,
            &format!("/api/users/paginated?page=1&pageSize=50&search={local_part}"),
        )
        .await,
    );
    assert!(
        found.is_empty(),
        "member: searching an address finds no one: {found:?}"
    );

    let agent = spawn(&f.pool, &f.agent);
    let rows = people(get(&agent, "/api/users/paginated?page=1&pageSize=50").await);
    let admin_row = rows
        .iter()
        .find(|r| r["uuid"] == json!(f.admin.uuid))
        .expect("admin row");
    assert!(
        has(admin_row, "email"),
        "agent: another's email: {admin_row}"
    );
    let found = people(
        get(
            &agent,
            &format!("/api/users/paginated?page=1&pageSize=50&search={local_part}"),
        )
        .await,
    );
    assert!(
        found.iter().any(|r| r["uuid"] == json!(f.admin.uuid)),
        "agent: finds someone by address"
    );
}

#[actix_web::test]
async fn an_asset_names_its_user_by_name_for_a_member() {
    let f = seed();
    for (who, viewer) in [("member", &f.member), ("agent", &f.agent)] {
        let srv = spawn(&f.pool, viewer);
        let is_staff = viewer.uuid != f.member.uuid;
        let one = get(&srv, &format!("/api/assets/{}", f.admins_asset)).await;
        let page = get(&srv, "/api/assets/paginated?page=1&pageSize=100").await;
        let by_user = get(&srv, &format!("/api/users/{}/assets", f.admin.uuid)).await;
        let find = |rows: &Value| -> Value {
            rows.as_array()
                .expect("assets")
                .iter()
                .find(|a| a["id"] == json!(f.admins_asset))
                .cloned()
                .expect("the admin's asset")
        };
        for asset in [one, find(&page["data"]), find(&by_user)] {
            let user = &asset["primary_user"];
            assert_eq!(user["uuid"], json!(f.admin.uuid), "{who}: {asset}");
            assert!(has(user, "name"), "{who}: the asset user's name: {asset}");
            for field in ["email", "role"] {
                assert_eq!(
                    has(user, field),
                    is_staff,
                    "{who}: the asset user's {field}: {asset}"
                );
            }
        }
    }
}
