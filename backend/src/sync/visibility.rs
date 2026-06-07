//! Per-viewer visibility for the sync read paths.
//!
//! The sync engine delivers `sync_actions` to clients by `groups`
//! overlap, but some aggregates need row-level visibility finer than
//! the group grant expresses. Documentation has page/collection ACLs
//! (restricted even among staff); tickets are restricted for
//! `member`-role users (requester/watcher only). This module is the
//! single place that decides "can THIS viewer see THIS row," shared by
//! all three read paths:
//!
//! - **bootstrap** (snapshot): filters at the query level via
//!   [`bootstrap_ticket_query`].
//! - **delta** (pull) and the live **SSE** `SyncActions` stream:
//!   filter a batch of actions via [`filter_actions`], which returns a
//!   keep-mask so each path rebuilds its own representation.
//!
//! Source of truth stays [`crate::repository::ticket_visibility`] +
//! the documentation access fns — this module only orchestrates them.
//!
//! Two privilege tiers, deliberately separate:
//! - `sees_all` (workspace Agent+ / platform admin) governs the ticket
//!   family. An agent sees every ticket.
//! - `is_doc_admin` (workspace Admin+) governs documentation. An agent
//!   is NOT a doc admin, so docs still filter for them.

use std::collections::{HashMap, HashSet};

use diesel::pg::Pg;

use crate::db::DbConnection;
use crate::models::{PlatformRole, SyncAggregate, User};
use crate::repository::ticket_visibility::{self, VisibilityContext};
use crate::repository::{comments, documentation, user_helpers};
use crate::schema::tickets;

/// Per-viewer visibility identity, built once per request (delta /
/// bootstrap) or per connection (SSE). `Copy` so it can be moved into
/// blocking filter closures (delta `tc.run`, SSE `web::block`).
#[derive(Clone, Copy)]
pub struct SyncViewer {
    /// Ticket-family visibility context (carries `sees_all`).
    pub ctx: VisibilityContext,
    /// Documentation gate (Admin+). Distinct from `sees_all`.
    pub is_doc_admin: bool,
}

impl SyncViewer {
    /// Build from a `User` row. Same role construction
    /// `sync::groups::admit_ticket_groups` uses, plus the doc-admin flag.
    pub fn resolve(conn: &mut DbConnection, user: &User) -> Self {
        let ctx = VisibilityContext::new(
            user.uuid,
            PlatformRole::from_db(&user.platform_role),
            user_helpers::bootstrap_workspace_role(conn, user.uuid),
        );
        let is_doc_admin = user_helpers::user_is_admin(conn, user);
        Self { ctx, is_doc_admin }
    }

    fn sees_all(&self) -> bool {
        self.ctx.sees_all()
    }
}

/// Minimal projection of one sync action needed for a visibility
/// decision. Both delta's `ActionRow` and an SSE `serde_json` row lower
/// into this via the `extract` closure passed to [`filter_actions`].
#[derive(Clone)]
pub struct ActionView {
    /// `None` when the wire aggregate name didn't parse (treated as a
    /// non-gated/reference row — allow).
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
}

/// True when this aggregate's visibility is governed by ticket access.
fn is_ticket_family(agg: SyncAggregate) -> bool {
    matches!(
        agg,
        SyncAggregate::Ticket
            | SyncAggregate::Comment
            | SyncAggregate::Attachment
            | SyncAggregate::TicketAsset
            | SyncAggregate::LinkedTicket
            | SyncAggregate::ProjectTicket
            | SyncAggregate::CycleTicket
    )
}

/// True when this aggregate carries a ticket id and so needs the
/// visible-ticket set resolved, even though it isn't strictly part of
/// the ticket family. `asset_usage` is an inventory ledger row that may
/// reference a ticket; a restricted viewer should only learn of usage
/// recorded against a ticket they can see.
fn needs_ticket_resolution(agg: SyncAggregate) -> bool {
    is_ticket_family(agg) || matches!(agg, SyncAggregate::AssetUsage)
}

/// Pure keep/drop decision for one action. All inputs pre-resolved so
/// this is I/O-free and exhaustively unit-testable.
///
/// - `visible_tickets`: the restricted viewer's visible ticket-id set;
///   `None` means the viewer is `sees_all` (ticket family not gated).
/// - `visible_comment_ids`: comment ids whose parent ticket is visible
///   AND non-internal (gates `attachment.created`).
/// - `hidden_pages` / `hidden_collections`: documentation exclusion sets
///   (apply to every viewer).
/// - `doc_fail` / `ticket_fail`: fail-closed flags — on a visibility
///   lookup error the affected family is dropped wholesale.
#[allow(clippy::too_many_arguments)]
fn action_is_visible(
    v: &ActionView,
    visible_tickets: Option<&HashSet<i32>>,
    visible_comment_ids: &HashSet<i32>,
    hidden_pages: &HashSet<i32>,
    hidden_collections: &HashSet<i32>,
    doc_fail: bool,
    ticket_fail: bool,
) -> bool {
    let Some(agg) = v.aggregate else {
        // Unknown/unparsed aggregate: not a gated family — allow.
        return true;
    };
    match agg {
        // Documentation: exclusion model — visible to everyone EXCEPT
        // the rows in the hidden set. Fail-closed drops all doc rows.
        SyncAggregate::DocumentationPage => {
            !doc_fail && v.aggregate_id.is_none_or(|id| !hidden_pages.contains(&id))
        }
        SyncAggregate::DocumentationCollection => {
            !doc_fail
                && v.aggregate_id
                    .is_none_or(|id| !hidden_collections.contains(&id))
        }
        // Ticket family: inclusion model for restricted viewers — visible
        // ONLY for tickets in the visible set. Staff (`visible_tickets ==
        // None`) keep everything.
        _ if is_ticket_family(agg) => {
            let Some(visible) = visible_tickets else {
                return true; // sees_all
            };
            // Bare-id prune signals (`{id}` only) carry no information about
            // a ticket the viewer can't see, and MUST reach a member even
            // when the underlying row is already gone — a hard-deleted
            // ticket/attachment can't be confirmed against the live tables,
            // so a visibility check would wrongly drop the prune and the row
            // would ghost in the member's pool forever. Let them through
            // (a prune of an id the member never had is a harmless no-op),
            // ahead of the fail-closed gate so a transient lookup failure
            // can't reintroduce the ghost.
            if v.is_delete && matches!(agg, SyncAggregate::Ticket | SyncAggregate::Attachment) {
                return true;
            }
            if ticket_fail {
                return false;
            }
            match agg {
                SyncAggregate::Ticket => v.aggregate_id.is_some_and(|id| visible.contains(&id)),
                SyncAggregate::Comment => {
                    v.ticket_id.is_some_and(|t| visible.contains(&t)) && v.is_internal != Some(true)
                }
                SyncAggregate::TicketAsset
                | SyncAggregate::LinkedTicket
                | SyncAggregate::ProjectTicket
                | SyncAggregate::CycleTicket => v.ticket_id.is_some_and(|t| visible.contains(&t)),
                SyncAggregate::Attachment => {
                    // Non-delete: gate by the parent comment's visibility.
                    // (Delete is handled by the bare-id prune branch above.)
                    v.comment_id
                        .is_some_and(|c| visible_comment_ids.contains(&c))
                }
                _ => unreachable!("is_ticket_family covers these"),
            }
        }
        // Inventory usage ledger: staff see all; a restricted viewer only
        // learns of usage recorded against a ticket they can see. Ad-hoc
        // events (restock / write-off, `ticket_id` null) are inventory ops
        // with no ticket tie-in -> dropped for restricted viewers.
        SyncAggregate::AssetUsage => match visible_tickets {
            None => true, // sees_all
            Some(visible) => !ticket_fail && v.ticket_id.is_some_and(|t| visible.contains(&t)),
        },
        // Inventory audit trail: workspace-wide staff data, no ticket tie-in.
        // Staff only; never delivered to restricted viewers.
        SyncAggregate::AssetAudit => visible_tickets.is_none(),
        // Reference data + everything else: allow. Future aggregates that
        // need gating must add an arm above (conscious opt-in).
        _ => true,
    }
}

/// Filter a batch of actions for a viewer, returning a keep-mask
/// parallel to `items`. Runs at most three batched, indexed queries —
/// and only when the batch actually contains that family. Never errors:
/// a visibility-lookup failure drops the affected family wholesale
/// (fail-closed) so a restricted viewer is never 500'd.
pub fn filter_actions<T>(
    conn: &mut DbConnection,
    viewer: &SyncViewer,
    items: &[T],
    extract: impl Fn(&T) -> ActionView,
) -> Vec<bool> {
    let views: Vec<ActionView> = items.iter().map(extract).collect();
    let sees_all = viewer.sees_all();

    // --- Documentation (applies to every viewer) ---
    let mut page_ids = Vec::new();
    let mut collection_ids = Vec::new();
    for v in &views {
        match v.aggregate {
            Some(SyncAggregate::DocumentationPage) => {
                if let Some(id) = v.aggregate_id {
                    page_ids.push(id);
                }
            }
            Some(SyncAggregate::DocumentationCollection) => {
                if let Some(id) = v.aggregate_id {
                    collection_ids.push(id);
                }
            }
            _ => {}
        }
    }
    let (hidden_pages, hidden_collections, doc_fail) = if page_ids.is_empty()
        && collection_ids.is_empty()
    {
        (HashSet::new(), HashSet::new(), false)
    } else {
        match documentation::hidden_documentation_ids(
            conn,
            &page_ids,
            &collection_ids,
            &viewer.ctx.user_uuid,
            viewer.is_doc_admin,
        ) {
            Ok((hp, hc)) => (hp, hc, false),
            Err(e) => {
                tracing::error!(error = %e, "sync visibility: documentation filter failed; dropping doc rows (fail-closed)");
                (HashSet::new(), HashSet::new(), true)
            }
        }
    };

    // --- Ticket-gated families (only for restricted viewers) ---
    // Covers the ticket family plus `asset_usage` (carries a ticket id).
    // `asset_audit` needs no resolution: it's dropped purely on the
    // `visible_tickets.is_some()` (restricted) signal below.
    let needs_resolution = views
        .iter()
        .any(|v| v.aggregate.is_some_and(needs_ticket_resolution));
    let (visible_tickets, visible_comment_ids, ticket_fail) = if sees_all || !needs_resolution {
        // sees_all => None (keep all). Nothing to resolve => an empty
        // visible set; note it stays `Some` for restricted viewers so the
        // asset_usage / asset_audit arms still drop correctly.
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
                        Some(SyncAggregate::Ticket) => {
                            if let Some(id) = v.aggregate_id {
                                candidates.push(id);
                            }
                        }
                        Some(SyncAggregate::Comment)
                        | Some(SyncAggregate::TicketAsset)
                        | Some(SyncAggregate::LinkedTicket)
                        | Some(SyncAggregate::ProjectTicket)
                        | Some(SyncAggregate::CycleTicket)
                        | Some(SyncAggregate::AssetUsage) => {
                            if let Some(t) = v.ticket_id {
                                candidates.push(t);
                            }
                        }
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

    views
        .iter()
        .map(|v| {
            action_is_visible(
                v,
                visible_tickets.as_ref(),
                &visible_comment_ids,
                &hidden_pages,
                &hidden_collections,
                doc_fail,
                ticket_fail,
            )
        })
        .collect()
}

/// The ticket query the bootstrap snapshot should load: every ticket for
/// staff, only the requester/watcher set for restricted viewers. Thin
/// wrapper over [`ticket_visibility::visible_tickets_query`] keyed off
/// the viewer.
pub fn bootstrap_ticket_query<'a>(viewer: &SyncViewer) -> tickets::BoxedQuery<'a, Pg> {
    ticket_visibility::visible_tickets_query(&viewer.ctx)
}

/// Cheap, I/O-free gate: does an action with this wire aggregate name
/// need a per-viewer visibility check? Documentation always (it has
/// per-row ACLs even among staff); the ticket family only for restricted
/// viewers. The SSE path uses this to skip the async DB hop for batches
/// that don't need filtering for this viewer.
pub fn wire_aggregate_is_gated(wire: &str, viewer: &SyncViewer) -> bool {
    match wire {
        "documentation_page" | "documentation_collection" => true,
        // Ticket family + ticket-tied / staff-only inventory aggregates are
        // only gated for restricted viewers; staff (`sees_all`) see them all.
        "ticket" | "comment" | "attachment" | "ticket_asset" | "linked_ticket"
        | "project_ticket" | "cycle_ticket" | "asset_usage" | "asset_audit" => !viewer.sees_all(),
        _ => false,
    }
}

/// Fail-closed keep-mask computed with no DB access: drops every gated
/// family (documentation for all viewers; the ticket family for
/// restricted viewers) and keeps reference data. Used by the SSE path
/// when the off-thread visibility lookup can't run (e.g. pool
/// exhaustion) so a transient failure can never leak.
pub fn fail_closed_mask<T>(
    viewer: &SyncViewer,
    items: &[T],
    extract: impl Fn(&T) -> ActionView,
) -> Vec<bool> {
    let empty: HashSet<i32> = HashSet::new();
    let visible_tickets = if viewer.sees_all() {
        None
    } else {
        Some(&empty)
    };
    items
        .iter()
        .map(|it| {
            action_is_visible(
                &extract(it),
                visible_tickets,
                &empty,
                &empty,
                &empty,
                true, // doc_fail
                true, // ticket_fail
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn view(agg: SyncAggregate, is_delete: bool) -> ActionView {
        ActionView {
            aggregate: Some(agg),
            is_delete,
            aggregate_id: None,
            ticket_id: None,
            is_internal: None,
            comment_id: None,
        }
    }

    fn restricted() -> Option<HashSet<i32>> {
        // Visible tickets {1}. (A `Some` set == restricted viewer.)
        Some(HashSet::from([1]))
    }

    fn check(v: &ActionView, visible: Option<&HashSet<i32>>, comments: &HashSet<i32>) -> bool {
        action_is_visible(
            v,
            visible,
            comments,
            &HashSet::new(),
            &HashSet::new(),
            false,
            false,
        )
    }

    #[test]
    fn staff_sees_all_ticket_family() {
        // visible_tickets == None => sees_all.
        let empty = HashSet::new();
        for agg in [
            SyncAggregate::Ticket,
            SyncAggregate::Comment,
            SyncAggregate::Attachment,
            SyncAggregate::TicketAsset,
            SyncAggregate::LinkedTicket,
            SyncAggregate::ProjectTicket,
        ] {
            assert!(check(&view(agg, false), None, &empty), "{agg:?} for staff");
        }
    }

    #[test]
    fn member_ticket_inclusion() {
        let vt = restricted();
        let empty = HashSet::new();
        let mut own = view(SyncAggregate::Ticket, false);
        own.aggregate_id = Some(1);
        let mut other = view(SyncAggregate::Ticket, false);
        other.aggregate_id = Some(2);
        assert!(check(&own, vt.as_ref(), &empty), "own ticket visible");
        assert!(!check(&other, vt.as_ref(), &empty), "other ticket hidden");
    }

    #[test]
    fn member_comment_drops_internal_and_others() {
        let vt = restricted();
        let empty = HashSet::new();
        let mut public_own = view(SyncAggregate::Comment, false);
        public_own.ticket_id = Some(1);
        public_own.is_internal = Some(false);
        let mut internal_own = view(SyncAggregate::Comment, false);
        internal_own.ticket_id = Some(1);
        internal_own.is_internal = Some(true);
        let mut public_other = view(SyncAggregate::Comment, false);
        public_other.ticket_id = Some(2);
        public_other.is_internal = Some(false);
        assert!(
            check(&public_own, vt.as_ref(), &empty),
            "own public comment"
        );
        assert!(
            !check(&internal_own, vt.as_ref(), &empty),
            "own internal comment hidden"
        );
        assert!(
            !check(&public_other, vt.as_ref(), &empty),
            "other's comment hidden"
        );
    }

    #[test]
    fn member_comment_delete_kept_for_visible_ticket() {
        // comment.deleted has ticket_id but no is_internal -> kept iff ticket visible.
        let vt = restricted();
        let empty = HashSet::new();
        let mut del_own = view(SyncAggregate::Comment, true);
        del_own.ticket_id = Some(1);
        let mut del_other = view(SyncAggregate::Comment, true);
        del_other.ticket_id = Some(2);
        assert!(check(&del_own, vt.as_ref(), &empty));
        assert!(!check(&del_other, vt.as_ref(), &empty));
    }

    #[test]
    fn member_junctions_follow_ticket() {
        let vt = restricted();
        let empty = HashSet::new();
        for agg in [
            SyncAggregate::TicketAsset,
            SyncAggregate::LinkedTicket,
            SyncAggregate::ProjectTicket,
        ] {
            let mut own = view(agg, false);
            own.ticket_id = Some(1);
            let mut other = view(agg, false);
            other.ticket_id = Some(2);
            assert!(check(&own, vt.as_ref(), &empty), "{agg:?} own");
            assert!(!check(&other, vt.as_ref(), &empty), "{agg:?} other");
        }
    }

    #[test]
    fn member_attachment_created_gated_by_comment_set() {
        let vt = restricted();
        let visible_comments = HashSet::from([10]);
        let mut visible_att = view(SyncAggregate::Attachment, false);
        visible_att.comment_id = Some(10);
        let mut hidden_att = view(SyncAggregate::Attachment, false);
        hidden_att.comment_id = Some(11);
        assert!(check(&visible_att, vt.as_ref(), &visible_comments));
        assert!(!check(&hidden_att, vt.as_ref(), &visible_comments));
    }

    #[test]
    fn member_bare_id_deletes_pass_as_prune() {
        // ticket.deleted / attachment.deleted carry only `{id}` — the row
        // is gone, so they pass through as harmless prune signals (else a
        // hard-deleted ticket ghosts in the member's pool forever).
        let vt = restricted();
        let empty = HashSet::new();
        let mut ticket_del = view(SyncAggregate::Ticket, true);
        ticket_del.aggregate_id = Some(999); // not in visible set
        let att_del = view(SyncAggregate::Attachment, true);
        assert!(
            check(&ticket_del, vt.as_ref(), &empty),
            "ticket delete prunes"
        );
        assert!(
            check(&att_del, vt.as_ref(), &empty),
            "attachment delete prunes"
        );
    }

    #[test]
    fn member_cycle_ticket_follows_ticket() {
        let vt = restricted();
        let empty = HashSet::new();
        let mut own = view(SyncAggregate::CycleTicket, false);
        own.ticket_id = Some(1);
        let mut other = view(SyncAggregate::CycleTicket, false);
        other.ticket_id = Some(2);
        assert!(check(&own, vt.as_ref(), &empty), "cycle_ticket own");
        assert!(!check(&other, vt.as_ref(), &empty), "cycle_ticket other");
    }

    #[test]
    fn member_asset_usage_gated_by_ticket() {
        let vt = restricted();
        let empty = HashSet::new();
        let mut on_visible = view(SyncAggregate::AssetUsage, false);
        on_visible.ticket_id = Some(1);
        let mut on_hidden = view(SyncAggregate::AssetUsage, false);
        on_hidden.ticket_id = Some(2);
        let adhoc = view(SyncAggregate::AssetUsage, false); // ticket_id None
        assert!(
            check(&on_visible, vt.as_ref(), &empty),
            "usage on own ticket"
        );
        assert!(
            !check(&on_hidden, vt.as_ref(), &empty),
            "usage on other ticket"
        );
        assert!(!check(&adhoc, vt.as_ref(), &empty), "ad-hoc usage dropped");
        // Staff see all usage, ad-hoc included.
        assert!(check(&on_hidden, None, &empty), "staff usage");
        assert!(check(&adhoc, None, &empty), "staff ad-hoc usage");
    }

    #[test]
    fn member_asset_audit_staff_only() {
        let vt = restricted();
        let empty = HashSet::new();
        let audit = view(SyncAggregate::AssetAudit, false);
        assert!(
            !check(&audit, vt.as_ref(), &empty),
            "audit hidden from member"
        );
        assert!(check(&audit, None, &empty), "audit visible to staff");
    }

    #[test]
    fn documentation_exclusion_applies_to_everyone() {
        let hidden_pages = HashSet::from([5]);
        let no_comments = HashSet::new();
        let mut hidden = view(SyncAggregate::DocumentationPage, false);
        hidden.aggregate_id = Some(5);
        let mut visible = view(SyncAggregate::DocumentationPage, false);
        visible.aggregate_id = Some(6);
        // Even a staff viewer (visible_tickets None) is gated on docs.
        let staff_hidden = action_is_visible(
            &hidden,
            None,
            &no_comments,
            &hidden_pages,
            &HashSet::new(),
            false,
            false,
        );
        let staff_visible = action_is_visible(
            &visible,
            None,
            &no_comments,
            &hidden_pages,
            &HashSet::new(),
            false,
            false,
        );
        assert!(!staff_hidden, "hidden doc dropped even for staff");
        assert!(staff_visible, "non-hidden doc kept");
    }

    #[test]
    fn reference_data_allow_default() {
        let vt = restricted();
        let empty = HashSet::new();
        for agg in [
            SyncAggregate::User,
            SyncAggregate::Asset,
            SyncAggregate::WorkflowState,
            SyncAggregate::Project,
            SyncAggregate::Cycle,
        ] {
            assert!(
                check(&view(agg, false), vt.as_ref(), &empty),
                "{agg:?} allowed"
            );
        }
    }

    #[test]
    fn fail_closed_drops_family() {
        let vt = restricted();
        let empty = HashSet::new();
        let mut t = view(SyncAggregate::Ticket, false);
        t.aggregate_id = Some(1); // would be visible, but ticket_fail drops it
        let doc_fail = action_is_visible(
            &{
                let mut d = view(SyncAggregate::DocumentationPage, false);
                d.aggregate_id = Some(6);
                d
            },
            vt.as_ref(),
            &empty,
            &HashSet::new(),
            &HashSet::new(),
            true,
            false,
        );
        let ticket_fail = action_is_visible(
            &t,
            vt.as_ref(),
            &empty,
            &HashSet::new(),
            &HashSet::new(),
            false,
            true,
        );
        assert!(!doc_fail, "doc_fail drops doc row");
        assert!(!ticket_fail, "ticket_fail drops ticket row");
    }

    #[test]
    fn unknown_aggregate_allowed() {
        let v = ActionView {
            aggregate: None,
            is_delete: false,
            aggregate_id: None,
            ticket_id: None,
            is_internal: None,
            comment_id: None,
        };
        assert!(check(&v, restricted().as_ref(), &HashSet::new()));
        let _ = Uuid::nil();
    }
}
