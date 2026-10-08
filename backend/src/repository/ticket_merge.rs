//! Ticket merge lifecycle.
//!
//! `execute_merge` folds one or more source tickets into a destination
//! ticket inside a single transaction: it moves comments and channel
//! messages, unions watchers / project / cycle / asset / tag / doc
//! links, rewrites the sources' other ticket links onto the
//! destination, records a `duplicate_of` edge per source, writes a
//! structured merge-marker comment on the destination, and emits the
//! `ticket.merged` / `ticket.merged_into` sync events. The whole thing
//! runs under `with_actor_context` so every audited write and every
//! sync row shares the actor's `correlation_id`.
//!
//! Lifecycle order is deterministic. The
//! handler stays thin: parse, authorise, call `execute_merge`, format
//! the response.

use diesel::prelude::*;
use diesel::sql_types::{Array, BigInt, Integer};
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{
    Comment, ContentFormat, NewComment, NewOutboundEmail, SyncAggregate, SyncOp, Ticket,
    WorkflowStateCategory,
};
use crate::sync::actor::ActorContext;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;
use crate::sync::session::with_actor_context;

/// The optimistic-lock token for one ticket: the workflow_state_id the
/// client last saw. The merge aborts if any ticket's state has moved
/// since the dialog opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedState {
    pub ticket_id: i32,
    pub workflow_state_id: i32,
}

/// Parsed merge request plus the resolved actor's workspace. The
/// handler builds this from the request body.
#[derive(Debug, Clone)]
pub struct MergeInput {
    pub destination_ticket_id: i32,
    pub source_ticket_ids: Vec<i32>,
    pub reason: Option<String>,
    /// Carried through to the post-commit notification step (Commit 5);
    /// the lifecycle records it in the `ticket.merged` event so the
    /// audit trail shows whether the customer was told.
    pub notify_customer: bool,
    /// Optional optimistic-lock tokens. Empty means "skip the check".
    pub expected_state: Vec<ExpectedState>,
    /// Agent-edited body for the merge-marker comment (the merge
    /// dialog's description area). `None` falls back to the generated
    /// summary. The structured channel_metadata is always attached so
    /// the activity card renders regardless.
    pub marker_body: Option<String>,
}

/// Counts and identifiers the API echoes back after a successful merge.
#[derive(Debug, Clone)]
pub struct MergeOutcome {
    pub merge_event_id: i64,
    pub destination: Ticket,
    pub merged_sources: Vec<Ticket>,
    pub comments_moved: usize,
    pub channel_messages_rerouted: usize,
    pub watchers_added_to_destination: usize,
    pub merge_marker_comment_id: i32,
    pub correlation_id: Option<Uuid>,
}

/// Pre-flight and execution failures. The handler maps each variant to
/// an HTTP status + machine-readable code.
#[derive(Debug)]
pub enum MergeError {
    /// No source ids supplied.
    EmptySources,
    /// A source id equals the destination id.
    SelfMerge(i32),
    /// A source or the destination (by number) is already a merge source.
    AlreadyMerged(i32),
    /// The destination sits in the terminal `merged` category.
    DestinationIsMerged,
    /// A ticket id resolved to a different workspace than the actor's.
    CrossWorkspace(i32),
    /// The destination is a recurrence series parent and must not
    /// absorb other tickets.
    RecurrenceParentDestination,
    /// A source or the destination id does not resolve in this
    /// workspace.
    NotFound(i32),
    /// One or more tickets' workflow state moved since the client
    /// snapshot. Carries the actual current states for the diverged
    /// tickets so the handler can tell the user which ones changed.
    StateConflict(Vec<ExpectedState>),
    /// The workspace has no seeded `merged` workflow state (should
    /// never happen; the migration seeds one per workspace).
    MergedStateMissing,
    /// The actor context carried no workspace id.
    MissingWorkspace,
    /// Any other database error.
    Db(diesel::result::Error),
}

impl From<diesel::result::Error> for MergeError {
    fn from(e: diesel::result::Error) -> Self {
        MergeError::Db(e)
    }
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MergeError::EmptySources => write!(f, "no source tickets supplied"),
            MergeError::SelfMerge(id) => {
                write!(f, "ticket {id} is both a source and the destination")
            }
            MergeError::AlreadyMerged(number) => write!(f, "ticket #{number} is already merged"),
            MergeError::DestinationIsMerged => write!(f, "destination ticket is itself merged"),
            MergeError::CrossWorkspace(id) => write!(f, "ticket {id} is in a different workspace"),
            MergeError::RecurrenceParentDestination => {
                write!(
                    f,
                    "destination is a recurrence parent and cannot absorb tickets"
                )
            }
            MergeError::NotFound(id) => write!(f, "ticket {id} not found"),
            MergeError::StateConflict(_) => write!(f, "tickets changed since the merge was opened"),
            MergeError::MergedStateMissing => write!(f, "workspace has no merged workflow state"),
            MergeError::MissingWorkspace => write!(f, "actor has no workspace context"),
            MergeError::Db(e) => write!(f, "database error: {e}"),
        }
    }
}

impl std::error::Error for MergeError {}

/// Encode `(workspace_id, ticket_id)` into one int64 advisory-lock key
/// so locks never collide across workspaces.
fn advisory_key(workspace_id: i32, ticket_id: i32) -> i64 {
    ((workspace_id as i64) << 32) | (ticket_id as i64 & 0xffff_ffff)
}

/// Merge `input.source_ticket_ids` into `input.destination_ticket_id`.
///
/// Runs every step in one `with_actor_context` transaction; any
/// pre-flight failure or DB error rolls the whole thing back. Post-
/// commit concerns (search reindex, SSE, customer notification) are the
/// caller's job and live in later commits.
pub fn execute_merge(
    conn: &mut DbConnection,
    input: MergeInput,
    actor: &ActorContext,
) -> Result<MergeOutcome, MergeError> {
    use crate::schema::{ticket_merges, tickets, workflow_states};

    let workspace_id = actor.workspace_id.ok_or(MergeError::MissingWorkspace)?;
    let target_id = input.destination_ticket_id;

    if input.source_ticket_ids.is_empty() {
        return Err(MergeError::EmptySources);
    }

    // Dedup sources and reject a source that equals the destination.
    let mut source_ids: Vec<i32> = input.source_ticket_ids.clone();
    source_ids.sort_unstable();
    source_ids.dedup();
    if let Some(&dup) = source_ids.iter().find(|&&id| id == target_id) {
        return Err(MergeError::SelfMerge(dup));
    }

    let reason = input
        .reason
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    with_actor_context(conn, actor, |conn| {
        // Step 2: advisory-lock every involved ticket, sorted so two
        // overlapping merges acquire in the same order and can't
        // deadlock. The second caller blocks until the first commits,
        // then sees the new merged state and fails pre-flight cleanly.
        let mut lock_ids: Vec<i32> = source_ids.clone();
        lock_ids.push(target_id);
        lock_ids.sort_unstable();
        lock_ids.dedup();
        for id in &lock_ids {
            diesel::sql_query("SELECT pg_advisory_xact_lock($1)")
                .bind::<BigInt, _>(advisory_key(workspace_id, *id))
                .execute(conn)?;
        }

        // Resolve the workspace's merged workflow state up front.
        let merged_state_id: i32 = workflow_states::table
            .filter(workflow_states::category.eq(WorkflowStateCategory::Merged))
            .filter(workflow_states::workspace_id.eq(workspace_id))
            .select(workflow_states::id)
            .first(conn)
            .optional()?
            .ok_or(MergeError::MergedStateMissing)?;

        // Step 3: re-read destination + sources under the lock.
        let destination = load_ticket(conn, target_id)?;
        if destination.workspace_id != workspace_id {
            return Err(MergeError::CrossWorkspace(target_id));
        }
        if is_merge_source(conn, target_id)? {
            return Err(MergeError::AlreadyMerged(destination.number));
        }
        if state_category(conn, destination.workflow_state_id)? == WorkflowStateCategory::Merged {
            return Err(MergeError::DestinationIsMerged);
        }
        // A recurrence series parent (carries an RRULE) must not absorb
        // tickets: the next occurrence would inherit polluted state.
        if destination.recurrence_rule.is_some() {
            return Err(MergeError::RecurrenceParentDestination);
        }

        let mut sources: Vec<Ticket> = Vec::with_capacity(source_ids.len());
        for &sid in &source_ids {
            let s = load_ticket(conn, sid)?;
            if s.workspace_id != workspace_id {
                return Err(MergeError::CrossWorkspace(sid));
            }
            if is_merge_source(conn, sid)? {
                return Err(MergeError::AlreadyMerged(s.number));
            }
            sources.push(s);
        }

        // Optimistic lock: every supplied token must still match.
        if !input.expected_state.is_empty() {
            let mut diverged = Vec::new();
            let mut check = |t: &Ticket| {
                if let Some(exp) = input.expected_state.iter().find(|e| e.ticket_id == t.id) {
                    if exp.workflow_state_id != t.workflow_state_id {
                        diverged.push(ExpectedState {
                            ticket_id: t.id,
                            workflow_state_id: t.workflow_state_id,
                        });
                    }
                }
            };
            check(&destination);
            sources.iter().for_each(&mut check);
            if !diverged.is_empty() {
                return Err(MergeError::StateConflict(diverged));
            }
        }

        let source_array = source_ids.clone();

        // Step 4: move sources to the merged state, and record the merge in
        // the satellite (one row per source). Merge metadata used to be four
        // columns on `tickets`; it now lives in `ticket_merges`.
        diesel::update(tickets::table.filter(tickets::id.eq_any(&source_array)))
            .set((
                tickets::workflow_state_id.eq(merged_state_id),
                tickets::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;
        let merged_at = chrono::Utc::now().naive_utc();
        let merge_rows: Vec<crate::models::NewTicketMerge> = source_array
            .iter()
            .map(|&sid| crate::models::NewTicketMerge {
                ticket_id: sid,
                merged_into_ticket_id: target_id,
                merged_at,
                merged_by_user_uuid: actor.uuid,
                merge_reason: reason.clone(),
            })
            .collect();
        diesel::insert_into(ticket_merges::table)
            .values(&merge_rows)
            .execute(conn)?;
        // A merged source is finished: recompute its SLA so the targets it
        // carried while open are cleared and the breach job has nothing left
        // to find.
        for &sid in &source_array {
            let source = load_ticket(conn, sid)?;
            crate::services::sla::recompute_and_stamp_sla_for_ticket(conn, &source);
        }

        // Step 5: move the replies, with their files, to the destination.
        let comments_moved = crate::repository::comments::move_comments_to_ticket(
            conn,
            &source_array,
            &destination,
        )?;

        // Step 6: reroute channel messages so future inbound replies
        // thread onto the destination.
        let channel_messages_rerouted = diesel::sql_query(
            "UPDATE channel_messages SET ticket_id = $1 WHERE ticket_id = ANY($2)",
        )
        .bind::<Integer, _>(target_id)
        .bind::<Array<Integer>, _>(&source_array)
        .execute(conn)?;

        // Step 7: everything else the sources carry moves through its usual
        // writer, so every client and webhook hears of it. Watchers, tags and
        // doc links accumulate on the destination; the sources keep theirs
        // (the source is still a real record). Projects, the cycle and
        // linked assets leave the sources: closed records shouldn't show on
        // boards.
        let watchers_added_to_destination = move_watchers(conn, target_id, &source_array)?;
        move_projects(conn, target_id, &source_array)?;
        move_cycle(conn, target_id, &source_array, actor.uuid)?;
        move_assets(conn, target_id, &source_array)?;
        union_tags(conn, target_id, &source_array, actor.uuid)?;
        crate::repository::documentation_page_tickets::copy_links_to_ticket(
            conn,
            &source_array,
            target_id,
        )?;

        // Step 8: rewrite the sources' OTHER ticket links onto the
        // destination (both directions), then drop every source link.
        move_links(conn, target_id, &source_array)?;

        // Step 9: record the canonical merge edge, one per source.
        for &sid in &source_ids {
            crate::repository::linked_tickets::link_tickets_directional(
                conn,
                sid,
                target_id,
                "duplicate_of",
                reason.clone(),
                actor.uuid,
            )?;
        }

        // Step 10: write the structured merge-marker comment on the
        // destination. Inserted directly (not via create_comment) so it
        // does NOT stamp the destination's first_response_at: a merge
        // marker is bookkeeping, not a staff reply to the customer.
        let marker = build_marker(
            &destination,
            &sources,
            actor,
            reason.as_deref(),
            input.marker_body.as_deref(),
        );
        let marker_comment: Comment = diesel::insert_into(crate::schema::comments::table)
            .values(&marker)
            .get_result(conn)?;

        let groups = groups::for_ticket(conn, &destination)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Comment,
                aggregate_id: marker_comment.id.to_string(),
                op: SyncOp::Insert,
                event_type: "comment.created",
                // Carry the render essentials (+ id) so the marker lands
                // as a real pool comment on the destination timeline,
                // pool-native (Phase 2), instead of a skipped side-event.
                data: json!({
                    "id": marker_comment.id,
                    "ticket_id": target_id,
                    "user_uuid": marker_comment.user_uuid,
                    "is_internal": marker_comment.is_internal,
                    "content_format": marker_comment.content_format,
                    "content": marker_comment.content,
                    "created_at": marker_comment.created_at,
                    "kind": "merge_marker",
                }),
                groups: groups.clone(),
                causation_id: None,
            },
        )?;

        // Step 11: emit the first-class merge events. One aggregate
        // event on the destination, one on each source. All share the
        // actor's correlation_id via the session GUC, so the audit log
        // and these rows stitch together.
        let merge_event_id = emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: target_id.to_string(),
                op: SyncOp::Update,
                event_type: "ticket.merged",
                data: json!({
                    "source_ticket_ids": source_ids,
                    "actor_uuid": actor.uuid,
                    "reason": reason,
                    "comments_moved": comments_moved,
                    "channel_messages_rerouted": channel_messages_rerouted,
                    "watchers_added": watchers_added_to_destination,
                    "customer_notified": input.notify_customer,
                    "merge_marker_comment_id": marker_comment.id,
                }),
                groups,
                causation_id: None,
            },
        )?;

        // Re-read the now-merged sources: their sync rows and the response
        // both carry the merged state.
        let mut merged_sources = Vec::with_capacity(source_ids.len());
        for &sid in &source_ids {
            merged_sources.push(load_ticket(conn, sid)?);
        }
        for source in &merged_sources {
            let source_groups = groups::for_ticket(conn, source)?;
            // The whole row moves the source out of every client's open
            // lists and kanban column; the merge fields drive the merged-into
            // banner and read-only composer. No `previous_*` keys, so the
            // merge raises no status-change or assignment notification; the
            // requester hears of it through the merge notice.
            let mut data = crate::repository::tickets::ticket_sync_row(conn, source)?;
            if let Some(obj) = data.as_object_mut() {
                obj.insert("merged_into_ticket_id".into(), json!(target_id));
                obj.insert("merged_at".into(), json!(merged_at));
                obj.insert("merged_by_user_uuid".into(), json!(actor.uuid));
                obj.insert("actor_uuid".into(), json!(actor.uuid));
            }
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::Ticket,
                    aggregate_id: source.id.to_string(),
                    op: SyncOp::Update,
                    event_type: "ticket.merged_into",
                    data,
                    groups: source_groups,
                    causation_id: None,
                },
            )?;
        }

        let destination = load_ticket(conn, target_id)?;

        Ok(MergeOutcome {
            merge_event_id,
            destination,
            merged_sources,
            comments_moved,
            channel_messages_rerouted,
            watchers_added_to_destination,
            merge_marker_comment_id: marker_comment.id,
            correlation_id: actor.correlation_id,
        })
    })
}

/// Fetch a ticket by id, mapping "not found" to a clean `MergeError`.
/// Whether a ticket is already a merge source (has a `ticket_merges` row).
/// Replaces the old `ticket.merged_into_ticket_id.is_some()` check now that
/// merge metadata lives in the satellite.
fn is_merge_source(conn: &mut DbConnection, ticket_id: i32) -> Result<bool, MergeError> {
    use crate::schema::ticket_merges;
    use diesel::dsl::{exists, select};
    Ok(select(exists(ticket_merges::table.find(ticket_id))).get_result(conn)?)
}

/// Batched merge lookup for the sync bootstrap: the `ticket_merges` row for
/// each merge-source ticket in `ticket_ids`, keyed by source ticket id. Absent
/// keys are unmerged tickets. Mirrors the other per-ticket membership maps so
/// the bootstrap denormalises merge state without a per-row join.
pub fn merges_for_tickets(
    conn: &mut DbConnection,
    ticket_ids: &[i32],
) -> diesel::QueryResult<std::collections::HashMap<i32, crate::models::TicketMerge>> {
    use crate::schema::ticket_merges;
    if ticket_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let rows: Vec<crate::models::TicketMerge> = ticket_merges::table
        .filter(ticket_merges::ticket_id.eq_any(ticket_ids))
        .load(conn)?;
    Ok(rows.into_iter().map(|m| (m.ticket_id, m)).collect())
}

fn load_ticket(conn: &mut DbConnection, id: i32) -> Result<Ticket, MergeError> {
    use crate::schema::tickets;
    tickets::table
        .find(id)
        .first::<Ticket>(conn)
        .optional()?
        .ok_or(MergeError::NotFound(id))
}

/// Resolve a workflow state's category.
fn state_category(
    conn: &mut DbConnection,
    state_id: i32,
) -> Result<WorkflowStateCategory, MergeError> {
    use crate::schema::workflow_states;
    Ok(workflow_states::table
        .find(state_id)
        .select(workflow_states::category)
        .first(conn)?)
}

/// Each source watcher onto the destination, once, keeping the most
/// permissive internal-notes setting of their rows on the sources and the
/// destination. Returns how many were newly added.
fn move_watchers(
    conn: &mut DbConnection,
    target_id: i32,
    sources: &[i32],
) -> Result<usize, MergeError> {
    use crate::repository::ticket_watchers::{
        add_watcher_with_notify, set_notify_on_internal_notes,
    };
    use crate::schema::ticket_watchers::dsl as w;
    let mut wanted: std::collections::BTreeMap<Uuid, bool> = std::collections::BTreeMap::new();
    let rows: Vec<(Uuid, bool)> = w::ticket_watchers
        .filter(w::ticket_id.eq_any(sources))
        .select((w::user_uuid, w::notify_on_internal_notes))
        .load(conn)?;
    for (user, notify) in rows {
        *wanted.entry(user).or_default() |= notify;
    }
    let current: std::collections::HashMap<Uuid, bool> = w::ticket_watchers
        .filter(w::ticket_id.eq(target_id))
        .select((w::user_uuid, w::notify_on_internal_notes))
        .load::<(Uuid, bool)>(conn)?
        .into_iter()
        .collect();
    let mut added = 0usize;
    for (user, notify) in wanted {
        match current.get(&user) {
            None => {
                if add_watcher_with_notify(conn, target_id, user, true, notify)? {
                    added += 1;
                }
            }
            // Already watching: only a setting the sources widen changes.
            Some(&on_target) => {
                if notify && !on_target {
                    set_notify_on_internal_notes(conn, target_id, &user, true)?;
                }
            }
        }
    }
    Ok(added)
}

/// The sources' projects onto the destination; the sources leave them.
fn move_projects(
    conn: &mut DbConnection,
    target_id: i32,
    sources: &[i32],
) -> Result<(), MergeError> {
    use crate::schema::project_tickets::dsl as p;
    let rows: Vec<(i32, i32)> = p::project_tickets
        .filter(p::ticket_id.eq_any(sources))
        .order((p::ticket_id.asc(), p::project_id.asc()))
        .select((p::ticket_id, p::project_id))
        .load(conn)?;
    for (source, project) in rows {
        crate::repository::projects::add_ticket_to_project(conn, project, target_id)?;
        crate::repository::projects::remove_ticket_from_project(conn, project, source)?;
    }
    Ok(())
}

/// The first source's cycle (by id) onto a destination that has none; every
/// source leaves its cycle. A ticket is in at most one cycle.
fn move_cycle(
    conn: &mut DbConnection,
    target_id: i32,
    sources: &[i32],
    actor: Option<Uuid>,
) -> Result<(), MergeError> {
    use crate::repository::cycles;
    let source_cycles = cycles::cycle_ids_for_tickets(conn, sources)?;
    if cycles::cycle_id_for_ticket(conn, target_id)?.is_none() {
        if let Some(&cycle) = sources.iter().find_map(|s| source_cycles.get(s)) {
            cycles::add_ticket(conn, cycle, target_id, actor)?;
        }
    }
    for source in sources.iter().filter(|s| source_cycles.contains_key(s)) {
        cycles::remove_ticket(conn, *source)?;
    }
    Ok(())
}

/// The sources' linked assets onto the destination; the sources let go of
/// them.
fn move_assets(conn: &mut DbConnection, target_id: i32, sources: &[i32]) -> Result<(), MergeError> {
    use crate::schema::ticket_assets::dsl as a;
    let mut on_target: std::collections::HashSet<i32> = a::ticket_assets
        .filter(a::ticket_id.eq(target_id))
        .select(a::asset_id)
        .load::<i32>(conn)?
        .into_iter()
        .collect();
    let rows: Vec<(i32, i32)> = a::ticket_assets
        .filter(a::ticket_id.eq_any(sources))
        .order((a::ticket_id.asc(), a::asset_id.asc()))
        .select((a::ticket_id, a::asset_id))
        .load(conn)?;
    for (source, asset) in rows {
        if on_target.insert(asset) {
            crate::repository::tickets::add_device_to_ticket(conn, target_id, asset)?;
        }
        crate::repository::tickets::remove_device_from_ticket(conn, source, asset)?;
    }
    Ok(())
}

/// The destination's tags become its own plus every source's.
fn union_tags(
    conn: &mut DbConnection,
    target_id: i32,
    sources: &[i32],
    actor: Option<Uuid>,
) -> Result<(), MergeError> {
    use crate::repository::tags;
    let mut union: std::collections::BTreeSet<i32> = tags::tag_ids_for_ticket(conn, target_id)?
        .into_iter()
        .collect();
    for ids in tags::tag_ids_for_tickets(conn, sources)?.into_values() {
        union.extend(ids);
    }
    let union: Vec<i32> = union.into_iter().collect();
    tags::set_tags_for_ticket(conn, target_id, &union, actor)?;
    Ok(())
}

/// Rewrite each source's links to tickets outside the merge onto the
/// destination, in the same direction and keeping the relation, description
/// and author; then drop every link the sources have. An edge the
/// destination already has, or one that would point at itself, isn't added.
fn move_links(conn: &mut DbConnection, target_id: i32, sources: &[i32]) -> Result<(), MergeError> {
    use crate::repository::linked_tickets::{link_tickets_directional, unlink_tickets};
    use crate::schema::linked_tickets::dsl as l;
    type Edge = (i32, i32, String, Option<String>, Option<Uuid>);
    let edges: Vec<Edge> = l::linked_tickets
        .filter(
            l::ticket_id
                .eq_any(sources)
                .or(l::linked_ticket_id.eq_any(sources)),
        )
        .order((l::ticket_id.asc(), l::linked_ticket_id.asc()))
        .select((
            l::ticket_id,
            l::linked_ticket_id,
            l::relation_type,
            l::description,
            l::created_by,
        ))
        .load(conn)?;
    let in_merge = |id: i32| id == target_id || sources.contains(&id);
    // Each pair once: unlinking takes both directions.
    let mut pairs: std::collections::BTreeSet<(i32, i32)> = std::collections::BTreeSet::new();
    for (from, to, relation, description, created_by) in edges {
        if sources.contains(&from) && !in_merge(to) {
            link_tickets_directional(conn, target_id, to, &relation, description, created_by)?;
        } else if sources.contains(&to) && !in_merge(from) {
            link_tickets_directional(conn, from, target_id, &relation, description, created_by)?;
        }
        pairs.insert((from.min(to), from.max(to)));
    }
    for (a, b) in pairs {
        unlink_tickets(conn, a, b)?;
    }
    Ok(())
}

/// Build the merge-marker comment. The human-readable body is a
/// fallback; the structured `channel_metadata.kind = 'merge_marker'`
/// blob is what the activity-feed card renders.
fn build_marker(
    destination: &Ticket,
    sources: &[Ticket],
    actor: &ActorContext,
    reason: Option<&str>,
    marker_body: Option<&str>,
) -> NewComment {
    let source_json: Vec<_> = sources
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "number": s.number,
                "title": s.title,
                "requester_uuid": s.requester_uuid,
                "opened_at": s.created_at,
            })
        })
        .collect();

    let mut lines = vec![format!("Merged {} ticket(s) into this one:", sources.len())];
    for s in sources {
        lines.push(format!("- #{}: \"{}\"", s.number, s.title));
    }
    if let Some(r) = reason {
        lines.push(format!("Reason: {r}"));
    }
    // Agent-edited body wins; otherwise fall back to the generated
    // summary. Either way the structured metadata below drives the card.
    let generated = lines.join("\n");
    let body_text = marker_body
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or(generated);
    let body_html = format!(
        "<p>{}</p>",
        body_text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('\n', "<br>")
    );

    let metadata = json!({
        "kind": "merge_marker",
        "source_ticket_ids": sources.iter().map(|s| s.id).collect::<Vec<_>>(),
        "source_ticket_numbers": sources.iter().map(|s| s.number).collect::<Vec<_>>(),
        "sources": source_json,
        "merged_into_ticket_id": destination.id,
        "merged_into_ticket_number": destination.number,
        "merged_by_user_uuid": actor.uuid,
        "reason": reason,
    });

    NewComment {
        content: body_text.clone(),
        ticket_id: destination.id,
        // Authored by the merging actor (per the resolved open question).
        // The merge actor always has a uuid; fall back to nil only to
        // keep the type total.
        user_uuid: actor.uuid.unwrap_or(Uuid::nil()),
        channel_metadata: Some(metadata),
        is_internal: false,
        content_format: ContentFormat::Html,
        body_text: Some(body_text),
        body_html: Some(body_html),
        ..Default::default()
    }
}

/// Where this ticket was merged to (populated only when the ticket is
/// itself a merge source).
#[derive(Debug, serde::Serialize)]
pub struct MergedIntoInfo {
    pub destination_id: i32,
    pub merged_at: Option<chrono::NaiveDateTime>,
    pub merged_by: Option<Uuid>,
    pub reason: Option<String>,
}

/// One merge that consumed sources into this ticket, reconstructed from
/// the `ticket.merged` sync event.
#[derive(Debug, serde::Serialize)]
pub struct MergeEvent {
    pub event_id: i64,
    pub merged_at: chrono::DateTime<chrono::Utc>,
    pub merged_by_user_uuid: Option<Uuid>,
    pub merged_by_name: Option<String>,
    pub source_ticket_ids: Vec<i32>,
    pub reason: Option<String>,
    pub comments_moved: i64,
    pub merge_marker_comment_id: Option<i32>,
}

/// Merge history for a ticket, from both directions.
#[derive(Debug, serde::Serialize)]
pub struct MergeHistory {
    pub merged_into: Option<MergedIntoInfo>,
    pub merge_events: Vec<MergeEvent>,
}

// sync-audit-only: read-only history query, emits nothing
/// Build the merge history for `ticket_id`: where it was merged to (if
/// it's a source) and the merges that consumed other tickets into it.
/// Reads through RLS, so it only sees the caller's workspace.
pub fn merge_history_for_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<MergeHistory> {
    use crate::schema::ticket_merges;
    use diesel::sql_types::{BigInt, Integer, Jsonb, Nullable, Text, Timestamptz, Uuid as SqlUuid};

    // Direction 1: this ticket as a merge source (a ticket_merges row).
    let merged_into = ticket_merges::table
        .find(ticket_id)
        .select((
            ticket_merges::merged_into_ticket_id,
            ticket_merges::merged_at,
            ticket_merges::merged_by_user_uuid,
            ticket_merges::merge_reason,
        ))
        .first::<(i32, chrono::NaiveDateTime, Option<Uuid>, Option<String>)>(conn)
        .optional()?
        .map(|(destination_id, at, by, reason)| MergedIntoInfo {
            destination_id,
            merged_at: Some(at),
            merged_by: by,
            reason,
        });

    // Direction 2: merges that consumed sources into this ticket.
    #[derive(diesel::QueryableByName)]
    struct EventRow {
        #[diesel(sql_type = BigInt)]
        sync_id: i64,
        #[diesel(sql_type = Timestamptz)]
        occurred_at: chrono::DateTime<chrono::Utc>,
        #[diesel(sql_type = Jsonb)]
        data: serde_json::Value,
        #[diesel(sql_type = Nullable<SqlUuid>)]
        actor_uuid: Option<Uuid>,
        #[diesel(sql_type = Nullable<Text>)]
        actor_name: Option<String>,
    }

    let rows: Vec<EventRow> = diesel::sql_query(
        "SELECT s.sync_id, s.occurred_at, s.data, s.actor_uuid, u.name AS actor_name \
         FROM sync_actions s \
         LEFT JOIN users u ON u.uuid = s.actor_uuid \
         WHERE s.event_type = 'ticket.merged' AND s.aggregate_id = $1::text \
         ORDER BY s.sync_id DESC LIMIT 50",
    )
    .bind::<Integer, _>(ticket_id)
    .load(conn)?;

    let merge_events = rows
        .into_iter()
        .map(|r| {
            let source_ticket_ids = r.data["source_ticket_ids"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_i64().map(|n| n as i32))
                        .collect()
                })
                .unwrap_or_default();
            MergeEvent {
                event_id: r.sync_id,
                merged_at: r.occurred_at,
                merged_by_user_uuid: r.actor_uuid,
                merged_by_name: r.actor_name,
                source_ticket_ids,
                reason: r.data["reason"].as_str().map(str::to_string),
                comments_moved: r.data["comments_moved"].as_i64().unwrap_or(0),
                merge_marker_comment_id: r.data["merge_marker_comment_id"]
                    .as_i64()
                    .map(|n| n as i32),
            }
        })
        .collect();

    Ok(MergeHistory {
        merged_into,
        merge_events,
    })
}

/// Enqueue a templated "your request was merged" reply to each source
/// ticket's customer, on the source's origin email channel. Best-effort
/// and post-commit; the handler calls this only when the merge dialog's
/// notify-customer box was ticked. Each outbound binds to the
/// destination ticket, so a customer reply threads onto the merged
/// target rather than reopening the source. Sources without an email
/// channel or a requester email are skipped. Returns the number
/// enqueued.
///
/// The notice ends with the workspace's security note when it's on, like a
/// reply. `base_url` is the note's last fallback for the domain it names,
/// used only when no sending address resolves.
pub fn enqueue_merge_notifications(
    conn: &mut DbConnection,
    destination: &Ticket,
    sources: &[Ticket],
    base_url: &str,
) -> QueryResult<usize> {
    use crate::repository::{
        channels as channels_repo, outbound_emails, site_settings as site_settings_repo,
        user_helpers,
    };
    use crate::services::channels::email_imap::ImapChannelConfig;
    use crate::services::channels::threading::{
        format_outbound_message_id, format_outbound_subject,
    };

    let settings = site_settings_repo::get_site_settings(conn)?;
    let locale = crate::utils::locale::effective_locale(None, &settings.default_locale);
    let mut body = crate::utils::i18n::tr(&locale, "merge-notification-customer-template");
    if let Some(note) = crate::utils::email_branding::security_note(
        conn,
        &settings,
        base_url,
        crate::utils::email_branding::SentFrom::Workspace,
    ) {
        body = format!("{body}\n\n{note}");
    }

    let mut enqueued = 0usize;
    for source in sources {
        let (Some(channel_id), Some(requester_uuid)) =
            (source.origin_channel_id, source.requester_uuid)
        else {
            continue;
        };

        // Email is the only channel that delivers a reply today.
        let channel = match channels_repo::find(conn, channel_id) {
            Ok(c) => c,
            Err(_) => continue,
        };
        if channel.provider != "email_imap" {
            continue;
        }
        let config = match serde_json::from_value::<ImapChannelConfig>(channel.config.clone()) {
            Ok(cfg) => cfg,
            Err(_) => continue,
        };
        let Some(recipient) = user_helpers::get_primary_email(&requester_uuid, conn) else {
            continue;
        };

        let message_id =
            format_outbound_message_id(destination.id, source.id, &config.reply_domain);
        let subject = format_outbound_subject(destination.number, &destination.title);
        // B3: customer replies to the merge notice should thread back into the
        // ticket via the channel's polled mailbox (see outbound.rs). Only when
        // the IMAP username is an address.
        let headers_json = if config.username.contains('@') {
            serde_json::json!({ "Reply-To": config.username })
        } else {
            serde_json::json!({})
        };

        outbound_emails::enqueue(
            conn,
            NewOutboundEmail {
                channel_id: Some(channel_id),
                ticket_id: Some(destination.id),
                comment_id: None,
                recipient,
                subject,
                body_text: body.clone(),
                body_html: None,
                message_id,
                in_reply_to: None,
                references_list: Vec::new(),
                headers_json,
                correlation_id: None,
                idempotency_key: None,
                sender_identity: crate::models::outbound_email_sender_identity::WORKSPACE
                    .to_string(),
                // A merge notice to the customer is conversation mail about
                // their own ticket: transactional, not an opt-out-able
                // notification (only internal ticket-activity notifications are).
                mail_class: crate::models::outbound_email_mail_class::TRANSACTIONAL.to_string(),
            },
        )?;
        enqueued += 1;
    }
    Ok(enqueued)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    fn actor_for(user_uuid: Uuid) -> ActorContext {
        ActorContext::user(user_uuid, Some(Uuid::new_v4())).with_workspace(1)
    }

    fn input(dest: i32, sources: Vec<i32>) -> MergeInput {
        MergeInput {
            destination_ticket_id: dest,
            source_ticket_ids: sources,
            reason: Some("same outage".to_string()),
            notify_customer: false,
            expected_state: Vec::new(),
            marker_body: None,
        }
    }

    fn add_watcher(conn: &mut DbConnection, ticket: i32, user: Uuid, notify: bool) {
        use crate::schema::ticket_watchers::dsl as w;
        diesel::insert_into(w::ticket_watchers)
            .values((
                w::ticket_id.eq(ticket),
                w::user_uuid.eq(user),
                w::auto_added.eq(false),
                w::notify_on_internal_notes.eq(notify),
                w::workspace_id.eq(1),
            ))
            .execute(conn)
            .unwrap();
    }

    fn count_sync(conn: &mut DbConnection, event_type: &str, aggregate_id: i32) -> i64 {
        use diesel::sql_types::{BigInt, Integer, Text};
        #[derive(diesel::QueryableByName)]
        struct C {
            #[diesel(sql_type = BigInt)]
            n: i64,
        }
        diesel::sql_query(
            "SELECT COUNT(*) AS n FROM sync_actions WHERE event_type = $1 AND aggregate_id = $2::text",
        )
        .bind::<Text, _>(event_type)
        .bind::<Integer, _>(aggregate_id)
        .get_result::<C>(conn)
        .unwrap()
        .n
    }

    #[test]
    fn happy_path_single_source() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "agent", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        let src = TestFixtures::renumber_ticket(&mut conn, src);
        let moved_comment =
            TestFixtures::create_comment(&mut conn, src.id, user.uuid, "from source");

        let outcome = execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        assert_eq!(outcome.comments_moved, 1);
        assert_eq!(outcome.merged_sources.len(), 1);
        let merged = &outcome.merged_sources[0];
        // Merge metadata now lives in the ticket_merges satellite.
        let merges = merges_for_tickets(&mut conn, &[merged.id]).unwrap();
        let rec = merges.get(&merged.id).expect("merge record");
        assert_eq!(rec.merged_into_ticket_id, dest.id);
        assert_eq!(rec.merged_by_user_uuid, Some(user.uuid));

        // Source sits in the merged category now.
        assert_eq!(
            state_category(&mut conn, merged.workflow_state_id).unwrap(),
            WorkflowStateCategory::Merged
        );

        // Marker comment exists on the destination, flagged structured,
        // naming the source by its number.
        use crate::schema::comments::dsl as c;
        let (content, meta): (String, Option<serde_json::Value>) = c::comments
            .filter(c::id.eq(outcome.merge_marker_comment_id))
            .select((c::content, c::channel_metadata))
            .first(&mut conn)
            .unwrap();
        let meta = meta.unwrap();
        assert_eq!(meta["kind"], "merge_marker");
        assert_eq!(
            meta["source_ticket_numbers"],
            serde_json::json!([src.number])
        );
        assert!(
            content.contains(&format!("- #{}: \"Source\"", src.number)),
            "{content}"
        );

        // duplicate_of edge recorded source -> dest.
        use crate::schema::linked_tickets::dsl as l;
        let rel: String = l::linked_tickets
            .filter(l::ticket_id.eq(src.id))
            .filter(l::linked_ticket_id.eq(dest.id))
            .select(l::relation_type)
            .first(&mut conn)
            .unwrap();
        assert_eq!(rel, "duplicate_of");

        // First-class events emitted.
        assert_eq!(count_sync(&mut conn, "ticket.merged", dest.id), 1);
        assert_eq!(count_sync(&mut conn, "ticket.merged_into", src.id), 1);

        // The moved comment's row names the destination for every client,
        // with the internal flag the sync filter needs.
        assert_eq!(count_sync(&mut conn, "comment.moved", moved_comment.id), 1);
        let data: serde_json::Value = {
            use crate::schema::sync_actions::dsl as s;
            s::sync_actions
                .filter(s::event_type.eq("comment.moved"))
                .filter(s::aggregate_id.eq(moved_comment.id.to_string()))
                .select(s::data)
                .first(&mut conn)
                .unwrap()
        };
        assert_eq!(data["ticket_id"], serde_json::json!(dest.id));
        assert_eq!(data["is_internal"], serde_json::json!(false));
    }

    #[test]
    fn happy_path_three_sources() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "agent3", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let s1 = TestFixtures::create_ticket(&mut conn, "S1", Some(user.uuid), None);
        let s2 = TestFixtures::create_ticket(&mut conn, "S2", Some(user.uuid), None);
        let s3 = TestFixtures::create_ticket(&mut conn, "S3", Some(user.uuid), None);

        let outcome = execute_merge(
            &mut conn,
            input(dest.id, vec![s1.id, s2.id, s3.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        assert_eq!(outcome.merged_sources.len(), 3);
        let ids: Vec<i32> = outcome.merged_sources.iter().map(|t| t.id).collect();
        let merges = merges_for_tickets(&mut conn, &ids).unwrap();
        assert!(ids
            .iter()
            .all(|id| merges.get(id).map(|r| r.merged_into_ticket_id) == Some(dest.id)));
        assert_eq!(count_sync(&mut conn, "ticket.merged_into", s2.id), 1);
    }

    /// The one `sync_actions` row of `event_type` for `aggregate_id`, as the
    /// notification outbox hands it to the deriver.
    fn sync_row(
        conn: &mut DbConnection,
        event_type: &str,
        aggregate_id: i32,
    ) -> crate::services::notifications::deriver::SyncActionRow {
        use crate::schema::sync_actions::dsl as s;
        let (sync_id, workspace_id, event_type, data, actor_uuid, actor_kind, occurred_at) =
            s::sync_actions
                .filter(s::event_type.eq(event_type))
                .filter(s::aggregate_id.eq(aggregate_id.to_string()))
                .select((
                    s::sync_id,
                    s::workspace_id,
                    s::event_type,
                    s::data,
                    s::actor_uuid,
                    s::actor_kind,
                    s::occurred_at,
                ))
                .first::<(
                    i64,
                    i32,
                    String,
                    serde_json::Value,
                    Option<Uuid>,
                    String,
                    chrono::DateTime<chrono::Utc>,
                )>(conn)
                .unwrap();
        crate::services::notifications::deriver::SyncActionRow {
            sync_id,
            workspace_id,
            event_type,
            data,
            actor_uuid,
            actor_kind,
            occurred_at,
        }
    }

    #[test]
    fn merged_into_carries_each_sources_merged_state() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "merge_state", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let s1 = TestFixtures::create_ticket(&mut conn, "S1", Some(user.uuid), None);
        let s2 = TestFixtures::create_ticket(&mut conn, "S2", Some(user.uuid), None);

        let outcome = execute_merge(
            &mut conn,
            input(dest.id, vec![s1.id, s2.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        // Every client moves each source out of its open lists and kanban
        // column from this row alone: the pool shallow-merges it, and the
        // board groups by the nested state's category.
        for merged in &outcome.merged_sources {
            let data = sync_row(&mut conn, "ticket.merged_into", merged.id).data;
            assert_eq!(data["workflow_state"]["category"], "merged", "{data}");
            assert_eq!(data["workflow_state_id"], merged.workflow_state_id);
            assert_eq!(data["merged_into_ticket_id"], dest.id);
            assert_eq!(data["title"], merged.title);
            assert_eq!(data["requester_uuid"], serde_json::json!(user.uuid));
        }
    }

    #[test]
    fn a_merged_source_keeps_no_sla_target() {
        use crate::schema::tickets::dsl as t;
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "merge_sla", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        // Stamped while it was being worked on.
        let later = chrono::Utc::now().naive_utc() + chrono::Duration::hours(2);
        diesel::update(t::tickets.find(src.id))
            .set((
                t::sla_response_target_at.eq(Some(later)),
                t::sla_resolution_target_at.eq(Some(later)),
            ))
            .execute(&mut conn)
            .unwrap();

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        let targets: (Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>) = t::tickets
            .find(src.id)
            .select((t::sla_response_target_at, t::sla_resolution_target_at))
            .first(&mut conn)
            .unwrap();
        assert_eq!(targets, (None, None), "the breach scan has nothing to find");
    }

    #[test]
    fn merge_rows_derive_no_notifications() {
        let mut conn = setup_test_connection();
        let agent = TestFixtures::create_user(&mut conn, "merge_agent", "user");
        let requester = TestFixtures::create_user(&mut conn, "merge_requester", "user");
        let assignee = TestFixtures::create_user(&mut conn, "merge_assignee", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(requester.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(requester.uuid), None);
        {
            use crate::schema::tickets::dsl as t;
            diesel::update(t::tickets.find(src.id))
                .set(t::assignee_uuid.eq(Some(assignee.uuid)))
                .execute(&mut conn)
                .unwrap();
        }

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(agent.uuid),
        )
        .unwrap();

        // The merge notice email is the customer's only word of a merge: the
        // source's assignee isn't newly assigned and the requester's status
        // didn't change by anyone's edit, so the ticket rows derive nothing.
        for row in [
            sync_row(&mut conn, "ticket.merged_into", src.id),
            sync_row(&mut conn, "ticket.merged", dest.id),
        ] {
            let intents = crate::services::notifications::deriver::derive(&row);
            assert!(
                intents.is_empty(),
                "{}: {intents:?} from {}",
                row.event_type,
                row.data
            );
            use crate::schema::notification_outbox::dsl as o;
            let enqueued: i64 = o::notification_outbox
                .filter(o::sync_id.eq(row.sync_id))
                .count()
                .get_result(&mut conn)
                .unwrap();
            assert_eq!(enqueued, 0, "{} enqueued for notifications", row.event_type);
        }
    }

    #[test]
    fn self_merge_rejected() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "self", "user");
        let t = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);

        let err =
            execute_merge(&mut conn, input(t.id, vec![t.id]), &actor_for(user.uuid)).unwrap_err();
        assert!(matches!(err, MergeError::SelfMerge(id) if id == t.id));
    }

    #[test]
    fn already_merged_source_rejected() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "chain", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let other = TestFixtures::create_ticket(&mut conn, "Other", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        // Second merge of the now-merged source must be refused. This is
        // the same outcome a serialised concurrent merge produces: the
        // loser acquires the lock after the winner commits and sees the
        // merged state.
        let err = execute_merge(
            &mut conn,
            input(other.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap_err();
        assert!(matches!(err, MergeError::AlreadyMerged(number) if number == src.number));
    }

    #[test]
    fn missing_source_rejected() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "missing", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);

        // A source id that does not resolve in this workspace. Under RLS
        // a cross-workspace ticket is likewise invisible and lands here.
        let err = execute_merge(
            &mut conn,
            input(dest.id, vec![999_999]),
            &actor_for(user.uuid),
        )
        .unwrap_err();
        assert!(matches!(err, MergeError::NotFound(999_999)));
    }

    #[test]
    fn optimistic_lock_conflict() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "optlock", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);

        let mut req = input(dest.id, vec![src.id]);
        // Stale snapshot: claim the destination was in a state it isn't.
        req.expected_state = vec![ExpectedState {
            ticket_id: dest.id,
            workflow_state_id: dest.workflow_state_id + 9999,
        }];

        let err = execute_merge(&mut conn, req, &actor_for(user.uuid)).unwrap_err();
        match err {
            MergeError::StateConflict(diverged) => {
                assert_eq!(diverged.len(), 1);
                assert_eq!(diverged[0].ticket_id, dest.id);
                assert_eq!(diverged[0].workflow_state_id, dest.workflow_state_id);
            }
            other => panic!("expected StateConflict, got {other:?}"),
        }
    }

    #[test]
    fn watchers_union_ors_notify_flag() {
        let mut conn = setup_test_connection();
        let owner = TestFixtures::create_user(&mut conn, "owner", "user");
        let shared = TestFixtures::create_user(&mut conn, "shared", "user");
        let only_src = TestFixtures::create_user(&mut conn, "onlysrc", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(owner.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(owner.uuid), None);

        // Shared watcher: notify=false on dest, notify=true on source.
        add_watcher(&mut conn, dest.id, shared.uuid, false);
        add_watcher(&mut conn, src.id, shared.uuid, true);
        // Source-only watcher gets added to dest.
        add_watcher(&mut conn, src.id, only_src.uuid, false);

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(owner.uuid),
        )
        .unwrap();

        use crate::schema::ticket_watchers::dsl as w;
        let shared_notify: bool = w::ticket_watchers
            .filter(w::ticket_id.eq(dest.id))
            .filter(w::user_uuid.eq(shared.uuid))
            .select(w::notify_on_internal_notes)
            .first(&mut conn)
            .unwrap();
        assert!(shared_notify, "OR of false|true must be true");

        let only_src_on_dest: Vec<bool> = w::ticket_watchers
            .filter(w::ticket_id.eq(dest.id))
            .filter(w::user_uuid.eq(only_src.uuid))
            .select(w::notify_on_internal_notes)
            .load(&mut conn)
            .unwrap();
        assert_eq!(
            only_src_on_dest,
            vec![false],
            "added with their setting, off"
        );
    }

    #[test]
    fn a_watcher_of_several_sources_is_added_once() {
        let mut conn = setup_test_connection();
        let owner = TestFixtures::create_user(&mut conn, "owner2", "user");
        let shared = TestFixtures::create_user(&mut conn, "shared2", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(owner.uuid), None);
        let s1 = TestFixtures::create_ticket(&mut conn, "S1", Some(owner.uuid), None);
        let s2 = TestFixtures::create_ticket(&mut conn, "S2", Some(owner.uuid), None);
        add_watcher(&mut conn, s1.id, shared.uuid, false);
        add_watcher(&mut conn, s2.id, shared.uuid, true);

        execute_merge(
            &mut conn,
            input(dest.id, vec![s1.id, s2.id]),
            &actor_for(owner.uuid),
        )
        .unwrap();

        use crate::schema::ticket_watchers::dsl as w;
        let on_dest: Vec<bool> = w::ticket_watchers
            .filter(w::ticket_id.eq(dest.id))
            .filter(w::user_uuid.eq(shared.uuid))
            .select(w::notify_on_internal_notes)
            .load(&mut conn)
            .unwrap();
        assert_eq!(on_dest, vec![true], "one row, OR of false|true");
    }

    #[test]
    fn comments_move_with_attachment() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "att", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        let comment = TestFixtures::create_comment(&mut conn, src.id, user.uuid, "has file");
        let att = TestFixtures::create_attachment(&mut conn, comment.id, "f.pdf");

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        use crate::schema::comments::dsl as c;
        let moved_ticket: i32 = c::comments
            .filter(c::id.eq(comment.id))
            .select(c::ticket_id)
            .first(&mut conn)
            .unwrap();
        assert_eq!(moved_ticket, dest.id);

        // Attachment rides along via comment_id (unchanged).
        use crate::schema::attachments::dsl as a;
        let still_linked: i32 = a::attachments
            .filter(a::id.eq(att.id))
            .select(a::comment_id)
            .first::<Option<i32>>(&mut conn)
            .unwrap()
            .unwrap();
        assert_eq!(still_linked, comment.id);
    }

    #[test]
    fn a_moved_replys_files_reach_the_destinations_audience() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "merge_files", "user");
        let project = TestFixtures::create_project(&mut conn, "Files");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        // Only the destination is on the project board, so its audience is
        // wider than the source's.
        {
            use crate::schema::project_tickets as p;
            diesel::insert_into(p::table)
                .values((p::project_id.eq(project.id), p::ticket_id.eq(dest.id)))
                .execute(&mut conn)
                .unwrap();
        }
        let reply = TestFixtures::create_comment(&mut conn, src.id, user.uuid, "has a file");
        let file = TestFixtures::create_attachment(&mut conn, reply.id, "f.pdf");
        let removed = TestFixtures::create_comment(&mut conn, src.id, user.uuid, "removed");
        let removed_file = TestFixtures::create_attachment(&mut conn, removed.id, "g.pdf");
        {
            use crate::schema::comments::dsl as c;
            diesel::update(c::comments.find(removed.id))
                .set(c::deleted_at.eq(Some(chrono::Utc::now().naive_utc())))
                .execute(&mut conn)
                .unwrap();
        }

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        let groups: Vec<Option<String>> = {
            use crate::schema::sync_actions::dsl as s;
            s::sync_actions
                .filter(s::event_type.eq("attachment.moved"))
                .filter(s::aggregate_id.eq(file.id.to_string()))
                .select(s::groups)
                .first(&mut conn)
                .unwrap()
        };
        assert!(
            groups.contains(&Some(format!("ticket:{}", dest.id))),
            "{groups:?}"
        );
        assert!(
            groups.contains(&Some(format!("project:{}", project.id))),
            "{groups:?}"
        );
        assert_eq!(count_sync(&mut conn, "attachment.moved", file.id), 1);
        // A removed reply stays out of the pool, and so do its files.
        assert_eq!(
            count_sync(&mut conn, "attachment.moved", removed_file.id),
            0
        );
        // Moving a file isn't adding one: nothing that raises AttachmentAdded.
        assert_eq!(count_sync(&mut conn, "attachment.attached", file.id), 0);
        assert_eq!(count_sync(&mut conn, "attachment.created", file.id), 0);
    }

    #[test]
    fn a_merge_moves_replies_that_share_a_client_id() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "merge_client_id", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        // One author sent a reply with the same client id to both tickets
        // (the API takes any id).
        let shared = Some(Uuid::new_v4());
        for ticket_id in [dest.id, src.id] {
            crate::repository::comments::create_comment(
                &mut conn,
                NewComment {
                    content: "<p>same id</p>".into(),
                    ticket_id,
                    user_uuid: user.uuid,
                    client_id: shared,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
        }

        let outcome = execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        assert_eq!(outcome.comments_moved, 1);
    }

    #[test]
    fn channel_messages_rerouted() {
        use crate::models::NewChannelMessage;
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "chan", "user");
        let channel = TestFixtures::create_channel(&mut conn, "email");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);

        use crate::schema::channel_messages::dsl as cm;
        let msg_id: i64 = diesel::insert_into(cm::channel_messages)
            .values(&NewChannelMessage {
                channel_id: channel.id,
                external_id: "ext-1".to_string(),
                direction: "inbound".to_string(),
                ticket_id: Some(src.id),
                comment_id: None,
                in_reply_to: None,
                from_address: Some("c@example.com".to_string()),
                author_user_uuid: None,
                raw_metadata: None,
            })
            .returning(cm::id)
            .get_result(&mut conn)
            .unwrap();

        let outcome = execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();
        assert_eq!(outcome.channel_messages_rerouted, 1);

        let now_on: Option<i32> = cm::channel_messages
            .filter(cm::id.eq(msg_id))
            .select(cm::ticket_id)
            .first(&mut conn)
            .unwrap();
        assert_eq!(now_on, Some(dest.id));
    }

    #[test]
    fn project_membership_moves_to_target() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "proj", "user");
        let project = TestFixtures::create_project(&mut conn, "Proj");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);

        use crate::schema::project_tickets::dsl as p;
        diesel::insert_into(p::project_tickets)
            .values((
                p::project_id.eq(project.id),
                p::ticket_id.eq(src.id),
                p::workspace_id.eq(1),
            ))
            .execute(&mut conn)
            .unwrap();

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        let on_dest: i64 = p::project_tickets
            .filter(p::project_id.eq(project.id))
            .filter(p::ticket_id.eq(dest.id))
            .count()
            .get_result(&mut conn)
            .unwrap();
        let on_src: i64 = p::project_tickets
            .filter(p::ticket_id.eq(src.id))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(on_dest, 1, "moved onto destination");
        assert_eq!(on_src, 0, "removed from closed source");
    }

    /// Sync rows of `event_type` for a junction's composite aggregate id.
    fn count_sync_key(conn: &mut DbConnection, event_type: &str, aggregate_id: &str) -> i64 {
        use crate::schema::sync_actions::dsl as s;
        s::sync_actions
            .filter(s::event_type.eq(event_type))
            .filter(s::aggregate_id.eq(aggregate_id))
            .count()
            .get_result(conn)
            .unwrap()
    }

    fn make_asset(conn: &mut DbConnection, name: &str) -> i32 {
        use crate::schema::assets::dsl as a;
        diesel::insert_into(a::assets)
            .values((
                a::name.eq(name),
                a::kind.eq("generic"),
                a::attributes.eq(serde_json::json!({})),
            ))
            .returning(a::id)
            .get_result(conn)
            .unwrap()
    }

    fn make_cycle(conn: &mut DbConnection, project_id: i32, name: &str) -> i32 {
        crate::repository::cycles::create(
            conn,
            crate::models::NewCycle {
                project_id,
                name: name.to_string(),
                start_at: None,
                end_at: None,
                state: "planned".to_string(),
                created_by: None,
            },
        )
        .unwrap()
        .id
    }

    #[test]
    fn a_merge_moves_projects_assets_and_cycles_through_their_writers() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "merge_boards", "user");
        let project = TestFixtures::create_project(&mut conn, "Boards");
        let cycle = make_cycle(&mut conn, project.id, "Sprint");
        let asset = make_asset(&mut conn, "Laptop");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        // The source's memberships, written directly so setup emits nothing.
        {
            use crate::schema::{cycle_tickets as c, project_tickets as p, ticket_assets as a};
            diesel::insert_into(p::table)
                .values((p::project_id.eq(project.id), p::ticket_id.eq(src.id)))
                .execute(&mut conn)
                .unwrap();
            diesel::insert_into(c::table)
                .values((c::cycle_id.eq(cycle), c::ticket_id.eq(src.id)))
                .execute(&mut conn)
                .unwrap();
            diesel::insert_into(a::table)
                .values((a::ticket_id.eq(src.id), a::asset_id.eq(asset)))
                .execute(&mut conn)
                .unwrap();
        }

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        let (d, s) = (dest.id, src.id);
        let (p, c, a) = (project.id, cycle, asset);
        assert_eq!(
            count_sync_key(&mut conn, "project_ticket.added", &format!("{p}:{d}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "project_ticket.removed", &format!("{p}:{s}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "cycle_ticket.added", &format!("{c}:{d}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "cycle_ticket.removed", &format!("{c}:{s}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "ticket_asset.added", &format!("{d}:{a}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "ticket_asset.removed", &format!("{s}:{a}")),
            1
        );
    }

    #[test]
    fn a_merge_keeps_the_destinations_cycle() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "merge_cycle", "user");
        let project = TestFixtures::create_project(&mut conn, "Cycles");
        let kept = make_cycle(&mut conn, project.id, "Kept");
        let other = make_cycle(&mut conn, project.id, "Other");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        {
            use crate::schema::cycle_tickets as c;
            diesel::insert_into(c::table)
                .values(&vec![
                    (c::cycle_id.eq(kept), c::ticket_id.eq(dest.id)),
                    (c::cycle_id.eq(other), c::ticket_id.eq(src.id)),
                ])
                .execute(&mut conn)
                .unwrap();
        }

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        assert_eq!(
            crate::repository::cycles::cycle_id_for_ticket(&mut conn, dest.id).unwrap(),
            Some(kept)
        );
        assert_eq!(
            crate::repository::cycles::cycle_id_for_ticket(&mut conn, src.id).unwrap(),
            None
        );
        assert_eq!(
            count_sync_key(
                &mut conn,
                "cycle_ticket.removed",
                &format!("{other}:{}", src.id)
            ),
            1
        );
    }

    #[test]
    fn a_merge_moves_tags_and_watchers_through_their_writers() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "merge_tags", "user");
        let watcher = TestFixtures::create_user(&mut conn, "merge_watcher", "user");
        let newcomer = TestFixtures::create_user(&mut conn, "merge_newcomer", "user");
        let tag = crate::repository::tags::create_tag(
            &mut conn,
            crate::models::NewTag {
                name: format!("merge-tag-{}", Uuid::new_v4().simple()),
                color: None,
                description: None,
            },
        )
        .unwrap();
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        {
            use crate::schema::ticket_tags as t;
            diesel::insert_into(t::table)
                .values((t::ticket_id.eq(src.id), t::tag_id.eq(tag.id)))
                .execute(&mut conn)
                .unwrap();
        }
        // Watching both: off on the destination, on on the source.
        add_watcher(&mut conn, dest.id, watcher.uuid, false);
        add_watcher(&mut conn, src.id, watcher.uuid, true);
        // Watching only the source, with internal notes off.
        add_watcher(&mut conn, src.id, newcomer.uuid, false);

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        assert_eq!(count_sync(&mut conn, "ticket.tags_changed", dest.id), 1);
        // The newcomer joins with their setting; the shared watcher's widens.
        assert_eq!(count_sync(&mut conn, "ticket.watcher_added", dest.id), 1);
        assert_eq!(
            count_sync(&mut conn, "ticket.watcher_pref_changed", dest.id),
            1
        );
        use crate::schema::ticket_watchers::dsl as w;
        let mut on_dest: Vec<(Uuid, bool)> = w::ticket_watchers
            .filter(w::ticket_id.eq(dest.id))
            .select((w::user_uuid, w::notify_on_internal_notes))
            .load(&mut conn)
            .unwrap();
        on_dest.sort();
        let mut expected = vec![(watcher.uuid, true), (newcomer.uuid, false)];
        expected.sort();
        assert_eq!(on_dest, expected);
    }

    #[test]
    fn a_merge_moves_links_through_their_writers() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "merge_links", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        let other = TestFixtures::create_ticket(&mut conn, "Other", Some(user.uuid), None);
        // An earlier merge's source: its one-way edge points at this source.
        let earlier = TestFixtures::create_ticket(&mut conn, "Earlier", Some(user.uuid), None);
        {
            use crate::schema::linked_tickets as l;
            diesel::insert_into(l::table)
                .values(&vec![
                    (
                        l::ticket_id.eq(src.id),
                        l::linked_ticket_id.eq(other.id),
                        l::relation_type.eq("related"),
                    ),
                    (
                        l::ticket_id.eq(other.id),
                        l::linked_ticket_id.eq(src.id),
                        l::relation_type.eq("related"),
                    ),
                    (
                        l::ticket_id.eq(earlier.id),
                        l::linked_ticket_id.eq(src.id),
                        l::relation_type.eq("duplicate_of"),
                    ),
                ])
                .execute(&mut conn)
                .unwrap();
        }

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        let (d, s, o) = (dest.id, src.id, other.id);
        assert_eq!(
            count_sync_key(&mut conn, "linked_ticket.added", &format!("{d}:{o}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "linked_ticket.added", &format!("{o}:{d}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "linked_ticket.removed", &format!("{s}:{o}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "linked_ticket.removed", &format!("{o}:{s}")),
            1
        );
        // The merge's own edge reaches clients too.
        assert_eq!(
            count_sync_key(&mut conn, "linked_ticket.added", &format!("{s}:{d}")),
            1
        );
        // The one-way edge moves one way, and only it is reported removed.
        let e = earlier.id;
        assert_eq!(
            count_sync_key(&mut conn, "linked_ticket.added", &format!("{e}:{d}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "linked_ticket.removed", &format!("{e}:{s}")),
            1
        );
        assert_eq!(
            count_sync_key(&mut conn, "linked_ticket.removed", &format!("{s}:{e}")),
            0
        );
    }

    #[test]
    fn linked_tickets_rewritten_and_self_link_dropped() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "links", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        let other = TestFixtures::create_ticket(&mut conn, "Other", Some(user.uuid), None);

        // src <-> other (rewrites onto dest) and src <-> dest (would
        // self-link, must be dropped).
        crate::repository::linked_tickets::link_tickets(&mut conn, src.id, other.id).unwrap();
        crate::repository::linked_tickets::link_tickets(&mut conn, src.id, dest.id).unwrap();

        execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();

        use crate::schema::linked_tickets::dsl as l;
        // No edge references the source any more.
        let src_edges: i64 = l::linked_tickets
            .filter(l::ticket_id.eq(src.id).or(l::linked_ticket_id.eq(src.id)))
            .count()
            .get_result(&mut conn)
            .unwrap();
        // Only the duplicate_of merge edge survives src -> dest.
        assert_eq!(src_edges, 1);

        // dest <-> other now exists (rewritten from src), no self-link.
        let dest_other: i64 = l::linked_tickets
            .filter(l::ticket_id.eq(dest.id))
            .filter(l::linked_ticket_id.eq(other.id))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(dest_other, 1);
    }

    #[test]
    fn audit_and_sync_share_correlation_id() {
        use diesel::sql_types::{BigInt, Text};
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "corr", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);

        let outcome = execute_merge(
            &mut conn,
            input(dest.id, vec![src.id]),
            &actor_for(user.uuid),
        )
        .unwrap();
        let corr = outcome
            .correlation_id
            .expect("actor carried a correlation id");

        #[derive(diesel::QueryableByName)]
        struct C {
            #[diesel(sql_type = BigInt)]
            n: i64,
        }
        let sync_rows: i64 = diesel::sql_query(
            "SELECT COUNT(*) AS n FROM sync_actions WHERE correlation_id = $1::uuid",
        )
        .bind::<Text, _>(corr.to_string())
        .get_result::<C>(&mut conn)
        .unwrap()
        .n;
        let audit_rows: i64 = diesel::sql_query(
            "SELECT COUNT(*) AS n FROM audit_log WHERE correlation_id = $1::uuid",
        )
        .bind::<Text, _>(corr.to_string())
        .get_result::<C>(&mut conn)
        .unwrap()
        .n;

        assert!(sync_rows > 0, "sync_actions rows share the correlation id");
        assert!(audit_rows > 0, "audit_log rows share the correlation id");
    }

    #[test]
    fn merge_notifications_enqueue_for_email_sources() {
        use crate::schema::{channels, outbound_emails, tickets};
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "customer", "user");
        TestFixtures::create_user_email(&mut conn, user.uuid, "customer@example.com", true);

        let channel: crate::models::Channel = diesel::insert_into(channels::table)
            .values(&crate::models::NewChannel {
                provider: "email_imap".to_string(),
                name: "mail".to_string(),
                enabled: true,
                config: serde_json::json!({
                    "host": "mail.example.com",
                    "username": "support@example.com",
                    "reply_domain": "example.com",
                }),
            })
            .get_result(&mut conn)
            .unwrap();

        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        diesel::update(tickets::table.find(src.id))
            .set(tickets::origin_channel_id.eq(channel.id))
            .execute(&mut conn)
            .unwrap();
        let src: Ticket = tickets::table.find(src.id).first(&mut conn).unwrap();

        let n = enqueue_merge_notifications(&mut conn, &dest, &[src], "").unwrap();
        assert_eq!(n, 1);

        let queued: i64 = outbound_emails::table
            .filter(outbound_emails::ticket_id.eq(dest.id))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(queued, 1);
    }

    #[test]
    fn merge_notice_ends_with_the_security_note() {
        use crate::schema::{channels, outbound_emails, tickets};
        let mut conn = setup_test_connection();
        crate::repository::site_settings::update_site_settings(
            &mut conn,
            crate::models::UpdateSiteSettings {
                app_name: Some("Acme IT".into()),
                email_security_note_enabled: Some(true),
                email_security_note_template: Some(Some(
                    "{{brand_name}} only emails you from {{domain}}.".into(),
                )),
                ..Default::default()
            },
        )
        .unwrap();
        let user = TestFixtures::create_user(&mut conn, "noted", "user");
        TestFixtures::create_user_email(&mut conn, user.uuid, "noted@example.com", true);
        let channel: crate::models::Channel = diesel::insert_into(channels::table)
            .values(&crate::models::NewChannel {
                provider: "email_imap".to_string(),
                name: "mail".to_string(),
                enabled: true,
                config: serde_json::json!({
                    "host": "mail.example.com",
                    "username": "support@example.com",
                    "reply_domain": "example.com",
                }),
            })
            .get_result(&mut conn)
            .unwrap();
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);
        let src: Ticket = diesel::update(tickets::table.find(src.id))
            .set(tickets::origin_channel_id.eq(channel.id))
            .get_result(&mut conn)
            .unwrap();

        let n = enqueue_merge_notifications(&mut conn, &dest, &[src], "https://desk.example.com")
            .unwrap();
        assert_eq!(n, 1);

        let body: String = outbound_emails::table
            .filter(outbound_emails::ticket_id.eq(dest.id))
            .select(outbound_emails::body_text)
            .first(&mut conn)
            .unwrap();
        // No sending identity resolves in a test, so the note names the
        // base URL's host, the last fallback.
        let settings = crate::repository::site_settings::get_site_settings(&mut conn).unwrap();
        let note = crate::utils::email_branding::security_note(
            &mut conn,
            &settings,
            "https://desk.example.com",
            crate::utils::email_branding::SentFrom::Workspace,
        )
        .expect("the note is on");
        assert!(note.starts_with("Acme IT only emails you from "), "{note}");
        assert!(body.ends_with(&format!("\n\n{note}")), "{body}");
    }

    #[test]
    fn merge_notifications_skip_sources_without_channel() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "nochan", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);

        let n = enqueue_merge_notifications(&mut conn, &dest, &[src], "").unwrap();
        assert_eq!(n, 0);
    }
}
