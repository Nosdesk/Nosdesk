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
///
/// `impact_score` is the count of distinct human encounters with
/// the gap. For ticket-typed signals that's unique tickets; for
/// failed-search signals it's how many times the query recurred.
/// Both are "demand for this missing doc" instances — comparable
/// enough to share one ranking column. The frontend renders the
/// badge with a label appropriate to the dominant signal type.
pub fn recompute_aggregates(conn: &mut DbConnection, gap_id: i64) -> Result<KnowledgeGap, Error> {
    use std::collections::HashSet;

    let signals = list_signals_for_gap(conn, gap_id)?;
    let evidence_count = signals.len() as i32;
    let last_evidence_at = signals.iter().map(|s| s.detected_at).max();

    let mut tickets: HashSet<i32> = HashSet::new();
    let mut search_occurrences: i64 = 0;
    for signal in &signals {
        tickets.extend(tickets_evidenced_by_signal(signal));
        if signal.signal_type == SIGNAL_FAILED_SEARCH {
            if let Some(count) = signal.payload.get("count").and_then(|v| v.as_i64()) {
                search_occurrences += count;
            }
        }
    }
    let impact_score = (tickets.len() as i64 + search_occurrences).min(i32::MAX as i64) as i32;

    update_gap(
        conn,
        gap_id,
        KnowledgeGapUpdate {
            evidence_count: Some(evidence_count),
            last_evidence_at: Some(last_evidence_at),
            impact_score: Some(impact_score),
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
/// every ticket the gap evidences gets a 'resolves' link in Phase
/// 1's documentation_page_tickets join. Manual-flag signals
/// reference one ticket (via source_ref); cluster signals reference
/// many (via payload.ticket_ids). Both shapes resolve here.
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
            for ticket_id in tickets_evidenced_by_signal(signal) {
                // Best-effort: a failed link shouldn't roll back
                // resolution itself (e.g. a member ticket may have
                // been deleted between detection and resolve).
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

/// Extract the ticket IDs a signal points to. Manual-flag signals
/// carry one ticket in source_ref; cluster signals list many in
/// payload.ticket_ids. Anything else returns an empty vec.
fn tickets_evidenced_by_signal(signal: &KnowledgeGapSignal) -> Vec<i32> {
    if signal.signal_type == SIGNAL_TICKET_CLUSTER {
        signal
            .payload
            .get("ticket_ids")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_i64().and_then(|n| i32::try_from(n).ok()))
                    .collect()
            })
            .unwrap_or_default()
    } else if signal.source_kind == SOURCE_TICKET {
        signal
            .source_ref
            .parse::<i32>()
            .ok()
            .map(|id| vec![id])
            .unwrap_or_default()
    } else {
        Vec::new()
    }
}

// =================================================================
// Cluster detection (Phase 2b)
// =================================================================
//
// Closed tickets without a 'resolves' link are candidates. We group
// by (category, most-recent-device model, channel provider). Groups
// with two or more members become `ticket_cluster` signals. The
// cluster fingerprint is the source_ref so re-running detection
// upserts the same gap, refreshing the payload as new tickets join
// the cluster.

#[derive(Debug, Clone)]
pub struct DetectedCluster {
    pub fingerprint: String,
    pub category_id: Option<i32>,
    pub category_name: Option<String>,
    pub device_model: Option<String>,
    pub channel_label: Option<String>,
    pub ticket_ids: Vec<i32>,
    pub sample_titles: Vec<String>,
}

#[derive(Debug, Default)]
pub struct DetectionStats {
    pub clusters_detected: usize,
    pub gaps_created: usize,
    pub gaps_updated: usize,
    pub new_gap_ids: Vec<i64>,
}

/// One row per closed-without-resolves-link candidate ticket,
/// pre-joined with the metadata clustering needs.
struct CandidateTicket {
    id: i32,
    title: String,
    category_id: Option<i32>,
    category_name: Option<String>,
    submitted_via: Option<String>,
    channel_provider: Option<String>,
}

fn load_candidate_tickets(
    conn: &mut DbConnection,
    days: i32,
) -> Result<Vec<CandidateTicket>, Error> {
    use crate::schema::{channels, documentation_page_tickets, ticket_categories, tickets};
    use std::collections::HashSet;

    let cutoff = Utc::now().naive_utc() - chrono::Duration::days(days as i64);

    // Two-pass: load closed tickets with metadata, then exclude the
    // ones that already have a 'resolves' link. We filter "closed"
    // by joining to workflow_states and matching the terminal
    // categories (Done, Cancelled).
    use crate::schema::workflow_states;
    use crate::models::WorkflowStateCategory;
    let rows: Vec<(i32, String, Option<i32>, Option<String>, Option<String>, Option<String>)> =
        tickets::table
            .inner_join(workflow_states::table)
            .left_join(ticket_categories::table.on(ticket_categories::id.nullable().eq(tickets::category_id)))
            .left_join(channels::table.on(channels::id.nullable().eq(tickets::origin_channel_id)))
            .filter(workflow_states::category.eq_any(vec![
                WorkflowStateCategory::Done,
                WorkflowStateCategory::Cancelled,
            ]))
            .filter(tickets::updated_at.ge(cutoff))
            .select((
                tickets::id,
                tickets::title,
                tickets::category_id,
                ticket_categories::name.nullable(),
                tickets::submitted_via,
                channels::provider.nullable(),
            ))
            .load(conn)?;

    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let candidate_ids: Vec<i32> = rows.iter().map(|r| r.0).collect();
    let resolved_ids: HashSet<i32> = documentation_page_tickets::table
        .filter(documentation_page_tickets::ticket_id.eq_any(&candidate_ids))
        .filter(
            documentation_page_tickets::link_type
                .eq(crate::repository::documentation_page_tickets::LINK_RESOLVES),
        )
        .select(documentation_page_tickets::ticket_id)
        .load::<i32>(conn)?
        .into_iter()
        .collect();

    Ok(rows
        .into_iter()
        .filter(|(id, _, _, _, _, _)| !resolved_ids.contains(id))
        .map(|(id, title, category_id, category_name, submitted_via, channel_provider)| {
            CandidateTicket {
                id,
                title,
                category_id,
                category_name,
                submitted_via,
                channel_provider,
            }
        })
        .collect())
}

/// For a set of ticket IDs, return the most-recently-linked
/// device's model per ticket. Tickets without a device or whose
/// device has no model just don't appear in the map.
fn most_recent_device_models(
    conn: &mut DbConnection,
    ticket_ids: &[i32],
) -> Result<std::collections::HashMap<i32, String>, Error> {
    use crate::schema::{devices, ticket_devices};

    if ticket_ids.is_empty() {
        return Ok(Default::default());
    }

    // DISTINCT ON (ticket_id) with ORDER BY ticket_id, created_at DESC
    // gives one row per ticket — the most-recently-linked device.
    let rows: Vec<(i32, Option<String>)> = ticket_devices::table
        .inner_join(devices::table.on(devices::id.eq(ticket_devices::device_id)))
        .filter(ticket_devices::ticket_id.eq_any(ticket_ids))
        .filter(devices::model.is_not_null())
        .distinct_on(ticket_devices::ticket_id)
        .order_by((
            ticket_devices::ticket_id,
            ticket_devices::created_at.desc(),
        ))
        .select((ticket_devices::ticket_id, devices::model))
        .load(conn)?;

    Ok(rows.into_iter().filter_map(|(tid, m)| m.map(|s| (tid, s))).collect())
}

/// Build a deterministic fingerprint string for a cluster key.
/// Keys are pipe-delimited for human readability and trivially
/// parseable; their content is opaque to anything outside this
/// module.
fn cluster_fingerprint(
    category_id: Option<i32>,
    device_model: Option<&str>,
    channel: Option<&str>,
) -> String {
    format!(
        "category:{}|device:{}|channel:{}",
        category_id.map(|id| id.to_string()).unwrap_or_else(|| "_".into()),
        device_model.unwrap_or("_"),
        channel.unwrap_or("_"),
    )
}

/// Group candidate tickets and emit clusters of size >= min_size.
/// Pure function over the loaded data so tests can drive it
/// directly without the DB.
fn group_into_clusters(
    candidates: Vec<CandidateTicket>,
    device_models: std::collections::HashMap<i32, String>,
    min_size: usize,
) -> Vec<DetectedCluster> {
    use std::collections::BTreeMap;

    type Key = (Option<i32>, Option<String>, Option<String>);
    let mut grouped: BTreeMap<Key, Vec<CandidateTicket>> = BTreeMap::new();

    for ticket in candidates {
        let device_model = device_models.get(&ticket.id).cloned();
        let channel = ticket
            .channel_provider
            .clone()
            .or_else(|| ticket.submitted_via.clone());
        let key = (ticket.category_id, device_model, channel);
        grouped.entry(key).or_default().push(ticket);
    }

    grouped
        .into_iter()
        .filter(|(_, tickets)| tickets.len() >= min_size)
        .map(|((category_id, device_model, channel), tickets)| {
            let fingerprint =
                cluster_fingerprint(category_id, device_model.as_deref(), channel.as_deref());
            // Up to three sample titles for the queue card preview.
            let sample_titles: Vec<String> =
                tickets.iter().take(3).map(|t| t.title.clone()).collect();
            let category_name = tickets
                .iter()
                .find_map(|t| t.category_name.clone());
            let ticket_ids: Vec<i32> = tickets.iter().map(|t| t.id).collect();
            DetectedCluster {
                fingerprint,
                category_id,
                category_name,
                device_model,
                channel_label: channel,
                ticket_ids,
                sample_titles,
            }
        })
        .collect()
}

/// Confidence weight for a cluster: scales with size, capped at
/// 100 (manual-flag's ceiling). 3 tickets ≈ 60, 5 ≈ 80, 7+ ≈ 100.
fn cluster_confidence(member_count: usize) -> i32 {
    let scaled = (member_count as i32) * 15;
    scaled.clamp(30, 100)
}

/// Headline copy for a cluster gap. Constructed from the
/// available facets — falls back gracefully when one's missing.
fn cluster_headline(cluster: &DetectedCluster) -> String {
    let count = cluster.ticket_ids.len();
    let mut facets: Vec<String> = Vec::new();
    if let Some(c) = &cluster.category_name {
        facets.push(c.clone());
    }
    if let Some(d) = &cluster.device_model {
        facets.push(d.clone());
    }
    if let Some(ch) = &cluster.channel_label {
        facets.push(format!("via {}", ch));
    }
    if facets.is_empty() {
        format!("{} similar tickets without docs", count)
    } else {
        format!("{} tickets — {}", count, facets.join(", "))
    }
}

/// Run cluster detection end-to-end: load candidates, group them,
/// upsert one gap + ticket_cluster signal per cluster. Idempotent;
/// re-running with new tickets joining a cluster updates the
/// signal payload and the gap aggregates.
pub fn run_cluster_detection(
    conn: &mut DbConnection,
    detected_by: Option<Uuid>,
    days: i32,
    min_size: usize,
) -> Result<DetectionStats, Error> {
    let candidates = load_candidate_tickets(conn, days)?;
    if candidates.is_empty() {
        return Ok(DetectionStats::default());
    }

    let ticket_ids: Vec<i32> = candidates.iter().map(|c| c.id).collect();
    let device_models = most_recent_device_models(conn, &ticket_ids)?;
    let clusters = group_into_clusters(candidates, device_models, min_size);

    let mut stats = DetectionStats::default();
    stats.clusters_detected = clusters.len();

    for cluster in clusters {
        conn.transaction::<_, Error, _>(|tx| {
            let existing = find_open_gap_for_source(tx, SOURCE_CLUSTER_KEY, &cluster.fingerprint)?;
            let was_created = existing.is_none();

            let gap = match existing {
                Some(g) => g,
                None => create_gap(
                    tx,
                    NewKnowledgeGap {
                        title: cluster_headline(&cluster),
                        description: None,
                        status: STATUS_OPEN.to_string(),
                        created_by: detected_by,
                        impact_score: 0,
                        evidence_count: 0,
                        last_evidence_at: None,
                    },
                )?,
            };

            attach_signal(
                tx,
                NewKnowledgeGapSignal {
                    gap_id: gap.id,
                    signal_type: SIGNAL_TICKET_CLUSTER.to_string(),
                    source_kind: SOURCE_CLUSTER_KEY.to_string(),
                    source_ref: cluster.fingerprint.clone(),
                    payload: serde_json::json!({
                        "ticket_ids": cluster.ticket_ids,
                        "sample_titles": cluster.sample_titles,
                        "category_id": cluster.category_id,
                        "category_name": cluster.category_name,
                        "device_model": cluster.device_model,
                        "channel_label": cluster.channel_label,
                        "member_count": cluster.ticket_ids.len(),
                    }),
                    confidence: cluster_confidence(cluster.ticket_ids.len()),
                    detected_by,
                },
            )?;

            recompute_aggregates(tx, gap.id)?;

            if was_created {
                stats.gaps_created += 1;
                stats.new_gap_ids.push(gap.id);
            } else {
                stats.gaps_updated += 1;
            }
            Ok(())
        })?;
    }

    Ok(stats)
}

// =================================================================
// Failed-search detection (Phase 2c)
// =================================================================
//
// Aggregates `search_query_log` rows where result_count = 0,
// grouped by their normalised query string. Queries that recurred
// `min_count` times in the window become `failed_search` signals
// on knowledge_gaps. Same upsert shape as cluster detection:
// re-running the detector picks up new occurrences and refreshes
// the count without duplicating gaps.

pub fn run_failed_search_detection(
    conn: &mut DbConnection,
    detected_by: Option<Uuid>,
    days: i32,
    min_count: i64,
) -> Result<DetectionStats, Error> {
    use crate::repository::search_query_log;

    let aggregates = search_query_log::aggregate_failed_searches(conn, days, min_count)?;
    let mut stats = DetectionStats::default();
    stats.clusters_detected = aggregates.len();

    for agg in aggregates {
        conn.transaction::<_, Error, _>(|tx| {
            let existing = find_open_gap_for_source(tx, SOURCE_SEARCH_QUERY, &agg.query_norm)?;
            let was_created = existing.is_none();

            let gap = match existing {
                Some(g) => g,
                None => create_gap(
                    tx,
                    NewKnowledgeGap {
                        title: format!("Customers searched: \"{}\"", agg.query_sample),
                        description: None,
                        status: STATUS_OPEN.to_string(),
                        created_by: detected_by,
                        impact_score: 0,
                        evidence_count: 0,
                        last_evidence_at: None,
                    },
                )?,
            };

            // Confidence scales with how often the query recurs,
            // capped at the manual-flag ceiling.
            let confidence = (agg.count as i32 * 5).clamp(20, 100);

            attach_signal(
                tx,
                NewKnowledgeGapSignal {
                    gap_id: gap.id,
                    signal_type: SIGNAL_FAILED_SEARCH.to_string(),
                    source_kind: SOURCE_SEARCH_QUERY.to_string(),
                    source_ref: agg.query_norm.clone(),
                    payload: serde_json::json!({
                        "query_sample": agg.query_sample,
                        "count": agg.count,
                        "first_seen": agg.first_seen,
                        "last_seen": agg.last_seen,
                    }),
                    confidence,
                    detected_by,
                },
            )?;

            recompute_aggregates(tx, gap.id)?;

            if was_created {
                stats.gaps_created += 1;
                stats.new_gap_ids.push(gap.id);
            } else {
                stats.gaps_updated += 1;
            }
            Ok(())
        })?;
    }

    Ok(stats)
}


// =================================================================
// Stale-doc detection (Phase 2d)
// =================================================================
//
// Cross-pollinates verification (Phase 1) with the docs<->tickets
// join (Phase 1): a page that's verified-but-stale AND has
// 'resolves' links to recently-closed tickets is a strong signal
// the doc has bit-rotted while the same kind of question keeps
// resurfacing in the queue. Surfaces as a stale_doc signal so
// editors see "this needs review" alongside the other gap types.

#[derive(Debug, Clone)]
pub struct StaleDocCandidate {
    pub page_id: i32,
    pub page_uuid: uuid::Uuid,
    pub page_title: String,
    pub page_slug: String,
    pub verified_at: chrono::NaiveDateTime,
    pub verify_interval_days: i32,
    pub recent_ticket_ids: Vec<i32>,
}

/// Find pages with stale verification that 'resolves' tickets
/// closed within the recent window. Pure DB read, no mutations.
fn find_stale_doc_candidates(
    conn: &mut DbConnection,
    recent_ticket_days: i32,
    min_recent_tickets: usize,
) -> Result<Vec<StaleDocCandidate>, Error> {
    use crate::schema::{documentation_page_tickets, documentation_pages, tickets};

    let now = chrono::Utc::now().naive_utc();
    let recent_cutoff = now - chrono::Duration::days(recent_ticket_days as i64);

    // Step 1: load verified-with-interval pages.
    let pages: Vec<(i32, uuid::Uuid, String, String, chrono::NaiveDateTime, i32)> =
        documentation_pages::table
            .filter(documentation_pages::verified_at.is_not_null())
            .filter(documentation_pages::verify_interval_days.is_not_null())
            .filter(documentation_pages::deleted_at.is_null())
            .select((
                documentation_pages::id,
                documentation_pages::uuid,
                documentation_pages::title,
                documentation_pages::slug,
                documentation_pages::verified_at.assume_not_null(),
                documentation_pages::verify_interval_days.assume_not_null(),
            ))
            .load(conn)?;

    // Filter in Rust to "stale": verified_at + interval < now.
    // SQL-side date arithmetic with chrono::Duration is awkward
    // through Diesel; the page set is small (one row per doc that
    // has been verified) so post-filter is cheap.
    let stale: Vec<_> = pages
        .into_iter()
        .filter(|(_, _, _, _, verified_at, days)| {
            *verified_at + chrono::Duration::days(*days as i64) < now
        })
        .collect();

    if stale.is_empty() {
        return Ok(Vec::new());
    }

    let stale_ids: Vec<i32> = stale.iter().map(|(id, _, _, _, _, _)| *id).collect();

    // Step 2: which of those pages have 'resolves' links to
    // tickets that closed in the recent window? Join workflow_states
    // so we can filter on the terminal categories.
    use crate::schema::workflow_states;
    use crate::models::WorkflowStateCategory;
    let resolves_tickets: Vec<(i32, i32)> = documentation_page_tickets::table
        .inner_join(tickets::table.on(tickets::id.eq(documentation_page_tickets::ticket_id)))
        .inner_join(
            workflow_states::table.on(workflow_states::id.eq(tickets::workflow_state_id)),
        )
        .filter(documentation_page_tickets::page_id.eq_any(&stale_ids))
        .filter(
            documentation_page_tickets::link_type
                .eq(crate::repository::documentation_page_tickets::LINK_RESOLVES),
        )
        .filter(workflow_states::category.eq_any(vec![
            WorkflowStateCategory::Done,
            WorkflowStateCategory::Cancelled,
        ]))
        .filter(tickets::updated_at.ge(recent_cutoff))
        .select((documentation_page_tickets::page_id, tickets::id))
        .load(conn)?;

    use std::collections::HashMap;
    let mut by_page: HashMap<i32, Vec<i32>> = HashMap::new();
    for (page_id, ticket_id) in resolves_tickets {
        by_page.entry(page_id).or_default().push(ticket_id);
    }

    Ok(stale
        .into_iter()
        .filter_map(|(page_id, page_uuid, page_title, page_slug, verified_at, verify_interval_days)| {
            let recent_ticket_ids = by_page.remove(&page_id)?;
            if recent_ticket_ids.len() < min_recent_tickets {
                return None;
            }
            Some(StaleDocCandidate {
                page_id,
                page_uuid,
                page_title,
                page_slug,
                verified_at,
                verify_interval_days,
                recent_ticket_ids,
            })
        })
        .collect())
}

/// Run stale-doc detection: upsert one stale_doc signal per
/// candidate page. Same upsert shape as cluster + failed-search:
/// re-running picks up new tickets joining the recent window
/// without duplicating gaps. Idempotent.
pub fn run_stale_doc_detection(
    conn: &mut DbConnection,
    detected_by: Option<Uuid>,
    recent_ticket_days: i32,
    min_recent_tickets: usize,
) -> Result<DetectionStats, Error> {
    let candidates = find_stale_doc_candidates(conn, recent_ticket_days, min_recent_tickets)?;
    let mut stats = DetectionStats::default();
    stats.clusters_detected = candidates.len();

    let now = chrono::Utc::now().naive_utc();
    for candidate in candidates {
        conn.transaction::<_, Error, _>(|tx| {
            let source_ref = candidate.page_id.to_string();
            let existing = find_open_gap_for_source(tx, SOURCE_PAGE, &source_ref)?;
            let was_created = existing.is_none();

            let days_stale = (now
                - (candidate.verified_at
                    + chrono::Duration::days(candidate.verify_interval_days as i64)))
            .num_days()
            .max(0);

            let gap = match existing {
                Some(g) => g,
                None => create_gap(
                    tx,
                    NewKnowledgeGap {
                        title: format!("Doc may be stale: {}", candidate.page_title),
                        description: None,
                        status: STATUS_OPEN.to_string(),
                        created_by: detected_by,
                        impact_score: 0,
                        evidence_count: 0,
                        last_evidence_at: None,
                    },
                )?,
            };

            // Confidence scales with both staleness age and recent
            // ticket activity, capped at the manual-flag ceiling.
            let confidence = (40
                + (days_stale.min(60) as i32) / 2
                + (candidate.recent_ticket_ids.len() as i32 * 5))
                .clamp(40, 100);

            attach_signal(
                tx,
                NewKnowledgeGapSignal {
                    gap_id: gap.id,
                    signal_type: SIGNAL_STALE_DOC.to_string(),
                    source_kind: SOURCE_PAGE.to_string(),
                    source_ref,
                    payload: serde_json::json!({
                        "page_uuid": candidate.page_uuid,
                        "page_title": candidate.page_title,
                        "page_slug": candidate.page_slug,
                        "verified_at": candidate.verified_at,
                        "verify_interval_days": candidate.verify_interval_days,
                        "days_stale": days_stale,
                        "recent_ticket_ids": candidate.recent_ticket_ids,
                    }),
                    confidence,
                    detected_by,
                },
            )?;

            recompute_aggregates(tx, gap.id)?;

            if was_created {
                stats.gaps_created += 1;
                stats.new_gap_ids.push(gap.id);
            } else {
                stats.gaps_updated += 1;
            }
            Ok(())
        })?;
    }

    Ok(stats)
}

/// Auto-dismiss any open stale_doc gaps for a page that just
/// got re-verified. Closes the editorial loop without forcing
/// the verifier to remember to also touch the gap queue.
/// Returns the count of gaps dismissed.
pub fn dismiss_stale_doc_gaps_for_page(
    conn: &mut DbConnection,
    page_id: i32,
    by_user: Uuid,
) -> Result<usize, Error> {
    let source_ref = page_id.to_string();
    let signals: Vec<KnowledgeGapSignal> = knowledge_gap_signals::table
        .inner_join(knowledge_gaps::table.on(knowledge_gaps::id.eq(knowledge_gap_signals::gap_id)))
        .filter(knowledge_gap_signals::signal_type.eq(SIGNAL_STALE_DOC))
        .filter(knowledge_gap_signals::source_kind.eq(SOURCE_PAGE))
        .filter(knowledge_gap_signals::source_ref.eq(&source_ref))
        .filter(knowledge_gap_signals::dismissed_at.is_null())
        .filter(knowledge_gaps::status.eq_any([STATUS_OPEN, STATUS_DRAFTING]))
        .select(knowledge_gap_signals::all_columns)
        .load(conn)?;

    let mut count = 0;
    for signal in signals {
        let _ = dismiss_signal(conn, signal.id, Some(by_user))?;
        let _ = update_gap(
            conn,
            signal.gap_id,
            KnowledgeGapUpdate {
                status: Some(STATUS_DISMISSED.to_string()),
                dismissed_at: Some(Some(chrono::Utc::now().naive_utc())),
                dismissed_by: Some(Some(by_user)),
                updated_at: Some(chrono::Utc::now().naive_utc()),
                ..Default::default()
            },
        )?;
        count += 1;
    }
    Ok(count)
}
