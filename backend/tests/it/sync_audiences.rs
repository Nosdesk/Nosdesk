//! Sync sends each kind of record only to the people it is for.
//!
//! One workspace holds a record of every kind whose audience is narrower than
//! "everyone in the workspace": a webhook (with a custom header), a mail
//! channel, two knowledge gaps (one drafting in a page only the admin can
//! open), an audit read, a group, two asset loans, a notification, a member's
//! draft upload, dashboard layouts, and a project holding a ticket a member
//! can't see. A workspace admin, a non-admin agent and a requester-role member
//! each read the feed the three ways a client does (the bootstrap snapshot,
//! the delta pull and the live stream) and the people routes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use futures::StreamExt;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::handlers::sse::{Envelope, SseEvent, SseState, SseStream};
use backend::handlers::sync::delta::ActionRow;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, DocumentationStatus, NewAsset, NewAttachment, NewChannel, NewDocumentationPage,
    NewGroup, NewProject, NewTicket, NewUserEmail, ProjectStatus, SyncAggregate, SyncOp,
    UserUpdate, WebhookUpdate,
};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::sync::actor::ActorContext;
use backend::sync::emit::{self, SyncEmit};
use backend::sync::session::run_in_workspace;
use backend::sync::visibility::SyncViewer;

use crate::common::{self, TestPool, WorkspaceSeed};

const REF: &str = "test:sync_audiences";
/// The webhook's custom header value: no client receives it.
const HEADER_SECRET: &str = "Bearer header-value-for-the-sink";

struct Fixture {
    pool: TestPool,
    ws: WorkspaceSeed,
    admin: Uuid,
    agent: Uuid,
    member: Uuid,
    /// A project ticket the member requested.
    member_ticket: i32,
    /// A project ticket the member can't see.
    hidden_ticket: i32,
    /// A gap being written in a page only the admin can open.
    gap_in_admins_page: i64,
    /// An asset whose primary user is the admin.
    admins_asset: i32,
}

fn email_of(uuid: Uuid) -> String {
    format!("{}@audience.test", uuid.simple())
}

fn seed(db: &common::TestDb) -> Fixture {
    common::ensure_test_keyring();
    let pool = db.pool_with_size(8);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let (admin, member) = (ws.admin_uuid, ws.member_uuid);
    let agent = common::insert_plain_user(&mut pool.get().expect("conn"), "Audience Agent");
    {
        let mut conn = pool.get().expect("conn");
        for user in [admin, agent, member] {
            diesel::insert_into(backend::schema::user_emails::table)
                .values(&NewUserEmail {
                    user_uuid: user,
                    email: email_of(user),
                    email_type: "personal".to_string(),
                    is_primary: true,
                    is_verified: true,
                    source: None,
                })
                .execute(&mut conn)
                .expect("primary email");
            // Each of them has arranged their dashboard.
            diesel::insert_into(backend::schema::user_preferences::table)
                .values((
                    backend::schema::user_preferences::user_uuid.eq(user),
                    backend::schema::user_preferences::dashboard_layout
                        .eq(json!({ "widgets": [{ "id": format!("layout-of-{user}"), "visible": true }] })),
                ))
                .on_conflict(backend::schema::user_preferences::user_uuid)
                .do_update()
                .set(
                    backend::schema::user_preferences::dashboard_layout
                        .eq(json!({ "widgets": [{ "id": format!("layout-of-{user}"), "visible": true }] })),
                )
                .execute(&mut conn)
                .expect("dashboard layout");
        }
    }

    let (member_ticket, hidden_ticket, gap_in_admins_page, admins_asset) = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        use backend::repository as repo;
        add_membership(
            c,
            ws.workspace_id,
            agent,
            "agent",
            SeatWriteAuthority::ControlPlane,
        )?;

        // People: everyone's row goes out again, now with a dashboard layout.
        for user in [admin, agent, member] {
            repo::users::update_user(
                &user,
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

        // Admin configuration: a webhook given a custom header, a mail channel.
        repo::webhooks::update_webhook(
            c,
            ws.webhook_id,
            WebhookUpdate {
                name: None,
                url: None,
                secret: None,
                events: None,
                enabled: None,
                headers: Some(json!({ "Authorization": HEADER_SECRET })),
                last_triggered_at: None,
                failure_count: None,
                disabled_reason: None,
            },
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
        // Someone read the audit log.
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

        // Staff working data: a group, a knowledge gap.
        repo::groups::create_group(
            c,
            NewGroup {
                name: "Escalations".to_string(),
                description: None,
                color: None,
                created_by: Some(admin),
            },
        )?;
        let state = repo::workflow_states::default_state(c)?.id;
        let ticket = |c: &mut backend::db::DbConnection, title: &str, requester: Uuid| {
            diesel::insert_into(backend::schema::tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state,
                    requester_uuid: Some(requester),
                    ..Default::default()
                })
                .returning(backend::schema::tickets::id)
                .get_result::<i32>(c)
        };
        let member_ticket = ticket(c, "My laptop won't start", member)?;
        let hidden_ticket = ticket(c, "Restructure the payroll team", admin)?;
        repo::knowledge_gaps::flag_ticket(c, hidden_ticket, "Restructure", admin, None)?;
        // A second gap, being written in a page restricted to the admin.
        let admins_page: i32 = diesel::insert_into(backend::schema::documentation_pages::table)
            .values(&NewDocumentationPage {
                uuid: Uuid::new_v4(),
                title: "Payroll export runbook".to_string(),
                slug: "payroll-export-runbook".to_string(),
                icon: None,
                cover_image: None,
                status: DocumentationStatus::Draft,
                created_by: admin,
                last_edited_by: admin,
                parent_id: None,
                display_order: None,
                is_public: false,
                is_template: false,
                yjs_state_vector: None,
                yjs_document: None,
                yjs_client_id: None,
                has_unsaved_changes: false,
            })
            .returning(backend::schema::documentation_pages::id)
            .get_result(c)?;
        repo::set_page_visibility(c, admins_page, vec![], vec![admin], None)?;
        let (gap, _, _) =
            repo::knowledge_gaps::flag_ticket(c, member_ticket, "My laptop", admin, None)?;
        repo::knowledge_gaps::start_drafting(c, gap.id, admins_page)?;

        // A project holding both tickets.
        let project = repo::projects::create_project(
            c,
            NewProject {
                name: "Office move".to_string(),
                description: None,
                status: ProjectStatus::Active,
                start_date: None,
                end_date: None,
            },
            None,
        )?;
        repo::projects::add_ticket_to_project(c, project.id, member_ticket)?;
        repo::projects::add_ticket_to_project(c, project.id, hidden_ticket)?;

        // Two loans: one to the member, one to the agent.
        for (name, borrower) in [("Member laptop", member), ("Agent laptop", agent)] {
            let asset: i32 = diesel::insert_into(backend::schema::assets::table)
                .values(&NewAsset {
                    name: name.to_string(),
                    serial_number: None,
                    manufacturer: None,
                    model: None,
                    location: None,
                    notes: None,
                    primary_user_uuid: None,
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
            repo::asset_loans::issue(
                c,
                repo::asset_loans::IssueLoan {
                    asset_id: asset,
                    borrower_user_uuid: borrower,
                    loaned_at: None,
                    due_back: None,
                    ticket_id: None,
                    notes: Some(format!("{name} for travel")),
                    actor_uuid: Some(admin),
                },
            )
            .map_err(|e| diesel::result::Error::QueryBuilderError(format!("{e:?}").into()))?;
        }

        // The member's own: a notification and a draft upload.
        emit::record(
            c,
            SyncEmit {
                aggregate: SyncAggregate::Notification,
                aggregate_id: "4242".to_string(),
                op: SyncOp::Insert,
                event_type: "notification.created",
                data: json!({ "id": 4242, "type": "ticket_updated", "title": "Your ticket moved" }),
                groups: vec![format!("user:{member}")],
                causation_id: None,
            },
        )?;
        repo::comments::create_attachment(
            c,
            NewAttachment {
                url: "/uploads/temp/draft.png".to_string(),
                name: "draft.png".to_string(),
                file_size: Some(10),
                mime_type: Some("image/png".to_string()),
                checksum: None,
                comment_id: None,
                uploaded_by: Some(member),
                transcription: None,
            },
        )?;

        // A plugin changed: every member's session loads plugins.
        emit::record(
            c,
            SyncEmit {
                aggregate: SyncAggregate::Plugin,
                aggregate_id: ws.plugin_id.to_string(),
                op: SyncOp::Update,
                event_type: "plugin.updated",
                data: json!({ "id": ws.plugin_id, "name": ws.plugin_name, "state": "disabled" }),
                groups: backend::sync::groups::workspace(),
                causation_id: None,
            },
        )?;
        // The admin's own desktop.
        let admins_asset: i32 = diesel::insert_into(backend::schema::assets::table)
            .values(&NewAsset {
                name: "Admin desktop".to_string(),
                serial_number: None,
                manufacturer: None,
                model: None,
                location: None,
                notes: None,
                primary_user_uuid: Some(admin),
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
        Ok::<_, diesel::result::Error>((member_ticket, hidden_ticket, gap.id, admins_asset))
    })
    .expect("seed records");

    Fixture {
        pool,
        ws,
        admin,
        agent,
        member,
        member_ticket,
        hidden_ticket,
        gap_in_admins_page,
        admins_asset,
    }
}

/// GET `uri` as `viewer` in the fixture's workspace.
async fn get_as(f: &Fixture, viewer: Uuid, uri: &str) -> String {
    let claims = Claims {
        sub: viewer.to_string(),
        name: "Viewer".to_string(),
        email: email_of(viewer),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    };
    let workspace = WorkspaceContext {
        workspace_id: f.ws.workspace_id,
        workspace_uuid: f.ws.workspace_uuid,
        slug: f.ws.slug.clone(),
        name: f.ws.slug.clone(),
        organisation_id: None,
        custom_domain: None,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(viewer, Some(corr)).with_workspace(f.ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(f.pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .configure(backend::handlers::sync::config)
                    .route(
                        "/users/paginated",
                        web::get().to(backend::handlers::get_paginated_users),
                    )
                    .route(
                        "/users/{uuid}",
                        web::get().to(backend::handlers::get_user_by_uuid),
                    )
                    .route(
                        "/assets/paginated/excluding",
                        web::get().to(backend::handlers::assets::get_paginated_devices_excluding),
                    )
                    .route(
                        "/assets/{id}",
                        web::get().to(backend::handlers::assets::get_device_by_id),
                    ),
            ),
    )
    .await;
    let resp =
        http_test::call_service(&app, http_test::TestRequest::get().uri(uri).to_request()).await;
    let status = resp.status();
    let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8 body");
    assert_eq!(status, StatusCode::OK, "GET {uri}: {body}");
    body
}

/// The delta from the start of the feed, as `viewer` asks for it, once it
/// covers the newest action. The delta serves only settled rows (below the
/// commit horizon, which is cluster-wide), so another test's open
/// transaction can hold the newest rows back for a moment.
async fn delta_rows(f: &Fixture, viewer: Uuid, newest: i64) -> Vec<Value> {
    let uri = format!(
        "/api/sync/delta?from=0&limit=5000&groups=workspace:{},user:{viewer}",
        f.ws.workspace_id
    );
    for _ in 0..200 {
        let body: Value = serde_json::from_str(&get_as(f, viewer, &uri).await).expect("delta json");
        if body["last_sync_id"].as_i64().is_some_and(|id| id >= newest) {
            return body["actions"].as_array().expect("actions").clone();
        }
        actix_web::rt::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("the delta never reached the newest action ({newest})");
}

/// The bootstrap snapshot's records, as `viewer` asks for it.
async fn bootstrap_rows(f: &Fixture, viewer: Uuid) -> Vec<Value> {
    let uri = format!(
        "/api/sync/bootstrap?groups=workspace:{},user:{viewer}&schema=test",
        f.ws.workspace_id
    );
    get_as(f, viewer, &uri)
        .await
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("bootstrap line"))
        // Records only; the stream also carries its cursor and schema.
        .filter(|row| row.get("__model__").is_some())
        .collect()
}

/// Every action recorded in the workspace, shaped as the live stream carries
/// them, plus one of a kind no server version knows.
fn every_action(f: &Fixture) -> Vec<Value> {
    use backend::schema::sync_actions;
    let rows: Vec<ActionRow> = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        sync_actions::table
            .order(sync_actions::sync_id.asc())
            .select((
                sync_actions::sync_id,
                sync_actions::aggregate,
                sync_actions::aggregate_id,
                sync_actions::op,
                sync_actions::event_type,
                sync_actions::schema_version,
                sync_actions::data,
                sync_actions::groups,
                sync_actions::actor_uuid,
                sync_actions::actor_kind,
                sync_actions::actor_ref,
                sync_actions::correlation_id,
                sync_actions::causation_id,
                sync_actions::occurred_at,
                sync_actions::xid8,
            ))
            .load::<ActionRow>(c)
    })
    .expect("load actions");
    let mut rows: Vec<Value> = rows
        .iter()
        .map(|r| serde_json::to_value(r).expect("row json"))
        .collect();
    let mut unknown = rows.last().cloned().expect("a row");
    unknown["aggregate"] = json!("record_kind_from_the_future");
    unknown["data"] = json!({ "secret_note": "only for a newer server's audience" });
    rows.push(unknown);
    rows
}

/// The rows of `actions` that `viewer`'s live stream sends.
async fn stream_rows(f: &Fixture, viewer: Uuid, actions: Vec<Value>) -> Vec<Value> {
    let (sync_viewer, allowed) = run_in_workspace(&f.pool, REF, f.ws.workspace_id, |c| {
        let user = backend::repository::users::get_user_by_uuid(&viewer, c)?;
        Ok::<_, diesel::result::Error>((
            SyncViewer::resolve(c, &user),
            backend::sync::groups::allowed_for_user(c, &user)?,
        ))
    })
    .expect("viewer");
    let registry =
        Arc::new(backend::services::connection_registry::ConnectionRegistry::with_limits(4, 16));
    let guard = registry
        .try_acquire((viewer, f.ws.workspace_id))
        .expect("connection slot");
    let envelope = Envelope {
        id: 1,
        event: SseEvent::SyncActions {
            actions: Value::Array(actions),
            last_xid8: 0,
            last_sync_id: 0,
            timestamp: chrono::Utc::now(),
        },
        source_client_id: None,
    };
    let mut stream = SseStream::new(
        Vec::new(),
        vec![envelope],
        format!("audience-{viewer}"),
        web::Data::new(SseState::new()),
        web::Data::new(f.pool.clone()),
        sync_viewer,
        Some(f.ws.workspace_id),
        Arc::new(allowed.into_iter().collect::<HashSet<String>>()),
        guard,
    );
    let frame = stream.next().await.expect("a frame").expect("frame bytes");
    let frame = String::from_utf8(frame.to_vec()).expect("utf8 frame");
    let data = frame
        .lines()
        .find_map(|l| l.strip_prefix("data: "))
        .expect("data line");
    let event: Value = serde_json::from_str(data).expect("frame json");
    let actions = event
        .get("actions")
        .or_else(|| event.get("data").and_then(|d| d.get("actions")))
        .unwrap_or_else(|| panic!("no actions in frame: {data}"));
    actions.as_array().expect("actions").clone()
}

/// Rows grouped by record kind (`aggregate`, or bootstrap's `__model__`).
fn by_kind(rows: &[Value]) -> HashMap<String, Vec<Value>> {
    let mut out: HashMap<String, Vec<Value>> = HashMap::new();
    for row in rows {
        let kind = row
            .get("aggregate")
            .or_else(|| row.get("__model__"))
            .and_then(Value::as_str)
            .expect("record kind")
            .to_string();
        out.entry(kind).or_default().push(row.clone());
    }
    out
}

/// The latest `user` row about `who` (a delta/stream row's `data`, or a
/// bootstrap row).
fn user_row(rows: &HashMap<String, Vec<Value>>, who: Uuid) -> Value {
    rows.get("user")
        .into_iter()
        .flatten()
        .map(|r| r.get("data").unwrap_or(r))
        .rfind(|d| d["uuid"] == json!(who))
        .cloned()
        .unwrap_or_else(|| panic!("no user row about {who}"))
}

fn has(row: &Value, field: &str) -> bool {
    row.get(field).is_some_and(|v| !v.is_null())
}

/// What a role receives of the feed, checked the same way for the delta and
/// the live stream.
fn check_feed(f: &Fixture, who: &str, viewer: Uuid, rows: &[Value]) {
    let kinds = by_kind(rows);
    let count = |kind: &str| kinds.get(kind).map_or(0, Vec::len);
    let is_admin = viewer == f.admin;
    let is_staff = viewer != f.member;

    assert_eq!(count("webhook") > 0, is_admin, "{who}: webhook rows");
    assert_eq!(count("channel") > 0, is_admin, "{who}: channel rows");
    assert_eq!(count("data") > 0, is_admin, "{who}: audit read rows");
    assert_eq!(
        count("knowledge_gap") > 0,
        is_staff,
        "{who}: knowledge gap rows"
    );
    assert_eq!(count("group_membership") > 0, is_staff, "{who}: group rows");
    assert!(count("plugin") > 0, "{who}: plugin rows reach everyone");
    assert_eq!(
        count("record_kind_from_the_future"),
        0,
        "{who}: a record kind this server doesn't know is withheld"
    );

    // A gap being written in a page only the admin can open reaches only the
    // admin, signals included.
    let gaps: HashSet<String> = kinds
        .get("knowledge_gap")
        .into_iter()
        .flatten()
        .filter_map(|r| r["aggregate_id"].as_str().map(str::to_string))
        .collect();
    assert_eq!(
        gaps.contains(&f.gap_in_admins_page.to_string()),
        is_admin,
        "{who}: the gap drafting in the admin's page"
    );

    // Webhook header values reach no one, admins included.
    for row in kinds.get("webhook").into_iter().flatten() {
        assert!(
            !row.to_string().contains(HEADER_SECRET),
            "{who}: a webhook row carries a header value: {row}"
        );
    }

    // Loans: staff see every loan, a member only their own.
    let borrowers: HashSet<String> = kinds
        .get("asset_loan")
        .into_iter()
        .flatten()
        .filter_map(|r| r["data"]["borrower_user_uuid"].as_str().map(str::to_string))
        .collect();
    let expected: HashSet<String> = if is_staff {
        [f.member, f.agent].iter().map(Uuid::to_string).collect()
    } else {
        [f.member.to_string()].into_iter().collect()
    };
    assert_eq!(borrowers, expected, "{who}: loan borrowers");

    // The member's own notification and draft upload reach the member only.
    assert_eq!(
        count("notification") > 0,
        viewer == f.member,
        "{who}: notification"
    );
    let draft = kinds
        .get("attachment")
        .into_iter()
        .flatten()
        .any(|r| r["data"]["name"] == json!("draft.png"));
    assert_eq!(
        draft,
        viewer == f.member,
        "{who}: the member's draft upload"
    );

    check_people(f, who, viewer, &kinds);
}

/// The `user` rows: a viewer's own row is whole; of others, everyone gets the
/// name and avatar, staff also the email and workspace role, and no one their
/// platform role or dashboard layout.
fn check_people(f: &Fixture, who: &str, viewer: Uuid, kinds: &HashMap<String, Vec<Value>>) {
    let is_staff = viewer != f.member;
    let own = user_row(kinds, viewer);
    for field in ["name", "email", "workspace_role", "dashboard_layout"] {
        assert!(has(&own, field), "{who}: own row lacks {field}: {own}");
    }
    for other in [f.admin, f.agent, f.member]
        .into_iter()
        .filter(|u| *u != viewer)
    {
        let row = user_row(kinds, other);
        assert!(
            has(&row, "name"),
            "{who}: another's row lacks a name: {row}"
        );
        assert_eq!(
            has(&row, "email"),
            is_staff,
            "{who}: another's email: {row}"
        );
        assert_eq!(
            has(&row, "workspace_role"),
            is_staff,
            "{who}: another's workspace role: {row}"
        );
        assert!(
            !has(&row, "platform_role"),
            "{who}: another's platform role: {row}"
        );
        assert!(
            !has(&row, "dashboard_layout"),
            "{who}: another's dashboard layout: {row}"
        );
    }
}

#[actix_web::test]
async fn each_role_receives_only_the_records_meant_for_it() {
    let db = common::TestDb::new();
    let f = seed(&db);
    let actions = every_action(&f);
    let newest = actions
        .iter()
        .filter_map(|a| a["sync_id"].as_i64())
        .max()
        .expect("actions");
    for (who, viewer) in [("member", f.member), ("agent", f.agent), ("admin", f.admin)] {
        let delta = delta_rows(&f, viewer, newest).await;
        check_feed(&f, &format!("{who} delta"), viewer, &delta);
        let live = stream_rows(&f, viewer, actions.clone()).await;
        check_feed(&f, &format!("{who} stream"), viewer, &live);
    }
}

#[actix_web::test]
async fn the_bootstrap_snapshot_follows_the_same_audiences() {
    let db = common::TestDb::new();
    let f = seed(&db);
    for (who, viewer) in [("member", f.member), ("agent", f.agent), ("admin", f.admin)] {
        let rows = bootstrap_rows(&f, viewer).await;
        let kinds = by_kind(&rows);
        let who = format!("{who} bootstrap");
        check_people(&f, &who, viewer, &kinds);
        // A project's ticket links follow the tickets.
        let linked: HashSet<i64> = kinds
            .get("project_ticket")
            .into_iter()
            .flatten()
            .filter_map(|r| r["ticket_id"].as_i64())
            .collect();
        assert!(
            linked.contains(&i64::from(f.member_ticket)),
            "{who}: a visible ticket's project link: {linked:?}"
        );
        assert_eq!(
            linked.contains(&i64::from(f.hidden_ticket)),
            viewer != f.member,
            "{who}: a hidden ticket's project link: {linked:?}"
        );
    }
}

/// `GET /api/users/paginated` as `viewer`, with `query` appended.
async fn people(f: &Fixture, viewer: Uuid, query: &str) -> Vec<Value> {
    let uri = format!("/api/users/paginated?page=1&pageSize=50{query}");
    let body: Value = serde_json::from_str(&get_as(f, viewer, &uri).await).expect("people json");
    body["data"].as_array().expect("data").clone()
}

fn uuids_of(rows: &[Value]) -> HashSet<Uuid> {
    rows.iter()
        .filter_map(|r| r["uuid"].as_str())
        .filter_map(|u| Uuid::parse_str(u).ok())
        .collect()
}

#[actix_web::test]
async fn the_people_routes_give_a_member_names_and_avatars() {
    let db = common::TestDb::new();
    let f = seed(&db);
    for (who, viewer) in [("member", f.member), ("agent", f.agent), ("admin", f.admin)] {
        let who = format!("{who} people list");
        let rows = people(&f, viewer, "").await;
        let kinds: HashMap<String, Vec<Value>> = [("user".to_string(), rows)].into();
        let is_staff = viewer != f.member;

        let own = user_row(&kinds, viewer);
        for field in [
            "email",
            "workspace_role",
            "platform_role",
            "dashboard_layout",
        ] {
            assert!(has(&own, field), "{who}: own row lacks {field}: {own}");
        }
        // Staff keep the full rows; a member gets the name and avatar.
        for other in [f.admin, f.agent, f.member]
            .into_iter()
            .filter(|u| *u != viewer)
        {
            let row = user_row(&kinds, other);
            assert!(has(&row, "name"), "{who}: another's name: {row}");
            for field in [
                "email",
                "workspace_role",
                "platform_role",
                "open_ticket_count",
                "created_at",
            ] {
                assert_eq!(
                    has(&row, field),
                    is_staff,
                    "{who}: another's {field}: {row}"
                );
            }
            if !is_staff {
                for field in [
                    "dashboard_layout",
                    "theme",
                    "signature",
                    "locale",
                    "timezone",
                ] {
                    assert!(!has(&row, field), "{who}: another's {field}: {row}");
                }
            }
        }

        // One person by uuid: the same fields.
        let other = if viewer == f.admin { f.member } else { f.admin };
        let row: Value =
            serde_json::from_str(&get_as(&f, viewer, &format!("/api/users/{other}")).await)
                .expect("person json");
        assert_eq!(
            has(&row, "email"),
            is_staff,
            "{who}: one person's email: {row}"
        );
        assert_eq!(
            has(&row, "platform_role"),
            is_staff,
            "{who}: one person's platform role: {row}"
        );
        assert!(
            has(&row, "editable"),
            "{who}: the caller's rights stay: {row}"
        );

        // An asset names its user the same way, alone or in a page.
        let asset: Value = serde_json::from_str(
            &get_as(&f, viewer, &format!("/api/assets/{}", f.admins_asset)).await,
        )
        .expect("asset json");
        let page: Value = serde_json::from_str(
            &get_as(
                &f,
                viewer,
                "/api/assets/paginated/excluding?pageSize=100&excludeIds=0",
            )
            .await,
        )
        .expect("asset page json");
        let in_page = page["data"]
            .as_array()
            .expect("data")
            .iter()
            .find(|a| a["id"] == json!(f.admins_asset))
            .cloned()
            .expect("the admin's asset in the page");
        for asset in [asset, in_page] {
            let user = &asset["primary_user"];
            assert_eq!(user["uuid"], json!(f.admin), "{who}: {asset}");
            if viewer != f.admin {
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
}

#[actix_web::test]
async fn a_member_finds_people_by_name_only() {
    let db = common::TestDb::new();
    let f = seed(&db);
    // The admin's address is `<uuid>@audience.test`.
    let local_part = format!("&search={}", f.admin.simple());
    assert!(
        uuids_of(&people(&f, f.agent, &local_part).await).contains(&f.admin),
        "staff find someone by address"
    );
    assert!(
        people(&f, f.member, &local_part).await.is_empty(),
        "a member searching someone's address finds no one"
    );
    assert!(
        uuids_of(&people(&f, f.member, "&search=Audience%20Agent").await).contains(&f.agent),
        "a member finds someone by name"
    );
    // Role and population filters would sort people by their role.
    for query in ["&role=admin", "&population=requesters", "&population=team"] {
        let seen = uuids_of(&people(&f, f.member, query).await);
        assert!(
            seen.contains(&f.admin) && seen.contains(&f.agent) && seen.contains(&f.member),
            "a member's {query} is ignored: {seen:?}"
        );
    }
    let filtered = uuids_of(&people(&f, f.agent, "&role=admin").await);
    assert!(
        filtered.contains(&f.admin) && !filtered.contains(&f.member),
        "staff filter by role"
    );
}
