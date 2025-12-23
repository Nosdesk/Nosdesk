use diesel::prelude::*;
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

// Article content operations
pub fn get_article_content_by_ticket_id(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<ArticleContent> {
    article_contents::table
        .filter(article_contents::ticket_id.eq(ticket_id))
        .first(conn)
}

pub fn create_article_content(conn: &mut DbConnection, new_article_content: NewArticleContent) -> QueryResult<ArticleContent> {
    diesel::insert_into(article_contents::table)
        .values(&new_article_content)
        .get_result(conn)
}

// Increment the revision number for an article content
pub fn increment_article_content_revision(
    conn: &mut DbConnection,
    article_content_id: i32
) -> QueryResult<ArticleContent> {
    diesel::update(article_contents::table.find(article_content_id))
        .set(article_contents::current_revision_number.eq(article_contents::current_revision_number + 1))
        .get_result(conn)
}

// Article content revision operations
pub fn create_article_content_revision(
    conn: &mut DbConnection,
    new_revision: NewArticleContentRevision
) -> QueryResult<ArticleContentRevision> {
    diesel::insert_into(article_content_revisions::table)
        .values(&new_revision)
        .get_result(conn)
}

pub fn get_article_content_revisions(
    conn: &mut DbConnection,
    article_content_id: i32
) -> QueryResult<Vec<ArticleContentRevision>> {
    article_content_revisions::table
        .filter(article_content_revisions::article_content_id.eq(article_content_id))
        .order(article_content_revisions::revision_number.desc())
        .load(conn)
}

pub fn get_article_content_revision(
    conn: &mut DbConnection,
    article_content_id: i32,
    revision_number: i32
) -> QueryResult<ArticleContentRevision> {
    article_content_revisions::table
        .filter(article_content_revisions::article_content_id.eq(article_content_id))
        .filter(article_content_revisions::revision_number.eq(revision_number))
        .first(conn)
}

pub fn get_latest_article_content_revision(
    conn: &mut DbConnection,
    article_content_id: i32
) -> QueryResult<ArticleContentRevision> {
    article_content_revisions::table
        .filter(article_content_revisions::article_content_id.eq(article_content_id))
        .order(article_content_revisions::revision_number.desc())
        .first(conn)
}

// Update Yjs state fields for ticket article content (snapshot-based persistence)
// Note: Does NOT update the parent ticket's updated_at - that should only happen
// when there are actual content changes, not on every sync/save
pub fn update_article_yjs_state(
    conn: &mut DbConnection,
    ticket_id: i32,
    yjs_document: Vec<u8>,
) -> QueryResult<ArticleContent> {
    // First check if article content exists for this ticket
    let existing = article_contents::table
        .filter(article_contents::ticket_id.eq(ticket_id))
        .first::<ArticleContent>(conn);

    match existing {
        Ok(article) => {
            // Update existing article content Yjs state
            diesel::update(article_contents::table.find(article.id))
                .set((
                    article_contents::yjs_document.eq(Some(yjs_document)),
                    article_contents::updated_at.eq(diesel::dsl::now),
                ))
                .get_result(conn)
        },
        Err(diesel::result::Error::NotFound) => {
            // Create new article content with Yjs state
            let new_content = NewArticleContent {
                ticket_id,
                yjs_state_vector: None,
                yjs_document: Some(yjs_document),
                yjs_client_id: None,
            };
            create_article_content(conn, new_content)
        },
        Err(e) => Err(e)
    }
}

// Update parent ticket's updated_at timestamp (call only when content actually changes)
pub fn update_ticket_modified_timestamp(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<usize> {
    diesel::update(tickets::table.find(ticket_id))
        .set(tickets::updated_at.eq(diesel::dsl::now))
        .execute(conn)
} 