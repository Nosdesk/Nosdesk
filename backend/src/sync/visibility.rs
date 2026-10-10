//! Per-viewer visibility for the sync read paths.
//!
//! The sync engine delivers `sync_actions` to clients by `groups` overlap;
//! this module decides, row by row, which of those a viewer receives and how
//! much of each. Every kind of record has an audience
//! ([`crate::sync::audience`]): everyone in the workspace, staff, admins,
//! whoever can see its ticket, whoever a documentation page is open to, or
//! the one person it is addressed to. Within those, a `user` row reaches a
//! workspace's sessions only when it is about one of its people
//! (`repository::directory`) and carries only the fields the viewer may see
//! ([`crate::sync::user_projection`]), and a knowledge gap is left out for a
//! viewer who can't open a page it names. It is the single place that
//! decides "can THIS viewer see THIS row", shared by all three read paths:
//!
//! - **bootstrap** (snapshot): filters at the query level via
//!   [`bootstrap_ticket_query`], and projects its `user` rows with
//!   [`project_row`].
//! - **delta** (pull) and the live **SSE** `SyncActions` stream: decide a
//!   batch of actions via [`deliveries`], which returns a [`Delivery`] per
//!   row so each path rebuilds its own representation, then [`project_row`]
//!   each sent row. A documentation record the viewer can't open is sent as
//!   a delete naming only its id ([`Retraction`]): losing access reads as a
//!   delete, and hidden reads as absent.
//!
//! Source of truth stays [`crate::repository::ticket_visibility`] + the
//! documentation access fns; this module only orchestrates them.
//!
//! Two privilege tiers, deliberately separate:
//! - `sees_all` (workspace Agent+ / platform admin): staff. Governs the
//!   ticket family and staff records. An agent sees every ticket.
//! - `is_admin` (workspace Admin+ / platform admin) governs documentation
//!   and admin records. An agent is NOT an admin, so docs still filter for
//!   them.

use std::collections::{HashMap, HashSet};

use diesel::pg::Pg;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{PlatformRole, SyncAggregate, SyncOp, User};
use crate::repository::ticket_visibility::{self, VisibilityContext};
use crate::repository::{comments, directory, documentation, knowledge_gaps, user_helpers};
use crate::schema::tickets;
use crate::sync::audience::{audience, Audience};
use crate::sync::user_projection;

/// Per-viewer visibility identity, built once per request (delta /
/// bootstrap) or per connection (SSE). `Copy` so it can be moved into
/// blocking filter closures (delta `tc.run`, SSE `web::block`).
#[derive(Clone, Copy)]
pub struct SyncViewer {
    /// Ticket-family visibility context (carries `sees_all`).
    pub ctx: VisibilityContext,
    /// Workspace admin or owner, or platform admin: documentation and admin
    /// records. Distinct from `sees_all`.
    pub is_admin: bool,
}

impl SyncViewer {
    /// Build from a `User` row. Same role construction
    /// `sync::groups::admit_ticket_groups` uses, plus the admin flag.
    pub fn resolve(conn: &mut DbConnection, user: &User) -> Self {
        let ctx = VisibilityContext::new(
            user.uuid,
            PlatformRole::from_db(&user.platform_role),
            user_helpers::workspace_role(conn, user.uuid),
        );
        let is_admin = user_helpers::user_is_admin(conn, user);
        Self { ctx, is_admin }
    }

    /// Staff: workspace agent and up, or platform admin.
    pub fn sees_all(&self) -> bool {
        self.ctx.sees_all()
    }

    /// The viewer as [`user_projection`] sees them.
    pub fn projection(&self) -> user_projection::Viewer {
        user_projection::Viewer {
            uuid: self.ctx.user_uuid,
            is_staff: self.sees_all(),
        }
    }

    /// The documentation reader this viewer is.
    pub fn pages(&self) -> documentation::PageAudience {
        documentation::PageAudience::User {
            user_uuid: self.ctx.user_uuid,
            is_admin: self.is_admin,
        }
    }
}

/// Minimal projection of one sync action needed for a visibility
/// decision. Delta's `ActionRow`, an SSE `serde_json` row and the activity
/// endpoint's rows lower into this via the `extract` closure passed to
/// [`filter_actions`].
#[derive(Clone)]
pub struct ActionView {
    /// `None` when the wire aggregate name didn't parse: a kind of record
    /// this server has no audience for, which no one receives.
    pub aggregate: Option<SyncAggregate>,
    pub is_delete: bool,
    /// Parsed `aggregate_id` (the ticket id for `ticket`, the page /
    /// collection id for documentation).
    pub aggregate_id: Option<i32>,
    /// `data.ticket_id` when present (comment / ticket_asset /
    /// linked_ticket / project_ticket).
    pub ticket_id: Option<i32>,
    /// `data.is_internal` when present (comment.created).
    pub is_internal: Option<bool>,
    /// `data.comment_id` when present (attachment.created).
    pub comment_id: Option<i32>,
    /// `aggregate_id` parsed as a uuid: the user a `user` row is about.
    pub subject_uuid: Option<Uuid>,
    /// `data` names only the row (`id` / `uuid`), as a delete prune does, so
    /// it says nothing about anyone.
    pub bare_id: bool,
    /// The people the row is addressed to: its `user:<uuid>` groups. A
    /// writer adds that group only for the person a record is for (a
    /// notification's recipient, a loan's borrower, a draft's uploader).
    pub addressees: Vec<Uuid>,
    /// `aggregate_id` as a knowledge gap id (gap ids are 64-bit).
    pub gap_id: Option<i64>,
}

impl ActionView {
    /// Lower a `sync_actions` row's columns into a view. One place, so the
    /// delta and the activity endpoint read the same keys
    /// ([`Self::from_wire`] is the live stream's).
    pub fn from_row(
        aggregate: SyncAggregate,
        op: SyncOp,
        aggregate_id: &str,
        data: &serde_json::Value,
        groups: &[Option<String>],
    ) -> Self {
        Self::lower(
            Some(aggregate),
            matches!(op, SyncOp::Delete),
            aggregate_id,
            data,
            groups.iter().flatten().map(String::as_str),
        )
    }

    /// Lower a serialized `ActionRow` (the live stream's JSON rows). An
    /// aggregate name this server doesn't know leaves `aggregate` empty.
    pub fn from_wire(row: &serde_json::Value) -> Self {
        let aggregate = row
            .get("aggregate")
            .cloned()
            .and_then(|v| serde_json::from_value::<SyncAggregate>(v).ok());
        let groups = row
            .get("groups")
            .and_then(|g| g.as_array())
            .into_iter()
            .flatten()
            .filter_map(|g| g.as_str());
        Self::lower(
            aggregate,
            row.get("op").and_then(|v| v.as_str()) == Some("D"),
            row.get("aggregate_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            row.get("data").unwrap_or(&serde_json::Value::Null),
            groups,
        )
    }

    fn lower<'a>(
        aggregate: Option<SyncAggregate>,
        is_delete: bool,
        aggregate_id: &str,
        data: &serde_json::Value,
        groups: impl Iterator<Item = &'a str>,
    ) -> Self {
        let i32_at = |key: &str| {
            data.get(key)
                .and_then(|v| v.as_i64())
                .and_then(|n| i32::try_from(n).ok())
        };
        Self {
            aggregate,
            is_delete,
            aggregate_id: aggregate_id.parse().ok(),
            ticket_id: i32_at("ticket_id"),
            is_internal: data.get("is_internal").and_then(|v| v.as_bool()),
            comment_id: i32_at("comment_id"),
            subject_uuid: Uuid::parse_str(aggregate_id).ok(),
            bare_id: names_only_the_row(data),
            addressees: groups
                .filter_map(|g| g.strip_prefix("user:"))
                .filter_map(|u| Uuid::parse_str(u).ok())
                .collect(),
            gap_id: aggregate_id.parse().ok(),
        }
    }
}

/// Whether a payload names only its row (`{"id": ..}` / `{"uuid": ..}`).
pub fn names_only_the_row(data: &serde_json::Value) -> bool {
    data.as_object()
        .is_some_and(|o| !o.is_empty() && o.keys().all(|k| k == "id" || k == "uuid"))
}

/// True when this aggregate carries a ticket id and so needs the
/// visible-ticket set resolved for a restricted viewer.
fn needs_ticket_resolution(agg: SyncAggregate) -> bool {
    audience(agg) == Audience::Ticket
}

/// What a batch's visibility needed from the database, resolved once per
/// batch so each row's decision is I/O-free.
struct Resolved {
    /// The restricted viewer's visible ticket ids; `None` for staff, whose
    /// ticket family is not gated.
    visible_tickets: Option<HashSet<i32>>,
    /// Comment ids whose ticket is visible and that aren't internal (gates
    /// `attachment.created`).
    visible_comment_ids: HashSet<i32>,
    /// Documentation the viewer can't open.
    hidden_pages: HashSet<i32>,
    hidden_collections: HashSet<i32>,
    /// Knowledge gaps naming a page the viewer can't open.
    hidden_gaps: HashSet<i64>,
    /// Which of the batch's `user` rows are about this workspace's people.
    people: HashSet<Uuid>,
    /// Fail-closed flags: a lookup error drops the affected family.
    doc_fail: bool,
    ticket_fail: bool,
    gap_fail: bool,
}

impl Resolved {
    /// Nothing resolved and every lookup failed: what a viewer gets when the
    /// database can't be asked.
    fn failed(viewer: &SyncViewer) -> Self {
        Self {
            visible_tickets: (!viewer.sees_all()).then(HashSet::new),
            visible_comment_ids: HashSet::new(),
            hidden_pages: HashSet::new(),
            hidden_collections: HashSet::new(),
            hidden_gaps: HashSet::new(),
            people: HashSet::new(),
            doc_fail: true,
            ticket_fail: true,
            gap_fail: true,
        }
    }
}

/// What a viewer gets of one action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// The row as recorded (then [`project_row`]).
    Send,
    /// A delete naming only the row's id ([`Retraction`]): a documentation
    /// record the viewer can't open, so a client that holds it drops it.
    Retract,
    /// Nothing.
    Drop,
    /// A documentation row whose visibility couldn't be looked up. Neither
    /// sent (it may be closed to the viewer) nor dropped (it may withdraw
    /// their access, and a cursor moved past it would never bring that
    /// back): the delta and the live stream fail the batch, and the client
    /// asks again.
    Unclassified,
}

/// Whether a batch holds a row that couldn't be classified, so the reader
/// must fail it rather than move past it.
pub fn any_unclassified(decided: &[Delivery]) -> bool {
    decided.contains(&Delivery::Unclassified)
}

/// What `viewer` gets of one action: the row when they may see it; for a
/// documentation record they can't open, a delete, so a client that had it
/// drops it; otherwise nothing. A documentation row whose lookup failed is
/// [`Delivery::Unclassified`].
fn action_delivery(v: &ActionView, viewer: &SyncViewer, r: &Resolved) -> Delivery {
    let is_doc = v
        .aggregate
        .is_some_and(|agg| audience(agg) == Audience::Docs);
    if is_doc && r.doc_fail {
        return Delivery::Unclassified;
    }
    if action_is_visible(v, viewer, r) {
        return Delivery::Send;
    }
    if is_doc && v.aggregate_id.is_some() {
        Delivery::Retract
    } else {
        Delivery::Drop
    }
}

/// The delete a [`Delivery::Retract`] sends in place of a documentation row.
/// It reads as the record's own delete does, and carries only what a client
/// needs to apply it: no actor, no correlation, no time.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Retraction {
    pub sync_id: i64,
    pub aggregate: SyncAggregate,
    pub aggregate_id: String,
    pub op: SyncOp,
    pub event_type: &'static str,
    pub schema_version: i16,
    /// `{"id": <id>}`.
    pub data: serde_json::Value,
    pub groups: Vec<Option<String>>,
}

impl Retraction {
    /// The retraction of a row. `None` for a kind that is never retracted.
    pub fn of(
        sync_id: i64,
        aggregate: SyncAggregate,
        aggregate_id: &str,
        schema_version: i16,
        groups: Vec<Option<String>>,
    ) -> Option<Self> {
        let event_type = match aggregate {
            SyncAggregate::DocumentationPage => "documentation_page.deleted",
            SyncAggregate::DocumentationCollection => "documentation_collection.deleted",
            _ => return None,
        };
        let id: i32 = aggregate_id.parse().ok()?;
        Some(Self {
            sync_id,
            aggregate,
            aggregate_id: aggregate_id.to_string(),
            op: SyncOp::Delete,
            event_type,
            schema_version,
            data: serde_json::json!({ "id": id }),
            groups,
        })
    }

    /// The retraction of a serialized action row (the live stream's shape).
    pub fn of_wire_row(row: &serde_json::Value) -> Option<Self> {
        let aggregate = row
            .get("aggregate")
            .cloned()
            .and_then(|v| serde_json::from_value::<SyncAggregate>(v).ok())?;
        let groups = row
            .get("groups")
            .cloned()
            .and_then(|g| serde_json::from_value::<Vec<Option<String>>>(g).ok())
            .unwrap_or_default();
        Self::of(
            row.get("sync_id")
                .and_then(|v| v.as_i64())
                .unwrap_or_default(),
            aggregate,
            row.get("aggregate_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default(),
            row.get("schema_version")
                .and_then(|v| v.as_i64())
                .and_then(|v| i16::try_from(v).ok())
                .unwrap_or(1),
            groups,
        )
    }
}

/// Pure keep/drop decision for one action, from its kind's
/// [`audience`] and what the batch resolved.
fn action_is_visible(v: &ActionView, viewer: &SyncViewer, r: &Resolved) -> bool {
    // A kind of record this server can't name has no audience here: no one
    // receives it.
    let Some(agg) = v.aggregate else {
        return false;
    };
    let me = viewer.ctx.user_uuid;
    let addressed_to_me = v.addressees.contains(&me);
    match audience(agg) {
        Audience::All => match agg {
            // A user's change is recorded in whichever workspace the writer
            // was pinned to, so this workspace's feed can hold a change to
            // someone from another one. It reaches a session only when it is
            // about the viewer or one of this workspace's people. A bare-id
            // delete goes to everyone: by the time a person is purged no
            // record names them any more, and the prune is what clears them
            // from every pool and cache.
            SyncAggregate::User => {
                (v.is_delete && v.bare_id)
                    || v.subject_uuid
                        .is_some_and(|u| u == me || r.people.contains(&u))
            }
            _ => true,
        },
        Audience::Staff => {
            viewer.sees_all()
                && match agg {
                    // A gap naming a page the viewer can't open is absent to
                    // them, like the page.
                    SyncAggregate::KnowledgeGap => {
                        !r.gap_fail && v.gap_id.is_none_or(|id| !r.hidden_gaps.contains(&id))
                    }
                    _ => true,
                }
        }
        Audience::Admin => viewer.is_admin,
        Audience::Addressee => addressed_to_me,
        Audience::StaffAndAddressee => viewer.sees_all() || addressed_to_me,
        // Documentation: exclusion model, visible to everyone EXCEPT the
        // rows in the hidden set. Fail-closed drops all doc rows.
        Audience::Docs => {
            let hidden = if agg == SyncAggregate::DocumentationCollection {
                &r.hidden_collections
            } else {
                &r.hidden_pages
            };
            !r.doc_fail && v.aggregate_id.is_none_or(|id| !hidden.contains(&id))
        }
        Audience::Ticket => ticket_row_is_visible(v, agg, r, addressed_to_me),
    }
}

/// The ticket family. Inclusion model for restricted viewers: visible ONLY
/// for tickets in the visible set. Staff (`visible_tickets == None`) keep
/// everything.
fn ticket_row_is_visible(
    v: &ActionView,
    agg: SyncAggregate,
    r: &Resolved,
    addressed_to_me: bool,
) -> bool {
    let Some(visible) = r.visible_tickets.as_ref() else {
        return true; // sees_all
    };
    // Bare-id prune signals (`{id}` only) carry no information about a
    // ticket the viewer can't see, and MUST reach a member even when the
    // underlying row is already gone: a hard-deleted ticket/attachment can't
    // be confirmed against the live tables, so a visibility check would
    // wrongly drop the prune and the row would ghost in the member's pool
    // forever. Let them through (a prune of an id the member never had is a
    // harmless no-op), ahead of the fail-closed gate so a transient lookup
    // failure can't bring the ghost back.
    if v.is_delete && matches!(agg, SyncAggregate::Ticket | SyncAggregate::Attachment) {
        return true;
    }
    // A draft upload not yet on a comment is its uploader's alone (its only
    // group is theirs).
    if agg == SyncAggregate::Attachment && v.comment_id.is_none() && addressed_to_me {
        return true;
    }
    if r.ticket_fail {
        return false;
    }
    let on_visible_ticket = v.ticket_id.is_some_and(|t| visible.contains(&t));
    match agg {
        SyncAggregate::Ticket => v.aggregate_id.is_some_and(|id| visible.contains(&id)),
        SyncAggregate::Comment => on_visible_ticket && v.is_internal != Some(true),
        // Non-delete: gate by the parent comment's visibility. (Delete is
        // handled by the bare-id prune branch above.)
        SyncAggregate::Attachment => v
            .comment_id
            .is_some_and(|c| r.visible_comment_ids.contains(&c)),
        // Usage recorded against a ticket the viewer can see. Ad-hoc events
        // (restock / write-off, `ticket_id` null) are inventory work with no
        // ticket, so a restricted viewer gets none.
        // TicketAsset, LinkedTicket, ProjectTicket, CycleTicket, AssetUsage:
        // follow the ticket.
        _ => on_visible_ticket,
    }
}

/// Filter a batch of actions for a viewer, returning a keep-mask
/// parallel to `items`: [`deliveries`] for a reader that only keeps or
/// drops (a retraction is dropped).
pub fn filter_actions<T>(
    conn: &mut DbConnection,
    viewer: &SyncViewer,
    items: &[T],
    extract: impl Fn(&T) -> ActionView,
) -> Vec<bool> {
    deliveries(conn, viewer, items, extract)
        .into_iter()
        .map(|d| d == Delivery::Send)
        .collect()
}

/// Decide a batch of actions for a viewer, returning a [`Delivery`]
/// parallel to `items`. Runs a few batched, indexed queries, and only
/// for the families the batch actually contains. Never errors: a
/// visibility-lookup failure drops the affected family wholesale
/// (fail-closed) so a restricted viewer is never 500'd.
pub fn deliveries<T>(
    conn: &mut DbConnection,
    viewer: &SyncViewer,
    items: &[T],
    extract: impl Fn(&T) -> ActionView,
) -> Vec<Delivery> {
    let views: Vec<ActionView> = items.iter().map(extract).collect();
    let sees_all = viewer.sees_all();
    let viewer_uuid = viewer.ctx.user_uuid;
    let has = |agg: SyncAggregate| views.iter().any(|v| v.aggregate == Some(agg));

    // --- Users (applies to every viewer) ---
    // Which of the batch's `user` rows are about this workspace's people.
    // The workspace is the connection's pin; with none, or on a lookup
    // failure, only the viewer's own row goes through (fail-closed).
    let subjects: Vec<Uuid> = views
        .iter()
        .filter(|v| v.aggregate == Some(SyncAggregate::User))
        .filter_map(|v| v.subject_uuid)
        .filter(|u| *u != viewer_uuid)
        .collect();
    let people: HashSet<Uuid> = if subjects.is_empty() {
        HashSet::new()
    } else {
        let resolved = crate::sync::session::current_workspace_id(conn).and_then(|ws| match ws {
            Some(ws) => directory::people_among(conn, ws, &subjects),
            None => Ok(HashSet::new()),
        });
        resolved.unwrap_or_else(|e| {
            tracing::error!(error = %e, "sync visibility: people lookup failed; dropping other users' rows (fail-closed)");
            HashSet::new()
        })
    };

    // --- Documentation (applies to every viewer) ---
    let mut page_ids = Vec::new();
    let mut collection_ids = Vec::new();
    for v in &views {
        match v.aggregate {
            Some(SyncAggregate::DocumentationPage) => page_ids.extend(v.aggregate_id),
            Some(SyncAggregate::DocumentationCollection) => collection_ids.extend(v.aggregate_id),
            _ => {}
        }
    }
    let (hidden_pages, hidden_collections, doc_fail) = if page_ids.is_empty()
        && collection_ids.is_empty()
    {
        (HashSet::new(), HashSet::new(), false)
    } else {
        match viewer.pages().hidden(conn, &page_ids, &collection_ids) {
            Ok((hp, hc)) => (hp, hc, false),
            Err(e) => {
                tracing::error!(error = %e, "sync visibility: documentation filter failed; dropping doc rows (fail-closed)");
                (HashSet::new(), HashSet::new(), true)
            }
        }
    };

    // --- Knowledge gaps (staff only; no one else receives them) ---
    // One read of the batch's gaps, for the pages they name now: a signal's
    // row carries only the gap's id.
    let gap_ids: Vec<i64> = if sees_all && has(SyncAggregate::KnowledgeGap) {
        views
            .iter()
            .filter(|v| v.aggregate == Some(SyncAggregate::KnowledgeGap))
            .filter_map(|v| v.gap_id)
            .collect()
    } else {
        Vec::new()
    };
    let (hidden_gaps, gap_fail) = if gap_ids.is_empty() {
        (HashSet::new(), false)
    } else {
        match knowledge_gaps::naming_pages_hidden_from(conn, &viewer.pages(), &gap_ids) {
            Ok(hidden) => (hidden, false),
            Err(e) => {
                tracing::error!(error = %e, "sync visibility: knowledge gap filter failed; dropping gap rows (fail-closed)");
                (HashSet::new(), true)
            }
        }
    };

    // --- Ticket-gated families (only for restricted viewers) ---
    let needs_resolution = views
        .iter()
        .any(|v| v.aggregate.is_some_and(needs_ticket_resolution));
    let (visible_tickets, visible_comment_ids, ticket_fail) = if sees_all || !needs_resolution {
        // sees_all => None (keep all). Nothing to resolve => an empty
        // visible set; it stays `Some` for restricted viewers.
        let vt = if sees_all { None } else { Some(HashSet::new()) };
        (vt, HashSet::new(), false)
    } else {
        // Resolve attachment parent comments first (their tickets join
        // the candidate set), then the visible-ticket set, then which
        // of those comments are actually visible.
        let attach_comment_ids: Vec<i32> = views
            .iter()
            .filter(|v| v.aggregate == Some(SyncAggregate::Attachment) && !v.is_delete)
            .filter_map(|v| v.comment_id)
            .collect();
        let comment_map = if attach_comment_ids.is_empty() {
            Ok(HashMap::new())
        } else {
            comments::ticket_and_internal_for_comments(conn, &attach_comment_ids)
        };

        match comment_map {
            Ok(comment_map) => {
                let mut candidates: Vec<i32> = Vec::new();
                for v in &views {
                    match v.aggregate {
                        Some(SyncAggregate::Ticket) => candidates.extend(v.aggregate_id),
                        Some(agg) if needs_ticket_resolution(agg) => candidates.extend(v.ticket_id),
                        _ => {}
                    }
                }
                candidates.extend(comment_map.values().map(|(t, _)| *t));

                match ticket_visibility::visible_ticket_ids(conn, &viewer.ctx, &candidates) {
                    Ok(visible) => {
                        let visible_comment_ids: HashSet<i32> = comment_map
                            .iter()
                            .filter(|(_, (t, internal))| !*internal && visible.contains(t))
                            .map(|(id, _)| *id)
                            .collect();
                        (Some(visible), visible_comment_ids, false)
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "sync visibility: ticket filter failed; dropping ticket-family rows (fail-closed)");
                        (Some(HashSet::new()), HashSet::new(), true)
                    }
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "sync visibility: comment resolve failed; dropping ticket-family rows (fail-closed)");
                (Some(HashSet::new()), HashSet::new(), true)
            }
        }
    };

    let resolved = Resolved {
        visible_tickets,
        visible_comment_ids,
        hidden_pages,
        hidden_collections,
        hidden_gaps,
        people,
        doc_fail,
        ticket_fail,
        gap_fail,
    };
    views
        .iter()
        .map(|v| action_delivery(v, viewer, &resolved))
        .collect()
}

/// A kept row as `viewer` may see it: a `user` row loses the fields they may
/// not see ([`user_projection`]); every other kind is sent whole. A bare-id
/// prune names no one and is left as it is.
pub fn project_row(
    viewer: &SyncViewer,
    aggregate: Option<SyncAggregate>,
    data: &mut serde_json::Value,
) {
    if aggregate == Some(SyncAggregate::User) && !names_only_the_row(data) {
        user_projection::value_for_viewer(data, viewer.projection());
    }
}

/// The ticket query the bootstrap snapshot should load: every ticket for
/// staff, only the requester/watcher set for restricted viewers. Thin
/// wrapper over [`ticket_visibility::visible_tickets_query`] keyed off
/// the viewer.
pub fn bootstrap_ticket_query<'a>(viewer: &SyncViewer) -> tickets::BoxedQuery<'a, Pg> {
    ticket_visibility::visible_tickets_query(&viewer.ctx)
}

/// Cheap, I/O-free gate: does an action with this wire aggregate name need
/// [`filter_actions`] (and [`project_row`]) for this viewer? A name this
/// server doesn't know always does, so it is dropped. The SSE path uses
/// this to skip the async DB hop for batches that don't need filtering for
/// this viewer.
pub fn wire_aggregate_is_gated(wire: &str, viewer: &SyncViewer) -> bool {
    let Ok(agg) = serde_json::from_value::<SyncAggregate>(serde_json::Value::from(wire)) else {
        return true;
    };
    match audience(agg) {
        // People rows are filtered to the workspace's people and projected.
        Audience::All => agg == SyncAggregate::User,
        // Staff see every staff record but a gap naming a page they can't
        // open.
        Audience::Staff => !viewer.sees_all() || agg == SyncAggregate::KnowledgeGap,
        Audience::Admin => !viewer.is_admin,
        Audience::StaffAndAddressee | Audience::Ticket => !viewer.sees_all(),
        Audience::Addressee | Audience::Docs => true,
    }
}

/// [`filter_actions`] on a connection from `pool`, pinned to the viewer's
/// workspace, for a caller that holds no request connection (the live event
/// stream). The pin matters: on an unpinned connection row security hides
/// every row the filter reads, and a documentation page it cannot load is not
/// counted as hidden, so the filter would let restricted pages through. A pool
/// or pin failure gives [`fail_closed_deliveries`].
pub fn filter_actions_pinned<T>(
    pool: &crate::db::Pool,
    workspace_id: i32,
    viewer: &SyncViewer,
    items: &[T],
    extract: impl Fn(&T) -> ActionView,
) -> Vec<bool> {
    deliveries_pinned(pool, workspace_id, viewer, items, extract)
        .into_iter()
        .map(|d| d == Delivery::Send)
        .collect()
}

/// [`deliveries`] on a pinned connection from `pool`, as
/// [`filter_actions_pinned`] explains. A pool or pin failure gives
/// [`fail_closed_deliveries`].
pub fn deliveries_pinned<T>(
    pool: &crate::db::Pool,
    workspace_id: i32,
    viewer: &SyncViewer,
    items: &[T],
    extract: impl Fn(&T) -> ActionView,
) -> Vec<Delivery> {
    let actor =
        crate::sync::actor::ActorContext::user_at_workspace(viewer.ctx.user_uuid, workspace_id);
    let decided = pool.get().ok().and_then(|mut conn| {
        crate::sync::session::with_actor_context(&mut conn, &actor, |c| {
            Ok::<_, diesel::result::Error>(deliveries(c, viewer, items, &extract))
        })
        .ok()
    });
    decided.unwrap_or_else(|| fail_closed_deliveries(viewer, items, extract))
}

/// Fail-closed deliveries computed with no DB access: drops every family
/// that needs a lookup (documentation and knowledge gaps for all viewers,
/// every user row but the viewer's own, the ticket family for restricted
/// viewers) and keeps what the viewer's role alone decides. Used by the SSE
/// path when the off-thread visibility lookup can't run (e.g. pool
/// exhaustion), so a transient failure never widens what a viewer gets.
pub fn fail_closed_deliveries<T>(
    viewer: &SyncViewer,
    items: &[T],
    extract: impl Fn(&T) -> ActionView,
) -> Vec<Delivery> {
    let failed = Resolved::failed(viewer);
    items
        .iter()
        .map(|it| action_delivery(&extract(it), viewer, &failed))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WorkspaceRole;

    fn view(agg: SyncAggregate, is_delete: bool) -> ActionView {
        ActionView {
            aggregate: Some(agg),
            is_delete,
            aggregate_id: None,
            ticket_id: None,
            is_internal: None,
            comment_id: None,
            subject_uuid: None,
            bare_id: false,
            addressees: Vec::new(),
            gap_id: None,
        }
    }

    fn viewer(role: WorkspaceRole) -> SyncViewer {
        SyncViewer {
            ctx: VisibilityContext::new(Uuid::nil(), PlatformRole::from_db("user"), Some(role)),
            is_admin: role.meets(WorkspaceRole::Admin),
        }
    }

    fn member() -> SyncViewer {
        viewer(WorkspaceRole::Member)
    }

    fn staff() -> SyncViewer {
        viewer(WorkspaceRole::Agent)
    }

    fn admin() -> SyncViewer {
        viewer(WorkspaceRole::Admin)
    }

    /// Nothing hidden; a restricted viewer sees ticket 1 only.
    fn resolved(viewer: &SyncViewer) -> Resolved {
        Resolved {
            visible_tickets: (!viewer.sees_all()).then(|| HashSet::from([1])),
            visible_comment_ids: HashSet::new(),
            hidden_pages: HashSet::new(),
            hidden_collections: HashSet::new(),
            hidden_gaps: HashSet::new(),
            people: HashSet::new(),
            doc_fail: false,
            ticket_fail: false,
            gap_fail: false,
        }
    }

    fn check(v: &ActionView, viewer: &SyncViewer) -> bool {
        action_is_visible(v, viewer, &resolved(viewer))
    }

    fn check_with_comments(v: &ActionView, viewer: &SyncViewer, comments: &[i32]) -> bool {
        let mut r = resolved(viewer);
        r.visible_comment_ids = comments.iter().copied().collect();
        action_is_visible(v, viewer, &r)
    }

    #[test]
    fn staff_sees_all_ticket_family() {
        for agg in [
            SyncAggregate::Ticket,
            SyncAggregate::Comment,
            SyncAggregate::Attachment,
            SyncAggregate::TicketAsset,
            SyncAggregate::LinkedTicket,
            SyncAggregate::ProjectTicket,
        ] {
            assert!(check(&view(agg, false), &staff()), "{agg:?} for staff");
        }
    }

    #[test]
    fn member_ticket_inclusion() {
        let mut own = view(SyncAggregate::Ticket, false);
        own.aggregate_id = Some(1);
        let mut other = view(SyncAggregate::Ticket, false);
        other.aggregate_id = Some(2);
        assert!(check(&own, &member()), "own ticket visible");
        assert!(!check(&other, &member()), "other ticket hidden");
    }

    #[test]
    fn member_comment_drops_internal_and_others() {
        let comment = |ticket, internal| {
            let mut c = view(SyncAggregate::Comment, false);
            c.ticket_id = Some(ticket);
            c.is_internal = Some(internal);
            c
        };
        assert!(check(&comment(1, false), &member()), "own public comment");
        assert!(
            !check(&comment(1, true), &member()),
            "own internal comment hidden"
        );
        assert!(
            !check(&comment(2, false), &member()),
            "other's comment hidden"
        );
    }

    #[test]
    fn member_comment_delete_kept_for_visible_ticket() {
        // comment.deleted has ticket_id but no is_internal -> kept iff ticket visible.
        let mut del_own = view(SyncAggregate::Comment, true);
        del_own.ticket_id = Some(1);
        let mut del_other = view(SyncAggregate::Comment, true);
        del_other.ticket_id = Some(2);
        assert!(check(&del_own, &member()));
        assert!(!check(&del_other, &member()));
    }

    #[test]
    fn member_junctions_follow_ticket() {
        for agg in [
            SyncAggregate::TicketAsset,
            SyncAggregate::LinkedTicket,
            SyncAggregate::ProjectTicket,
            SyncAggregate::CycleTicket,
        ] {
            let mut own = view(agg, false);
            own.ticket_id = Some(1);
            let mut other = view(agg, false);
            other.ticket_id = Some(2);
            assert!(check(&own, &member()), "{agg:?} own");
            assert!(!check(&other, &member()), "{agg:?} other");
        }
    }

    #[test]
    fn member_attachment_created_gated_by_comment_set() {
        let mut visible_att = view(SyncAggregate::Attachment, false);
        visible_att.comment_id = Some(10);
        let mut hidden_att = view(SyncAggregate::Attachment, false);
        hidden_att.comment_id = Some(11);
        assert!(check_with_comments(&visible_att, &member(), &[10]));
        assert!(!check_with_comments(&hidden_att, &member(), &[10]));
    }

    #[test]
    fn a_draft_upload_reaches_its_uploader() {
        let mut draft = view(SyncAggregate::Attachment, false);
        assert!(!check(&draft, &member()), "someone else's draft");
        draft.addressees = vec![Uuid::nil()];
        assert!(check(&draft, &member()), "the member's own draft");
        // Fail-closed ticket lookups don't take the member's own draft away.
        assert!(action_is_visible(
            &draft,
            &member(),
            &Resolved::failed(&member())
        ));
    }

    #[test]
    fn member_bare_id_deletes_pass_as_prune() {
        // ticket.deleted / attachment.deleted carry only `{id}`: the row is
        // gone, so they pass through as harmless prune signals (else a
        // hard-deleted ticket ghosts in the member's pool forever).
        let mut ticket_del = view(SyncAggregate::Ticket, true);
        ticket_del.aggregate_id = Some(999); // not in visible set
        let att_del = view(SyncAggregate::Attachment, true);
        assert!(check(&ticket_del, &member()), "ticket delete prunes");
        assert!(check(&att_del, &member()), "attachment delete prunes");
    }

    #[test]
    fn member_asset_usage_gated_by_ticket() {
        let mut on_visible = view(SyncAggregate::AssetUsage, false);
        on_visible.ticket_id = Some(1);
        let mut on_hidden = view(SyncAggregate::AssetUsage, false);
        on_hidden.ticket_id = Some(2);
        let adhoc = view(SyncAggregate::AssetUsage, false); // ticket_id None
        assert!(check(&on_visible, &member()), "usage on own ticket");
        assert!(!check(&on_hidden, &member()), "usage on other ticket");
        assert!(!check(&adhoc, &member()), "ad-hoc usage dropped");
        // Staff see all usage, ad-hoc included.
        assert!(check(&on_hidden, &staff()), "staff usage");
        assert!(check(&adhoc, &staff()), "staff ad-hoc usage");
    }

    #[test]
    fn staff_records_reach_staff_only() {
        for agg in [
            SyncAggregate::AssetAudit,
            SyncAggregate::AssetLifecycleEvent,
            SyncAggregate::AssetMedia,
            SyncAggregate::Assignment,
            SyncAggregate::GroupMembership,
            SyncAggregate::KnowledgeGap,
            SyncAggregate::TicketReference,
        ] {
            let row = view(agg, false);
            assert!(!check(&row, &member()), "{agg:?} hidden from a member");
            assert!(check(&row, &staff()), "{agg:?} reaches an agent");
            assert!(check(&row, &admin()), "{agg:?} reaches an admin");
        }
    }

    #[test]
    fn admin_records_reach_admins_only() {
        for agg in [
            SyncAggregate::Channel,
            SyncAggregate::Data,
            SyncAggregate::Webhook,
        ] {
            let row = view(agg, false);
            assert!(!check(&row, &member()), "{agg:?} hidden from a member");
            assert!(!check(&row, &staff()), "{agg:?} hidden from an agent");
            assert!(check(&row, &admin()), "{agg:?} reaches an admin");
        }
    }

    #[test]
    fn addressed_records_reach_their_addressee() {
        let mut notification = view(SyncAggregate::Notification, false);
        assert!(
            !check(&notification, &admin()),
            "someone else's notification"
        );
        notification.addressees = vec![Uuid::nil()];
        assert!(
            check(&notification, &member()),
            "the member's own notification"
        );

        let mut loan = view(SyncAggregate::AssetLoan, false);
        assert!(check(&loan, &staff()), "staff see every loan");
        assert!(!check(&loan, &member()), "someone else's loan");
        loan.addressees = vec![Uuid::nil()];
        assert!(check(&loan, &member()), "the member's own loan");
    }

    #[test]
    fn a_gap_naming_a_hidden_page_is_left_out() {
        let mut gap = view(SyncAggregate::KnowledgeGap, false);
        gap.gap_id = Some(7);
        let mut r = resolved(&staff());
        r.hidden_gaps = HashSet::from([7]);
        assert!(!action_is_visible(&gap, &staff(), &r));
        gap.gap_id = Some(8);
        assert!(action_is_visible(&gap, &staff(), &r));
        r.gap_fail = true;
        assert!(!action_is_visible(&gap, &staff(), &r), "fail-closed");
    }

    #[test]
    fn documentation_exclusion_applies_to_everyone() {
        let mut hidden = view(SyncAggregate::DocumentationPage, false);
        hidden.aggregate_id = Some(5);
        let mut visible = view(SyncAggregate::DocumentationPage, false);
        visible.aggregate_id = Some(6);
        // Even a staff viewer is gated on docs. A record they can't open is
        // sent as a delete, so one they held goes away.
        let mut r = resolved(&staff());
        r.hidden_pages = HashSet::from([5]);
        assert_eq!(
            action_delivery(&hidden, &staff(), &r),
            Delivery::Retract,
            "hidden doc retracted even for staff"
        );
        assert_eq!(
            action_delivery(&visible, &staff(), &r),
            Delivery::Send,
            "non-hidden doc kept"
        );
        // A failed lookup neither sends nor drops: the reader fails the batch.
        r.doc_fail = true;
        assert_eq!(
            action_delivery(&hidden, &staff(), &r),
            Delivery::Unclassified
        );
        assert_eq!(
            action_delivery(&visible, &staff(), &r),
            Delivery::Unclassified
        );
    }

    #[test]
    fn a_retraction_names_only_the_record() {
        let row = serde_json::json!({
            "sync_id": 9,
            "aggregate": "documentation_page",
            "aggregate_id": "5",
            "op": "U",
            "event_type": "documentation_page.visibility_changed",
            "schema_version": 1,
            "data": { "id": 5, "title": "Salaries" },
            "groups": ["workspace:1"],
            "actor_uuid": Uuid::new_v4(),
            "actor_kind": "user",
            "actor_ref": null,
            "correlation_id": Uuid::new_v4(),
            "causation_id": null,
            "occurred_at": "2026-10-11T00:00:00Z",
        });
        let retraction =
            serde_json::to_value(Retraction::of_wire_row(&row).expect("a retraction")).unwrap();
        assert_eq!(
            retraction,
            serde_json::json!({
                "sync_id": 9,
                "aggregate": "documentation_page",
                "aggregate_id": "5",
                "op": "D",
                "event_type": "documentation_page.deleted",
                "schema_version": 1,
                "data": { "id": 5 },
                "groups": ["workspace:1"],
            })
        );
        let ticket = serde_json::json!({ "aggregate": "ticket", "aggregate_id": "5" });
        assert!(
            Retraction::of_wire_row(&ticket).is_none(),
            "only documentation retracts"
        );
    }

    #[test]
    fn records_for_everyone_reach_a_member() {
        for agg in [
            SyncAggregate::Asset,
            SyncAggregate::WorkflowState,
            SyncAggregate::Project,
            SyncAggregate::Cycle,
            SyncAggregate::Plugin,
        ] {
            assert!(check(&view(agg, false), &member()), "{agg:?} allowed");
        }
    }

    #[test]
    fn user_rows_only_for_self_and_the_workspaces_people() {
        let (colleague, stranger) = (Uuid::new_v4(), Uuid::new_v4());
        let me = Uuid::nil();
        // (is_delete, bare_id): a full row, a full-row delete (as recorded
        // before deletes were trimmed), and a bare-id prune.
        let about = |u: Uuid, (is_delete, bare_id): (bool, bool)| {
            let mut v = view(SyncAggregate::User, is_delete);
            v.subject_uuid = Some(u);
            v.bare_id = bare_id;
            v
        };
        for viewer in [member(), staff()] {
            let mut r = resolved(&viewer);
            r.people = HashSet::from([colleague]);
            for shape in [(false, false), (true, false), (true, true)] {
                let kept = |u| action_is_visible(&about(u, shape), &viewer, &r);
                assert!(kept(me), "the viewer's own row: {shape:?}");
                assert!(kept(colleague), "one of the workspace's people: {shape:?}");
                // A prune names nobody, and must clear a purged person from
                // every pool; anything more stays with the workspace's people.
                assert_eq!(
                    kept(stranger),
                    shape == (true, true),
                    "someone from another workspace: {shape:?}"
                );
            }
        }
        // No subject (an unparseable aggregate id) names nobody.
        assert!(!check(&view(SyncAggregate::User, false), &staff()));
    }

    #[test]
    fn fail_closed_drops_family() {
        let mut ticket = view(SyncAggregate::Ticket, false);
        ticket.aggregate_id = Some(1); // would be visible, but ticket_fail drops it
        let mut page = view(SyncAggregate::DocumentationPage, false);
        page.aggregate_id = Some(6);
        let failed = Resolved::failed(&member());
        assert!(
            !action_is_visible(&page, &member(), &failed),
            "doc_fail drops doc row"
        );
        assert!(
            !action_is_visible(&ticket, &member(), &failed),
            "ticket_fail drops ticket row"
        );
        assert!(
            action_is_visible(&view(SyncAggregate::Plugin, false), &member(), &failed),
            "records for everyone need no lookup"
        );
    }

    #[test]
    fn an_unknown_kind_of_record_reaches_no_one() {
        let mut v = view(SyncAggregate::Plugin, false);
        v.aggregate = None;
        for viewer in [member(), staff(), admin()] {
            assert!(!check(&v, &viewer));
        }
        let parsed = ActionView::from_wire(&serde_json::json!({
            "aggregate": "record_kind_from_the_future",
            "aggregate_id": "1",
            "op": "I",
            "data": {},
            "groups": ["workspace:1"],
        }));
        assert!(parsed.aggregate.is_none());
        assert!(wire_aggregate_is_gated(
            "record_kind_from_the_future",
            &admin()
        ));
    }

    #[test]
    fn gating_follows_the_audience() {
        // Records for everyone skip the lookup; people rows never do.
        assert!(!wire_aggregate_is_gated("plugin", &member()));
        assert!(wire_aggregate_is_gated("user", &admin()));
        // Admin records: gated for anyone but an admin.
        assert!(wire_aggregate_is_gated("webhook", &staff()));
        assert!(!wire_aggregate_is_gated("webhook", &admin()));
        // Staff records: gated for a member; a gap is checked for staff too.
        assert!(wire_aggregate_is_gated("group_membership", &member()));
        assert!(!wire_aggregate_is_gated("group_membership", &staff()));
        assert!(wire_aggregate_is_gated("knowledge_gap", &staff()));
        // Addressed records and documentation always.
        assert!(wire_aggregate_is_gated("notification", &admin()));
        assert!(wire_aggregate_is_gated("documentation_page", &admin()));
    }

    #[test]
    fn addressees_are_the_rows_user_groups() {
        let someone = Uuid::new_v4();
        let v = ActionView::from_row(
            SyncAggregate::AssetLoan,
            SyncOp::Insert,
            "3",
            &serde_json::json!({ "id": 3 }),
            &[
                Some("workspace:1".to_string()),
                Some(format!("user:{someone}")),
                Some("asset:9".to_string()),
            ],
        );
        assert_eq!(v.addressees, vec![someone]);
    }
}
