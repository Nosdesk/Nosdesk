//! Rollout orchestration: turn a selected group of devices into a
//! project plus one ticket per device, each ticket linked to its asset.
//! This is the planner-to-projects handoff. It lives in the service layer
//! because it spans aggregates (projects + tickets + ticket_assets);
//! every write goes through the owning repository so sync events and
//! audit context are handled exactly as a normal create would.

use diesel::result::Error as DieselError;
use diesel::Connection;

use crate::db::DbConnection;
use crate::models::{NewProject, NewTicket, ProjectStatus, TicketPriority};
use crate::repository::{
    assets as assets_repo, projects as projects_repo, tickets as tickets_repo,
};

/// What to mint. `asset_ids` are exact device ids the caller resolved
/// from the grouping dataset, already validated/deduped at the boundary.
pub struct RolloutSpec {
    pub name: String,
    pub description: Option<String>,
    pub workflow_state_id: i32,
    pub priority: TicketPriority,
    pub asset_ids: Vec<i32>,
}

pub struct RolloutResult {
    pub project_id: i32,
    pub ticket_count: usize,
}

/// Create the rollout in one transaction. A device id that no longer
/// resolves (deleted, or outside the workspace under RLS) is skipped
/// rather than failing the whole batch, so a stale selection still
/// produces a usable rollout for the devices that remain.
pub fn create_rollout(
    conn: &mut DbConnection,
    spec: RolloutSpec,
) -> Result<RolloutResult, DieselError> {
    conn.transaction(|conn| {
        let project = projects_repo::create_project(
            conn,
            NewProject {
                name: spec.name,
                description: spec.description,
                status: ProjectStatus::Active,
                start_date: None,
                end_date: None,
            },
            None,
        )?;

        let mut ticket_count = 0usize;
        for asset_id in &spec.asset_ids {
            let asset = match assets_repo::get_device_by_id(conn, *asset_id) {
                Ok(a) => a,
                Err(DieselError::NotFound) => continue,
                Err(e) => return Err(e),
            };
            let ticket = tickets_repo::create_ticket_in_project(
                conn,
                NewTicket {
                    title: asset.name.clone(),
                    workflow_state_id: spec.workflow_state_id,
                    priority: spec.priority,
                    ..Default::default()
                },
                project.id,
            )?;
            tickets_repo::add_device_to_ticket(conn, ticket.id, asset.id)?;
            ticket_count += 1;
        }

        Ok(RolloutResult {
            project_id: project.id,
            ticket_count,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::NewAsset;
    use crate::test_helpers::setup_test_connection;

    fn new_asset(name: &str) -> NewAsset {
        NewAsset {
            name: name.to_string(),
            serial_number: None,
            manufacturer: None,
            model: None,
            location: None,
            notes: None,
            primary_user_uuid: None,
            purchase_date: None,
            asset_tag: None,
            kind: "device".to_string(),
            attributes: serde_json::json!({}),
            quantity: None,
            unit: None,
            external_sync_source: None,
            low_stock_threshold: None,
        }
    }

    #[test]
    fn create_rollout_mints_project_and_one_ticket_per_device() {
        let mut conn = setup_test_connection();
        let state = crate::repository::workflow_states::default_state(&mut conn)
            .expect("workflow_states must be seeded for tests");

        let a1 = assets_repo::create_device(&mut conn, new_asset("Laptop A")).unwrap();
        let a2 = assets_repo::create_device(&mut conn, new_asset("Laptop B")).unwrap();

        let result = create_rollout(
            &mut conn,
            RolloutSpec {
                name: "Win10 refresh".to_string(),
                description: None,
                workflow_state_id: state.id,
                priority: TicketPriority::Medium,
                asset_ids: vec![a1.id, a2.id],
            },
        )
        .unwrap();

        assert_eq!(result.ticket_count, 2);

        // Each device is linked to exactly one ticket in the new project.
        use crate::schema::{project_tickets, ticket_assets};
        use diesel::prelude::*;
        let project_ticket_count: i64 = project_tickets::table
            .filter(project_tickets::project_id.eq(result.project_id))
            .count()
            .get_result(&mut conn)
            .unwrap();
        assert_eq!(project_ticket_count, 2);
        for asset_id in [a1.id, a2.id] {
            let links: i64 = ticket_assets::table
                .filter(ticket_assets::asset_id.eq(asset_id))
                .count()
                .get_result(&mut conn)
                .unwrap();
            assert_eq!(
                links, 1,
                "asset {asset_id} should link to one rollout ticket"
            );
        }
    }

    #[test]
    fn create_rollout_skips_unknown_device_ids() {
        let mut conn = setup_test_connection();
        let state = crate::repository::workflow_states::default_state(&mut conn)
            .expect("workflow_states must be seeded for tests");
        let a1 = assets_repo::create_device(&mut conn, new_asset("Only real")).unwrap();

        let result = create_rollout(
            &mut conn,
            RolloutSpec {
                name: "Partial".to_string(),
                description: None,
                workflow_state_id: state.id,
                priority: TicketPriority::None,
                asset_ids: vec![a1.id, 999_999],
            },
        )
        .unwrap();

        assert_eq!(result.ticket_count, 1);
    }
}
