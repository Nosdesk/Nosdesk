//! The daily cleanup removes what the public request form leaves when nobody
//! follows through (a request never confirmed, the account made for it), and
//! nothing that anyone used.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use diesel::prelude::*;

use backend::models::NewTicket;
use backend::repository::tickets::{self, TicketCreationAnnotation};
use backend::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

const WS: i32 = 1;

fn guest(conn: &mut backend::db::DbConnection, email: &str) -> backend::models::User {
    match find_or_create_guest_user(email, "Guest", conn, None).expect("guest") {
        GuestUserResult::Created(u) => u,
        _ => panic!("expected a new guest"),
    }
}

fn request(conn: &mut backend::db::DbConnection, requester: uuid::Uuid, pending: bool) -> i32 {
    use backend::schema::workflow_states;
    let state: i32 = workflow_states::table
        .filter(workflow_states::workspace_id.eq(WS))
        .filter(workflow_states::is_default.eq(true))
        .select(workflow_states::id)
        .first(conn)
        .expect("default state");
    tickets::create_ticket_with_annotation(
        conn,
        NewTicket {
            title: "Printer".into(),
            workflow_state_id: state,
            requester_uuid: Some(requester),
            verification_state: pending.then(|| "pending".to_string()),
            ..Default::default()
        },
        TicketCreationAnnotation::default(),
        None,
    )
    .expect("create ticket")
    .id
}

/// Age a request and the account behind it past the unconfirmed window.
fn backdate(conn: &mut backend::db::DbConnection, ticket: i32, user: uuid::Uuid) {
    let actor = ActorContext::system("test:backdate").with_workspace(WS);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, |c| {
        diesel::sql_query(
            "UPDATE tickets SET created_at = now() - interval '15 days' WHERE id = $1",
        )
        .bind::<diesel::sql_types::Integer, _>(ticket)
        .execute(c)?;
        diesel::sql_query(
            "UPDATE users SET created_at = now() - interval '15 days' WHERE uuid = $1",
        )
        .bind::<diesel::sql_types::Uuid, _>(user)
        .execute(c)?;
        Ok(())
    })
    .expect("backdate");
}

fn user_exists(conn: &mut backend::db::DbConnection, user: uuid::Uuid) -> bool {
    use backend::schema::users;
    users::table
        .find(user)
        .filter(users::deleted_at.is_null())
        .count()
        .get_result::<i64>(conn)
        .expect("count")
        > 0
}

fn ticket_exists(conn: &mut backend::db::DbConnection, ticket: i32) -> bool {
    use backend::schema::tickets;
    tickets::table
        .find(ticket)
        .count()
        .get_result::<i64>(conn)
        .expect("count")
        > 0
}

#[actix_web::test]
async fn an_unconfirmed_request_and_its_unused_account_are_removed() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");

    // Never confirmed, never used: both go.
    let abandoned = guest(&mut conn, "abandoned@residue.test");
    let abandoned_request = request(&mut conn, abandoned.uuid, true);
    backdate(&mut conn, abandoned_request, abandoned.uuid);

    // Confirmed (their request reached the team): both stay.
    let kept = guest(&mut conn, "kept@residue.test");
    let kept_request = request(&mut conn, kept.uuid, false);
    backdate(&mut conn, kept_request, kept.uuid);

    // Recent and unconfirmed: still within its window.
    let recent = guest(&mut conn, "recent@residue.test");
    let recent_request = request(&mut conn, recent.uuid, true);
    drop(conn);

    let tmp = tempfile::tempdir().expect("temp search dir");
    let search =
        Arc::new(backend::services::search::SearchService::new(tmp.path(), &pool).expect("search"));
    backend::services::scheduled_jobs::guest_residue_cleanup(pool.clone(), search)
        .await
        .expect("sweep");

    let mut conn = pool.get().expect("conn");
    assert!(
        !ticket_exists(&mut conn, abandoned_request),
        "unconfirmed request removed"
    );
    assert!(
        !user_exists(&mut conn, abandoned.uuid),
        "unused account removed"
    );
    assert!(ticket_exists(&mut conn, kept_request));
    assert!(user_exists(&mut conn, kept.uuid));
    assert!(
        ticket_exists(&mut conn, recent_request),
        "still within its window"
    );
    assert!(user_exists(&mut conn, recent.uuid));
}
