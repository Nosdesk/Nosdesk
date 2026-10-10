//! The daily cleanup removes what the public request form leaves when nobody
//! follows through (a request never confirmed, an upload never attached, the
//! account made for it), and nothing that anyone used. It runs the way
//! production runs it: on the `nosdesk_app` pool, where a connection starts
//! with no workspace, so every audited delete has to bring its own.

#![allow(clippy::expect_used)]

use std::sync::Arc;

use diesel::prelude::*;
use diesel::sql_types::{Integer, Text, Uuid as SqlUuid};

use backend::models::NewTicket;
use backend::repository::tickets::{self, TicketCreationAnnotation};
use backend::repository::user_helpers::{find_or_create_guest_user, GuestUserResult};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

fn in_workspace<T>(
    conn: &mut backend::db::DbConnection,
    ws: i32,
    f: impl FnOnce(&mut backend::db::DbConnection) -> QueryResult<T>,
) -> T {
    let actor = ActorContext::system("test:guest_residue_seed").with_workspace(ws);
    with_actor_context::<_, diesel::result::Error>(conn, &actor, f).expect("seed")
}

fn guest(conn: &mut backend::db::DbConnection, ws: i32, email: &str) -> backend::models::User {
    in_workspace(conn, ws, |c| {
        match find_or_create_guest_user(email, "Guest", c, None)? {
            GuestUserResult::Created(u) => Ok(u),
            _ => panic!("expected a new guest"),
        }
    })
}

fn request(
    conn: &mut backend::db::DbConnection,
    ws: i32,
    requester: uuid::Uuid,
    pending: bool,
) -> i32 {
    use backend::schema::workflow_states;
    in_workspace(conn, ws, |c| {
        let state: i32 = workflow_states::table
            .filter(workflow_states::workspace_id.eq(ws))
            .filter(workflow_states::is_default.eq(true))
            .select(workflow_states::id)
            .first(c)?;
        Ok(tickets::create_ticket_with_annotation(
            c,
            NewTicket {
                title: "Printer".into(),
                workflow_state_id: state,
                requester_uuid: Some(requester),
                verification_state: pending.then(|| "pending".to_string()),
                ..Default::default()
            },
            TicketCreationAnnotation::default(),
            None,
        )?
        .id)
    })
}

/// An upload the request form stored but no request ever claimed, a day old.
fn abandoned_upload(conn: &mut backend::db::DbConnection, ws: i32) -> i32 {
    #[derive(QueryableByName)]
    struct Id {
        #[diesel(sql_type = Integer)]
        id: i32,
    }
    in_workspace(conn, ws, |c| {
        Ok(diesel::sql_query(
            "INSERT INTO attachments (url, name, created_at) \
             VALUES ('/uploads/tickets/residue.png', 'residue.png', now() - interval '2 days') \
             RETURNING id",
        )
        .get_result::<Id>(c)?
        .id)
    })
}

/// Age an account (and its request, if any) past the unconfirmed window.
fn backdate(conn: &mut backend::db::DbConnection, ws: i32, ticket: Option<i32>, user: uuid::Uuid) {
    in_workspace(conn, ws, |c| {
        if let Some(ticket) = ticket {
            diesel::sql_query(
                "UPDATE tickets SET created_at = now() - interval '15 days' WHERE id = $1",
            )
            .bind::<Integer, _>(ticket)
            .execute(c)?;
        }
        diesel::sql_query(
            "UPDATE users SET created_at = now() - interval '15 days' WHERE uuid = $1",
        )
        .bind::<SqlUuid, _>(user)
        .execute(c)
    });
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

fn attachment_exists(conn: &mut backend::db::DbConnection, id: i32) -> bool {
    use backend::schema::attachments;
    attachments::table
        .find(id)
        .count()
        .get_result::<i64>(conn)
        .expect("count")
        > 0
}

/// Workspaces of the system-attributed audit rows recording `pk` deleted from
/// `table`. Read on the test superuser connection, which sees every
/// workspace's audit rows.
fn system_delete_audits(conn: &mut backend::db::DbConnection, table: &str, pk: &str) -> Vec<i32> {
    #[derive(QueryableByName)]
    struct Ws {
        #[diesel(sql_type = Integer)]
        workspace_id: i32,
    }
    diesel::sql_query(
        "SELECT workspace_id FROM audit_log \
         WHERE table_name = $1 AND pk_text = $2 AND op = 'D' AND actor_uuid IS NULL",
    )
    .bind::<Text, _>(table)
    .bind::<Text, _>(pk)
    .load::<Ws>(conn)
    .expect("audit rows")
    .into_iter()
    .map(|r| r.workspace_id)
    .collect()
}

#[actix_web::test]
async fn an_unconfirmed_request_and_its_unused_account_are_removed() {
    crate::common::ensure_test_keyring();
    let db = crate::common::TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    // A workspace other than the bootstrap one, so the audit rows prove they
    // land in the account's own workspace.
    let ws = crate::common::seed_two_workspaces(&mut conn).b.workspace_id;

    // Never confirmed, never used: the request, the upload and the account go.
    let abandoned = guest(&mut conn, ws, "abandoned@residue.test");
    let abandoned_request = request(&mut conn, ws, abandoned.uuid, true);
    backdate(&mut conn, ws, Some(abandoned_request), abandoned.uuid);
    let upload = abandoned_upload(&mut conn, ws);

    // Confirmed (their request reached the team): both stay.
    let kept = guest(&mut conn, ws, "kept@residue.test");
    let kept_request = request(&mut conn, ws, kept.uuid, false);
    backdate(&mut conn, ws, Some(kept_request), kept.uuid);

    // Recent and unconfirmed: still within its window.
    let recent = guest(&mut conn, ws, "recent@residue.test");
    let recent_request = request(&mut conn, ws, recent.uuid, true);

    // Aged and unused but in no workspace any more: nowhere to record its
    // removal, so it stays.
    let homeless = guest(&mut conn, ws, "homeless@residue.test");
    backdate(&mut conn, ws, None, homeless.uuid);
    diesel::sql_query("DELETE FROM workspace_members WHERE user_uuid = $1")
        .bind::<SqlUuid, _>(homeless.uuid)
        .execute(&mut conn)
        .expect("drop membership");
    drop(conn);

    let tmp = tempfile::tempdir().expect("temp search dir");
    let search =
        Arc::new(backend::services::search::SearchService::new(tmp.path(), &pool).expect("search"));
    let runtime = db.runtime_pool(2);
    backend::services::scheduled_jobs::guest_residue_sweep(&runtime, Some(&search))
        .await
        .expect("sweep");

    let mut conn = pool.get().expect("conn");
    assert!(
        !ticket_exists(&mut conn, abandoned_request),
        "unconfirmed request removed"
    );
    assert!(
        !attachment_exists(&mut conn, upload),
        "abandoned upload removed"
    );
    assert!(
        !user_exists(&mut conn, abandoned.uuid),
        "unused account removed"
    );
    assert_eq!(
        system_delete_audits(&mut conn, "users", &abandoned.uuid.to_string()),
        vec![ws],
        "the account's removal is audited as the system job, in its workspace"
    );
    assert_eq!(
        system_delete_audits(&mut conn, "tickets", &abandoned_request.to_string()),
        vec![ws],
        "the request's removal is audited as the system job, in its workspace"
    );
    assert!(ticket_exists(&mut conn, kept_request));
    assert!(user_exists(&mut conn, kept.uuid));
    assert!(
        ticket_exists(&mut conn, recent_request),
        "still within its window"
    );
    assert!(user_exists(&mut conn, recent.uuid));
    assert!(user_exists(&mut conn, homeless.uuid));
}
