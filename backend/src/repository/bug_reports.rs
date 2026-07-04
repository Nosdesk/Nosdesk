//! User-submitted bug reports. One row per submission of the in-app
//! "Report a problem" modal. Writes are workspace-scoped via RLS;
//! the handler establishes the workspace context before calling
//! into this module.

use crate::db::DbConnection;
use crate::models::{BugReport, NewBugReport};
use diesel::prelude::*;
use diesel::result::Error as DieselError;

// sync-audit-only: user-submitted bug report; product feedback log, no sync subscriber
/// Insert a freshly captured bug report. Returns the persisted row
/// so the handler can log the generated id.
pub fn insert(conn: &mut DbConnection, report: NewBugReport) -> Result<BugReport, DieselError> {
    use crate::schema::bug_reports::dsl::*;

    diesel::insert_into(bug_reports)
        .values(&report)
        .get_result::<BugReport>(conn)
}

/// List bug reports across every workspace, newest first, for the
/// platform-admin operator view. Cross-tenant by design, so the caller
/// must supply a BYPASSRLS connection (`PlatformConn`). `limit` is
/// clamped by the handler; `offset` drives simple page-through.
pub fn list_recent(
    conn: &mut DbConnection,
    limit: i64,
    offset: i64,
) -> Result<Vec<BugReport>, DieselError> {
    use crate::schema::bug_reports::dsl::*;

    bug_reports
        .order(received_at.desc())
        .limit(limit)
        .offset(offset)
        .load::<BugReport>(conn)
}
