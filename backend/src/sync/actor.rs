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

/// The single workspace that exists on every self-hosted install.
/// Used by bootstrap / pre-request paths that need to pin GUCs
/// before any request resolves a WorkspaceContext.
///
/// In hosted mode this constant is still meaningful (the platform's
/// "control plane" workspace), but per-tenant code paths MUST NOT
/// reach for it. They resolve workspace_id from the request's
/// WorkspaceContext or from a workspace_members lookup.
pub const BOOTSTRAP_WORKSPACE_ID: i32 = 1;

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
    /// Workspace this action runs against. Set by the request
    /// pipeline from the resolved `WorkspaceContext` (see
    /// `middleware::workspace_context`); `None` for super-admin /
    /// platform-level paths that legitimately operate across
    /// workspaces. Drives the `app.workspace_id` Postgres GUC the
    /// Phase 3 RLS policies read.
    pub workspace_id: Option<i32>,
}

impl ActorContext {
    /// Construct an actor context for a JWT-authenticated user.
    /// Workspace defaults to `None`; the request pipeline calls
    /// `.with_workspace(...)` after constructing if a workspace
    /// context is resolved (almost always true for user actions).
    pub fn user(uuid: Uuid, correlation_id: Option<Uuid>) -> Self {
        Self {
            kind: ActorKind::User,
            uuid: Some(uuid),
            reference: None,
            correlation_id,
            client_tx_id: None,
            workspace_id: None,
        }
    }

    /// Construct an actor context for an unattributed system action
    /// (background job, migration, scheduler). Most system actions
    /// are workspace-scoped (per-workspace jobs); the caller pins
    /// the workspace with `.with_workspace(ws_id)`. The handful of
    /// genuinely cross-workspace jobs (registry sync, partition
    /// rotation) leave it as `None`.
    pub fn system(reference: impl Into<String>) -> Self {
        Self {
            kind: ActorKind::System,
            uuid: None,
            reference: Some(reference.into()),
            correlation_id: None,
            client_tx_id: None,
            workspace_id: None,
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
            workspace_id: None,
        }
    }

    /// System actor pinned to the bootstrap workspace. Use for any
    /// code that runs outside an authenticated request but writes to
    /// audited tables: admin_setup, env-var seed, default content
    /// seeding, bootstrap-time plugin install, CLI tools.
    ///
    /// `reference` should name the call site (e.g. "admin_setup",
    /// "cli:import_tickets") so the audit row's actor_kind="system" +
    /// actor_ref="<name>" tells a reviewer who wrote the row.
    pub fn bootstrap(reference: impl Into<String>) -> Self {
        Self::system(reference).with_workspace(BOOTSTRAP_WORKSPACE_ID)
    }

    /// Authenticated-user actor pinned to a specific workspace. Use
    /// from credential-verified-but-pre-session flows: mfa_enable_login,
    /// password reset confirm, invitation accept, OAuth callback's
    /// existing-user update step. The workspace_id comes from a
    /// workspace_members lookup against the verified user (see
    /// `repository::workspaces::primary_workspace_for_user`).
    pub fn user_at_workspace(user_uuid: Uuid, workspace_id: i32) -> Self {
        Self::user(user_uuid, None).with_workspace(workspace_id)
    }

    /// Builder that pins the workspace for this actor context.
    /// Called by the request pipeline after `ActorContext::user(...)`
    /// once the `WorkspaceContext` middleware has resolved one. The
    /// take-by-value + return-Self shape matches the rest of the
    /// codebase's builder pattern and avoids forcing every existing
    /// `ActorContext::user(...)` call site to pass a workspace they
    /// haven't computed yet.
    pub fn with_workspace(mut self, workspace_id: i32) -> Self {
        self.workspace_id = Some(workspace_id);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_is_a_system_actor_pinned_to_workspace_one() {
        let actor = ActorContext::bootstrap("admin_setup");
        assert_eq!(actor.kind, ActorKind::System);
        assert_eq!(actor.uuid, None);
        assert_eq!(actor.reference.as_deref(), Some("admin_setup"));
        assert_eq!(actor.workspace_id, Some(BOOTSTRAP_WORKSPACE_ID));
    }

    #[test]
    fn user_at_workspace_carries_uuid_and_workspace() {
        let user_uuid = Uuid::now_v7();
        let actor = ActorContext::user_at_workspace(user_uuid, 7);
        assert_eq!(actor.kind, ActorKind::User);
        assert_eq!(actor.uuid, Some(user_uuid));
        assert_eq!(actor.reference, None);
        assert_eq!(actor.workspace_id, Some(7));
    }
}
