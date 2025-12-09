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