//! Requester answers to "is it fixed?" (`ticket_ratings`).
//!
//! One row per ticket and rater; answering again replaces it. Each answer
//! emits `ticket.rated` so the ticket's activity shows it to the team.

use diesel::prelude::*;
use serde_json::json;
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::{SyncAggregate, SyncOp, Ticket, TicketRating};
use crate::schema::{ticket_ratings, tickets};
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

/// Whether a rating means fixed (`good`) or still needs help (`bad`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rating {
    Good,
    Bad,
}

impl Rating {
    pub fn as_str(self) -> &'static str {
        match self {
            Rating::Good => "good",
            Rating::Bad => "bad",
        }
    }
}

/// Record `rater`'s answer on `ticket_id`, replacing any earlier one.
pub fn record(
    conn: &mut DbConnection,
    ticket_id: i32,
    rater: Uuid,
    rating: Rating,
    comment: Option<&str>,
) -> QueryResult<TicketRating> {
    conn.transaction(|conn| {
        let ticket: Ticket = tickets::table.find(ticket_id).first(conn)?;
        let row: TicketRating = diesel::insert_into(ticket_ratings::table)
            .values((
                ticket_ratings::ticket_id.eq(ticket_id),
                ticket_ratings::rater_uuid.eq(rater),
                ticket_ratings::rating.eq(rating.as_str()),
                ticket_ratings::comment.eq(comment),
            ))
            .on_conflict((ticket_ratings::ticket_id, ticket_ratings::rater_uuid))
            .do_update()
            .set((
                ticket_ratings::rating.eq(rating.as_str()),
                ticket_ratings::comment.eq(comment),
                ticket_ratings::updated_at.eq(diesel::dsl::now),
            ))
            .returning(TicketRating::as_returning())
            .get_result(conn)?;
        let groups = groups::for_ticket(conn, &ticket)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Ticket,
                aggregate_id: ticket_id.to_string(),
                op: SyncOp::Update,
                event_type: "ticket.rated",
                data: json!({
                    "ticket_id": ticket_id,
                    "rater_uuid": rater,
                    "rating": row.rating,
                    "comment": row.comment,
                }),
                groups,
                causation_id: None,
            },
        )?;
        Ok(row)
    })
}

/// The requester says it's fixed: an open ticket moves to the first done
/// state, and the answer is recorded as good. `false` (nothing changed) when
/// `user` isn't the ticket's requester.
pub fn resolve_as_requester(
    conn: &mut DbConnection,
    ticket_id: i32,
    user: Uuid,
    comment: Option<&str>,
    observer: Option<&dyn crate::repository::tickets::TicketUpdatedObserver>,
) -> QueryResult<bool> {
    use crate::models::WorkflowStateCategory as Cat;
    conn.transaction(|conn| {
        let Some(ticket) = tickets::table
            .find(ticket_id)
            .filter(tickets::requester_uuid.eq(user))
            .first::<Ticket>(conn)
            .optional()?
        else {
            return Ok(false);
        };
        let category =
            crate::repository::workflow_states::category_of(conn, ticket.workflow_state_id)?;
        if !matches!(category, Some(Cat::Done | Cat::Cancelled | Cat::Merged)) {
            let done = crate::repository::workflow_states::first_in_category(conn, Cat::Done)?;
            let now = chrono::Utc::now().naive_utc();
            crate::repository::tickets::update_ticket_partial(
                conn,
                ticket_id,
                crate::models::TicketUpdate {
                    workflow_state_id: Some(done.id),
                    updated_at: Some(now),
                    ..Default::default()
                },
                observer,
            )?;
        }
        record(conn, ticket_id, user, Rating::Good, comment)?;
        Ok(true)
    })
}

/// The latest answer on a ticket, if its requester has given one.
pub fn for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Option<TicketRating>> {
    ticket_ratings::table
        .filter(ticket_ratings::ticket_id.eq(ticket_id))
        .order(ticket_ratings::updated_at.desc())
        .select(TicketRating::as_select())
        .first(conn)
        .optional()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    #[test]
    fn only_the_requester_resolves_and_rates_their_request() {
        use crate::models::WorkflowStateCategory as Cat;
        let mut conn = setup_test_connection();
        let requester = TestFixtures::create_user(&mut conn, "rate_requester", "user");
        let watcher = TestFixtures::create_user(&mut conn, "rate_watcher", "user");
        let ticket = TestFixtures::create_ticket(&mut conn, "VPN", Some(requester.uuid), None);

        assert!(!resolve_as_requester(&mut conn, ticket.id, watcher.uuid, None, None).unwrap());
        assert!(for_ticket(&mut conn, ticket.id).unwrap().is_none());

        assert!(
            resolve_as_requester(&mut conn, ticket.id, requester.uuid, Some("Thanks"), None)
                .unwrap()
        );
        let t: Ticket = tickets::table.find(ticket.id).first(&mut conn).unwrap();
        let category =
            crate::repository::workflow_states::category_of(&mut conn, t.workflow_state_id)
                .unwrap();
        assert_eq!(category, Some(Cat::Done));
        assert!(t.closed_at.is_some());
        let rating = for_ticket(&mut conn, ticket.id).unwrap().unwrap();
        assert_eq!(
            (rating.rating.as_str(), rating.comment.as_deref()),
            ("good", Some("Thanks"))
        );

        // Answering again replaces the earlier answer.
        record(&mut conn, ticket.id, requester.uuid, Rating::Bad, None).unwrap();
        let rating = for_ticket(&mut conn, ticket.id).unwrap().unwrap();
        assert_eq!((rating.rating.as_str(), rating.comment), ("bad", None));
    }
}
