//! Each workspace numbers its own tickets: the first is 1 whatever other
//! workspaces hold, and a number given explicitly (an import keeping its
//! numbers) is kept while the workspace's sequence moves past it. Numbering
//! holds no lock until the creating transaction ends, so tickets created at
//! the same time don't wait on each other, and a restore leaves each
//! workspace counting on from its restored numbers. Run as the app role
//! through a pool shaped like production's.

use diesel::prelude::*;

use backend::models::NewTicket;
use backend::repository::{tickets, workflow_states};
use backend::schema::tickets as tickets_table;
use backend::services::backup;
use backend::sync::actor::ActorContext;
use backend::sync::session::{run_in_workspace, with_actor_context};

use crate::common::{self, TestPool};

const REF: &str = "test:ticket_numbers";

/// Open a ticket in `ws` the way the app does; its number.
fn open(pool: &TestPool, ws: i32, title: &str) -> i32 {
    run_in_workspace(pool, REF, ws, |c| {
        let state = workflow_states::default_state(c)?.id;
        tickets::create_ticket(
            c,
            NewTicket {
                title: title.to_string(),
                workflow_state_id: state,
                ..Default::default()
            },
        )
    })
    .expect("open ticket")
    .number
}

#[test]
fn each_workspace_numbers_its_own_tickets() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let pool = db.runtime_pool(2);

    assert_eq!(open(&pool, a, "Printer jammed"), 1);
    assert_eq!(open(&pool, a, "Monitor flickers"), 2);
    assert_eq!(
        open(&pool, b, "Laptop won't boot"),
        1,
        "B counts on its own"
    );
    assert_eq!(open(&pool, a, "VPN drops"), 3);
}

#[test]
fn an_explicit_number_is_kept_and_counted_past() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = seeded.a.workspace_id;
    let pool = db.runtime_pool(2);

    let kept: i32 = run_in_workspace(&pool, REF, a, |c| {
        let state = workflow_states::default_state(c)?.id;
        diesel::insert_into(tickets_table::table)
            .values((
                tickets_table::title.eq("Imported"),
                tickets_table::workflow_state_id.eq(state),
                tickets_table::number.eq(41),
            ))
            .returning(tickets_table::number)
            .get_result(c)
    })
    .expect("insert with a number");
    assert_eq!(kept, 41);
    assert_eq!(open(&pool, a, "Next"), 42);
}

#[test]
fn tickets_opened_at_the_same_time_dont_wait_on_each_other() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let a = seeded.a.workspace_id;
    let pool = db.runtime_pool(3);
    let actor = ActorContext::system(REF).with_workspace(a);
    let new_ticket = |c: &mut backend::db::DbConnection, title: &str| {
        let state = workflow_states::default_state(c)?.id;
        tickets::create_ticket(
            c,
            NewTicket {
                title: title.to_string(),
                workflow_state_id: state,
                ..Default::default()
            },
        )
    };

    let mut first = pool.get().expect("conn");
    let mut second = pool.get().expect("conn");
    let (one, two) = with_actor_context(&mut first, &actor, |c| {
        let one = new_ticket(c, "Printer jammed")?.number;
        // The first transaction is still open: the second must not wait for it.
        let two = with_actor_context(&mut second, &actor, |c| {
            diesel::sql_query("SET LOCAL lock_timeout = '2s'").execute(c)?;
            new_ticket(c, "Monitor flickers")
        })?
        .number;
        Ok::<_, diesel::result::Error>((one, two))
    })
    .expect("both tickets open");
    assert_eq!((one, two), (1, 2));
}

#[test]
fn a_restore_counts_on_from_the_restored_numbers() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    common::with_upload_dir();
    let a = common::seed_two_workspaces(&mut conn).a.workspace_id;
    let pool = db.runtime_pool(2);
    open(&pool, a, "Printer jammed");
    open(&pool, a, "Monitor flickers");

    let job = common::seed_backup_job(&mut conn);
    let archive = backup::create_backup(&mut conn, job, None).expect("create_backup");
    // As on a fresh instance, which has no sequence for the workspace yet.
    diesel::sql_query(format!("DROP SEQUENCE ticket_numbers.workspace_{a}"))
        .execute(&mut conn)
        .expect("drop the workspace's sequence");
    backup::restore_database(
        &mut conn,
        &archive,
        None,
        backup::RestoreOptions {
            force_non_empty: true,
            ..Default::default()
        },
    )
    .expect("restore_database");

    assert_eq!(open(&pool, a, "VPN drops"), 3);
}
