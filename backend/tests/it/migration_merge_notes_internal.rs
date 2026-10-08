//! Merge notes written before they were internal become internal.
//!
//! A merge note names every ticket merged into its destination, whoever asked
//! for it. Notes written before the change are public, so the destination's
//! requester would see them in the portal and their ticket activity. The
//! migration makes them internal, in `comments` and in the `comment.created`
//! rows the activity feed and sync replay read, and takes the agent's merge
//! reason off `ticket.merged` rows. Runs the migration set against a database
//! seeded just before it.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{Array, Integer, Jsonb, Nullable, Text};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;
use backend::models::{PlatformRole, SyncAggregate, SyncOp, WorkspaceRole};
use backend::repository::ticket_visibility::VisibilityContext;
use backend::sync::visibility::{filter_actions, ActionView, SyncViewer};

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run `sql` the way the rows would already exist: the table's user triggers
/// off. Returns the id it hands back, if any.
fn existing(conn: &mut PgConnection, table: &str, sql: &str) -> Option<i32> {
    diesel::sql_query(format!("ALTER TABLE {table} DISABLE TRIGGER USER"))
        .execute(conn)
        .expect("disable triggers");
    let id = if sql.contains("RETURNING id") {
        Some(
            diesel::sql_query(sql)
                .get_result::<Id>(conn)
                .unwrap_or_else(|e| panic!("seed {table}: {e}"))
                .id,
        )
    } else {
        diesel::sql_query(sql)
            .execute(conn)
            .unwrap_or_else(|e| panic!("seed {table}: {e}"));
        None
    };
    diesel::sql_query(format!("ALTER TABLE {table} ENABLE TRIGGER USER"))
        .execute(conn)
        .expect("enable triggers");
    id
}

struct Seeded {
    ws: i32,
    requester: uuid::Uuid,
    ticket: i32,
    reply: i32,
    marker: i32,
}

/// A ticket another was merged into, as merges used to write it: its
/// requester's reply, a public merge note, and the sync rows for both and for
/// the merge.
fn seed(conn: &mut PgConnection) -> Seeded {
    let ws = existing(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name) VALUES ('merge-notes', 'Merge Notes') RETURNING id",
    )
    .expect("workspace");
    let state = existing(
        conn,
        "workflow_states",
        &format!(
            "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
             VALUES ({ws}, 'Open', 'backlog', 'gray', 0, true) RETURNING id"
        ),
    )
    .expect("state");
    let requester = uuid::Uuid::new_v4();
    let agent = uuid::Uuid::new_v4();
    existing(
        conn,
        "users",
        &format!("INSERT INTO users (uuid, name) VALUES ('{requester}', 'Requester'), ('{agent}', 'Agent')"),
    );
    existing(
        conn,
        "workspace_members",
        &format!(
            "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at) \
             VALUES ({ws}, '{requester}', 'member', now()), ({ws}, '{agent}', 'agent', now())"
        ),
    );
    let ticket = existing(
        conn,
        "tickets",
        &format!(
            "INSERT INTO tickets (workspace_id, title, workflow_state_id, requester_uuid, number) \
             VALUES ({ws}, 'VPN down', {state}, '{requester}', 1) RETURNING id"
        ),
    )
    .expect("ticket");
    let reply = existing(
        conn,
        "comments",
        &format!(
            "INSERT INTO comments (workspace_id, ticket_id, user_uuid, content) \
             VALUES ({ws}, {ticket}, '{requester}', 'Still down') RETURNING id"
        ),
    )
    .expect("reply");
    let marker = existing(
        conn,
        "comments",
        &format!(
            "INSERT INTO comments (workspace_id, ticket_id, user_uuid, content, is_internal, channel_metadata) \
             VALUES ({ws}, {ticket}, '{agent}', 'Merged #2 (Payroll access for Jo) into this ticket.', \
                     false, '{{\"kind\": \"merge_marker\"}}') RETURNING id"
        ),
    )
    .expect("marker");
    let groups = format!("ARRAY['workspace:{ws}', 'ticket:{ticket}']");
    for (comment, extra) in [(reply, ""), (marker, ", 'kind', 'merge_marker'")] {
        existing(
            conn,
            "sync_actions",
            &format!(
                "INSERT INTO sync_actions (workspace_id, aggregate, aggregate_id, op, event_type, data, groups) \
                 VALUES ({ws}, 'comment', '{comment}', 'I', 'comment.created', \
                         jsonb_build_object('id', {comment}, 'ticket_id', {ticket}, 'is_internal', false{extra}), \
                         {groups})"
            ),
        );
    }
    existing(
        conn,
        "sync_actions",
        &format!(
            "INSERT INTO sync_actions (workspace_id, aggregate, aggregate_id, op, event_type, data, groups) \
             VALUES ({ws}, 'ticket', '{ticket}', 'U', 'ticket.merged', \
                     jsonb_build_object('source_ticket_ids', jsonb_build_array(2), \
                                        'reason', 'Jo asked twice', 'comments_moved', 0), \
                     {groups})"
        ),
    );
    Seeded {
        ws,
        requester,
        ticket,
        reply,
        marker,
    }
}

#[derive(QueryableByName)]
struct Row {
    #[diesel(sql_type = Text)]
    aggregate_id: String,
    #[diesel(sql_type = Jsonb)]
    data: serde_json::Value,
    #[diesel(sql_type = Array<Nullable<Text>>)]
    groups: Vec<Option<String>>,
}

#[test]
fn an_earlier_merge_note_is_internal_after_the_migration() {
    let db = FreshDb::new();
    let mut conn = PgConnection::establish(&db.url).expect("connect fresh db");
    diesel::sql_query(
        "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (\
         version VARCHAR(50) PRIMARY KEY NOT NULL, \
         run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&mut conn)
    .expect("create migrations table");

    let mut pending = conn
        .pending_migrations(MIGRATIONS)
        .expect("list pending migrations");
    pending.sort_by_key(|m| m.name().to_string());

    // Seed just before the migration, or after everything when there is none.
    let mut seeded = None;
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_merge_notes_internal") && seeded.is_none() {
            seeded = Some(seed(&mut conn));
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let s = seeded.unwrap_or_else(|| seed(&mut conn));
    drop(conn);

    // Read as the app does, pinned to the workspace.
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .test_on_check_out(false)
        .build(backend::db::ResettingManager::new(&db.url))
        .expect("pool");
    let mut conn = pool.get().expect("conn");
    diesel::sql_query(format!(
        "SELECT set_config('app.workspace_id', '{}', false)",
        s.ws
    ))
    .execute(&mut conn)
    .expect("pin workspace");

    // The portal's comment list.
    let public: Vec<i32> =
        backend::repository::comments::get_public_comments_by_ticket_id(&mut conn, s.ticket)
            .expect("public comments")
            .into_iter()
            .map(|c| c.id)
            .collect();
    assert_eq!(
        public,
        vec![s.reply],
        "the portal shows the reply, not the merge note"
    );

    // The ticket activity feed (and a sync replay): the same keep-mask.
    let rows: Vec<Row> = diesel::sql_query(format!(
        "SELECT aggregate_id, data, groups FROM sync_actions \
         WHERE event_type = 'comment.created' AND data->>'ticket_id' = '{}' ORDER BY sync_id",
        s.ticket
    ))
    .load(&mut conn)
    .expect("comment rows");
    assert_eq!(rows.len(), 2);
    let viewer = SyncViewer {
        ctx: VisibilityContext::new(s.requester, PlatformRole::User, Some(WorkspaceRole::Member)),
        is_admin: false,
    };
    let keep = filter_actions(&mut conn, &viewer, &rows, |r| {
        ActionView::from_row(
            SyncAggregate::Comment,
            SyncOp::Insert,
            &r.aggregate_id,
            &r.data,
            &r.groups,
        )
    });
    let kept: Vec<String> = rows
        .iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(r, _)| r.aggregate_id.clone())
        .collect();
    assert_eq!(
        kept,
        vec![s.reply.to_string()],
        "the requester's activity has the reply, not merge note {}",
        s.marker
    );

    // The merge's sync row no longer carries the agent's reason.
    let merged: Row = diesel::sql_query(format!(
        "SELECT aggregate_id, data, groups FROM sync_actions \
         WHERE event_type = 'ticket.merged' AND aggregate_id = '{}'",
        s.ticket
    ))
    .get_result(&mut conn)
    .expect("ticket.merged row");
    assert_eq!(merged.data.get("reason"), None, "{}", merged.data);
    assert_eq!(merged.data["comments_moved"], 0, "the rest stays");
}
