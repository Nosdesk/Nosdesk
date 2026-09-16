use super::documentation::DocumentationPage;
use super::groups::Group;
use super::users::UserInfoWithAvatar;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Documentation Collections
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::documentation_collections)]
pub struct DocumentationCollection {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub slug: String,
    /// Short tagline shown above the rich description editor.
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub is_system: bool,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub display_order: i32,
    /// Yjs binary state for the collection's rich description.
    /// Replaces the old root_page_id pattern: the collection owns
    /// its overview content directly instead of pointing at a
    /// special "main page".
    pub description_yjs: Option<Vec<u8>>,
    pub description_state_vector: Option<Vec<u8>>,
    /// Plain-text projection of `description_yjs` for search.
    pub description_text: Option<String>,
    /// When true, cross-collection wikilinks render as
    /// "Restricted page" for viewers without read access, instead
    /// of leaking the page title.
    pub hide_titles_from_non_members: bool,
    pub workspace_id: i32,
    /// Fencing token from the per-document ownership claim (Phase 2
    /// affinity); see the note on `ArticleContent::fence_token`.
    pub fence_token: Option<i64>,
    /// When true, pages in this collection that have never been
    /// verified surface a "needs verification" prompt. Off by
    /// default: an unverified page is neutral, not unchecked.
    pub require_verification: bool,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_collections)]
pub struct NewDocumentationCollection {
    pub uuid: Uuid,
    pub name: String,
    pub slug: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub is_system: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::documentation_collections)]
pub struct DocumentationCollectionUpdate {
    pub name: Option<String>,
    pub slug: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
    pub hide_titles_from_non_members: Option<bool>,
    pub require_verification: Option<bool>,
    pub description_text: Option<Option<String>>,
}

/// Yjs blob update issued by the collaboration handler when a
/// collection's description editor saves. Kept separate from
/// `DocumentationCollectionUpdate` so the metadata-edit surface
/// can't accidentally clobber the binary Yjs state.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::documentation_collections)]
pub struct DocumentationCollectionDescriptionYjsUpdate {
    pub description_yjs: Option<Vec<u8>>,
    pub description_state_vector: Option<Vec<u8>>,
    pub updated_at: Option<NaiveDateTime>,
}

// Collection with visibility and page count
#[derive(Debug, Serialize, Deserialize)]
pub struct CollectionWithDetails {
    #[serde(flatten)]
    pub collection: DocumentationCollection,
    pub visible_to_groups: Vec<Group>,
    pub visible_to_users: Vec<UserInfoWithAvatar>,
    pub is_public: bool,
    pub page_count: i64,
}

// Collection-Page junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::documentation_collection_pages)]
#[diesel(belongs_to(DocumentationCollection, foreign_key = collection_id))]
#[diesel(belongs_to(DocumentationPage, foreign_key = page_id))]
#[diesel(primary_key(collection_id, page_id))]
pub struct DocumentationCollectionPage {
    pub collection_id: i32,
    pub page_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_collection_pages)]
pub struct NewDocumentationCollectionPage {
    pub collection_id: i32,
    pub page_id: i32,
    pub created_by: Option<Uuid>,
}

// Collection-Group visibility junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::documentation_collection_visibility)]
#[diesel(belongs_to(DocumentationCollection, foreign_key = collection_id))]
#[diesel(primary_key(id))]
pub struct DocumentationCollectionVisibility {
    pub collection_id: i32,
    pub group_id: Option<i32>,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub id: i32,
    pub user_uuid: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_collection_visibility)]
pub struct NewDocumentationCollectionVisibility {
    pub collection_id: i32,
    pub group_id: Option<i32>,
    pub created_by: Option<Uuid>,
    pub user_uuid: Option<Uuid>,
}
