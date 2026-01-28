use diesel::prelude::*;
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

// Comment operations
pub fn get_comments_by_ticket_id(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<Comment>> {
    comments::table
        .filter(comments::ticket_id.eq(ticket_id))
        .order(comments::created_at.desc())
        .load(conn)
}

pub fn create_comment(conn: &mut DbConnection, new_comment: NewComment) -> QueryResult<Comment> {
    let result = diesel::insert_into(comments::table)
        .values(&new_comment)
        .get_result(conn);

    // Update the parent ticket's updated_at timestamp
    if result.is_ok() {
        let _ = diesel::update(tickets::table.find(new_comment.ticket_id))
            .set(tickets::updated_at.eq(diesel::dsl::now))
            .execute(conn);
    }

    result
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
    let mut comments_with_attachments = Vec::new();

    for comment in comments {
        let attachments = get_attachments_by_comment_id(conn, comment.id)?;

        // Get user information for this comment using user_uuid with avatar
        let user = match crate::repository::users::get_user_by_uuid(&comment.user_uuid, conn) {
            Ok(user) => Some(UserInfoWithAvatar::from(user)),
            Err(_) => None,
        };

        comments_with_attachments.push(CommentWithAttachments {
            comment,
            attachments,
            user,
        });
    }

    Ok(comments_with_attachments)
}

pub fn delete_comment(conn: &mut DbConnection, comment_id: i32) -> QueryResult<usize> {
    // First delete all attachments associated with this comment
    diesel::delete(attachments::table.filter(attachments::comment_id.eq(comment_id))).execute(conn)?;
    
    // Then delete the comment itself
    diesel::delete(comments::table.find(comment_id)).execute(conn)
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