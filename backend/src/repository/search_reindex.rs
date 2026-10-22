//! Reads behind a per-workspace search reindex
//! (`SearchService::reindex_workspace`).
//!
//! Each content loader returns one workspace's rows a page at a time, in id
//! order after `after`, so the reindex can index a large workspace in short
//! transactions. They filter by `workspace_id` explicitly as well as running
//! pinned to it, so an elevated caller still gets one workspace's rows. The
//! set matches what a full rebuild indexes (`search::indexer::rebuild_index`).

use std::collections::HashMap;

use diesel::prelude::*;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models;
use crate::schema::{
    article_contents, assets, attachments, comments, documentation_pages, projects, tickets,
    user_emails, users, workspace_members, workspaces,
};

/// A page of the workspace's tickets.
pub fn tickets_after(
    conn: &mut DbConnection,
    workspace_id: i32,
    after: i32,
    limit: i64,
) -> QueryResult<Vec<models::Ticket>> {
    tickets::table
        .filter(tickets::workspace_id.eq(workspace_id))
        .filter(tickets::id.gt(after))
        .order(tickets::id.asc())
        .limit(limit)
        .load(conn)
}

/// The article bodies of `ticket_ids`.
pub fn article_contents_for(
    conn: &mut DbConnection,
    workspace_id: i32,
    ticket_ids: &[i32],
) -> QueryResult<Vec<models::ArticleContent>> {
    article_contents::table
        .filter(article_contents::workspace_id.eq(workspace_id))
        .filter(article_contents::ticket_id.eq_any(ticket_ids))
        .load(conn)
}

/// The titles of `ticket_ids`, by id.
pub fn ticket_titles(
    conn: &mut DbConnection,
    workspace_id: i32,
    ticket_ids: &[i32],
) -> QueryResult<HashMap<i32, String>> {
    let rows: Vec<(i32, String)> = tickets::table
        .filter(tickets::workspace_id.eq(workspace_id))
        .filter(tickets::id.eq_any(ticket_ids))
        .select((tickets::id, tickets::title))
        .load(conn)?;
    Ok(rows.into_iter().collect())
}

/// A page of the workspace's comments.
pub fn comments_after(
    conn: &mut DbConnection,
    workspace_id: i32,
    after: i32,
    limit: i64,
) -> QueryResult<Vec<models::Comment>> {
    comments::table
        .filter(comments::workspace_id.eq(workspace_id))
        .filter(comments::id.gt(after))
        .order(comments::id.asc())
        .limit(limit)
        .load(conn)
}

/// A page of the workspace's transcribed attachments on comments, each with
/// its comment's ticket id and title.
pub fn transcribed_attachments_after(
    conn: &mut DbConnection,
    workspace_id: i32,
    after: i32,
    limit: i64,
) -> QueryResult<Vec<(models::Attachment, i32, String)>> {
    attachments::table
        .inner_join(comments::table.on(comments::id.nullable().eq(attachments::comment_id)))
        .inner_join(tickets::table.on(tickets::id.eq(comments::ticket_id)))
        .filter(attachments::workspace_id.eq(workspace_id))
        .filter(attachments::transcription.is_not_null())
        .filter(attachments::id.gt(after))
        .order(attachments::id.asc())
        .limit(limit)
        .select((
            attachments::all_columns,
            comments::ticket_id,
            tickets::title,
        ))
        .load(conn)
}

/// A page of the workspace's documentation pages, leaving out those in
/// Trash: search never finds them, so a reindex prunes them.
pub fn documentation_pages_after(
    conn: &mut DbConnection,
    workspace_id: i32,
    after: i32,
    limit: i64,
) -> QueryResult<Vec<models::DocumentationPage>> {
    documentation_pages::table
        .filter(documentation_pages::workspace_id.eq(workspace_id))
        .filter(documentation_pages::status.ne(models::DocumentationStatus::Deleted))
        .filter(documentation_pages::id.gt(after))
        .order(documentation_pages::id.asc())
        .limit(limit)
        .load(conn)
}

/// A page of the workspace's assets.
pub fn assets_after(
    conn: &mut DbConnection,
    workspace_id: i32,
    after: i32,
    limit: i64,
) -> QueryResult<Vec<models::Asset>> {
    assets::table
        .filter(assets::workspace_id.eq(workspace_id))
        .filter(assets::id.gt(after))
        .order(assets::id.asc())
        .limit(limit)
        .load(conn)
}

/// A page of the workspace's projects.
pub fn projects_after(
    conn: &mut DbConnection,
    workspace_id: i32,
    after: i32,
    limit: i64,
) -> QueryResult<Vec<models::Project>> {
    projects::table
        .filter(projects::workspace_id.eq(workspace_id))
        .filter(projects::id.gt(after))
        .order(projects::id.asc())
        .limit(limit)
        .load(conn)
}

/// The workspace's active members.
pub fn active_member_uuids(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<Vec<Uuid>> {
    workspace_members::table
        .filter(workspace_members::workspace_id.eq(workspace_id))
        .filter(workspace_members::removed_at.is_null())
        .select(workspace_members::user_uuid)
        .load(conn)
}

/// A person's search document inputs: the user, their primary email and every
/// workspace they are an active member of.
pub type IndexedUser = (models::User, Option<String>, Vec<i32>);

/// The search document inputs for each of `uuids` that names a user who is
/// not deleted. Memberships span workspaces, so the caller reads elevated.
/// Removed memberships and archived workspaces are left out, as
/// `workspaces::list_memberships_for_user` leaves them out for one user.
pub fn users_for_index(conn: &mut DbConnection, uuids: &[Uuid]) -> QueryResult<Vec<IndexedUser>> {
    let found: Vec<models::User> = users::table
        .filter(users::uuid.eq_any(uuids))
        .filter(users::deleted_at.is_null())
        .load(conn)?;
    let found_uuids: Vec<Uuid> = found.iter().map(|u| u.uuid).collect();

    let emails: HashMap<Uuid, String> = user_emails::table
        .filter(user_emails::user_uuid.eq_any(&found_uuids))
        .filter(user_emails::is_primary.eq(true))
        .select((user_emails::user_uuid, user_emails::email))
        .load::<(Uuid, String)>(conn)?
        .into_iter()
        .collect();

    let mut memberships: HashMap<Uuid, Vec<i32>> = HashMap::new();
    let rows: Vec<(Uuid, i32)> = workspace_members::table
        .inner_join(workspaces::table.on(workspaces::id.eq(workspace_members::workspace_id)))
        .filter(workspace_members::user_uuid.eq_any(&found_uuids))
        .filter(workspace_members::removed_at.is_null())
        .filter(workspaces::archived_at.is_null())
        .order(workspaces::id.asc())
        .select((
            workspace_members::user_uuid,
            workspace_members::workspace_id,
        ))
        .load(conn)?;
    for (user_uuid, workspace_id) in rows {
        memberships.entry(user_uuid).or_default().push(workspace_id);
    }

    Ok(found
        .into_iter()
        .map(|user| {
            let email = emails.get(&user.uuid).cloned();
            let workspace_ids = memberships.remove(&user.uuid).unwrap_or_default();
            (user, email, workspace_ids)
        })
        .collect())
}

/// The channel a process that can't open the search index asks the servers
/// that hold it to reindex a workspace on (`services::search::reindex_requests`).
pub const REQUEST_CHANNEL: &str = "nosdesk_search_reindex";

// sync-audit-only: a wake-up notification, not a row; the import it follows is audited
/// Ask every listening server to rebuild workspace `workspace_id`'s search
/// documents. Delivered when the caller's transaction commits.
pub fn request(conn: &mut DbConnection, workspace_id: i32) -> QueryResult<()> {
    diesel::sql_query("SELECT pg_notify($1, $2)")
        .bind::<diesel::sql_types::Text, _>(REQUEST_CHANNEL)
        .bind::<diesel::sql_types::Text, _>(workspace_id.to_string())
        .execute(conn)
        .map(|_| ())
}
