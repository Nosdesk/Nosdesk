//! Knowledge Gaps repository.
//!
//! Phase 2a of the docs/KB redesign. Two entities:
//!   - `knowledge_gaps`: canonical editorial entity with lifecycle
//!     (open → drafting → resolved | dismissed) and ranking metadata.
//!   - `knowledge_gap_signals`: raw evidence with a polymorphic
//!     source reference. Each detection mechanism (manual flag in
//!     2a, ticket clusters in 2b, failed searches in 2c, stale
//!     docs in 2d, AI-suggested in Phase 3) writes here.
//!
//! Everything in here is composable so the four detection
//! mechanisms can share code: `find_open_gap_for_source` is the
//! dedup primitive, `attach_signal` adds evidence to a gap (or
//! creates one), and `resolve_gap` cascades into Phase 1's
//! documentation_page_tickets join so resolution lineage carries
//! through.

use crate::db::DbConnection;
use crate::models::{
    KnowledgeGap, KnowledgeGapSignal, KnowledgeGapUpdate, NewKnowledgeGap, NewKnowledgeGapSignal,
};
use crate::schema::{knowledge_gap_signals, knowledge_gaps};
use chrono::Utc;
use diesel::prelude::*;
use diesel::result::Error;
use uuid::Uuid;

// -----------------------------------------------------------------
// Constants — kept here so handlers don't sprinkle string literals.
// -----------------------------------------------------------------

pub const STATUS_OPEN: &str = "open";
pub const STATUS_DRAFTING: &str = "drafting";
pub const STATUS_RESOLVED: &str = "resolved";
pub const STATUS_DISMISSED: &str = "dismissed";

pub const SIGNAL_MANUAL_FLAG: &str = "manual_flag";
pub const SIGNAL_TICKET_CLUSTER: &str = "ticket_cluster";
pub const SIGNAL_FAILED_SEARCH: &str = "failed_search";
pub const SIGNAL_STALE_DOC: &str = "stale_doc";
pub const SIGNAL_AI_SUGGESTED: &str = "ai_suggested";

pub const SOURCE_TICKET: &str = "ticket";
pub const SOURCE_SEARCH_QUERY: &str = "search_query";
pub const SOURCE_CLUSTER_KEY: &str = "cluster_key";
pub const SOURCE_PAGE: &str = "page";

/// Confidence weights per signal type. Manual flag is 100 because
/// a human said so; auto-detection signals are scaled by their
/// own logic (cluster size, query frequency) before reaching here.
pub fn default_confidence(signal_type: &str) -> i32 {
    match signal_type {
        SIGNAL_MANUAL_FLAG => 100,
        SIGNAL_TICKET_CLUSTER => 60,
        SIGNAL_FAILED_SEARCH => 40,
        SIGNAL_STALE_DOC => 50,
        SIGNAL_AI_SUGGESTED => 70,
        _ => 50,
    }
}

// -----------------------------------------------------------------
// Read paths
// -----------------------------------------------------------------

/// Find an open (or drafting) gap that already covers a given
/// source. Used by every detection mechanism to dedup before
/// creating a new gap. Joins through the live signal index so
/// dismissed signals don't keep a closed gap alive in the lookup.
pub fn find_open_gap_for_source(
    conn: &mut DbConnection,
    source_kind: &str,
    source_ref: &str,
) -> Result<Option<KnowledgeGap>, Error> {
    knowledge_gaps::table
        .inner_join(
            knowledge_gap_signals::table
                .on(knowledge_gap_signals::gap_id.eq(knowledge_gaps::id)),
        )
        .filter(knowledge_gap_signals::source_kind.eq(source_kind))
        .filter(knowledge_gap_signals::source_ref.eq(source_ref))
        .filter(knowledge_gap_signals::dismissed_at.is_null())
        .filter(knowledge_gaps::status.eq_any([STATUS_OPEN, STATUS_DRAFTING]))
        .select(knowledge_gaps::all_columns)
        .first::<KnowledgeGap>(conn)
        .optional()
}

pub fn get_gap(conn: &mut DbConnection, gap_id: i64) -> Result<KnowledgeGap, Error> {
    knowledge_gaps::table.find(gap_id).first(conn)
}

pub fn list_signals_for_gap(
    conn: &mut DbConnection,
    gap_id: i64,
) -> Result<Vec<KnowledgeGapSignal>, Error> {
    knowledge_gap_signals::table
        .filter(knowledge_gap_signals::gap_id.eq(gap_id))
        .filter(knowledge_gap_signals::dismissed_at.is_null())
        .order_by(knowledge_gap_signals::detected_at.desc())
        .load(conn)
}

/// Filter shape for the queue view. All filters are optional;
/// callers compose with whatever subset they need.
#[derive(Debug, Default)]
pub struct GapListFilter {
    /// Statuses to include. Empty = ['open', 'drafting'] default.
    pub statuses: Vec<String>,
    /// Limit / offset for pagination.
    pub limit: i64,
    pub offset: i64,
}

pub fn list_gaps(
    conn: &mut DbConnection,
    filter: GapListFilter,
) -> Result<Vec<KnowledgeGap>, Error> {
    use diesel::dsl::sql;
    use diesel::sql_types::Bool;

    let statuses = if filter.statuses.is_empty() {
        vec![STATUS_OPEN.to_string(), STATUS_DRAFTING.to_string()]
    } else {
        filter.statuses
    };

    knowledge_gaps::table
        .filter(knowledge_gaps::status.eq_any(statuses))
        .filter(sql::<Bool>("true"))
        .order_by((
            knowledge_gaps::impact_score.desc(),
            knowledge_gaps::last_evidence_at.desc().nulls_last(),
        ))
        .limit(if filter.limit > 0 { filter.limit } else { 100 })
        .offset(filter.offset)
        .load(conn)
}

// -----------------------------------------------------------------
// Mutations
// -----------------------------------------------------------------

/// Insert a new gap. Caller is responsible for attaching at least
/// one signal afterwards via `attach_signal_to_gap`.
pub fn create_gap(conn: &mut DbConnection, new_gap: NewKnowledgeGap) -> Result<KnowledgeGap, Error> {
    diesel::insert_into(knowledge_gaps::table)
        .values(&new_gap)
        .get_result(conn)
}

pub fn update_gap(
    conn: &mut DbConnection,
    gap_id: i64,
    update: KnowledgeGapUpdate,
) -> Result<KnowledgeGap, Error> {
    diesel::update(knowledge_gaps::table.find(gap_id))
        .set(update)
        .get_result(conn)
}

pub fn attach_signal(
    conn: &mut DbConnection,
    new_signal: NewKnowledgeGapSignal,
) -> Result<KnowledgeGapSignal, Error> {
    diesel::insert_into(knowledge_gap_signals::table)
        .values(&new_signal)
        .on_conflict((
            knowledge_gap_signals::gap_id,
            knowledge_gap_signals::source_kind,
            knowledge_gap_signals::source_ref,
        ))
        .do_update()
        .set((
            knowledge_gap_signals::dismissed_at.eq(None::<chrono::NaiveDateTime>),
            knowledge_gap_signals::dismissed_by.eq(None::<Uuid>),
            knowledge_gap_signals::confidence.eq(new_signal.confidence),
            knowledge_gap_signals::payload.eq(new_signal.payload.clone()),
        ))
        .get_result(conn)
}

/// Dismiss a single signal (e.g. user unflags one ticket). Returns
/// the count of *live* signals remaining on the gap so callers can
/// decide whether to dismiss the whole gap.
pub fn dismiss_signal(
    conn: &mut DbConnection,
    signal_id: i64,
    by_user: Option<Uuid>,
) -> Result<i64, Error> {
    let gap_id: i64 = diesel::update(knowledge_gap_signals::table.find(signal_id))
        .set((
            knowledge_gap_signals::dismissed_at.eq(Some(Utc::now().naive_utc())),
            knowledge_gap_signals::dismissed_by.eq(by_user),
        ))
        .returning(knowledge_gap_signals::gap_id)
        .get_result(conn)?;

    knowledge_gap_signals::table
        .filter(knowledge_gap_signals::gap_id.eq(gap_id))
        .filter(knowledge_gap_signals::dismissed_at.is_null())
        .count()
        .get_result(conn)
}

/// Recompute and persist `evidence_count`, `last_evidence_at`,
/// and `impact_score` from the gap's currently-live signals.
/// Cheap to call after every signal mutation; one query each.
pub fn recompute_aggregates(conn: &mut DbConnection, gap_id: i64) -> Result<KnowledgeGap, Error> {
    use diesel::dsl::{max, sum};

    let (count_opt, last_opt, impact_opt): (
        Option<i64>,
        Option<chrono::NaiveDateTime>,
        Option<i64>,
    ) = knowledge_gap_signals::table
        .filter(knowledge_gap_signals::gap_id.eq(gap_id))
        .filter(knowledge_gap_signals::dismissed_at.is_null())
        .select((
            diesel::dsl::count(knowledge_gap_signals::id).nullable(),
            max(knowledge_gap_signals::detected_at),
            sum(knowledge_gap_signals::confidence),
        ))
        .first(conn)?;

    let count = count_opt.unwrap_or(0) as i32;
    let impact = impact_opt.unwrap_or(0).min(i32::MAX as i64) as i32;

    update_gap(
        conn,
        gap_id,
        KnowledgeGapUpdate {
            evidence_count: Some(count),
            last_evidence_at: Some(last_opt),
            impact_score: Some(impact),
            updated_at: Some(Utc::now().naive_utc()),
            ..Default::default()
        },
    )
}

/// Manual-flag entry point. Idempotent: if an open or drafting
/// gap already covers this ticket, attach a manual_flag signal to
/// it (or refresh an existing one); otherwise create a new gap
/// with a single manual_flag signal. Returns the gap.
pub fn flag_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    ticket_title: &str,
    flagged_by: Uuid,
    reason: Option<String>,
) -> Result<(KnowledgeGap, KnowledgeGapSignal, bool), Error> {
    // (gap, signal, was_created)
    conn.transaction::<_, Error, _>(|tx| {
        let source_ref = ticket_id.to_string();
        let existing = find_open_gap_for_source(tx, SOURCE_TICKET, &source_ref)?;

        let gap = match existing {
            Some(g) => g,
            None => create_gap(
                tx,
                NewKnowledgeGap {
                    title: format!("Ticket #{}: {}", ticket_id, ticket_title),
                    description: reason.clone(),
                    status: STATUS_OPEN.to_string(),
                    created_by: Some(flagged_by),
                    impact_score: 0,
                    evidence_count: 0,
                    last_evidence_at: None,
                },
            )?,
        };

        let signal = attach_signal(
            tx,
            NewKnowledgeGapSignal {
                gap_id: gap.id,
                signal_type: SIGNAL_MANUAL_FLAG.to_string(),
                source_kind: SOURCE_TICKET.to_string(),
                source_ref,
                payload: serde_json::json!({
                    "reason": reason,
                    "ticket_title": ticket_title,
                }),
                confidence: default_confidence(SIGNAL_MANUAL_FLAG),
                detected_by: Some(flagged_by),
            },
        )?;

        let was_created = gap.created_at == gap.updated_at && gap.evidence_count == 0;
        let refreshed = recompute_aggregates(tx, gap.id)?;
        Ok((refreshed, signal, was_created))
    })
}

/// Inverse of `flag_ticket`. Dismisses the user's manual_flag
/// signal on the given ticket. If that was the gap's last live
/// signal, the gap auto-dismisses too. Returns the (possibly
/// updated) gap.
pub fn unflag_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
    by_user: Uuid,
) -> Result<Option<KnowledgeGap>, Error> {
    conn.transaction::<_, Error, _>(|tx| {
        let source_ref = ticket_id.to_string();
        let signal: Option<KnowledgeGapSignal> = knowledge_gap_signals::table
            .filter(knowledge_gap_signals::source_kind.eq(SOURCE_TICKET))
            .filter(knowledge_gap_signals::source_ref.eq(&source_ref))
            .filter(knowledge_gap_signals::signal_type.eq(SIGNAL_MANUAL_FLAG))
            .filter(knowledge_gap_signals::dismissed_at.is_null())
            .first(tx)
            .optional()?;

        let Some(signal) = signal else {
            return Ok(None);
        };

        let remaining = dismiss_signal(tx, signal.id, Some(by_user))?;
        if remaining == 0 {
            // Last live signal — auto-dismiss the gap.
            let gap = update_gap(
                tx,
                signal.gap_id,
                KnowledgeGapUpdate {
                    status: Some(STATUS_DISMISSED.to_string()),
                    dismissed_at: Some(Some(Utc::now().naive_utc())),
                    dismissed_by: Some(Some(by_user)),
                    updated_at: Some(Utc::now().naive_utc()),
                    ..Default::default()
                },
            )?;
            return Ok(Some(gap));
        }
        let gap = recompute_aggregates(tx, signal.gap_id)?;
        Ok(Some(gap))
    })
}

/// Dismiss a gap explicitly (the user said "this isn't really a
/// gap"). Cascades nothing — signals stay so the dismissal is
/// recoverable if the gap is later re-opened.
pub fn dismiss_gap(
    conn: &mut DbConnection,
    gap_id: i64,
    by_user: Uuid,
) -> Result<KnowledgeGap, Error> {
    update_gap(
        conn,
        gap_id,
        KnowledgeGapUpdate {
            status: Some(STATUS_DISMISSED.to_string()),
            dismissed_at: Some(Some(Utc::now().naive_utc())),
            dismissed_by: Some(Some(by_user)),
            updated_at: Some(Utc::now().naive_utc()),
            ..Default::default()
        },
    )
}

/// Resolve a gap by linking it to a documentation page. Cascades:
/// every ticket-typed signal on the gap gets a 'resolves' link
/// inserted into Phase 1's documentation_page_tickets join, so the
/// page's "Linked tickets" panel populates automatically.
pub fn resolve_gap(
    conn: &mut DbConnection,
    gap_id: i64,
    page_id: i32,
    by_user: Uuid,
) -> Result<KnowledgeGap, Error> {
    use crate::repository::documentation_page_tickets;

    conn.transaction::<_, Error, _>(|tx| {
        let signals = list_signals_for_gap(tx, gap_id)?;
        for signal in &signals {
            if signal.source_kind != SOURCE_TICKET {
                continue;
            }
            if let Ok(ticket_id) = signal.source_ref.parse::<i32>() {
                // Best-effort: a failed link shouldn't roll back
                // the resolution itself (e.g. ticket might have
                // been deleted). Ignore individual errors.
                let _ = documentation_page_tickets::upsert_link(
                    tx,
                    page_id,
                    ticket_id,
                    documentation_page_tickets::LINK_RESOLVES,
                    Some(by_user),
                );
            }
        }
        update_gap(
            tx,
            gap_id,
            KnowledgeGapUpdate {
                status: Some(STATUS_RESOLVED.to_string()),
                resolved_page_id: Some(Some(page_id)),
                resolved_at: Some(Some(Utc::now().naive_utc())),
                updated_at: Some(Utc::now().naive_utc()),
                ..Default::default()
            },
        )
    })
}
