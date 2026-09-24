use chrono::NaiveDateTime;
use diesel::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// Knowledge Gaps (Phase 2a of the docs/KB redesign)
// ============================================================================
//
// `knowledge_gaps` is the canonical entity (lifecycle, ranking,
// resolution); `knowledge_gap_signals` carries raw evidence with
// a polymorphic source reference. See the migration for the data
// model rationale; the short version is "every detection
// mechanism writes into the same shape so an LLM in Phase 3 can
// consume them uniformly."

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Clone)]
#[diesel(table_name = crate::schema::knowledge_gaps)]
pub struct KnowledgeGap {
    pub id: i64,
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub assignee_uuid: Option<Uuid>,
    pub resolved_page_id: Option<i32>,
    pub evidence_count: i32,
    pub last_evidence_at: Option<NaiveDateTime>,
    pub impact_score: i32,
    pub created_at: NaiveDateTime,
    pub updated_at: NaiveDateTime,
    pub created_by: Option<Uuid>,
    pub dismissed_at: Option<NaiveDateTime>,
    pub dismissed_by: Option<Uuid>,
    pub resolved_at: Option<NaiveDateTime>,
    pub workspace_id: i32,
    /// The draft page this gap is being written as, while `drafting`.
    pub draft_page_id: Option<i32>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::knowledge_gaps)]
pub struct NewKnowledgeGap {
    pub title: String,
    pub description: Option<String>,
    pub status: String,
    pub created_by: Option<Uuid>,
    pub impact_score: i32,
    pub evidence_count: i32,
    pub last_evidence_at: Option<NaiveDateTime>,
}

#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = crate::schema::knowledge_gaps)]
pub struct KnowledgeGapUpdate {
    pub title: Option<String>,
    pub description: Option<Option<String>>,
    pub status: Option<String>,
    pub assignee_uuid: Option<Option<Uuid>>,
    pub resolved_page_id: Option<Option<i32>>,
    pub evidence_count: Option<i32>,
    pub last_evidence_at: Option<Option<NaiveDateTime>>,
    pub impact_score: Option<i32>,
    pub updated_at: Option<NaiveDateTime>,
    pub dismissed_at: Option<Option<NaiveDateTime>>,
    pub dismissed_by: Option<Option<Uuid>>,
    pub resolved_at: Option<Option<NaiveDateTime>>,
    pub draft_page_id: Option<Option<i32>>,
}

#[derive(Debug, Serialize, Deserialize, Queryable, Identifiable, Associations, Clone)]
#[diesel(table_name = crate::schema::knowledge_gap_signals)]
#[diesel(belongs_to(KnowledgeGap, foreign_key = gap_id))]
pub struct KnowledgeGapSignal {
    pub id: i64,
    pub gap_id: i64,
    pub signal_type: String,
    pub source_kind: String,
    pub source_ref: String,
    pub payload: serde_json::Value,
    pub confidence: i32,
    pub detected_by: Option<Uuid>,
    pub detected_at: NaiveDateTime,
    pub dismissed_at: Option<NaiveDateTime>,
    pub dismissed_by: Option<Uuid>,
    pub workspace_id: i32,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = crate::schema::knowledge_gap_signals)]
pub struct NewKnowledgeGapSignal {
    pub gap_id: i64,
    pub signal_type: String,
    pub source_kind: String,
    pub source_ref: String,
    pub payload: serde_json::Value,
    pub confidence: i32,
    pub detected_by: Option<Uuid>,
}

// ── CSP violation reports ─────────────────────────────────────
//
// Browser-submitted reports of Content-Security-Policy violations.
// See `repository/csp_reports.rs` for upsert semantics; identical
// reports increment `occurrence_count` rather than inserting new
// rows.
