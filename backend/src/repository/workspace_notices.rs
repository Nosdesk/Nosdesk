//! Known-issue notices (`workspace_notices`).
//!
//! Staff post one during an outage; the portal and guest pages show the one
//! that's live. Notices aren't part of the sync feed: the pages read them on
//! load, and staff manage them from one list.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{NoticeFields, WorkspaceNotice};
use crate::schema::workspace_notices;

/// The notice to show now: live (started, not ended), most recently updated.
pub fn active(conn: &mut DbConnection, now: DateTime<Utc>) -> QueryResult<Option<WorkspaceNotice>> {
    workspace_notices::table
        .filter(workspace_notices::starts_at.le(now))
        .filter(workspace_notices::ends_at.gt(now))
        .order(workspace_notices::updated_at.desc())
        .select(WorkspaceNotice::as_select())
        .first(conn)
        .optional()
}

/// Every notice, newest first, for the staff list.
pub fn list(conn: &mut DbConnection) -> QueryResult<Vec<WorkspaceNotice>> {
    workspace_notices::table
        .order(workspace_notices::created_at.desc())
        .limit(100)
        .select(WorkspaceNotice::as_select())
        .load(conn)
}

pub fn find(conn: &mut DbConnection, id: i32) -> QueryResult<Option<WorkspaceNotice>> {
    workspace_notices::table
        .find(id)
        .select(WorkspaceNotice::as_select())
        .first(conn)
        .optional()
}

// sync-audit-only: notices are read on page load, not synced; the audit trigger records writes
pub fn create(
    conn: &mut DbConnection,
    fields: &NoticeFields,
    author: Uuid,
) -> QueryResult<WorkspaceNotice> {
    diesel::insert_into(workspace_notices::table)
        .values((fields, workspace_notices::created_by.eq(author)))
        .returning(WorkspaceNotice::as_returning())
        .get_result(conn)
}

// sync-audit-only: notices are read on page load, not synced; the audit trigger records writes
pub fn update(
    conn: &mut DbConnection,
    id: i32,
    fields: &NoticeFields,
) -> QueryResult<WorkspaceNotice> {
    diesel::update(workspace_notices::table.find(id))
        .set((fields, workspace_notices::updated_at.eq(diesel::dsl::now)))
        .returning(WorkspaceNotice::as_returning())
        .get_result(conn)
}

// sync-audit-only: notices are read on page load, not synced; the audit trigger records writes
pub fn end_now(conn: &mut DbConnection, id: i32) -> QueryResult<WorkspaceNotice> {
    let now = Utc::now();
    let notice = find(conn, id)?.ok_or(diesel::result::Error::NotFound)?;
    // A notice scheduled for later ends before it ever showed.
    let starts_at = notice.starts_at.min(now - chrono::Duration::seconds(1));
    diesel::update(workspace_notices::table.find(id))
        .set((
            workspace_notices::starts_at.eq(starts_at),
            workspace_notices::ends_at.eq(now),
            workspace_notices::updated_at.eq(now),
        ))
        .returning(WorkspaceNotice::as_returning())
        .get_result(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use chrono::Duration;

    fn fields(starts_in: i64, ends_in: i64) -> NoticeFields {
        let now = Utc::now();
        NoticeFields {
            title: "Email is down".into(),
            body: Some("We're on it.".into()),
            severity: "outage".into(),
            starts_at: now + Duration::minutes(starts_in),
            ends_at: now + Duration::minutes(ends_in),
            incident_ticket_id: None,
        }
    }

    #[test]
    fn only_a_started_unended_notice_is_live() {
        let mut conn = setup_test_connection();
        let author = TestFixtures::create_user(&mut conn, "notice_author", "technician");
        assert!(active(&mut conn, Utc::now()).unwrap().is_none());

        let scheduled = create(&mut conn, &fields(60, 120), author.uuid).unwrap();
        assert!(active(&mut conn, Utc::now()).unwrap().is_none(), "not yet");

        let live = create(&mut conn, &fields(-5, 60), author.uuid).unwrap();
        assert_eq!(
            active(&mut conn, Utc::now()).unwrap().map(|n| n.id),
            Some(live.id)
        );

        let ended = end_now(&mut conn, live.id).unwrap();
        assert!(ended.ends_at <= Utc::now());
        assert!(active(&mut conn, Utc::now()).unwrap().is_none(), "ended");

        // Ending one that hasn't started yet keeps its window valid.
        let ended_early = end_now(&mut conn, scheduled.id).unwrap();
        assert!(ended_early.starts_at < ended_early.ends_at);
    }

    #[test]
    fn an_edit_can_clear_the_body_and_incident_ticket() {
        let mut conn = setup_test_connection();
        let author = TestFixtures::create_user(&mut conn, "notice_clear_author", "technician");
        let ticket = TestFixtures::create_ticket(&mut conn, "Incident", Some(author.uuid), None);
        let mut f = fields(-5, 60);
        f.incident_ticket_id = Some(ticket.id);
        let notice = create(&mut conn, &f, author.uuid).unwrap();
        assert_eq!(notice.incident_ticket_id, Some(ticket.id));

        f.body = None;
        f.incident_ticket_id = None;
        let cleared = update(&mut conn, notice.id, &f).unwrap();
        assert_eq!(cleared.body, None);
        assert_eq!(cleared.incident_ticket_id, None);
    }
}
