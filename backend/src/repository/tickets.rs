use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use uuid::Uuid;
use std::sync::Arc;
use tracing::{debug, warn};

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;
use crate::utils::storage::Storage;

// ============= Helper Functions for Enum Parsing =============

/// Parse a status string into a TicketStatus enum
fn parse_ticket_status(status: &str) -> TicketStatus {
    match status {
        "open" => TicketStatus::Open,
        "in-progress" => TicketStatus::InProgress,
        "closed" => TicketStatus::Closed,
        _ => TicketStatus::Open, // Default to open if unknown
    }
}

/// Parse a priority string into a TicketPriority enum
fn parse_ticket_priority(priority: &str) -> TicketPriority {
    match priority {
        "low" => TicketPriority::Low,
        "medium" => TicketPriority::Medium,
        "high" => TicketPriority::High,
        _ => TicketPriority::Medium, // Default to medium if unknown
    }
}

// Get all tickets
pub fn get_all_tickets(conn: &mut DbConnection) -> QueryResult<Vec<Ticket>> {
    tickets::table.load(conn)
}

pub fn get_ticket_by_id(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Ticket> {
    tickets::table
        .find(ticket_id)
        .first(conn)
}

pub fn create_ticket(conn: &mut DbConnection, new_ticket: NewTicket) -> QueryResult<Ticket> {
    diesel::insert_into(tickets::table)
        .values(&new_ticket)
        .get_result(conn)
}

/// Look up a ticket by its opaque guest-lookup token. Used by the public
/// `/api/public/tickets/{token}` status endpoint.
pub fn find_by_lookup_token(conn: &mut DbConnection, token: Uuid) -> QueryResult<Ticket> {
    tickets::table
        .filter(tickets::guest_lookup_token.eq(token))
        .first(conn)
}

/// Flip every `verification_state = 'pending'` ticket requested by the given
/// user over to `'verified'` and return the newly-released tickets.
///
/// Called by the accept-invitation flow: once the submitter has proven they
/// own the email they gave us, every guest ticket they've filed (potentially
/// more than one within the 7-day invitation window) is released into the
/// tech queue at the same time.
pub fn verify_pending_tickets_for_user(
    conn: &mut DbConnection,
    user_uuid: Uuid,
) -> QueryResult<Vec<Ticket>> {
    diesel::update(
        tickets::table
            .filter(tickets::requester_uuid.eq(Some(user_uuid)))
            .filter(tickets::verification_state.eq("pending")),
    )
    .set((
        tickets::verification_state.eq("verified"),
        tickets::updated_at.eq(chrono::Utc::now().naive_utc()),
    ))
    .get_results(conn)
}

pub fn update_ticket(conn: &mut DbConnection, ticket_id: i32, ticket: NewTicket) -> QueryResult<Ticket> {
    diesel::update(tickets::table.find(ticket_id))
        .set(&ticket)
        .get_result(conn)
}

// Add a new function for partial ticket updates
pub fn update_ticket_partial(conn: &mut DbConnection, ticket_id: i32, ticket_update: crate::models::TicketUpdate) -> QueryResult<Ticket> {
    debug!(ticket_id, update = ?ticket_update, "Updating ticket");
    
    diesel::update(tickets::table.find(ticket_id))
        .set(&ticket_update)
        .get_result(conn)
}

/// Comprehensive ticket deletion that cleans up all associated data and files
pub async fn delete_ticket_with_cleanup(
    conn: &mut DbConnection, 
    ticket_id: i32,
    storage: Arc<dyn Storage>
) -> Result<usize, Error> {
    // Start a transaction to ensure all operations succeed or fail together
    conn.transaction(|conn| {
        // 1. First, get all comments for this ticket to find attachments
        let comments = crate::repository::comments::get_comments_by_ticket_id(conn, ticket_id)?;
        
        // 2. Collect all attachment paths for file cleanup
        let mut attachment_paths = Vec::new();
        for comment in &comments {
            let attachments = crate::repository::comments::get_attachments_by_comment_id(conn, comment.id)?;
            for attachment in &attachments {
                // Extract the storage path from the URL
                if let Some(storage_path) = extract_storage_path_from_url(&attachment.url) {
                    attachment_paths.push(storage_path);
                }
                // Delete the attachment record
                diesel::delete(crate::schema::attachments::table.find(attachment.id)).execute(conn)?;
            }
        }
        
        // 3. Delete all comments for this ticket
        diesel::delete(crate::schema::comments::table.filter(crate::schema::comments::ticket_id.eq(ticket_id))).execute(conn)?;
        
        // 4. Delete linked tickets relationships
        diesel::delete(crate::schema::linked_tickets::table.filter(
            crate::schema::linked_tickets::ticket_id.eq(ticket_id)
                .or(crate::schema::linked_tickets::linked_ticket_id.eq(ticket_id))
        )).execute(conn)?;
        
        // 5. Delete ticket-device relationships
        diesel::delete(crate::schema::ticket_devices::table.filter(
            crate::schema::ticket_devices::ticket_id.eq(ticket_id)
        )).execute(conn)?;
        
        // 6. Delete ticket-project relationships
        diesel::delete(crate::schema::project_tickets::table.filter(
            crate::schema::project_tickets::ticket_id.eq(ticket_id)
        )).execute(conn)?;
        
        // 7. Delete article content
        diesel::delete(crate::schema::article_contents::table.filter(
            crate::schema::article_contents::ticket_id.eq(ticket_id)
        )).execute(conn)?;
        
        // 8. Finally, delete the ticket itself
        let result = diesel::delete(tickets::table.find(ticket_id)).execute(conn)?;
        
        // Return the attachment paths for file cleanup (outside transaction)
        Ok((result, attachment_paths))
    }).map(|(result, attachment_paths)| {
        // Clean up files after successful database transaction
        // This is done outside the transaction to avoid blocking the database
        tokio::spawn(async move {
            for path in attachment_paths {
                if let Err(e) = storage.delete_file(&path).await {
                    warn!(path, error = ?e, "Failed to delete file during ticket cleanup");
                }
            }
        });
        result
    })
}

/// Extract storage path from attachment URL
/// Converts /uploads/tickets/123/filename.ext to tickets/123/filename.ext
fn extract_storage_path_from_url(url: &str) -> Option<String> {
    url.strip_prefix("/uploads/")
        .filter(|r| r.starts_with("tickets/") || r.starts_with("temp/"))
        .map(String::from)
}

// Composite operations for tickets
pub fn get_complete_ticket(conn: &mut DbConnection, ticket_id: i32) -> Result<CompleteTicket, Error> {
    // Get the main ticket first
    let ticket = get_ticket_by_id(conn, ticket_id)?;
    debug!(id = ticket.id, title = %ticket.title, "Found ticket");
    
    // Look up complete user data for requester and assignee
    let requester_user = ticket.requester_uuid.as_ref()
        .and_then(|uuid| crate::repository::get_user_by_uuid(uuid, conn).ok())
        .map(crate::models::UserInfoWithAvatar::from);
    
    let assignee_user = ticket.assignee_uuid.as_ref()
        .and_then(|uuid| crate::repository::get_user_by_uuid(uuid, conn).ok())
        .map(UserInfoWithAvatar::from);
    
    // Get devices associated with this ticket through the junction table
    let devices = get_devices_for_ticket(conn, ticket_id).unwrap_or_default();
    
    // Get comments for this ticket
    let comments = crate::repository::comments::get_comments_by_ticket_id(conn, ticket_id)?;
    let mut comments_with_attachments = Vec::new();
    
    for comment in comments {
        let attachments = crate::repository::comments::get_attachments_by_comment_id(conn, comment.id)?;

        // Get user information for this comment with avatar
        let user = crate::repository::users::get_user_by_uuid(&comment.user_uuid, conn)
            .ok()
            .map(UserInfoWithAvatar::from);

        comments_with_attachments.push(CommentWithAttachments {
            comment,
            attachments,
            user,
        });
    }
    
    // Get article content (now handled by Yjs collaborative editing)
    let article_content: Option<String> = None;
    
    // Get linked tickets
    let linked_tickets = crate::repository::linked_tickets::get_linked_tickets(conn, ticket_id).unwrap_or_default();
    debug!(ticket_id, count = linked_tickets.len(), "Found linked tickets");
    
    // Get projects for this ticket
    let projects = crate::repository::projects::get_projects_for_ticket(conn, ticket_id).unwrap_or_default();
    debug!(ticket_id, count = projects.len(), "Found projects for ticket");
    
    Ok(CompleteTicket {
        ticket,
        requester_user,
        assignee_user,
        devices,
        comments: comments_with_attachments,
        article_content,
        linked_tickets,
        projects,
    })
}

// Import from JSON
pub fn import_ticket_from_json(conn: &mut DbConnection, ticket_json: &TicketJson) -> Result<Ticket, Error> {
    // Use helper functions for enum parsing
    let status = parse_ticket_status(&ticket_json.status);
    let priority = parse_ticket_priority(&ticket_json.priority);

    // Create the ticket
    let new_ticket = NewTicket {
        title: ticket_json.title.clone(),
        status,
        priority,
        requester_uuid: Some(Uuid::parse_str(&ticket_json.requester).unwrap_or_else(|_| Uuid::now_v7())),
        assignee_uuid: if ticket_json.assignee.is_empty() {
            None
        } else {
            Uuid::parse_str(&ticket_json.assignee).ok()
        },
        category_id: None,
        submitted_via: None,
        guest_lookup_token: None,
        verification_state: None,
        origin_channel_id: None,
    };

    let ticket = create_ticket(conn, new_ticket)?;

    // Create device if present (without ticket association)
    if let Some(device_json) = &ticket_json.device {
        let new_device = NewDevice {
            name: device_json.name.clone(),
            hostname: Some(device_json.hostname.clone()),
            device_type: None,
            serial_number: Some(device_json.serial_number.clone()),
            manufacturer: None, // Will be populated during Microsoft Entra sync
            model: Some(device_json.model.clone()),
            warranty_status: Some(device_json.warranty_status.clone()),
            location: None,
            notes: None,
            primary_user_uuid: None, // Will be populated during Microsoft Entra sync
            microsoft_device_id: None,
            intune_device_id: None,
            entra_device_id: None,
            compliance_state: None,
            last_sync_time: None,
            operating_system: None,
            os_version: None,
            is_managed: None,
            enrollment_date: None,
            warranty_start_date: None,
            warranty_end_date: None,
            purchase_date: None,
            asset_tag: None,
        };

        crate::repository::devices::create_device(conn, new_device)?;
    }

    // Create comments and attachments if present
    if let Some(comments_json) = &ticket_json.comments {
        // Default system user UUID for imported comments
        let default_user_uuid = Uuid::parse_str("00000000-0000-0000-0000-000000000001")
            .unwrap_or_else(|_| Uuid::now_v7());

        for comment_json in comments_json {
            let new_comment = NewComment {
                content: comment_json.content.clone(),
                ticket_id: ticket.id,
                user_uuid: default_user_uuid,
                channel_metadata: None,
                is_internal: false,
            };

            let comment = crate::repository::comments::create_comment(conn, new_comment)?;

            // Create attachments for this comment
            for attachment_json in &comment_json.attachments {
                let new_attachment = NewAttachment {
                    url: attachment_json.url.clone(),
                    name: attachment_json.name.clone(),
                    file_size: None,
                    mime_type: None,
                    checksum: None,
                    comment_id: Some(comment.id),
                    uploaded_by: None,
                    transcription: None,
                };

                crate::repository::comments::create_attachment(conn, new_attachment)?;
            }
        }
    }

    // Create article content if present
    if ticket_json.article_content.is_some() {
        let new_article_content = NewArticleContent {
            ticket_id: ticket.id,
            yjs_state_vector: None,
            yjs_document: None,
            yjs_client_id: None,
        };

        crate::repository::article_content::create_article_content(conn, new_article_content)?;
    }

    Ok(ticket)
}

// Ticket-Device relationship functions
pub fn add_device_to_ticket(conn: &mut DbConnection, ticket_id: i32, device_id: i32) -> QueryResult<TicketDevice> {
    let new_ticket_device = NewTicketDevice {
        ticket_id,
        device_id,
    };
    
    diesel::insert_into(ticket_devices::table)
        .values(&new_ticket_device)
        .get_result(conn)
}

pub fn remove_device_from_ticket(conn: &mut DbConnection, ticket_id: i32, device_id: i32) -> QueryResult<usize> {
    diesel::delete(
        ticket_devices::table
            .filter(ticket_devices::ticket_id.eq(ticket_id))
            .filter(ticket_devices::device_id.eq(device_id))
    ).execute(conn)
}

pub fn get_devices_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<Device>> {
    ticket_devices::table
        .inner_join(devices::table)
        .filter(ticket_devices::ticket_id.eq(ticket_id))
        .select(devices::all_columns)
        .load(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_status_known_values() {
        assert_eq!(parse_ticket_status("open"), TicketStatus::Open);
        assert_eq!(parse_ticket_status("in-progress"), TicketStatus::InProgress);
        assert_eq!(parse_ticket_status("closed"), TicketStatus::Closed);
    }

    #[test]
    fn parse_status_unknown_defaults_to_open() {
        assert_eq!(parse_ticket_status("unknown"), TicketStatus::Open);
        assert_eq!(parse_ticket_status(""), TicketStatus::Open);
    }

    #[test]
    fn parse_priority_known_values() {
        assert_eq!(parse_ticket_priority("low"), TicketPriority::Low);
        assert_eq!(parse_ticket_priority("medium"), TicketPriority::Medium);
        assert_eq!(parse_ticket_priority("high"), TicketPriority::High);
    }

    #[test]
    fn parse_priority_unknown_defaults_to_medium() {
        assert_eq!(parse_ticket_priority("critical"), TicketPriority::Medium);
        assert_eq!(parse_ticket_priority(""), TicketPriority::Medium);
    }

    #[test]
    fn extract_storage_path_tickets() {
        assert_eq!(
            extract_storage_path_from_url("/uploads/tickets/abc.pdf"),
            Some("tickets/abc.pdf".into())
        );
    }

    #[test]
    fn extract_storage_path_temp() {
        assert_eq!(
            extract_storage_path_from_url("/uploads/temp/xyz.png"),
            Some("temp/xyz.png".into())
        );
    }

    #[test]
    fn extract_storage_path_unknown_returns_none() {
        assert_eq!(extract_storage_path_from_url("/other/path.pdf"), None);
        assert_eq!(extract_storage_path_from_url("https://example.com/file"), None);
    }

    // ---- Guest-submission helpers ----

    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

    /// Insert a ticket with an explicit verification state. The shared
    /// fixture helper always passes `None`, which is the wrong shape for
    /// testing the pending-release path.
    fn insert_ticket_with_state(
        conn: &mut DbConnection,
        requester: Uuid,
        state: Option<&str>,
    ) -> Ticket {
        let new_ticket = NewTicket {
            title: "T".into(),
            status: TicketStatus::Open,
            priority: TicketPriority::Medium,
            requester_uuid: Some(requester),
            assignee_uuid: None,
            category_id: None,
            submitted_via: Some("guest".into()),
            guest_lookup_token: Some(Uuid::new_v4()),
            verification_state: state.map(|s| s.to_string()),
            origin_channel_id: None,
        };
        diesel::insert_into(tickets::table)
            .values(&new_ticket)
            .get_result(conn)
            .expect("insert ticket")
    }

    #[test]
    fn verify_pending_flips_only_requesters_pending_tickets() {
        let mut conn = setup_test_connection();
        let alice = TestFixtures::create_user(&mut conn, "Alice", UserRole::User);
        let bob = TestFixtures::create_user(&mut conn, "Bob", UserRole::User);

        // Alice has two pending + one already-verified + one nullable
        // (authenticated) ticket. Bob has one pending. Only Alice's two
        // pending tickets should flip.
        let alice_pending_1 = insert_ticket_with_state(&mut conn, alice.uuid, Some("pending"));
        let alice_pending_2 = insert_ticket_with_state(&mut conn, alice.uuid, Some("pending"));
        let alice_verified = insert_ticket_with_state(&mut conn, alice.uuid, Some("verified"));
        let alice_null = insert_ticket_with_state(&mut conn, alice.uuid, None);
        let bob_pending = insert_ticket_with_state(&mut conn, bob.uuid, Some("pending"));

        let released =
            verify_pending_tickets_for_user(&mut conn, alice.uuid).expect("verify returns");

        let released_ids: Vec<i32> = released.iter().map(|t| t.id).collect();
        assert_eq!(released_ids.len(), 2);
        assert!(released_ids.contains(&alice_pending_1.id));
        assert!(released_ids.contains(&alice_pending_2.id));

        // Already-verified, null, and Bob's pending should be untouched.
        let mut fetch = |id| get_ticket_by_id(&mut conn, id).unwrap();
        assert_eq!(fetch(alice_verified.id).verification_state.as_deref(), Some("verified"));
        assert_eq!(fetch(alice_null.id).verification_state, None);
        assert_eq!(fetch(bob_pending.id).verification_state.as_deref(), Some("pending"));
    }

    #[test]
    fn verify_pending_noop_when_user_has_no_pending() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "NoPending", UserRole::User);
        let released = verify_pending_tickets_for_user(&mut conn, user.uuid).unwrap();
        assert!(released.is_empty());
    }

    #[test]
    fn find_by_lookup_token_returns_match() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "Lookup", UserRole::User);
        let ticket = insert_ticket_with_state(&mut conn, user.uuid, Some("verified"));
        let token = ticket.guest_lookup_token.expect("fixture sets a token");

        let found = find_by_lookup_token(&mut conn, token).expect("found");
        assert_eq!(found.id, ticket.id);
    }

    #[test]
    fn find_by_lookup_token_not_found_for_random_uuid() {
        let mut conn = setup_test_connection();
        let result = find_by_lookup_token(&mut conn, Uuid::new_v4());
        assert!(matches!(result, Err(diesel::result::Error::NotFound)));
    }
}

