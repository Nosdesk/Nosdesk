//! Knowledge gaps don't show documentation pages the viewer can't open.
//!
//! One page is restricted to an "insider" agent by a direct grant. To an
//! "outsider" agent it reads as absent on the gap routes: a stale-doc gap
//! about it is left out of the queue and the dashboard count, and can't be
//! opened, dismissed or written up; a ticket it resolves can still be flagged
//! (no page says so); a gap being drafted in it is answered without the draft;
//! and a gap can't be resolved with it. The insider and a workspace admin see
//! everything.

use std::collections::HashMap;
use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use tokio::sync::RwLock as TokioRwLock;
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{
    Claims, DocumentationStatus, NewDocumentationPage, NewTicket, WorkflowStateCategory,
};
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::repository::{documentation_page_tickets, knowledge_gaps};
use backend::services::notifications::NotificationService;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::run_in_workspace;

use crate::common::{self, TestPool};

const REF: &str = "test:knowledge_gap_hidden_pages";
/// The restricted page's title and slug: neither may reach the outsider.
const HIDDEN_TITLE: &str = "Payroll Export Runbook";
const HIDDEN_SLUG: &str = "payroll-export-runbook";

struct Fixture {
    workspace: WorkspaceContext,
    admin: Uuid,
    /// A requester-role member: the gap queue is staff data.
    member: Uuid,
    insider: Uuid,
    outsider: Uuid,
    hidden: i32,
    /// The stale-doc gap detection raised about `hidden`.
    stale_gap: i64,
    /// An open ticket `hidden` resolves, not yet flagged.
    resolved_by_hidden: i32,
    /// A gap flagged on another ticket, still open.
    open_gap: i64,
    /// A flagged ticket whose gap is being drafted in `hidden`.
    drafting_ticket: i32,
    drafting_gap: i64,
}

fn setup(pool: &TestPool) -> Fixture {
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let ws = seeded.a.workspace_id;
    let admin = seeded.a.admin_uuid;
    let (insider, outsider) = {
        let mut conn = pool.get().expect("conn");
        (
            common::insert_plain_user(&mut conn, "Insider Agent"),
            common::insert_plain_user(&mut conn, "Outsider Agent"),
        )
    };

    let seeded_docs = run_in_workspace(pool, REF, ws, |c| {
        use backend::schema::{documentation_pages, tickets};
        for agent in [insider, outsider] {
            add_membership(c, ws, agent, "agent", SeatWriteAuthority::ControlPlane)?;
        }
        let hidden: i32 = diesel::insert_into(documentation_pages::table)
            .values(&NewDocumentationPage {
                uuid: Uuid::new_v4(),
                title: HIDDEN_TITLE.to_string(),
                slug: HIDDEN_SLUG.to_string(),
                icon: None,
                cover_image: None,
                status: DocumentationStatus::Published,
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
            .returning(documentation_pages::id)
            .get_result(c)?;
        // Verified long ago on a 30-day cycle: stale.
        diesel::update(documentation_pages::table.find(hidden))
            .set((
                documentation_pages::verified_at.eq(Some(
                    chrono::Utc::now().naive_utc() - chrono::Duration::days(100),
                )),
                documentation_pages::verify_interval_days.eq(Some(30)),
            ))
            .execute(c)?;
        backend::repository::set_page_visibility(c, hidden, vec![], vec![insider], None)?;

        let ticket = |c: &mut backend::db::DbConnection, title: &str, state: i32| {
            diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: title.to_string(),
                    workflow_state_id: state,
                    ..Default::default()
                })
                .returning(tickets::id)
                .get_result::<i32>(c)
        };
        let done = backend::repository::workflow_states::first_in_category(
            c,
            WorkflowStateCategory::Done,
        )?
        .id;
        let open = backend::repository::workflow_states::default_state(c)?.id;
        // A ticket the page resolved that closed recently makes it a stale doc.
        let closed = ticket(c, "Payroll export failed", done)?;
        documentation_page_tickets::upsert_link(c, hidden, closed, "resolves", None)?;
        let resolved_by_hidden = ticket(c, "Payroll export failed again", open)?;
        documentation_page_tickets::upsert_link(c, hidden, resolved_by_hidden, "resolves", None)?;

        let stats = knowledge_gaps::run_stale_doc_detection(c, Some(admin), 30, 1)?;
        assert_eq!(
            stats.new_gap_ids.len(),
            1,
            "detection raises one stale-doc gap"
        );
        let stale_gap = stats.new_gap_ids[0];

        let other = ticket(c, "Printer jammed", open)?;
        let (open_gap, _, _) =
            knowledge_gaps::flag_ticket(c, other, "Printer jammed", admin, None)?;

        let drafting_ticket = ticket(c, "Payroll file rejected by the bank", open)?;
        let (drafting_gap, _, _) = knowledge_gaps::flag_ticket(
            c,
            drafting_ticket,
            "Payroll file rejected by the bank",
            admin,
            None,
        )?;
        knowledge_gaps::start_drafting(c, drafting_gap.id, hidden)?;
        Ok((
            hidden,
            stale_gap,
            resolved_by_hidden,
            open_gap.id,
            drafting_ticket,
            drafting_gap.id,
        ))
    })
    .expect("seed docs and gaps");
    let (hidden, stale_gap, resolved_by_hidden, open_gap, drafting_ticket, drafting_gap) =
        seeded_docs;

    Fixture {
        workspace: WorkspaceContext {
            workspace_id: ws,
            workspace_uuid: seeded.a.workspace_uuid,
            slug: seeded.a.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        },
        admin,
        member: seeded.a.member_uuid,
        insider,
        outsider,
        hidden,
        stale_gap,
        resolved_by_hidden,
        open_gap,
        drafting_ticket,
        drafting_gap,
    }
}

fn claims(user: Uuid) -> Claims {
    Claims {
        sub: user.to_string(),
        name: "Gap Caller".to_string(),
        email: "caller@example.com".to_string(),
        platform_role: "user".to_string(),
        scope: "full".to_string(),
        sid: None,
        workspace_uuid: None,
        exp: (chrono::Utc::now().timestamp() + 3600) as usize,
        iat: chrono::Utc::now().timestamp() as usize,
    }
}

/// The caller rides in a test-only header so one app serves every caller.
const AS: &str = "x-test-as";

fn request(method: Method, uri: &str, user: Uuid, body: Option<Value>) -> http_test::TestRequest {
    let req = http_test::TestRequest::default()
        .method(method)
        .uri(uri)
        .insert_header((AS, user.to_string()));
    match body {
        Some(body) => req.set_json(body),
        None => req,
    }
}

fn mentions_hidden_page(body: &str) -> bool {
    body.contains(HIDDEN_TITLE) || body.contains(HIDDEN_SLUG)
}

fn gap_ids(body: &str) -> Vec<i64> {
    serde_json::from_str::<Vec<Value>>(body)
        .expect("a gap list")
        .iter()
        .filter_map(|g| g["id"].as_i64())
        .collect()
}

#[actix_web::test]
async fn a_hidden_page_reads_as_absent_on_the_gap_routes() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(6);
    let f = setup(&pool);

    let search_dir = tempfile::tempdir().expect("tempdir");
    let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
    let notifications =
        NotificationService::new(pool.clone(), Arc::new(TokioRwLock::new(HashMap::new())));
    let workspace = f.workspace.clone();
    let corr = Uuid::now_v7();
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            .app_data(web::Data::new(notifications))
            .wrap_fn(move |req, srv| {
                let user = req
                    .headers()
                    .get(AS)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| Uuid::parse_str(v).ok())
                    .expect("caller header");
                let actor =
                    ActorContext::user(user, Some(corr)).with_workspace(workspace.workspace_id);
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(claims(user));
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor));
                srv.call(req)
            })
            .service(
                web::scope("/api")
                    .configure(backend::handlers::documentation::config)
                    .route(
                        "/dashboard/stats",
                        web::get().to(backend::handlers::dashboard::get_stats),
                    ),
            ),
    )
    .await;
    let call = |method: Method, uri: String, user: Uuid, body: Option<Value>| {
        let req = request(method, &uri, user, body).to_request();
        let app = &app;
        async move {
            let resp = http_test::call_service(app, req).await;
            let status = resp.status();
            let body = String::from_utf8(http_test::read_body(resp).await.to_vec()).expect("utf8");
            (status, body)
        }
    };

    // Those who can open the page see its gap.
    for (who, user) in [("insider", f.insider), ("admin", f.admin)] {
        let (status, body) = call(Method::GET, "/api/knowledge-gaps".into(), user, None).await;
        assert_eq!(status, StatusCode::OK, "{who}: list");
        assert!(
            gap_ids(&body).contains(&f.stale_gap),
            "{who}: the stale-doc gap is listed"
        );
        let (status, _) = call(
            Method::GET,
            format!("/api/knowledge-gaps/{}", f.stale_gap),
            user,
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{who}: the stale-doc gap opens");
    }
    // The insider flagging a ticket the page resolves is told it is documented.
    let (status, body) = call(
        Method::POST,
        format!("/api/tickets/{}/flag-as-gap", f.resolved_by_hidden),
        f.insider,
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "insider: already documented");
    assert!(body.contains("TICKET_ALREADY_DOCUMENTED"), "{body}");
    // Flagging a ticket whose gap is drafting answers with the gap and its draft.
    let flag_drafting = format!("/api/tickets/{}/flag-as-gap", f.drafting_ticket);
    let (status, body) = call(
        Method::POST,
        flag_drafting.clone(),
        f.insider,
        Some(json!({})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "insider: flag a drafting gap: {body}"
    );
    let gap: Value = serde_json::from_str(&body).expect("gap json");
    assert_eq!(gap["id"], json!(f.drafting_gap), "{body}");
    assert_eq!(gap["draft_page_id"], json!(f.hidden), "insider: {body}");

    // The outsider: the page isn't there.
    let (status, body) = call(Method::GET, "/api/knowledge-gaps".into(), f.outsider, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !gap_ids(&body).contains(&f.stale_gap),
        "outsider: a gap about a page they can't open is left out of the queue"
    );
    assert!(
        gap_ids(&body).contains(&f.open_gap),
        "outsider: other gaps stay"
    );
    assert!(
        !mentions_hidden_page(&body),
        "outsider: list names the hidden page: {body}"
    );

    let (status, body) = call(
        Method::GET,
        format!("/api/knowledge-gaps/{}", f.stale_gap),
        f.outsider,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "outsider: the stale-doc gap: {body}"
    );
    assert!(!mentions_hidden_page(&body));

    let (status, body) = call(
        Method::POST,
        format!("/api/tickets/{}/flag-as-gap", f.resolved_by_hidden),
        f.outsider,
        Some(json!({})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "outsider: no page they can open documents the ticket, so it flags: {body}"
    );
    assert!(
        !mentions_hidden_page(&body),
        "outsider: flag names the hidden page: {body}"
    );

    let (status, body) = call(
        Method::POST,
        format!("/api/knowledge-gaps/{}/dismiss", f.stale_gap),
        f.outsider,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "outsider: dismissing the stale-doc gap: {body}"
    );

    let (status, body) = call(
        Method::POST,
        "/api/documentation/pages".into(),
        f.outsider,
        Some(json!({ "title": "Payroll notes", "gap_id": f.stale_gap })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "outsider: writing a page for the stale-doc gap: {body}"
    );

    let (status, body) = call(
        Method::POST,
        format!("/api/knowledge-gaps/{}/resolve", f.open_gap),
        f.outsider,
        Some(json!({ "page_id": f.hidden })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "outsider: resolving with a page they can't open: {body}"
    );
    // The dashboard's top gaps are the same queue.
    let top_gap_ids = |body: &str| -> Option<Vec<i64>> {
        let stats: Value = serde_json::from_str(body).expect("stats json");
        stats["knowledgeGaps"]["top"]
            .as_array()
            .map(|top| top.iter().filter_map(|g| g["id"].as_i64()).collect())
    };
    let stats = "/api/dashboard/stats?include=knowledge_gaps".to_string();
    let gap_total = |body: &str| -> Option<i64> {
        let stats: Value = serde_json::from_str(body).expect("stats json");
        stats["knowledgeGaps"]["total"].as_i64()
    };
    let (status, body) = call(Method::GET, stats.clone(), f.insider, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        top_gap_ids(&body).is_some_and(|ids| ids.contains(&f.stale_gap)),
        "insider: the dashboard lists the stale-doc gap: {body}"
    );
    let insider_total = gap_total(&body).expect("insider: a gap count");
    let (status, body) = call(Method::GET, stats.clone(), f.outsider, None).await;
    assert_eq!(status, StatusCode::OK);
    let outsider_top = top_gap_ids(&body).expect("outsider: a gap queue");
    assert!(
        !outsider_top.contains(&f.stale_gap) && !outsider_top.contains(&f.drafting_gap),
        "outsider: the dashboard leaves out gaps naming the hidden page: {body}"
    );
    // Both gaps naming the hidden page drop out of the count too, so it
    // agrees with the (short) list.
    assert_eq!(
        gap_total(&body),
        Some(outsider_top.len() as i64),
        "outsider: the dashboard counts gaps it doesn't list: {body}"
    );
    assert_eq!(
        insider_total,
        outsider_top.len() as i64 + 2,
        "insider: the count includes the two gaps naming the hidden page"
    );
    let (status, body) = call(Method::GET, stats, f.member, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        top_gap_ids(&body),
        None,
        "a requester gets no gap queue: {body}"
    );

    // Flagging and unflagging a ticket whose gap is drafting in the hidden
    // page answer with the gap, less the draft. (Last: the unflag removes the
    // ticket's only flag, which dismisses the gap.)
    for (method, body) in [(Method::POST, Some(json!({}))), (Method::DELETE, None)] {
        let (status, resp) = call(method.clone(), flag_drafting.clone(), f.outsider, body).await;
        assert_eq!(status, StatusCode::OK, "outsider: {method} flag: {resp}");
        let gap: Value = serde_json::from_str(&resp).expect("gap json");
        assert_eq!(gap["id"], json!(f.drafting_gap), "{resp}");
        assert_eq!(
            gap["draft_page_id"],
            Value::Null,
            "outsider: {method} flag names the hidden draft: {resp}"
        );
    }

    let still_open = run_in_workspace(&pool, REF, f.workspace.workspace_id, |c| {
        knowledge_gaps::get_gap(c, f.open_gap)
    })
    .expect("reload gap");
    assert_eq!(
        still_open.status, "open",
        "the refused resolve changed nothing"
    );
    assert_eq!(still_open.resolved_page_id, None);
    let stale = run_in_workspace(&pool, REF, f.workspace.workspace_id, |c| {
        knowledge_gaps::get_gap(c, f.stale_gap)
    })
    .expect("reload stale-doc gap");
    assert_eq!(
        (stale.status.as_str(), stale.draft_page_id),
        ("open", None),
        "the refused dismiss and page create changed nothing"
    );
}
