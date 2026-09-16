use super::tickets::{ContentFormat, Ticket};
use super::users::{User, UserInfoWithAvatar};
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    /// What the bytes in `content` are. Drives the outbound dispatcher's
    /// HTML / plaintext composition for replies.
    pub content_format: ContentFormat,
    /// Raw text/plain MIME part (or the plaintext body for plaintext-only
    /// inbound messages). NULL for non-email comments.
    ///
    /// Backend-only: `skip` keeps it off the wire because the
    /// renderer reads `new_content` / `quoted_content` instead, and
    /// shipping the full raw body on every comment list inflates
    /// payloads with no consumer.
    #[serde(skip)]
    pub body_text: Option<String>,
    /// Raw text/html MIME part. Pre-sanitisation; Pass 2 of the email
    /// rendering plan introduces a separate `sanitised_html` column for
    /// the render-ready form. NULL for non-email comments and for emails
    /// without an HTML alternative.
    ///
    /// Backend-only — same reasoning as `body_text`.
    #[serde(skip)]
    pub body_html: Option<String>,
    /// Just-the-reply extraction, output of the quote splitter at ingest.
    /// Plain text or HTML depending on which path the parser took
    /// (use `content_format` to disambiguate). NULL for non-email
    /// comments.
    pub new_content: Option<String>,
    /// Extracted prior-thread quoted block. NULL when nothing was
    /// detected or when the comment isn't email-derived. Same format
    /// rule as `new_content`.
    pub quoted_content: Option<String>,
    /// Storage path (not URL) to the persisted .eml. Powers "Show
    /// original message" and lets us re-run the splitter on policy
    /// change without re-fetching from the upstream mailbox. NULL for
    /// non-email comments and for email comments ingested before this
    /// column existed.
    ///
    /// `skip` because the storage path is internal infrastructure; the
    /// frontend constructs the public URL from the comment id
    /// (`/api/comments/{id}/raw.eml`) and doesn't need the backing
    /// path. Hiding it also avoids leaking storage layout (S3 keys,
    /// LocalStorage roots) in API responses.
    #[serde(skip)]
    pub raw_source_uri: Option<String>,
    pub workspace_id: i32,
    /// Native-first render tier set by the inbound pipeline:
    /// `text` / `simple` / `rich` (see `email_render_kind`). NULL for
    /// non-email comments (agent markdown) and email comments ingested
    /// before this column existed; the frontend falls back to its
    /// per-`content_format` rendering when NULL.
    pub render_kind: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Insertable, Default)]
#[diesel(table_name = crate::schema::comments)]
pub struct NewComment {
    pub content: String,
    pub ticket_id: i32,
    pub user_uuid: Uuid,
    #[serde(default)]
    pub channel_metadata: Option<serde_json::Value>,
    #[serde(default)]
    pub is_internal: bool,
    /// Defaults to HTML so the regular helpdesk UI (the only path that
    /// posts comments through the API today) doesn't have to opt in.
    /// Inbound channel adapters set this explicitly to match what they
    /// stored in `content`.
    #[serde(default)]
    pub content_format: ContentFormat,
    /// Inbound-email-only — see `Comment` field docs. UI-authored
    /// comments leave all four NULL and just fill `content`.
    #[serde(default)]
    pub body_text: Option<String>,
    #[serde(default)]
    pub body_html: Option<String>,
    #[serde(default)]
    pub new_content: Option<String>,
    #[serde(default)]
    pub quoted_content: Option<String>,
    #[serde(default)]
    pub raw_source_uri: Option<String>,
    /// Render tier (`text`/`simple`/`rich`) from `email_render_kind`.
    /// Inbound channel adapters set this; UI-authored comments leave it
    /// NULL.
    #[serde(default)]
    pub render_kind: Option<String>,
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
    pub workspace_id: i32,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct CommentWithAttachments {
    #[serde(flatten)]
    pub comment: Comment,
    pub attachments: Vec<Attachment>,
    pub user: Option<UserInfoWithAvatar>, // Use enhanced user info with avatar
    /// Sender's external address (email for IMAP; equivalent identity
    /// for chat channels). Sourced from the joined `channel_messages`
    /// row when the comment came from a channel; `None` for comments
    /// authored through the helpdesk UI. Surfaced as a top-level field
    /// rather than digging into `channel_metadata` so the frontend
    /// reads a single typed field instead of probing a JSON blob.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_address: Option<String>,
    /// Whether this comment has an archived raw RFC-822 source the
    /// frontend can fetch via `GET /api/comments/{id}/raw.eml`.
    /// Derived from `Comment::raw_source_uri`'s presence so the
    /// frontend can render the "Show original message" affordance
    /// conditionally without learning the storage path itself.
    pub has_raw_source: bool,
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
    /// Format the editor that produced `content` is sending. Optional
    /// from the wire so older clients keep working — the handler falls
    /// back to the `ContentFormat` default (HTML).
    #[serde(default)]
    pub content_format: ContentFormat,
    /// Internal note flag from the composer's toggle. Optional on the wire
    /// (defaults to a public comment). Was previously absent here, so the
    /// value the client sent was silently dropped and every note saved as
    /// public, leaking internal notes to requester-facing views + outbound.
    #[serde(default)]
    pub is_internal: bool,
    /// Client-minted id (UUID) for optimistic-create reconciliation: echoed
    /// into the comment.created sync action's `correlation_id` so the client
    /// matches the server echo to its pending optimistic row structurally,
    /// instead of a temp-id swap + an author/time/content dedup heuristic.
    #[serde(default)]
    pub client_id: Option<Uuid>,
}
