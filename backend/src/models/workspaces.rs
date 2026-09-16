use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ===== WORKSPACE MODELS =====
// One row per tenant. Phase 1 of the multi-tenant migration
// created this table + bootstrapped the default workspace at
// id=1 for backward compatibility with the existing
// single-tenant deployment. Phase 2 introduces the
// WorkspaceContext extractor + middleware that resolves a
// workspace per request (subdomain in hosted mode, default in
// self-hosted).

#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable, Selectable)]
#[diesel(table_name = crate::schema::workspaces)]
pub struct Workspace {
    pub id: i32,
    pub uuid: Uuid,
    pub slug: String,
    pub name: String,
    /// Opaque plan identifier (free / starter / pro / enterprise
    /// / self_hosted). Intentionally not CHECK-constrained at
    /// the DB layer — the billing surface churns these values.
    pub plan: String,
    pub settings: serde_json::Value,
    pub created_at: DateTime<Utc>,
    pub archived_at: Option<DateTime<Utc>>,
    /// Nullable seam for a future MSP / enterprise
    /// organisations-as-parent-of-workspaces tier. NULL on
    /// every workspace today.
    pub organisation_id: Option<i32>,
    /// Customer-owned hostname (e.g. `support.acme.com`) that
    /// routes to this workspace. NULL when the workspace is
    /// reached via its `<slug>.nosdesk.app` subdomain only.
    /// Managed by the control plane via the
    /// `PATCH /api/internal/v1/workspaces/{slug}/custom-domain`
    /// endpoint (M5 Task 5).
    pub custom_domain: Option<String>,
    /// Staff-seat cap (NULL = unlimited). Set on a self-serve trial provision
    /// (to 5) and lifted to NULL on subscription activation. Only staff roles
    /// (owner/admin/agent) count against it. See `add_staff_membership_capped`.
    pub seat_limit: Option<i32>,
}

/// Insertable for a new workspace row. The product owns workspace
/// identity per the M5 locked decision: callers must pre-generate
/// the UUID rather than relying on the DB default so the same UUID
/// can be mirrored into the control plane's instance row.
/// `plan` is omitted so the DB column default (`'free'`) applies;
/// the control plane's plan-management surface mutates it later.
#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::workspaces)]
pub struct NewWorkspace {
    pub uuid: Uuid,
    pub slug: String,
    pub name: String,
    /// Staff-seat cap (NULL = unlimited). Set by the control plane on a
    /// self-serve trial provision (to 5) and lifted to NULL on activation;
    /// self-hosted / operator-provisioned workspaces leave it None.
    pub seat_limit: Option<i32>,
}

/// Per-workspace membership for a global user. A user can be a
/// member of multiple workspaces; the role here is workspace-
/// scoped and layered on top of the user's global role
/// (`UserRole`).
#[derive(Debug, Clone, Serialize, Deserialize, Queryable, Selectable, Insertable)]
#[diesel(table_name = crate::schema::workspace_members)]
#[diesel(primary_key(workspace_id, user_uuid))]
pub struct WorkspaceMember {
    pub workspace_id: i32,
    pub user_uuid: Uuid,
    /// One of: owner, admin, member. CHECK-constrained at the
    /// schema layer.
    pub role: String,
    pub invited_at: DateTime<Utc>,
    pub accepted_at: Option<DateTime<Utc>>,
    /// When the membership was revoked; `None` means active. The row is kept
    /// so historical actors still resolve to a name. Every role, permission
    /// and seat-counting read must filter on this; only identity resolution
    /// may ignore it.
    pub removed_at: Option<DateTime<Utc>>,
}
