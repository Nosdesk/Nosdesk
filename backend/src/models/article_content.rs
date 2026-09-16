use super::tickets::Ticket;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::article_contents)]
#[diesel(belongs_to(Ticket))]
pub struct ArticleContent {
    pub id: i32,
    pub ticket_id: Option<i32>,
    pub current_revision_number: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub updated_at: NaiveDateTime,
    pub updated_by: Option<Uuid>,
    // Yjs document state (current version) - snapshot-based persistence
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub workspace_id: i32,
    /// Fencing token from the per-document ownership claim (Phase 2
    /// affinity). The owning machine stamps its claim's monotonic token
    /// on each snapshot write; a conditional write rejects a stale owner
    /// whose token is lower. NULL on rows written in single-instance
    /// mode (no claim).
    pub fence_token: Option<i64>,
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::article_contents)]
pub struct NewArticleContent {
    pub ticket_id: i32,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
}

/// Append-only crash-recovery checkpoint for a collaborative document.
/// Written by the collaboration checkpoint loop between the heavier
/// `article_contents` saves so a hard crash loses seconds, not the whole
/// save interval. `document_id` is the namespaced doc id (the same key
/// used for the Redis cache); `snapshot` is a full Yjs v1 update,
/// `state_vector` its encoded state vector. Workspace-scoped via RLS.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::yjs_snapshots)]
pub struct NewYjsSnapshot<'a> {
    pub workspace_id: i32,
    pub document_id: &'a str,
    pub snapshot: &'a [u8],
    pub state_vector: &'a [u8],
}

// Article Content Revision models for version history
// Simplified: removed redundant yjs_document_snapshot field (DRY principle)
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::article_content_revisions)]
#[diesel(belongs_to(ArticleContent))]
pub struct ArticleContentRevision {
    pub id: i32,
    pub article_content_id: i32,
    pub revision_number: i32,
    pub yjs_state_vector: Vec<u8>,
    pub yjs_document_content: Vec<u8>,
    pub contributed_by: Vec<Option<Uuid>>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::article_content_revisions)]
pub struct NewArticleContentRevision {
    pub article_content_id: i32,
    pub revision_number: i32,
    pub yjs_state_vector: Vec<u8>,
    pub yjs_document_content: Vec<u8>,
    pub contributed_by: Vec<Option<Uuid>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ArticleContentRevisionResponse {
    pub id: i32,
    pub article_content_id: i32,
    pub revision_number: i32,
    pub contributed_by: Vec<Option<Uuid>>,
    pub created_at: NaiveDateTime,
}

impl From<ArticleContentRevision> for ArticleContentRevisionResponse {
    fn from(revision: ArticleContentRevision) -> Self {
        ArticleContentRevisionResponse {
            id: revision.id,
            article_content_id: revision.article_content_id,
            revision_number: revision.revision_number,
            contributed_by: revision.contributed_by,
            created_at: revision.created_at,
        }
    }
}
