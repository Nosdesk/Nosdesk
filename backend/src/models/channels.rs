use super::tickets::Ticket;
use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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

/// `channels.provider` values for the email ingestion paths. `email_imap`
/// polls a mailbox (self-host / niche providers); `email_forward` receives
/// mail the customer forwards to a generated `<token>@inbound.<domain>`
/// address; `email_managed` receives mail addressed directly to the hosted
/// workspace's managed default address `support@<slug>.<tenant_domain>`
/// (routed by slug, at most one per workspace, auto-created on first
/// inbound). All feed the same parse pipeline; only the ingestion source
/// differs.
pub const CHANNEL_PROVIDER_EMAIL_IMAP: &str = "email_imap";
pub const CHANNEL_PROVIDER_EMAIL_FORWARD: &str = "email_forward";
pub const CHANNEL_PROVIDER_EMAIL_MANAGED: &str = "email_managed";

/// `inbound_addresses.status` values, in lockstep with the
/// `inbound_addresses_status_check` SQL constraint. `active` addresses route;
/// `retired` ones are kept on record but no longer resolve.
pub const INBOUND_ADDRESS_STATUS_ACTIVE: &str = "active";
pub const INBOUND_ADDRESS_STATUS_RETIRED: &str = "retired";

/// `inbound_dead_letters.reason` values. `unknown_token` is clean mail (scans
/// passed) that resolved to no active forwarding token; `unknown_recipient`
/// is clean mail to a managed-style address (`support@<label>.<domain>`)
/// whose label matched no active workspace slug.
pub const INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN: &str = "unknown_token";
pub const INBOUND_DEAD_LETTER_REASON_UNKNOWN_RECIPIENT: &str = "unknown_recipient";
/// Routed to a known workspace but the raw MIME could not be parsed.
pub const INBOUND_DEAD_LETTER_REASON_UNPARSEABLE: &str = "unparseable";

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
    pub workspace_id: i32,
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
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
    /// Framed AES-256-GCM blob (`utils::encryption::Keyring` shape).
    /// AAD = `channel_id.to_be_bytes() ‖ b":" ‖ credential_type.as_bytes()`.
    pub encrypted_value: Vec<u8>,
    pub encrypted_kek_id: i16,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::channel_credentials)]
pub struct NewChannelCredential {
    pub channel_id: i32,
    pub credential_type: String,
    pub encrypted_value: Vec<u8>,
    pub encrypted_kek_id: i16,
    pub expires_at: Option<NaiveDateTime>,
}

/// Per-workspace outbound email identity (one row per workspace).
///
/// Deliberately NOT `Serialize`: `encrypted_smtp_password` must never reach
/// a client. The admin handler builds a separate response DTO carrying a
/// `password_configured` flag instead of the ciphertext. The blob is a
/// framed AES-256-GCM value (`utils::encryption::Keyring` shape) with
/// AAD = `workspace_id.to_be_bytes() ‖ b".nosdesk.workspace.email.v1"`,
/// decrypted by the outbound resolver at send time.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = crate::schema::workspace_email_settings)]
pub struct WorkspaceEmailSettings {
    pub workspace_id: i32,
    pub enabled: bool,
    pub from_name: String,
    pub from_email: String,
    pub smtp_host: String,
    pub smtp_port: i32,
    pub smtp_security: String,
    pub smtp_username: String,
    pub encrypted_smtp_password: Option<Vec<u8>>,
    pub encrypted_kek_id: Option<i16>,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    /// How this workspace sends: `fallback` (instance identity),
    /// `verified_domain` (DKIM-signed via the instance relay), or
    /// `smtp_relay` (the workspace's own relay, the `smtp_*` columns).
    /// See [`workspace_email_sending_mode`].
    pub sending_mode: String,
    /// The verified sending domain (the `From` domain), for `verified_domain`.
    pub sending_domain: Option<String>,
    /// DKIM selector (`<selector>._domainkey.<sending_domain>`).
    pub dkim_selector: Option<String>,
    /// `rsa` | `ed25519`.
    pub dkim_algorithm: Option<String>,
    /// KEK-encrypted DKIM private key (framed AES-256-GCM blob), AAD-bound to
    /// the workspace. Redacted from the audit log.
    pub encrypted_dkim_private_key: Option<Vec<u8>>,
    /// kek_id sidecar for `encrypted_dkim_private_key`.
    pub dkim_kek_id: Option<i16>,
    /// `unverified` | `pending` | `verified` | `failed`. Only `verified`
    /// permits sending from the workspace's domain.
    pub verification_status: String,
    pub verified_at: Option<NaiveDateTime>,
}

/// Sending-mode + verification-status constants, kept in lockstep with the
/// `workspace_email_settings_*_check` SQL constraints.
pub mod workspace_email_sending_mode {
    pub const FALLBACK: &str = "fallback";
    pub const VERIFIED_DOMAIN: &str = "verified_domain";
    pub const SMTP_RELAY: &str = "smtp_relay";
}

pub mod workspace_email_verification_status {
    pub const UNVERIFIED: &str = "unverified";
    pub const PENDING: &str = "pending";
    pub const VERIFIED: &str = "verified";
    pub const FAILED: &str = "failed";
}

/// Editable fields of [`WorkspaceEmailSettings`]. Omits `workspace_id` (the
/// RLS GUC fills it on insert), the password and DKIM columns (managed
/// separately by `set_password`/`clear_password` and `provision_dkim`), and
/// the timestamps. `sending_mode` chooses how the workspace sends; the
/// verified-domain fields are populated by `provision_dkim`.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::workspace_email_settings)]
pub struct UpsertWorkspaceEmailSettings {
    pub enabled: bool,
    pub from_name: String,
    pub from_email: String,
    pub smtp_host: String,
    pub smtp_port: i32,
    pub smtp_security: String,
    pub smtp_username: String,
    /// See [`workspace_email_sending_mode`].
    pub sending_mode: String,
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
    pub workspace_id: i32,
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

/// A forwarding address (`<token>@inbound.<domain>`) owned by an
/// `email_forward` channel. The `token` is the routing key the inbound
/// webhook resolves; see `repository::inbound_addresses` and the
/// `inbound_addresses` migration for the capability rationale.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Associations)]
#[diesel(table_name = crate::schema::inbound_addresses)]
#[diesel(belongs_to(Channel))]
pub struct InboundAddress {
    pub id: i32,
    pub token: String,
    pub channel_id: i32,
    /// See [`INBOUND_ADDRESS_STATUS_ACTIVE`] / [`INBOUND_ADDRESS_STATUS_RETIRED`].
    pub status: String,
    pub created_at: NaiveDateTime,
    pub workspace_id: i32,
}

/// Insert shape for a new forwarding address. `status` defaults to `active`,
/// `workspace_id` is filled from the RLS GUC, and the timestamp defaults at
/// the DB; the caller supplies only the channel and the generated token.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::inbound_addresses)]
pub struct NewInboundAddress {
    pub token: String,
    pub channel_id: i32,
}

/// A platform-level dead-letter row: clean inbound mail (spam/virus scans
/// passed) that resolved to no active forwarding token. Untenanted by design
/// (see the `inbound_dead_letters` migration) because an unknown token can't
/// be attributed to a workspace; surfaced to the operator so a misconfigured
/// forward is diagnosable rather than silently lost.
#[derive(Debug, Clone, Serialize, Queryable, Identifiable)]
#[diesel(table_name = crate::schema::inbound_dead_letters)]
pub struct InboundDeadLetter {
    pub id: i64,
    pub envelope_recipient: String,
    pub from_address: Option<String>,
    pub subject: Option<String>,
    pub s3_key: String,
    /// See [`INBOUND_DEAD_LETTER_REASON_UNKNOWN_TOKEN`].
    pub reason: String,
    pub received_at: NaiveDateTime,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::inbound_dead_letters)]
pub struct NewInboundDeadLetter {
    pub envelope_recipient: String,
    pub from_address: Option<String>,
    pub subject: Option<String>,
    pub s3_key: String,
    pub reason: String,
}
