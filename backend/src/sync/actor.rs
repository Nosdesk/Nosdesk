//! Actor context threaded through every repository write.
//!
//! Repository helpers no longer fetch "current user" from a request;
//! the handler builds an `ActorContext` once per request from the JWT
//! claims and passes it through. Background jobs and migrations build
//! an `ActorContext` with `kind = System` instead. Plugin-emitted
//! events build one with `kind = Plugin`.
//!
//! The same context drives the Postgres GUCs (`app.actor_uuid`,
//! `app.correlation_id`) used by the audit_log trigger, so trigger
//! rows and `sync_actions` rows agree on who did what.

use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    User,
    System,
    Plugin,
}

impl ActorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "user",
            Self::System => "system",
            Self::Plugin => "plugin",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActorContext {
    pub kind: ActorKind,
    /// User UUID (when `kind = User`) or plugin owner UUID (when
    /// `kind = Plugin`). `None` for unattributed system actors.
    pub uuid: Option<Uuid>,
    /// Free-form reference for plugin / system actors: plugin slug,
    /// scheduler job id, etc. `None` for human users.
    pub reference: Option<String>,
    /// Correlation id stitches multi-request causal chains together.
    /// Set per request from the inbound `X-Correlation-Id` header or
    /// generated if absent.
    pub correlation_id: Option<Uuid>,
    /// Optional client-supplied transaction id; the sync engine uses
    /// it for idempotent retry dedup.
    pub client_tx_id: Option<String>,
}

impl ActorContext {
    /// Construct an actor context for a JWT-authenticated user.
    pub fn user(uuid: Uuid, correlation_id: Option<Uuid>) -> Self {
        Self {
            kind: ActorKind::User,
            uuid: Some(uuid),
            reference: None,
            correlation_id,
            client_tx_id: None,
        }
    }

    /// Construct an actor context for an unattributed system action
    /// (background job, migration, scheduler).
    pub fn system(reference: impl Into<String>) -> Self {
        Self {
            kind: ActorKind::System,
            uuid: None,
            reference: Some(reference.into()),
            correlation_id: None,
            client_tx_id: None,
        }
    }

    /// Construct an actor context for a plugin-emitted event.
    pub fn plugin(plugin_uuid: Uuid, plugin_slug: impl Into<String>) -> Self {
        Self {
            kind: ActorKind::Plugin,
            uuid: Some(plugin_uuid),
            reference: Some(plugin_slug.into()),
            correlation_id: None,
            client_tx_id: None,
        }
    }
}
