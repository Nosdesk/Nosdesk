use diesel::prelude::*;
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

/// Observer fired after a ticket's article content (the Yjs blob
/// persisted from the collaborative editor) is successfully saved.
/// Mirrors `UserCreatedObserver` in shape: defined at the repo layer
/// so the helper doesn't import any service module, and implemented
/// elsewhere by the search service to keep the search index in sync
/// when the editor auto-saves.
pub trait ArticleContentSavedObserver: Send + Sync {
    fn article_content_saved(&self, ticket: &Ticket, article: &ArticleContent);
}

// Article content operations
pub fn get_article_content_by_ticket_id(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<ArticleContent> {
    article_contents::table
        .filter(article_contents::ticket_id.eq(ticket_id))
        .first(conn)
}

// sync-audit-only: Yjs document persistence (CRDT substrate, not a sync aggregate)
pub fn create_article_content(
    conn: &mut DbConnection,
    new_article_content: NewArticleContent,
) -> QueryResult<ArticleContent> {
    diesel::insert_into(article_contents::table)
        .values(&new_article_content)
        .get_result(conn)
}

// Increment the revision number for an article content
// sync-audit-only: Yjs document persistence (CRDT substrate, not a sync aggregate)
pub fn increment_article_content_revision(
    conn: &mut DbConnection,
    article_content_id: i32,
) -> QueryResult<ArticleContent> {
    diesel::update(article_contents::table.find(article_content_id))
        .set(
            article_contents::current_revision_number
                .eq(article_contents::current_revision_number + 1),
        )
        .get_result(conn)
}

// Article content revision operations
// sync-audit-only: Yjs document persistence (CRDT substrate, not a sync aggregate)
pub fn create_article_content_revision(
    conn: &mut DbConnection,
    new_revision: NewArticleContentRevision,
) -> QueryResult<ArticleContentRevision> {
    diesel::insert_into(article_content_revisions::table)
        .values(&new_revision)
        .get_result(conn)
}

pub fn get_article_content_revisions(
    conn: &mut DbConnection,
    article_content_id: i32,
) -> QueryResult<Vec<ArticleContentRevision>> {
    article_content_revisions::table
        .filter(article_content_revisions::article_content_id.eq(article_content_id))
        .order(article_content_revisions::revision_number.desc())
        .load(conn)
}

pub fn get_article_content_revision(
    conn: &mut DbConnection,
    article_content_id: i32,
    revision_number: i32,
) -> QueryResult<ArticleContentRevision> {
    article_content_revisions::table
        .filter(article_content_revisions::article_content_id.eq(article_content_id))
        .filter(article_content_revisions::revision_number.eq(revision_number))
        .first(conn)
}

pub fn get_latest_article_content_revision(
    conn: &mut DbConnection,
    article_content_id: i32,
) -> QueryResult<ArticleContentRevision> {
    article_content_revisions::table
        .filter(article_content_revisions::article_content_id.eq(article_content_id))
        .order(article_content_revisions::revision_number.desc())
        .first(conn)
}

// Update Yjs state fields for ticket article content (snapshot-based persistence)
// Note: Does NOT update the parent ticket's updated_at - that should only happen
// when there are actual content changes, not on every sync/save
// sync-audit-only: Yjs document persistence (CRDT substrate, not a sync aggregate)
pub fn update_article_yjs_state(
    conn: &mut DbConnection,
    ticket_id: i32,
    yjs_document: Vec<u8>,
    // Ownership-claim fencing token (Phase 2 affinity). `Some(f)` gates
    // the write so a stale owner (lower token) cannot clobber the current
    // owner's state; `None` (single-instance / Redis-down) writes
    // unconditionally.
    fence: Option<i64>,
    observer: Option<&dyn ArticleContentSavedObserver>,
) -> QueryResult<ArticleContent> {
    // First check if article content exists for this ticket
    let existing = article_contents::table
        .filter(article_contents::ticket_id.eq(ticket_id))
        .first::<ArticleContent>(conn);

    let result: ArticleContent = match existing {
        Ok(article) => match fence {
            // Fenced write: skip (leave the row unchanged) when our token
            // is older than what's stored, i.e. a newer owner has taken
            // over. A no-row result is the stale-owner case, not an error.
            Some(f) => {
                let updated = diesel::update(
                    article_contents::table
                        .filter(article_contents::id.eq(article.id))
                        .filter(
                            article_contents::fence_token
                                .is_null()
                                .or(article_contents::fence_token.le(f)),
                        ),
                )
                .set((
                    article_contents::yjs_document.eq(Some(yjs_document)),
                    article_contents::fence_token.eq(f),
                    article_contents::updated_at.eq(diesel::dsl::now),
                ))
                .get_result::<ArticleContent>(conn)
                .optional()?;
                match updated {
                    Some(a) => a,
                    None => {
                        tracing::warn!(
                            ticket_id,
                            fence = f,
                            "Skipped Yjs save: stale ownership fence"
                        );
                        article
                    }
                }
            }
            None => diesel::update(article_contents::table.find(article.id))
                .set((
                    article_contents::yjs_document.eq(Some(yjs_document)),
                    article_contents::updated_at.eq(diesel::dsl::now),
                ))
                .get_result(conn)?,
        },
        Err(diesel::result::Error::NotFound) => {
            // Create new article content with Yjs state
            let new_content = NewArticleContent {
                ticket_id,
                yjs_state_vector: None,
                yjs_document: Some(yjs_document),
                yjs_client_id: None,
            };
            create_article_content(conn, new_content)?
        }
        Err(e) => return Err(e),
    };

    // Notify the observer with both the parent ticket and the saved
    // article so a search-side implementation can call
    // `spawn_index_ticket(ticket, article)` without a follow-up
    // round trip. The ticket fetch is best-effort: if it fails the
    // observer is skipped rather than failing the save itself.
    if let Some(observer) = observer {
        if let Ok(ticket) = tickets::table.find(ticket_id).first::<Ticket>(conn) {
            observer.article_content_saved(&ticket, &result);
        }
    }

    Ok(result)
}

// Update parent ticket's updated_at timestamp (call only when content actually changes)
// sync-audit-only: Yjs document persistence (CRDT substrate, not a sync aggregate)
pub fn update_ticket_modified_timestamp(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<usize> {
    diesel::update(tickets::table.find(ticket_id))
        .set(tickets::updated_at.eq(diesel::dsl::now))
        .execute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn create_and_get_article_content() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "artuser", "admin");
        let ticket = TestFixtures::create_ticket(&mut conn, "Art Ticket", Some(user.uuid), None);

        let new = NewArticleContent {
            ticket_id: ticket.id,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
        };
        let article = create_article_content(&mut conn, new).unwrap();
        assert_eq!(article.ticket_id, Some(ticket.id));

        let fetched = get_article_content_by_ticket_id(&mut conn, ticket.id).unwrap();
        assert_eq!(fetched.id, article.id);
    }

    #[test]
    fn increment_revision() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "revuser", "admin");
        let ticket = TestFixtures::create_ticket(&mut conn, "Rev Ticket", Some(user.uuid), None);

        let new = NewArticleContent {
            ticket_id: ticket.id,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
        };
        let article = create_article_content(&mut conn, new).unwrap();
        let original_rev = article.current_revision_number;

        let updated = increment_article_content_revision(&mut conn, article.id).unwrap();
        assert_eq!(updated.current_revision_number, original_rev + 1);
    }
}
