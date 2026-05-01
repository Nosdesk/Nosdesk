//! Dashboard statistics aggregation.
//!
//! Computes the counts that back the `/api/dashboard/stats`
//! endpoint. The endpoint accepts an `include` parameter so the
//! frontend's dashboard widget registry can ask for only what
//! the active widget set actually needs; this module's `compute`
//! function honours that subset, computing only the requested
//! groups.
//!
//! All counts run as grouped scans against indexes added in
//! `migrations/2026-04-27-000000_dashboard_stats_indexes`. Adding
//! a new stat group? Add a `StatsGroup` variant, populate the
//! corresponding `Option<...>` field in `compute`, and document
//! the JSON key in the frontend widget registry.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::QueryResult;
use serde::Serialize;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{TicketPriority, WorkflowStateCategory};
use crate::schema::{tickets, workflow_states};

/// Discrete computation units the dashboard can request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatsGroup {
    Queue,
    Yours,
    Summary,
    /// Top-of-queue knowledge gaps for the editorial widget.
    KnowledgeGaps,
}

impl StatsGroup {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "queue" => Some(Self::Queue),
            "yours" => Some(Self::Yours),
            "summary" => Some(Self::Summary),
            "knowledge_gaps" => Some(Self::KnowledgeGaps),
            _ => None,
        }
    }

    pub fn all() -> HashSet<Self> {
        [Self::Queue, Self::Yours, Self::Summary, Self::KnowledgeGaps]
            .into_iter()
            .collect()
    }

    pub fn all_keys() -> &'static [&'static str] {
        &["queue", "yours", "summary", "knowledge_gaps"]
    }
}

/// Top-level response. Each field is `Option<...>` and skipped
/// from JSON when not requested, so the wire payload stays tight
/// for partial requests.
#[derive(Serialize, Default)]
pub struct StatsBundle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub queue: Option<QueueStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub yours: Option<ScopedStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<ScopedStats>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "knowledgeGaps")]
    pub knowledge_gaps: Option<KnowledgeGapsStats>,
}

/// Knowledge-gaps top-of-queue snapshot. Returns total open and
/// the top N by impact_score so the dashboard widget can render
/// without a follow-up `/api/knowledge-gaps` call.
#[derive(Serialize, Default)]
pub struct KnowledgeGapsStats {
    pub total: i64,
    pub top: Vec<KnowledgeGapsStatsItem>,
}

#[derive(Serialize)]
pub struct KnowledgeGapsStatsItem {
    pub id: i64,
    pub title: String,
    #[serde(rename = "impactScore")]
    pub impact_score: i32,
    #[serde(rename = "evidenceCount")]
    pub evidence_count: i32,
    #[serde(rename = "lastEvidenceAt")]
    pub last_evidence_at: Option<chrono::NaiveDateTime>,
}

/// Stats for the shared work queue. Not user-scoped.
#[derive(Serialize, Default)]
pub struct QueueStats {
    pub total: i64,
    pub unassigned: i64,
    pub open: i64,
    #[serde(rename = "inProgress")]
    pub in_progress: i64,
    #[serde(rename = "highPriority")]
    pub high_priority: i64,
    #[serde(rename = "closedToday")]
    pub closed_today: i64,
}

/// Stats scoped to a specific user (either as assignee, for
/// `yours`, or as requester, for `summary`).
#[derive(Serialize, Default)]
pub struct ScopedStats {
    pub open: i64,
    #[serde(rename = "inProgress")]
    pub in_progress: i64,
    pub closed: i64,
    #[serde(rename = "closedToday")]
    pub closed_today: i64,
    #[serde(rename = "highPriority")]
    pub high_priority: i64,
}

pub fn compute(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
    groups: &HashSet<StatsGroup>,
) -> QueryResult<StatsBundle> {
    let mut bundle = StatsBundle::default();
    if groups.contains(&StatsGroup::Queue) {
        bundle.queue = Some(queue_stats(conn)?);
    }
    if groups.contains(&StatsGroup::Yours) {
        bundle.yours = Some(scoped_stats_assignee(conn, user_uuid)?);
    }
    if groups.contains(&StatsGroup::Summary) {
        bundle.summary = Some(scoped_stats_requester(conn, user_uuid)?);
    }
    if groups.contains(&StatsGroup::KnowledgeGaps) {
        bundle.knowledge_gaps = Some(knowledge_gaps_stats(conn)?);
    }
    Ok(bundle)
}

fn knowledge_gaps_stats(conn: &mut DbConnection) -> QueryResult<KnowledgeGapsStats> {
    use crate::schema::knowledge_gaps;

    // Total open+drafting (the "active" set the queue view shows).
    let total: i64 = knowledge_gaps::table
        .filter(knowledge_gaps::status.eq_any(["open", "drafting"]))
        .count()
        .get_result(conn)?;

    // Top 5 by impact_score. The composite index
    // idx_knowledge_gaps_active covers this — see the migration.
    let top: Vec<(i64, String, i32, i32, Option<chrono::NaiveDateTime>)> =
        knowledge_gaps::table
            .filter(knowledge_gaps::status.eq_any(["open", "drafting"]))
            .order_by((
                knowledge_gaps::impact_score.desc(),
                knowledge_gaps::last_evidence_at.desc().nulls_last(),
            ))
            .select((
                knowledge_gaps::id,
                knowledge_gaps::title,
                knowledge_gaps::impact_score,
                knowledge_gaps::evidence_count,
                knowledge_gaps::last_evidence_at,
            ))
            .limit(5)
            .load(conn)?;

    Ok(KnowledgeGapsStats {
        total,
        top: top
            .into_iter()
            .map(|(id, title, impact_score, evidence_count, last_evidence_at)| {
                KnowledgeGapsStatsItem {
                    id,
                    title,
                    impact_score,
                    evidence_count,
                    last_evidence_at,
                }
            })
            .collect(),
    })
}

fn queue_stats(conn: &mut DbConnection) -> QueryResult<QueueStats> {
    // Diesel can't form a cross-table GROUP BY here (the two columns
    // live on different tables in the join), so we group on
    // workflow_state_id (single-table) and fold to category in Rust.
    // The cardinality of (workflow_state_id, priority) is at most
    // ~6 * 3 = 18 rows.
    let rows: Vec<(i32, TicketPriority, i64)> = tickets::table
        .group_by((tickets::workflow_state_id, tickets::priority))
        .select((tickets::workflow_state_id, tickets::priority, count_star()))
        .load(conn)?;

    let mut s = QueueStats::default();
    for (ws_id, priority, count) in &rows {
        s.total += count;
        let cat = crate::repository::workflow_states::category_of(conn, *ws_id)?
            .unwrap_or(WorkflowStateCategory::Backlog);
        match cat.legacy_status() {
            "open" => s.open += count,
            "in-progress" => s.in_progress += count,
            // Closed tickets are still counted in `total` (they exist),
            // but the legacy widget didn't surface them in this struct.
            _ => {}
        }
        if matches!(priority, TicketPriority::High) {
            s.high_priority += count;
        }
    }

    // "Unassigned" === non-terminal tickets without an assignee.
    s.unassigned = tickets::table
        .inner_join(workflow_states::table)
        .filter(tickets::assignee_uuid.is_null())
        .filter(workflow_states::category.eq_any(vec![
            WorkflowStateCategory::Triage,
            WorkflowStateCategory::Backlog,
            WorkflowStateCategory::Active,
            WorkflowStateCategory::InReview,
        ]))
        .count()
        .get_result(conn)?;

    s.closed_today = tickets::table
        .inner_join(workflow_states::table)
        .filter(workflow_states::category.eq_any(vec![
            WorkflowStateCategory::Done,
            WorkflowStateCategory::Cancelled,
        ]))
        .filter(tickets::closed_at.ge(today_start()))
        .count()
        .get_result(conn)?;

    Ok(s)
}

fn scoped_stats_assignee(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> QueryResult<ScopedStats> {
    let rows: Vec<(i32, TicketPriority, i64)> = tickets::table
        .filter(tickets::assignee_uuid.eq(*user_uuid))
        .group_by((tickets::workflow_state_id, tickets::priority))
        .select((tickets::workflow_state_id, tickets::priority, count_star()))
        .load(conn)?;

    let mut s = aggregate_rows(conn, &rows)?;

    s.closed_today = tickets::table
        .inner_join(workflow_states::table)
        .filter(tickets::assignee_uuid.eq(*user_uuid))
        .filter(workflow_states::category.eq_any(vec![
            WorkflowStateCategory::Done,
            WorkflowStateCategory::Cancelled,
        ]))
        .filter(tickets::closed_at.ge(today_start()))
        .count()
        .get_result(conn)?;

    Ok(s)
}

fn scoped_stats_requester(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> QueryResult<ScopedStats> {
    let rows: Vec<(i32, TicketPriority, i64)> = tickets::table
        .filter(tickets::requester_uuid.eq(*user_uuid))
        .group_by((tickets::workflow_state_id, tickets::priority))
        .select((tickets::workflow_state_id, tickets::priority, count_star()))
        .load(conn)?;

    let mut s = aggregate_rows(conn, &rows)?;

    s.closed_today = tickets::table
        .inner_join(workflow_states::table)
        .filter(tickets::requester_uuid.eq(*user_uuid))
        .filter(workflow_states::category.eq_any(vec![
            WorkflowStateCategory::Done,
            WorkflowStateCategory::Cancelled,
        ]))
        .filter(tickets::closed_at.ge(today_start()))
        .count()
        .get_result(conn)?;

    Ok(s)
}

fn aggregate_rows(
    conn: &mut DbConnection,
    rows: &[(i32, TicketPriority, i64)],
) -> QueryResult<ScopedStats> {
    let mut s = ScopedStats::default();
    for (ws_id, priority, count) in rows {
        let cat = crate::repository::workflow_states::category_of(conn, *ws_id)?
            .unwrap_or(WorkflowStateCategory::Backlog);
        match cat.legacy_status() {
            "open" => s.open += count,
            "in-progress" => s.in_progress += count,
            "closed" => s.closed += count,
            _ => {}
        }
        if matches!(priority, TicketPriority::High) {
            s.high_priority += count;
        }
    }
    Ok(s)
}

fn today_start() -> DateTime<Utc> {
    Utc::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("00:00:00 is always a valid time")
        .and_utc()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_known_keys() {
        assert_eq!(StatsGroup::parse("queue"), Some(StatsGroup::Queue));
        assert_eq!(StatsGroup::parse("yours"), Some(StatsGroup::Yours));
        assert_eq!(StatsGroup::parse("summary"), Some(StatsGroup::Summary));
    }

    #[test]
    fn parse_unknown_key_rejected() {
        assert_eq!(StatsGroup::parse("everything"), None);
        assert_eq!(StatsGroup::parse(""), None);
    }

    #[test]
    fn all_keys_match_parse_round_trip() {
        for key in StatsGroup::all_keys() {
            assert!(
                StatsGroup::parse(key).is_some(),
                "key {key} should round-trip through parse",
            );
        }
    }

    #[test]
    fn empty_bundle_serializes_to_empty_object() {
        let bundle = StatsBundle::default();
        let json = serde_json::to_string(&bundle).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn partial_bundle_omits_missing_groups() {
        let bundle = StatsBundle {
            queue: Some(QueueStats { unassigned: 5, ..Default::default() }),
            yours: None,
            summary: None,
            knowledge_gaps: None,
        };
        let json = serde_json::to_string(&bundle).unwrap();
        assert!(json.contains("\"queue\""));
        assert!(!json.contains("\"yours\""));
        assert!(!json.contains("\"summary\""));
    }
}
