use diesel::prelude::*;
use diesel::result::Error;
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

pub fn update_article_content(conn: &mut DbConnection, ticket_id: i32, article_content: NewArticleContent) -> QueryResult<ArticleContent> {
    // First check if article content exists for this ticket
    let existing = article_contents::table
        .filter(article_contents::ticket_id.eq(ticket_id))
        .first::<ArticleContent>(conn);

    match existing {
        Ok(article) => {
            // Update existing article content
            diesel::update(article_contents::table.find(article.id))
                .set(&article_content)
                .get_result(conn)
        },
        Err(Error::NotFound) => {
            // Create new article content if it doesn't exist
            create_article_content(conn, article_content)
        },
        Err(e) => Err(e)
    }
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