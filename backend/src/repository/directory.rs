//! A workspace's people, and identity resolution scoped to them.
//!
//! `users` carries no row-level security, deliberately: one account can hold
//! memberships in several workspaces, so the table is global and RLS has no
//! workspace column to key on. The consequence is that a uuid-keyed read of
//! `users` is unscoped. Pinning the request's workspace does not help, because
//! the pin constrains the RLS-bearing tables joined alongside it (the role
//! lookup), not the identity row itself. Any authenticated member could
//! therefore read any user in the deployment by uuid, primary email included.
//!
//! The scope has to come from the workspace's own records. A user is one of a
//! workspace's people when that workspace names them:
//!
//! - in a membership, current or former (`workspace_members`);
//! - in a profile kept for them there, such as a directory-synced contact
//!   (`user_profiles`);
//! - as the requester or assignee of one of its tickets (`tickets`);
//! - as a watcher of one of its tickets (`ticket_watchers`);
//! - as the author of a comment on one of its tickets (`comments`).
//!
//! Members cover staff and most requesters. The other sources hold people the
//! workspace works with whose account began somewhere else: a sender whose
//! address already had an account, or a directory-synced employee with no
//! membership row. [`people`] and [`people_among`] are the one definition.
//! The sync bootstrap, the read-side sync filter, the people lists and the
//! lookups below all use it, so they cannot disagree about who is in a
//! workspace.
//!
//! **Membership status is deliberately ignored here.** These functions resolve
//! *who someone is*, not *what they may do*: a former colleague still has to
//! render as a name on the tickets and comments they left behind. Everything
//! that decides authority reads `workspace_members` with `removed_at IS NULL`
//! instead; see `workspace_members_active_filter_lint`.

use std::collections::HashSet;

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::User;
use crate::schema::users;

/// Every one of the workspace's people (see the module docs).
pub fn people(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<HashSet<Uuid>> {
    named_in_workspace(conn, workspace_id, None)
}

/// Those of `candidates` who are the workspace's people. Each source is
/// probed by its user column, so this stays cheap however large the
/// workspace is.
pub fn people_among(
    conn: &mut DbConnection,
    workspace_id: i32,
    candidates: &[Uuid],
) -> QueryResult<HashSet<Uuid>> {
    if candidates.is_empty() {
        return Ok(HashSet::new());
    }
    named_in_workspace(conn, workspace_id, Some(candidates))
}

/// The one definition behind [`people`] and [`people_among`]: the users each
/// source names in `workspace_id`, optionally only among `among`. Every source
/// is a tenant table read with an explicit workspace predicate, so an elevated
/// connection gets the same answer as a pinned one.
// members-any-status: a former member is still one of the workspace's people;
// the tickets and comments they left behind must keep rendering their name.
fn named_in_workspace(
    conn: &mut DbConnection,
    workspace_id: i32,
    among: Option<&[Uuid]>,
) -> QueryResult<HashSet<Uuid>> {
    use crate::schema::{comments, ticket_watchers, tickets, user_profiles, workspace_members};

    let mut out = HashSet::new();
    // One query per (table, user column). `nullable()` lets the nullable
    // ticket columns and the NOT NULL ones share a shape; it adds nothing to
    // the SQL, so each probe still uses that column's index.
    macro_rules! named_by {
        ($table:ident, $column:ident) => {{
            let mut query = $table::table
                .filter($table::workspace_id.eq(workspace_id))
                .select($table::$column.nullable())
                .distinct()
                .into_boxed();
            if let Some(candidates) = among {
                query = query.filter($table::$column.nullable().eq_any(candidates));
            }
            out.extend(query.load::<Option<Uuid>>(conn)?.into_iter().flatten());
        }};
    }
    named_by!(workspace_members, user_uuid);
    named_by!(user_profiles, user_uuid);
    named_by!(tickets, requester_uuid);
    named_by!(tickets, assignee_uuid);
    named_by!(ticket_watchers, user_uuid);
    named_by!(comments, user_uuid);
    Ok(out)
}

/// The workspace's people as user rows, by name.
pub fn list_people(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<Vec<User>> {
    let uuids: Vec<Uuid> = people(conn, workspace_id)?.into_iter().collect();
    users::table
        .filter(users::uuid.eq_any(uuids))
        .order((users::name.asc(), users::uuid.asc()))
        .load::<User>(conn)
}

/// The identity of one user, if they are one of this workspace's people.
/// `Ok(None)` for a stranger, so a caller cannot tell "no such user" from
/// "not someone you can see".
pub fn find_member(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuid: Uuid,
) -> QueryResult<Option<User>> {
    if !people_among(conn, workspace_id, &[user_uuid])?.contains(&user_uuid) {
        return Ok(None);
    }
    users::table.find(user_uuid).first::<User>(conn).optional()
}

/// [`find_member`] for many uuids at once. Strangers are absent from the
/// result rather than reported, matching the singular form: a batch lookup
/// must not become an oracle for which uuids exist in other workspaces.
pub fn find_members(
    conn: &mut DbConnection,
    workspace_id: i32,
    user_uuids: &[Uuid],
) -> QueryResult<Vec<User>> {
    let known: Vec<Uuid> = people_among(conn, workspace_id, user_uuids)?
        .into_iter()
        .collect();
    if known.is_empty() {
        return Ok(Vec::new());
    }
    users::table
        .filter(users::uuid.eq_any(known))
        .load::<User>(conn)
}
