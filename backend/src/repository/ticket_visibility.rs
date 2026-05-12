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
use crate::models::{Claims, UserRole};
use crate::schema::{ticket_watchers, tickets};

/// Lightweight projection of `Claims` carrying only the visibility-
/// relevant fields. Letting handlers pass a `&Claims` directly keeps
/// the call sites short.
pub struct VisibilityContext {
    pub user_uuid: Uuid,
    pub role: UserRole,
}

impl VisibilityContext {
    /// Build from JWT claims. Returns `None` when the claims carry
    /// a malformed UUID or role string — the caller maps that to
    /// `401 Unauthorized` since claims of that shape shouldn't pass
    /// the auth middleware.
    pub fn from_claims(claims: &Claims) -> Option<Self> {
        let user_uuid = Uuid::parse_str(&claims.sub).ok()?;
        let role = match claims.role.as_str() {
            "admin" => UserRole::Admin,
            "technician" => UserRole::Technician,
            "user" => UserRole::User,
            _ => return None,
        };
        Some(Self { user_uuid, role })
    }

    /// Build from the `AuthContext` extractor that most handlers
    /// already destructure out of the request. Avoids re-parsing
    /// the claims string-fields when the typed UUID + UserRole are
    /// already on hand.
    pub fn from_auth(auth: &AuthContext) -> Self {
        Self {
            user_uuid: auth.user_uuid,
            role: auth.role,
        }
    }

    /// True when the role is unrestricted (sees every ticket). v1
    /// treats both Admin and Technician this way. v1.1's restricted-
    /// technician mode would flip this for some technicians and is
    /// the natural extension point.
    fn sees_all(&self) -> bool {
        matches!(self.role, UserRole::Admin | UserRole::Technician)
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
pub fn visible_tickets_query<'a>(
    ctx: &VisibilityContext,
) -> tickets::BoxedQuery<'a, Pg> {
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
        tickets::table
            .find(ticket_id)
            .filter(
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

    fn ctx(user_uuid: Uuid, role: UserRole) -> VisibilityContext {
        VisibilityContext { user_uuid, role }
    }

    #[test]
    fn admin_sees_every_ticket() {
        let mut conn = setup_test_connection();
        let admin = TestFixtures::create_user(&mut conn, "admin", UserRole::Admin);
        let other = TestFixtures::create_user(&mut conn, "stranger", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "shh", Some(other.uuid), None);

        assert!(can_view_ticket(&mut conn, &ctx(admin.uuid, UserRole::Admin), ticket.id).unwrap());
    }

    #[test]
    fn technician_sees_every_ticket() {
        let mut conn = setup_test_connection();
        let tech = TestFixtures::create_user(&mut conn, "tech", UserRole::Technician);
        let requester = TestFixtures::create_user(&mut conn, "req", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "other", Some(requester.uuid), None);

        assert!(can_view_ticket(&mut conn, &ctx(tech.uuid, UserRole::Technician), ticket.id).unwrap());
    }

    #[test]
    fn user_sees_own_ticket_as_requester() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "alice", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "mine", Some(user.uuid), None);

        assert!(can_view_ticket(&mut conn, &ctx(user.uuid, UserRole::User), ticket.id).unwrap());
    }

    #[test]
    fn user_cannot_see_unrelated_ticket() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", UserRole::User);
        let bob = TestFixtures::create_user(&mut conn, "bob", UserRole::User);
        let bob_ticket = TestFixtures::create_ticket(&mut conn, "bob's", Some(bob.uuid), None);

        assert!(!can_view_ticket(&mut conn, &ctx(alice.uuid, UserRole::User), bob_ticket.id)
            .unwrap());
    }

    #[test]
    fn user_sees_ticket_they_watch() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", UserRole::User);
        let bob = TestFixtures::create_user(&mut conn, "bob", UserRole::User);
        let ticket = TestFixtures::create_ticket(&mut conn, "shared", Some(bob.uuid), None);
        crate::repository::ticket_watchers::add_watcher(&mut conn, ticket.id, alice.uuid, false)
            .unwrap();

        assert!(can_view_ticket(&mut conn, &ctx(alice.uuid, UserRole::User), ticket.id).unwrap());
    }

    #[test]
    fn visible_ticket_ids_filters_to_subset() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", UserRole::User);
        let bob = TestFixtures::create_user(&mut conn, "bob", UserRole::User);
        let mine = TestFixtures::create_ticket(&mut conn, "mine", Some(alice.uuid), None);
        let hers = TestFixtures::create_ticket(&mut conn, "hers", Some(bob.uuid), None);

        let visible = visible_ticket_ids(
            &mut conn,
            &ctx(alice.uuid, UserRole::User),
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
        let alice = TestFixtures::create_user(&mut conn, "alice", UserRole::User);
        let visible = visible_ticket_ids(&mut conn, &ctx(alice.uuid, UserRole::User), &[]).unwrap();
        assert!(visible.is_empty());
    }

    #[test]
    fn visible_tickets_query_filters_for_user() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "alice", UserRole::User);
        let bob = TestFixtures::create_user(&mut conn, "bob", UserRole::User);
        let mine = TestFixtures::create_ticket(&mut conn, "mine", Some(alice.uuid), None);
        let _hers = TestFixtures::create_ticket(&mut conn, "hers", Some(bob.uuid), None);
        let watched = TestFixtures::create_ticket(&mut conn, "watched", Some(bob.uuid), None);
        crate::repository::ticket_watchers::add_watcher(&mut conn, watched.id, alice.uuid, false)
            .unwrap();

        let alice_ctx = ctx(alice.uuid, UserRole::User);
        let ids: Vec<i32> = visible_tickets_query(&alice_ctx)
            .select(tickets::id)
            .load(&mut conn)
            .unwrap();
        assert!(ids.contains(&mine.id));
        assert!(ids.contains(&watched.id));
        assert!(!ids.contains(&_hers.id));
    }
}
