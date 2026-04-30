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
use crate::models::{TicketPriority, TicketStatus};
use crate::schema::tickets;

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
    // One grouped scan over (status, priority). The idx_tickets_
    // status_priority composite covers this exactly.
    let rows: Vec<(TicketStatus, TicketPriority, i64)> = tickets::table
        .group_by((tickets::status, tickets::priority))
        .select((tickets::status, tickets::priority, count_star()))
        .load(conn)?;

    let mut s = QueueStats::default();
    for (status, priority, count) in &rows {
        s.total += count;
        match status {
            TicketStatus::Open => s.open += count,
            TicketStatus::InProgress => s.in_progress += count,
            TicketStatus::Closed => {}
        }
        if matches!(priority, TicketPriority::High) {
            s.high_priority += count;
        }
    }

    // Convention in this codebase: filter the typed status enum
    // via `eq_any(vec![...])` rather than `.eq(...)`. The
    // generated `TicketStatus` SqlType doesn't satisfy the
    // bounds for direct `.eq()` filters, so a single-value
    // vector is the idiomatic workaround.
    s.unassigned = tickets::table
        .filter(tickets::assignee_uuid.is_null())
        .filter(tickets::status.eq_any(vec![TicketStatus::Open, TicketStatus::InProgress]))
        .count()
        .get_result(conn)?;

    s.closed_today = tickets::table
        .filter(tickets::status.eq_any(vec![TicketStatus::Closed]))
        .filter(tickets::closed_at.ge(today_start()))
        .count()
        .get_result(conn)?;

    Ok(s)
}

fn scoped_stats_assignee(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> QueryResult<ScopedStats> {
    let rows: Vec<(TicketStatus, TicketPriority, i64)> = tickets::table
        .filter(tickets::assignee_uuid.eq(*user_uuid))
        .group_by((tickets::status, tickets::priority))
        .select((tickets::status, tickets::priority, count_star()))
        .load(conn)?;

    let mut s = aggregate_rows(&rows);

    s.closed_today = tickets::table
        .filter(tickets::assignee_uuid.eq(*user_uuid))
        .filter(tickets::status.eq_any(vec![TicketStatus::Closed]))
        .filter(tickets::closed_at.ge(today_start()))
        .count()
        .get_result(conn)?;

    Ok(s)
}

fn scoped_stats_requester(
    conn: &mut DbConnection,
    user_uuid: &Uuid,
) -> QueryResult<ScopedStats> {
    let rows: Vec<(TicketStatus, TicketPriority, i64)> = tickets::table
        .filter(tickets::requester_uuid.eq(*user_uuid))
        .group_by((tickets::status, tickets::priority))
        .select((tickets::status, tickets::priority, count_star()))
        .load(conn)?;

    let mut s = aggregate_rows(&rows);

    s.closed_today = tickets::table
        .filter(tickets::requester_uuid.eq(*user_uuid))
        .filter(tickets::status.eq_any(vec![TicketStatus::Closed]))
        .filter(tickets::closed_at.ge(today_start()))
        .count()
        .get_result(conn)?;

    Ok(s)
}

fn aggregate_rows(rows: &[(TicketStatus, TicketPriority, i64)]) -> ScopedStats {
    let mut s = ScopedStats::default();
    for (status, priority, count) in rows {
        match status {
            TicketStatus::Open => s.open += count,
            TicketStatus::InProgress => s.in_progress += count,
            TicketStatus::Closed => s.closed += count,
        }
        if matches!(priority, TicketPriority::High) {
            s.high_priority += count;
        }
    }
    s
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
        };
        let json = serde_json::to_string(&bundle).unwrap();
        assert!(json.contains("\"queue\""));
        assert!(!json.contains("\"yours\""));
        assert!(!json.contains("\"summary\""));
    }
}
