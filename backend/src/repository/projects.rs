use std::collections::HashMap;

use diesel::dsl::count;
use diesel::prelude::*;
use diesel::result::Error;
use diesel::QueryResult;
use serde_json::json;
use tracing::{debug, warn};
use uuid::Uuid;

use crate::db::DbConnection;
use crate::models::*;
use crate::schema::*;
use crate::sync::emit::{self, SyncEmit};
use crate::sync::groups;

pub fn get_projects_with_ticket_count(
    conn: &mut DbConnection,
) -> Result<Vec<ProjectWithTicketCount>, Error> {
    // One pass over project_tickets to get every (project_id, count)
    // pair, then one pass over projects. Replaces an N+1 that fired
    // a COUNT() per project on every list-page render.
    let counts: Vec<(i32, i64)> = project_tickets::table
        .group_by(project_tickets::project_id)
        .select((
            project_tickets::project_id,
            count(project_tickets::ticket_id),
        ))
        .load(conn)?;
    let count_map: HashMap<i32, i64> = counts.into_iter().collect();

    let all_projects = projects::table.load::<Project>(conn)?;

    Ok(all_projects
        .into_iter()
        .map(|project| {
            let ticket_count = count_map.get(&project.id).copied().unwrap_or(0);
            ProjectWithTicketCount {
                id: project.id,
                name: project.name,
                description: project.description,
                status: project.status,
                start_date: project.start_date,
                end_date: project.end_date,
                created_at: project.created_at,
                updated_at: project.updated_at,
                ticket_count,
                tickets: None,
            }
        })
        .collect())
}

pub fn get_project_with_ticket_count(
    conn: &mut DbConnection,
    project_id: i32,
) -> Result<ProjectWithTicketCount, Error> {
    let project = projects::table.find(project_id).first::<Project>(conn)?;

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
        tickets: None,
    })
}

/// Observer fired after a project is created or updated. The
/// implementor re-indexes the project in search so name / description
/// / status edits surface in global search regardless of which handler
/// made the change.
pub trait ProjectIndexedObserver: Send + Sync {
    fn project_indexed(&self, project: &Project);
}

/// Observer fired after a project is deleted. The implementor removes
/// the project from the search index so it stops appearing in results.
pub trait ProjectDeletedObserver: Send + Sync {
    fn project_deleted(&self, project_id: i32);
}

pub fn create_project(
    conn: &mut DbConnection,
    new_project: NewProject,
    observer: Option<&dyn ProjectIndexedObserver>,
) -> QueryResult<Project> {
    let project = conn.transaction::<_, diesel::result::Error, _>(|conn| {
        let project: Project = diesel::insert_into(projects::table)
            .values(&new_project)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Project,
                aggregate_id: project.id.to_string(),
                op: SyncOp::Insert,
                event_type: "project.created",
                data: json!({
                    "id": project.id,
                    "name": project.name,
                    "description": project.description,
                    "status": project.status,
                }),
                groups: groups::for_project(project.id),
                causation_id: None,
            },
        )?;
        Ok(project)
    })?;
    if let Some(obs) = observer {
        obs.project_indexed(&project);
    }
    Ok(project)
}

pub fn update_project(
    conn: &mut DbConnection,
    project_id: i32,
    project_update: ProjectUpdate,
    observer: Option<&dyn ProjectIndexedObserver>,
) -> QueryResult<Project> {
    // Set updated_at to current time if not provided
    let project_update = if project_update.updated_at.is_none() {
        let mut update = project_update;
        update.updated_at = Some(chrono::Utc::now().naive_utc());
        update
    } else {
        project_update
    };

    let project = conn.transaction::<_, diesel::result::Error, _>(|conn| {
        let project: Project = diesel::update(projects::table.find(project_id))
            .set(&project_update)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::Project,
                aggregate_id: project.id.to_string(),
                op: SyncOp::Update,
                event_type: "project.updated",
                data: json!({
                    "id": project.id,
                    "name": project.name,
                    "description": project.description,
                    "status": project.status,
                }),
                groups: groups::for_project(project.id),
                causation_id: None,
            },
        )?;
        Ok(project)
    })?;
    if let Some(obs) = observer {
        obs.project_indexed(&project);
    }
    Ok(project)
}

pub fn delete_project(
    conn: &mut DbConnection,
    project_id: i32,
    observer: Option<&dyn ProjectDeletedObserver>,
) -> QueryResult<usize> {
    // This will also delete all project_tickets entries due to ON DELETE CASCADE.
    // Capture the project before delete so the emit fans out to the
    // right groups (the project_tickets cascade will remove ticket
    // associations, but the deleted project itself still belongs to
    // workspace + project:<id>).
    let result = conn.transaction::<_, diesel::result::Error, _>(|conn| {
        let result = diesel::delete(projects::table.find(project_id)).execute(conn)?;
        if result > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::Project,
                    aggregate_id: project_id.to_string(),
                    op: SyncOp::Delete,
                    event_type: "project.deleted",
                    data: json!({ "id": project_id }),
                    groups: groups::for_project(project_id),
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })?;
    if result > 0 {
        if let Some(obs) = observer {
            obs.project_deleted(project_id);
        }
    }
    Ok(result)
}

// Project-Ticket association operations
pub fn add_ticket_to_project(
    conn: &mut DbConnection,
    project_id: i32,
    ticket_id: i32,
) -> QueryResult<ProjectTicket> {
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

    debug!(
        project_id,
        ticket_id,
        display_order = new_order,
        "Creating new project-ticket association"
    );
    conn.transaction(|conn| {
        let association: ProjectTicket = diesel::insert_into(project_tickets::table)
            .values(&new_association)
            .get_result(conn)?;
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::ProjectTicket,
                aggregate_id: format!("{}:{}", project_id, ticket_id),
                op: SyncOp::Insert,
                event_type: "project_ticket.added",
                data: json!({
                    "project_id": project_id,
                    "ticket_id": ticket_id,
                    "display_order": association.display_order,
                }),
                // Both the project and the ticket get the event so a
                // sync client watching either fan-out sees the
                // association land.
                groups: {
                    let mut g = groups::for_project(project_id);
                    g.push(format!("ticket:{}", ticket_id));
                    g
                },
                causation_id: None,
            },
        )?;
        Ok(association)
    })
}

pub fn remove_ticket_from_project(
    conn: &mut DbConnection,
    project_id: i32,
    ticket_id: i32,
) -> QueryResult<usize> {
    conn.transaction(|conn| {
        let result = diesel::delete(
            project_tickets::table
                .filter(project_tickets::project_id.eq(project_id))
                .filter(project_tickets::ticket_id.eq(ticket_id)),
        )
        .execute(conn)?;
        if result > 0 {
            emit::record(
                conn,
                SyncEmit {
                    aggregate: SyncAggregate::ProjectTicket,
                    aggregate_id: format!("{}:{}", project_id, ticket_id),
                    op: SyncOp::Delete,
                    event_type: "project_ticket.removed",
                    data: json!({ "project_id": project_id, "ticket_id": ticket_id }),
                    groups: {
                        let mut g = groups::for_project(project_id);
                        g.push(format!("ticket:{}", ticket_id));
                        g
                    },
                    causation_id: None,
                },
            )?;
        }
        Ok(result)
    })
}

pub fn get_project_tickets(
    conn: &mut DbConnection,
    project_id: i32,
) -> QueryResult<Vec<TicketListItem>> {
    let raw_tickets: Vec<(Ticket, i32)> = project_tickets::table
        .filter(project_tickets::project_id.eq(project_id))
        .inner_join(tickets::table)
        .select((tickets::all_columns, project_tickets::display_order))
        .order(project_tickets::display_order.asc())
        .load::<(Ticket, i32)>(conn)?;

    // Collect every UUID we'll need to enrich, dedupe, then issue
    // one SELECT instead of two per ticket. Previously a 1000-ticket
    // project fired 2000+ user lookups serially.
    let mut needed: std::collections::HashSet<Uuid> = std::collections::HashSet::new();
    for (ticket, _) in &raw_tickets {
        if let Some(u) = ticket.requester_uuid {
            needed.insert(u);
        }
        if let Some(u) = ticket.assignee_uuid {
            needed.insert(u);
        }
    }

    let user_map: HashMap<Uuid, UserInfoWithAvatar> = if needed.is_empty() {
        HashMap::new()
    } else {
        let uuids: Vec<Uuid> = needed.into_iter().collect();
        users::table
            .filter(users::uuid.eq_any(&uuids))
            .load::<User>(conn)?
            .into_iter()
            .map(|u| (u.uuid, UserInfoWithAvatar::from(u)))
            .collect()
    };

    Ok(raw_tickets
        .into_iter()
        .map(|(ticket, _display_order)| {
            let requester_user = ticket
                .requester_uuid
                .and_then(|u| user_map.get(&u).cloned());
            let assignee_user = ticket.assignee_uuid.and_then(|u| user_map.get(&u).cloned());
            TicketListItem {
                ticket,
                requester_user,
                assignee_user,
            }
        })
        .collect())
}

// Get projects for a ticket
pub fn get_projects_for_ticket(
    conn: &mut DbConnection,
    ticket_id: i32,
) -> QueryResult<Vec<Project>> {
    debug!(ticket_id, "Getting projects for ticket");

    project_tickets::table
        .filter(project_tickets::ticket_id.eq(ticket_id))
        .inner_join(projects::table)
        .select(projects::all_columns)
        .load::<Project>(conn)
}

/// Update the display order of tickets within a project.
/// Takes a list of (ticket_id, display_order) pairs.
///
/// Emits a single project_ticket.reordered event with the full new
/// order in `data`, rather than one row per association — kanban /
/// drag-drop interactions can shuffle dozens of tickets at once and
/// per-row events would create a lot of noise on the sync bus.
pub fn update_project_ticket_orders(
    conn: &mut DbConnection,
    project_id: i32,
    orders: Vec<(i32, i32)>,
) -> QueryResult<()> {
    debug!(
        project_id,
        count = orders.len(),
        "Updating project ticket orders"
    );

    conn.transaction(|conn| {
        for (ticket_id, new_order) in &orders {
            diesel::update(
                project_tickets::table
                    .filter(project_tickets::project_id.eq(project_id))
                    .filter(project_tickets::ticket_id.eq(*ticket_id)),
            )
            .set(project_tickets::display_order.eq(*new_order))
            .execute(conn)?;
        }
        let order_payload: Vec<serde_json::Value> = orders
            .iter()
            .map(|(t, o)| json!({ "ticket_id": t, "display_order": o }))
            .collect();
        emit::record(
            conn,
            SyncEmit {
                aggregate: SyncAggregate::ProjectTicket,
                aggregate_id: project_id.to_string(),
                op: SyncOp::Update,
                event_type: "project_ticket.reordered",
                data: json!({ "project_id": project_id, "orders": order_payload }),
                groups: groups::for_project(project_id),
                causation_id: None,
            },
        )?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::UserRole;
    use crate::test_helpers::{setup_test_connection, TestFixtures};

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
        delete_project(&mut conn, project.id, None).unwrap();

        // Ticket should still exist
        assert!(crate::repository::tickets::get_ticket_by_id(&mut conn, ticket.id).is_ok());
        // Project gone
        let projects = get_projects_for_ticket(&mut conn, ticket.id).unwrap();
        assert!(projects.is_empty());
    }
}
