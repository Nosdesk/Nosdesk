//! Ticket visibility primitive.
//!
//! Single source of truth for "which tickets is this user allowed
//! to read." Built to OWASP A01:2021 / 2025 (Broken Access Control)
//! and OWASP IDOR Cheatsheet guidance: scope the query at the data
//! layer, not in the handler. Both list and single-record endpoints
//! consume the same predicate so they can't drift.
//!
//! ## Visibility model (v1)
//!
//! | Role         | Sees                                                 |
//! |--------------|------------------------------------------------------|
//! | `Admin`      | All tickets                                          |
//! | `Technician` | All tickets                                          |
//! | `User`       | Tickets where they are the requester OR a watcher    |
//!
//! Matches the Zendesk / JSM default: agents see everything in
//! scope, restriction is opt-in. v1.1 will likely add a
//! "restricted technician" mode that filters by group membership;
//! the match arm in `visible_tickets_query` is the one-place hook
//! for that extension.
//!
//! ## Internal notes
//!
//! Whole-ticket visibility and internal-note visibility are
//! deliberately separate concerns. A `User` who is the requester
//! sees the ticket but never sees `is_internal` comments — that
//! filter lives at the comment layer (search filter, notification
//! gate). This module is just about "which `tickets` rows can the
//! user reach at all."
//!
//! ## 404 vs 403
//!
//! Per OWASP IDOR Cheatsheet: unauthorized read of a resource the
//! user shouldn't know exists returns `404 Not Found`. `403
//! Forbidden` would leak existence and enable ID enumeration.
//! Handlers call `can_view_ticket(...)` and map `false` to
//! `errors::not_found_msg("Ticket not found")`.

use diesel::dsl::exists;
use diesel::pg::Pg;
use diesel::prelude::*;
use diesel::select;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::extractors::AuthContext;
use crate::models::{Claims, PlatformRole, WorkspaceRole};
use crate::schema::{ticket_watchers, tickets};

/// Lightweight projection of `Claims` carrying only the visibility-
/// relevant fields. Letting handlers pass a `&Claims` directly keeps
/// the call sites short. `Copy` (two scalar fields) so the sync read
/// paths can stash it in a `SyncViewer` and move copies into blocking
/// closures without re-deriving the role split.
#[derive(Clone, Copy)]
pub struct VisibilityContext {
    pub user_uuid: Uuid,
    /// True when the user sees every ticket (staff: platform admin or
    /// workspace agent/admin/owner). False restricts them to tickets
    /// they requested or watch.
    sees_all: bool,
}

impl VisibilityContext {
    /// Build from an explicit role split. `sees_all` is true for
    /// platform admins and anyone at workspace-agent tier or higher.
    pub fn new(
        user_uuid: Uuid,
        platform_role: PlatformRole,
        workspace_role: Option<WorkspaceRole>,
    ) -> Self {
        let sees_all = platform_role.is_platform_admin()
            || workspace_role.is_some_and(|r| r.meets(WorkspaceRole::Agent));
        Self {
            user_uuid,
            sees_all,
        }
    }

    /// Build from JWT claims plus a connection. Claims carry the
    /// platform role but not the per-workspace role (the token is
    /// workspace-independent), so the workspace role is looked up in
    /// the bootstrap workspace. Returns `None` for a malformed subject
    /// UUID — the caller maps that to 401.
    ///
    /// Prefer [`from_auth`](Self::from_auth) when an `AuthContext` is
    /// already in scope: it resolved the workspace role for the
    /// request's workspace without an extra query.
    pub fn resolve(claims: &Claims, conn: &mut DbConnection) -> Option<Self> {
        let user_uuid = Uuid::parse_str(&claims.sub).ok()?;
        let platform_role = PlatformRole::from_db(&claims.platform_role);
        let workspace_role = crate::repository::user_helpers::workspace_role(conn, user_uuid);
        Some(Self::new(user_uuid, platform_role, workspace_role))
    }

    /// Visibility for the customer portal: ALWAYS requester-scoped, never
    /// sees-all, whatever the underlying user's role. The portal is an
    /// ownership-bounded surface, so a user who also happens to be an agent
    /// must still see only their own tickets there. Do not derive portal
    /// visibility from a role lookup; it must be forced.
    pub fn requester_only(user_uuid: Uuid) -> Self {
        Self {
            user_uuid,
            sees_all: false,
        }
    }

    /// Build from the `AuthContext` extractor that most handlers
    /// already destructure out of the request. Uses the workspace
    /// role AuthContext already resolved for the request's workspace.
    pub fn from_auth(auth: &AuthContext) -> Self {
        Self {
            user_uuid: auth.user_uuid,
            sees_all: auth.can_handle_tickets(),
        }
    }

    /// True when the user is unrestricted (sees every ticket). Public so
    /// the sync visibility layer can short-circuit ticket-family filtering
    /// for staff without re-deriving the role split.
    pub fn sees_all(&self) -> bool {
        self.sees_all
    }
}

/// Returns a boxed Diesel query filtered to tickets the given user
/// is allowed to read. List endpoints consume this directly so they
/// can paginate / order / filter without re-deriving the predicate.
///
/// Boxed because the predicate shape differs per role (no filter
/// for staff, two `OR`-combined clauses for end-users) and Diesel's
/// type-level branching would force every caller to use a trait
/// object anyway.
pub fn visible_tickets_query<'a>(ctx: &VisibilityContext) -> tickets::BoxedQuery<'a, Pg> {
    let base = tickets::table.into_boxed();
    if ctx.sees_all() {
        return base;
    }
    // User role: requester OR present in ticket_watchers.
    let watched_ticket_ids = ticket_watchers::table
        .filter(ticket_watchers::user_uuid.eq(ctx.user_uuid))
        .select(ticket_watchers::ticket_id);
    base.filter(
        tickets::requester_uuid
            .eq(ctx.user_uuid)
            .or(tickets::id.eq_any(watched_ticket_ids)),
    )
}

/// True when the user can read this specific ticket. Derived from
/// the same predicate as `visible_tickets_query` via a single
/// `SELECT EXISTS (...)` so list and single-record access never
/// drift apart.
///
/// Single-record handlers use this as a gate before loading detail:
///
/// ```ignore
/// let ctx = VisibilityContext::from_claims(&claims)
///     .ok_or_else(|| errors::unauthorized("Authentication required"))?;
/// if !ticket_visibility::can_view_ticket(&mut conn, &ctx, ticket_id)? {
///     return errors::not_found_msg("Ticket not found");
/// }
/// // ... load full ticket detail ...
/// ```
/// Given a candidate list of ticket ids, return the subset the
/// caller can read. Used by search and any other surface that
/// produces a pre-baked list of ticket references (search index,
/// in-memory caches) and needs to filter them post-hoc against
/// the visibility predicate.
///
/// Returns an empty set on empty input and a fast all-pass for
/// staff (still a single `IN`-filtered SELECT so callers don't
/// have to special-case role).
pub fn visible_ticket_ids(
    conn: &mut DbConnection,
    ctx: &VisibilityContext,
    candidate_ids: &[i32],
) -> QueryResult<std::collections::HashSet<i32>> {
    if candidate_ids.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let ids: Vec<i32> = visible_tickets_query(ctx)
        .filter(tickets::id.eq_any(candidate_ids))
        .select(tickets::id)
        .load(conn)?;
    Ok(ids.into_iter().collect())
}

pub fn can_view_ticket(
    conn: &mut DbConnection,
    ctx: &VisibilityContext,
    ticket_id: i32,
) -> QueryResult<bool> {
    if ctx.sees_all() {
        // Admin / Technician: visibility check collapses to "does
        // the ticket exist?" Cheap single-keyed lookup.
        return select(exists(tickets::table.find(ticket_id))).get_result(conn);
    }
    // End-user: requester OR watcher.
    let watched_ticket_ids = ticket_watchers::table
        .filter(ticket_watchers::user_uuid.eq(ctx.user_uuid))
        .select(ticket_watchers::ticket_id);
    select(exists(
        tickets::table.find(ticket_id).filter(
            tickets::requester_uuid
                .eq(ctx.user_uuid)
                .or(tickets::id.eq_any(watched_ticket_ids)),
        ),
    ))
    .get_result(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    fn ctx(user_uuid: Uuid, role: &str) -> VisibilityContext {
        let (platform_role, workspace_role) = crate::utils::parse_roles(role).unwrap();
        VisibilityContext::new(user_uuid, platform_role, Some(workspace_role))
    }

    #[test]
    fn admin_sees_every_ticket() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "admin", "admin");
        let other = TestFixtures::create_user(&mut conn, "stranger", "user");
        let ticket = TestFixtures::create_ticket(&mut conn, "shh", Some(other.uuid), None);

        assert!(can_view_ticket(&mut conn, &ctx(admin.uuid, "admin"), ticket.id).unwrap());
    }

    #[test]
    fn visibility_is_governed_by_the_pinned_workspace_not_ambient_connection() {
        // Regression for the TicketAccess unpinned-connection bug: the gate
        // ran can_view_ticket on a raw connection whose app.workspace_id was
        // whatever lingered on the pooled connection (now scrubbed on every
        // checkout by ResettingManager). Since the query carries no explicit workspace filter
        // and leans on the tickets RLS policy, an unpinned connection scoped
        // to no workspace, 404ing every ticket; a leaked one could scope to
        // the wrong tenant. Lock the precondition: even an all-seeing admin
        // resolves nothing without a pinned workspace, and re-pinning
        // restores the gate.
        use diesel::sql_types::{Nullable, Text};

        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "admin", "admin");
        let ticket = TestFixtures::create_ticket(&mut conn, "scoped", Some(admin.uuid), None);
        let vis = ctx(admin.uuid, "admin");

        // Baseline: the fixture connection is pinned to the ticket's workspace.
        assert!(can_view_ticket(&mut conn, &vis, ticket.id).unwrap());

        // Capture, then clear, the pinned workspace to mimic a freshly
        // checked-out connection that nothing has re-pinned.
        let pinned: Option<String> = diesel::select(diesel::dsl::sql::<Nullable<Text>>(
            "current_setting('app.workspace_id', true)",
        ))
        .get_result(&mut conn)
        .unwrap();
        diesel::sql_query("SELECT set_config('app.workspace_id', '', false)")
            .execute(&mut conn)
            .unwrap();
        assert!(
            !can_view_ticket(&mut conn, &vis, ticket.id).unwrap(),
            "an unpinned connection must resolve no ticket, even for an admin"
        );

        // Re-pin: visibility is restored, proving the workspace pin (not
        // ambient connection state) is what governs the gate.
        diesel::sql_query("SELECT set_config('app.workspace_id', $1, false)")
            .bind::<Text, _>(pinned.unwrap_or_default())
            .execute(&mut conn)
            .unwrap();
        assert!(can_view_ticket(&mut conn, &vis, ticket.id).unwrap());
    }

    #[test]
    fn technician_sees_every_ticket() {
        let mut conn = setup_test_connection();
        let tech = TestFixtures::create_user(&mut conn, "tech", "technician");
        let requester = TestFixtures::create_user(&mut conn, "req", "user");
        let ticket = TestFixtures::create_ticket(&mut conn, "other", Some(requester.uuid), None);

        assert!(can_view_ticket(&mut conn, &ctx(tech.uuid, "technician"), ticket.id).unwrap());
    }

    #[test]
    fn requester_only_confines_even_a_technician_to_their_own() {
        // The portal is ownership-bounded regardless of role: a user who is
        // ALSO a technician (and would otherwise see every ticket) must see
        // only their own tickets through the portal's forced-requester context.
        let mut conn = setup_test_connection();
        let tech = TestFixtures::create_user(&mut conn, "tech-portal", "technician");
        let other = TestFixtures::create_user(&mut conn, "other-cust", "user");
        let theirs = TestFixtures::create_ticket(&mut conn, "not yours", Some(other.uuid), None);
        let own = TestFixtures::create_ticket(&mut conn, "yours", Some(tech.uuid), None);

        // Role-derived context: the tech sees the other customer's ticket.
        assert!(can_view_ticket(&mut conn, &ctx(tech.uuid, "technician"), theirs.id).unwrap());

        // Portal (requester_only): forced non-sees-all, confined to own tickets.
        let portal = VisibilityContext::requester_only(tech.uuid);
        assert!(!portal.sees_all(), "portal visibility must never see all");
        assert!(
            !can_view_ticket(&mut conn, &portal, theirs.id).unwrap(),
            "portal must not see another customer's ticket"
        );
        assert!(
            can_view_ticket(&mut conn, &portal, own.id).unwrap(),
            "portal sees the user's own ticket"
        );
    }

    #[test]
    fn user_sees_own_ticket_as_requester() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "alice", "user");
        let ticket = TestFixtures::create_ticket(&mut conn, "mine", Some(user.uuid), None);

        assert!(can_view_ticket(&mut conn, &ctx(user.uuid, "user"), ticket.id).unwrap());
    }

    #[test]
    fn user_cannot_see_unrelated_ticket() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", "user");
        let bob = TestFixtures::create_user(&mut conn, "bob", "user");
        let bob_ticket = TestFixtures::create_ticket(&mut conn, "bob's", Some(bob.uuid), None);

        assert!(!can_view_ticket(&mut conn, &ctx(alice.uuid, "user"), bob_ticket.id).unwrap());
    }

    #[test]
    fn user_sees_ticket_they_watch() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", "user");
        let bob = TestFixtures::create_user(&mut conn, "bob", "user");
        let ticket = TestFixtures::create_ticket(&mut conn, "shared", Some(bob.uuid), None);
        crate::repository::ticket_watchers::add_watcher(&mut conn, ticket.id, alice.uuid, false)
            .unwrap();

        assert!(can_view_ticket(&mut conn, &ctx(alice.uuid, "user"), ticket.id).unwrap());
    }

    #[test]
    fn visible_ticket_ids_filters_to_subset() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", "user");
        let bob = TestFixtures::create_user(&mut conn, "bob", "user");
        let mine = TestFixtures::create_ticket(&mut conn, "mine", Some(alice.uuid), None);
        let hers = TestFixtures::create_ticket(&mut conn, "hers", Some(bob.uuid), None);

        let visible = visible_ticket_ids(
            &mut conn,
            &ctx(alice.uuid, "user"),
            &[mine.id, hers.id, 999_999],
        )
        .unwrap();
        assert!(visible.contains(&mine.id));
        assert!(!visible.contains(&hers.id));
        assert!(!visible.contains(&999_999));
    }

    #[test]
    fn visible_ticket_ids_empty_input_returns_empty() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", "user");
        let visible = visible_ticket_ids(&mut conn, &ctx(alice.uuid, "user"), &[]).unwrap();
        assert!(visible.is_empty());
    }

    #[test]
    fn visible_tickets_query_filters_for_user() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", "user");
        let bob = TestFixtures::create_user(&mut conn, "bob", "user");
        let mine = TestFixtures::create_ticket(&mut conn, "mine", Some(alice.uuid), None);
        let _hers = TestFixtures::create_ticket(&mut conn, "hers", Some(bob.uuid), None);
        let watched = TestFixtures::create_ticket(&mut conn, "watched", Some(bob.uuid), None);
        crate::repository::ticket_watchers::add_watcher(&mut conn, watched.id, alice.uuid, false)
            .unwrap();

        let alice_ctx = ctx(alice.uuid, "user");
        let ids: Vec<i32> = visible_tickets_query(&alice_ctx)
            .select(tickets::id)
            .load(&mut conn)
            .unwrap();
        assert!(ids.contains(&mine.id));
        assert!(ids.contains(&watched.id));
        assert!(!ids.contains(&_hers.id));
    }
}
