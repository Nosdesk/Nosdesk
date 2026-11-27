//! Zendesk's JSON, as much of it as the connector reads. Every field is
//! optional or defaulted: Zendesk adds fields freely and leaves others null
//! on old records, and one odd ticket must not stop a migration.
//!
//! Shapes follow developer.zendesk.com (Support API, 2026): incremental
//! cursor exports for users and tickets, cursor pagination (`meta`,
//! `links`) for organizations, ticket fields and comments.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer};

/// A missing field and an explicit `null` both read as the default: Zendesk
/// writes `null` for an empty list or flag on some records, and serde's
/// `default` alone covers only the missing field.
fn null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

/// [`null_default`] for a flag that is on unless the record says otherwise.
fn null_yes<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    Ok(Option::<bool>::deserialize(d)?.unwrap_or(true))
}

/// `GET /api/v2/incremental/users/cursor.json`
#[derive(Debug, Deserialize)]
pub struct UsersPage {
    #[serde(default, deserialize_with = "null_default")]
    pub users: Vec<User>,
    #[serde(default)]
    pub after_cursor: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub end_of_stream: bool,
}

/// `GET /api/v2/incremental/tickets/cursor.json`
#[derive(Debug, Deserialize)]
pub struct TicketsPage {
    #[serde(default, deserialize_with = "null_default")]
    pub tickets: Vec<Ticket>,
    /// Sideloaded with `include=metric_sets`: when each ticket was solved.
    #[serde(default, deserialize_with = "null_default")]
    pub metric_sets: Vec<MetricSet>,
    #[serde(default)]
    pub after_cursor: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub end_of_stream: bool,
}

/// The `meta` block of a cursor-paginated list.
#[derive(Debug, Default, Deserialize)]
pub struct Meta {
    #[serde(default, deserialize_with = "null_default")]
    pub has_more: bool,
    #[serde(default)]
    pub after_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct OrganizationsPage {
    #[serde(default, deserialize_with = "null_default")]
    pub organizations: Vec<Organization>,
    #[serde(default, deserialize_with = "null_default")]
    pub meta: Meta,
}

/// `GET /api/v2/users/show_many.json?ids=...`. The list must be there: an
/// answer without it says nothing about who is gone, so it doesn't read.
#[derive(Debug, Deserialize)]
pub struct UsersList {
    pub users: Vec<User>,
}

/// `GET /api/v2/organizations/show_many.json?ids=...`
#[derive(Debug, Deserialize)]
pub struct OrganizationsList {
    pub organizations: Vec<Organization>,
}

/// `GET /api/v2/groups.json`
#[derive(Debug, Deserialize)]
pub struct GroupsPage {
    #[serde(default, deserialize_with = "null_default")]
    pub groups: Vec<Named>,
    #[serde(default, deserialize_with = "null_default")]
    pub meta: Meta,
}

/// `GET /api/v2/brands.json`
#[derive(Debug, Deserialize)]
pub struct BrandsPage {
    #[serde(default, deserialize_with = "null_default")]
    pub brands: Vec<Named>,
    #[serde(default, deserialize_with = "null_default")]
    pub meta: Meta,
}

/// A group or brand: its name is all the import note needs.
#[derive(Debug, Clone, Deserialize)]
pub struct Named {
    pub id: i64,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct TicketFieldsPage {
    #[serde(default, deserialize_with = "null_default")]
    pub ticket_fields: Vec<TicketField>,
    #[serde(default, deserialize_with = "null_default")]
    pub meta: Meta,
}

#[derive(Debug, Deserialize)]
pub struct CommentsPage {
    #[serde(default, deserialize_with = "null_default")]
    pub comments: Vec<Comment>,
    #[serde(default, deserialize_with = "null_default")]
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
    #[serde(default = "yes", deserialize_with = "null_yes")]
    pub active: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub suspended: bool,
    #[serde(default)]
    pub organization_id: Option<i64>,
    /// 0 custom agent, 1 light agent, 2 chat agent, 3 chat agent added as a
    /// contributor, 4 admin, 5 billing admin; null for anyone else.
    #[serde(default)]
    pub role_type: Option<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub only_private_comments: bool,
    /// An agent of another Zendesk account, through ticket sharing.
    #[serde(default, deserialize_with = "null_default")]
    pub shared_agent: bool,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub shared_phone_number: bool,
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
    #[serde(default, deserialize_with = "null_default")]
    pub domain_names: Vec<String>,
    /// Being replaced by `ticket_restriction`, which wins when present.
    #[serde(default)]
    pub shared_tickets: Option<bool>,
    /// `organization` (members see each other's tickets) or `restricted`.
    #[serde(default)]
    pub ticket_restriction: Option<String>,
    /// Set in the JSON export for a deleted organization.
    #[serde(default)]
    pub deleted_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
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
    #[serde(default, deserialize_with = "null_default")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub requester_id: Option<i64>,
    #[serde(default)]
    pub submitter_id: Option<i64>,
    #[serde(default)]
    pub assignee_id: Option<i64>,
    #[serde(default)]
    pub organization_id: Option<i64>,
    #[serde(default)]
    pub group_id: Option<i64>,
    #[serde(default)]
    pub brand_id: Option<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub collaborator_ids: Vec<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub follower_ids: Vec<i64>,
    #[serde(default, deserialize_with = "null_default")]
    pub email_cc_ids: Vec<i64>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "null_default")]
    pub custom_fields: Vec<CustomFieldValue>,
    /// Present in the full JSON export only; the API path fetches comments
    /// per ticket.
    #[serde(default)]
    pub comments: Option<Vec<Comment>>,
    /// The ticket's metrics, when the export carries them inline; the API
    /// path sideloads them per page instead.
    #[serde(default)]
    pub metric_set: Option<MetricSet>,
}

/// A ticket's metrics, as much as the connector reads: when it was solved.
#[derive(Debug, Clone, Deserialize)]
pub struct MetricSet {
    #[serde(default)]
    pub ticket_id: Option<i64>,
    #[serde(default)]
    pub solved_at: Option<DateTime<Utc>>,
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
    /// Absent or null reads as private: an internal note must never come
    /// across as a public reply.
    #[serde(default, deserialize_with = "null_default")]
    pub public: bool,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, deserialize_with = "null_default")]
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
