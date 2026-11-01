use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    pub session_id: Uuid,
    /// OIDC id_token from login, kept for RP-initiated logout (id_token_hint).
    /// NULL for local/password logins.
    pub oidc_id_token: Option<String>,
    /// When the session last proved who is using it (see
    /// `services::step_up`). Queryable is positional, so the fields follow
    /// the schema's column order.
    pub confirmed_at: Option<chrono::NaiveDateTime>,
    /// A portal session opened from a ticket's link reaches only that ticket
    /// (`handlers::portal::PortalScope`). NULL for every other session.
    pub scope_ticket_id: Option<i32>,
    /// How the identity provider checked who signed in (its `amr`), on a
    /// session opened through the provider; empty when it didn't say. NULL
    /// for every other sign-in.
    pub provider_amr: Option<Vec<Option<String>>>,
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
    /// See `ActiveSession::oidc_id_token`. Set by the OIDC login paths only.
    pub oidc_id_token: Option<String>,
    /// See `ActiveSession::confirmed_at`.
    pub confirmed_at: Option<chrono::NaiveDateTime>,
    /// See `ActiveSession::provider_amr`.
    pub provider_amr: Option<Vec<Option<String>>>,
}
