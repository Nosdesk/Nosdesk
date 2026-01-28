use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use tracing::{debug, warn};

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;

pub fn get_projects_with_ticket_count(conn: &mut DbConnection) -> Result<Vec<ProjectWithTicketCount>, Error> {
    // Get all projects
    let all_projects = projects::table.load::<Project>(conn)?;
    
    // For each project, count the tickets
    let mut projects_with_count = Vec::new();
    
    for project in all_projects {
        let count = project_tickets::table
            .filter(project_tickets::project_id.eq(project.id))
            .count()
            .get_result::<i64>(conn)?;
        
        projects_with_count.push(ProjectWithTicketCount {
            id: project.id,
            name: project.name,
            description: project.description,
            status: project.status,
            start_date: project.start_date,
            end_date: project.end_date,
            created_at: project.created_at,
            updated_at: project.updated_at,
            ticket_count: count,
        });
    }
    
    Ok(projects_with_count)
}

pub fn get_project_with_ticket_count(conn: &mut DbConnection, project_id: i32) -> Result<ProjectWithTicketCount, Error> {
    let project = projects::table
        .find(project_id)
        .first::<Project>(conn)?;
    
    let count = project_tickets::table
        .filter(project_tickets::project_id.eq(project_id))
        .count()
        .get_result::<i64>(conn)?;
    
    Ok(ProjectWithTicketCount {
        id: project.id,
        name: project.name,
        description: project.description,
        status: project.status,
        start_date: project.start_date,
        end_date: project.end_date,
        created_at: project.created_at,
        updated_at: project.updated_at,
        ticket_count: count,
    })
}

pub fn create_project(conn: &mut DbConnection, new_project: NewProject) -> QueryResult<Project> {
    diesel::insert_into(projects::table)
        .values(&new_project)
        .get_result(conn)
}

pub fn update_project(conn: &mut DbConnection, project_id: i32, project_update: ProjectUpdate) -> QueryResult<Project> {
    // Set updated_at to current time if not provided
    let project_update = if project_update.updated_at.is_none() {
        let mut update = project_update;
        update.updated_at = Some(chrono::Utc::now().naive_utc());
        update
    } else {
        project_update
    };
    
    diesel::update(projects::table.find(project_id))
        .set(&project_update)
        .get_result(conn)
}

pub fn delete_project(conn: &mut DbConnection, project_id: i32) -> QueryResult<usize> {
    // This will also delete all project_tickets entries due to ON DELETE CASCADE
    diesel::delete(projects::table.find(project_id)).execute(conn)
}

// Project-Ticket association operations
pub fn add_ticket_to_project(conn: &mut DbConnection, project_id: i32, ticket_id: i32) -> QueryResult<ProjectTicket> {
    // First check if the ticket exists
    match crate::repository::tickets::get_ticket_by_id(conn, ticket_id) {
        Ok(_) => debug!(ticket_id, "Ticket exists"),
        Err(e) => {
            warn!(ticket_id, error = ?e, "Ticket does not exist");
            return Err(Error::NotFound);
        }
    }

    // Then check if the project exists
    match projects::table.find(project_id).first::<Project>(conn) {
        Ok(_) => debug!(project_id, "Project exists"),
        Err(e) => {
            warn!(project_id, error = ?e, "Project does not exist");
            return Err(Error::NotFound);
        }
    }

    // Check if the association already exists
    let existing = project_tickets::table
        .filter(project_tickets::project_id.eq(project_id))
        .filter(project_tickets::ticket_id.eq(ticket_id))
        .first::<ProjectTicket>(conn);

    if let Ok(association) = existing {
        debug!(project_id, ticket_id, "Association already exists");
        return Ok(association);
    }

    // Get max display_order for this project and add 1
    let max_order: Option<i32> = project_tickets::table
        .filter(project_tickets::project_id.eq(project_id))
        .select(diesel::dsl::max(project_tickets::display_order))
        .first(conn)?;

    let new_order = max_order.unwrap_or(0) + 1;

    let new_association = NewProjectTicket {
        project_id,
        ticket_id,
        display_order: new_order,
    };

    debug!(project_id, ticket_id, display_order = new_order, "Creating new project-ticket association");
    diesel::insert_into(project_tickets::table)
        .values(&new_association)
        .get_result(conn)
}

pub fn remove_ticket_from_project(conn: &mut DbConnection, project_id: i32, ticket_id: i32) -> QueryResult<usize> {
    diesel::delete(
        project_tickets::table
            .filter(project_tickets::project_id.eq(project_id))
            .filter(project_tickets::ticket_id.eq(ticket_id))
    ).execute(conn)
}

pub fn get_project_tickets(conn: &mut DbConnection, project_id: i32) -> QueryResult<Vec<TicketListItem>> {
    let raw_tickets: Vec<(Ticket, i32)> = project_tickets::table
        .filter(project_tickets::project_id.eq(project_id))
        .inner_join(tickets::table)
        .select((tickets::all_columns, project_tickets::display_order))
        .order(project_tickets::display_order.asc())
        .load::<(Ticket, i32)>(conn)?;

    // Enrich tickets with user information
    let mut ticket_list_items = Vec::new();
    for (ticket, _display_order) in raw_tickets {
        let requester_user = ticket.requester_uuid.as_ref()
            .and_then(|uuid| crate::repository::get_user_by_uuid(uuid, conn).ok())
            .map(UserInfoWithAvatar::from);

        let assignee_user = ticket.assignee_uuid.as_ref()
            .and_then(|uuid| crate::repository::get_user_by_uuid(uuid, conn).ok())
            .map(UserInfoWithAvatar::from);

        ticket_list_items.push(TicketListItem {
            ticket,
            requester_user,
            assignee_user,
        });
    }

    Ok(ticket_list_items)
}

// Get projects for a ticket
pub fn get_projects_for_ticket(conn: &mut DbConnection, ticket_id: i32) -> QueryResult<Vec<Project>> {
    debug!(ticket_id, "Getting projects for ticket");

    project_tickets::table
        .filter(project_tickets::ticket_id.eq(ticket_id))
        .inner_join(projects::table)
        .select(projects::all_columns)
        .load::<Project>(conn)
}

/// Update the display order of tickets within a project
/// Takes a list of (ticket_id, display_order) pairs
pub fn update_project_ticket_orders(
    conn: &mut DbConnection,
    project_id: i32,
    orders: Vec<(i32, i32)>,
) -> QueryResult<()> {
    debug!(project_id, count = orders.len(), "Updating project ticket orders");

    for (ticket_id, new_order) in orders {
        diesel::update(
            project_tickets::table
                .filter(project_tickets::project_id.eq(project_id))
                .filter(project_tickets::ticket_id.eq(ticket_id)),
        )
        .set(project_tickets::display_order.eq(new_order))
        .execute(conn)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::{setup_test_connection, TestFixtures};
    use crate::models::UserRole;

    #[test]
    fn create_and_get_project_with_ticket_count() {
        let mut conn = setup_test_connection();
        let project = TestFixtures::create_project(&mut conn, "Alpha");

        let fetched = get_project_with_ticket_count(&mut conn, project.id).unwrap();
        assert_eq!(fetched.name, "Alpha");
        assert_eq!(fetched.ticket_count, 0);
    }

    #[test]
    fn add_ticket_to_project_increments_count() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "projuser", UserRole::User);
        let project = TestFixtures::create_project(&mut conn, "Beta");
        let ticket = TestFixtures::create_ticket(&mut conn, "T1", Some(user.uuid), None);

        add_ticket_to_project(&mut conn, project.id, ticket.id).unwrap();

        let fetched = get_project_with_ticket_count(&mut conn, project.id).unwrap();
        assert_eq!(fetched.ticket_count, 1);
    }

    #[test]
    fn add_ticket_to_project_is_idempotent() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "idemproj", UserRole::User);
        let project = TestFixtures::create_project(&mut conn, "Gamma");
        let ticket = TestFixtures::create_ticket(&mut conn, "T1", Some(user.uuid), None);

        add_ticket_to_project(&mut conn, project.id, ticket.id).unwrap();
        add_ticket_to_project(&mut conn, project.id, ticket.id).unwrap();

        let fetched = get_project_with_ticket_count(&mut conn, project.id).unwrap();
        assert_eq!(fetched.ticket_count, 1);
    }

    #[test]
    fn remove_ticket_from_project_works() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "rmproj", UserRole::User);
        let project = TestFixtures::create_project(&mut conn, "Delta");
        let ticket = TestFixtures::create_ticket(&mut conn, "T1", Some(user.uuid), None);

        add_ticket_to_project(&mut conn, project.id, ticket.id).unwrap();
        remove_ticket_from_project(&mut conn, project.id, ticket.id).unwrap();

        let fetched = get_project_with_ticket_count(&mut conn, project.id).unwrap();
        assert_eq!(fetched.ticket_count, 0);
    }

    #[test]
    fn display_order_increments_automatically() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "orduser", UserRole::User);
        let project = TestFixtures::create_project(&mut conn, "Order");
        let t1 = TestFixtures::create_ticket(&mut conn, "T1", Some(user.uuid), None);
        let t2 = TestFixtures::create_ticket(&mut conn, "T2", Some(user.uuid), None);

        let pt1 = add_ticket_to_project(&mut conn, project.id, t1.id).unwrap();
        let pt2 = add_ticket_to_project(&mut conn, project.id, t2.id).unwrap();

        assert_eq!(pt1.display_order, 1);
        assert_eq!(pt2.display_order, 2);
    }

    #[test]
    fn get_projects_for_ticket_works() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "ptuser", UserRole::User);
        let p1 = TestFixtures::create_project(&mut conn, "P1");
        let p2 = TestFixtures::create_project(&mut conn, "P2");
        let ticket = TestFixtures::create_ticket(&mut conn, "Shared", Some(user.uuid), None);

        add_ticket_to_project(&mut conn, p1.id, ticket.id).unwrap();
        add_ticket_to_project(&mut conn, p2.id, ticket.id).unwrap();

        let projects = get_projects_for_ticket(&mut conn, ticket.id).unwrap();
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"P1"));
        assert!(names.contains(&"P2"));
    }

    #[test]
    fn delete_project_cascades_associations() {
        let mut conn = setup_test_connection();
        let user = TestFixtures::create_user(&mut conn, "delproj", UserRole::User);
        let project = TestFixtures::create_project(&mut conn, "Doomed");
        let ticket = TestFixtures::create_ticket(&mut conn, "T", Some(user.uuid), None);

        add_ticket_to_project(&mut conn, project.id, ticket.id).unwrap();
        delete_project(&mut conn, project.id).unwrap();

        // Ticket should still exist
        assert!(crate::repository::tickets::get_ticket_by_id(&mut conn, ticket.id).is_ok());
        // Project gone
        let projects = get_projects_for_ticket(&mut conn, ticket.id).unwrap();
        assert!(projects.is_empty());
    }
}