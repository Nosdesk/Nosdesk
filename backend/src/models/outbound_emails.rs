use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Outbound email queue (Item J Pass 1)
// ---------------------------------------------------------------------------
//
// Durable, retryable replacement for the `tokio::spawn` fire-and-forget
// outbound path. Every external-channel send goes through this queue; a
// worker drains via SELECT FOR UPDATE SKIP LOCKED and dispatches to SMTP.
// See migrations/2026-05-11-300000_outbound_emails_queue and
// `services/email_queue/` for the worker.

/// One outbound email row. The `status` column is constrained at the
/// schema layer to `pending | sending | sent | failed | dead | suppressed`.
///
/// `QueryableByName` is needed alongside `Queryable` because the worker's
/// claim path uses a CTE-with-UPDATE pattern that Diesel's typed builder
/// can't express; the raw `sql_query(...).load::<OutboundEmail>()` shape
/// needs the by-name variant.
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, QueryableByName, Identifiable)]
#[diesel(table_name = crate::schema::outbound_emails)]
pub struct OutboundEmail {
    pub id: i64,
    /// `Some(channel_id)` for ticket-reply rows that thread back into
    /// an inbound channel; `None` for transactional sends (password
    /// reset, invitation, notification) that don't belong to any
    /// channel. The worker skips the `channel_messages` book-keeping
    /// step when this is None.
    pub channel_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub recipient: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    /// Stamped at enqueue, persisted, reused on every retry. Receiving
    /// MTAs and customer MUAs dedupe on Message-ID — this is the
    /// primary defense against crash-mid-send duplicates.
    pub message_id: String,
    pub in_reply_to: Option<String>,
    /// Diesel renders `TEXT[]` columns as `Vec<Option<String>>`; nulls
    /// inside the array are unused but the type plumbing requires it.
    pub references_list: Vec<Option<String>>,
    pub headers_json: serde_json::Value,
    pub status: String,
    pub attempts: i32,
    pub last_error: Option<String>,
    pub last_smtp_code: Option<i32>,
    pub next_attempt_at: chrono::DateTime<chrono::Utc>,
    pub lease_token: Option<Uuid>,
    pub lease_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub sent_at: Option<chrono::DateTime<chrono::Utc>>,
    pub failed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub correlation_id: Option<Uuid>,
    /// Stamped when an inbound DSN linked back to this row via
    /// the deterministic Message-ID. NULL for the normal happy
    /// path. See migration `2026-05-12-100000_outbound_email_bounce_fields`.
    pub bounced_at: Option<chrono::DateTime<chrono::Utc>>,
    /// The address the remote MTA rejected. Usually matches
    /// `recipient`; differs when the original recipient was a
    /// distribution list / forwarder and the failure came from
    /// the downstream member.
    pub bounce_recipient: Option<String>,
    /// Raw RFC 3464 Diagnostic-Code or Status text from the
    /// DSN's `message/delivery-status` part. Verbatim so the
    /// admin queue UI can show the upstream reason without us
    /// having to guess at categorisation.
    pub bounce_diagnostic: Option<String>,
    /// Optional caller-supplied key for at-least-once → effectively-
    /// once enqueue. Two enqueues with the same key collapse to a
    /// single queue row (see `repository::outbound_emails::enqueue_idempotent`).
    /// Channel-reply rows leave it NULL — they're already deduped at
    /// the handler layer via stable Message-ID.
    pub idempotency_key: Option<String>,
    pub workspace_id: i32,
    /// The sending provider's own message id. **Always NULL under SMTP** (no
    /// provider id; the RFC `message_id` is the only identity). The worker still
    /// plumbs it through `mark_sent`, so it is ready for a future transport that
    /// returns one, but nothing populates it today.
    pub provider_message_id: Option<String>,
    /// Which sending identity the worker uses for this row (see
    /// [`outbound_email_sender_identity`]): `workspace` (the workspace's own
    /// SMTP identity, falling back to the instance identity) or `platform`
    /// (the instance identity, for auth mail that must not originate from a
    /// tenant relay). Decided at enqueue.
    pub sender_identity: String,
    /// Notification vs transactional (see [`outbound_email_mail_class`]).
    /// Drives deliverability headers (List-Unsubscribe on notification only).
    /// Last field so the column order matches the schema.
    pub mail_class: String,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = crate::schema::outbound_emails)]
pub struct NewOutboundEmail {
    /// `Some(channel_id)` for channel-mediated ticket replies. `None`
    /// for transactional sends that don't bind to any channel.
    pub channel_id: Option<i32>,
    pub ticket_id: Option<i32>,
    pub comment_id: Option<i32>,
    pub recipient: String,
    pub subject: String,
    pub body_text: String,
    pub body_html: Option<String>,
    pub message_id: String,
    pub in_reply_to: Option<String>,
    pub references_list: Vec<Option<String>>,
    pub headers_json: serde_json::Value,
    pub correlation_id: Option<Uuid>,
    /// Idempotency key — see `OutboundEmail::idempotency_key`. Use
    /// `enqueue_idempotent` when this is `Some`; `enqueue` with a
    /// None key for fire-and-forget channel replies.
    pub idempotency_key: Option<String>,
    /// See [`outbound_email_sender_identity`]: `workspace` for conversation /
    /// notification mail, `platform` for password reset / invitation.
    pub sender_identity: String,
    /// See [`outbound_email_mail_class`]: `notification` (opt-out-able) or
    /// `transactional` (must-deliver). Set explicitly at enqueue.
    pub mail_class: String,
}

/// Status string constants. Centralised so Rust callers (worker, repo,
/// admin handlers) and SQL CHECK constraint stay in lockstep.
pub mod outbound_email_status {
    pub const PENDING: &str = "pending";
    pub const SENDING: &str = "sending";
    pub const SENT: &str = "sent";
    pub const FAILED: &str = "failed";
    pub const DEAD: &str = "dead";
    pub const SUPPRESSED: &str = "suppressed";
}

/// Sender-identity constants, kept in lockstep with the
/// `outbound_emails_sender_identity_check` SQL constraint.
///
/// `WORKSPACE` is tenant-content mail (notifications, the portal sign-in link):
/// it sends ONLY from the workspace's own verified sending domain and is
/// deferred (never sent from the platform) until one is configured, so
/// tenant-controlled content never leaves on the platform domain — a phishing
/// and deliverability-reputation boundary. `PLATFORM` is platform-own mail
/// (account/auth, billing) to the platform's own users: it pins the instance
/// identity and never originates from a tenant relay.
pub mod outbound_email_sender_identity {
    pub const WORKSPACE: &str = "workspace";
    pub const PLATFORM: &str = "platform";
}

/// Mail-class constants, kept in lockstep with the
/// `outbound_emails_mail_class_check` SQL constraint. `NOTIFICATION` is
/// opt-out-able mail (ticket-update notifications) that carries
/// List-Unsubscribe; `TRANSACTIONAL` is must-deliver mail (password reset,
/// invitation, the agent's reply, auto-ack) that never does. A distinct axis
/// from sender identity: a conversation reply is `workspace` + `transactional`.
pub mod outbound_email_mail_class {
    pub const TRANSACTIONAL: &str = "transactional";
    pub const NOTIFICATION: &str = "notification";
}
