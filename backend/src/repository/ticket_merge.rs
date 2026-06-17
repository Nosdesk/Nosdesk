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
//! Lifecycle order mirrors `docs/ticket-merge-plan.md` section 5. The
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
    /// A source or the destination is already a merge source.
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
            MergeError::AlreadyMerged(id) => write!(f, "ticket {id} is already merged"),
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

// sync-pending-wire: emits ticket.merged / ticket.merged_into via sync::emit::record inside the txn
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
    use crate::schema::{tickets, workflow_states};

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
        if destination.merged_into_ticket_id.is_some() {
            return Err(MergeError::AlreadyMerged(target_id));
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
            if s.merged_into_ticket_id.is_some() {
                return Err(MergeError::AlreadyMerged(sid));
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

        // Step 4: mark sources merged and move them to the merged state.
        diesel::update(tickets::table.filter(tickets::id.eq_any(&source_array)))
            .set((
                tickets::merged_into_ticket_id.eq(target_id),
                tickets::merged_at.eq(diesel::dsl::now),
                tickets::merged_by_user_uuid.eq(actor.uuid),
                tickets::merge_reason.eq(reason.clone()),
                tickets::workflow_state_id.eq(merged_state_id),
                tickets::updated_at.eq(diesel::dsl::now),
            ))
            .execute(conn)?;

        // Step 5: move comments (attachments ride along via comment_id).
        let comments_moved =
            diesel::sql_query("UPDATE comments SET ticket_id = $1 WHERE ticket_id = ANY($2)")
                .bind::<Integer, _>(target_id)
                .bind::<Array<Integer>, _>(&source_array)
                .execute(conn)?;

        // Step 6: reroute channel messages so future inbound replies
        // thread onto the destination.
        let channel_messages_rerouted = diesel::sql_query(
            "UPDATE channel_messages SET ticket_id = $1 WHERE ticket_id = ANY($2)",
        )
        .bind::<Integer, _>(target_id)
        .bind::<Array<Integer>, _>(&source_array)
        .execute(conn)?;

        // Step 7: union watchers onto the destination. Source rows stay
        // put (the source is still a real record). notify_on_internal_
        // notes ORs so the destination keeps the most permissive flag.
        let watchers_added_to_destination = diesel::sql_query(
            "INSERT INTO ticket_watchers \
                 (ticket_id, user_uuid, auto_added, notify_on_internal_notes, workspace_id) \
             SELECT $1, sw.user_uuid, TRUE, sw.notify_on_internal_notes, sw.workspace_id \
             FROM ticket_watchers sw \
             WHERE sw.ticket_id = ANY($2) \
             ON CONFLICT (ticket_id, user_uuid) DO UPDATE SET \
                 notify_on_internal_notes = \
                     ticket_watchers.notify_on_internal_notes OR EXCLUDED.notify_on_internal_notes",
        )
        .bind::<Integer, _>(target_id)
        .bind::<Array<Integer>, _>(&source_array)
        .execute(conn)?;

        // Project / cycle / asset memberships union onto the
        // destination, then drop from the sources (closed records
        // shouldn't show on boards). Tags and doc links accumulate on
        // the destination; leaving them on the source is harmless.
        union_then_clear(
            conn,
            "project_tickets",
            "project_id",
            target_id,
            &source_array,
            true,
        )?;
        union_then_clear(
            conn,
            "cycle_tickets",
            "cycle_id",
            target_id,
            &source_array,
            true,
        )?;
        union_then_clear(
            conn,
            "ticket_assets",
            "asset_id",
            target_id,
            &source_array,
            true,
        )?;
        union_then_clear(
            conn,
            "ticket_tags",
            "tag_id",
            target_id,
            &source_array,
            false,
        )?;
        union_doc_links(conn, target_id, &source_array)?;

        // Step 8: rewrite the sources' OTHER ticket links onto the
        // destination (both directions), then drop every source link.
        // INSERT ... SELECT ON CONFLICT DO NOTHING sidesteps PK
        // collisions when the destination already shares that edge, and
        // the WHERE clauses exclude edges that would self-link.
        diesel::sql_query(
            "INSERT INTO linked_tickets \
                 (ticket_id, linked_ticket_id, relation_type, description, created_by, workspace_id) \
             SELECT $1, lt.linked_ticket_id, lt.relation_type, lt.description, lt.created_by, lt.workspace_id \
             FROM linked_tickets lt \
             WHERE lt.ticket_id = ANY($2) \
               AND lt.linked_ticket_id <> $1 \
               AND lt.linked_ticket_id <> ALL($2) \
             ON CONFLICT DO NOTHING",
        )
        .bind::<Integer, _>(target_id)
        .bind::<Array<Integer>, _>(&source_array)
        .execute(conn)?;
        diesel::sql_query(
            "INSERT INTO linked_tickets \
                 (ticket_id, linked_ticket_id, relation_type, description, created_by, workspace_id) \
             SELECT lt.ticket_id, $1, lt.relation_type, lt.description, lt.created_by, lt.workspace_id \
             FROM linked_tickets lt \
             WHERE lt.linked_ticket_id = ANY($2) \
               AND lt.ticket_id <> $1 \
               AND lt.ticket_id <> ALL($2) \
             ON CONFLICT DO NOTHING",
        )
        .bind::<Integer, _>(target_id)
        .bind::<Array<Integer>, _>(&source_array)
        .execute(conn)?;
        diesel::sql_query(
            "DELETE FROM linked_tickets WHERE ticket_id = ANY($1) OR linked_ticket_id = ANY($1)",
        )
        .bind::<Array<Integer>, _>(&source_array)
        .execute(conn)?;

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

        for source in &sources {
            let source_groups = groups::for_ticket(conn, source)?;
            // Re-read post-update so the emit carries the persisted merge
            // fields and an `id` (so the pool applies it as an op-U on the
            // source ticket row rather than skipping a pk-less side event).
            // Drives the merged-into banner + read-only composer
            // pool-native (Phase 2).
            let merged = load_ticket(conn, source.id)?;
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::Ticket,
                    aggregate_id: source.id.to_string(),
                    op: SyncOp::Update,
                    event_type: "ticket.merged_into",
                    data: json!({
                        "id": source.id,
                        "merged_into_ticket_id": merged.merged_into_ticket_id,
                        "merged_at": merged.merged_at,
                        "merged_by_user_uuid": merged.merged_by_user_uuid,
                        "actor_uuid": actor.uuid,
                    }),
                    groups: source_groups,
                    causation_id: None,
                },
            )?;
        }

        // Re-read the now-merged rows for the response DTO.
        let destination = load_ticket(conn, target_id)?;
        let mut merged_sources = Vec::with_capacity(source_ids.len());
        for &sid in &source_ids {
            merged_sources.push(load_ticket(conn, sid)?);
        }

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

/// Union a two-column junction table (`<other>_id`, `ticket_id`) onto
/// the destination via raw SQL, then optionally delete the source rows.
/// `other_col` is the non-ticket key column. `clear_sources` drops the
/// sources' rows after copying (project / cycle / asset boards
/// shouldn't list closed records); false leaves them (tags accumulate).
fn union_then_clear(
    conn: &mut DbConnection,
    table: &str,
    other_col: &str,
    target_id: i32,
    sources: &[i32],
    clear_sources: bool,
) -> Result<(), MergeError> {
    let insert = format!(
        "INSERT INTO {table} ({other_col}, ticket_id, workspace_id) \
         SELECT j.{other_col}, $1, j.workspace_id FROM {table} j \
         WHERE j.ticket_id = ANY($2) ON CONFLICT DO NOTHING"
    );
    diesel::sql_query(insert)
        .bind::<Integer, _>(target_id)
        .bind::<Array<Integer>, _>(sources)
        .execute(conn)?;

    if clear_sources {
        let delete = format!("DELETE FROM {table} WHERE ticket_id = ANY($1)");
        diesel::sql_query(delete)
            .bind::<Array<Integer>, _>(sources)
            .execute(conn)?;
    }
    Ok(())
}

/// Union documentation_page_tickets onto the destination, preserving
/// each row's link_type.
fn union_doc_links(
    conn: &mut DbConnection,
    target_id: i32,
    sources: &[i32],
) -> Result<(), MergeError> {
    diesel::sql_query(
        "INSERT INTO documentation_page_tickets \
             (page_id, ticket_id, link_type, created_by, workspace_id) \
         SELECT d.page_id, $1, d.link_type, d.created_by, d.workspace_id \
         FROM documentation_page_tickets d \
         WHERE d.ticket_id = ANY($2) ON CONFLICT DO NOTHING",
    )
    .bind::<Integer, _>(target_id)
    .bind::<Array<Integer>, _>(sources)
    .execute(conn)?;
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
                "title": s.title,
                "requester_uuid": s.requester_uuid,
                "opened_at": s.created_at,
            })
        })
        .collect();

    let mut lines = vec![format!("Merged {} ticket(s) into this one:", sources.len())];
    for s in sources {
        lines.push(format!("- #{}: \"{}\"", s.id, s.title));
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
        "sources": source_json,
        "merged_into_ticket_id": destination.id,
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
    use crate::schema::tickets;
    use diesel::sql_types::{BigInt, Integer, Jsonb, Nullable, Text, Timestamptz, Uuid as SqlUuid};

    // Direction 1: this ticket as a source.
    let row: Option<(
        Option<i32>,
        Option<chrono::NaiveDateTime>,
        Option<Uuid>,
        Option<String>,
    )> = tickets::table
        .find(ticket_id)
        .select((
            tickets::merged_into_ticket_id,
            tickets::merged_at,
            tickets::merged_by_user_uuid,
            tickets::merge_reason,
        ))
        .first(conn)
        .optional()?;

    let merged_into = row.and_then(|(into, at, by, reason)| {
        into.map(|destination_id| MergedIntoInfo {
            destination_id,
            merged_at: at,
            merged_by: by,
            reason,
        })
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
pub fn enqueue_merge_notifications(
    conn: &mut DbConnection,
    destination: &Ticket,
    sources: &[Ticket],
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
    let body = crate::utils::i18n::tr(&locale, "merge-notification-customer-template");

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
        let reply_domain = match serde_json::from_value::<ImapChannelConfig>(channel.config.clone())
        {
            Ok(cfg) => cfg.reply_domain,
            Err(_) => continue,
        };
        let Some(recipient) = user_helpers::get_primary_email(&requester_uuid, conn) else {
            continue;
        };

        let message_id = format_outbound_message_id(destination.id, source.id, &reply_domain);
        let subject = format_outbound_subject(destination.id, &destination.title);

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
                headers_json: serde_json::json!({}),
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
        assert_eq!(merged.merged_into_ticket_id, Some(dest.id));
        assert!(merged.merged_at.is_some());
        assert_eq!(merged.merged_by_user_uuid, Some(user.uuid));

        // Source sits in the merged category now.
        assert_eq!(
            state_category(&mut conn, merged.workflow_state_id).unwrap(),
            WorkflowStateCategory::Merged
        );

        // Marker comment exists on the destination, flagged structured.
        use crate::schema::comments::dsl as c;
        let meta: Option<serde_json::Value> = c::comments
            .filter(c::id.eq(outcome.merge_marker_comment_id))
            .select(c::channel_metadata)
            .first(&mut conn)
            .unwrap();
        assert_eq!(meta.unwrap()["kind"], "merge_marker");

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
        assert!(outcome
            .merged_sources
            .iter()
            .all(|t| t.merged_into_ticket_id == Some(dest.id)));
        assert_eq!(count_sync(&mut conn, "ticket.merged_into", s2.id), 1);
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
        assert!(matches!(err, MergeError::AlreadyMerged(id) if id == src.id));
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

        let only_src_on_dest: i64 = w::ticket_watchers
            .filter(w::ticket_id.eq(dest.id))
            .filter(w::user_uuid.eq(only_src.uuid))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(only_src_on_dest, 1);
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

        let n = enqueue_merge_notifications(&mut conn, &dest, &[src]).unwrap();
        assert_eq!(n, 1);

        let queued: i64 = outbound_emails::table
            .filter(outbound_emails::ticket_id.eq(dest.id))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(queued, 1);
    }

    #[test]
    fn merge_notifications_skip_sources_without_channel() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "nochan", "user");
        let dest = TestFixtures::create_ticket(&mut conn, "Dest", Some(user.uuid), None);
        let src = TestFixtures::create_ticket(&mut conn, "Source", Some(user.uuid), None);

        let n = enqueue_merge_notifications(&mut conn, &dest, &[src]).unwrap();
        assert_eq!(n, 0);
    }
}
