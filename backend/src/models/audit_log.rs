use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Audit log
// ---------------------------------------------------------------------------
//
// Read-only model. The Postgres trigger `audit_log_trigger()` writes rows
// from inside user-facing transactions; the application never inserts into
// this table directly. See `repository/audit_log.rs` for the read API and
// `migrations/2026-05-02-100000_sync_substrate/up.sql` for the trigger.

#[derive(Debug, Clone, Serialize, Deserialize, Queryable)]
#[diesel(table_name = crate::schema::audit_log)]
pub struct AuditLogRow {
    pub id: i64,
    pub table_name: String,
    pub pk_text: String,
    /// One of 'I' / 'U' / 'D' (CHECK constraint enforced by Postgres).
    pub op: String,
    pub before_jsonb: Option<serde_json::Value>,
    pub after_jsonb: Option<serde_json::Value>,
    /// Set only on UPDATE; lists JSONB keys that changed.
    pub changed_cols: Option<Vec<Option<String>>>,
    pub actor_uuid: Option<Uuid>,
    pub correlation_id: Option<Uuid>,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub workspace_id: i32,
}
