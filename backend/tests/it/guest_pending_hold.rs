//! A guest ticket awaiting email confirmation is held out of the workspace.
//!
//! While `verification_state = 'pending'` its events carry only the ticket's
//! own group (no workspace audience, so no SSE, delta, webhook, notification
//! or activity entry), and no staff viewer can see or open it. Confirmation
//! releases it with one `ticket.created` to the full audience, carrying the
//! submission's `created_via`.

#![allow(clippy::expect_used)]

use diesel::prelude::*;

use backend::db::DbConnection;
use backend::models::{NewTicket, PlatformRole, WorkspaceRole};
use backend::repository::ticket_visibility::{
    can_view_ticket, visible_ticket_ids, VisibilityContext,
};
use backend::repository::tickets::{self, TicketCreationAnnotation};
use backend::sync::groups::has_workspace_audience;

const WS: i32 = 1;

fn default_state(conn: &mut DbConnection) -> i32 {
    use backend::schema::workflow_states;
    workflow_states::table
        .filter(workflow_states::workspace_id.eq(WS))
        .filter(workflow_states::is_default.eq(true))
        .select(workflow_states::id)
        .first(conn)
        .expect("default state")
}

fn guest_ticket(
    conn: &mut DbConnection,
    requester: uuid::Uuid,
    pending: bool,
) -> backend::models::Ticket {
    let state = default_state(conn);
    tickets::create_ticket_with_annotation(
        conn,
        NewTicket {
            title: "Printer on fire".into(),
            workflow_state_id: state,
            requester_uuid: Some(requester),
            verification_state: pending.then(|| "pending".to_string()),
            ..Default::default()
        },
        TicketCreationAnnotation {
            source: Some("guest_portal".into()),
            from_email: Some("guest@example.com".into()),
            from_name: Some("Guest".into()),
            subject: Some("Printer on fire".into()),
        },
        None,
    )
    .expect("create ticket")
}

/// Every `ticket.created` for this ticket: (groups, created_via source).
fn created_events(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> Vec<(Vec<Option<String>>, serde_json::Value)> {
    use backend::schema::sync_actions;
    sync_actions::table
        .filter(sync_actions::aggregate_id.eq(ticket_id.to_string()))
        .filter(sync_actions::event_type.eq("ticket.created"))
        .order(sync_actions::sync_id.asc())
        .select((sync_actions::groups, sync_actions::data))
        .load::<(Vec<Option<String>>, serde_json::Value)>(conn)
        .expect("events")
        .into_iter()
        .map(|(g, d)| (g, d["created_via"]["source"].clone()))
        .collect()
}

fn staff(user: uuid::Uuid) -> VisibilityContext {
    VisibilityContext::new(user, PlatformRole::User, Some(WorkspaceRole::Admin))
}

#[test]
fn a_pending_guest_ticket_is_held_until_its_submitter_confirms() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let guest = crate::common::insert_user(&mut conn, "Guest");
    let agent = crate::common::insert_user(&mut conn, "Agent");
    let ticket = guest_ticket(&mut conn, guest.uuid, true);

    // Held: the event reaches only the ticket's own group, and staff can't see it.
    let events = created_events(&mut conn, ticket.id);
    assert_eq!(events.len(), 1);
    assert!(
        !has_workspace_audience(&events[0].0),
        "held: {:?}",
        events[0].0
    );
    let ctx = staff(agent.uuid);
    assert!(!can_view_ticket(&mut conn, &ctx, ticket.id).expect("can view"));
    assert!(visible_ticket_ids(&mut conn, &ctx, &[ticket.id])
        .expect("visible")
        .is_empty());

    // Confirmed: one ticket.created to the workspace, as submitted.
    let released = tickets::verify_pending_tickets_for_user(&mut conn, guest.uuid).expect("verify");
    assert_eq!(released.len(), 1);
    let events = created_events(&mut conn, ticket.id);
    assert_eq!(events.len(), 2);
    assert!(
        has_workspace_audience(&events[1].0),
        "released: {:?}",
        events[1].0
    );
    assert_eq!(events[1].1, "guest_portal");
    assert!(can_view_ticket(&mut conn, &ctx, ticket.id).expect("can view"));
}

#[test]
fn a_guest_ticket_without_confirmation_reaches_the_workspace_at_once() {
    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let guest = crate::common::insert_user(&mut conn, "Guest");
    let agent = crate::common::insert_user(&mut conn, "Agent");
    let ticket = guest_ticket(&mut conn, guest.uuid, false);

    let events = created_events(&mut conn, ticket.id);
    assert_eq!(events.len(), 1);
    assert!(has_workspace_audience(&events[0].0));
    assert!(can_view_ticket(&mut conn, &staff(agent.uuid), ticket.id).expect("can view"));
}

/// Mint a guest-confirmation (or plain invitation) token as the send path does.
fn invitation_token(conn: &mut DbConnection, user: uuid::Uuid, guest: bool) -> String {
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
        guest.then(|| serde_json::json!({ "source": "guest_ticket_submission" })),
    )
    .expect("mint token");
    issued.raw_token
}

fn confirm_server(pool: &crate::common::TestPool) -> actix_test::TestServer {
    use actix_web::{web, App};
    use backend::services::search::SearchService;
    use std::sync::Arc;
    let pool = pool.clone();
    actix_test::start(move || {
        let tmp = tempfile::tempdir().expect("temp search dir");
        let search = Arc::new(SearchService::new(tmp.path(), &pool).expect("init search"));
        std::mem::forget(tmp);
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .app_data(web::Data::new(search))
            .route(
                "/confirm-guest",
                web::post().to(backend::handlers::invitation::confirm_guest_submission),
            )
    })
}

/// The emailed link releases the held ticket without setting a password (the
/// only route on hosted, where local credentials are off), once.
#[actix_web::test]
async fn the_confirmation_link_releases_a_held_ticket_without_a_password() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let guest = crate::common::insert_user(&mut conn, "Guest");
    diesel::insert_into(backend::schema::workspace_members::table)
        .values((
            backend::schema::workspace_members::workspace_id.eq(WS),
            backend::schema::workspace_members::user_uuid.eq(guest.uuid),
            backend::schema::workspace_members::role.eq(WorkspaceRole::Member.as_str()),
        ))
        .on_conflict_do_nothing()
        .execute(&mut conn)
        .expect("membership");
    let ticket = guest_ticket(&mut conn, guest.uuid, true);
    let plain = invitation_token(&mut conn, guest.uuid, false);
    let token = invitation_token(&mut conn, guest.uuid, true);

    let srv = confirm_server(&pool);
    let client = awc::Client::new();
    let post = |t: &str| {
        client
            .post(srv.url("/confirm-guest"))
            .send_json(&serde_json::json!({ "token": t }))
    };

    // A staff invitation can't be spent here: it must still set a password.
    assert_eq!(post(&plain).await.expect("send").status(), 400);

    assert_eq!(post(&token).await.expect("send").status(), 200);
    let state: Option<String> = backend::schema::tickets::table
        .find(ticket.id)
        .select(backend::schema::tickets::verification_state)
        .first(&mut conn)
        .expect("ticket");
    assert_ne!(state.as_deref(), Some("pending"), "released");
    let events = created_events(&mut conn, ticket.id);
    assert_eq!(events.len(), 2);
    assert!(has_workspace_audience(&events[1].0));

    assert_eq!(
        post(&token).await.expect("send").status(),
        400,
        "single use"
    );
}

/// Every webhook delivery the outbox holds for `webhook_id`, by event type,
/// draining it as the background worker does.
fn drain_webhook_events(conn: &mut DbConnection, webhook_id: i32) -> Vec<String> {
    use backend::services::webhooks::service::WebhookService;
    use backend::sync::actor::ActorContext;
    use backend::sync::session::with_actor_bypass_context;
    let actor = ActorContext::system("test:guest_pending_hold");
    let mut raised = Vec::new();
    loop {
        let (tasks, more) =
            with_actor_bypass_context(conn, &actor, WebhookService::drain_batch_txn)
                .expect("drain outbox");
        raised.extend(
            tasks
                .into_iter()
                .filter(|t| t.webhook_id == webhook_id)
                .map(|t| t.payload.event_type),
        );
        if !more {
            return raised;
        }
    }
}

/// The groups of each `event_type` row recorded for `aggregate_id`.
fn event_groups(
    conn: &mut DbConnection,
    event_type: &str,
    aggregate_id: i32,
) -> Vec<Vec<Option<String>>> {
    use backend::schema::sync_actions;
    sync_actions::table
        .filter(sync_actions::event_type.eq(event_type))
        .filter(sync_actions::aggregate_id.eq(aggregate_id.to_string()))
        .order(sync_actions::sync_id.asc())
        .select(sync_actions::groups)
        .load(conn)
        .expect("events")
}

/// A held submission's description and files reach integrations once it's
/// confirmed, as an unheld one's do at once.
#[test]
fn confirming_a_guest_ticket_sends_its_held_reply_and_files() {
    use backend::models::{NewAttachment, NewComment};
    use backend::repository::comments;

    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let guest = crate::common::insert_user(&mut conn, "Guest");
    let webhook = backend::repository::webhooks::create_webhook(
        &mut conn,
        "Integration".into(),
        "https://sink.invalid/hook".into(),
        "secret".into(),
        vec!["comment.added".into(), "attachment.added".into()],
        None,
        None,
    )
    .expect("webhook");
    let ticket = guest_ticket(&mut conn, guest.uuid, true);
    // The description and a file, written as the guest form writes them.
    let reply = comments::create_comment(
        &mut conn,
        NewComment {
            content: "<p>It smells of toner</p>".into(),
            ticket_id: ticket.id,
            user_uuid: guest.uuid,
            ..Default::default()
        },
        None,
    )
    .expect("reply");
    let draft = comments::create_attachment(
        &mut conn,
        NewAttachment {
            url: "/uploads/temp/photo.jpg".into(),
            name: "photo.jpg".into(),
            file_size: Some(4),
            mime_type: Some("image/jpeg".into()),
            checksum: None,
            comment_id: None,
            uploaded_by: None,
            transcription: None,
        },
    )
    .expect("draft");
    comments::reparent_attachment(
        &mut conn,
        draft.id,
        &format!("/uploads/tickets/{}/photo.jpg", ticket.id),
        reply.id,
        guest.uuid,
    )
    .expect("claim");

    // Held: nothing reaches the integration.
    assert!(drain_webhook_events(&mut conn, webhook.id).is_empty());

    tickets::verify_pending_tickets_for_user(&mut conn, guest.uuid).expect("verify");

    for (event_type, id) in [
        ("comment.created", reply.id),
        ("attachment.attached", draft.id),
    ] {
        let groups = event_groups(&mut conn, event_type, id);
        assert!(
            groups.iter().any(|g| has_workspace_audience(g)),
            "{event_type} after release: {groups:?}"
        );
    }
    let mut raised = drain_webhook_events(&mut conn, webhook.id);
    raised.sort();
    assert_eq!(raised, ["attachment.added", "comment.added"]);
    // Confirming once more sends nothing again.
    tickets::verify_pending_tickets_for_user(&mut conn, guest.uuid).expect("verify again");
    assert!(drain_webhook_events(&mut conn, webhook.id).is_empty());
}

/// Each held event is sent on once: the copy names the held row, a second
/// release sends nothing, and another ticket's held events stay held.
#[test]
fn a_held_event_is_released_once_and_only_for_its_ticket() {
    use backend::models::NewComment;
    use backend::repository::comments;
    use backend::schema::sync_actions;

    let db = crate::common::TestDb::new();
    let mut conn = db.conn();
    let guest = crate::common::insert_user(&mut conn, "Guest");
    let other = crate::common::insert_user(&mut conn, "Other guest");
    let ticket = guest_ticket(&mut conn, guest.uuid, true);
    let elsewhere = guest_ticket(&mut conn, other.uuid, true);
    let mut reply_on = |ticket_id: i32, user_uuid: uuid::Uuid| {
        comments::create_comment(
            &mut conn,
            NewComment {
                content: "<p>It smells of toner</p>".into(),
                ticket_id,
                user_uuid,
                ..Default::default()
            },
            None,
        )
        .expect("reply")
    };
    let reply = reply_on(ticket.id, guest.uuid);
    let held_reply = reply_on(elsewhere.id, other.uuid);

    let released = tickets::verify_pending_tickets_for_user(&mut conn, guest.uuid).expect("verify");
    let ticket = released.into_iter().next().expect("released ticket");

    let reply_rows = |conn: &mut DbConnection| -> Vec<(uuid::Uuid, Option<uuid::Uuid>)> {
        sync_actions::table
            .filter(sync_actions::event_type.eq("comment.created"))
            .filter(sync_actions::aggregate_id.eq(reply.id.to_string()))
            .order(sync_actions::sync_id.asc())
            .select((sync_actions::event_uuid, sync_actions::causation_id))
            .load(conn)
            .expect("rows")
    };
    let rows = reply_rows(&mut conn);
    assert_eq!(rows.len(), 2, "the held reply and its release");
    assert_eq!(rows[1].1, Some(rows[0].0), "the release names the held row");

    let again = backend::sync::hold::release(&mut conn, &ticket).expect("release again");
    assert_eq!(again, 0, "a second release sends nothing");
    assert_eq!(reply_rows(&mut conn).len(), 2);

    let held = event_groups(&mut conn, "comment.created", held_reply.id);
    assert_eq!(held.len(), 1, "another ticket's held rows stay put");
    assert!(
        !has_workspace_audience(&held[0]),
        "still held: {:?}",
        held[0]
    );
}
