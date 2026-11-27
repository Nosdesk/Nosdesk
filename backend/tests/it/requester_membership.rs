//! A requester an address already stands for elsewhere joins the workspace
//! their request lands in. Accounts are shared across workspaces, so a guest
//! of workspace A who writes to workspace B's mailbox (or B's guest form) is
//! reused, and the request's "View request" link, the portal sign-in link and
//! the sign-in code all need a membership in B.
//!
//! Only someone seated in another workspace joins. One the team here removed
//! stays removed, and so does someone with no seat anywhere, who may have been
//! removed here before removal kept the row; a deleted account is never
//! seated. Their mail still opens a ticket, as it always has.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use actix_web::{web, App};
use chrono::Utc;
use diesel::prelude::*;

use backend::db::DbConnection;
use backend::handlers::guest::{create_guest_submission, CreatedSubmission, GuestSubmission};
use backend::models::TicketPriority;
use backend::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use backend::repository::workspaces::{self, RemoveMembershipOutcome, SeatWriteAuthority};
use backend::services::channels::pipeline::PipelineOutcome;
use backend::services::search::SearchService;
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_context};

use crate::channel_backlog::{message, opened, Inbox};
use crate::common::{self, TestDb, TestPool};
use crate::portal_session::{context_of, link_server};

const REF: &str = "test:requester_membership";

/// Two workspaces, and an inbox on B's mailbox.
fn inbox_on_b() -> (Inbox, i32) {
    common::ensure_test_keyring();
    let db = TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = seeded.a.workspace_id;
    let pool = db.runtime_pool(4);
    (Inbox::on(Some(db), pool, seeded.b), a)
}

/// A guest of `ws`, as its guest form or mailbox provisions one.
fn guest_of(pool: &TestPool, ws: i32, email: &str) -> uuid::Uuid {
    run_in_workspace(pool, REF, ws, |c| {
        match find_or_create_guest_user(email, "Sam", c, None)? {
            GuestUserResult::Created(u) => Ok(u.uuid),
            _ => panic!("expected a new guest"),
        }
    })
    .expect("guest")
}

/// The person's role in `ws`, `None` without an active membership.
fn role_in(pool: &TestPool, ws: i32, user: uuid::Uuid) -> Option<String> {
    run_in_workspace(pool, REF, ws, |c| workspaces::membership(c, ws, user))
        .expect("membership")
        .map(|m| m.role)
}

fn conn(pool: &TestPool) -> DbConnection {
    pool.get().expect("conn")
}

/// A connection as the test superuser, for writes the app makes elsewhere.
fn seed_conn(inbox: &Inbox) -> DbConnection {
    conn(&inbox._db.as_ref().expect("sandbox").pool_with_size(1))
}

/// Every membership row the person has in `ws`, removed or not.
fn rows_in(inbox: &Inbox, ws: i32, user: uuid::Uuid) -> i64 {
    use backend::schema::workspace_members as m;
    m::table
        .filter(m::user_uuid.eq(user))
        .filter(m::workspace_id.eq(ws))
        .count()
        .get_result(&mut seed_conn(inbox))
        .expect("count")
}

/// Delete the person's membership rows (in `ws`, or everywhere), as removal
/// did before rows outlived the membership, or as the bug left a requester.
fn delete_rows(inbox: &Inbox, user: uuid::Uuid, ws: Option<i32>) {
    use backend::schema::workspace_members as m;
    let mut c = seed_conn(inbox);
    let rows = match ws {
        Some(ws) => diesel::delete(
            m::table
                .filter(m::user_uuid.eq(user))
                .filter(m::workspace_id.eq(ws)),
        )
        .execute(&mut c),
        None => diesel::delete(m::table.filter(m::user_uuid.eq(user))).execute(&mut c),
    }
    .expect("delete membership rows");
    assert!(rows > 0, "had a row to delete");
}

/// A verified primary address for `user`.
fn verified_email(inbox: &Inbox, user: uuid::Uuid, email: &str) {
    diesel::insert_into(backend::schema::user_emails::table)
        .values(&backend::models::NewUserEmail {
            user_uuid: user,
            email: email.into(),
            email_type: "personal".into(),
            is_primary: true,
            is_verified: true,
            source: None,
        })
        .execute(&mut seed_conn(inbox))
        .expect("email");
}

fn search(pool: &TestPool) -> (tempfile::TempDir, Arc<SearchService>) {
    let dir = tempfile::tempdir().expect("search dir");
    let search = Arc::new(SearchService::new(dir.path(), pool).expect("search"));
    (dir, search)
}

/// Submit B's request form as the guest handler does, pinned to B.
fn submit_form(inbox: &Inbox, email: &str, verification_required: bool) -> CreatedSubmission {
    let b = inbox.ws.workspace_id;
    let (_dir, search) = search(&inbox.pool);
    let submission = GuestSubmission {
        email,
        name: "Sam",
        title: "Printer jammed",
        description: "It jams on every page.",
        priority: TicketPriority::Medium,
        category_id: None,
        verification_required,
        lookup_token: uuid::Uuid::new_v4(),
    };
    let actor = ActorContext::system("guest:ticket_create").with_workspace(b);
    match with_actor_context(&mut conn(&inbox.pool), &actor, |c| {
        create_guest_submission(c, b, &search, &submission)
    }) {
        Ok(created) => created,
        Err(_) => panic!("the submission was refused"),
    }
}

/// Where the ticket's "View request" link for `user` lands.
async fn view_request(inbox: &Inbox, user: uuid::Uuid, ticket_id: i32) -> String {
    let ws = inbox.ws.workspace_id;
    let mut c = conn(&inbox.pool);
    let token =
        backend::utils::portal_ticket_link::sign(&mut c, ws, user, ticket_id).expect("sign");
    let srv = link_server(&inbox.pool, context_of(&mut c, ws));
    let resp = awc::Client::builder()
        .disable_redirects()
        .finish()
        .get(srv.url(&format!("/api/portal/auth/ticket?t={token}")))
        .send()
        .await
        .expect("send");
    assert_eq!(resp.status(), 302);
    resp.headers()
        .get("location")
        .expect("location")
        .to_str()
        .expect("ascii")
        .to_string()
}

fn ticket_path(number: i32) -> String {
    backend::handlers::portal::portal_path(&format!("/tickets/{number}"))
}

fn sign_in_error() -> String {
    backend::handlers::portal::portal_path("/login?signin_error=1")
}

fn is_member(inbox: &Inbox, user: uuid::Uuid) -> bool {
    backend::middleware::cookie_auth::is_workspace_member(
        &mut conn(&inbox.pool),
        inbox.ws.workspace_id,
        user,
    )
}

#[actix_web::test]
async fn a_guest_of_another_workspace_who_mails_in_can_open_their_request() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let guest = guest_of(&inbox.pool, a, "sam@guest.example");

    let ticket_id = opened(
        inbox
            .receive(
                &message("Sam <sam@guest.example>", "Printer", "cross-1"),
                Utc::now(),
            )
            .await,
    );
    let ticket = inbox.ticket(ticket_id);
    assert_eq!(ticket.requester_uuid, Some(guest), "the account is reused");
    assert_eq!(role_in(&inbox.pool, b, guest).as_deref(), Some("member"));
    assert_eq!(role_in(&inbox.pool, a, guest).as_deref(), Some("member"));
    // The gate the View request link, the magic link and the code share.
    assert!(is_member(&inbox, guest));
    assert_eq!(
        view_request(&inbox, guest, ticket_id).await,
        ticket_path(ticket.number)
    );
}

/// Redeeming a link in A verifies the address, so their next mail to B takes
/// the verified-sender branch instead of the guest reuse.
#[actix_web::test]
async fn a_requester_verified_in_another_workspace_who_mails_in_can_open_their_request() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let guest = guest_of(&inbox.pool, a, "verified@guest.example");
    backend::repository::user_emails::mark_primary_verified(&mut seed_conn(&inbox), &guest)
        .expect("verify");

    let ticket_id = opened(
        inbox
            .receive(
                &message("Sam <verified@guest.example>", "Laptop", "cross-2"),
                Utc::now(),
            )
            .await,
    );
    let ticket = inbox.ticket(ticket_id);
    assert_eq!(ticket.requester_uuid, Some(guest));
    assert_eq!(role_in(&inbox.pool, b, guest).as_deref(), Some("member"));
    assert_eq!(
        view_request(&inbox, guest, ticket_id).await,
        ticket_path(ticket.number)
    );
}

/// A staff member forwarding a customer's mail makes the customer the
/// requester, and the customer is seated.
#[actix_web::test]
async fn a_forwarded_customer_from_another_workspace_is_seated() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let customer = guest_of(&inbox.pool, a, "fwd@guest.example");
    verified_email(&inbox, inbox.ws.admin_uuid, "admin@b.example");

    let raw = "From: Admin <admin@b.example>\r\nTo: support@example.com\r\n\
               Subject: Fwd: Printer\r\nMessage-ID: <fwd-1@b.example>\r\n\
               Date: Mon, 1 Jan 2024 09:00:00 +0000\r\n\r\n\
               Please help this customer.\r\n\r\n\
               ---------- Forwarded message ---------\r\n\
               From: Sam <fwd@guest.example>\r\n\
               Date: Mon, Jan 1, 2024 at 08:00 AM\r\n\
               Subject: Printer\r\n\
               To: <admin@b.example>\r\n\r\n\
               My printer is on fire.\r\n";
    let ticket_id = opened(inbox.receive(raw, Utc::now()).await);
    let ticket = inbox.ticket(ticket_id);
    assert_eq!(ticket.requester_uuid, Some(customer), "the customer");
    assert_eq!(role_in(&inbox.pool, b, customer).as_deref(), Some("member"));
    assert_eq!(
        view_request(&inbox, customer, ticket_id).await,
        ticket_path(ticket.number)
    );
}

/// A requester left without a seat (as the bug left them) who replies on
/// their thread is seated by the reply.
#[actix_web::test]
async fn a_reply_on_an_existing_thread_seats_its_requester() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let guest = guest_of(&inbox.pool, a, "reply@guest.example");
    let ticket_id = opened(
        inbox
            .receive(
                &message("Sam <reply@guest.example>", "Printer", "thread-1"),
                Utc::now(),
            )
            .await,
    );
    delete_rows(&inbox, guest, Some(b));
    assert!(!is_member(&inbox, guest));

    let reply = "From: Sam <reply@guest.example>\r\nTo: support@example.com\r\n\
                 Subject: Re: Printer\r\nMessage-ID: <thread-2@customer.example>\r\n\
                 In-Reply-To: <thread-1@customer.example>\r\n\
                 References: <thread-1@customer.example>\r\n\
                 Date: Mon, 1 Jan 2024 10:00:00 +0000\r\n\r\nStill broken.\r\n";
    let outcome = inbox.receive(reply, Utc::now()).await;
    assert!(
        matches!(outcome, PipelineOutcome::ReplyAppended { ticket_id: t, .. } if t == ticket_id),
        "{outcome:?}"
    );
    assert_eq!(role_in(&inbox.pool, b, guest).as_deref(), Some("member"));
}

/// B's request form seats a guest of A, so the request's link signs them in.
#[actix_web::test]
async fn a_guest_of_another_workspace_who_uses_the_guest_form_can_open_their_request() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let guest = guest_of(&inbox.pool, a, "form@guest.example");

    let created = submit_form(&inbox, "Form@Guest.example", false);
    assert_eq!(created.user.uuid, guest);
    assert!(!created.is_new_guest);
    assert_eq!(role_in(&inbox.pool, b, guest).as_deref(), Some("member"));
    assert_eq!(
        view_request(&inbox, guest, created.ticket.id).await,
        ticket_path(created.ticket.number)
    );
}

/// With confirmation required, B's request is held until the guest confirms,
/// and confirming releases it in B, whichever workspace the account began in.
/// The account is seated in B by the confirmation, not by the submission: an
/// unproven address seats nobody.
#[actix_web::test]
async fn a_guest_of_another_workspace_confirms_a_held_request() {
    use backend::schema::reset_tokens as rt;
    use backend::utils::reset_tokens::{ResetTokenUtils, TokenType};

    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let guest = guest_of(&inbox.pool, a, "held@guest.example");

    let created = submit_form(&inbox, "held@guest.example", true);
    assert_eq!(created.user.uuid, guest);
    assert_eq!(
        created.ticket.verification_state.as_deref(),
        Some("pending")
    );
    assert_eq!(
        role_in(&inbox.pool, b, guest),
        None,
        "not seated before proof"
    );
    assert_eq!(rows_in(&inbox, b, guest), 0);

    // The confirmation the form sends names B. (Mailing its link needs a
    // configured origin; the token is stored first either way.)
    let http = actix_web::test::TestRequest::default().to_http_request();
    {
        use actix_web::HttpMessage as _;
        http.extensions_mut()
            .insert(context_of(&mut conn(&inbox.pool), b));
    }
    let actor = ActorContext::system("guest:ticket_create").with_workspace(b);
    with_actor_context::<_, diesel::result::Error>(&mut conn(&inbox.pool), &actor, |c| {
        backend::handlers::users::send_guest_ticket_confirmation(
            c,
            &http,
            guest,
            "held@guest.example",
            b,
        );
        Ok(())
    })
    .expect("send confirmation");
    let metadata: Option<serde_json::Value> = rt::table
        .filter(rt::user_uuid.eq(guest))
        .select(rt::metadata)
        .first(&mut seed_conn(&inbox))
        .expect("token");
    assert_eq!(metadata.expect("metadata")["workspace_id"], b);

    // Confirming with such a token releases the request in B.
    let issued = ResetTokenUtils::create_reset_token(guest, TokenType::Invitation);
    backend::repository::reset_tokens::create_reset_token(
        &mut seed_conn(&inbox),
        &issued.token_hash,
        guest,
        TokenType::Invitation.as_str(),
        None,
        None,
        issued.expires_at,
        Some(serde_json::json!({ "source": "guest_ticket_submission", "workspace_id": b })),
    )
    .expect("mint token");
    let (_dir, search) = search(&inbox.pool);
    let app = actix_web::test::init_service(
        App::new()
            .app_data(web::Data::new(inbox.pool.clone()))
            .app_data(web::Data::new(search))
            .route(
                "/confirm-guest",
                web::post().to(backend::handlers::invitation::confirm_guest_submission),
            ),
    )
    .await;
    let req = actix_web::test::TestRequest::post()
        .uri("/confirm-guest")
        .set_json(serde_json::json!({ "token": issued.raw_token }))
        .to_request();
    let resp = actix_web::test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    assert_eq!(
        inbox
            .ticket(created.ticket.id)
            .verification_state
            .as_deref(),
        Some("verified")
    );
    assert_eq!(role_in(&inbox.pool, b, guest).as_deref(), Some("member"));
    assert_eq!(
        view_request(&inbox, guest, created.ticket.id).await,
        ticket_path(created.ticket.number)
    );
}

/// No seat anywhere: they may have been removed here before removal kept the
/// row, so their mail opens a ticket but seats nobody.
#[actix_web::test]
async fn a_requester_with_no_seat_anywhere_is_not_seated() {
    let (inbox, _a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let former = guest_of(&inbox.pool, b, "legacy@guest.example");
    delete_rows(&inbox, former, None);

    let ticket_id = opened(
        inbox
            .receive(
                &message("Sam <legacy@guest.example>", "Again", "legacy-1"),
                Utc::now(),
            )
            .await,
    );
    assert_eq!(inbox.ticket(ticket_id).requester_uuid, Some(former));
    assert_eq!(rows_in(&inbox, b, former), 0, "not seated");
}

/// A deleted account is never seated.
#[actix_web::test]
async fn a_deleted_account_is_not_seated() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let gone = guest_of(&inbox.pool, a, "gone@guest.example");
    run_in_workspace(&inbox.pool, REF, a, |c| {
        backend::repository::users::soft_delete_user(&gone, c)
    })
    .expect("soft delete");

    let ticket_id = opened(
        inbox
            .receive(
                &message("Sam <gone@guest.example>", "Hello", "gone-1"),
                Utc::now(),
            )
            .await,
    );
    assert_eq!(inbox.ticket(ticket_id).requester_uuid, Some(gone));
    assert_eq!(rows_in(&inbox, b, gone), 0, "not seated");
}

/// A person the team removed stays removed when they write in again: the
/// mail still opens a ticket, as it always has, but their seat is not given
/// back, so their link signs nobody in. A staff member writing in keeps the
/// role they hold.
#[actix_web::test]
async fn a_removed_requester_stays_removed_and_staff_keep_their_role() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let former = guest_of(&inbox.pool, b, "former@guest.example");
    // A seat elsewhere, so only the removal keeps them out.
    run_in_workspace(&inbox.pool, REF, a, |c| {
        workspaces::add_membership(c, a, former, "member", SeatWriteAuthority::Product)
    })
    .expect("seat in A");
    let removed = run_in_workspace(&inbox.pool, REF, b, |c| {
        workspaces::remove_membership(c, b, former, SeatWriteAuthority::Product)
    })
    .expect("remove");
    assert_eq!(removed, RemoveMembershipOutcome::Removed);

    let ticket_id = opened(
        inbox
            .receive(
                &message("Former <former@guest.example>", "Again", "removed-1"),
                Utc::now(),
            )
            .await,
    );
    assert_eq!(inbox.ticket(ticket_id).requester_uuid, Some(former));
    assert_eq!(role_in(&inbox.pool, b, former), None, "still removed");
    assert!(!is_member(&inbox, former));
    assert_eq!(
        view_request(&inbox, former, ticket_id).await,
        sign_in_error()
    );

    // B's admin, verified, mails the desk directly.
    let admin = inbox.ws.admin_uuid;
    verified_email(&inbox, admin, "admin@b.example");
    let ticket_id = opened(
        inbox
            .receive(
                &message("Admin <admin@b.example>", "My own laptop", "admin-1"),
                Utc::now(),
            )
            .await,
    );
    assert_eq!(inbox.ticket(ticket_id).requester_uuid, Some(admin));
    assert_eq!(role_in(&inbox.pool, b, admin).as_deref(), Some("admin"));
}

/// A ticket's verification state, read past row security.
fn state_of(inbox: &Inbox, ticket: i32) -> Option<String> {
    use backend::schema::tickets as t;
    t::table
        .find(ticket)
        .select(t::verification_state)
        .first(&mut seed_conn(inbox))
        .expect("ticket")
}

/// A guest confirmation for `user` naming `ws`, as the form mints one.
fn guest_confirmation(inbox: &Inbox, user: uuid::Uuid, ws: i32) -> String {
    use backend::utils::reset_tokens::{ResetTokenUtils, TokenType};
    let issued = ResetTokenUtils::create_reset_token(user, TokenType::Invitation);
    backend::repository::reset_tokens::create_reset_token(
        &mut seed_conn(inbox),
        &issued.token_hash,
        user,
        TokenType::Invitation.as_str(),
        None,
        None,
        issued.expires_at,
        Some(serde_json::json!({ "source": "guest_ticket_submission", "workspace_id": ws })),
    )
    .expect("mint token");
    issued.raw_token
}

/// Whether the confirmation `raw` is spent.
fn spent(inbox: &Inbox, raw: &str) -> bool {
    use backend::schema::reset_tokens as rt;
    let hash = backend::utils::reset_tokens::ResetTokenUtils::hash_token(raw);
    rt::table
        .filter(rt::token_hash.eq(hash))
        .select(rt::is_used)
        .first(&mut seed_conn(inbox))
        .expect("token")
}

/// Accept a confirmation with a password, as self-hosted's accept page does.
async fn accept_with_password(inbox: &Inbox, raw: &str) -> u16 {
    let (_dir, search) = search(&inbox.pool);
    let app = actix_web::test::init_service(
        App::new()
            .app_data(web::Data::new(inbox.pool.clone()))
            .app_data(web::Data::new(search))
            .route(
                "/accept",
                web::post().to(backend::handlers::invitation::accept_invitation),
            ),
    )
    .await;
    let req = actix_web::test::TestRequest::post()
        .uri("/accept")
        .set_json(serde_json::json!({ "token": raw, "password": "correct horse battery" }))
        .to_request();
    match actix_web::test::try_call_service(&app, req).await {
        Ok(r) => r.status().as_u16(),
        Err(e) => e.as_response_error().status_code().as_u16(),
    }
}

/// Self-hosted, where the link sets a password: confirming B's held request
/// seats the account in B and releases it there, though the account began
/// in A (its lowest workspace, which the accept once fell back to).
#[actix_web::test]
async fn accepting_with_a_password_releases_the_request_where_it_was_held() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let guest = guest_of(&inbox.pool, a, "password@guest.example");
    let created = submit_form(&inbox, "password@guest.example", true);
    assert_eq!(role_in(&inbox.pool, b, guest), None);

    let raw = guest_confirmation(&inbox, guest, b);
    assert_eq!(accept_with_password(&inbox, &raw).await, 200);
    assert_eq!(
        state_of(&inbox, created.ticket.id).as_deref(),
        Some("verified")
    );
    assert_eq!(role_in(&inbox.pool, b, guest).as_deref(), Some("member"));
    assert!(spent(&inbox, &raw));
}

/// A seat that can't be written leaves the request held and the link usable;
/// following it again once the fault clears releases the request.
#[actix_web::test]
async fn a_failed_seat_leaves_the_request_recoverable() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let guest = guest_of(&inbox.pool, a, "fault@guest.example");
    let created = submit_form(&inbox, "fault@guest.example", true);
    let raw = guest_confirmation(&inbox, guest, b);

    diesel::sql_query(
        "CREATE FUNCTION test_refuse_seat() RETURNS trigger LANGUAGE plpgsql AS \
         $$ BEGIN RAISE EXCEPTION 'seat refused'; END $$",
    )
    .execute(&mut seed_conn(&inbox))
    .expect("install fault");
    diesel::sql_query(format!(
        "CREATE TRIGGER test_refuse_seat BEFORE INSERT ON workspace_members FOR EACH ROW \
         WHEN (NEW.user_uuid = '{guest}') EXECUTE FUNCTION test_refuse_seat()"
    ))
    .execute(&mut seed_conn(&inbox))
    .expect("install fault");
    assert_eq!(accept_with_password(&inbox, &raw).await, 500);
    assert_eq!(
        state_of(&inbox, created.ticket.id).as_deref(),
        Some("pending")
    );
    assert_eq!(rows_in(&inbox, b, guest), 0);
    assert!(!spent(&inbox, &raw), "the link still works");

    diesel::sql_query("DROP TRIGGER test_refuse_seat ON workspace_members")
        .execute(&mut seed_conn(&inbox))
        .expect("clear fault");
    assert_eq!(accept_with_password(&inbox, &raw).await, 200);
    assert_eq!(
        state_of(&inbox, created.ticket.id).as_deref(),
        Some("verified")
    );
    assert_eq!(role_in(&inbox.pool, b, guest).as_deref(), Some("member"));
}
