//! The database keeps a row's references inside its own workspace: a key
//! between workspace tables includes `workspace_id`, so a write naming another
//! workspace's row is refused whatever connection makes it, and clearing a
//! reference leaves the row's own workspace alone.

use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error};

use backend::models::NewTicket;
use backend::repository::workflow_states;
use backend::schema::{ticket_categories, tickets};
use backend::sync::actor::ActorContext;
use backend::sync::session::{
    pin_workspace, run_in_workspace, with_actor_bypass_context, BackgroundRunError,
};

use crate::common;

const REF: &str = "test:workspace_scoped_keys";

fn ticket_in_state(state: i32) -> NewTicket {
    NewTicket {
        title: "Printer jammed".to_string(),
        workflow_state_id: state,
        ..Default::default()
    }
}

fn is_fk_violation(error: &Error) -> bool {
    matches!(
        error,
        Error::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)
    )
}

#[test]
fn a_reference_to_another_workspaces_row_is_refused() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let b_state = run_in_workspace(&pool, REF, b, workflow_states::default_state)
        .expect("B's default state")
        .id;

    let pinned = run_in_workspace(&pool, REF, a, |c| {
        diesel::insert_into(tickets::table)
            .values(&ticket_in_state(b_state))
            .execute(c)
    });
    assert!(
        matches!(&pinned, Err(BackgroundRunError::Db(e)) if is_fk_violation(e)),
        "a ticket in A can't take B's state: {pinned:?}"
    );

    // Elevated past row security (BYPASSRLS), pinned to A: still refused.
    let mut conn = pool.get().expect("conn");
    let elevated = with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        pin_workspace(c, a)?;
        diesel::insert_into(tickets::table)
            .values(&ticket_in_state(b_state))
            .execute(c)
    });
    assert!(
        matches!(&elevated, Err(e) if is_fk_violation(e)),
        "refused on an elevated connection too: {elevated:?}"
    );

    // A's own state is fine.
    run_in_workspace(&pool, REF, a, |c| {
        let own = workflow_states::default_state(c)?.id;
        diesel::insert_into(tickets::table)
            .values(&ticket_in_state(own))
            .execute(c)
    })
    .expect("a ticket in A with A's state");
}

#[test]
fn clearing_a_reference_keeps_the_rows_workspace() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let seeded = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let a = seeded.a.workspace_id;

    let ticket_id = run_in_workspace(&pool, REF, a, |c| {
        let category: i32 = diesel::insert_into(ticket_categories::table)
            .values(ticket_categories::name.eq("Hardware"))
            .returning(ticket_categories::id)
            .get_result(c)?;
        let state = workflow_states::default_state(c)?.id;
        let ticket: i32 = diesel::insert_into(tickets::table)
            .values(&NewTicket {
                category_id: Some(category),
                ..ticket_in_state(state)
            })
            .returning(tickets::id)
            .get_result(c)?;
        // `ON DELETE SET NULL (category_id)`: only the reference is cleared.
        diesel::delete(ticket_categories::table.find(category)).execute(c)?;
        Ok(ticket)
    })
    .expect("seed and delete the category");

    let (category, workspace) = run_in_workspace(&pool, REF, a, |c| {
        tickets::table
            .find(ticket_id)
            .select((tickets::category_id, tickets::workspace_id))
            .first::<(Option<i32>, i32)>(c)
    })
    .expect("reload ticket");
    assert_eq!(category, None);
    assert_eq!(workspace, a);
}
