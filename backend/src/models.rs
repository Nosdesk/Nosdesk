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
#[allow(dead_code)]
fn serialize_uuid_as_string<S>(uuid: &Uuid, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&uuid.to_string())
}

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

impl ToSql<crate::schema::sql_types::TicketStatus, Pg> for TicketStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            TicketStatus::Open => "open",
            TicketStatus::InProgress => "in-progress",
            TicketStatus::Closed => "closed",
        };
        out.write_all(s.as_bytes())?;
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

impl ToSql<crate::schema::sql_types::TicketPriority, Pg> for TicketPriority {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            TicketPriority::Low => "low",
            TicketPriority::Medium => "medium",
            TicketPriority::High => "high",
        };
        out.write_all(s.as_bytes())?;
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

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
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
    pub category_id: Option<Option<i32>>,
}

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
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

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
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
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::comments)]
pub struct NewComment {
    pub content: String,
    pub ticket_id: i32,
    pub user_uuid: Uuid,
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

#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable, Associations)]
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
}

impl ToSql<crate::schema::sql_types::DocumentationStatus, Pg> for DocumentationStatus {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            DocumentationStatus::Draft => "draft",
            DocumentationStatus::Published => "published",
            DocumentationStatus::Archived => "archived",
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
    pub slug: Option<String>,
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

impl ToSql<crate::schema::sql_types::UserRole, Pg> for UserRole {
    fn to_sql<'b>(&'b self, out: &mut Output<'b, '_, Pg>) -> serialize::Result {
        let s = match *self {
            UserRole::Admin => "admin",
            UserRole::Technician => "technician",
            UserRole::User => "user",
        };
        out.write_all(s.as_bytes())?;
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
#[derive(Debug, Serialize, Deserialize, Identifiable, Queryable)]
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
}

// User info for comments - minimal user data to include with comments
#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub uuid: Uuid,
    pub name: String,
}

// Enhanced UserInfo with avatar data for efficient frontend display
#[derive(Debug, Serialize, Deserialize)]
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
    pub scope: String, // Token scope: "full" for normal sessions, "mfa_recovery" for limited MFA management
    pub exp: usize,   // Expiration time
    pub iat: usize,   // Issued at
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

// Add the ArticleContentChunk struct for handling chunked article content
#[derive(Deserialize, Debug)]
#[allow(dead_code)]
pub struct ArticleContentChunk {
    pub chunk_index: i32,
    pub total_chunks: usize,
    pub is_last_chunk: bool,
    pub content: String,
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
    pub slug: Option<String>,
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

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
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
    pub slug: Option<String>,
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
    #[allow(dead_code)]
    pub fn from_complete_ticket(
        complete_ticket: CompleteTicket,
        requester_name: String,
        assignee_name: String,
    ) -> Self {
        Self {
            id: complete_ticket.ticket.id,
            title: complete_ticket.ticket.title,
            status: complete_ticket.ticket.status,
            priority: complete_ticket.ticket.priority,
            requester: requester_name,
            assignee: assignee_name,
            created: complete_ticket.ticket.created_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            modified: complete_ticket.ticket.updated_at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string(),
            devices: complete_ticket.devices,
            comments: complete_ticket.comments,
            article_content: complete_ticket.article_content,
            linked_tickets: complete_ticket.linked_tickets,
            projects: complete_ticket.projects,
        }
    }
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
    pub session_token: String,
    pub user_uuid: Uuid,
    pub device_name: Option<String>,
    pub ip_address: Option<ipnetwork::IpNetwork>,
    pub user_agent: Option<String>,
    pub location: Option<String>,
    pub created_at: chrono::NaiveDateTime,
    pub last_active: chrono::NaiveDateTime,
    pub expires_at: chrono::NaiveDateTime,
    pub is_current: bool,
}

/// New active session for creation
#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::active_sessions)]
pub struct NewActiveSession {
    pub session_token: String,
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
}

/// New refresh token for creation
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::refresh_tokens)]
pub struct NewRefreshToken {
    pub token_hash: String,
    pub user_uuid: Uuid,
    pub expires_at: chrono::NaiveDateTime,
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

/// Type alias for Results in security operations using anyhow for applications
#[allow(dead_code)]
pub type SecurityResult<T> = anyhow::Result<T>;

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

/// Request to initiate MFA reset
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaResetRequest {
    pub email: String,
    pub password: String,
}

/// Response for MFA reset initiation
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaResetResponse {
    pub message: String,
    pub token_id: Option<String>, // Only for admin users
    pub requires_admin_approval: bool,
}

/// Request to complete MFA reset
#[derive(Debug, Serialize, Deserialize)]
pub struct MfaResetCompleteRequest {
    pub token: String,
    pub email_code: Option<String>, // Email verification code
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
}

#[derive(Debug, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::site_settings)]
pub struct UpdateSiteSettings {
    pub app_name: Option<String>,
    pub logo_url: Option<Option<String>>,
    pub logo_light_url: Option<Option<String>>,
    pub favicon_url: Option<Option<String>>,
    pub primary_color: Option<Option<String>>,
    pub updated_by: Option<Uuid>,
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

// Group with member count for list views
#[derive(Debug, Serialize, Deserialize)]
pub struct GroupWithMemberCount {
    #[serde(flatten)]
    pub group: Group,
    pub member_count: i64,
    pub device_count: i64,
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
    pub enabled: bool,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub installed_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
    pub source: String,
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
    pub enabled: bool,
    pub trust_level: String,
    pub installed_by: Option<Uuid>,
    pub source: String,
}

/// Plugin update changeset
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::plugins)]
pub struct PluginUpdate {
    pub display_name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub manifest: Option<serde_json::Value>,
    pub enabled: Option<bool>,
    pub trust_level: Option<String>,
}

/// Plugin bundle update changeset
#[derive(Debug, AsChangeset)]
#[diesel(table_name = crate::schema::plugins)]
pub struct PluginBundleUpdate {
    pub bundle_hash: Option<String>,
    pub bundle_size: Option<i32>,
    pub bundle_uploaded_at: Option<NaiveDateTime>,
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

/// Plugin manifest structure (matches frontend manifest.json format)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub version: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub author: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    #[serde(default)]
    pub components: std::collections::HashMap<String, PluginComponentConfig>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub settings: Vec<PluginSettingDefinition>,
}

/// Plugin component configuration in manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginComponentConfig {
    pub slot: String,
    pub entry: String,
    #[serde(default)]
    pub context: Vec<String>,
    pub label: Option<String>,
    pub icon: Option<String>,
}

/// Plugin setting definition in manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginSettingDefinition {
    pub key: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    pub label: String,
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    pub default: Option<serde_json::Value>,
}

/// Request to install a plugin
#[derive(Debug, Deserialize)]
pub struct InstallPluginRequest {
    pub manifest: PluginManifest,
    #[serde(default)]
    pub trust_level: Option<String>,
}

/// Request to update a plugin
#[derive(Debug, Deserialize)]
pub struct UpdatePluginRequest {
    pub enabled: Option<bool>,
    pub manifest: Option<PluginManifest>,
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
    pub enabled: bool,
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
            enabled: p.enabled,
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