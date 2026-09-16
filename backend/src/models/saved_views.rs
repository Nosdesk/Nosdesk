use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Saved view: a per-user / per-project / workspace-wide preset
/// bundling a `ViewShape` and `FilterState`. The two JSON columns
/// are validated client-side; the server treats them as opaque so
/// plugin-defined view shapes round-trip without a wire change.
///
/// History note: earlier revisions carried `is_default` (with a
/// partial unique index enforcing one per scope) and `archived_at`
/// (soft delete). Both dropped 2026-05-09 in favour of: the single
/// built-in `MY_OPEN_VIEW` fallback for "what shows by default,"
/// and hard `DELETE` for "delete a view." Neither column had a
/// user-facing surface (no admin UI to set the default; no archived-
/// views browser or restore flow), and the `is_default` mechanism
/// invited the bug where a user could accidentally promote a view
/// to default and then have no way to find or change that setting.
#[derive(Debug, Clone, Serialize, Deserialize, Identifiable, Queryable)]
#[diesel(table_name = crate::schema::saved_views)]
pub struct SavedView {
    pub id: i32,
    pub uuid: Uuid,
    pub scope: String,
    pub scope_id: Option<String>,
    pub name: String,
    pub shape: serde_json::Value,
    pub filter: serde_json::Value,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Dataset the view applies to: 'tickets' | 'assets' |
    /// 'users'. Existing rows backfilled to 'tickets'. The
    /// handler refuses workspace/project scope on non-ticket
    /// datasets so the access model stays ticket-specific.
    pub dataset: String,
    pub workspace_id: i32,
    /// Renderer the dashboard SavedViewWidget shell uses for this
    /// view: 'list' (the default, no chart) | 'kpi_tile' | 'line' |
    /// 'horizontal_bar' | 'heatmap' | 'leaderboard' | 'table'. CHECK-
    /// constrained at the DB level; the handler validates the same
    /// allowlist before write.
    pub viz_type: String,
    /// Per-renderer config blob: measures, group-by, top-N, grain,
    /// chart_source tagged union, etc. The shape varies per viz_type.
    pub viz_config: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Insertable)]
#[diesel(table_name = crate::schema::saved_views)]
pub struct NewSavedView {
    pub scope: String,
    pub scope_id: Option<String>,
    pub name: String,
    pub shape: serde_json::Value,
    pub filter: serde_json::Value,
    pub created_by: Uuid,
    pub dataset: String,
    pub viz_type: String,
    pub viz_config: serde_json::Value,
}

#[derive(Debug, Default, Serialize, Deserialize, AsChangeset)]
#[diesel(table_name = crate::schema::saved_views)]
pub struct SavedViewUpdate {
    pub name: Option<String>,
    pub shape: Option<serde_json::Value>,
    pub filter: Option<serde_json::Value>,
    pub viz_type: Option<String>,
    pub viz_config: Option<serde_json::Value>,
}
