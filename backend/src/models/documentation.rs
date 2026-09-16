use super::tickets::Ticket;
use super::users::UserInfoWithAvatar;
use super::workflow_states::WorkflowStateCategory;
use chrono::NaiveDateTime;
use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::prelude::*;
use diesel::serialize::{self, IsNull, Output, ToSql};
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

// Documentation Status Enum
#[derive(
    Debug,
    Serialize,
    Deserialize,
    Clone,
    Copy,
    PartialEq,
    diesel::deserialize::FromSqlRow,
    diesel::expression::AsExpression,
)]
#[diesel(sql_type = crate::schema::sql_types::DocumentationStatus)]
pub enum DocumentationStatus {
    #[serde(rename = "draft")]
    Draft,
    #[serde(rename = "published")]
    Published,
    #[serde(rename = "archived")]
    Archived,
    #[serde(rename = "deleted")]
    Deleted,
}

impl ToSql<crate::schema::sql_types::DocumentationStatus, Pg> for DocumentationStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            DocumentationStatus::Draft => "draft",
            DocumentationStatus::Published => "published",
            DocumentationStatus::Archived => "archived",
            DocumentationStatus::Deleted => "deleted",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::DocumentationStatus, Pg> for DocumentationStatus {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"draft" => Ok(DocumentationStatus::Draft),
            b"published" => Ok(DocumentationStatus::Published),
            b"archived" => Ok(DocumentationStatus::Archived),
            b"deleted" => Ok(DocumentationStatus::Deleted),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

// Documentation Page
#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::documentation_pages)]
pub struct DocumentationPage {
    pub id: i32,
    pub uuid: Uuid,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: DocumentationStatus,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub created_by: Uuid,
    pub last_edited_by: Uuid,
    pub parent_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: bool,
    pub is_template: bool,
    pub archived_at: Option<chrono::NaiveDateTime>,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub has_unsaved_changes: bool,
    pub deleted_at: Option<chrono::NaiveDateTime>,
    /// User who last marked the page as verified, or None if the
    /// page has never been verified.
    pub verified_by: Option<Uuid>,
    /// Timestamp of the last verification. Combined with
    /// verify_interval_days this drives the staleness banner.
    pub verified_at: Option<chrono::NaiveDateTime>,
    /// Days after verified_at before the page is considered stale.
    /// None means verification doesn't expire (evergreen reference
    /// docs).
    pub verify_interval_days: Option<i32>,
    pub workspace_id: i32,
    /// Fencing token from the per-document ownership claim (Phase 2
    /// affinity); see the note on `ArticleContent::fence_token`.
    pub fence_token: Option<i64>,
}

// Documentation Page with Children
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentationPageWithChildren {
    pub page: DocumentationPage,
    pub children: Vec<DocumentationPage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PageOrder {
    pub page_id: i32,
    pub display_order: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionOrder {
    pub collection_id: i32,
    pub display_order: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::documentation_pages)]
pub struct NewDocumentationPage {
    pub uuid: Uuid,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: DocumentationStatus,
    pub created_by: Uuid,
    pub last_edited_by: Uuid,
    pub parent_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: bool,
    pub is_template: bool,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub has_unsaved_changes: bool,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::documentation_pages)]
pub struct DocumentationPageUpdate {
    pub title: Option<String>,
    pub slug: Option<String>,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: Option<DocumentationStatus>,
    pub last_edited_by: Option<Uuid>,
    pub parent_id: Option<Option<i32>>,
    pub display_order: Option<i32>,
    pub is_public: Option<bool>,
    pub is_template: Option<bool>,
    pub archived_at: Option<Option<chrono::NaiveDateTime>>,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub has_unsaved_changes: Option<bool>,
    pub updated_at: Option<chrono::NaiveDateTime>,
    pub deleted_at: Option<Option<chrono::NaiveDateTime>>,
    pub verified_by: Option<Option<Uuid>>,
    pub verified_at: Option<Option<chrono::NaiveDateTime>>,
    pub verify_interval_days: Option<Option<i32>>,
}

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::documentation_revisions)]
pub struct DocumentationRevision {
    pub id: i32,
    pub page_id: i32,
    pub revision_number: i32,
    pub title: String,
    pub yjs_document_snapshot: Vec<u8>,
    pub yjs_state_vector: Vec<u8>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by: Uuid,
    pub change_summary: Option<String>,
    pub workspace_id: i32,
}

// Response models for API
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentationPageResponse {
    pub id: i32,
    pub uuid: Uuid,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub cover_image: Option<String>,
    pub status: DocumentationStatus,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub created_by: UserInfoWithAvatar,
    pub last_edited_by: UserInfoWithAvatar,
    pub parent_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: bool,
    pub is_template: bool,
    pub archived_at: Option<chrono::NaiveDateTime>,
    pub deleted_at: Option<chrono::NaiveDateTime>,
    pub has_unsaved_changes: bool,
    pub children: Option<Vec<DocumentationPageResponse>>,
    pub content: Option<String>,
    /// Verifier (resolved with avatar). None if the page has never
    /// been verified.
    pub verified_by: Option<UserInfoWithAvatar>,
    pub verified_at: Option<chrono::NaiveDateTime>,
    pub verify_interval_days: Option<i32>,
    /// Computed convenience for the frontend: true when the page
    /// has been verified, has an interval set, and the verification
    /// has expired. Pages with no interval are never stale.
    pub is_stale: bool,
    /// True when any collection containing this page has
    /// `require_verification` set. Gates the "needs verification"
    /// prompt for never-verified pages; false by default so an
    /// unverified page reads as neutral, not unchecked.
    pub requires_verification: bool,
    /// Embedded ticket links, populated when the caller passes
    /// `?embed=tickets`. None means the field wasn't requested
    /// (which is different from "no links" — that's `Some(vec![])`).
    /// Skipped from JSON when None so list responses stay lean.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linked_tickets: Option<Vec<DocumentationPageTicketEmbed>>,
}

/// Slim hydrated ticket-link record returned inline on a page when
/// `?embed=tickets` is requested. Mirrors the standalone
/// PageTicketLinkResponse from the page-tickets endpoint, kept
/// in this module so the type lives next to its consumer.
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentationPageTicketEmbed {
    pub ticket_id: i32,
    pub link_type: String,
    pub created_at: NaiveDateTime,
    pub ticket_title: Option<String>,
    pub ticket_category: Option<WorkflowStateCategory>,
}

// ============================================================================
// Documentation Page <-> Ticket links
// ============================================================================
//
// Many-to-many between docs and tickets. `link_type` distinguishes
// "this doc resolved that ticket" (created from / answers it) from
// "this doc is referenced from that ticket" (relevant context, but
// the doc didn't originate from it).

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Associations, Clone)]
#[diesel(table_name = crate::schema::documentation_page_tickets)]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
#[diesel(belongs_to(Ticket, foreign_key = ticket_id))]
#[diesel(primary_key(page_id, ticket_id))]
pub struct DocumentationPageTicket {
    pub page_id: i32,
    pub ticket_id: i32,
    pub link_type: String,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_page_tickets)]
pub struct NewDocumentationPageTicket {
    pub page_id: i32,
    pub ticket_id: i32,
    pub link_type: String,
    pub created_by: Option<Uuid>,
}

// ============================================================================
// Documentation Page Visibility - Page-level group access control
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::documentation_page_visibility)]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
#[diesel(primary_key(id))]
pub struct DocumentationPageVisibility {
    pub page_id: i32,
    pub group_id: Option<i32>,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub id: i32,
    pub user_uuid: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_page_visibility)]
pub struct NewDocumentationPageVisibility {
    pub page_id: i32,
    pub group_id: Option<i32>,
    pub created_by: Option<Uuid>,
    pub user_uuid: Option<Uuid>,
}

// ============================================================================
// Documentation Page Embeddings - Tracks transclusion relationships
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_page_embeddings)]
pub struct NewDocumentationPageEmbedding {
    pub source_page_id: i32,
    pub target_page_id: i32,
}

// ============================================================================
// Documentation Subscriptions
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::documentation_subscriptions)]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
pub struct DocumentationSubscription {
    pub id: i32,
    pub user_uuid: Uuid,
    pub page_id: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_subscriptions)]
pub struct NewDocumentationSubscription {
    pub user_uuid: Uuid,
    pub page_id: i32,
}

// ============================================================================
// Documentation Starred Pages
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::documentation_starred_pages)]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
pub struct DocumentationStarredPage {
    pub id: i32,
    pub user_uuid: Uuid,
    pub page_id: i32,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_starred_pages)]
pub struct NewDocumentationStarredPage {
    pub user_uuid: Uuid,
    pub page_id: i32,
}

/// Info returned for starred pages (used by sidebar API)
#[derive(Debug, Serialize, Deserialize)]
pub struct StarredPageInfo {
    pub page_id: i32,
    pub title: String,
    pub slug: String,
    pub icon: Option<String>,
    pub starred_at: chrono::DateTime<chrono::Utc>,
}
