// models.rs
use chrono::{NaiveDate, NaiveDateTime};
use diesel::prelude::*;
use diesel::deserialize::{self, FromSql};
use diesel::pg::{Pg, PgValue};
use diesel::serialize::{self, IsNull, Output, ToSql};
// Removed unused import: use diesel::sql_types::Text;
use serde::{Deserialize, Serialize};
use std::io::Write;
use uuid::Uuid;

// Simple UUID serialization helpers
fn serialize_optional_uuid_as_string<S>(uuid: &Option<Uuid>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&uuid.map(|u| u.to_string()).unwrap_or_default())
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[derive(diesel::deserialize::FromSqlRow, diesel::expression::AsExpression)]
#[diesel(sql_type = crate::schema::sql_types::TicketStatus)]
pub enum TicketStatus {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "in-progress")]
    InProgress,
    #[serde(rename = "closed")]
    Closed,
}

impl TicketStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TicketStatus::Open => "open",
            TicketStatus::InProgress => "in-progress",
            TicketStatus::Closed => "closed",
        }
    }
}

impl ToSql<crate::schema::sql_types::TicketStatus, Pg> for TicketStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::TicketStatus, Pg> for TicketStatus {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"open" => Ok(TicketStatus::Open),
            b"in-progress" => Ok(TicketStatus::InProgress),
            b"closed" => Ok(TicketStatus::Closed),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[derive(diesel::deserialize::FromSqlRow, diesel::expression::AsExpression)]
#[diesel(sql_type = crate::schema::sql_types::TicketPriority)]
pub enum TicketPriority {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "medium")]
    Medium,
    #[serde(rename = "high")]
    High,
}

impl TicketPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            TicketPriority::Low => "low",
            TicketPriority::Medium => "medium",
            TicketPriority::High => "high",
        }
    }
}

impl ToSql<crate::schema::sql_types::TicketPriority, Pg> for TicketPriority {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::TicketPriority, Pg> for TicketPriority {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"low" => Ok(TicketPriority::Low),
            b"medium" => Ok(TicketPriority::Medium),
            b"high" => Ok(TicketPriority::High),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::tickets)]
pub struct Ticket {
    pub id: i32,
    pub title: String,
    pub status: TicketStatus,
    pub priority: TicketPriority,
    #[serde(serialize_with = "serialize_optional_uuid_as_string", rename = "requester")]
    pub requester_uuid: Option<Uuid>,
    #[serde(serialize_with = "serialize_optional_uuid_as_string", rename = "assignee")]
    pub assignee_uuid: Option<Uuid>,
    #[serde(rename = "created")]  // Map to frontend field name
    pub created_at: NaiveDateTime,
    #[serde(rename = "modified")] // Map to frontend field name
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub closed_at: Option<NaiveDateTime>,
    pub closed_by: Option<Uuid>,
    pub category_id: Option<i32>,
    pub submitted_via: Option<String>,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub guest_lookup_token: Option<Uuid>,
    pub verification_state: Option<String>,
    /// FK to the channel this ticket originated from (email mailbox, Slack
    /// workspace, etc.). Null for tickets submitted via the normal UI or
    /// the guest web form.
    pub origin_channel_id: Option<i32>,
}

// Ticket implementation removed - serialization now handled by serde attributes

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::tickets)]
pub struct NewTicket {
    pub title: String,
    pub status: TicketStatus,
    pub priority: TicketPriority,
    pub requester_uuid: Option<Uuid>,
    pub assignee_uuid: Option<Uuid>,
    pub category_id: Option<i32>,
    pub submitted_via: Option<String>,
    pub guest_lookup_token: Option<Uuid>,
    pub verification_state: Option<String>,
    pub origin_channel_id: Option<i32>,
}

// Add a new struct for partial ticket updates
#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::tickets)]
pub struct TicketUpdate {
    pub title: Option<String>,
    pub status: Option<TicketStatus>,
    pub priority: Option<TicketPriority>,
    pub requester_uuid: Option<Option<Uuid>>,
    pub assignee_uuid: Option<Option<Uuid>>,
    pub updated_at: Option<NaiveDateTime>,
    pub closed_at: Option<Option<NaiveDateTime>>,
    pub verification_state: Option<Option<String>>,
    pub origin_channel_id: Option<Option<i32>>,
    pub category_id: Option<Option<i32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::devices)]
pub struct Device {
    pub id: i32,
    pub name: String,
    pub hostname: Option<String>,
    pub device_type: Option<String>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub warranty_status: Option<String>,
    pub location: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub microsoft_device_id: Option<String>,
    pub intune_device_id: Option<String>,
    pub entra_device_id: Option<String>,
    pub compliance_state: Option<String>,
    pub last_sync_time: Option<NaiveDateTime>,
    pub operating_system: Option<String>,
    pub os_version: Option<String>,
    pub is_managed: Option<bool>,
    pub enrollment_date: Option<NaiveDateTime>,
    pub warranty_start_date: Option<NaiveDate>,
    pub warranty_end_date: Option<NaiveDate>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::devices)]
pub struct NewDevice {
    pub name: String,
    pub hostname: Option<String>,
    pub device_type: Option<String>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub warranty_status: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub microsoft_device_id: Option<String>,
    pub intune_device_id: Option<String>,
    pub entra_device_id: Option<String>,
    pub compliance_state: Option<String>,
    pub last_sync_time: Option<NaiveDateTime>,
    pub operating_system: Option<String>,
    pub os_version: Option<String>,
    pub is_managed: Option<bool>,
    pub enrollment_date: Option<NaiveDateTime>,
    pub warranty_start_date: Option<NaiveDate>,
    pub warranty_end_date: Option<NaiveDate>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::devices)]
pub struct DeviceUpdate {
    pub name: Option<String>,
    pub hostname: Option<String>,
    pub device_type: Option<String>,
    pub serial_number: Option<String>,
    pub manufacturer: Option<String>,
    pub model: Option<String>,
    pub warranty_status: Option<String>,
    pub location: Option<String>,
    pub notes: Option<String>,
    pub primary_user_uuid: Option<Uuid>,
    pub microsoft_device_id: Option<String>,
    pub intune_device_id: Option<String>,
    pub entra_device_id: Option<String>,
    pub compliance_state: Option<String>,
    pub last_sync_time: Option<NaiveDateTime>,
    pub operating_system: Option<String>,
    pub os_version: Option<String>,
    pub is_managed: Option<bool>,
    pub enrollment_date: Option<NaiveDateTime>,
    pub warranty_start_date: Option<NaiveDate>,
    pub warranty_end_date: Option<NaiveDate>,
    pub purchase_date: Option<NaiveDate>,
    pub asset_tag: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::ticket_devices)]
#[diesel(belongs_to(Ticket))]
#[diesel(belongs_to(Device))]
#[diesel(primary_key(ticket_id, device_id))]
pub struct TicketDevice {
    pub ticket_id: i32,
    pub device_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::ticket_devices)]
pub struct NewTicketDevice {
    pub ticket_id: i32,
    pub device_id: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::comments)]
#[diesel(belongs_to(Ticket))]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
pub struct Comment {
    pub id: i32,
    pub content: String,
    pub ticket_id: i32,
    pub user_uuid: Uuid,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub is_edited: bool,
    pub edit_count: i32,
    /// Free-form per-channel metadata (our emitted Message-ID for email,
    /// Slack thread_ts, Discord message id, etc.). Null for comments
    /// authored through the normal Nosdesk UI without channel context.
    pub channel_metadata: Option<serde_json::Value>,
    /// True = tech-to-tech note. Never shown to requesters in their
    /// portal view; never relayed back through the originating channel.
    pub is_internal: bool,
    /// Soft-delete marker. Set by future channel-edit/delete pipeline
    /// handlers when Slack/Teams/Discord signal a deleted message.
    pub deleted_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::comments)]
pub struct NewComment {
    pub content: String,
    pub ticket_id: i32,
    pub user_uuid: Uuid,
    #[serde(default)]
    pub channel_metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub is_internal: bool,
}

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::attachments)]
#[diesel(belongs_to(Comment))]
pub struct Attachment {
    pub id: i32,
    pub url: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub comment_id: Option<i32>,
    pub uploaded_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub transcription: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::attachments)]
pub struct NewAttachment {
    pub url: String,
    pub name: String,
    pub file_size: Option<i64>,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub comment_id: Option<i32>,
    pub uploaded_by: Option<Uuid>,
    pub transcription: Option<String>,
}

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
}

#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::article_contents)]
pub struct NewArticleContent {
    pub ticket_id: i32,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct CompleteTicket {
    #[serde(flatten)]
    pub ticket: Ticket,
    pub requester_user: Option<UserInfoWithAvatar>,  // Complete requester data
    pub assignee_user: Option<UserInfoWithAvatar>,   // Complete assignee data
    pub devices: Vec<Device>,
    pub comments: Vec<CommentWithAttachments>,
    pub article_content: Option<String>,
    pub linked_tickets: Vec<i32>,
    pub projects: Vec<Project>,
}

// Simplified ticket for lists - includes user info but not heavy data like comments
#[derive(Debug, Serialize, Deserialize)]
pub struct TicketListItem {
    #[serde(flatten)]
    pub ticket: Ticket,
    pub requester_user: Option<UserInfoWithAvatar>,  // Complete requester data
    pub assignee_user: Option<UserInfoWithAvatar>,   // Complete assignee data
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentWithAttachments {
    #[serde(flatten)]
    pub comment: Comment,
    pub attachments: Vec<Attachment>,
    pub user: Option<UserInfoWithAvatar>,  // Use enhanced user info with avatar
}

// JSON import struct that matches the structure in tickets.json
#[derive(Debug, Serialize, Deserialize)]
pub struct TicketJson {
    pub id: i32,
    pub title: String,
    pub status: String,
    pub priority: String,
    pub created: String,
    pub modified: String,
    pub assignee: String,
    pub requester: String,
    pub device: Option<DeviceJson>,
    pub comments: Option<Vec<CommentJson>>,
    pub article_content: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeviceJson {
    pub id: String,
    pub name: String,
    pub hostname: String,
    #[serde(rename = "serialNumber")]
    pub serial_number: String,
    pub model: String,
    #[serde(rename = "warrantyStatus")]
    pub warranty_status: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentJson {
    pub id: i32,
    pub content: String,
    pub user_uuid: String,
    #[serde(rename = "createdAt")]
    pub created_at: String,
    pub attachments: Vec<AttachmentJson>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AttachmentJson {
    pub url: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TicketsJson {
    pub tickets: Vec<TicketJson>,
}

// Documentation Status Enum
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[derive(diesel::deserialize::FromSqlRow, diesel::expression::AsExpression)]
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
    pub ticket_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: bool,
    pub is_template: bool,
    pub archived_at: Option<chrono::NaiveDateTime>,
    pub yjs_state_vector: Option<Vec<u8>>,
    pub yjs_document: Option<Vec<u8>>,
    pub yjs_client_id: Option<i64>,
    pub has_unsaved_changes: bool,
    pub deleted_at: Option<chrono::NaiveDateTime>,
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

// User Role Enum
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[derive(diesel::deserialize::FromSqlRow, diesel::expression::AsExpression)]
#[diesel(sql_type = crate::schema::sql_types::UserRole)]
pub enum UserRole {
    #[serde(rename = "admin")]
    Admin,
    #[serde(rename = "technician")]
    Technician,
    #[serde(rename = "user")]
    User,
}

impl UserRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            UserRole::Admin => "admin",
            UserRole::Technician => "technician",
            UserRole::User => "user",
        }
    }
}

impl ToSql<crate::schema::sql_types::UserRole, Pg> for UserRole {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        out.write_all(self.as_str().as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::UserRole, Pg> for UserRole {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"admin" => Ok(UserRole::Admin),
            b"technician" => Ok(UserRole::Technician),
            b"user" => Ok(UserRole::User),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

// User model - updated to match the actual database schema
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::users)]
#[diesel(primary_key(uuid))]
pub struct User {
    pub uuid: Uuid,
    pub name: String,
    // Email removed - now stored in user_emails table only
    pub role: UserRole,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub password_changed_at: Option<NaiveDateTime>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub mfa_secret: Option<String>,
    pub mfa_enabled: bool,
    pub mfa_backup_codes: Option<serde_json::Value>,
    pub passkey_credentials: Option<serde_json::Value>,
    /// Free-form text appended to outbound channel replies as the
    /// agent's email signature. Stored as-is; user owns formatting.
    /// `None` / empty → no signature appended.
    pub signature: Option<String>,
    /// Per-user dashboard customization. `None` means the client falls
    /// back to the role default. Shape is authoritative client-side
    /// (`{ widgets: [{ id, visible }] }`) and validated on update.
    pub dashboard_layout: Option<serde_json::Value>,
}

// New user for creation
// Note: Email is no longer part of NewUser - it's created separately in user_emails table
#[derive(Debug, Serialize, Deserialize, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct NewUser {
    pub uuid: Uuid,
    pub name: String,
    // Email removed - handled separately via user_emails table
    pub role: UserRole,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub mfa_secret: Option<String>,
    pub mfa_enabled: bool,
    pub mfa_backup_codes: Option<serde_json::Value>,
    pub passkey_credentials: Option<serde_json::Value>,
    #[serde(default)]
    pub signature: Option<String>,
    #[serde(default)]
    pub dashboard_layout: Option<serde_json::Value>,
}

// Add a separate struct for user registration with password
#[derive(Deserialize, Debug)]
pub struct UserRegistration {
    pub name: String,
    pub email: String,
    pub role: String, 
    pub password: String,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
}

// User update struct
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct UserUpdate {
    pub name: Option<String>,
    // Email removed - update via user_emails table instead
    pub role: Option<UserRole>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub updated_at: Option<chrono::NaiveDateTime>,
    /// `Option<Option<String>>` semantics: outer `None` = leave as-is;
    /// `Some(None)` = clear; `Some(Some(s))` = set.
    pub signature: Option<Option<String>>,
    /// `None` = no change. `Some(value)` = persist this JSON blob as
    /// the user's dashboard layout. Reset-to-defaults is handled
    /// client-side: the frontend computes the role default and sends
    /// it as a concrete payload.
    pub dashboard_layout: Option<serde_json::Value>,
}

// User update with password for admin/user management
#[derive(Debug, Serialize, Deserialize)]
pub struct UserUpdateWithPassword {
    pub name: Option<String>,
    // Email removed - update via user_emails table
    pub role: Option<String>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub password: Option<String>,
    /// Free-form text appended to outbound channel replies as the
    /// agent's signature. `None` in the payload → no change. Empty
    /// string clears it.
    pub signature: Option<String>,
    /// Dashboard layout JSON (see `UserUpdate::dashboard_layout`).
    #[serde(default)]
    pub dashboard_layout: Option<serde_json::Value>,
}

// User profile update for profile management
#[derive(Debug, Serialize, Deserialize)]
pub struct UserProfileUpdate {
    pub name: Option<String>,
    // Email removed - update via user_emails table
    pub role: Option<String>,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub password: Option<String>,
    /// Email signature appended to outbound channel replies.
    pub signature: Option<String>,
}

// User response with minimal information
#[derive(Debug, Serialize, Deserialize)]
pub struct UserResponse {
    pub uuid: Uuid,
    pub name: String,
    pub email: Option<String>, // Now optional - populated from user_emails table
    pub role: UserRole,
    pub pronouns: Option<String>,
    pub avatar_url: Option<String>,
    pub banner_url: Option<String>,
    pub avatar_thumb: Option<String>,
    pub theme: Option<String>,
    pub microsoft_uuid: Option<Uuid>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_ticket_count: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_count: Option<i64>,
    /// Per-user dashboard layout JSON, or null = client uses defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dashboard_layout: Option<serde_json::Value>,
}

// User info for comments - minimal user data to include with comments
#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub uuid: Uuid,
    pub name: String,
}

// Enhanced UserInfo with avatar data for efficient frontend display
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UserInfoWithAvatar {
    pub uuid: Uuid,
    pub name: String,
    pub avatar_url: Option<String>,
    pub avatar_thumb: Option<String>,
}

// Convert User to UserResponse
// Note: This From implementation sets email to None
// Use repository::user_helpers::get_user_with_primary_email() to include email
impl From<User> for UserResponse {
    fn from(user: User) -> Self {
        UserResponse {
            uuid: user.uuid,
            name: user.name,
            email: None, // Email must be fetched from user_emails table separately
            role: user.role,
            pronouns: user.pronouns,
            avatar_url: user.avatar_url,
            banner_url: user.banner_url,
            avatar_thumb: user.avatar_thumb,
            theme: user.theme,
            microsoft_uuid: user.microsoft_uuid,
            created_at: user.created_at,
            updated_at: user.updated_at,
            open_ticket_count: None,
            device_count: None,
            dashboard_layout: user.dashboard_layout,
        }
    }
}

// Convert User to UserInfo
impl From<User> for UserInfo {
    fn from(user: User) -> Self {
        UserInfo {
            uuid: user.uuid,
            name: user.name,
        }
    }
}

impl From<User> for UserInfoWithAvatar {
    fn from(user: User) -> Self {
        UserInfoWithAvatar {
            uuid: user.uuid,
            name: user.name,
            avatar_url: user.avatar_url,
            avatar_thumb: user.avatar_thumb,
        }
    }
}

// User Email models for storing multiple email addresses per user
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::user_emails)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
pub struct UserEmail {
    pub id: i32,
    pub user_uuid: Uuid,
    pub email: String,
    pub email_type: String,
    pub is_primary: bool,
    pub is_verified: bool,
    pub source: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_emails)]
pub struct NewUserEmail {
    pub user_uuid: Uuid,
    pub email: String,
    pub email_type: String,
    pub is_primary: bool,
    pub is_verified: bool,
    pub source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::user_emails)]
pub struct UserEmailUpdate {
    pub is_primary: Option<bool>,
    pub is_verified: Option<bool>,
    pub updated_at: Option<NaiveDateTime>,
}

// Extended User response that includes all email addresses
#[derive(Debug, Serialize, Deserialize)]
pub struct UserWithEmails {
    #[serde(flatten)]
    pub user: UserResponse,
    pub emails: Vec<UserEmail>,
}

// Project Status Enum
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[derive(diesel::deserialize::FromSqlRow, diesel::expression::AsExpression)]
#[diesel(sql_type = crate::schema::sql_types::ProjectStatus)]
pub enum ProjectStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "completed")]
    Completed,
    #[serde(rename = "archived")]
    Archived,
}

impl ToSql<crate::schema::sql_types::ProjectStatus, Pg> for ProjectStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            ProjectStatus::Active => "active",
            ProjectStatus::Completed => "completed",
            ProjectStatus::Archived => "archived",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::ProjectStatus, Pg> for ProjectStatus {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"active" => Ok(ProjectStatus::Active),
            b"completed" => Ok(ProjectStatus::Completed),
            b"archived" => Ok(ProjectStatus::Archived),
            _ => Err("Unrecognized enum variant".into()),
        }
    }
}

// Project model
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::projects)]
pub struct Project {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub owner_uuid: Option<Uuid>,
}

// New Project for creating projects
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::projects)]
pub struct NewProject {
    pub name: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
}

// Project Update for partial updates
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::projects)]
pub struct ProjectUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<ProjectStatus>,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub updated_at: Option<NaiveDateTime>,
}

// Project Ticket association
#[derive(Debug, Serialize, Deserialize, Identifiable, Associations, Queryable)]
#[diesel(belongs_to(Project))]
#[diesel(belongs_to(Ticket))]
#[diesel(table_name = crate::schema::project_tickets)]
#[diesel(primary_key(project_id, ticket_id))]
pub struct ProjectTicket {
    pub project_id: i32,
    pub ticket_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub display_order: i32,
}

// New Project Ticket for creating associations
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::project_tickets)]
pub struct NewProjectTicket {
    pub project_id: i32,
    pub ticket_id: i32,
    pub display_order: i32,
}

// Project with ticket count for API responses
#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectWithTicketCount {
    pub id: i32,
    pub name: String,
    pub description: Option<String>,
    pub status: ProjectStatus,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub ticket_count: i64,
    /// Optional embedded ticket list, populated only when the
    /// `GET /projects/{id}?embed=tickets` flag is set. Skipped from
    /// JSON when absent so the legacy unbundled response shape is
    /// unchanged for existing callers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tickets: Option<Vec<TicketListItem>>,
}

// LinkedTicket model
#[derive(Debug, Serialize, Deserialize, Identifiable, Associations, Queryable)]
#[diesel(table_name = crate::schema::linked_tickets)]
#[diesel(primary_key(ticket_id, linked_ticket_id))]
#[diesel(belongs_to(Ticket, foreign_key = ticket_id))]
pub struct LinkedTicket {
    pub ticket_id: i32,
    pub linked_ticket_id: i32,
    pub link_type: String,
    pub description: Option<String>,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::linked_tickets)]
pub struct NewLinkedTicket {
    pub ticket_id: i32,
    pub linked_ticket_id: i32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AttachmentData {
    pub id: Option<i32>,
    pub url: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NewCommentWithAttachments {
    pub content: String,
    // user_id/user_uuid removed - extracted from JWT token for security
    pub attachments: Vec<AttachmentData>,
}

// JWT Claims structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,  // Subject (user UUID as string for JWT compatibility)
    pub name: String, // User's name
    pub email: String, // User's email
    pub role: String, // User's role
    #[serde(default = "default_scope")] // Default to "full" for backward compatibility with existing tokens
    pub scope: String, // Token scope: "full" for normal sessions
    #[serde(default)] // Session ID (UUID) — None for SSE/API tokens
    pub sid: Option<String>,
    pub exp: usize,   // Expiration time
    pub iat: usize,   // Issued at
}

impl Claims {
    /// Parse the `sid` claim into a UUID. Returns None for SSE/API tokens.
    pub fn session_uuid(&self) -> Option<Uuid> {
        self.sid.as_deref().and_then(|s| s.parse().ok())
    }
}

// Default scope for backward compatibility
fn default_scope() -> String {
    "full".to_string()
}

// Login request structure
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

// Login response structure - supports both standard login and MFA flow
// Note: tokens are now in httpOnly cookies, only CSRF token is in response body
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    pub success: bool,
    pub mfa_required: Option<bool>,
    pub mfa_setup_required: Option<bool>,
    pub passkey_mfa_required: Option<bool>,
    pub user_uuid: Option<String>,
    pub csrf_token: Option<String>, // CSRF token for the frontend
    pub user: Option<UserResponse>,
    pub message: Option<String>,
    pub mfa_backup_code_used: Option<bool>,
    pub requires_backup_code_regeneration: Option<bool>,
    pub backup_codes: Option<Vec<String>>, // Present when MFA is enabled during login setup
}

/// Request for MFA verification during login
#[derive(Debug, Deserialize)]
pub struct MfaLoginRequest {
    pub email: String,
    pub password: String,
    pub mfa_token: String,
}

/// Request for recovery code login (passkey-MFA users who can't use their passkey)
#[derive(Debug, Deserialize)]
pub struct RecoveryLoginRequest {
    pub email: String,
    pub password: String,
    pub recovery_code: String,
}

/// Request for MFA setup during login (unauthenticated)
#[derive(Debug, Deserialize)]
pub struct MfaSetupLoginRequest {
    pub email: String,
    pub password: String,
}

/// Request for enabling MFA during login (unauthenticated)
#[derive(Debug, Deserialize)]
pub struct MfaEnableLoginRequest {
    pub email: String,
    pub password: String,
    pub token: String,
    pub secret: Option<String>,
}

/// Response for token refresh
/// Note: tokens are now in httpOnly cookies, only CSRF token is in response
#[derive(Debug, Serialize)]
pub struct RefreshTokenResponse {
    pub success: bool,
    pub csrf_token: String,
}

#[derive(Deserialize, Debug)]
pub struct PasswordChangeRequest {
    pub current_password: String,
    pub new_password: String,
}

// Authentication Provider models
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[derive(diesel::deserialize::FromSqlRow, diesel::expression::AsExpression)]
#[diesel(sql_type = diesel::sql_types::Text)]
pub enum AuthProviderType {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "microsoft")]
    Microsoft,
    #[serde(rename = "google")]
    Google,
    #[serde(rename = "saml")]
    Saml,
}

impl ToSql<diesel::sql_types::Text, Pg> for AuthProviderType {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            AuthProviderType::Local => "local",
            AuthProviderType::Microsoft => "microsoft",
            AuthProviderType::Google => "google",
            AuthProviderType::Saml => "saml",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<diesel::sql_types::Text, Pg> for AuthProviderType {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"local" => Ok(AuthProviderType::Local),
            b"microsoft" => Ok(AuthProviderType::Microsoft),
            b"google" => Ok(AuthProviderType::Google),
            b"saml" => Ok(AuthProviderType::Saml),
            _ => Err("Unrecognized auth provider type".into()),
        }
    }
}

// Environment-based AuthProvider struct (replaces database-stored providers)
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthProvider {
    pub id: i32,
    pub name: String,
    pub provider_type: String,
    pub enabled: bool,
    pub is_default: bool,
}

impl AuthProvider {
    pub fn new(id: i32, name: String, provider_type: String, enabled: bool, is_default: bool) -> Self {
        Self {
            id,
            name,
            provider_type,
            enabled,
            is_default,
        }
    }
}

// Request models for authentication
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthProviderConfigRequest {
    pub provider_id: i32,
    pub configs: Vec<ConfigItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigItem {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

// Response model for client display
#[derive(Debug, Serialize, Deserialize)]
pub struct AuthProviderWithConfig {
    pub id: i32,
    pub provider_type: String,
    pub name: String,
    pub enabled: bool,
    pub is_default: bool,
    pub configs: Vec<AuthProviderConfigResponse>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthProviderConfigResponse {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

// OAuth state management
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthState {
    pub state: String,
    pub redirect_uri: String,
    pub provider_type: String,
    pub exp: usize,
    pub user_connection: Option<bool>,
    /// PKCE code verifier (for OIDC providers)
    pub pkce_verifier: Option<String>,
    /// Nonce for ID token validation (for OIDC providers)
    pub nonce: Option<String>,
}

// OAuth Authentication request
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthRequest {
    pub provider_type: String,
    pub redirect_uri: Option<String>,
    pub user_connection: Option<bool>,
}

// OAuth callback/exchange parameters
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct OAuthExchangeRequest {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>
}

// Microsoft Entra specific models
#[derive(Debug, Serialize, Deserialize)]
pub struct MicrosoftAuthConfig {
    pub client_id: String,
    pub tenant_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

// Models for user authentication identities
#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::user_auth_identities)]
pub struct UserAuthIdentity {
    pub id: i32,
    pub user_uuid: Uuid,
    pub provider_type: String,
    pub external_id: String,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub password_hash: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_auth_identities)]
pub struct NewUserAuthIdentity {
    pub user_uuid: Uuid,
    pub provider_type: String,
    pub external_id: String,
    pub email: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub password_hash: Option<String>,
}

// For displaying auth identities in the user profile
#[derive(Debug, Serialize, Deserialize)]
pub struct UserAuthIdentityDisplay {
    pub id: i32,
    pub provider_type: String,
    pub provider_name: String,
    pub email: Option<String>,
    pub created_at: NaiveDateTime,
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
    pub ticket_id: Option<i32>,
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
    pub ticket_id: Option<Option<i32>>,
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
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_revisions)]
pub struct NewDocumentationRevision {
    pub page_id: i32,
    pub revision_number: i32,
    pub title: String,
    pub yjs_document_snapshot: Vec<u8>,
    pub yjs_state_vector: Vec<u8>,
    pub created_by: Uuid,
    pub change_summary: Option<String>,
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
    pub ticket_id: Option<i32>,
    pub display_order: Option<i32>,
    pub is_public: bool,
    pub is_template: bool,
    pub archived_at: Option<chrono::NaiveDateTime>,
    pub deleted_at: Option<chrono::NaiveDateTime>,
    pub has_unsaved_changes: bool,
    pub children: Option<Vec<DocumentationPageResponse>>,
    pub content: Option<String>,
}

// Sync History Models
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct SyncHistory {
    pub id: i32,
    pub sync_type: String,
    pub status: String,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
    pub tenant_id: Option<String>,
    pub initiated_by: Option<Uuid>,
    pub is_delta: bool,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct NewSyncHistory {
    pub sync_type: String,
    pub status: String,
    pub started_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
    pub tenant_id: Option<String>,
    pub is_delta: bool,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::sync_history)]
pub struct SyncHistoryUpdate {
    pub status: Option<String>,
    pub completed_at: Option<Option<NaiveDateTime>>,
    pub error_message: Option<String>,
    pub records_processed: Option<i32>,
    pub records_created: Option<i32>,
    pub records_updated: Option<i32>,
    pub records_failed: Option<i32>,
}

// Delta tokens for incremental sync (Microsoft Graph delta queries)
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::sync_delta_tokens)]
pub struct SyncDeltaToken {
    pub id: i32,
    pub provider_type: String,
    pub entity_type: String,
    pub delta_link: String,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::sync_delta_tokens)]
pub struct NewSyncDeltaToken {
    pub provider_type: String,
    pub entity_type: String,
    pub delta_link: String,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::sync_delta_tokens)]
pub struct SyncDeltaTokenUpdate {
    pub delta_link: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProgressPoint {
    pub name: String,
    pub sort_order: i32,
}

// Onboarding models
#[derive(Debug, Serialize, Deserialize)]
pub struct OnboardingStatus {
    pub requires_setup: bool,
    pub user_count: i64,
    pub microsoft_auth_enabled: bool,
    pub oidc_enabled: bool,
    pub oidc_display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AdminSetupRequest {
    pub name: String,
    pub email: String,
    pub password: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AdminSetupResponse {
    pub success: bool,
    pub message: String,
    pub user: Option<UserResponse>,
}

// Frontend-compatible version of CompleteTicket
#[derive(Debug, Serialize)]
pub struct CompleteTicketResponse {
    pub id: i32,
    pub title: String,
    pub status: TicketStatus,
    pub priority: TicketPriority,
    pub requester: String,
    pub assignee: String,
    pub created: String,
    pub modified: String,
    pub devices: Vec<Device>,
    pub comments: Vec<CommentWithAttachments>,
    pub article_content: Option<String>,
    pub linked_tickets: Vec<i32>,
    pub projects: Vec<Project>,
}

impl CompleteTicketResponse {
}

// === MFA (Multi-Factor Authentication) Models ===

/// QR code matrix data for frontend rendering
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrMatrix {
    /// Width/height of the QR code (always square)
    pub size: usize,
    /// Flattened boolean array (row-major order), true = dark module
    pub data: Vec<bool>,
}

/// Response for MFA setup request
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaSetupResponse {
    pub secret: String,
    pub qr_code: String,
    pub backup_codes: Vec<String>,
    /// QR code matrix data for animated rendering
    pub qr_matrix: Option<QrMatrix>,
}

/// Request for verifying MFA setup
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaVerifySetupRequest {
    pub token: String,
    pub secret: String,
}

/// Response for MFA setup verification
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaVerifySetupResponse {
    pub success: bool,
    pub backup_codes: Vec<String>,
}

/// Request for enabling MFA
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaEnableRequest {
    pub token: String,
    pub secret: Option<String>,
    pub backup_codes: Option<Vec<String>>,
}

/// Request for disabling MFA
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaDisableRequest {
    pub password: String,
}

/// Request for regenerating backup codes
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaRegenerateBackupCodesRequest {
    pub password: String,
}

/// Response for regenerating backup codes
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaRegenerateBackupCodesResponse {
    pub backup_codes: Vec<String>,
}

/// Response for MFA status
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaStatusResponse {
    pub enabled: bool,
    pub has_backup_codes: bool,
}

/// Update struct for user MFA fields
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct UserMfaUpdate {
    pub mfa_secret: Option<String>,
    pub mfa_enabled: Option<bool>,
    pub mfa_backup_codes: Option<serde_json::Value>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

/// Update struct for user passkey credentials
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::users)]
pub struct UserPasskeyUpdate {
    pub passkey_credentials: Option<serde_json::Value>,
    pub updated_at: Option<chrono::NaiveDateTime>,
}

// ===== SESSION MANAGEMENT MODELS =====

/// Active user sessions for session management and revocation
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::active_sessions)]
pub struct ActiveSession {
    pub id: i32,
    pub user_uuid: Uuid,
    pub device_name: Option<String>,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub last_active: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub is_current: bool,
    pub session_id: Uuid,
}

/// New active session for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::active_sessions)]
pub struct NewActiveSession {
    pub user_uuid: Uuid,
    pub device_name: Option<String>,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub expires_at: chrono::NaiveDateTime,
    pub is_current: bool,
}

/// Update struct for active sessions
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::active_sessions)]
pub struct ActiveSessionUpdate {
    pub last_active: Option<chrono::NaiveDateTime>,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub is_current: Option<bool>,
}

/// Refresh token for JWT token rotation
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::refresh_tokens)]
pub struct RefreshToken {
    pub id: i32,
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub created_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub session_id: Option<Uuid>,
    pub family_id: Uuid,
    pub is_used: bool,
    pub used_at: Option<chrono::NaiveDateTime>,
    pub replaced_by_hash: Option<String>,
    pub grace_expires_at: Option<chrono::NaiveDateTime>,
}

/// New refresh token for creation
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::refresh_tokens)]
pub struct NewRefreshToken {
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub expires_at: chrono::NaiveDateTime,
    pub session_id: Option<Uuid>,
    pub family_id: Uuid,
}

// ===== API TOKEN MODELS =====

/// API token for programmatic access (stored in database)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::api_tokens)]
pub struct ApiToken {
    pub id: i32,
    pub uuid: Uuid,
    pub token_hash: String,
    pub token_prefix: String,
    pub user_uuid: Uuid,
    pub name: String,
    pub scopes: Option<Vec<Option<String>>>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub last_used_at: Option<chrono::NaiveDateTime>,
    pub last_used_ip: Option<ipnetwork::IpNetwork>,
}

/// New API token for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::api_tokens)]
pub struct NewApiToken {
    pub token_hash: String,
    pub token_prefix: String,
    pub user_uuid: Uuid,
    pub name: String,
    pub scopes: Option<Vec<Option<String>>>,
    pub created_by: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
}

/// Request to create a new API token
#[derive(Debug, Deserialize)]
pub struct CreateApiTokenRequest {
    pub name: String,
    pub user_uuid: Uuid,
    #[serde(default)]
    pub expires_in_days: Option<i64>,
    #[serde(default)]
    pub scopes: Option<Vec<String>>,
}

/// Response when an API token is created (includes the raw token - only shown once!)
#[derive(Debug, Serialize)]
pub struct ApiTokenCreatedResponse {
    pub uuid: Uuid,
    pub token: String,
    pub token_prefix: String,
    pub name: String,
    pub user_uuid: Uuid,
    pub expires_at: Option<chrono::NaiveDateTime>,
}

/// API token info for listing (no sensitive data)
#[derive(Debug, Serialize)]
pub struct ApiTokenInfo {
    pub uuid: Uuid,
    pub token_prefix: String,
    pub name: String,
    pub user_uuid: Uuid,
    pub user_name: String,
    pub scopes: Vec<String>,
    pub created_at: chrono::NaiveDateTime,
    pub created_by_name: String,
    pub expires_at: Option<chrono::NaiveDateTime>,
    pub revoked_at: Option<chrono::NaiveDateTime>,
    pub last_used_at: Option<chrono::NaiveDateTime>,
}

/// Response model for active sessions in user profile
#[derive(Debug, Serialize, Deserialize)]
pub struct ActiveSessionResponse {
    pub id: i32,
    pub session_id: String,
    pub device_name: Option<String>,
    pub location: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub last_active: chrono::NaiveDateTime,
    pub is_current: bool,
}

impl From<ActiveSession> for ActiveSessionResponse {
    fn from(session: ActiveSession) -> Self {
        ActiveSessionResponse {
            id: session.id,
            session_id: session.session_id.to_string(),
            device_name: session.device_name,
            location: session.location,
            ip_address: session.ip_address.map(|ip| ip.to_string()),
            created_at: session.created_at,
            last_active: session.last_active,
            is_current: session.is_current,
        }
    }
}

// ===== SECURITY EVENTS MODELS =====

/// Security events for MFA and authentication monitoring
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::security_events)]
pub struct SecurityEvent {
    pub id: i32,
    pub user_uuid: Uuid,
    pub event_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub details: Option<serde_json::Value>,
    pub severity: String,
    pub created_at: chrono::NaiveDateTime,
    pub session_id: Option<i32>,
}

/// New security event for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::security_events)]
pub struct NewSecurityEvent {
    pub user_uuid: Uuid,
    pub event_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub details: Option<serde_json::Value>,
    pub severity: String,
    pub session_id: Option<i32>,
}

/// Security event types enum for type safety
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash)]
pub enum SecurityEventType {
    #[serde(rename = "login_success")]
    LoginSuccess,
    #[serde(rename = "login_failed")]
    LoginFailed,
    #[serde(rename = "mfa_enabled")]
    MfaEnabled,
    #[serde(rename = "mfa_disabled")]
    MfaDisabled,
    #[serde(rename = "mfa_failed")]
    MfaFailed,
    #[serde(rename = "mfa_success")]
    MfaSuccess,
    #[serde(rename = "backup_codes_used")]
    BackupCodesUsed,
    #[serde(rename = "backup_codes_regenerated")]
    BackupCodesRegenerated,
    #[serde(rename = "password_changed")]
    PasswordChanged,
    #[serde(rename = "session_revoked")]
    SessionRevoked,
    #[serde(rename = "account_locked")]
    AccountLocked,
    #[serde(rename = "suspicious_activity")]
    SuspiciousActivity,
}

impl SecurityEventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LoginSuccess => "login_success",
            Self::LoginFailed => "login_failed",
            Self::MfaEnabled => "mfa_enabled",
            Self::MfaDisabled => "mfa_disabled",
            Self::MfaFailed => "mfa_failed",
            Self::MfaSuccess => "mfa_success",
            Self::BackupCodesUsed => "backup_codes_used",
            Self::BackupCodesRegenerated => "backup_codes_regenerated",
            Self::PasswordChanged => "password_changed",
            Self::SessionRevoked => "session_revoked",
            Self::AccountLocked => "account_locked",
            Self::SuspiciousActivity => "suspicious_activity",
        }
    }
}

impl std::fmt::Display for SecurityEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for SecurityEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "login_success" => Ok(Self::LoginSuccess),
            "login_failed" => Ok(Self::LoginFailed),
            "mfa_enabled" => Ok(Self::MfaEnabled),
            "mfa_disabled" => Ok(Self::MfaDisabled),
            "mfa_failed" => Ok(Self::MfaFailed),
            "mfa_success" => Ok(Self::MfaSuccess),
            "backup_codes_used" => Ok(Self::BackupCodesUsed),
            "backup_codes_regenerated" => Ok(Self::BackupCodesRegenerated),
            "password_changed" => Ok(Self::PasswordChanged),
            "session_revoked" => Ok(Self::SessionRevoked),
            "account_locked" => Ok(Self::AccountLocked),
            "suspicious_activity" => Ok(Self::SuspiciousActivity),
            _ => Err(format!("Invalid security event type: {s}")),
        }
    }
}

/// Security event severity enum
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecurityEventSeverity {
    #[serde(rename = "info")]
    Info,
    #[serde(rename = "warning")]
    Warning,
    #[serde(rename = "critical")]
    Critical,
}

impl SecurityEventSeverity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Critical => "critical",
        }
    }
}

impl std::fmt::Display for SecurityEventSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for SecurityEventSeverity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "info" => Ok(Self::Info),
            "warning" => Ok(Self::Warning),
            "critical" => Ok(Self::Critical),
            _ => Err(format!("Invalid security event severity: {s}")),
        }
    }
}

/// Response model for security events in user profile
#[derive(Debug, Serialize, Deserialize)]
pub struct SecurityEventResponse {
    pub id: i32,
    pub event_type: String,
    pub ip_address: Option<String>,
    pub location: Option<String>,
    pub severity: String,
    pub created_at: chrono::NaiveDateTime,
    pub details: Option<serde_json::Value>,
}

impl From<SecurityEvent> for SecurityEventResponse {
    fn from(event: SecurityEvent) -> Self {
        SecurityEventResponse {
            id: event.id,
            event_type: event.event_type,
            ip_address: event.ip_address.map(|ip| ip.to_string()),
            location: event.location,
            severity: event.severity,
            created_at: event.created_at,
            details: event.details,
        }
    }
}

// ===== RESET TOKENS MODELS =====

/// Generic reset tokens for password resets, MFA resets, and other temporary tokens
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::reset_tokens)]
#[diesel(primary_key(token_hash))]
pub struct ResetToken {
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub token_type: String,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub used_at: Option<chrono::NaiveDateTime>,
    pub is_used: bool,
    pub metadata: Option<serde_json::Value>,
}

/// New reset token for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::reset_tokens)]
pub struct NewResetToken<'a> {
    pub token_hash: &'a str,
    pub user_uuid: Uuid,
    pub token_type: &'a str,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<&'a str>,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub metadata: Option<serde_json::Value>,
}

/// Update struct for reset tokens
#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::reset_tokens)]
pub struct ResetTokenUpdate {
    pub used_at: Option<chrono::NaiveDateTime>,
    pub is_used: Option<bool>,
    pub metadata: Option<serde_json::Value>,
}

// ===== PASSWORD RESET MODELS =====

/// Request to initiate password reset
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetRequest {
    pub email: String,
}

/// Response for password reset initiation
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetResponse {
    pub message: String,
}

/// Request to complete password reset with token
#[derive(Debug, Serialize, Deserialize)]
pub struct PasswordResetCompleteRequest {
    pub token: String,
    pub new_password: String,
}

/// Session revocation request
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionRevocationRequest {
    pub session_id: Option<i32>, // If None, revoke all others
}

// ===== INVITATION MODELS =====

/// Request to accept an invitation and set password
#[derive(Debug, Serialize, Deserialize)]
pub struct AcceptInvitationRequest {
    pub token: String,
    pub password: String,
}

/// Response for invitation acceptance
#[derive(Debug, Serialize, Deserialize)]
pub struct AcceptInvitationResponse {
    pub success: bool,
    pub message: String,
}

/// Request to validate an invitation token (check if it's valid before showing the form)
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateInvitationRequest {
    pub token: String,
}

/// Response for invitation validation
#[derive(Debug, Serialize, Deserialize)]
pub struct ValidateInvitationResponse {
    pub valid: bool,
    pub user_email: Option<String>,
    pub user_name: Option<String>,
    pub message: Option<String>,
    /// Classification of the invitation's origin so the frontend can tailor
    /// copy ("confirm your ticket submission" vs generic onboarding).
    /// `"guest_ticket"` when the token was issued by a public ticket
    /// submission; `"invitation"` for an admin-sent invitation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
}

/// Response for session operations
#[derive(Debug, Serialize, Deserialize)]
pub struct SessionResponse {
    pub message: String,
    pub sessions_revoked: usize,
}

// User ticket views for tracking recently viewed tickets
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(belongs_to(Ticket))]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct UserTicketView {
    pub id: i32,
    pub user_uuid: Uuid,
    pub ticket_id: i32,
    pub first_viewed_at: NaiveDateTime,
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct NewUserTicketView {
    pub user_uuid: Uuid,
    pub ticket_id: i32,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::user_ticket_views)]
pub struct UpdateUserTicketView {
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
}

// Response structure for recent tickets API
#[derive(Debug, Serialize, Deserialize)]
pub struct RecentTicket {
    pub id: i32,
    pub title: String,
    pub status: TicketStatus,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub requester: Option<Uuid>,
    #[serde(serialize_with = "serialize_optional_uuid_as_string")]
    pub assignee: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub last_viewed_at: NaiveDateTime,
    pub view_count: i32,
}

// ============================================================================
// Site Settings - Branding and Customization
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::site_settings)]
pub struct SiteSettings {
    pub id: i32,
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub updated_by: Option<Uuid>,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_default_priority: Option<String>,
    pub guest_ticket_rate_limit_per_hour: i32,
    pub guest_ticket_email_verification: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
    /// Whether to send a one-off "thanks, we got your message" reply
    /// when a channel message opens a fresh ticket. Defaults true.
    pub channel_auto_ack_enabled: bool,
    /// Admin-overridden template for the auto-ack body. `None` uses
    /// the built-in default (see
    /// [`crate::services::channels::auto_ack::DEFAULT_TEMPLATE`]).
    pub channel_auto_ack_template: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::site_settings)]
pub struct UpdateSiteSettings {
    pub app_name: Option<String>,
    pub logo_url: Option<Option<String>>,
    pub logo_light_url: Option<Option<String>>,
    pub favicon_url: Option<Option<String>>,
    pub primary_color: Option<Option<String>>,
    pub updated_by: Option<Uuid>,
    pub guest_tickets_enabled: Option<bool>,
    pub guest_public_docs_enabled: Option<bool>,
    pub guest_kb_search_enabled: Option<bool>,
    pub guest_ticket_lookup_enabled: Option<bool>,
    pub guest_help_page_enabled: Option<bool>,
    pub guest_ticket_default_priority: Option<Option<String>>,
    pub guest_ticket_rate_limit_per_hour: Option<i32>,
    pub guest_ticket_email_verification: Option<bool>,
    pub guest_ticket_attachments_enabled: Option<bool>,
    pub guest_ticket_intro_message: Option<Option<String>>,
    pub channel_auto_ack_enabled: Option<bool>,
    pub channel_auto_ack_template: Option<Option<String>>,
}

// API response for site settings (without internal fields)
#[derive(Debug, Serialize, Deserialize)]
pub struct SiteSettingsResponse {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub updated_at: NaiveDateTime,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_default_priority: Option<String>,
    pub guest_ticket_rate_limit_per_hour: i32,
    pub guest_ticket_email_verification: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
}

impl From<SiteSettings> for SiteSettingsResponse {
    fn from(settings: SiteSettings) -> Self {
        SiteSettingsResponse {
            app_name: settings.app_name,
            logo_url: settings.logo_url,
            logo_light_url: settings.logo_light_url,
            favicon_url: settings.favicon_url,
            primary_color: settings.primary_color,
            updated_at: settings.updated_at,
            guest_tickets_enabled: settings.guest_tickets_enabled,
            guest_public_docs_enabled: settings.guest_public_docs_enabled,
            guest_kb_search_enabled: settings.guest_kb_search_enabled,
            guest_ticket_lookup_enabled: settings.guest_ticket_lookup_enabled,
            guest_help_page_enabled: settings.guest_help_page_enabled,
            guest_ticket_default_priority: settings.guest_ticket_default_priority,
            guest_ticket_rate_limit_per_hour: settings.guest_ticket_rate_limit_per_hour,
            guest_ticket_email_verification: settings.guest_ticket_email_verification,
            guest_ticket_attachments_enabled: settings.guest_ticket_attachments_enabled,
            guest_ticket_intro_message: settings.guest_ticket_intro_message,
        }
    }
}

// Public subset — safe to expose on /api/public/settings (no auth required)
#[derive(Debug, Serialize, Deserialize)]
pub struct PublicSiteSettings {
    pub app_name: String,
    pub logo_url: Option<String>,
    pub logo_light_url: Option<String>,
    pub favicon_url: Option<String>,
    pub primary_color: Option<String>,
    pub guest_tickets_enabled: bool,
    pub guest_public_docs_enabled: bool,
    pub guest_kb_search_enabled: bool,
    pub guest_ticket_lookup_enabled: bool,
    pub guest_help_page_enabled: bool,
    pub guest_ticket_attachments_enabled: bool,
    pub guest_ticket_intro_message: Option<String>,
}

impl From<&SiteSettings> for PublicSiteSettings {
    fn from(s: &SiteSettings) -> Self {
        PublicSiteSettings {
            app_name: s.app_name.clone(),
            logo_url: s.logo_url.clone(),
            logo_light_url: s.logo_light_url.clone(),
            favicon_url: s.favicon_url.clone(),
            primary_color: s.primary_color.clone(),
            guest_tickets_enabled: s.guest_tickets_enabled,
            guest_public_docs_enabled: s.guest_public_docs_enabled,
            guest_kb_search_enabled: s.guest_kb_search_enabled,
            guest_ticket_lookup_enabled: s.guest_ticket_lookup_enabled,
            guest_help_page_enabled: s.guest_help_page_enabled,
            guest_ticket_attachments_enabled: s.guest_ticket_attachments_enabled,
            guest_ticket_intro_message: s.guest_ticket_intro_message.clone(),
        }
    }
}

// ============================================================================
// Backup Jobs - System Backup and Restore
// ============================================================================

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct BackupJob {
    pub id: Uuid,
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct NewBackupJob {
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::backup_jobs)]
pub struct BackupJobUpdate {
    pub status: Option<String>,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub completed_at: Option<NaiveDateTime>,
}

// API response for backup jobs
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupJobResponse {
    pub id: String,
    pub job_type: String,
    pub status: String,
    pub include_sensitive: bool,
    pub file_path: Option<String>,
    pub file_size: Option<i64>,
    pub error_message: Option<String>,
    pub created_by: Option<String>,
    pub created_at: NaiveDateTime,
    pub completed_at: Option<NaiveDateTime>,
}

impl From<BackupJob> for BackupJobResponse {
    fn from(job: BackupJob) -> Self {
        BackupJobResponse {
            id: job.id.to_string(),
            job_type: job.job_type,
            status: job.status,
            include_sensitive: job.include_sensitive,
            file_path: job.file_path,
            file_size: job.file_size,
            error_message: job.error_message,
            created_by: job.created_by.map(|u| u.to_string()),
            created_at: job.created_at,
            completed_at: job.completed_at,
        }
    }
}

// Request to start an export backup
#[derive(Debug, Serialize, Deserialize)]
pub struct StartBackupExportRequest {
    pub include_sensitive: bool,
    pub password: Option<String>,
}

// Request to execute a restore
#[derive(Debug, Serialize, Deserialize)]
pub struct ExecuteRestoreRequest {
    pub password: Option<String>,
}

// Backup manifest for archive metadata
#[derive(Debug, Serialize, Deserialize)]
pub struct BackupManifest {
    pub version: String,
    pub created_at: String,
    pub nosdesk_version: String,
    pub include_sensitive: bool,
    pub tables: std::collections::HashMap<String, TableManifest>,
    pub files: FilesManifest,
    pub encryption: Option<EncryptionManifest>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TableManifest {
    pub count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FilesManifest {
    pub total_count: i64,
    pub total_size_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptionManifest {
    pub algorithm: String,
    pub kdf: String,
    pub salt: String,
    pub nonce: String,
}

// Restore preview response
#[derive(Debug, Serialize, Deserialize)]
pub struct RestorePreview {
    pub manifest: BackupManifest,
    pub has_encrypted_sensitive: bool,
    pub warnings: Vec<String>,
}

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
    pub devices: Vec<Device>,
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
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::user_groups)]
pub struct NewUserGroup {
    pub user_uuid: Uuid,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
}

// Device-Group junction table
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::device_groups)]
#[diesel(belongs_to(Group))]
#[diesel(belongs_to(Device))]
#[diesel(primary_key(device_id, group_id))]
pub struct DeviceGroup {
    pub device_id: i32,
    pub group_id: i32,
    pub created_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub external_source: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::device_groups)]
pub struct NewDeviceGroup {
    pub device_id: i32,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
    pub external_source: Option<String>,
}

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
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::category_group_visibility)]
pub struct NewCategoryGroupVisibility {
    pub category_id: i32,
    pub group_id: i32,
    pub created_by: Option<Uuid>,
}

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
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::documentation_collection_visibility)]
pub struct NewDocumentationCollectionVisibility {
    pub collection_id: i32,
    pub group_id: Option<i32>,
    pub created_by: Option<Uuid>,
    pub user_uuid: Option<Uuid>,
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

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Associations)]
#[diesel(table_name = crate::schema::documentation_page_embeddings)]
#[diesel(primary_key(source_page_id, target_page_id))]
#[diesel(belongs_to(DocumentationPage, foreign_key = source_page_id))]
pub struct DocumentationPageEmbedding {
    pub source_page_id: i32,
    pub target_page_id: i32,
    pub created_at: chrono::NaiveDateTime,
}

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

// ============================================================================
// Assignment Rules - Automatic Ticket Assignment
// ============================================================================

/// Assignment method enum - how tickets are assigned
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[derive(diesel::deserialize::FromSqlRow, diesel::expression::AsExpression)]
#[diesel(sql_type = crate::schema::sql_types::AssignmentMethod)]
pub enum AssignmentMethod {
    #[serde(rename = "direct_user")]
    DirectUser,
    #[serde(rename = "group_round_robin")]
    GroupRoundRobin,
    #[serde(rename = "group_random")]
    GroupRandom,
    #[serde(rename = "group_queue")]
    GroupQueue,
}

impl ToSql<crate::schema::sql_types::AssignmentMethod, Pg> for AssignmentMethod {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            AssignmentMethod::DirectUser => "direct_user",
            AssignmentMethod::GroupRoundRobin => "group_round_robin",
            AssignmentMethod::GroupRandom => "group_random",
            AssignmentMethod::GroupQueue => "group_queue",
        };
        out.write_all(s.as_bytes())?;
        Ok(IsNull::No)
    }
}

impl FromSql<crate::schema::sql_types::AssignmentMethod, Pg> for AssignmentMethod {
    fn from_sql(bytes: PgValue) -> deserialize::Result<Self> {
        match bytes.as_bytes() {
            b"direct_user" => Ok(AssignmentMethod::DirectUser),
            b"group_round_robin" => Ok(AssignmentMethod::GroupRoundRobin),
            b"group_random" => Ok(AssignmentMethod::GroupRandom),
            b"group_queue" => Ok(AssignmentMethod::GroupQueue),
            _ => Err("Unrecognized assignment method".into()),
        }
    }
}

impl std::fmt::Display for AssignmentMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            AssignmentMethod::DirectUser => "direct_user",
            AssignmentMethod::GroupRoundRobin => "group_round_robin",
            AssignmentMethod::GroupRandom => "group_random",
            AssignmentMethod::GroupQueue => "group_queue",
        };
        write!(f, "{s}")
    }
}

/// Core assignment rule configuration
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::assignment_rules)]
pub struct AssignmentRule {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub description: Option<String>,
    pub priority: i32,
    pub is_active: bool,
    pub method: AssignmentMethod,
    pub target_user_uuid: Option<Uuid>,
    pub target_group_id: Option<i32>,
    pub trigger_on_create: bool,
    pub trigger_on_category_change: bool,
    pub category_id: Option<i32>,
    pub conditions: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::assignment_rules)]
pub struct NewAssignmentRule {
    pub name: String,
    pub description: Option<String>,
    pub priority: i32,
    pub is_active: bool,
    pub method: AssignmentMethod,
    pub target_user_uuid: Option<Uuid>,
    pub target_group_id: Option<i32>,
    pub trigger_on_create: bool,
    pub trigger_on_category_change: bool,
    pub category_id: Option<i32>,
    pub conditions: Option<serde_json::Value>,
    pub created_by: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::assignment_rules)]
pub struct AssignmentRuleUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub priority: Option<i32>,
    pub is_active: Option<bool>,
    pub method: Option<AssignmentMethod>,
    pub target_user_uuid: Option<Option<Uuid>>,
    pub target_group_id: Option<Option<i32>>,
    pub trigger_on_create: Option<bool>,
    pub trigger_on_category_change: Option<bool>,
    pub category_id: Option<Option<i32>>,
    pub conditions: Option<serde_json::Value>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Round-robin and assignment state tracking
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::assignment_rule_state)]
#[diesel(belongs_to(AssignmentRule, foreign_key = rule_id))]
#[diesel(primary_key(rule_id))]
pub struct AssignmentRuleState {
    pub rule_id: i32,
    pub last_assigned_index: i32,
    pub total_assignments: i32,
    pub last_assigned_at: Option<NaiveDateTime>,
    pub last_assigned_user_uuid: Option<Uuid>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::assignment_rule_state)]
pub struct NewAssignmentRuleState {
    pub rule_id: i32,
    pub last_assigned_index: i32,
    pub total_assignments: i32,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::assignment_rule_state)]
pub struct AssignmentRuleStateUpdate {
    pub last_assigned_index: Option<i32>,
    pub total_assignments: Option<i32>,
    pub last_assigned_at: Option<NaiveDateTime>,
    pub last_assigned_user_uuid: Option<Uuid>,
}

/// Assignment audit log entry
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::assignment_log)]
#[diesel(belongs_to(AssignmentRule, foreign_key = rule_id))]
#[diesel(belongs_to(Ticket))]
pub struct AssignmentLog {
    pub id: i32,
    pub ticket_id: i32,
    pub rule_id: Option<i32>,
    pub trigger_type: String,
    pub previous_assignee_uuid: Option<Uuid>,
    pub new_assignee_uuid: Option<Uuid>,
    pub method: AssignmentMethod,
    pub context: Option<serde_json::Value>,
    pub assigned_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::assignment_log)]
pub struct NewAssignmentLog {
    pub ticket_id: i32,
    pub rule_id: Option<i32>,
    pub trigger_type: String,
    pub previous_assignee_uuid: Option<Uuid>,
    pub new_assignee_uuid: Option<Uuid>,
    pub method: AssignmentMethod,
    pub context: Option<serde_json::Value>,
}

/// Assignment rule with related data for API responses
#[derive(Debug, Serialize, Deserialize)]
pub struct AssignmentRuleWithDetails {
    #[serde(flatten)]
    pub rule: AssignmentRule,
    pub target_user: Option<UserInfoWithAvatar>,
    pub target_group: Option<Group>,
    pub category: Option<TicketCategory>,
    pub state: Option<AssignmentRuleState>,
}

/// Trigger types for assignment evaluation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssignmentTrigger {
    TicketCreated,
    CategoryChanged,
}

impl AssignmentTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssignmentTrigger::TicketCreated => "ticket_created",
            AssignmentTrigger::CategoryChanged => "category_changed",
        }
    }
}

/// Result of automatic assignment evaluation
#[derive(Debug, Clone)]
pub struct AssignmentResult {
    pub rule_id: i32,
    pub rule_name: String,
    pub assigned_user_uuid: Option<Uuid>,
    pub method: AssignmentMethod,
}

// ============================================================================
// Notification Models
// ============================================================================

/// Notification type definition
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Clone)]
#[diesel(table_name = crate::schema::notification_types)]
pub struct NotificationType {
    pub id: i32,
    pub code: String,
    pub name: String,
    pub description: Option<String>,
    pub category: String,
    pub default_channels: serde_json::Value,
    pub created_at: NaiveDateTime,
}

/// User notification preference
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::notification_preferences)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
#[diesel(belongs_to(NotificationType, foreign_key = notification_type_id))]
pub struct NotificationPreference {
    pub id: i32,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub channel: String,
    pub enabled: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::notification_preferences)]
pub struct NewNotificationPreference {
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub channel: String,
    pub enabled: bool,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::notification_preferences)]
pub struct NotificationPreferenceUpdate {
    pub enabled: Option<bool>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Persistent notification record
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::notifications)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
#[diesel(belongs_to(NotificationType, foreign_key = notification_type_id))]
pub struct Notification {
    pub id: i32,
    pub uuid: Uuid,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub channels_delivered: serde_json::Value,
    pub is_read: bool,
    pub read_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::notifications)]
pub struct NewNotification {
    pub uuid: Uuid,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub channels_delivered: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::notifications)]
pub struct NotificationUpdate {
    pub is_read: Option<bool>,
    pub read_at: Option<NaiveDateTime>,
    pub channels_delivered: Option<serde_json::Value>,
}

/// Rate limit tracking for email notifications
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations, Clone)]
#[diesel(table_name = crate::schema::notification_rate_limits)]
#[diesel(belongs_to(User, foreign_key = user_uuid))]
#[diesel(belongs_to(NotificationType, foreign_key = notification_type_id))]
pub struct NotificationRateLimit {
    pub id: i32,
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
    pub last_notified_at: NaiveDateTime,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::notification_rate_limits)]
pub struct NewNotificationRateLimit {
    pub user_uuid: Uuid,
    pub notification_type_id: i32,
    pub entity_type: String,
    pub entity_id: i32,
}

/// API response for notification preferences (grouped by type)
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationPreferenceResponse {
    pub notification_type: String,
    pub notification_name: String,
    pub description: Option<String>,
    pub category: String,
    pub channels: std::collections::HashMap<String, bool>,
}

/// API response for a notification
#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationResponse {
    pub id: i32,
    pub uuid: Uuid,
    pub notification_type: String,
    pub entity_type: String,
    pub entity_id: i32,
    pub title: String,
    pub body: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub is_read: bool,
    pub created_at: NaiveDateTime,
}

// ===== WEBHOOK MODELS =====

/// Webhook configuration (stored in database)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct Webhook {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<Option<String>>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: i32,
    pub disabled_reason: Option<String>,
}

/// New webhook for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct NewWebhook {
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<Option<String>>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_by: Option<Uuid>,
}

/// Webhook update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::webhooks)]
pub struct WebhookUpdate {
    pub name: Option<String>,
    pub url: Option<String>,
    pub secret: Option<String>,
    pub events: Option<Vec<Option<String>>>,
    pub enabled: Option<bool>,
    pub headers: Option<serde_json::Value>,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: Option<i32>,
    pub disabled_reason: Option<Option<String>>,
}

/// Webhook delivery record
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct WebhookDelivery {
    pub id: i32,
    pub uuid: Uuid,
    pub webhook_id: i32,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub request_headers: Option<serde_json::Value>,
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub response_headers: Option<serde_json::Value>,
    pub attempt_number: i32,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub next_retry_at: Option<NaiveDateTime>,
}

/// New webhook delivery for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct NewWebhookDelivery {
    pub webhook_id: i32,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub request_headers: Option<serde_json::Value>,
    pub attempt_number: i32,
}

/// Webhook delivery update
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::webhook_deliveries)]
pub struct WebhookDeliveryUpdate {
    pub response_status: Option<i32>,
    pub response_body: Option<String>,
    pub response_headers: Option<serde_json::Value>,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub next_retry_at: Option<Option<NaiveDateTime>>,
    pub attempt_number: Option<i32>,
}

// ===== WEBHOOK API TYPES =====

/// Request to create a webhook
#[derive(Debug, Deserialize)]
pub struct CreateWebhookRequest {
    pub name: String,
    pub url: String,
    pub events: Vec<String>,
    #[serde(default)]
    pub headers: Option<serde_json::Value>,
}

/// Request to update a webhook
#[derive(Debug, Deserialize)]
pub struct UpdateWebhookRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub events: Option<Vec<String>>,
    pub enabled: Option<bool>,
    #[serde(default)]
    pub headers: Option<serde_json::Value>,
    pub regenerate_secret: Option<bool>,
}

/// Webhook response (hides full secret)
#[derive(Debug, Serialize)]
pub struct WebhookResponse {
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret_preview: String,
    pub events: Vec<String>,
    pub enabled: bool,
    pub headers: Option<serde_json::Value>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub last_triggered_at: Option<NaiveDateTime>,
    pub failure_count: i32,
    pub disabled_reason: Option<String>,
}

impl Webhook {
    /// Returns a preview of the secret (first 12 chars + "...")
    pub fn secret_preview(&self) -> String {
        format!("{}...", self.secret.chars().take(12).collect::<String>())
    }
}

impl From<Webhook> for WebhookResponse {
    fn from(w: Webhook) -> Self {
        // Compute secret_preview before any moves
        let secret_preview = w.secret_preview();
        WebhookResponse {
            uuid: w.uuid,
            name: w.name,
            url: w.url,
            secret_preview,
            events: w.events.into_iter().flatten().collect(),
            enabled: w.enabled,
            headers: w.headers,
            created_at: w.created_at,
            updated_at: w.updated_at,
            last_triggered_at: w.last_triggered_at,
            failure_count: w.failure_count,
            disabled_reason: w.disabled_reason,
        }
    }
}

/// Webhook created response (shows full secret once)
#[derive(Debug, Serialize)]
pub struct WebhookCreatedResponse {
    pub uuid: Uuid,
    pub name: String,
    pub url: String,
    pub secret: String,
    pub events: Vec<String>,
}

/// Delivery history entry
#[derive(Debug, Serialize)]
pub struct WebhookDeliveryResponse {
    pub uuid: Uuid,
    pub event_type: String,
    pub response_status: Option<i32>,
    pub duration_ms: Option<i32>,
    pub error_message: Option<String>,
    pub delivered_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub attempt_number: i32,
}

// ===== PLUGIN SYSTEM TYPES =====

/// Plugin trust level
#[derive(Debug, Serialize, Deserialize, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum PluginTrustLevel {
    Official,
    Verified,
    #[default]
    Community,
}


impl std::fmt::Display for PluginTrustLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginTrustLevel::Official => write!(f, "official"),
            PluginTrustLevel::Verified => write!(f, "verified"),
            PluginTrustLevel::Community => write!(f, "community"),
        }
    }
}

impl std::str::FromStr for PluginTrustLevel {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "official" => Ok(PluginTrustLevel::Official),
            "verified" => Ok(PluginTrustLevel::Verified),
            "community" => Ok(PluginTrustLevel::Community),
            _ => Err(anyhow::anyhow!("Unknown trust level: {}", s)),
        }
    }
}

/// Installed plugin
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugins)]
pub struct Plugin {
    pub id: i32,
    pub uuid: Uuid,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: serde_json::Value,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub installed_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
    pub source: String,
    /// Base64 Ed25519 pubkey that signed this bundle. The current
    /// install pipeline always populates this (every install path
    /// goes through signature verification), so production rows
    /// have `Some`; the column is `Option` only to tolerate
    /// pre-signing-system rows in upgraded databases.
    pub signer_pubkey: Option<String>,
    /// Which authority chain recognised this signer: `nosdesk-root`
    /// | `verified-publisher` | `community-publisher` | `local` |
    /// `dev`. See `services::plugins::signing::sources`.
    pub signer_source: Option<String>,
    /// Full signature envelope captured at install time for audit.
    pub signature_metadata: Option<serde_json::Value>,
    /// Validated `icon.svg` bytes extracted from the signed zip at
    /// install time. Served verbatim from `GET /api/plugins/{uuid}/icon`.
    pub icon_svg: Option<Vec<u8>>,
    /// Lifecycle state. Stringly-typed in the DB (VARCHAR with a
    /// CHECK constraint) but parsed into the typed `PluginState`
    /// enum on read; consumers match exhaustively, eliminating
    /// the typo class that the constants module was prone to.
    pub state: PluginState,
    /// Bundle bytes stored inline. Replaces the previous on-disk
    /// uploads-volume staging so install becomes a single
    /// transactional write (DB row + bundle bytes commit together
    /// or both roll back). NULL only on legacy rows installed
    /// before this column existed; reinstall populates it. Capped
    /// at `install::MAX_BUNDLE_SIZE` (500 KB).
    pub bundle_js: Option<Vec<u8>>,
}

impl Plugin {
    /// True when the plugin is in the `installed` state (active +
    /// loaded). Replaces the old `enabled` boolean for callers that
    /// only need a yes/no view.
    pub fn is_active(&self) -> bool {
        matches!(self.state, PluginState::Installed)
    }
}

/// Lifecycle state of a plugin row. Stored as a `VARCHAR(32)` in
/// `plugins.state` with a CHECK constraint enforcing the allowlist;
/// the typed enum here is the canonical in-memory representation.
/// Custom Diesel `ToSql<Text>` / `FromSql<Text>` impls handle the
/// wire conversion. Adding a new variant means migrating the DB
/// CHECK constraint AND extending the exhaustive matches that
/// fall out elsewhere; the compiler points at every site.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    diesel::AsExpression,
    diesel::FromSqlRow,
    serde::Serialize,
)]
#[diesel(sql_type = diesel::sql_types::Text)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// Active. Bundle is served, components render, events dispatch.
    Installed,
    /// Admin paused. Bundle is NOT served, components don't render,
    /// but the row + plugin_data are intact and a flip back to
    /// `Installed` restores everything.
    Disabled,
    /// Trust-chain failure (signer revoked, signature mismatched on
    /// re-check). Refused for new use; existing data preserved for
    /// audit. Triggered by background revocation sweeps; never set
    /// by user action.
    Quarantined,
    /// Plugin was uninstalled via a manifest declaring
    /// `lifecycle.on_uninstall = preserve`. The row + plugin_data
    /// + collection rows are kept so a future reinstall of the same
    /// plugin name reattaches the data automatically. Bundle is
    /// removed from disk.
    Uninstalled,
}

impl PluginState {
    pub fn as_db_str(&self) -> &'static str {
        match self {
            PluginState::Installed => "installed",
            PluginState::Disabled => "disabled",
            PluginState::Quarantined => "quarantined",
            PluginState::Uninstalled => "uninstalled",
        }
    }

    pub fn from_db_str(s: &str) -> Result<Self, String> {
        match s {
            "installed" => Ok(PluginState::Installed),
            "disabled" => Ok(PluginState::Disabled),
            "quarantined" => Ok(PluginState::Quarantined),
            "uninstalled" => Ok(PluginState::Uninstalled),
            other => Err(format!("unknown plugin state {other:?}")),
        }
    }
}

impl std::fmt::Display for PluginState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_db_str())
    }
}

impl<'de> serde::Deserialize<'de> for PluginState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        PluginState::from_db_str(&s).map_err(serde::de::Error::custom)
    }
}

impl diesel::serialize::ToSql<diesel::sql_types::Text, diesel::pg::Pg> for PluginState {
    fn to_sql<'b>(
        &'b self,
        out: &mut diesel::serialize::Output<'b, '_, diesel::pg::Pg>,
    ) -> diesel::serialize::Result {
        <str as diesel::serialize::ToSql<diesel::sql_types::Text, diesel::pg::Pg>>::to_sql(
            self.as_db_str(),
            &mut out.reborrow(),
        )
    }
}

impl diesel::deserialize::FromSql<diesel::sql_types::Text, diesel::pg::Pg> for PluginState {
    fn from_sql(
        bytes: <diesel::pg::Pg as diesel::backend::Backend>::RawValue<'_>,
    ) -> diesel::deserialize::Result<Self> {
        let s = <String as diesel::deserialize::FromSql<
            diesel::sql_types::Text,
            diesel::pg::Pg,
        >>::from_sql(bytes)?;
        PluginState::from_db_str(&s).map_err(|e| e.into())
    }
}

/// New plugin for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugins)]
pub struct NewPlugin {
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: serde_json::Value,
    /// Initial lifecycle state, almost always `PluginState::Installed`.
    pub state: PluginState,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub source: String,
    pub signer_pubkey: Option<String>,
    pub signer_source: Option<String>,
    pub signature_metadata: Option<serde_json::Value>,
    pub icon_svg: Option<Vec<u8>>,
}

/// Plugin update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugins)]
pub struct PluginUpdate {
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub manifest: Option<serde_json::Value>,
    pub state: Option<PluginState>,
    pub trust_level: Option<String>,
    pub signer_pubkey: Option<String>,
    pub signer_source: Option<String>,
    pub signature_metadata: Option<serde_json::Value>,
    /// `Some(Some(bytes))` writes the icon, `Some(None)` clears it,
    /// `None` leaves it alone. Distinct from the other signer
    /// fields' `Option<T>` because clearing-to-NULL on update is
    /// realistic here (a new plugin version might drop its icon).
    pub icon_svg: Option<Option<Vec<u8>>>,
}

/// Plugin bundle update changeset. `bundle_js` carries the raw
/// bytes; `bundle_hash`/`size`/`uploaded_at` are denormalised
/// metadata kept in sync. All four fields are written in the
/// same row update so they can't drift.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::plugins)]
pub struct PluginBundleUpdate {
    pub bundle_js: Option<Vec<u8>>,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
}

/// Publisher whose Ed25519 pubkey is trusted to sign `verified` or
/// `community` tier plugins. Populated from the signed nosdesk.com
/// keylist; revocation is expressed by setting `revoked_at`.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_trusted_publishers)]
pub struct TrustedPublisher {
    pub id: i32,
    pub pubkey: String,
    pub display_name: String,
    pub tier: String,
    pub website: Option<String>,
    pub added_at: NaiveDateTime,
    pub revoked_at: Option<NaiveDateTime>,
}

#[derive(Debug, Insertable, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_trusted_publishers)]
pub struct NewTrustedPublisher {
    pub pubkey: String,
    pub display_name: String,
    pub tier: String,
    pub website: Option<String>,
    pub revoked_at: Option<NaiveDateTime>,
}

/// Single-row table holding the instance's local Ed25519 signing
/// keypair. `encrypted_sk` is AES-256-GCM ciphertext under the same
/// key material as MFA secrets (see `utils::encryption`).
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::plugin_local_signing_key)]
pub struct LocalSigningKey {
    pub id: i32,
    pub pubkey: String,
    pub encrypted_sk: Vec<u8>,
    pub fingerprint: String,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_local_signing_key)]
pub struct NewLocalSigningKey {
    pub id: i32,
    pub pubkey: String,
    pub encrypted_sk: Vec<u8>,
    pub fingerprint: String,
}

/// Single-row table that persists the anti-rollback counters from
/// the last registry snapshot the instance accepted. Durability
/// across restarts is load-bearing: without it, an attacker who
/// forces a restart could race the first boot fetch with an older
/// signed snapshot.
#[derive(Debug, Clone, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::plugin_registry_state)]
pub struct PluginRegistryState {
    pub id: i32,
    pub publishers_version: i64,
    pub index_version: i64,
    pub last_fetched_at: Option<NaiveDateTime>,
    pub last_fetch_error: Option<String>,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, AsChangeset, Default)]
#[diesel(table_name = crate::schema::plugin_registry_state)]
pub struct PluginRegistryStateUpdate {
    pub publishers_version: Option<i64>,
    pub index_version: Option<i64>,
    pub last_fetched_at: Option<Option<NaiveDateTime>>,
    pub last_fetch_error: Option<Option<String>>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Plugin data type - settings (admin-configured) or storage (plugin-managed)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginDataType {
    Setting,
    Storage,
}

impl std::fmt::Display for PluginDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PluginDataType::Setting => write!(f, "setting"),
            PluginDataType::Storage => write!(f, "storage"),
        }
    }
}

/// Consolidated plugin data (settings and storage in one table)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_data)]
pub struct PluginData {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub data_type: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl PluginData {
    /// Check if this is a setting (admin-configured)
    pub fn is_setting(&self) -> bool {
        self.data_type == "setting"
    }

    /// Check if this is storage (plugin-managed)
    pub fn is_storage(&self) -> bool {
        self.data_type == "storage"
    }
}

/// New plugin data for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_data)]
pub struct NewPluginData {
    pub plugin_id: i32,
    pub data_type: String,
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
}

/// Plugin data update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_data)]
pub struct PluginDataUpdate {
    pub value: Option<Option<serde_json::Value>>,
}

/// Plugin activity log entry
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::plugin_activity)]
pub struct PluginActivity {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
    pub created_at: NaiveDateTime,
}

/// New plugin activity entry for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_activity)]
pub struct NewPluginActivity {
    pub plugin_id: i32,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
}

// ===== PLUGIN API TYPES =====

/// Plugin manifest structure (matches frontend manifest.json format).
///
/// `deny_unknown_fields` is load-bearing: every field a plugin
/// declares must be one this binary understands, otherwise we fail
/// closed at install. Combined with `manifest_version`, that lets
/// us evolve the schema without ambiguity. v2 plugins declare
/// `manifest_version: 2` and the parser dispatches to a different
/// struct; v1 plugins are forever interpreted by the rules below.
///
/// Trust-affecting fields (`name`, `permissions`, `engines`, etc.)
/// are part of the canonical archive digest because they live in
/// `manifest.json`, so the signer commits to all of them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    /// MUST be 1 for the schema described here. Future bumps go to
    /// 2/3/etc. Validators dispatch on this.
    pub manifest_version: u32,

    /// Stable plugin identifier. Lowercase ASCII letters, digits,
    /// and hyphens. Used as the DB key and display URL slug.
    pub name: String,

    /// User-facing name. Free-form, locale-neutral.
    #[serde(rename = "displayName")]
    pub display_name: String,

    /// SemVer string (e.g. "2.1.0"). Compared between installs to
    /// detect upgrades.
    pub version: String,

    /// Short user-facing description. Free-form.
    pub description: Option<String>,

    /// SPDX license identifier (e.g. "MIT", "Apache-2.0",
    /// "BUSL-1.1"). Optional but strongly recommended.
    pub license: Option<String>,

    /// Author display name. For non-official plugins (verified /
    /// community tier), the install pipeline asserts this matches
    /// the publishers.json entry for the signing key. Local-tier
    /// installs skip the check.
    pub author: Option<String>,

    /// Source repository URL.
    pub repository: Option<String>,

    /// Plugin homepage / documentation URL.
    pub homepage: Option<String>,

    /// Issue tracker URL. Distinct from `repository` because some
    /// plugins host code on one host and bugs on another (e.g.
    /// Bugzilla, Linear, internal tracker).
    pub bugs: Option<String>,

    /// Support contact: email or URL. Surfaced on the registry
    /// browse UI so users know where to ask for help. Format
    /// validated lightly: must contain `@` or look like a URL.
    pub support_contact: Option<String>,

    /// Engine compatibility. Plugin will be refused if the
    /// instance doesn't satisfy these constraints.
    pub engines: PluginEngines,

    /// Other plugins this one depends on. Each value is a semver
    /// requirement against the dep's `version`. The install
    /// pipeline refuses if a declared dep isn't installed; it does
    /// NOT auto-install transitively (registry-driven install
    /// surfaces the prompt for the operator). Reserved shape for
    /// future inter-plugin APIs and ordering guarantees; even
    /// without those, having the declaration prevents silent
    /// "plugin assumes peer is present" footguns.
    #[serde(default)]
    pub dependencies: std::collections::BTreeMap<String, String>,

    /// Discovery taxonomy for the registry browse UI. Values are
    /// validated against an allowlist of known categories.
    #[serde(default)]
    pub categories: Vec<String>,

    /// Free-form discovery tags. No allowlist; the registry build
    /// can lowercase + dedupe but doesn't reject unknowns.
    #[serde(default)]
    pub tags: Vec<String>,

    /// Paths inside the zip pointing at PNG/SVG screenshots for
    /// the registry browse UI. Validated at install.
    #[serde(default)]
    pub screenshots: Vec<String>,

    /// Capability grants the plugin requests. Parsed at manifest
    /// load time into typed `Permission` values; unknown or
    /// malformed entries fail deserialisation, so consumers past
    /// this point never see raw permission strings.
    #[serde(default)]
    pub permissions: Vec<crate::services::plugins::types::Permission>,

    /// Components this plugin contributes. Keyed by component name
    /// (used as the entry-point key in the bundle's default export).
    #[serde(default)]
    pub components: std::collections::BTreeMap<String, PluginComponentConfig>,

    /// Events the plugin subscribes to. Validated against an
    /// allowlist; unknown events refused.
    #[serde(default)]
    pub events: Vec<String>,

    /// Plugin-defined settings rendered in the admin UI.
    #[serde(default)]
    pub settings: Vec<PluginSettingDefinition>,

    /// Plugin-owned collections. Each carries its own
    /// `schema_version` so future migrations can be expressed.
    #[serde(default)]
    pub collections: std::collections::BTreeMap<String, CollectionDefinition>,

    /// Declarative auth configuration: maps exact hostnames to
    /// auth strategies the proxy injects automatically. Wildcards
    /// are NOT permitted as auth keys (a future schema bump can
    /// loosen this if a real use case appears); each declared host
    /// must be covered by at least one `network:` permission.
    #[serde(default)]
    pub auth: std::collections::BTreeMap<crate::services::plugins::types::Host, PluginAuthConfig>,

    /// Lifecycle policy declarations. Default cascades plugin data
    /// on uninstall; plugins that store user-meaningful work
    /// should declare `on_uninstall: "preserve"`.
    #[serde(default)]
    pub lifecycle: PluginLifecyclePolicy,

    /// Palette-triggerable actions the plugin contributes. Reserved
    /// in v1: declared, validated, but the runtime palette is not
    /// yet implemented. Refused at install if non-empty until the
    /// dispatcher lands.
    #[serde(default)]
    pub commands: Vec<PluginCommandDefinition>,

    /// Menu contributions, keyed by menu identifier (e.g.
    /// `ticket-context`). Reserved in v1.
    #[serde(default)]
    pub menus: std::collections::BTreeMap<String, Vec<PluginMenuItem>>,

    /// URL-handler claims, e.g. `nosdesk://plugin/<plugin-name>/...`
    /// patterns this plugin owns. Reserved in v1.
    #[serde(default)]
    pub url_handlers: Vec<PluginUrlHandler>,

    /// Forward-compat bucket for typed inter-plugin exports.
    /// Modelled as a `BTreeMap<String, serde_json::Value>` so the
    /// same `is_empty()` predicate gates every reserved field;
    /// previously this was `serde_json::Value` with an
    /// `is_null()` check that let `{}` slip through. v1 refuses
    /// any non-empty value at install.
    #[serde(default)]
    pub extensions: std::collections::BTreeMap<String, serde_json::Value>,
}

/// Engine compatibility constraints. Both values are required.
/// Refused at install when not satisfied.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginEngines {
    /// SemVer requirement against the running Nosdesk version
    /// (e.g. ">=1.5.0", "^2.0", "1.4.x").
    pub nosdesk: String,

    /// Plugin runtime API major version the plugin was built
    /// against. Currently must be "1". The runtime exposes the
    /// supported version range to plugin code via `api.version`.
    pub plugin_api: String,
}

/// Declarative lifecycle policy. v1 honours `on_uninstall` only;
/// future fields here can land without breaking older manifests
/// because new defaults are added with `#[serde(default)]`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PluginLifecyclePolicy {
    /// What happens to plugin-owned data when the plugin is
    /// uninstalled. `cascade` deletes all `plugin_data` and
    /// `plugin_collection_rows` for the plugin; `preserve` keeps
    /// them, supporting reinstall-without-data-loss flows.
    #[serde(default)]
    pub on_uninstall: PluginUninstallPolicy,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginUninstallPolicy {
    #[default]
    Cascade,
    Preserve,
}

/// Palette command contributed by a plugin. Reserved for the
/// future command-palette dispatcher; v1 install refuses non-empty
/// `commands` arrays.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginCommandDefinition {
    /// Stable namespaced identifier, e.g. `github.sync`.
    pub id: String,
    /// User-facing label.
    pub title: String,
    /// Optional context filter (matches `KNOWN_CONTEXTS`).
    pub when: Option<String>,
}

/// Menu item contributed by a plugin. Reserved in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginMenuItem {
    /// Command id this entry invokes.
    pub command: String,
    /// Optional grouping hint (e.g. `integrations`).
    pub group: Option<String>,
}

/// URL handler claim, e.g. `nosdesk://plugin/<plugin-name>/<pattern>`.
/// Reserved in v1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginUrlHandler {
    /// Glob-like pattern under the plugin's namespace, e.g. `link/*`.
    pub pattern: String,
    /// Command id to invoke when matched.
    pub command: Option<String>,
}

/// Authentication configuration for a specific domain/host pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PluginAuthConfig {
    /// Authorization: Bearer <secret_value>
    Bearer {
        secret: String,
    },
    /// Authorization: Basic base64(username:password)
    Basic {
        username_secret: String,
        password_secret: String,
    },
    /// Custom header with secret value (e.g. X-API-Key)
    ApiKey {
        header: String,
        secret: String,
    },
    /// OAuth2 Client Credentials flow: exchanges client_id + client_secret for a bearer token
    Oauth2ClientCredentials {
        token_url: String,
        client_id_secret: String,
        client_secret_secret: String,
    },
}

/// Plugin component configuration in manifest. The `kind` field
/// reserves space for future component shapes (settings tabs,
/// admin pages, background workers, webhook handlers); v1 only
/// implements `slot`-kind components, but the field is required
/// so future plugins can be expressed without a manifest version
/// bump.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginComponentConfig {
    /// What this component IS. Defaults to `slot` for backward
    /// readability; future kinds expand the allowed set.
    #[serde(default)]
    pub kind: PluginComponentKind,

    /// For `kind = slot`: the slot identifier (validated against
    /// allowlist). For other kinds, semantics differ.
    pub slot: String,

    /// Entry-point key inside the plugin's bundle default export.
    pub entry: String,

    /// Context types the component receives at render time
    /// (e.g. `["ticket"]`). Validated against allowlist.
    #[serde(default)]
    pub context: Vec<String>,

    pub label: Option<String>,
    pub icon: Option<String>,
    pub action: Option<PluginComponentAction>,
}

/// Component kind. Only `Slot` is implemented in v1; the others
/// are reserved enum variants so a future plugin declaring
/// `kind: "admin_page"` is parseable today (and rejected at
/// install with a clear "kind not yet supported" error rather
/// than a parse failure that looks like a bug).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginComponentKind {
    #[default]
    Slot,
    /// Reserved: a settings panel rendered inside the plugin's
    /// settings dialog instead of the declarative settings form.
    Settings,
    /// Reserved: a full admin page mounted at /admin/plugins/<name>/...
    AdminPage,
    /// Reserved: a backend worker invoked on a schedule.
    Worker,
    /// Reserved: a webhook handler matching a registered path.
    Webhook,
}

impl PluginComponentKind {
    /// Wire-format string for this kind (matches the serde
    /// `rename_all = "snake_case"`). Used by validators when
    /// reporting "kind X is not supported" without depending on
    /// `serde_json` to round-trip.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Slot => "slot",
            Self::Settings => "settings",
            Self::AdminPage => "admin_page",
            Self::Worker => "worker",
            Self::Webhook => "webhook",
        }
    }
}

/// Plugin component action for unified "+ Add" menu
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginComponentAction {
    pub label: String,
}

/// Plugin setting definition in manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingDefinition {
    pub key: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    pub label: String,
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub default: Option<serde_json::Value>,
    /// Storage scope. `global` (default) means one value per
    /// instance; `user` means one value per logged-in user
    /// (e.g. each user's own GitHub PAT). Reserved in v1: the
    /// install validator refuses `user`-scoped settings until the
    /// per-user storage layer lands. Declaring the field now
    /// prevents the storage layout from being implicitly committed
    /// to "everything global" by the first wave of plugins.
    #[serde(default)]
    pub scope: PluginSettingScope,
    #[serde(default)]
    pub options: Option<Vec<PluginSettingOption>>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PluginSettingScope {
    #[default]
    Global,
    User,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSettingOption {
    pub value: String,
    pub label: String,
}

/// Request to toggle a plugin's lifecycle state. The endpoint
/// only honours the enabled-toggle (Installed <-> Disabled);
/// manifest edits used to be allowed here but were removed
/// because they bypassed signature reverification: an admin
/// could rewrite a verified plugin's stored manifest while the
/// signer fields kept claiming the original signer signed it.
/// Manifest changes now flow through the signed install paths
/// (zip upload, registry install) which re-verify end-to-end.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdatePluginRequest {
    pub enabled: Option<bool>,
}

/// Plugin response (for API)
#[derive(Debug, Serialize)]
pub struct PluginResponse {
    pub uuid: Uuid,
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub manifest: PluginManifest,
    /// Lifecycle state. Serialises to one of `installed` /
    /// `disabled` / `quarantined` / `uninstalled` on the wire.
    /// The frontend toggles render rows where this is `installed`
    /// or `disabled`; the others are rendered as read-only audit
    /// entries.
    pub state: PluginState,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub installed_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
    pub source: String,
}

impl Plugin {
    /// Parse the manifest JSON into a PluginManifest struct
    pub fn parse_manifest(&self) -> Result<PluginManifest, serde_json::Error> {
        serde_json::from_value(self.manifest.clone())
    }
}

impl TryFrom<Plugin> for PluginResponse {
    type Error = serde_json::Error;

    fn try_from(p: Plugin) -> Result<Self, Self::Error> {
        let manifest = p.parse_manifest()?;
        Ok(PluginResponse {
            uuid: p.uuid,
            name: p.name,
            display_name: p.display_name,
            version: p.version,
            description: p.description,
            manifest,
            state: p.state,
            trust_level: p.trust_level,
            installed_by: p.installed_by,
            installed_at: p.installed_at,
            updated_at: p.updated_at,
            bundle_hash: p.bundle_hash,
            bundle_size: p.bundle_size,
            bundle_uploaded_at: p.bundle_uploaded_at,
            source: p.source,
        })
    }
}

/// Plugin setting response (hides secret values)
#[derive(Debug, Serialize)]
pub struct PluginSettingResponse {
    pub key: String,
    pub value: Option<serde_json::Value>,
    pub is_secret: bool,
}

impl From<PluginData> for PluginSettingResponse {
    fn from(d: PluginData) -> Self {
        PluginSettingResponse {
            key: d.key,
            // Hide secret values in response
            value: if d.is_secret { None } else { d.value },
            is_secret: d.is_secret,
        }
    }
}

/// Request to set a plugin setting or storage
#[derive(Debug, Deserialize)]
pub struct SetPluginDataRequest {
    pub key: String,
    pub value: serde_json::Value,
}

/// Plugin storage response
#[derive(Debug, Serialize)]
pub struct PluginStorageResponse {
    pub key: String,
    pub value: Option<serde_json::Value>,
}

impl From<PluginData> for PluginStorageResponse {
    fn from(d: PluginData) -> Self {
        PluginStorageResponse {
            key: d.key,
            value: d.value,
        }
    }
}

/// Plugin activity response
#[derive(Debug, Serialize)]
pub struct PluginActivityResponse {
    pub uuid: Uuid,
    pub action: String,
    pub details: Option<serde_json::Value>,
    pub user_uuid: Option<Uuid>,
    pub created_at: NaiveDateTime,
}

impl From<PluginActivity> for PluginActivityResponse {
    fn from(a: PluginActivity) -> Self {
        PluginActivityResponse {
            uuid: a.uuid,
            action: a.action,
            details: a.details,
            user_uuid: a.user_uuid,
            created_at: a.created_at,
        }
    }
}

/// Request for proxied external API calls
#[derive(Debug, Deserialize)]
pub struct PluginProxyRequest {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    pub headers: Option<std::collections::HashMap<String, String>>,
    pub body: Option<serde_json::Value>,
    /// Body encoding: "json" (default) or "form" (application/x-www-form-urlencoded)
    pub content_type: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

/// Response from proxied external API call
#[derive(Debug, Serialize)]
pub struct PluginProxyResponse {
    pub status: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Option<serde_json::Value>,
}

// ===== PLUGIN COLLECTION TYPES =====

/// Collection field definition in plugin manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionFieldDefinition {
    #[serde(rename = "type")]
    pub field_type: String,
    pub label: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub reference: Option<String>,
}

/// Collection definition in plugin manifest. `schema_version` is
/// required so future plugin versions can express migrations
/// (rename, drop, retype a field) without losing data. v1
/// recognises only schema_version 1.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CollectionDefinition {
    pub schema_version: u32,
    pub label: Option<String>,
    pub fields: std::collections::HashMap<String, CollectionFieldDefinition>,
}

/// Plugin collection schema (DB row)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
#[diesel(belongs_to(Plugin))]
pub struct PluginCollectionSchema {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// New collection schema for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
pub struct NewPluginCollectionSchema {
    pub plugin_id: i32,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
}

/// Collection schema update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_collection_schemas)]
pub struct PluginCollectionSchemaUpdate {
    pub schema: Option<serde_json::Value>,
    pub version: Option<i32>,
}

/// Plugin collection row (DB row)
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
#[diesel(belongs_to(PluginCollectionSchema, foreign_key = schema_id))]
pub struct PluginCollectionRow {
    pub id: i32,
    pub uuid: Uuid,
    pub plugin_id: i32,
    pub schema_id: i32,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

/// New collection row for insertion
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
pub struct NewPluginCollectionRow {
    pub plugin_id: i32,
    pub schema_id: i32,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
}

/// Collection row update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugin_collection_rows)]
pub struct PluginCollectionRowUpdate {
    pub data: Option<serde_json::Value>,
}

// ===== COLLECTION API TYPES =====

/// Query params for listing collection rows
#[derive(Debug, Deserialize)]
pub struct CollectionQueryParams {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub filter: Option<String>,
    pub sort_by: Option<String>,
    pub sort_order: Option<String>,
}

/// Request to create a collection row
#[derive(Debug, Deserialize)]
pub struct CreateCollectionRowRequest {
    pub data: serde_json::Value,
}

/// Request to update a collection row
#[derive(Debug, Deserialize)]
pub struct UpdateCollectionRowRequest {
    pub data: serde_json::Value,
}

/// Collection row API response
#[derive(Debug, Serialize)]
pub struct CollectionRowResponse {
    pub uuid: Uuid,
    pub data: serde_json::Value,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

impl From<PluginCollectionRow> for CollectionRowResponse {
    fn from(row: PluginCollectionRow) -> Self {
        CollectionRowResponse {
            uuid: row.uuid,
            data: row.data,
            created_by: row.created_by,
            created_at: row.created_at,
            updated_at: row.updated_at,
        }
    }
}

/// Paginated collection rows response
#[derive(Debug, Serialize)]
pub struct CollectionListResponse {
    pub rows: Vec<CollectionRowResponse>,
    pub total: i64,
}

/// Collection schema API response
#[derive(Debug, Serialize)]
pub struct CollectionSchemaResponse {
    pub uuid: Uuid,
    pub collection_name: String,
    pub schema: serde_json::Value,
    pub version: i32,
    pub row_count: i64,
}

// ============================================================================
// Channels — multi-channel message ingestion framework
// ============================================================================
//
// See services/channels/mod.rs for the adapter trait hierarchy and event
// shapes; these structs are the persisted representations. The tables
// model N channel instances from day one even though phase 1 ships a
// single-mailbox admin UI.

/// Direction of a [`ChannelMessage`]. Stored as a string in the DB so new
/// variants don't require schema churn; validated by a CHECK constraint.
pub const CHANNEL_DIRECTION_INBOUND: &str = "inbound";
pub const CHANNEL_DIRECTION_OUTBOUND: &str = "outbound";

/// Credential-type tags stored on [`ChannelCredential::credential_type`].
/// Not an enum because new providers (Slack, Teams, Discord) each bring
/// their own credential kinds — keeping this as a string keeps the schema
/// open for extension without migration.
pub const CRED_TYPE_IMAP_PASSWORD: &str = "imap_password";

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::channels)]
pub struct Channel {
    pub id: i32,
    pub provider: String,
    pub name: String,
    pub enabled: bool,
    pub config: serde_json::Value,
    pub runtime_state: serde_json::Value,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub last_polled_at: Option<NaiveDateTime>,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::channels)]
pub struct NewChannel {
    pub provider: String,
    pub name: String,
    pub enabled: bool,
    pub config: serde_json::Value,
}

/// Partial update to an existing channel. `Option<Option<T>>` fields use
/// `Some(None)` to explicitly clear; plain `None` means "don't change."
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::channels)]
pub struct ChannelUpdate {
    pub provider: Option<String>,
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub config: Option<serde_json::Value>,
    pub runtime_state: Option<serde_json::Value>,
    pub last_polled_at: Option<Option<NaiveDateTime>>,
    pub updated_at: Option<NaiveDateTime>,
}

/// Encrypted secret associated with a channel. The plaintext value never
/// leaves `utils::encryption`; this struct carries only the ciphertext.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::channel_credentials)]
#[diesel(belongs_to(Channel))]
pub struct ChannelCredential {
    pub id: i32,
    pub channel_id: i32,
    pub credential_type: String,
    pub encrypted_value: String,
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::channel_credentials)]
pub struct NewChannelCredential {
    pub channel_id: i32,
    pub credential_type: String,
    pub encrypted_value: String,
    pub expires_at: Option<NaiveDateTime>,
}

/// Ledger row — one per inbound or outbound message through a channel.
/// Used for dedup (unique on `channel_id, external_id, direction`),
/// thread resolution (lookup by `external_id`), and audit.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::channel_messages)]
#[diesel(belongs_to(Channel))]
#[diesel(belongs_to(Ticket))]
pub struct ChannelMessage {
    pub id: i64,
    pub channel_id: i32,
    pub external_id: String,
    pub direction: String,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub in_reply_to: Option<String>,
    pub from_address: Option<String>,
    pub author_user_uuid: Option<Uuid>,
    pub raw_metadata: Option<serde_json::Value>,
    pub received_at: NaiveDateTime,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::channel_messages)]
pub struct NewChannelMessage {
    pub channel_id: i32,
    pub external_id: String,
    pub direction: String,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub in_reply_to: Option<String>,
    pub from_address: Option<String>,
    pub author_user_uuid: Option<Uuid>,
    pub raw_metadata: Option<serde_json::Value>,
}
// ---------- Canned responses ----------

/// Reusable reply template that techs can pull into the ticket
/// composer with one click. Shared across the team (not per-user);
/// `created_by` is informational.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct CannedResponse {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub created_by: Option<Uuid>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
}

#[derive(Debug, Insertable, Deserialize)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct NewCannedResponse {
    pub title: String,
    pub body: String,
    pub created_by: Option<Uuid>,
}

/// Partial-update payload. `Option<T>` fields leave the column
/// untouched when `None`.
#[derive(Debug, Default, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::canned_responses)]
pub struct CannedResponseUpdate {
    pub title: Option<String>,
    pub body: Option<String>,
    pub updated_at: Option<NaiveDateTime>,
}
