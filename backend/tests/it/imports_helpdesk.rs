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
        cancel: Default::default(),
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
async fn a_ticket_that_fails_rolls_back_alone_and_the_next_run_writes_it() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());

    // The second ticket can't be written: it alone rolls back, in the same
    // batch as the first, and the run carries on and says so.
    diesel::sql_query(
        "ALTER TABLE tickets ADD CONSTRAINT test_refuse CHECK (title <> 'New laptop')",
    )
    .execute(&mut pool.get().expect("conn"))
    .expect("constraint");
    let mut opts = options(&ws, Uuid::now_v7());
    opts.batch_size = 200;
    let first = run(&pool, &bundle, &opts).await;
    assert_eq!(first.tickets_written, 1);
    assert_eq!(first.tickets_failed.len(), 1, "{:?}", first.tickets_failed);
    assert!(
        first.tickets_failed[0].contains("Zendesk #102"),
        "{:?}",
        first.tickets_failed
    );
    assert_eq!(imported_tickets(&pool, &ws).len(), 1);
    let report = backend::services::imports::helpdesk::report::render_report(&first);
    assert!(report.contains("Zendesk #102"), "{report}");

    diesel::sql_query("ALTER TABLE tickets DROP CONSTRAINT test_refuse")
        .execute(&mut pool.get().expect("conn"))
        .expect("drop constraint");
    let resumed = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!((resumed.tickets_written, resumed.tickets_skipped), (1, 1));
    assert!(resumed.tickets_failed.is_empty());
    let rows = imported_tickets(&pool, &ws);
    assert_eq!(rows.len(), 2);
    // Ticket 101's replies were written once, in the first run.
    assert_eq!(comments_of(&pool, &ws, rows[0].id).len(), 4);
}

#[actix_web::test]
async fn a_record_with_a_nul_is_skipped_and_reported_and_the_rest_lands() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let mut list = tickets();
    // A bundle from elsewhere, not mapped through the connector.
    list[0].subject = "VPN\u{0} drops".into();
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &list);
    // And a line that isn't a ticket at all.
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(bundle.root().join("tickets.jsonl"))
            .expect("tickets");
        writeln!(f, "{{\"not\": \"a ticket\"}}").expect("line");
    }
    let report = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!(report.tickets_written, 1);
    assert_eq!(
        report.tickets_failed.len(),
        1,
        "{:?}",
        report.tickets_failed
    );
    assert!(report.tickets_failed[0].contains("Zendesk #101"));
    assert_eq!(report.unreadable_lines, 1);
    assert_eq!(imported_tickets(&pool, &ws)[0].number, 102);
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
    assert_eq!(seat_of(&pool, ws.workspace_id, suspended), None);
    assert_eq!(seat_of(&pool, ws.workspace_id, gone), None);
    // They are the workspace's people all the same (a profile kept for
    // them), and a seat granted later is an ordinary one: an admin can make
    // the former agent an agent again.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        assert!(backend::repository::directory::is_person(
            c,
            ws.workspace_id,
            gone
        )?);
        backend::repository::workspaces::add_membership(
            c,
            ws.workspace_id,
            gone,
            "agent",
            backend::repository::workspaces::SeatWriteAuthority::Product,
        )
    })
    .expect("seat the former agent");
    assert_eq!(
        seat_of(&pool, ws.workspace_id, gone),
        Some(("agent".to_string(), false))
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

fn ordinary_ticket(pool: &TestPool, ws: &WorkspaceSeed, title: &str) -> backend::models::Ticket {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        backend::repository::tickets::create_ticket(
            c,
            backend::models::NewTicket {
                title: title.into(),
                workflow_state_id: state.id,
                requester_uuid: Some(ws.member_uuid),
                ..Default::default()
            },
        )
    })
    .expect("ordinary ticket")
}

#[actix_web::test]
async fn kept_numbers_are_reserved_and_a_taken_one_falls_back() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());

    // A ticket opened here while the import runs (after its first batch)
    // takes a number after every number the import keeps.
    let opened = std::sync::Mutex::new(None);
    let store = storage();
    loader::load(&pool, store, &bundle, &options(&ws, Uuid::now_v7()), &|p| {
        let mut opened = opened.lock().expect("lock");
        if p.tickets_done == 1 && opened.is_none() {
            *opened = Some(ordinary_ticket(&pool, &ws, "Opened meanwhile").number);
        }
    })
    .await
    .expect("load");
    assert_eq!(*opened.lock().expect("lock"), Some(103));
    let numbers: Vec<i32> = imported_tickets(&pool, &ws)
        .iter()
        .map(|t| t.number)
        .collect();
    assert_eq!(numbers, vec![101, 102]);

    // Deleting an imported ticket doesn't make the workspace's own tickets
    // look negative: only the one opened here counts against keeping.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("DELETE FROM tickets WHERE number = 102 AND submitted_via = 'import'")
            .execute(c)
    })
    .expect("delete");
    let mut keep = options(&ws, Uuid::now_v7());
    keep.numbers = NumberPolicy::Keep;
    let err = loader::plan(&pool, &bundle, &keep)
        .await
        .expect_err("one ticket of the workspace's own");
    assert!(err.to_string().contains("already has 1 tickets"), "{err}");
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("DELETE FROM tickets WHERE title = 'Opened meanwhile'").execute(c)
    })
    .expect("delete own");
    assert!(
        loader::plan(&pool, &bundle, &keep)
            .await
            .expect("plan")
            .keep_numbers,
        "only the source's own tickets remain, one of them deleted"
    );

    // A new source ticket whose number is already held here (#101) gets the
    // next one, and the report says so.
    let mut reused = raised_by("201", "u-req", &[]);
    reused.number = Some(101);
    let dir2 = tempfile::tempdir().expect("dir");
    let report = run(&pool, &write_bundle(dir2.path(), &[reused]), &keep).await;
    assert_eq!(report.tickets_written, 1);
    assert_eq!(report.numbers_moved.len(), 1, "{:?}", report.numbers_moved);
    assert!(report.numbers_moved[0].contains("#101 is taken"));

    // Numbers past the cap aren't kept.
    let mut huge = raised_by("202", "u-req", &[]);
    huge.number = Some(2_000_000_000);
    let dir3 = tempfile::tempdir().expect("dir");
    let plan = loader::plan(
        &pool,
        &write_bundle(dir3.path(), &[huge]),
        &options(&ws, Uuid::now_v7()),
    )
    .await
    .expect("plan");
    assert!(!plan.keep_numbers, "{}", plan.numbers_reason);
}

#[actix_web::test]
async fn two_loads_at_once_write_each_ticket_once() {
    let db = TestDb::new();
    let pool = db.pool_with_size(8);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());
    let (a, b) = (options(&ws, Uuid::now_v7()), options(&ws, Uuid::now_v7()));
    let (one, two) = futures::join!(run(&pool, &bundle, &a), run(&pool, &bundle, &b));
    assert!(one.tickets_failed.is_empty(), "{:?}", one.tickets_failed);
    assert!(two.tickets_failed.is_empty(), "{:?}", two.tickets_failed);
    assert_eq!(one.tickets_written + two.tickets_written, 2);
    assert_eq!(one.comments_written + two.comments_written, 5);
    assert_eq!(one.attachments_written + two.attachments_written, 1);
    let rows = imported_tickets(&pool, &ws);
    assert_eq!(rows.len(), 2);
    assert_eq!(comments_of(&pool, &ws, rows[0].id).len(), 4);
}

#[derive(QueryableByName, Debug)]
struct Standing {
    #[diesel(sql_type = Text)]
    category: String,
    #[diesel(sql_type = Nullable<Timestamptz>)]
    closed_at: Option<DateTime<Utc>>,
    #[diesel(sql_type = Timestamptz)]
    updated_at: DateTime<Utc>,
}

fn standing(pool: &TestPool, ws: &WorkspaceSeed, number: i32) -> Standing {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT s.category::text AS category, t.closed_at, t.updated_at FROM tickets t \
             JOIN workflow_states s ON s.id = t.workflow_state_id WHERE t.number = $1",
        )
        .bind::<Integer, _>(number)
        .get_result::<Standing>(c)
    })
    .expect("standing")
}

fn live_rules(pool: &TestPool, ws: &WorkspaceSeed) {
    let mut conn = pool.get().expect("conn");
    diesel::sql_query(
        "INSERT INTO rules (workspace_id, name, trigger_kind, trigger_config, actions, state) VALUES \
         ($1, 'On create', 'ticket_created', '{}'::jsonb, \
          '[{\"kind\":\"set_priority\",\"config\":{\"priority\":\"high\"}}]'::jsonb, 'live'), \
         ($1, 'On update', 'ticket_updated', '{}'::jsonb, \
          '[{\"kind\":\"set_priority\",\"config\":{\"priority\":\"high\"}}]'::jsonb, 'live'), \
         ($1, 'After an hour', 'time_elapsed', '{\"clock_name\":\"since_created\",\"minutes\":60}'::jsonb, \
          '[{\"kind\":\"set_priority\",\"config\":{\"priority\":\"high\"}}]'::jsonb, 'live')",
    )
    .bind::<Integer, _>(ws.workspace_id)
    .execute(&mut conn)
    .expect("live rules");
}

#[derive(QueryableByName, Debug)]
struct Assigned {
    #[diesel(sql_type = Nullable<diesel::sql_types::Uuid>)]
    assignee_uuid: Option<Uuid>,
}

fn assignee_of(pool: &TestPool, ws: &WorkspaceSeed, number: i32) -> Option<Uuid> {
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("SELECT assignee_uuid FROM tickets WHERE number = $1")
            .bind::<Integer, _>(number)
            .get_result::<Assigned>(c)
    })
    .expect("assignee")
    .assignee_uuid
}

fn ticket_id(pool: &TestPool, ws: &WorkspaceSeed, number: i32) -> i32 {
    imported_tickets(pool, ws)
        .into_iter()
        .find(|t| t.number == number)
        .expect("ticket")
        .id
}

#[actix_web::test]
async fn a_cutover_rerun_follows_the_source_on_tickets_nobody_changed_here() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    live_rules(&pool, &ws);
    let mut cast = people();
    cast.push(person(
        "u-agent2",
        "second@acme.test",
        PersonRole::Agent,
        true,
    ));
    let mut list = tickets();
    list.push(raised_by("103", "u-req", &[]));
    list.push(raised_by("104", "u-req", &[]));
    let dir = tempfile::tempdir().expect("dir");
    run(
        &pool,
        &write_bundle_of(dir.path(), &cast, &list),
        &options(&ws, Uuid::now_v7()),
    )
    .await;

    // History keeps the source's last change as its own.
    assert_eq!(standing(&pool, &ws, 101).updated_at, at(3, 17));
    assert_eq!(standing(&pool, &ws, 101).category, "done");
    assert_eq!(standing(&pool, &ws, 102).category, "active");
    let first_agent = assignee_of(&pool, &ws, 102).expect("assigned");

    // Before the cutover, someone here replies on 103 (which moves its
    // updated_at) and adds a watcher to 104 (which only the activity log
    // shows).
    let (t103, t104) = (ticket_id(&pool, &ws, 103), ticket_id(&pool, &ws, 104));
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        backend::repository::comments::create_comment(
            c,
            backend::models::NewComment {
                content: "On it".into(),
                ticket_id: t103,
                user_uuid: ws.admin_uuid,
                is_internal: true,
                ..Default::default()
            },
            None,
        )?;
        backend::repository::ticket_watchers::add_watcher(c, t104, ws.admin_uuid, false)
    })
    .expect("changes here");

    // At the cutover: Zendesk reopened 101, solved 102 and gave it to the
    // second agent, and solved 103 and 104.
    let mut fresher = list.clone();
    for t in fresher.iter_mut() {
        t.updated_at = at(8, 12);
        if t.source_id == "101" {
            t.status = StatusKind::Open;
            t.status_label = "open".into();
            t.solved_at = None;
        } else {
            t.status = StatusKind::Solved;
            t.status_label = "solved".into();
            t.solved_at = Some(at(8, 12));
        }
        if t.source_id == "102" {
            t.assignee = Some("u-agent2".into());
        }
    }
    let job = Uuid::now_v7();
    let dir2 = tempfile::tempdir().expect("dir");
    let bundle2 = write_bundle_of(dir2.path(), &cast, &fresher);
    let report = run(&pool, &bundle2, &options(&ws, job)).await;
    assert_eq!(report.tickets_updated, 2, "{report:?}");
    let reopened = standing(&pool, &ws, 101);
    assert_eq!(reopened.category, "active");
    assert_eq!(reopened.closed_at, None, "a reopen clears the closing");
    let solved = standing(&pool, &ws, 102);
    assert_eq!(solved.category, "done");
    assert_eq!(solved.closed_at, Some(at(8, 12)));
    let second_agent = assignee_of(&pool, &ws, 102).expect("assigned");
    assert_ne!(second_agent, first_agent, "the assignee follows the source");
    // Changed here, by a reply or by activity alone: left alone, and listed.
    assert_eq!(standing(&pool, &ws, 103).category, "active");
    assert_eq!(standing(&pool, &ws, 104).category, "active");
    assert_eq!(
        report.tickets_changed_here.len(),
        2,
        "{:?}",
        report.tickets_changed_here
    );

    // Still quiet, with live rules that would fire: the updates reached
    // clients but no outbox, and no clock started.
    assert!(
        count(
            &pool,
            "SELECT count(*) AS n FROM sync_actions WHERE actor_ref = $1 AND aggregate = 'ticket'",
            job
        ) > 0
    );
    for outbox in ["notification_outbox", "webhook_outbox", "rule_event_outbox"] {
        let n = count(
            &pool,
            &format!(
                "SELECT count(*) AS n FROM {outbox} o JOIN sync_actions s ON s.sync_id = o.sync_id \
                 WHERE s.actor_ref = $1"
            ),
            job,
        );
        assert_eq!(n, 0, "{outbox} got {n} rows from the cutover");
    }
    backend::services::rules::elapsed::reconcile(&pool).expect("reconcile");
    {
        let mut conn = pool.get().expect("conn");
        let clocks: Count = diesel::sql_query(
            "SELECT count(*) AS n FROM ticket_elapsed_queue q JOIN tickets t ON t.id = q.ticket_id \
             WHERE t.submitted_via = 'import'",
        )
        .get_result(&mut conn)
        .expect("clocks");
        assert_eq!(clocks.n, 0);
    }

    // The same bundle again changes nothing: 101 and 102 were written by the
    // import last and are no newer at the source; 103 and 104 are still
    // changed here.
    let again = run(&pool, &bundle2, &options(&ws, Uuid::now_v7())).await;
    assert_eq!(again.tickets_updated, 0);
    assert_eq!(again.tickets_changed_here.len(), 2);
}

#[actix_web::test]
async fn a_change_made_after_the_batch_read_is_seen_under_the_lock() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    run(
        &pool,
        &write_bundle(dir.path(), &tickets()),
        &options(&ws, Uuid::now_v7()),
    )
    .await;
    use backend::repository::import_source_ids as idmap;
    let scope = idmap::Scope {
        workspace_id: ws.workspace_id,
        source: "zendesk",
        instance: "acme",
    };
    // The batch reads 102 as untouched; someone changes it before the
    // import gets to it; the locked read sees the change.
    let read = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        idmap::imported_tickets(c, scope, &["102".to_string()])
    })
    .expect("read");
    assert!(read["102"].untouched);
    let id = read["102"].ticket_id;
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("UPDATE tickets SET title = 'Renamed here' WHERE id = $1")
            .bind::<Integer, _>(id)
            .execute(c)
    })
    .expect("edit");
    let locked = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        idmap::lock_imported_ticket(c, scope, "102")
    })
    .expect("lock");
    assert_eq!(locked, Some((id, false)));
}

#[actix_web::test]
async fn new_replies_on_a_ticket_merged_here_go_to_where_it_was_merged() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    run(
        &pool,
        &write_bundle(dir.path(), &tickets()),
        &options(&ws, Uuid::now_v7()),
    )
    .await;
    let (t101, t102) = (ticket_id(&pool, &ws, 101), ticket_id(&pool, &ws, 102));
    backend::repository::ticket_merge::execute_merge(
        &mut pool.get().expect("conn"),
        backend::repository::ticket_merge::MergeInput {
            destination_ticket_id: t101,
            source_ticket_ids: vec![t102],
            reason: None,
            notify_customer: false,
            expected_state: Vec::new(),
            marker_body: None,
        },
        &backend::sync::actor::ActorContext::user(ws.admin_uuid, None)
            .with_workspace(ws.workspace_id),
    )
    .expect("merge");
    let before = comments_of(&pool, &ws, t101).len();

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
    assert_eq!(report.comments_written, 1);
    let thread = comments_of(&pool, &ws, t101);
    assert_eq!(thread.len(), before + 1);
    assert!(thread.iter().any(|c| c.content.contains("Ordered.")));
    assert!(
        report
            .ticket_notes
            .iter()
            .any(|n| n.contains("Zendesk #102") && n.contains("#101")),
        "{:?}",
        report.ticket_notes
    );
}

/// Adds `address` to `user` as a secondary address, verified or not.
fn add_address(pool: &TestPool, user: Uuid, address: &str, verified: bool) {
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, email_type, is_primary, is_verified) \
         VALUES ($1, $2, 'personal', false, $3)",
    )
    .bind::<diesel::sql_types::Uuid, _>(user)
    .bind::<Text, _>(address)
    .bind::<Bool, _>(verified)
    .execute(&mut pool.get().expect("conn"))
    .expect("address");
}

#[actix_web::test]
async fn an_unverified_address_stands_for_nobody_and_a_removed_seat_stays_removed() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    // A member claims the CEO's address on their own account, unverified;
    // another holds a verified secondary address; a third was removed here.
    add_address(&pool, ws.member_uuid, "ceo@customer.test", false);
    let verified = account(
        &pool,
        "Verified Alias",
        None,
        "main@customer.test",
        &[],
        Some((ws.workspace_id, "member")),
    );
    add_address(&pool, verified, "alias@customer.test", true);
    let removed = account(
        &pool,
        "Removed Here",
        None,
        "removed@customer.test",
        &[],
        Some((ws.workspace_id, "member")),
    );
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        backend::repository::workspaces::remove_membership(
            c,
            ws.workspace_id,
            removed,
            backend::repository::workspaces::SeatWriteAuthority::Product,
        )
    })
    .expect("remove");

    let cast = vec![
        person("1", "ceo@customer.test", PersonRole::Requester, true),
        person("2", "alias@customer.test", PersonRole::Requester, true),
        person("3", "removed@customer.test", PersonRole::Requester, true),
    ];
    let list = vec![
        raised_by("401", "1", &[]),
        raised_by("402", "2", &[]),
        raised_by("403", "3", &[]),
    ];
    let dir = tempfile::tempdir().expect("dir");
    run(
        &pool,
        &write_bundle_of(dir.path(), &cast, &list),
        &options(&ws, Uuid::now_v7()),
    )
    .await;
    let by_number = requesters(&pool, &ws);
    assert_ne!(
        by_number[&401], ws.member_uuid,
        "an unverified claim matches nobody"
    );
    assert_eq!(
        by_number[&402], verified,
        "a verified secondary address still matches"
    );
    assert_eq!(
        by_number[&403], removed,
        "a removed member is still known here"
    );
    assert_eq!(
        seat_of(&pool, ws.workspace_id, removed),
        Some(("member".to_string(), true)),
        "and their seat stays removed"
    );
}

#[actix_web::test]
async fn one_person_who_cant_be_seated_doesnt_stop_the_run() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    // No room for another staff seat.
    {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query(
            "UPDATE workspaces SET seat_limit = (SELECT count(*) FROM workspace_members \
             WHERE workspace_id = $1 AND removed_at IS NULL AND role IN ('owner','admin','agent')) \
             WHERE id = $1",
        )
        .bind::<Integer, _>(ws.workspace_id)
        .execute(&mut conn)
        .expect("seat limit");
    }
    let dir = tempfile::tempdir().expect("dir");
    let report = run(
        &pool,
        &write_bundle(dir.path(), &tickets()),
        &options(&ws, Uuid::now_v7()),
    )
    .await;
    assert_eq!(report.tickets_written, 2);
    assert_eq!(
        report.people_problems.len(),
        1,
        "{:?}",
        report.people_problems
    );
    assert!(report.people_problems[0].contains("Alex Agent"));
    let agent = account_of(&pool, "agent@acme.test");
    assert_eq!(
        seat_of(&pool, ws.workspace_id, agent),
        None,
        "brought across without a seat"
    );
    let thread = comments_of(&pool, &ws, imported_tickets(&pool, &ws)[0].id);
    assert!(
        thread.iter().any(|c| c.author == "Alex Agent"),
        "{thread:?}"
    );
}
