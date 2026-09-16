use super::groups::Group;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Ticket Categories - Category Management
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::ticket_categories)]
pub struct TicketCategory {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub display_order: i32,
    pub is_active: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::ticket_categories)]
pub struct NewTicketCategory {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub display_order: i32,
    pub is_active: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::ticket_categories)]
pub struct TicketCategoryUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub icon: Option<String>,
    pub display_order: Option<i32>,
    pub is_active: Option<bool>,
    pub updated_at: Option<NaiveDateTime>,
}

// Category with visibility information for admin views
#[derive(Debug, Serialize, Deserialize)]
pub struct CategoryWithVisibility {
    #[serde(flatten)]
    pub category: TicketCategory,
    pub visible_to_groups: Vec<Group>,
    pub is_public: bool, // true if no group restrictions (visible to all)
}

// Category-Group visibility junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::category_group_visibility)]
#[diesel(belongs_to(TicketCategory, foreign_key = category_id))]
#[diesel(belongs_to(Group))]
#[diesel(primary_key(category_id, group_id))]
pub struct CategoryGroupVisibility {
    pub category_id: i32,
    pub group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::category_group_visibility)]
pub struct NewCategoryGroupVisibility {
    pub category_id: i32,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
}
