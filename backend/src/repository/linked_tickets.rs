use diesel::prelude::*;
use diesel::QueryResult;
use tracing::debug;

use crate::db::DbConnection;
use crate::models::*;

// Linked Tickets
pub fn get_linked_tickets(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<i32>> {
    use crate::schema::linked_tickets;
    use diesel::prelude::*;

    debug!(ticket_id, "Getting linked tickets");

    // Use explicit table and column references to avoid ambiguity
    let linked_ids = linked_tickets::table
        .filter(linked_tickets::ticket_id.eq(ticket_id))
        .select(linked_tickets::linked_ticket_id)
        .load::<i32>(conn)?;

    debug!(ticket_id, count = linked_ids.len(), "Found linked tickets");

    Ok(linked_ids)
}

pub fn link_tickets(conn: &mut DbConnection, ticket1_id: i32, ticket2_id: i32) -> QueryResult<()> {
    use crate::schema::linked_tickets;

    debug!(ticket1_id, ticket2_id, "Linking tickets");

    // First, check if the tickets exist
    let ticket1 = crate::repository::tickets::get_ticket_by_id(conn, ticket1_id)?;
    let ticket2 = crate::repository::tickets::get_ticket_by_id(conn, ticket2_id)?;

    debug!(id = ticket1.id, title = %ticket1.title, "Found ticket1");
    debug!(id = ticket2.id, title = %ticket2.title, "Found ticket2");

    // Check if the links already exist
    let existing_links_1_to_2 = linked_tickets::table
        .filter(linked_tickets::ticket_id.eq(ticket1_id))
        .filter(linked_tickets::linked_ticket_id.eq(ticket2_id))
        .count()
        .get_result::<i64>(conn)?;

    let existing_links_2_to_1 = linked_tickets::table
        .filter(linked_tickets::ticket_id.eq(ticket2_id))
        .filter(linked_tickets::linked_ticket_id.eq(ticket1_id))
        .count()
        .get_result::<i64>(conn)?;

    debug!(from = ticket1_id, to = ticket2_id, count = existing_links_1_to_2, "Existing links");
    debug!(from = ticket2_id, to = ticket1_id, count = existing_links_2_to_1, "Existing links");

    // Create bidirectional links
    let new_link1 = NewLinkedTicket {
        ticket_id: ticket1.id,
        linked_ticket_id: ticket2.id,
    };

    let new_link2 = NewLinkedTicket {
        ticket_id: ticket2.id,
        linked_ticket_id: ticket1.id,
    };

    // Insert both links in a transaction
    conn.transaction(|conn| {
        let inserted_1_to_2 = diesel::insert_into(linked_tickets::table)
            .values(&new_link1)
            .on_conflict_do_nothing()
            .execute(conn)?;

        let inserted_2_to_1 = diesel::insert_into(linked_tickets::table)
            .values(&new_link2)
            .on_conflict_do_nothing()
            .execute(conn)?;

        debug!(from = ticket1_id, to = ticket2_id, inserted = inserted_1_to_2, "Inserted links");
        debug!(from = ticket2_id, to = ticket1_id, inserted = inserted_2_to_1, "Inserted links");

        Ok(())
    })
}

pub fn unlink_tickets(conn: &mut DbConnection, ticket1_id: i32, ticket2_id: i32) -> QueryResult<()> {
    use crate::schema::linked_tickets::dsl::*;

    debug!(ticket1_id, ticket2_id, "Unlinking tickets");

    // Check if the links exist before attempting to delete
    let links_from_1_to_2 = linked_tickets
        .filter(ticket_id.eq(ticket1_id))
        .filter(linked_ticket_id.eq(ticket2_id))
        .count()
        .get_result::<i64>(conn)?;

    let links_from_2_to_1 = linked_tickets
        .filter(ticket_id.eq(ticket2_id))
        .filter(linked_ticket_id.eq(ticket1_id))
        .count()
        .get_result::<i64>(conn)?;

    debug!(from = ticket1_id, to = ticket2_id, count = links_from_1_to_2, "Found links");
    debug!(from = ticket2_id, to = ticket1_id, count = links_from_2_to_1, "Found links");

    // Delete both links in a transaction
    conn.transaction(|conn| {
        // Delete link from ticket1 to ticket2
        let deleted_1_to_2 = diesel::delete(
            linked_tickets
                .filter(ticket_id.eq(ticket1_id))
                .filter(linked_ticket_id.eq(ticket2_id))
        ).execute(conn)?;

        // Delete link from ticket2 to ticket1
        let deleted_2_to_1 = diesel::delete(
            linked_tickets
                .filter(ticket_id.eq(ticket2_id))
                .filter(linked_ticket_id.eq(ticket1_id))
        ).execute(conn)?;

        debug!(from = ticket1_id, to = ticket2_id, deleted = deleted_1_to_2, "Deleted links");
        debug!(from = ticket2_id, to = ticket1_id, deleted = deleted_2_to_1, "Deleted links");

        Ok(())
    })
} 