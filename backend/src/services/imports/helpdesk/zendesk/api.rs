//! Zendesk's JSON, as much of it as the connector reads. Every field is
//! optional or defaulted: Zendesk adds fields freely and leaves others null
//! on old records, and one odd ticket must not stop a migration.
//!
//! Shapes follow developer.zendesk.com (Support API, 2026): incremental
//! cursor exports for users and tickets, cursor pagination (`meta`,
//! `links`) for organizations, ticket fields and comments.

use chrono::{DateTime, Utc};
use serde::Deserialize;

/// `GET /api/v2/incremental/users/cursor.json`
#[derive(Debug, Deserialize)]
pub struct UsersPage {
    #[serde(default)]
    pub users: Vec<User>,
    #[serde(default)]
    pub after_cursor: Option<String>,
    #[serde(default)]
    pub end_of_stream: bool,
}

/// `GET /api/v2/incremental/tickets/cursor.json`
#[derive(Debug, Deserialize)]
pub struct TicketsPage {
    #[serde(default)]
    pub tickets: Vec<Ticket>,
    #[serde(default)]
    pub after_cursor: Option<String>,
    #[serde(default)]
    pub end_of_stream: bool,
}

/// The `meta` block of a cursor-paginated list.
#[derive(Debug, Default, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub has_more: bool,
    #[serde(default)]
    pub after_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OrganizationsPage {
    #[serde(default)]
    pub organizations: Vec<Organization>,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Debug, Deserialize)]
pub struct TicketFieldsPage {
    #[serde(default)]
    pub ticket_fields: Vec<TicketField>,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Debug, Deserialize)]
pub struct CommentsPage {
    #[serde(default)]
    pub comments: Vec<Comment>,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Debug, Clone, Deserialize)]
pub struct User {
    pub id: i64,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub suspended: bool,
    #[serde(default)]
    pub organization_id: Option<i64>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct Organization {
    pub id: i64,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TicketField {
    pub id: i64,
    #[serde(default)]
    pub title: Option<String>,
    /// Dropdown choices: a ticket stores the option's `value` (a tag), the
    /// person saw its `name`.
    #[serde(default)]
    pub custom_field_options: Option<Vec<FieldOption>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldOption {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CustomFieldValue {
    pub id: i64,
    #[serde(default)]
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ticket {
    pub id: i64,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub raw_subject: Option<String>,
    /// The first comment's text. Only read by the export path, for a ticket
    /// that arrives without its comments.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub priority: Option<String>,
    #[serde(default, rename = "type")]
    pub ticket_type: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub requester_id: Option<i64>,
    #[serde(default)]
    pub submitter_id: Option<i64>,
    #[serde(default)]
    pub assignee_id: Option<i64>,
    #[serde(default)]
    pub collaborator_ids: Vec<i64>,
    #[serde(default)]
    pub follower_ids: Vec<i64>,
    #[serde(default)]
    pub email_cc_ids: Vec<i64>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub custom_fields: Vec<CustomFieldValue>,
    /// Present in the full JSON export only; the API path fetches comments
    /// per ticket.
    #[serde(default)]
    pub comments: Option<Vec<Comment>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Comment {
    pub id: i64,
    #[serde(default)]
    pub author_id: Option<i64>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub html_body: Option<String>,
    #[serde(default)]
    pub plain_body: Option<String>,
    #[serde(default = "yes")]
    pub public: bool,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub attachments: Vec<Attachment>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Attachment {
    pub id: i64,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub content_url: Option<String>,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
}
