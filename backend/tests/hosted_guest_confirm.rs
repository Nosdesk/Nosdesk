//! On a hosted tenant origin, confirming a guest submission signs the guest in
//! to the portal and sends them to the ticket they confirmed, by its number.
//! The ticket here has an id that is not its number, so a link built from the
//! id would land on another ticket or none.
//!
//! A standalone binary rather than part of `tests/it`: deployment mode is
//! process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use actix_web::dev::Service as _;
use actix_web::{web, App, HttpMessage as _};
use diesel::prelude::*;

use backend::db::DbConnection;
use backend::extractors::WorkspaceContext;
use backend::models::{NewTicket, Ticket, WorkspaceRole};
use backend::repository::tickets::{self, TicketCreationAnnotation};
use backend::repository::workflow_states;
use backend::repository::workspaces::find_by_id;
use backend::services::search::SearchService;
use backend::sync::session::run_in_workspace;

const REF: &str = "test:hosted_guest_confirm";

fn open(pool: &common::TestPool, ws: i32, title: &str, pending_for: Option<uuid::Uuid>) -> Ticket {
    run_in_workspace(pool, REF, ws, |c| {
        let state = workflow_states::default_state(c)?.id;
        tickets::create_ticket_with_annotation(
            c,
            NewTicket {
                title: title.to_string(),
                workflow_state_id: state,
                requester_uuid: pending_for,
                verification_state: pending_for.map(|_| "pending".to_string()),
                ..Default::default()
            },
            TicketCreationAnnotation {
                source: Some("guest_portal".into()),
                from_email: Some("guest@example.com".into()),
                from_name: Some("Guest".into()),
                subject: Some(title.to_string()),
            },
            None,
        )
    })
    .expect("open ticket")
}

/// A guest-confirmation token, minted as the send path does.
fn guest_token(conn: &mut DbConnection, user: uuid::Uuid) -> String {
    use backend::utils::reset_tokens::{ResetTokenUtils, TokenType};
    let issued = ResetTokenUtils::create_reset_token(user, TokenType::Invitation);
    backend::repository::reset_tokens::create_reset_token(
        conn,
        &issued.token_hash,
        user,
        TokenType::Invitation.as_str(),
        None,
        None,
        issued.expires_at,
        Some(serde_json::json!({ "source": "guest_ticket_submission" })),
    )
    .expect("mint token");
    issued.raw_token
}

#[actix_web::test]
async fn confirming_on_hosted_sends_the_guest_to_the_tickets_number() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let pool = db.pool_with_size(4);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (&seeded.a, &seeded.b);

    // Tickets in the other workspace move ids ahead of this one's numbers.
    for title in ["Elsewhere one", "Elsewhere two"] {
        open(&pool, b.workspace_id, title, None);
    }
    let mut conn = pool.get().expect("conn");
    let guest = common::insert_plain_user(&mut conn, "Guest");
    diesel::insert_into(backend::schema::workspace_members::table)
        .values((
            backend::schema::workspace_members::workspace_id.eq(a.workspace_id),
            backend::schema::workspace_members::user_uuid.eq(guest),
            backend::schema::workspace_members::role.eq(WorkspaceRole::Member.as_str()),
        ))
        .on_conflict_do_nothing()
        .execute(&mut conn)
        .expect("membership");
    let ticket = open(&pool, a.workspace_id, "Printer on fire", Some(guest));
    assert_ne!(
        ticket.id, ticket.number,
        "the id must differ from the number"
    );
    let token = guest_token(&mut conn, guest);

    let origin = {
        let w = find_by_id(&mut conn, a.workspace_id)
            .expect("workspace lookup")
            .expect("workspace exists");
        WorkspaceContext {
            workspace_id: w.id,
            workspace_uuid: w.uuid,
            slug: w.slug,
            name: w.name,
            custom_domain: w.custom_domain,
            organisation_id: w.organisation_id,
        }
    };
    let index_dir = tempfile::tempdir().expect("index dir");
    let search = std::sync::Arc::new(SearchService::new(index_dir.path(), &pool).expect("search"));
    let app = actix_web::test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            // The tenant origin's workspace, as the hosted middleware resolves it.
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(origin.clone());
                srv.call(req)
            })
            .route(
                "/confirm-guest",
                web::post().to(backend::handlers::invitation::confirm_guest_submission),
            ),
    )
    .await;

    let req = actix_web::test::TestRequest::post()
        .uri("/confirm-guest")
        .set_json(serde_json::json!({ "token": token }))
        .to_request();
    let resp = actix_web::test::call_service(&app, req).await;
    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = actix_web::test::read_body_json(resp).await;
    assert_eq!(
        body["redirect_to"],
        format!("/tickets/{}", ticket.number),
        "by number (id {}): {body}",
        ticket.id
    );
}
