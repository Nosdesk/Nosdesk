//! Accounts are shared across workspaces, so a workspace's staff must not reach
//! someone who belongs only to another one, and nobody's role in one workspace
//! decides what they may do in another.
//!
//! - A ticket's requester is one of the workspace's own people. Naming a
//!   stranger by uuid is refused as a uuid no account has is refused, and a
//!   stranger's name finds nobody, so their address never reaches the caller.
//! - Inbound mail trusts a forward only from staff of the workspace the mail
//!   is for: an agent of another workspace is an ordinary sender there, and a
//!   workspace's own agent keeps the trust whatever seats they hold elsewhere.
//! - An address that is staff in any workspace is never reused for a guest
//!   request, and mail that fails DMARC is never ingested as that account.

use std::sync::Arc;

use actix_web::dev::Service;
use actix_web::http::{Method, StatusCode};
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use chrono::Utc;
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::extractors::WorkspaceContext;
use backend::middleware::RequestContext;
use backend::models::{Claims, NewTicket, NewUserEmail};
use backend::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use backend::repository::workflow_states;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::schema::{tickets, user_emails};
use backend::services::channels::pipeline::PipelineOutcome;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::channel_backlog::{as_admin, opened, Inbox};
use crate::common::{self, TestPool, WorkspaceSeed};

/// Where the person who belongs only to workspace B can be reached.
const STRANGER_EMAIL: &str = "ceo@bigcorp.example";

struct Fixture {
    db: common::TestDb,
    _search_dir: tempfile::TempDir,
    pool: TestPool,
    a: WorkspaceSeed,
    b: WorkspaceSeed,
    search: Arc<SearchService>,
}

impl Fixture {
    fn new() -> Self {
        common::ensure_test_keyring();
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let two = common::seed_two_workspaces(&mut pool.get().expect("conn"));
        let search_dir = tempfile::tempdir().expect("search dir");
        let search = Arc::new(SearchService::new(search_dir.path(), &pool).expect("init search"));
        let f = Self {
            db,
            _search_dir: search_dir,
            pool,
            a: two.a,
            b: two.b,
            search,
        };
        f.email(f.b.member_uuid, STRANGER_EMAIL, true);
        f
    }

    /// Give `user` a primary address.
    fn email(&self, user: Uuid, email: &str, verified: bool) {
        diesel::insert_into(user_emails::table)
            .values(&NewUserEmail {
                user_uuid: user,
                email: email.to_string(),
                email_type: "personal".to_string(),
                is_primary: true,
                is_verified: verified,
                source: None,
            })
            .execute(&mut self.pool.get().expect("conn"))
            .expect("add the address");
    }

    /// A new account with no seat anywhere.
    fn account(&self, name: &str) -> Uuid {
        common::insert_plain_user(&mut self.pool.get().expect("conn"), name)
    }

    /// Seat `user` in `ws` with `role`.
    fn seat(&self, ws: &WorkspaceSeed, user: Uuid, role: &str) {
        as_admin(&self.pool, ws, |c| {
            add_membership(
                c,
                ws.workspace_id,
                user,
                role,
                SeatWriteAuthority::ControlPlane,
            )
        });
    }

    /// A ticket in A that A's member raised.
    fn ticket(&self) -> i32 {
        as_admin(&self.pool, &self.a, |c| {
            let open = workflow_states::default_state(c)?.id;
            diesel::insert_into(tickets::table)
                .values(&NewTicket {
                    title: "Printer jammed".to_string(),
                    workflow_state_id: open,
                    requester_uuid: Some(self.a.member_uuid),
                    ..Default::default()
                })
                .returning(tickets::id)
                .get_result(c)
        })
    }

    fn requester_of(&self, ticket: i32) -> Option<Uuid> {
        as_admin(&self.pool, &self.a, |c| {
            tickets::table
                .find(ticket)
                .select(tickets::requester_uuid)
                .first(c)
        })
    }

    async fn patch(&self, ticket: i32, body: Value) -> (StatusCode, Value) {
        self.send(Method::PATCH, &format!("/api/tickets/{ticket}"), body)
            .await
    }

    /// Push one ticket update; `Ok` when applied, `Err(reason)` when refused.
    async fn push(&self, ticket: i32, patch: Value) -> Result<(), String> {
        let (status, body) = self
            .send(
                Method::POST,
                "/api/sync/push",
                json!([{
                    "tx_id": Uuid::now_v7().to_string(),
                    "aggregate": "ticket",
                    "model_id": ticket.to_string(),
                    "op": "U",
                    "patch": patch,
                }]),
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        match body["rejected"].as_array().and_then(|r| r.first()) {
            Some(rejected) => Err(rejected["reason"].as_str().unwrap_or_default().to_string()),
            None => Ok(()),
        }
    }

    /// Call the ticket routes and sync push in A as A's admin.
    async fn send(&self, method: Method, uri: &str, body: Value) -> (StatusCode, Value) {
        let user = self.a.admin_uuid;
        let now = Utc::now().timestamp();
        let claims = Claims {
            sub: user.to_string(),
            name: "Admin".to_string(),
            email: "admin@example.com".to_string(),
            platform_role: "user".to_string(),
            scope: "full".to_string(),
            sid: None,
            workspace_uuid: None,
            exp: (now + 3600) as usize,
            iat: now as usize,
            file_path: None,
        };
        let workspace = WorkspaceContext {
            workspace_id: self.a.workspace_id,
            workspace_uuid: self.a.workspace_uuid,
            slug: self.a.slug.clone(),
            name: "A".to_string(),
            organisation_id: None,
            custom_domain: None,
        };
        let corr = Uuid::now_v7();
        let actor = ActorContext::user(user, Some(corr)).with_workspace(self.a.workspace_id);
        let app = http_test::init_service(
            App::new()
                .app_data(web::Data::new(self.pool.clone()))
                .app_data(web::Data::new(self.search.clone()))
                .wrap_fn(move |req, srv| {
                    req.extensions_mut().insert(workspace.clone());
                    req.extensions_mut().insert(claims.clone());
                    req.extensions_mut()
                        .insert(RequestContext::new(corr, actor.clone()));
                    srv.call(req)
                })
                .service(
                    web::scope("/api")
                        .configure(backend::handlers::tickets::config)
                        .route(
                            "/sync/push",
                            web::post().to(backend::handlers::sync::push::push),
                        ),
                ),
        )
        .await;
        let mut req = http_test::TestRequest::default().method(method).uri(uri);
        if !body.is_null() {
            req = req.set_json(body);
        }
        let resp = http_test::call_service(&app, req.to_request()).await;
        let status = resp.status();
        let bytes = http_test::read_body(resp).await;
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// An inbox on B's mailbox, reached the way the channel poll reaches it.
    fn inbox_b(&self) -> Inbox {
        Inbox::on(None, self.db.runtime_pool(4), self.b.clone())
    }
}

/// Mail into the inbox, with the receiving server's DMARC verdict and a body.
fn mail(from: &str, dmarc: &str, id: &str, body: &str) -> String {
    format!(
        "Authentication-Results: mx.example.com; dmarc={dmarc} header.from=example\r\n\
         From: {from}\r\nTo: support@example.com\r\nSubject: Fwd: Laptop\r\n\
         Message-ID: <{id}@sender.example>\r\nDate: Mon, 1 Jan 2024 09:00:00 +0000\r\n\r\n\
         {body}\r\n"
    )
}

/// A forward of `customer`'s message, as Gmail writes one.
fn forward_of(customer: &str) -> String {
    format!(
        "Please handle this one.\r\n\r\n\
         ---------- Forwarded message ---------\r\n\
         From: Customer <{customer}>\r\n\
         Date: Mon, Jan 1, 2024 at 10:00 AM\r\n\
         Subject: Laptop\r\n\
         To: <someone@example.com>\r\n\r\n\
         My laptop won't start.\r\n"
    )
}

fn requester_in(inbox: &Inbox, ticket: i32) -> Option<Uuid> {
    inbox.ticket(ticket).requester_uuid
}

fn shows_stranger(body: &Value) -> bool {
    body.to_string().contains(STRANGER_EMAIL)
}

#[actix_web::test]
async fn a_person_of_another_workspace_cannot_be_made_a_requester() {
    let f = Fixture::new();
    let ticket = f.ticket();
    let stranger = f.b.member_uuid;
    let nobody = Uuid::new_v4();

    // By uuid: refused exactly as a uuid no account has is refused.
    let (nobody_status, nobody_body) = f
        .patch(ticket, json!({ "requester": nobody.to_string() }))
        .await;
    let (status, body) = f
        .patch(ticket, json!({ "requester": stranger.to_string() }))
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        (status, &body),
        (nobody_status, &nobody_body),
        "a stranger reads as no one"
    );
    assert!(!shows_stranger(&body), "{body}");
    assert_eq!(f.requester_of(ticket), Some(f.a.member_uuid));

    // By name: finds nobody, so nothing changes, as for a name no one has.
    let (status, body) = f.patch(ticket, json!({ "requester": "b Member" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(!shows_stranger(&body), "{body}");
    assert_eq!(f.requester_of(ticket), Some(f.a.member_uuid));

    // Through sync push, which is how the app sets a requester.
    assert_eq!(
        f.push(ticket, json!({ "requester_uuid": stranger })).await,
        f.push(ticket, json!({ "requester_uuid": nobody })).await,
    );
    assert_eq!(
        f.push(ticket, json!({ "requester_uuid": stranger })).await,
        Err("invalid_reference".to_string())
    );
    assert_eq!(f.requester_of(ticket), Some(f.a.member_uuid));

    // A new ticket filed for them.
    let open = as_admin(&f.pool, &f.a, workflow_states::default_state).id;
    let new = |requester: Uuid| {
        json!({
            "title": "On someone's behalf",
            "workflow_state_id": open,
            "priority": "medium",
            "requester_uuid": requester,
        })
    };
    let (nobody_status, nobody_body) = f.send(Method::POST, "/api/tickets", new(nobody)).await;
    let (status, body) = f.send(Method::POST, "/api/tickets", new(stranger)).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!((status, &body), (nobody_status, &nobody_body));

    // Nothing in A names them.
    let named: i64 = as_admin(&f.pool, &f.a, |c| {
        tickets::table
            .filter(tickets::requester_uuid.eq(stranger))
            .count()
            .get_result(c)
    });
    assert_eq!(named, 0);
}

#[actix_web::test]
async fn the_workspace_s_own_people_can_be_made_requesters() {
    let f = Fixture::new();
    let ticket = f.ticket();

    let (status, body) = f
        .patch(ticket, json!({ "requester": f.a.admin_uuid.to_string() }))
        .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(f.requester_of(ticket), Some(f.a.admin_uuid));

    let (status, body) = f.patch(ticket, json!({ "requester": "a Member" })).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(f.requester_of(ticket), Some(f.a.member_uuid));

    // The app adds someone new by address, then picks them.
    let (status, created) = crate::add_requester::post_user(
        &f.pool,
        &f.a,
        json!({
            "name": "Pat New",
            "email": "pat.new@example.com",
            "role": "user",
            "send_invitation": false,
        }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let pat = Uuid::parse_str(created["uuid"].as_str().expect("uuid")).expect("uuid");
    assert_eq!(
        f.push(ticket, json!({ "requester_uuid": pat })).await,
        Ok(())
    );
    assert_eq!(f.requester_of(ticket), Some(pat));
}

/// An agent of their own (trial) workspace forwards "a customer's" mail to
/// B, which owes them no trust: they are the requester, and the person the
/// forward names is not.
///
/// A guard, not a reproduction: it passed before the check named
/// `channel.workspace_id`. The ingest transaction's `set_actor` drops the
/// elevated connection to the runtime role and pins B, so row security
/// already confined the role read to B there; no path reaches that read
/// elevated, which is what would have returned A's role. The explicit
/// workspace keeps it right if that ever changes.
#[actix_web::test]
async fn a_forward_from_staff_of_another_workspace_is_ordinary_mail() {
    let f = Fixture::new();
    f.email(f.a.admin_uuid, "agent@trial.example", true);
    let inbox = f.inbox_b();

    let ticket = opened(
        inbox
            .receive(
                &mail(
                    "Trial Agent <agent@trial.example>",
                    "pass",
                    "fwd-elsewhere",
                    &forward_of(STRANGER_EMAIL),
                ),
                Utc::now(),
            )
            .await,
    );
    assert_eq!(requester_in(&inbox, ticket), Some(f.a.admin_uuid));
}

/// B's own agent, who also raised a request in A, still forwards for a
/// customer in B.
#[actix_web::test]
async fn a_workspace_s_agent_forwards_whatever_seats_they_hold_elsewhere() {
    let f = Fixture::new();
    let agent = f.account("Bea Agent");
    // The seat elsewhere first, in the lower workspace.
    f.seat(&f.a, agent, "member");
    f.seat(&f.b, agent, "agent");
    f.email(agent, "bea@helpdesk.example", true);
    let inbox = f.inbox_b();

    let ticket = opened(
        inbox
            .receive(
                &mail(
                    "Bea Agent <bea@helpdesk.example>",
                    "pass",
                    "fwd-own",
                    &forward_of("customer@client.example"),
                ),
                Utc::now(),
            )
            .await,
    );
    let requester = requester_in(&inbox, ticket).expect("requester");
    assert_ne!(requester, agent, "the forward names the requester");
    let address = as_admin(&inbox.pool, &f.b, |c| {
        user_emails::table
            .filter(user_emails::user_uuid.eq(requester))
            .select(user_emails::email)
            .first::<String>(c)
    });
    assert_eq!(address, "customer@client.example");
}

/// Someone types an agent's address into B's guest form: the agent's
/// account is not reused, whichever workspace they are staff in.
#[test]
fn a_guest_request_never_reuses_staff_of_another_workspace() {
    let f = Fixture::new();
    f.email(f.a.admin_uuid, "agent@trial.example", false);

    let mut conn = f.pool.get().expect("conn");
    let actor = ActorContext::system("test:guest").with_workspace(f.b.workspace_id);
    let outcome = with_actor_context(&mut conn, &actor, |c| {
        find_or_create_guest_user("agent@trial.example", "Not The Agent", c, None)
    })
    .expect("guest lookup");
    assert!(
        matches!(outcome, GuestUserResult::EmailClaimed),
        "an agent elsewhere is claimed"
    );
}

/// Mail into B that fails DMARC and names an agent of A is not ingested as
/// that agent, even when they hold a requester seat in B as well.
#[actix_web::test]
async fn spoofed_mail_naming_staff_of_another_workspace_is_refused() {
    let f = Fixture::new();
    let agent = f.account("Ada Agent");
    // The requester seat in B first, so it is the row an unordered read finds.
    f.seat(&f.b, agent, "member");
    f.seat(&f.a, agent, "agent");
    f.email(agent, "ada@trial.example", false);
    let inbox = f.inbox_b();

    let outcome = inbox
        .receive(
            &mail(
                "Ada Agent <ada@trial.example>",
                "fail",
                "spoof",
                "Please reset my password.",
            ),
            Utc::now(),
        )
        .await;
    assert!(
        matches!(outcome, PipelineOutcome::SkippedSpoofedSender),
        "{outcome:?}"
    );
    let attached: i64 = as_admin(&inbox.pool, &f.b, |c| {
        tickets::table
            .filter(tickets::requester_uuid.eq(agent))
            .count()
            .get_result(c)
    });
    assert_eq!(attached, 0);
}

/// The DMARC guard runs before the ingest transaction, on the elevated
/// connection, so it judges B's own agent by the seats it reads there: a
/// requester seat in A must not make them look like a requester.
#[actix_web::test]
async fn spoofed_mail_naming_a_workspace_s_agent_is_refused_whatever_seats_they_hold_elsewhere() {
    let f = Fixture::new();
    let agent = f.account("Bo Agent");
    // The seat elsewhere in the lower workspace, so it is the row an
    // unordered read finds first.
    f.seat(&f.a, agent, "member");
    f.seat(&f.b, agent, "agent");
    f.email(agent, "bo@helpdesk.example", false);
    let inbox = f.inbox_b();

    let outcome = inbox
        .receive(
            &mail(
                "Bo Agent <bo@helpdesk.example>",
                "fail",
                "spoof-own",
                &forward_of("customer@client.example"),
            ),
            Utc::now(),
        )
        .await;
    assert!(
        matches!(outcome, PipelineOutcome::SkippedSpoofedSender),
        "{outcome:?}"
    );
}
