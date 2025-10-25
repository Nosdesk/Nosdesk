use diesel::prelude::*;
use diesel::QueryResult;

use crate::db::DbConnection;
use crate::models::*;

// Linked Tickets
pub fn get_linked_tickets(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<i32>> {
    use crate::schema::linked_tickets;
    use diesel::prelude::*;
    
    println!("Getting linked tickets for ticket ID: {}", ticket_id);
    
    // Use explicit table and column references to avoid ambiguity
    let linked_ids = linked_tickets::table
        .filter(linked_tickets::ticket_id.eq(ticket_id))
        .select(linked_tickets::linked_ticket_id)
        .load::<i32>(conn)?;
        
    println!("Found {} linked tickets for ticket ID {}: {:?}", linked_ids.len(), ticket_id, linked_ids);
    
    Ok(linked_ids)
}

pub fn link_tickets(conn: &mut DbConnection, ticket1_id: i32, ticket2_id: i32) -> QueryResult<()> {
    use crate::schema::linked_tickets;
    
    // Print debug information
    println!("Linking tickets: {} and {}", ticket1_id, ticket2_id);
    
    // First, check if the tickets exist
    let ticket1 = crate::repository::tickets::get_ticket_by_id(conn, ticket1_id)?;
    let ticket2 = crate::repository::tickets::get_ticket_by_id(conn, ticket2_id)?;
    
    println!("Found ticket1: {} - {}", ticket1.id, ticket1.title);
    println!("Found ticket2: {} - {}", ticket2.id, ticket2.title);
    
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
        
    println!("Found {} existing links from ticket {} to {}", existing_links_1_to_2, ticket1_id, ticket2_id);
    println!("Found {} existing links from ticket {} to {}", existing_links_2_to_1, ticket2_id, ticket1_id);
    
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
            
        println!("Inserted {} links from ticket {} to {}", inserted_1_to_2, ticket1_id, ticket2_id);
        println!("Inserted {} links from ticket {} to {}", inserted_2_to_1, ticket2_id, ticket1_id);
        
        Ok(())
    })
}

pub fn unlink_tickets(conn: &mut DbConnection, ticket1_id: i32, ticket2_id: i32) -> QueryResult<()> {
    use crate::schema::linked_tickets::dsl::*;
    
    // Print debug information
    println!("Unlinking tickets: {} and {}", ticket1_id, ticket2_id);
    
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
        
    println!("Found {} links from ticket {} to {}", links_from_1_to_2, ticket1_id, ticket2_id);
    println!("Found {} links from ticket {} to {}", links_from_2_to_1, ticket2_id, ticket1_id);
    
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
        
        println!("Deleted {} links from ticket {} to {}", deleted_1_to_2, ticket1_id, ticket2_id);
        println!("Deleted {} links from ticket {} to {}", deleted_2_to_1, ticket2_id, ticket1_id);
        
        Ok(())
    })
} 