use super::assets::Asset;
use super::users::UserInfoWithAvatar;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Groups - User Group Management
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::groups)]
pub struct Group {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub group_type: Option<String>,
    pub mail_enabled: bool,
    pub security_enabled: bool,
    pub last_synced_at: Option<NaiveDateTime>,
    pub sync_enabled: bool,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::groups)]
pub struct NewGroup {
    pub name: String,
    pub description: Option<String>,
    pub color: Option<String>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::groups)]
pub struct NewExternalGroup {
    pub name: String,
    pub description: Option<String>,
    pub external_id: Option<String>,
    pub external_source: Option<String>,
    pub group_type: Option<String>,
    pub mail_enabled: bool,
    pub security_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::groups)]
pub struct GroupUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::groups)]
pub struct ExternalGroupUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub group_type: Option<String>,
    pub mail_enabled: Option<bool>,
    pub security_enabled: Option<bool>,
    pub last_synced_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

// Group include (composite group membership)
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::group_includes)]
#[diesel(primary_key(parent_group_id, child_group_id))]
pub struct GroupInclude {
    pub parent_group_id: i32,
    pub child_group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::group_includes)]
pub struct NewGroupInclude {
    pub parent_group_id: i32,
    pub child_group_id: i32,
    pub created_by: Option<Uuid>,
}

// Lightweight group summary for include display
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct GroupSummary {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub color: Option<String>,
    pub external_source: Option<String>,
    pub member_count: i64,
    pub members: Vec<UserInfoWithAvatar>,
}

// Group with member count for list views
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupWithMemberCount {
    #[serde(flatten)]
    pub group: Group,
    pub member_count: i64,
    pub device_count: i64,
    pub included_group_count: i64,
}

// Group with full member details
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupWithMembers {
    #[serde(flatten)]
    pub group: Group,
    pub members: Vec<UserInfoWithAvatar>,
}

// Group with members and devices (for detail view)
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupDetails {
    #[serde(flatten)]
    pub group: Group,
    pub members: Vec<UserInfoWithAvatar>,
    pub devices: Vec<Asset>,
    pub included_groups: Vec<GroupSummary>,
    pub included_in: Vec<GroupSummary>,
}

// User-Group junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::user_groups)]
#[diesel(belongs_to(Group))]
#[diesel(primary_key(user_uuid, group_id))]
pub struct UserGroup {
    pub user_uuid: Uuid,
    pub group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_groups)]
pub struct NewUserGroup {
    pub user_uuid: Uuid,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
}

// Asset ↔ directory-group junction. Links an asset to a directory `Group`
// (Intune/Entra-synced or manual). Named for what it is now that the native
// `asset_groups` entity below owns the unqualified "asset group" concept.
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::asset_directory_memberships)]
#[diesel(belongs_to(Group))]
#[diesel(belongs_to(Asset, foreign_key = asset_id))]
#[diesel(primary_key(asset_id, group_id))]
pub struct AssetDirectoryMembership {
    pub asset_id: i32,
    pub group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub external_source: Option<String>,
    pub workspace_id: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::asset_directory_memberships)]
pub struct NewAssetDirectoryMembership {
    pub asset_id: i32,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
    pub external_source: Option<String>,
}
