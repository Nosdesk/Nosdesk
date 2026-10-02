use crate::db::DbConnection;
use crate::models::{NewUserTicketView, RecentTicket, UpdateUserTicketView, UserTicketView};
use chrono::Utc;
use diesel::prelude::*;
use uuid::Uuid;

/// How many recent-tickets rows the sidebar / dashboard widget
/// fetch in one go. Surfaced as a constant so the handler and
/// any future consumer agree without a magic number drifting
/// between them.
pub const RECENT_TICKETS_LIMIT: i64 = 15;

// These run on a caller-supplied connection (via TenantConn) rather
// than a private pool wrapper. `user_ticket_views` is a tenant table:
// it carries `workspace_id` (RLS) and an audit_log trigger, both of
// which need `app.workspace_id` set on the transaction. The old
// raw-pool path had no workspace context, so writes failed the audit
// trigger's NOT NULL constraint and reads were fail-closed by RLS.

// sync-audit-only: operational view-tracking table; the per-row diff is captured by the audit_log trigger, no tier-1 event
/// Record a ticket view: insert a new row or bump the existing one.
pub fn record_view(
    conn: &mut DbConnection,
    user_uuid_param: Uuid,
    ticket_id_param: i32,
) -> Result<UserTicketView, diesel::result::Error> {
    use crate::schema::user_ticket_views::dsl::*;

    let existing = user_ticket_views
        .filter(user_uuid.eq(user_uuid_param))
        .filter(ticket_id.eq(ticket_id_param))
        .first::<UserTicketView>(conn)
        .optional()?;

    if let Some(view) = existing {
        let update = UpdateUserTicketView {
            last_viewed_at: Utc::now().naive_utc(),
            view_count: view.view_count + 1,
        };
        diesel::update(user_ticket_views.find(view.id))
            .set(&update)
            .get_result(conn)
    } else {
        let new_view = NewUserTicketView {
            user_uuid: user_uuid_param,
            ticket_id: ticket_id_param,
        };
        diesel::insert_into(user_ticket_views)
            .values(&new_view)
            .get_result(conn)
    }
}

// sync-audit-only: operational view-tracking table; the per-row diff is captured by the audit_log trigger, no tier-1 event
/// Delete a ticket view record for a user.
pub fn delete_view(
    conn: &mut DbConnection,
    user_uuid_param: Uuid,
    ticket_id_param: i32,
) -> Result<usize, diesel::result::Error> {
    use crate::schema::user_ticket_views::dsl::*;

    diesel::delete(
        user_ticket_views
            .filter(user_uuid.eq(user_uuid_param))
            .filter(ticket_id.eq(ticket_id_param)),
    )
    .execute(conn)
}

/// Get recent tickets for a user.
///
/// Selects `workflow_state_id` directly; the client resolves it to a
/// category / colour via the workspace workflow-states store, so this
/// query neither joins `workflow_states` nor touches the category
/// cache in its mapping loop.
pub fn get_recent_tickets(
    conn: &mut DbConnection,
    user_uuid_param: Uuid,
    limit: i64,
) -> Result<Vec<RecentTicket>, diesel::result::Error> {
    use crate::schema::{tickets, user_ticket_views};

    let rows: Vec<(
        i32,
        i32,
        String,
        i32,
        Option<Uuid>,
        Option<Uuid>,
        chrono::NaiveDateTime,
        chrono::NaiveDateTime,
        chrono::NaiveDateTime,
        i32,
    )> = user_ticket_views::table
        .inner_join(tickets::table.on(user_ticket_views::ticket_id.eq(tickets::id)))
        .filter(user_ticket_views::user_uuid.eq(user_uuid_param))
        .order(user_ticket_views::last_viewed_at.desc())
        .limit(limit)
        .select((
            tickets::id,
            tickets::number,
            tickets::title,
            tickets::workflow_state_id,
            tickets::requester_uuid,
            tickets::assignee_uuid,
            tickets::created_at,
            tickets::updated_at,
            user_ticket_views::last_viewed_at,
            user_ticket_views::view_count,
        ))
        .load(conn)?;

    Ok(rows
        .into_iter()
        .map(
            |(tid, number, ttitle, ws_id, req, ass, created, updated, last_viewed, views)| {
                RecentTicket {
                    id: tid,
                    number,
                    title: ttitle,
                    workflow_state_id: ws_id,
                    requester: req,
                    assignee: ass,
                    created_at: created,
                    updated_at: updated,
                    last_viewed_at: last_viewed,
                    view_count: views,
                }
            },
        )
        .collect())
}

/// For each of `ticket_ids`: when someone other than `viewer` last replied
/// publicly, and whether that's newer than the viewer's last look at the
/// ticket (a reply they haven't seen). Tickets nobody else has replied on are
/// absent.
pub fn replies_for_viewer(
    conn: &mut DbConnection,
    viewer: Uuid,
    ticket_ids: &[i32],
) -> QueryResult<std::collections::HashMap<i32, (chrono::DateTime<Utc>, bool)>> {
    use crate::schema::{comments, user_ticket_views as v};
    use diesel::dsl::max;

    if ticket_ids.is_empty() {
        return Ok(Default::default());
    }
    let replies: Vec<(i32, Option<chrono::DateTime<Utc>>)> = comments::table
        .filter(comments::ticket_id.eq_any(ticket_ids))
        .filter(comments::is_internal.eq(false))
        .filter(comments::deleted_at.is_null())
        .filter(comments::user_uuid.ne(viewer))
        .group_by(comments::ticket_id)
        .select((comments::ticket_id, max(comments::created_at)))
        .load(conn)?;
    let seen: std::collections::HashMap<i32, chrono::DateTime<Utc>> = v::table
        .filter(v::user_uuid.eq(viewer))
        .filter(v::ticket_id.eq_any(ticket_ids))
        .select((v::ticket_id, v::last_viewed_at))
        .load::<(i32, chrono::DateTime<Utc>)>(conn)?
        .into_iter()
        .collect();
    Ok(replies
        .into_iter()
        .filter_map(|(ticket, at)| {
            let at = at?;
            let unread = seen.get(&ticket).is_none_or(|looked| *looked < at);
            Some((ticket, (at, unread)))
        })
        .collect())
}

#[cfg(test)]
mod replies_tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn a_reply_from_someone_else_is_unread_until_the_viewer_looks() {
        let mut conn = setup_test_connection();
        let me = TestFixtures::create_user(&mut conn, "replies_me", "user");
        let agent = TestFixtures::create_user(&mut conn, "replies_agent", "technician");
        let ticket = TestFixtures::create_ticket(&mut conn, "Replies", Some(me.uuid), None);
        let quiet = TestFixtures::create_ticket(&mut conn, "Quiet", Some(me.uuid), None);
        TestFixtures::create_comment(&mut conn, ticket.id, me.uuid, "mine");
        TestFixtures::create_comment(&mut conn, quiet.id, me.uuid, "only mine");

        let reply = TestFixtures::create_comment(&mut conn, ticket.id, agent.uuid, "theirs");
        let map = replies_for_viewer(&mut conn, me.uuid, &[ticket.id, quiet.id]).unwrap();
        assert!(
            !map.contains_key(&quiet.id),
            "my own comments aren't replies"
        );
        assert!(map[&ticket.id].1, "not looked at yet");

        record_view(&mut conn, me.uuid, ticket.id).unwrap();
        assert!(!replies_for_viewer(&mut conn, me.uuid, &[ticket.id]).unwrap()[&ticket.id].1);

        // A later reply is new again.
        diesel::update(crate::schema::comments::table.find(reply.id))
            .set(crate::schema::comments::created_at.eq(Utc::now() + chrono::Duration::minutes(5)))
            .execute(&mut conn)
            .unwrap();
        assert!(replies_for_viewer(&mut conn, me.uuid, &[ticket.id]).unwrap()[&ticket.id].1);
    }
}
