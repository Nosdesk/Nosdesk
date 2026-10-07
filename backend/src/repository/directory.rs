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
//! - on one of its tickets: requester, assignee, creator, closer, watcher, or
//!   author of a comment;
//! - on its documentation: a page's creator, last editor or verifier, or a
//!   revision's author;
//! - on its assets: a device's user or manager, or a loan's borrower.
//!
//! Members cover staff and most requesters, and everyone who has left since a
//! membership row began to outlive the membership. The other sources hold the
//! rest: people whose account began somewhere else (a sender whose address
//! already had an account, a directory-synced employee with no membership
//! row), and people removed before then, whose row is gone but whose work
//! still names them. A membership row means membership and nothing else, so
//! none is made up for them. `PEOPLE_SOURCES` is the one definition. The sync
//! bootstrap, the read-side sync filter, the people lists and the lookups
//! below all use it, so they cannot disagree about who is in a workspace.
//!
//! The workspace export (`services::workspace_export`) collects its users
//! differently on purpose: every user any exported row names through a user
//! foreign key (about a hundred columns, found by introspection), because an
//! import must satisfy each of those keys. That set contains this one. It is
//! too broad to compute on every request, and most of it (notification
//! recipients, token owners, saved-view authors) is never shown as a person.
//!
//! **Membership status is deliberately ignored here.** These functions resolve
//! *who someone is*, not *what they may do*: a former colleague still has to
//! render as a name on the tickets and comments they left behind. Everything
//! that decides authority reads `workspace_members` with `removed_at IS NULL`
//! instead; see `workspace_members_active_filter_lint`.

use std::collections::HashSet;

use diesel::prelude::*;
use diesel::sql_types::Bool;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::User;
use crate::schema::users;

/// Where a workspace's own records name a person: `(table, user column)`, each
/// table carrying `workspace_id`. The one definition of a workspace's people;
/// [`listed_people`] and [`is_person_sql`] render it, so a list and a lookup
/// can't disagree. Each pair has a `(workspace_id, column)` index (the
/// `people_indexes` migration, or a primary key), so a list reads only the
/// workspace's own index entries and a lookup is one probe per source.
///
/// Membership comes first and covers almost everyone. The rest name people
/// the workspace's records show by name (see the module docs).
// members-any-status: a former member is still one of the workspace's people;
// the tickets and comments they left behind must keep rendering their name.
const PEOPLE_SOURCES: [(&str, &str); 15] = [
    ("workspace_members", "user_uuid"),
    ("user_profiles", "user_uuid"),
    ("tickets", "requester_uuid"),
    ("tickets", "assignee_uuid"),
    ("tickets", "created_by"),
    ("tickets", "closed_by"),
    ("ticket_watchers", "user_uuid"),
    ("comments", "user_uuid"),
    ("documentation_pages", "created_by"),
    ("documentation_pages", "last_edited_by"),
    ("documentation_pages", "verified_by"),
    ("documentation_revisions", "created_by"),
    ("assets", "primary_user_uuid"),
    ("assets", "managed_by_user_uuid"),
    ("asset_loans", "borrower_user_uuid"),
];

/// A filter on `users` keeping the workspace's people, as one subquery the
/// planner runs once per query: for a list (the people pages, the pickers,
/// the bootstrap roster), not a per-row check. `workspace_id` is an `i32`, so
/// interpolating it is injection-safe.
pub fn listed_people(workspace_id: i32) -> diesel::expression::SqlLiteral<Bool> {
    let union = PEOPLE_SOURCES
        .iter()
        .map(|(table, column)| {
            format!(
                "SELECT {table}.{column} FROM {table} \
                 WHERE {table}.workspace_id = {workspace_id} AND {table}.{column} IS NOT NULL"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ");
    diesel::dsl::sql::<Bool>(&format!("users.uuid IN ({union})"))
}

/// Whether the user `uuid_expr` (a SQL expression, such as `users.uuid`) is
/// one of the workspace's people, as one probe per source rather than a scan
/// of the workspace. For checking a few known users.
fn is_person_sql(workspace_id: i32, uuid_expr: &str) -> String {
    let probes = PEOPLE_SOURCES
        .iter()
        .map(|(table, column)| {
            format!(
                "EXISTS (SELECT 1 FROM {table} \
                 WHERE {table}.workspace_id = {workspace_id} AND {table}.{column} = {uuid_expr})"
            )
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("({probes})")
}

/// Every one of the workspace's people (see the module docs).
pub fn people(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<HashSet<Uuid>> {
    Ok(users::table
        .filter(listed_people(workspace_id))
        .select(users::uuid)
        .load::<Uuid>(conn)?
        .into_iter()
        .collect())
}

/// Those of `candidates` who are the workspace's people. Probes each source
/// by its index, so this stays cheap however large the workspace is.
pub fn people_among(
    conn: &mut DbConnection,
    workspace_id: i32,
    candidates: &[Uuid],
) -> QueryResult<HashSet<Uuid>> {
    if candidates.is_empty() {
        return Ok(HashSet::new());
    }
    Ok(users::table
        .filter(users::uuid.eq_any(candidates))
        .filter(diesel::dsl::sql::<Bool>(&is_person_sql(
            workspace_id,
            "users.uuid",
        )))
        .select(users::uuid)
        .load::<Uuid>(conn)?
        .into_iter()
        .collect())
}

/// The workspace's people as user rows, by name.
pub fn list_people(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<Vec<User>> {
    users::table
        .filter(listed_people(workspace_id))
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
