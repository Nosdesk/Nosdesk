//! A requester an address already stands for elsewhere joins the workspace
//! their request lands in. Accounts are shared across workspaces, so a guest
//! of workspace A who writes to workspace B's mailbox (or B's guest form) is
//! reused, and the request's "View request" link, the portal sign-in link and
//! the sign-in code all need a membership in B. One the team here removed
//! stays removed: their mail still opens a ticket, but nothing signs them back
//! in.

#![allow(clippy::expect_used)]

use chrono::Utc;

use backend::db::DbConnection;
use backend::repository::user_helpers::{
    find_or_create_guest_requester, find_or_create_guest_user, GuestUserResult,
};
use backend::repository::workspaces::{self, RemoveMembershipOutcome, SeatWriteAuthority};
use backend::sync::session::run_in_workspace;

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

/// B's guest form resolves its requester with `find_or_create_guest_requester`
/// (the handler itself checks the address's mail servers, so it isn't driven
/// here).
#[actix_web::test]
async fn a_guest_of_another_workspace_who_uses_the_guest_form_joins_it() {
    let (inbox, a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let guest = guest_of(&inbox.pool, a, "form@guest.example");

    let reused = run_in_workspace(
        &inbox.pool,
        REF,
        b,
        |c| match find_or_create_guest_requester("Form@Guest.example", "Sam", b, c, None)? {
            GuestUserResult::Existing(u) => Ok(u.uuid),
            _ => panic!("expected the guest of A to be reused"),
        },
    )
    .expect("guest form");
    assert_eq!(reused, guest);
    assert_eq!(role_in(&inbox.pool, b, guest).as_deref(), Some("member"));
}

/// A person the team removed stays removed when they write in again: the
/// mail still opens a ticket, as it always has, but their seat is not given
/// back, so their link signs nobody in. A staff member writing in keeps the
/// role they hold.
#[actix_web::test]
async fn a_removed_requester_stays_removed_and_staff_keep_their_role() {
    let (inbox, _a) = inbox_on_b();
    let b = inbox.ws.workspace_id;
    let former = guest_of(&inbox.pool, b, "former@guest.example");
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
    {
        use diesel::RunQueryDsl as _;
        diesel::insert_into(backend::schema::user_emails::table)
            .values(&backend::models::NewUserEmail {
                user_uuid: admin,
                email: "admin@b.example".into(),
                email_type: "personal".into(),
                is_primary: true,
                is_verified: true,
                source: None,
            })
            .execute(&mut seed_conn(&inbox))
            .expect("admin email");
    }
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
