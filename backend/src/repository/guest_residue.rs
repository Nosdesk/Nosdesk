//! What the public request form leaves behind when nobody follows through:
//! requests never confirmed, uploads never attached, and accounts made for an
//! address that never confirmed anything. Found cross-workspace (run these
//! under the bypass context) and removed by the daily cleanup job.

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer, Text, Timestamptz};
use uuid::Uuid;

use crate::db::DbConnection;

/// Days a request may wait for its email to be confirmed, and a never-used
/// guest account may stay.
pub const UNCONFIRMED_DAYS: i64 = 14;

#[derive(QueryableByName)]
struct WsId {
    #[diesel(sql_type = Integer)]
    workspace_id: i32,
    #[diesel(sql_type = Integer)]
    id: i32,
}

#[derive(QueryableByName)]
struct Upload {
    #[diesel(sql_type = Integer)]
    workspace_id: i32,
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    url: String,
}

#[derive(QueryableByName)]
struct Account {
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    uuid: Uuid,
}

/// `(workspace_id, ticket_id)` of requests still waiting for confirmation
/// since before `cutoff`.
// sync-audit-only: a read-only scan (raw SELECT), no write
pub fn stale_pending_tickets(
    conn: &mut DbConnection,
    cutoff: chrono::DateTime<chrono::Utc>,
    limit: i64,
) -> QueryResult<Vec<(i32, i32)>> {
    let rows: Vec<WsId> = diesel::sql_query(
        "SELECT workspace_id, id FROM tickets \
         WHERE verification_state = 'pending' AND created_at < $1 \
         ORDER BY id LIMIT $2",
    )
    .bind::<Timestamptz, _>(cutoff)
    .bind::<BigInt, _>(limit)
    .load(conn)?;
    Ok(rows.into_iter().map(|r| (r.workspace_id, r.id)).collect())
}

/// `(workspace_id, attachment_id, url)` of request-form uploads never
/// attached to a request, older than `cutoff`.
// sync-audit-only: a read-only scan (raw SELECT), no write
pub fn orphan_guest_uploads(
    conn: &mut DbConnection,
    cutoff: chrono::DateTime<chrono::Utc>,
    limit: i64,
) -> QueryResult<Vec<(i32, i32, String)>> {
    let rows: Vec<Upload> = diesel::sql_query(
        "SELECT workspace_id, id, url FROM attachments \
         WHERE comment_id IS NULL AND uploaded_by IS NULL AND created_at < $1 \
         ORDER BY id LIMIT $2",
    )
    .bind::<Timestamptz, _>(cutoff)
    .bind::<BigInt, _>(limit)
    .load(conn)?;
    Ok(rows
        .into_iter()
        .map(|r| (r.workspace_id, r.id, r.url))
        .collect())
}

/// Accounts the request form made for an address that never confirmed and
/// that were never used for anything: only unconfirmed request-form email,
/// no requests, comments, sign-in identities, followed requests or roles
/// above requester anywhere, created before `cutoff`.
// members-any-status: anyone who ever held a staff seat, even a removed one, is never an unused guest
// sync-audit-only: a read-only scan (raw SELECT), no write
pub fn never_confirmed_guests(
    conn: &mut DbConnection,
    cutoff: chrono::DateTime<chrono::Utc>,
    limit: i64,
) -> QueryResult<Vec<Uuid>> {
    let rows: Vec<Account> = diesel::sql_query(
        "SELECT u.uuid FROM users u \
         WHERE u.deleted_at IS NULL AND u.created_at < $1 AND u.platform_role = 'user' \
           AND EXISTS (SELECT 1 FROM user_emails e WHERE e.user_uuid = u.uuid AND e.source = $2) \
           AND NOT EXISTS (SELECT 1 FROM user_emails e WHERE e.user_uuid = u.uuid \
                           AND (e.is_verified OR e.source IS DISTINCT FROM $2)) \
           AND NOT EXISTS (SELECT 1 FROM tickets t WHERE t.requester_uuid = u.uuid) \
           AND NOT EXISTS (SELECT 1 FROM comments c WHERE c.user_uuid = u.uuid) \
           AND NOT EXISTS (SELECT 1 FROM user_auth_identities i WHERE i.user_uuid = u.uuid) \
           AND NOT EXISTS (SELECT 1 FROM ticket_watchers w WHERE w.user_uuid = u.uuid) \
           AND NOT EXISTS (SELECT 1 FROM workspace_members m WHERE m.user_uuid = u.uuid \
                           AND m.role <> 'member') \
         ORDER BY u.created_at LIMIT $3",
    )
    .bind::<Timestamptz, _>(cutoff)
    .bind::<Text, _>(crate::repository::user_helpers::GUEST_EMAIL_SOURCE)
    .bind::<BigInt, _>(limit)
    .load(conn)?;
    Ok(rows.into_iter().map(|r| r.uuid).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn only_untouched_unconfirmed_guests_are_found() {
        let mut conn = setup_test_connection();
        let future = chrono::Utc::now() + chrono::Duration::days(1);
        let guest = match crate::repository::user_helpers::find_or_create_guest_user(
            "residue.guest@example.test",
            "Guest",
            &mut conn,
            None,
        )
        .unwrap()
        {
            crate::repository::user_helpers::GuestUserResult::Created(u) => u,
            _ => panic!("expected a new guest"),
        };
        let found = never_confirmed_guests(&mut conn, future, 1000).unwrap();
        assert!(found.contains(&guest.uuid));
        // A request of theirs keeps them.
        TestFixtures::create_ticket(&mut conn, "Theirs", Some(guest.uuid), None);
        assert!(!never_confirmed_guests(&mut conn, future, 1000)
            .unwrap()
            .contains(&guest.uuid));
        // A real member is never touched.
        let member = TestFixtures::create_user(&mut conn, "residue_member", "user");
        assert!(!never_confirmed_guests(&mut conn, future, 1000)
            .unwrap()
            .contains(&member.uuid));
    }

    #[test]
    fn stale_pending_requests_are_found_and_live_ones_are_not() {
        let mut conn = setup_test_connection();
        let t = TestFixtures::create_ticket(&mut conn, "Pending", None, None);
        diesel::update(crate::schema::tickets::table.find(t.id))
            .set(crate::schema::tickets::verification_state.eq(Some("pending")))
            .execute(&mut conn)
            .unwrap();
        let future = chrono::Utc::now() + chrono::Duration::days(1);
        assert!(stale_pending_tickets(&mut conn, future, 1000)
            .unwrap()
            .iter()
            .any(|(_, id)| *id == t.id));
        let past = chrono::Utc::now() - chrono::Duration::days(1);
        assert!(!stale_pending_tickets(&mut conn, past, 1000)
            .unwrap()
            .iter()
            .any(|(_, id)| *id == t.id));
    }
}
