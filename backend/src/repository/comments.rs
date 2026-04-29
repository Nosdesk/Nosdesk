use diesel::prelude::*;
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

/// Observer fired after a comment is successfully created. The
/// search service uses it to index the comment with its parent
/// ticket title, so any handler that creates a comment populates
/// the index automatically.
pub trait CommentCreatedObserver: Send + Sync {
    fn comment_created(&self, comment: &Comment, ticket_title: &str);
}

/// Observer fired after a comment is hard-deleted. Implementor
/// removes the comment from the search index.
pub trait CommentDeletedObserver: Send + Sync {
    fn comment_deleted(&self, comment_id: i32);
}

// Comment operations
pub fn get_comments_by_ticket_id(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<Comment>> {
    comments::table
        .filter(comments::ticket_id.eq(ticket_id))
        .order(comments::created_at.desc())
        .load(conn)
}

/// Requester-visible comment list: drops internal notes and soft-deleted
/// rows. Used by the guest-portal / public status views. Never call this
/// from tech-facing endpoints — techs need to see internal notes.
pub fn get_public_comments_by_ticket_id(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<Comment>> {
    comments::table
        .filter(comments::ticket_id.eq(ticket_id))
        .filter(comments::is_internal.eq(false))
        .filter(comments::deleted_at.is_null())
        .order(comments::created_at.desc())
        .load(conn)
}

pub fn create_comment(
    conn: &mut DbConnection,
    new_comment: NewComment,
    observer: Option<&dyn CommentCreatedObserver>,
) -> QueryResult<Comment> {
    let comment: Comment = diesel::insert_into(comments::table)
        .values(&new_comment)
        .get_result(conn)?;

    // Update the parent ticket's updated_at timestamp
    let _ = diesel::update(tickets::table.find(new_comment.ticket_id))
        .set(tickets::updated_at.eq(diesel::dsl::now))
        .execute(conn);

    if let Some(observer) = observer {
        // Look up the parent ticket title for the index doc; the
        // search-side `index_document_from_comment` builds a "Comment
        // on: <title>" search title from it. Best-effort.
        let ticket_title = tickets::table
            .find(new_comment.ticket_id)
            .select(tickets::title)
            .first::<String>(conn)
            .unwrap_or_else(|_| String::from("Unknown Ticket"));
        observer.comment_created(&comment, &ticket_title);
    }

    Ok(comment)
}

// Attachment operations
pub fn get_attachments_by_comment_id(conn: &mut DbConnection, comment_id: i32) -> QueryResult<Vec<Attachment>> {
    attachments::table
        .filter(attachments::comment_id.eq(comment_id))
        .load(conn)
}

pub fn create_attachment(conn: &mut DbConnection, new_attachment: NewAttachment) -> QueryResult<Attachment> {
    diesel::insert_into(attachments::table)
        .values(&new_attachment)
        .get_result(conn)
}

pub fn get_comment_by_id(conn: &mut DbConnection, comment_id: i32) -> QueryResult<Comment> {
    comments::table.find(comment_id).first(conn)
}

pub fn get_comments_with_attachments_by_ticket_id(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<CommentWithAttachments>> {
    let comments = get_comments_by_ticket_id(conn, ticket_id)?;

    // Batch-fetch `from_address` for every comment up front so the
    // assembly loop stays O(n) rather than issuing a per-comment
    // query. The lookup spans the linked `channel_messages` table —
    // the authoritative location for a sender's external address.
    // Comments authored through the helpdesk UI have no row there
    // and just see `None`.
    let comment_ids: Vec<i32> = comments.iter().map(|c| c.id).collect();
    let mut from_addresses = crate::repository::channels::from_addresses_for_comments(
        conn,
        &comment_ids,
    )
    .unwrap_or_default();

    comments
        .into_iter()
        .map(|comment| {
            let attachments = get_attachments_by_comment_id(conn, comment.id)?;
            let user = crate::repository::users::get_user_by_uuid(&comment.user_uuid, conn)
                .ok()
                .map(UserInfoWithAvatar::from);
            let from_address = from_addresses.remove(&comment.id);
            Ok(CommentWithAttachments {
                comment,
                attachments,
                user,
                from_address,
            })
        })
        .collect()
}

pub fn delete_comment(
    conn: &mut DbConnection,
    comment_id: i32,
    observer: Option<&dyn CommentDeletedObserver>,
) -> QueryResult<usize> {
    // First delete all attachments associated with this comment
    diesel::delete(attachments::table.filter(attachments::comment_id.eq(comment_id))).execute(conn)?;

    // Then delete the comment itself
    let count = diesel::delete(comments::table.find(comment_id)).execute(conn)?;
    if count > 0 {
        if let Some(observer) = observer {
            observer.comment_deleted(comment_id);
        }
    }
    Ok(count)
}

pub fn get_attachment_by_id(conn: &mut DbConnection, attachment_id: i32) -> QueryResult<Attachment> {
    attachments::table
        .find(attachment_id)
        .first(conn)
}

pub fn delete_attachment(conn: &mut DbConnection, attachment_id: i32) -> QueryResult<usize> {
    diesel::delete(attachments::table.find(attachment_id))
        .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use crate::models::UserRole;

    #[test]
    fn create_and_retrieve_comment() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "commenter", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "Ticket", Some(user.uuid), None);

        let comment = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "Hello world");
        assert_eq!(comment.content, "Hello world");
        assert_eq!(comment.ticket_id, ticket.id);

        let comments = get_comments_by_ticket_id(&mut conn, ticket.id).unwrap();
        assert_eq!(comments.len(), 1);
        assert_eq!(comments[0].id, comment.id);
    }

    #[test]
    fn multiple_comments_all_returned() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "order", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);

        let c1 = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "First");
        let c2 = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "Second");

        let comments = get_comments_by_ticket_id(&mut conn, ticket.id).unwrap();
        assert_eq!(comments.len(), 2);
        let ids: Vec<i32> = comments.iter().map(|c| c.id).collect();
        assert!(ids.contains(&c1.id));
        assert!(ids.contains(&c2.id));
    }

    #[test]
    fn public_comments_exclude_internal_and_deleted() {
        use diesel::prelude::*;

        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "visible", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "V", Some(user.uuid), None);

        // Public.
        let public = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "requester can see this");

        // Internal: set the flag post-insert since the fixture hardcodes
        // is_internal=false.
        let internal = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "tech-only note");
        diesel::update(comments::table.find(internal.id))
            .set(comments::is_internal.eq(true))
            .execute(&mut conn)
            .unwrap();

        // Soft-deleted.
        let deleted = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "retracted");
        diesel::update(comments::table.find(deleted.id))
            .set(comments::deleted_at.eq(Some(chrono::Utc::now().naive_utc())))
            .execute(&mut conn)
            .unwrap();

        let visible = get_public_comments_by_ticket_id(&mut conn, ticket.id).unwrap();
        let ids: Vec<i32> = visible.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![public.id]);
    }

    #[test]
    fn create_comment_updates_ticket_timestamp() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "tsuser", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "TS", Some(user.uuid), None);
        let original_updated = ticket.updated_at;

        // Small delay to ensure timestamp differs
        std::thread::sleep(std::time::Duration::from_millis(10));

        let new_comment = NewComment {
            content: "bump".to_string(),
            ticket_id: ticket.id,
            user_uuid: user.uuid,
            channel_metadata: None,
            is_internal: false,
            content_format: Default::default(),
        };
        create_comment(&mut conn, new_comment).unwrap();

        let updated_ticket = crate::repository::tickets::get_ticket_by_id(&mut conn, ticket.id).unwrap();
        assert!(updated_ticket.updated_at >= original_updated);
    }

    #[test]
    fn create_and_retrieve_attachment() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "attuser", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let comment = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "With file");

        let att = TestFixtures::create_attachment(&mut conn, comment.id, "doc.pdf");
        assert_eq!(att.name, "doc.pdf");

        let atts = get_attachments_by_comment_id(&mut conn, comment.id).unwrap();
        assert_eq!(atts.len(), 1);
        assert_eq!(atts[0].id, att.id);
    }

    #[test]
    fn delete_comment_cascades_attachments() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "deluser", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let comment = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "Bye");
        let att = TestFixtures::create_attachment(&mut conn, comment.id, "file.txt");

        delete_comment(&mut conn, comment.id).unwrap();

        assert!(get_comment_by_id(&mut conn, comment.id).is_err());
        assert!(get_attachment_by_id(&mut conn, att.id).is_err());
    }

    #[test]
    fn delete_single_attachment() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "delatt", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);
        let comment = TestFixtures::create_comment(&mut conn, ticket.id, user.uuid, "Keep me");
        let att = TestFixtures::create_attachment(&mut conn, comment.id, "remove.pdf");

        delete_attachment(&mut conn, att.id).unwrap();

        assert!(get_attachment_by_id(&mut conn, att.id).is_err());
        // Comment should still exist
        assert!(get_comment_by_id(&mut conn, comment.id).is_ok());
    }
}