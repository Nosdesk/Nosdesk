//! A rule's conditions are checked when it's saved, and the save says how
//! many of the workspace's recent tickets they match. Driven through the
//! rules routes as `nosdesk_app`, so the preview reads only the request's
//! workspace.

use actix_web::dev::{Service, ServiceResponse};
use actix_web::http::StatusCode;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::Claims;
use backend::sync::actor::ActorContext;

use crate::common::{self, TestPool, WorkspaceSeed};

/// Call the rules routes as `ws`'s admin.
async fn as_admin(
    pool: &TestPool,
    ws: &WorkspaceSeed,
    req: http_test::TestRequest,
) -> ServiceResponse {
    let now = chrono::Utc::now().timestamp();
    let claims = Claims {
        sub: ws.admin_uuid.to_string(),
        name: "Admin".to_string(),
        email: "admin@example.com".to_string(),
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
    let actor = ActorContext::user(ws.admin_uuid, Some(corr)).with_workspace(ws.workspace_id);
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
            .service(web::scope("/api").configure(backend::handlers::rules::config)),
    )
    .await;
    http_test::call_service(&app, req.to_request()).await
}

async fn json_of(resp: ServiceResponse) -> (StatusCode, Value) {
    let status = resp.status();
    (status, http_test::read_body_json(resp).await)
}

/// Workspace A's tickets, newest last. The merged one and the untouched
/// blank one are never previewed.
struct Seeded {
    printer_jam: i32,
    printer_offline: i32,
    vpn_down: i32,
    category_id: i32,
    vip_tag: i32,
}

fn seed_tickets(
    conn: &mut backend::db::DbConnection,
    ws: &WorkspaceSeed,
    requester: Uuid,
) -> Seeded {
    use backend::models::{NewTicket, TicketPriority, WorkflowStateCategory};
    use backend::schema::{
        tags, ticket_categories, ticket_tags, tickets, user_profiles, workflow_states,
    };
    use diesel::prelude::*;

    let actor = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    backend::sync::session::with_actor_context(conn, &actor, |c| {
        let open: i32 = workflow_states::table
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        let merged: i32 = workflow_states::table
            .filter(workflow_states::category.eq(WorkflowStateCategory::Merged))
            .select(workflow_states::id)
            .first(c)?;
        let category_id: i32 = ticket_categories::table
            .select(ticket_categories::id)
            .order(ticket_categories::id.asc())
            .first(c)?;
        let vip_tag: i32 = diesel::insert_into(tags::table)
            .values(tags::name.eq("vip"))
            .returning(tags::id)
            .get_result(c)?;
        diesel::insert_into(user_profiles::table)
            .values((
                user_profiles::user_uuid.eq(requester),
                user_profiles::organization.eq("Acme Corp"),
            ))
            .execute(c)?;
        let mut ticket = |title: &str,
                          priority: TicketPriority,
                          requester: Option<Uuid>,
                          state: i32,
                          category: Option<i32>| {
            diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state,
                    priority,
                    requester_uuid: requester,
                    category_id: category,
                    ..Default::default()
                })
                .returning(tickets::id)
                .get_result::<i32>(c)
        };
        let printer_jam = ticket(
            "Printer jammed",
            TicketPriority::High,
            Some(requester),
            open,
            None,
        )?;
        let printer_offline = ticket(
            "Printer offline",
            TicketPriority::Low,
            Some(requester),
            open,
            None,
        )?;
        let vpn_down = ticket(
            "VPN down",
            TicketPriority::High,
            None,
            open,
            Some(category_id),
        )?;
        ticket(
            "Printer jammed again",
            TicketPriority::High,
            Some(requester),
            merged,
            None,
        )?;
        let blank = ticket(
            "Untitled",
            TicketPriority::High,
            Some(requester),
            open,
            None,
        )?;
        diesel::update(tickets::table.find(blank))
            .set(tickets::blank_at.eq(tickets::updated_at.nullable()))
            .execute(c)?;
        diesel::insert_into(ticket_tags::table)
            .values((
                ticket_tags::ticket_id.eq(printer_jam),
                ticket_tags::tag_id.eq(vip_tag),
            ))
            .execute(c)?;
        Ok::<_, diesel::result::Error>(Seeded {
            printer_jam,
            printer_offline,
            vpn_down,
            category_id,
            vip_tag,
        })
    })
    .expect("seed tickets")
}

/// A requester with a primary email, in no workspace's profile yet.
fn requester(conn: &mut backend::db::DbConnection) -> Uuid {
    use backend::models::NewUserEmail;
    use backend::schema::user_emails;
    use diesel::prelude::*;

    let uuid = common::insert_plain_user(conn, "Pat Requester");
    diesel::insert_into(user_emails::table)
        .values(&NewUserEmail {
            user_uuid: uuid,
            email: "Pat@Acme.example".to_string(),
            email_type: "personal".to_string(),
            is_primary: true,
            is_verified: true,
            source: None,
        })
        .execute(conn)
        .expect("requester email");
    uuid
}

/// A high-priority printer ticket in workspace B, which A's previews must
/// never count.
fn seed_other_workspace(conn: &mut backend::db::DbConnection, ws: &WorkspaceSeed) {
    use backend::models::{NewTicket, TicketPriority};
    use backend::schema::{tickets, workflow_states};
    use diesel::prelude::*;

    let actor = ActorContext::user(ws.admin_uuid, None).with_workspace(ws.workspace_id);
    backend::sync::session::with_actor_context(conn, &actor, |c| {
        let open: i32 = workflow_states::table
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        diesel::insert_into(tickets::table)
            .values(&NewTicket {
                title: "Printer jammed in B".to_string(),
                workflow_state_id: open,
                priority: TicketPriority::High,
                ..Default::default()
            })
            .execute(c)
    })
    .expect("seed B");
}

fn leaf(field: &str, op: &str, value: Value) -> Value {
    json!({ "kind": "leaf", "field": field, "op": op, "value": value })
}

fn event_rule(ws: &WorkspaceSeed, conditions: Value) -> Value {
    json!({
        "name": "Route printer tickets",
        "trigger_kind": "ticket_created",
        "conditions": conditions,
        "actions": [{
            "kind": "assign",
            "config": { "method": "direct", "user_uuid": ws.admin_uuid }
        }],
    })
}

fn ids(preview: &Value) -> Vec<i64> {
    preview["sample_ticket_ids"]
        .as_array()
        .expect("samples")
        .iter()
        .filter_map(Value::as_i64)
        .collect()
}

#[actix_web::test]
async fn saving_a_rule_previews_it_against_recent_tickets() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.pool_with_size(2).get().expect("conn");
    let seeded = common::seed_two_workspaces(&mut conn);
    let (a, b) = (&seeded.a, &seeded.b);
    let pat = requester(&mut conn);
    let t = seed_tickets(&mut conn, a, pat);
    seed_other_workspace(&mut conn, b);
    drop(conn);
    let pool = db.runtime_pool(4);

    // High priority printer tickets: the jam only. The merged duplicate,
    // the blank ticket and B's ticket are not scanned.
    let printers = json!({ "kind": "and", "children": [
        leaf("ticket.priority", "eq", json!("high")),
        leaf("ticket.title", "contains", json!("PRINTER")),
    ]});
    let (status, rule) = json_of(
        as_admin(
            &pool,
            a,
            http_test::TestRequest::post()
                .uri("/api/rules")
                .set_json(event_rule(a, printers.clone())),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{rule}");
    assert_eq!(rule["conditions"], printers, "stored as sent");
    assert_eq!(rule["state"], "draft");
    assert_eq!(rule["preview"]["scanned"], 3);
    assert_eq!(rule["preview"]["matched"], 1);
    assert_eq!(ids(&rule["preview"]), vec![t.printer_jam as i64]);
    let id = rule["id"].as_i64().expect("rule id");

    // An update previews the conditions it saves, newest ticket first.
    for (conditions, expected) in [
        (
            leaf(
                "ticket.requester.email",
                "ends_with",
                json!("@acme.EXAMPLE"),
            ),
            vec![t.printer_offline, t.printer_jam],
        ),
        (
            leaf("ticket.requester.organization", "eq", json!("acme corp")),
            vec![t.printer_offline, t.printer_jam],
        ),
        (
            leaf("ticket.tag_ids", "has_any", json!([t.vip_tag])),
            vec![t.printer_jam],
        ),
        (
            leaf("ticket.category_id", "eq", json!(t.category_id)),
            vec![t.vpn_down],
        ),
        (
            json!({ "kind": "leaf", "field": "ticket.requester_uuid", "op": "is_null" }),
            vec![t.vpn_down],
        ),
    ] {
        let (status, saved) = json_of(
            as_admin(
                &pool,
                a,
                http_test::TestRequest::put()
                    .uri(&format!("/api/rules/{id}"))
                    .set_json(json!({ "conditions": conditions })),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{conditions}: {saved}");
        assert_eq!(
            saved["preview"]["matched"],
            expected.len() as i64,
            "{conditions}"
        );
        let expected: Vec<i64> = expected.into_iter().map(i64::from).collect();
        assert_eq!(ids(&saved["preview"]), expected, "{conditions}");
    }

    // A rename alone still reports the preview of the stored conditions.
    let (status, renamed) = json_of(
        as_admin(
            &pool,
            a,
            http_test::TestRequest::put()
                .uri(&format!("/api/rules/{id}"))
                .set_json(json!({ "name": "Route VPN tickets" })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(renamed["preview"]["matched"], 1);

    // A manual rule has no conditions, so every recent ticket matches.
    let (status, manual) = json_of(
        as_admin(
            &pool,
            a,
            http_test::TestRequest::post()
                .uri("/api/rules")
                .set_json(json!({
                    "name": "Escalate",
                    "trigger_kind": "manual",
                    "actions": [{ "kind": "set_priority", "config": { "priority": "urgent" } }],
                })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(manual["preview"]["matched"], 3);
    assert_eq!(manual["preview"]["scanned"], 3);

    // B's admin sees only B's ticket.
    let (status, elsewhere) = json_of(
        as_admin(
            &pool,
            b,
            http_test::TestRequest::post()
                .uri("/api/rules/preview")
                .set_json(json!({
                    "trigger_kind": "ticket_created",
                    "conditions": leaf("ticket.title", "contains", json!("printer")),
                })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{elsewhere}");
    assert_eq!(elsewhere["scanned"], 1);
    assert_eq!(elsewhere["matched"], 1);
}

#[actix_web::test]
async fn the_preview_endpoint_counts_matches_without_saving() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.pool_with_size(2).get().expect("conn");
    let seeded = common::seed_two_workspaces(&mut conn);
    let a = &seeded.a;
    let pat = requester(&mut conn);
    let t = seed_tickets(&mut conn, a, pat);
    drop(conn);
    let pool = db.runtime_pool(4);

    let preview = |conditions: Value, trigger: &str| {
        http_test::TestRequest::post()
            .uri("/api/rules/preview")
            .set_json(json!({ "trigger_kind": trigger, "conditions": conditions }))
    };

    let (status, body) = json_of(
        as_admin(
            &pool,
            a,
            preview(
                json!({ "kind": "not", "child": leaf("ticket.priority", "eq", json!("high")) }),
                "ticket_created",
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        body,
        json!({ "matched": 1, "scanned": 3, "sample_ticket_ids": [t.printer_offline] })
    );

    // Past tickets have no event, so an event leaf counts as met and the
    // rest of the tree decides.
    let (status, body) = json_of(
        as_admin(
            &pool,
            a,
            preview(
                json!({ "kind": "and", "children": [
                    leaf("event.changed_fields", "has_any", json!(["ticket.priority"])),
                    leaf("ticket.priority", "eq", json!("high")),
                ]}),
                "ticket_updated",
            ),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["matched"], 2);
    assert_eq!(ids(&body), vec![t.vpn_down as i64, t.printer_jam as i64]);

    // Nothing was saved.
    let (_, listed) =
        json_of(as_admin(&pool, a, http_test::TestRequest::get().uri("/api/rules")).await).await;
    assert_eq!(listed, json!([]));
}

#[actix_web::test]
async fn conditions_that_cannot_run_are_refused_at_save() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.pool_with_size(2).get().expect("conn");
    let seeded = common::seed_two_workspaces(&mut conn);
    let (a, b) = (&seeded.a, &seeded.b);
    let pat = requester(&mut conn);
    seed_tickets(&mut conn, a, pat);
    let b_category: i32 = {
        use backend::schema::ticket_categories;
        use diesel::prelude::*;
        let actor = ActorContext::user(b.admin_uuid, None).with_workspace(b.workspace_id);
        backend::sync::session::with_actor_context(&mut conn, &actor, |c| {
            ticket_categories::table
                .select(ticket_categories::id)
                .first::<i32>(c)
        })
        .expect("B category")
    };
    drop(conn);
    let pool = db.runtime_pool(4);

    let create = |body: Value| {
        http_test::TestRequest::post()
            .uri("/api/rules")
            .set_json(body)
    };

    // An operator the field's kind doesn't take names the field, the
    // operator and the kind.
    let (status, body) = json_of(
        as_admin(
            &pool,
            a,
            create(event_rule(
                a,
                json!({ "kind": "or", "children": [
                    leaf("ticket.priority", "eq", json!("high")),
                    leaf("ticket.title", "gt", json!("a")),
                ]}),
            )),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "RULE_CONDITION_INVALID");
    assert_eq!(
        body["details"],
        json!({
            "path": "conditions.children[1]",
            "field": "ticket.title",
            "op": "gt",
            "field_kind": "string",
        })
    );

    for (conditions, trigger, message) in [
        (
            leaf("ticket.category_id", "eq", json!(b_category)),
            "ticket_created",
            "ticket.category_id",
        ),
        (
            leaf("ticket.tag_ids", "has_any", json!([987_654])),
            "ticket_created",
            "ticket.tag_ids",
        ),
        (
            leaf("reply.kind", "eq", json!("customer")),
            "ticket_created",
            "ticket_replied",
        ),
        (
            leaf("ticket.priority", "in", json!([])),
            "ticket_created",
            "at least one",
        ),
        (
            leaf("ticket.title", "matches_regex", json!(".*printer")),
            "ticket_created",
            ".*",
        ),
        (
            json!([leaf("ticket.priority", "eq", json!("high"))]),
            "ticket_created",
            "not a list",
        ),
    ] {
        let mut rule = event_rule(a, conditions.clone());
        rule["trigger_kind"] = json!(trigger);
        let (status, body) = json_of(as_admin(&pool, a, create(rule)).await).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{conditions}: {body}");
        assert_eq!(body["code"], "RULE_CONDITION_INVALID", "{conditions}");
        let error = body["error"].as_str().unwrap_or_default();
        assert!(error.contains(message), "{conditions}: {error}");

        // The preview endpoint refuses what a save refuses.
        let (status, _) = json_of(
            as_admin(
                &pool,
                a,
                http_test::TestRequest::post()
                    .uri("/api/rules/preview")
                    .set_json(json!({ "trigger_kind": trigger, "conditions": conditions })),
            )
            .await,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "preview {conditions}");
    }

    // A manual rule still takes no conditions at all.
    let mut manual = event_rule(a, leaf("ticket.priority", "eq", json!("high")));
    manual["trigger_kind"] = json!("manual");
    let (status, body) = json_of(as_admin(&pool, a, create(manual)).await).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "RULE_VALIDATION");

    // B's own category is fine in B.
    let (status, _) = json_of(
        as_admin(
            &pool,
            b,
            create(event_rule(
                b,
                leaf("ticket.category_id", "eq", json!(b_category)),
            )),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    // An update with bad conditions leaves the rule as it was.
    let good = leaf("ticket.priority", "eq", json!("high"));
    let (status, rule) =
        json_of(as_admin(&pool, a, create(event_rule(a, good.clone()))).await).await;
    assert_eq!(status, StatusCode::CREATED);
    let id = rule["id"].as_i64().expect("rule id");
    let (status, body) = json_of(
        as_admin(
            &pool,
            a,
            http_test::TestRequest::put()
                .uri(&format!("/api/rules/{id}"))
                .set_json(json!({
                    "conditions": leaf("ticket.category_id", "eq", json!(b_category)),
                })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["details"]["id"], b_category);
    let (_, stored) = json_of(
        as_admin(
            &pool,
            a,
            http_test::TestRequest::get().uri(&format!("/api/rules/{id}")),
        )
        .await,
    )
    .await;
    assert_eq!(stored["conditions"], good);

    // Switching the trigger re-checks the stored conditions against it.
    let (status, body) = json_of(
        as_admin(
            &pool,
            a,
            http_test::TestRequest::put()
                .uri(&format!("/api/rules/{id}"))
                .set_json(json!({
                    "trigger_kind": "ticket_replied",
                    "conditions": leaf("event.changed_fields", "has_any", json!(["ticket.title"])),
                })),
        )
        .await,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["details"]["field"], "event.changed_fields");
}
