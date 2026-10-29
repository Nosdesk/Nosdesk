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

// A restore that fails after moving the ticket counters puts them back. The
// failure is injected by wrapping `sync_ticket_number_sequences`, which runs
// as it does in a restore and then raises, so the counters have really moved.

const INJECTED: &str = "injected: the restore fails after the counters moved";

/// The next restore fails straight after moving the counters, `pause_secs`
/// later.
fn fail_after_counters_move(conn: &mut backend::db::DbConnection, pause_secs: f64) {
    use diesel::connection::SimpleConnection;
    conn.batch_execute(&format!(
        "ALTER FUNCTION public.sync_ticket_number_sequences() \
             RENAME TO sync_ticket_number_sequences_real; \
         CREATE FUNCTION public.sync_ticket_number_sequences() RETURNS void \
             LANGUAGE plpgsql AS $$ \
         BEGIN \
             PERFORM public.sync_ticket_number_sequences_real(); \
             PERFORM pg_sleep({pause_secs}); \
             RAISE EXCEPTION '{INJECTED}'; \
         END $$;"
    ))
    .expect("wrap the counter sync");
}

fn restore(
    conn: &mut backend::db::DbConnection,
    archive: &std::path::Path,
) -> Result<backup::RestoreStats, backup::BackupError> {
    backup::restore_database(
        conn,
        archive,
        None,
        backup::RestoreOptions {
            force_non_empty: true,
            ..Default::default()
        },
    )
}

/// The restore's own error, not a counter repair failure.
fn assert_injected(err: backup::BackupError) {
    assert!(
        matches!(err, backup::BackupError::DatabaseError(_)),
        "the restore's error comes back as it was: {err}"
    );
    assert!(err.to_string().contains(INJECTED), "{err}");
}

/// Delete `ws`'s ticket `number` the way the app does.
fn delete(pool: &TestPool, ws: i32, number: i32) {
    run_in_workspace(pool, REF, ws, |c| {
        let id: i32 = tickets_table::table
            .filter(tickets_table::number.eq(number))
            .select(tickets_table::id)
            .first(c)?;
        tickets::delete_ticket_with_cleanup(c, id).map(|_| ())
    })
    .expect("delete ticket");
}

/// `ws`'s ticket numbers.
fn numbers(pool: &TestPool, ws: i32) -> Vec<i32> {
    run_in_workspace(pool, REF, ws, |c| {
        tickets_table::table
            .select(tickets_table::number)
            .order(tickets_table::number)
            .load(c)
    })
    .expect("list numbers")
}

#[test]
fn a_failed_restore_puts_back_counters_its_backup_had_below_live_numbers() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    common::with_upload_dir();
    let seeded = common::seed_two_workspaces(&mut conn);
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let pool = db.runtime_pool(2);
    open(&pool, a, "Printer jammed");
    open(&pool, a, "Monitor flickers");
    let job = common::seed_backup_job(&mut conn);
    let archive = backup::create_backup(&mut conn, job, None).expect("create_backup");

    // Since the backup: A goes on to 5 and deletes 5; B, empty in the backup,
    // opens its first.
    for title in ["VPN drops", "Keyboard sticks", "Mouse lost"] {
        open(&pool, a, title);
    }
    delete(&pool, a, 5);
    assert_eq!(open(&pool, b, "Laptop won't boot"), 1);

    fail_after_counters_move(&mut conn, 0.0);
    assert_injected(restore(&mut conn, &archive).expect_err("the restore fails"));

    assert_eq!(numbers(&pool, a), [1, 2, 3, 4], "the rows rolled back");
    assert_eq!(
        open(&pool, a, "Next"),
        6,
        "counts on from what A handed out, not its backup's 2 or its live 4"
    );
    assert_eq!(
        open(&pool, b, "Next"),
        2,
        "B counts on past its live ticket"
    );
}

#[test]
fn a_failed_restore_puts_back_counters_its_backup_had_above_live_numbers() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    common::with_upload_dir();
    let seeded = common::seed_two_workspaces(&mut conn);
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let pool = db.runtime_pool(2);
    for title in [
        "Printer jammed",
        "Monitor flickers",
        "VPN drops",
        "Keyboard sticks",
    ] {
        open(&pool, a, title);
    }
    let job = common::seed_backup_job(&mut conn);
    let archive = backup::create_backup(&mut conn, job, None).expect("create_backup");

    // The live server never got past A's 2, as a server holding a backup from
    // another one: its counter says so too.
    delete(&pool, a, 4);
    delete(&pool, a, 3);
    diesel::sql_query(format!("SELECT setval('ticket_numbers.workspace_{a}', 2)"))
        .execute(&mut conn)
        .expect("set A's counter");

    fail_after_counters_move(&mut conn, 0.0);
    assert_injected(restore(&mut conn, &archive).expect_err("the restore fails"));

    assert_eq!(numbers(&pool, a), [1, 2], "the rows rolled back");
    assert_eq!(open(&pool, a, "Next"), 3, "no numbers skipped");
    assert_eq!(
        open(&pool, b, "Laptop won't boot"),
        1,
        "an empty workspace starts at 1"
    );
}

#[test]
fn a_ticket_opened_while_a_restore_fails_takes_the_next_live_number() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    common::with_upload_dir();
    let a = common::seed_two_workspaces(&mut conn).a.workspace_id;
    let pool = db.runtime_pool(2);
    open(&pool, a, "Printer jammed");
    let job = common::seed_backup_job(&mut conn);
    let archive = backup::create_backup(&mut conn, job, None).expect("create_backup");
    open(&pool, a, "Monitor flickers");
    open(&pool, a, "VPN drops");
    // Long enough for the ticket below to be waiting before the restore ends.
    fail_after_counters_move(&mut conn, 1.0);

    let mut watcher = db.conn();
    let number = std::thread::scope(|s| {
        let restoring = s.spawn(move || restore(&mut conn, &archive));
        // Wait until the restore holds tickets, then open one. pg_locks spans
        // every database, and test databases share their template's oids.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        loop {
            #[derive(QueryableByName)]
            struct Held {
                #[diesel(sql_type = diesel::sql_types::Bool)]
                held: bool,
            }
            let held: Held = diesel::sql_query(
                "SELECT EXISTS (SELECT 1 FROM pg_locks \
                 WHERE database = (SELECT oid FROM pg_database WHERE datname = current_database()) \
                   AND relation = 'public.tickets'::regclass \
                   AND mode = 'AccessExclusiveLock' AND granted) AS held",
            )
            .get_result(&mut watcher)
            .expect("read pg_locks");
            if held.held {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the restore never held tickets"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let number = open(&pool, a, "Opened during the restore");
        assert_injected(
            restoring
                .join()
                .expect("restore thread")
                .expect_err("the restore fails"),
        );
        number
    });
    assert_eq!(
        number, 4,
        "it waited for the restore, then counted on from 3"
    );
}

#[test]
fn a_completed_restore_counts_on_from_its_backup_over_newer_tickets() {
    let db = common::TestDb::new();
    let mut conn = db.conn();
    common::with_upload_dir();
    let a = common::seed_two_workspaces(&mut conn).a.workspace_id;
    let pool = db.runtime_pool(2);
    open(&pool, a, "Printer jammed");
    open(&pool, a, "Monitor flickers");
    let job = common::seed_backup_job(&mut conn);
    let archive = backup::create_backup(&mut conn, job, None).expect("create_backup");
    open(&pool, a, "VPN drops");
    open(&pool, a, "Keyboard sticks");

    restore(&mut conn, &archive).expect("restore_database");

    assert_eq!(numbers(&pool, a), [1, 2]);
    assert_eq!(open(&pool, a, "Next"), 3);
}
