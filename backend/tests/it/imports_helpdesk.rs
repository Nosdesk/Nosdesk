//! The helpdesk import's loader, end to end against a real database: a bundle
//! (as a Zendesk connector would write it) brought into a workspace.
//!
//! What it must get right, because a switcher judges it on these:
//! - history keeps its dates, numbers, internal notes and authors;
//! - nothing is sent: no notification, rule or webhook outbox row for any
//!   imported event, and no SLA on imported tickets;
//! - running it again adds nothing, and picks up only new replies.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Integer, Nullable, Text, Timestamptz};
use uuid::Uuid;

use backend::services::imports::helpdesk::bundle::{
    Attachment, Body, BodyFormat, Bundle, Comment, Manifest, Person, PersonRole, PriorityLevel,
    StatusKind, Ticket, FORMAT_VERSION,
};
use backend::services::imports::helpdesk::loader::{self, LoadOptions, NumberPolicy};
use backend::sync::session::run_in_workspace;

use crate::common::{TestDb, TestPool, WorkspaceSeed};

const REF: &str = "test:imports_helpdesk";

fn at(day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2025, 3, day, hour, 0, 0)
        .single()
        .expect("date")
}

fn html(s: &str) -> Body {
    Body {
        format: BodyFormat::Html,
        value: s.into(),
    }
}

fn comment(
    id: &str,
    author: Option<&str>,
    public: bool,
    body: &str,
    when: DateTime<Utc>,
) -> Comment {
    Comment {
        source_id: id.into(),
        author: author.map(str::to_string),
        public,
        body: html(body),
        created_at: when,
        attachments: vec![],
    }
}

fn people() -> Vec<Person> {
    let p = |id: &str, email: Option<&str>, name: &str, role| Person {
        source_id: id.into(),
        email: email.map(str::to_string),
        name: name.into(),
        role,
        active: true,
        organization: None,
        created_at: None,
        updated_at: None,
    };
    vec![
        p(
            "u-req",
            Some("requester@customer.test"),
            "Robin Requester",
            PersonRole::Requester,
        ),
        p(
            "u-agent",
            Some("agent@acme.test"),
            "Alex Agent",
            PersonRole::Agent,
        ),
        p("u-phone", None, "Phone Only", PersonRole::Requester),
        p(
            "u-unused",
            Some("never@customer.test"),
            "Never Raised Anything",
            PersonRole::Requester,
        ),
    ]
}

fn tickets() -> Vec<Ticket> {
    let mut solved = Ticket {
        source_id: "101".into(),
        number: Some(101),
        subject: "VPN drops every hour".into(),
        status: StatusKind::Solved,
        status_label: "solved".into(),
        priority: Some(PriorityLevel::High),
        ticket_type: Some("incident".into()),
        tags: vec!["vpn".into(), "network".into()],
        requester: Some("u-req".into()),
        assignee: Some("u-agent".into()),
        watchers: vec![],
        created_at: at(1, 9),
        updated_at: at(3, 17),
        solved_at: Some(at(3, 16)),
        fields: BTreeMap::from([("Region".to_string(), "APAC".to_string())]),
        comments: vec![
            comment(
                "c1",
                Some("u-req"),
                true,
                "<p>The VPN drops every hour.</p>",
                at(1, 9),
            ),
            comment(
                "c2",
                Some("u-agent"),
                false,
                "<p>Probably the DHCP lease.</p>",
                at(1, 10),
            ),
            comment(
                "c3",
                Some("u-agent"),
                true,
                "<p>Fixed: lease extended.</p>",
                at(3, 16),
            ),
        ],
    };
    solved.comments[2].attachments.push(Attachment {
        source_id: "a1".into(),
        file_name: "lease.txt".into(),
        content_type: Some("text/plain".into()),
        size: Some(12),
        stored: true,
        missing_reason: None,
        source_url: None,
        retry: false,
    });
    solved.comments[2].attachments.push(Attachment {
        source_id: "a2".into(),
        file_name: "gone.png".into(),
        content_type: Some("image/png".into()),
        size: None,
        stored: false,
        missing_reason: Some("off-host link refused".into()),
        source_url: None,
        retry: false,
    });
    let open = Ticket {
        source_id: "102".into(),
        number: Some(102),
        subject: "New laptop".into(),
        status: StatusKind::Open,
        status_label: "open".into(),
        priority: Some(PriorityLevel::Normal),
        ticket_type: None,
        tags: vec![],
        requester: Some("u-phone".into()),
        assignee: Some("u-agent".into()),
        watchers: vec!["u-req".into()],
        created_at: at(5, 8),
        updated_at: at(5, 9),
        solved_at: None,
        fields: BTreeMap::new(),
        comments: vec![
            comment(
                "c4",
                Some("u-phone"),
                true,
                "<p>Need a laptop.</p>",
                at(5, 8),
            ),
            comment(
                "c5",
                Some("u-ghost"),
                true,
                "<p>From someone the export lost.</p>",
                at(5, 9),
            ),
        ],
    };
    vec![solved, open]
}

fn write_bundle(dir: &std::path::Path, tickets: &[Ticket]) -> Bundle {
    write_bundle_of(dir, &people(), tickets)
}

fn write_bundle_of(dir: &std::path::Path, people: &[Person], tickets: &[Ticket]) -> Bundle {
    let b = Bundle::create(dir).expect("bundle");
    let mut w = b.people_writer(true).expect("people");
    for p in people {
        w.write(p).expect("person");
    }
    w.checkpoint().expect("flush");
    let mut w = b.tickets_writer(0).expect("tickets");
    for t in tickets {
        w.write(t).expect("ticket");
    }
    w.checkpoint().expect("flush");
    std::fs::write(b.file_path("a1"), b"lease: 24h\n").expect("file");
    b.write_manifest(&Manifest {
        format_version: FORMAT_VERSION,
        source: "zendesk".into(),
        source_scope: "acme".into(),
        extracted_at: Utc::now(),
        counts: BTreeMap::from([("tickets".to_string(), tickets.len() as u64)]),
        notes: vec![],
    })
    .expect("manifest");
    b
}

fn storage() -> Arc<dyn backend::utils::storage::Storage> {
    let dir = tempfile::tempdir().expect("storage").keep();
    Arc::new(backend::utils::storage::LocalStorage::new(
        dir.to_string_lossy().into_owned(),
        "/files".into(),
    ))
}

fn options(ws: &WorkspaceSeed, job: Uuid) -> LoadOptions {
    LoadOptions {
        job_id: job,
        workspace_id: ws.workspace_id,
        actor: ws.admin_uuid,
        numbers: NumberPolicy::Auto,
        agent_map: HashMap::new(),
        create_agents: true,
        batch_size: 1,
    }
}

#[derive(QueryableByName, Debug)]
struct TicketRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Integer)]
    number: i32,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Timestamptz)]
    created_at: DateTime<Utc>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    closed_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Text)]
    sla_override: String,
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    assignee_uuid: Option<Uuid>,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    sla_response_target_at: Option<DateTime<Utc>>,
}

fn imported_tickets(pool: &TestPool, ws: &WorkspaceSeed) -> Vec<TicketRow> {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT id, number, title, created_at, closed_at, sla_override, assignee_uuid, \
             sla_response_target_at FROM tickets WHERE submitted_via = 'import' ORDER BY number",
        )
        .load::<TicketRow>(c)
    })
    .expect("tickets")
}

#[derive(QueryableByName, Debug)]
struct CommentRow {
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = Bool)]
    is_internal: bool,
    #[diesel(sql_type = Timestamptz)]
    created_at: DateTime<Utc>,
    #[diesel(sql_type = Text)]
    author: String,
}

fn comments_of(pool: &TestPool, ws: &WorkspaceSeed, ticket_id: i32) -> Vec<CommentRow> {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT c.content, c.is_internal, c.created_at, u.name AS author \
             FROM comments c JOIN users u ON u.uuid = c.user_uuid \
             WHERE c.ticket_id = $1 ORDER BY c.created_at, c.id",
        )
        .bind::<Integer, _>(ticket_id)
        .load::<CommentRow>(c)
    })
    .expect("comments")
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Count with a raw admin connection: the outboxes aren't tenant tables.
fn count(pool: &TestPool, sql: &str, job: Uuid) -> i64 {
    let mut conn = pool.get().expect("conn");
    diesel::sql_query(sql)
        .bind::<Text, _>(format!("import:{job}"))
        .get_result::<Count>(&mut conn)
        .expect("count")
        .n
}

async fn run(pool: &TestPool, bundle: &Bundle, opts: &LoadOptions) -> loader::LoadReport {
    let store = storage();
    loader::load(pool, store, bundle, opts, &|_| {})
        .await
        .expect("load")
}

#[actix_web::test]
async fn a_zendesk_bundle_lands_as_quiet_history_and_reruns_add_nothing() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    // Live automation, so "fires no rule" is tested against rules that
    // would fire: one on every new ticket, one an hour after creation.
    {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query(
            "INSERT INTO rules (workspace_id, name, trigger_kind, trigger_config, actions, state) VALUES \
             ($1, 'On create', 'ticket_created', '{}'::jsonb, \
              '[{\"kind\":\"set_priority\",\"config\":{\"priority\":\"high\"}}]'::jsonb, 'live'), \
             ($1, 'After an hour', 'time_elapsed', '{\"clock_name\":\"since_created\",\"minutes\":60}'::jsonb, \
              '[{\"kind\":\"set_priority\",\"config\":{\"priority\":\"high\"}}]'::jsonb, 'live')",
        )
        .bind::<Integer, _>(ws.workspace_id)
        .execute(&mut conn)
        .expect("live rules");
    }

    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());
    let job = Uuid::now_v7();
    let opts = options(&ws, job);

    // The dry run writes nothing and says what will happen.
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.tickets, 2);
    assert_eq!(plan.comments, 5);
    assert_eq!(plan.attachments_missing, 1);
    assert!(
        imported_tickets(&pool, &ws).is_empty(),
        "a plan writes nothing"
    );
    // Only people the tickets mention: not u-unused.
    assert_eq!(plan.people.values().sum::<u64>(), 3, "{:?}", plan.people);

    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 2);
    assert_eq!(report.comments_written, 5);
    assert_eq!(report.attachments_written, 1);
    assert_eq!(
        report.attachment_problems.len(),
        1,
        "{:?}",
        report.attachment_problems
    );

    // History keeps its dates and, in a workspace with no tickets of its
    // own, its numbers. No SLA.
    let rows = imported_tickets(&pool, &ws);
    assert_eq!(rows.len(), 2);
    let solved = &rows[0];
    assert_eq!(solved.title, "VPN drops every hour");
    assert_eq!(solved.created_at, at(1, 9));
    assert_eq!(solved.closed_at, Some(at(3, 16)));
    assert_eq!(solved.sla_override, "none");
    assert_eq!(solved.sla_response_target_at, None);
    assert!(plan.keep_numbers, "{}", plan.numbers_reason);
    assert_eq!((rows[0].number, rows[1].number), (101, 102));
    // The source agent was created as an agent here, so they're assignable.
    assert!(solved.assignee_uuid.is_some());

    // The conversation: the import note first, then each reply with its own
    // date, author and visibility.
    let thread = comments_of(&pool, &ws, solved.id);
    assert_eq!(thread.len(), 4, "{thread:?}");
    assert!(thread[0].is_internal && thread[0].content.contains("Imported from Zendesk #101"));
    assert!(thread[0].content.contains("Region: APAC"));
    assert_eq!(thread[1].author, "Robin Requester");
    assert!(!thread[1].is_internal);
    assert_eq!(thread[1].created_at, at(1, 9));
    assert!(
        thread[2].is_internal,
        "a Zendesk private comment stays an internal note"
    );
    assert_eq!(thread[2].author, "Alex Agent");

    // A reply whose author the export lost is credited to the placeholder.
    let open = &rows[1];
    let thread = comments_of(&pool, &ws, open.id);
    assert!(
        thread.iter().any(|c| c.author == "Unknown (Zendesk)"),
        "{thread:?}"
    );

    // The run is the import's own, not the operator's: a system actor under
    // the job's reference, and a job row that names who ran it.
    assert_eq!(
        count(
            &pool,
            "SELECT count(*) AS n FROM sync_actions WHERE actor_ref = $1 AND actor_kind <> 'system'",
            job,
        ),
        0
    );
    {
        #[derive(QueryableByName)]
        struct Job {
            #[diesel(sql_type = Text)]
            job_type: String,
            #[diesel(sql_type = Text)]
            status: String,
            #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
            created_by: Option<Uuid>,
            #[diesel(sql_type = Nullable<Integer>)]
            records_committed: Option<i32>,
            #[diesel(sql_type = Integer)]
            workspace_id: i32,
        }
        let row: Job = diesel::sql_query(
            "SELECT job_type, status, created_by, records_committed, workspace_id \
             FROM import_jobs WHERE id = $1",
        )
        .bind::<diesel::sql_types::Uuid, _>(job)
        .get_result(&mut pool.get().expect("conn"))
        .expect("the run's job row");
        assert_eq!(
            (row.job_type.as_str(), row.status.as_str()),
            ("helpdesk", "done")
        );
        assert_eq!(row.created_by, Some(ws.admin_uuid));
        assert_eq!(row.records_committed, Some(2));
        assert_eq!(row.workspace_id, ws.workspace_id);
    }

    // Quiet: events were recorded (clients and the feed see the rows), but
    // none reached the notification, rule or webhook outboxes.
    let events = count(
        &pool,
        "SELECT count(*) AS n FROM sync_actions WHERE actor_ref = $1",
        job,
    );
    assert!(events > 0);
    for outbox in ["notification_outbox", "webhook_outbox", "rule_event_outbox"] {
        let n = count(
            &pool,
            &format!(
                "SELECT count(*) AS n FROM {outbox} o JOIN sync_actions s ON s.sync_id = o.sync_id \
                 WHERE s.actor_ref = $1"
            ),
            job,
        );
        assert_eq!(n, 0, "{outbox} got {n} rows from the import");
    }

    // The elapsed-rule sweep doesn't start clocks on imported history,
    // however old and open it is.
    backend::services::rules::elapsed::reconcile(&pool).expect("reconcile");
    let clocks = |ticket: i32| -> i64 {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query("SELECT count(*) AS n FROM ticket_elapsed_queue WHERE ticket_id = $1")
            .bind::<Integer, _>(ticket)
            .get_result::<Count>(&mut conn)
            .expect("clocks")
            .n
    };
    assert_eq!(
        clocks(rows[1].id),
        0,
        "an imported open ticket runs no rule clock"
    );

    // Control: an ordinary ticket written after the import reaches every
    // outbox and gets a clock, so the zeros above are the marker's doing.
    let own = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        backend::repository::tickets::create_ticket(
            c,
            backend::models::NewTicket {
                title: "Ordinary".into(),
                workflow_state_id: state.id,
                requester_uuid: Some(ws.member_uuid),
                ..Default::default()
            },
        )
    })
    .expect("ordinary ticket");
    for outbox in ["notification_outbox", "webhook_outbox", "rule_event_outbox"] {
        let mut conn = pool.get().expect("conn");
        let n: Count = diesel::sql_query(format!(
            "SELECT count(*) AS n FROM {outbox} o JOIN sync_actions s ON s.sync_id = o.sync_id \
             WHERE s.aggregate_id = $1 AND s.event_type = 'ticket.created'"
        ))
        .bind::<Text, _>(own.id.to_string())
        .get_result(&mut conn)
        .expect("count");
        assert_eq!(n.n, 1, "an ordinary ticket reaches {outbox}");
    }
    backend::services::rules::elapsed::reconcile(&pool).expect("reconcile");
    assert_eq!(
        clocks(own.id),
        1,
        "an ordinary open ticket does get a clock"
    );

    // Again, unchanged: nothing new.
    let again = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!(
        (
            again.tickets_written,
            again.comments_written,
            again.attachments_written
        ),
        (0, 0, 0)
    );
    assert_eq!(imported_tickets(&pool, &ws).len(), 2);

    // A fresher export with one new reply: only the reply is added.
    let mut fresher = tickets();
    fresher[1].comments.push(comment(
        "c6",
        Some("u-agent"),
        true,
        "<p>Ordered.</p>",
        at(6, 9),
    ));
    let dir2 = tempfile::tempdir().expect("dir");
    let bundle2 = write_bundle(dir2.path(), &fresher);
    let delta = run(&pool, &bundle2, &options(&ws, Uuid::now_v7())).await;
    assert_eq!((delta.tickets_written, delta.comments_written), (0, 1));
    assert_eq!(comments_of(&pool, &ws, open.id).len(), 4);
}

#[actix_web::test]
async fn numbers_follow_existing_tickets_when_the_workspace_has_its_own() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    // A ticket of the workspace's own.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        backend::repository::tickets::create_ticket(
            c,
            backend::models::NewTicket {
                title: "Ours".into(),
                workflow_state_id: state.id,
                ..Default::default()
            },
        )
    })
    .expect("own ticket");

    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());
    let opts = options(&ws, Uuid::now_v7());

    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert!(!plan.keep_numbers, "{}", plan.numbers_reason);

    let mut keep = opts.clone();
    keep.numbers = NumberPolicy::Keep;
    assert!(
        loader::plan(&pool, &bundle, &keep).await.is_err(),
        "keeping numbers is refused when they could collide"
    );

    run(&pool, &bundle, &opts).await;
    let rows = imported_tickets(&pool, &ws);
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter().all(|r| r.number != 101 && r.number != 102),
        "{rows:?}"
    );
}

#[actix_web::test]
async fn a_failed_batch_rolls_back_alone_and_the_next_run_resumes() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());

    // The second ticket can't be written: its batch fails, the first's stays.
    diesel::sql_query(
        "ALTER TABLE tickets ADD CONSTRAINT test_refuse CHECK (title <> 'New laptop')",
    )
    .execute(&mut pool.get().expect("conn"))
    .expect("constraint");
    let failed = loader::load(
        &pool,
        storage(),
        &bundle,
        &options(&ws, Uuid::now_v7()),
        &|_| {},
    )
    .await;
    let err = failed.expect_err("the second batch fails");
    assert!(err.to_string().contains("Zendesk #102"), "{err}");
    assert_eq!(
        imported_tickets(&pool, &ws).len(),
        1,
        "the first batch stays"
    );

    diesel::sql_query("ALTER TABLE tickets DROP CONSTRAINT test_refuse")
        .execute(&mut pool.get().expect("conn"))
        .expect("drop constraint");
    let resumed = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!((resumed.tickets_written, resumed.tickets_skipped), (1, 1));
    let rows = imported_tickets(&pool, &ws);
    assert_eq!(rows.len(), 2);
    // Ticket 101's replies were written once, in the first run.
    assert_eq!(comments_of(&pool, &ws, rows[0].id).len(), 4);
}

#[actix_web::test]
async fn a_ticket_deleted_here_stays_deleted_and_does_not_block_reruns() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());
    run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;

    let laptop = imported_tickets(&pool, &ws)
        .into_iter()
        .find(|t| t.number == 102)
        .expect("102");
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("DELETE FROM tickets WHERE id = $1")
            .bind::<Integer, _>(laptop.id)
            .execute(c)
    })
    .expect("delete");

    // A newer export with a new reply on the deleted ticket.
    let mut fresher = tickets();
    fresher[1].comments.push(comment(
        "c6",
        Some("u-agent"),
        true,
        "<p>Ordered.</p>",
        at(6, 9),
    ));
    let dir2 = tempfile::tempdir().expect("dir");
    let report = run(
        &pool,
        &write_bundle(dir2.path(), &fresher),
        &options(&ws, Uuid::now_v7()),
    )
    .await;
    assert_eq!(report.tickets_written, 0);
    assert_eq!(report.comments_written, 0);
    assert!(
        report
            .ticket_notes
            .iter()
            .any(|n| n.contains("Zendesk #102")),
        "{:?}",
        report.ticket_notes
    );
    assert_eq!(imported_tickets(&pool, &ws).len(), 1);
}

#[actix_web::test]
async fn a_record_listed_twice_in_a_bundle_is_written_once() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;

    // The incremental export can list a ticket twice when it changes during
    // the run; a reply and a person can repeat too.
    let mut list = tickets();
    let dup_reply = list[0].comments[0].clone();
    list[0].comments.push(dup_reply);
    list.push(list[1].clone());
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &list);
    {
        let mut w = bundle.people_writer(false).expect("people");
        w.write(&people()[0]).expect("person again");
        w.checkpoint().expect("flush");
    }
    let mut opts = options(&ws, Uuid::now_v7());
    opts.batch_size = 200;

    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert!(
        plan.keep_numbers,
        "a ticket repeating its own number is fine: {}",
        plan.numbers_reason
    );
    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 2);
    assert!(report.ticket_notes.is_empty(), "{:?}", report.ticket_notes);
    let rows = imported_tickets(&pool, &ws);
    assert_eq!(rows.len(), 2);
    assert_eq!(
        comments_of(&pool, &ws, rows[0].id).len(),
        4,
        "the repeated reply is written once"
    );
}

fn person(id: &str, email: &str, role: PersonRole, active: bool) -> Person {
    Person {
        source_id: id.into(),
        email: Some(email.into()),
        name: format!("Source {id}"),
        role,
        active,
        organization: None,
        created_at: None,
        updated_at: None,
    }
}

fn raised_by(id: &str, requester: &str, watchers: &[&str]) -> Ticket {
    Ticket {
        source_id: id.into(),
        number: id.parse().ok(),
        subject: format!("Ticket {id}"),
        status: StatusKind::Open,
        status_label: "open".into(),
        priority: None,
        ticket_type: None,
        tags: vec![],
        requester: Some(requester.into()),
        assignee: None,
        watchers: watchers.iter().map(|w| w.to_string()).collect(),
        created_at: at(1, 9),
        updated_at: at(1, 10),
        solved_at: None,
        fields: BTreeMap::new(),
        comments: vec![comment(
            &format!("c{id}"),
            Some(requester),
            true,
            "<p>Help</p>",
            at(1, 9),
        )],
    }
}

/// An account from outside the import: `platform_role` as given, a primary
/// address, any secondary ones, and a seat in `seat_in` with `role`.
fn account(
    pool: &TestPool,
    name: &str,
    platform_role: Option<&str>,
    primary: &str,
    secondary: &[&str],
    seat_in: Option<(i32, &str)>,
) -> Uuid {
    #[derive(QueryableByName)]
    struct Id {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        uuid: Uuid,
    }
    let mut conn = pool.get().expect("conn");
    let uuid = diesel::sql_query(
        "INSERT INTO users (uuid, name, platform_role) VALUES (gen_random_uuid(), $1, coalesce($2, 'user')) \
         RETURNING uuid",
    )
    .bind::<Text, _>(name)
    .bind::<Nullable<Text>, _>(platform_role)
    .get_result::<Id>(&mut conn)
    .expect("user")
    .uuid;
    let mut emails = vec![(primary, true)];
    emails.extend(secondary.iter().map(|e| (*e, false)));
    for (email, is_primary) in emails {
        diesel::sql_query(
            "INSERT INTO user_emails (user_uuid, email, email_type, is_primary, is_verified) \
             VALUES ($1, $2, 'personal', $3, true)",
        )
        .bind::<diesel::sql_types::Uuid, _>(uuid)
        .bind::<Text, _>(email)
        .bind::<Bool, _>(is_primary)
        .execute(&mut conn)
        .expect("email");
    }
    if let Some((ws, role)) = seat_in {
        crate::common::seat(&mut conn, ws, uuid, role);
    }
    uuid
}

#[derive(QueryableByName, Debug)]
struct Requested {
    #[diesel(sql_type = Integer)]
    number: i32,
    #[diesel(sql_type = diesel::sql_types::Uuid)]
    requester: Uuid,
}

fn requesters(pool: &TestPool, ws: &WorkspaceSeed) -> HashMap<i32, Uuid> {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT number, requester_uuid AS requester FROM tickets WHERE submitted_via = 'import'",
        )
        .load::<Requested>(c)
    })
    .expect("requesters")
    .into_iter()
    .map(|r| (r.number, r.requester))
    .collect()
}

#[derive(QueryableByName, Debug)]
struct Seat {
    #[diesel(sql_type = Text)]
    role: String,
    #[diesel(sql_type = Bool)]
    removed: bool,
}

/// The account's seat in `ws`, with whether it was removed.
fn seat_of(pool: &TestPool, ws: i32, user: Uuid) -> Option<(String, bool)> {
    let mut conn = pool.get().expect("conn");
    diesel::sql_query(
        "SELECT role, removed_at IS NOT NULL AS removed FROM workspace_members \
         WHERE workspace_id = $1 AND user_uuid = $2",
    )
    .bind::<Integer, _>(ws)
    .bind::<diesel::sql_types::Uuid, _>(user)
    .get_result::<Seat>(&mut conn)
    .optional()
    .expect("seat")
    .map(|s| (s.role, s.removed))
}

fn account_of(pool: &TestPool, email: &str) -> Uuid {
    #[derive(QueryableByName)]
    struct Id {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        user_uuid: Uuid,
    }
    let mut conn = pool.get().expect("conn");
    diesel::sql_query("SELECT user_uuid FROM user_emails WHERE lower(email) = lower($1)")
        .bind::<Text, _>(email)
        .get_result::<Id>(&mut conn)
        .expect("account")
        .user_uuid
}

fn watchers_of(pool: &TestPool, ws: &WorkspaceSeed, number: i32) -> Vec<Uuid> {
    #[derive(QueryableByName)]
    struct W {
        #[diesel(sql_type = diesel::sql_types::Uuid)]
        user_uuid: Uuid,
    }
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT w.user_uuid FROM ticket_watchers w JOIN tickets t ON t.id = w.ticket_id \
             WHERE t.number = $1",
        )
        .bind::<Integer, _>(number)
        .load::<W>(c)
    })
    .expect("watchers")
    .into_iter()
    .map(|w| w.user_uuid)
    .collect()
}

#[actix_web::test]
async fn accounts_from_elsewhere_are_attached_only_within_the_workspaces_reach() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let two = crate::common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (ws, other) = (two.a, two.b);

    let agent_elsewhere = account(
        &pool,
        "Agent Elsewhere",
        None,
        "agent@other.test",
        &[],
        Some((other.workspace_id, "agent")),
    );
    let platform = account(
        &pool,
        "Platform Admin",
        Some("platform_admin"),
        "root@other.test",
        &[],
        None,
    );
    let aliased = account(
        &pool,
        "Has An Alias",
        None,
        "main@other.test",
        &["alias@other.test"],
        Some((other.workspace_id, "member")),
    );
    let reachable = account(
        &pool,
        "Requester Elsewhere",
        None,
        "known@other.test",
        &[],
        Some((other.workspace_id, "member")),
    );

    let bundle_people = vec![
        person("1", "agent@other.test", PersonRole::Requester, true),
        person("2", "root@other.test", PersonRole::Requester, true),
        person("3", "ALIAS@other.test", PersonRole::Requester, true),
        person("4", "known@other.test", PersonRole::Requester, true),
        person("5", "suspended@customer.test", PersonRole::Requester, false),
        person("6", "gone@acme.test", PersonRole::Agent, false),
        person("7", "watcher@customer.test", PersonRole::Requester, true),
    ];
    let list = vec![
        raised_by("301", "1", &[]),
        raised_by("302", "2", &[]),
        raised_by("303", "3", &[]),
        raised_by("304", "4", &["5", "6", "7", "1"]),
        raised_by("305", "5", &[]),
    ];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_of(dir.path(), &bundle_people, &list);
    let mut opts = options(&ws, Uuid::now_v7());
    opts.create_agents = false;
    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 5);

    let by_number = requesters(&pool, &ws);
    // Staff elsewhere, a platform role and a secondary address of an account
    // from elsewhere are never attached: their tickets go to the placeholder.
    let placeholder = by_number[&301];
    for (n, account) in [(301, agent_elsewhere), (302, platform), (303, aliased)] {
        assert_eq!(by_number[&n], placeholder, "ticket {n}");
        assert_ne!(by_number[&n], account, "ticket {n}");
        assert_eq!(
            seat_of(&pool, ws.workspace_id, account),
            None,
            "no seat for {n}"
        );
    }
    let thread = comments_of(&pool, &ws, imported_tickets(&pool, &ws)[0].id);
    assert!(
        thread.iter().any(|c| c.author == "Unknown (Zendesk)"),
        "{thread:?}"
    );
    {
        #[derive(QueryableByName)]
        struct Meta {
            #[diesel(sql_type = Nullable<Text>)]
            email: Option<String>,
        }
        let kept: Vec<Option<String>> = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
            diesel::sql_query(
                "SELECT c.channel_metadata->'import'->>'author_email' AS email FROM comments c \
                 JOIN tickets t ON t.id = c.ticket_id WHERE t.number = 301 AND NOT c.is_internal",
            )
            .load::<Meta>(c)
        })
        .expect("meta")
        .into_iter()
        .map(|m| m.email)
        .collect();
        assert_eq!(kept, vec![Some("agent@other.test".to_string())]);
    }

    // A requester from elsewhere within reach is reused and seated here.
    assert_eq!(by_number[&304], reachable);
    assert_eq!(
        seat_of(&pool, ws.workspace_id, reachable),
        Some(("member".to_string(), false))
    );

    // Suspended and former people get accounts without a seat, and don't
    // watch; a seated requester does.
    let suspended = account_of(&pool, "suspended@customer.test");
    let gone = account_of(&pool, "gone@acme.test");
    let watcher = account_of(&pool, "watcher@customer.test");
    assert_eq!(by_number[&305], suspended);
    assert_eq!(
        seat_of(&pool, ws.workspace_id, suspended),
        Some(("member".to_string(), true))
    );
    assert_eq!(
        seat_of(&pool, ws.workspace_id, gone),
        Some(("member".to_string(), true))
    );
    let watching = watchers_of(&pool, &ws, 304);
    assert!(watching.contains(&watcher), "{watching:?}");
    for not_watching in [suspended, gone, agent_elsewhere, placeholder] {
        assert!(!watching.contains(&not_watching), "{watching:?}");
    }

    // A rerun attaches no one new and keeps the placeholder.
    run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!(requesters(&pool, &ws)[&301], placeholder);
    assert_eq!(seat_of(&pool, ws.workspace_id, agent_elsewhere), None);
}

#[cfg(unix)]
#[actix_web::test]
async fn files_come_only_from_the_bundle_and_are_checked_as_uploads_are() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let mut list = tickets();
    for (id, name) in [("a3", "drawing.svg"), ("a4", "notes.txt")] {
        list[0].comments[2].attachments.push(Attachment {
            source_id: id.into(),
            file_name: name.into(),
            content_type: Some("text/plain".into()),
            size: None,
            stored: true,
            missing_reason: None,
            source_url: None,
            retry: false,
        });
    }
    let bundle = write_bundle(dir.path(), &list);
    std::fs::write(
        bundle.file_path("a3"),
        br#"<svg xmlns="http://www.w3.org/2000/svg"><script>alert(1)</script></svg>"#,
    )
    .expect("svg");
    let secret = dir.path().join("operator-secret");
    std::fs::write(&secret, b"DATABASE_URL=postgres://secret").expect("secret");
    std::os::unix::fs::symlink(&secret, bundle.file_path("a4")).expect("link");

    let report = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!(
        report.attachments_written, 1,
        "{:?}",
        report.attachment_problems
    );
    for name in ["drawing.svg", "notes.txt"] {
        assert!(
            report
                .attachment_problems
                .iter()
                .any(|p| p.starts_with(name)),
            "{name}: {:?}",
            report.attachment_problems
        );
    }
}
