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
    Attachment, Body, BodyFormat, Bundle, Comment, Fill, Manifest, Organization, PeopleSource,
    Person, PersonRole, PriorityLevel, StatusKind, Ticket, FORMAT_VERSION, PERSON_DETAIL,
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
        deleted: false,
        limited_staff: false,
        custom_role: false,
        organization: None,
        organization_id: None,
        phone: None,
        shared_phone: false,
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
        organization_id: None,
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
        retries: 0,
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
        retries: 0,
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
        organization_id: None,
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

/// A bundle as the API fetch writes it, so anyone it lacks (u-ghost) is gone
/// at Zendesk and the run goes ahead without --allow-missing-people.
fn write_bundle_of(dir: &std::path::Path, people: &[Person], tickets: &[Ticket]) -> Bundle {
    write_bundle_from(dir, people, tickets, Some(PeopleSource::Api))
}

fn write_bundle_from(
    dir: &std::path::Path,
    people: &[Person],
    tickets: &[Ticket],
    people_source: Option<PeopleSource>,
) -> Bundle {
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
        people_source,
        person_detail: PERSON_DETAIL,
        inputs: None,
        fill: None,
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
        allow_missing_people: false,
        batch_size: 1,
        people_batch_size: 500,
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

    // A reply by someone gone at Zendesk is credited to the placeholder.
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
        deleted: false,
        limited_staff: false,
        custom_role: false,
        organization: None,
        organization_id: None,
        phone: None,
        shared_phone: false,
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
        organization_id: None,
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
            retries: 0,
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
    list.push(raised_by("105", "u-req", &[]));
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
    // updated_at), adds a watcher to 104 (which only the activity log
    // shows), and drags 105 into place on a board (which isn't an edit).
    let (t103, t104, t105) = (
        ticket_id(&pool, &ws, 103),
        ticket_id(&pool, &ws, 104),
        ticket_id(&pool, &ws, 105),
    );
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        backend::repository::tickets::update_ticket_partial(
            c,
            t105,
            backend::models::TicketUpdate {
                rank: Some(Some("a0".to_string())),
                ..Default::default()
            },
            None,
        )
        .map_err(diesel::result::Error::from)
    })
    .expect("reorder");
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
    assert_eq!(report.tickets_updated, 3, "{report:?}");
    assert_eq!(
        standing(&pool, &ws, 105).category,
        "done",
        "a reorder here doesn't hold the cutover back"
    );
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

    // The same bundle again changes nothing: 101, 102 and 105 were written
    // by the import last and are no newer at the source; 103 and 104 are
    // still changed here.
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

#[actix_web::test]
async fn an_edit_that_lands_while_the_cutover_runs_is_not_overwritten() {
    let db = TestDb::new();
    let pool = db.pool_with_size(6);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    run(
        &pool,
        &write_bundle(dir.path(), &tickets()),
        &options(&ws, Uuid::now_v7()),
    )
    .await;
    let t102 = ticket_id(&pool, &ws, 102);

    // Someone here starts an edit of 102 and holds it uncommitted while the
    // cutover reads its batch (seeing 102 untouched), then commits.
    let (started, edit_started) = std::sync::mpsc::channel();
    let editor = {
        let pool = pool.clone();
        let ws_id = ws.workspace_id;
        std::thread::spawn(move || {
            run_in_workspace(&pool, REF, ws_id, |c| {
                diesel::sql_query("UPDATE tickets SET title = 'Renamed here' WHERE id = $1")
                    .bind::<Integer, _>(t102)
                    .execute(c)?;
                started.send(()).expect("signal");
                std::thread::sleep(std::time::Duration::from_secs(3));
                Ok(())
            })
            .expect("edit")
        })
    };
    edit_started.recv().expect("edit started");

    let mut fresher = tickets();
    fresher[1].status = StatusKind::Solved;
    fresher[1].status_label = "solved".into();
    fresher[1].solved_at = Some(at(8, 12));
    fresher[1].updated_at = at(8, 12);
    let dir2 = tempfile::tempdir().expect("dir");
    let report = run(
        &pool,
        &write_bundle(dir2.path(), &fresher),
        &options(&ws, Uuid::now_v7()),
    )
    .await;
    editor.join().expect("editor");

    // The write waited for the edit and then saw it: 102 is left as the
    // edit left it, and reported.
    assert_eq!(report.tickets_updated, 0, "{report:?}");
    assert_eq!(standing(&pool, &ws, 102).category, "active");
    assert!(
        report
            .tickets_changed_here
            .iter()
            .any(|n| n.contains("Zendesk #102")),
        "{:?}",
        report.tickets_changed_here
    );
}

#[actix_web::test]
async fn the_fallback_never_makes_a_second_account_for_an_address() {
    use backend::repository::import_source_ids::Scope;
    use backend::services::imports::helpdesk::people::{self, PeopleOptions};
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    // An account holds the address in another case.
    let holder = account(&pool, "Holder", None, "Dup@Acme.test", &[], None);
    let map = HashMap::new();
    let opts = PeopleOptions {
        scope: Scope {
            workspace_id: ws.workspace_id,
            source: "zendesk",
            instance: "acme",
        },
        job_id: Uuid::now_v7(),
        agent_map: &map,
        create_agents: false,
    };
    let placeholder = Uuid::now_v7();
    let staff = person("9", "dup@acme.test", PersonRole::Agent, true);
    let instead = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        Ok(people::fallback(c, &staff, &[&staff], &opts, placeholder))
    })
    .expect("fallback");
    assert!(instead.withheld(), "{instead:?}");
    assert_eq!(account_of(&pool, "dup@acme.test"), holder);
    let holders: Count = diesel::sql_query(
        "SELECT count(*) AS n FROM user_emails WHERE lower(email) = 'dup@acme.test'",
    )
    .get_result(&mut pool.get().expect("conn"))
    .expect("count");
    assert_eq!(holders.n, 1, "no second account for the address");

    // With the address free, staff keep their name on an unseated account.
    let free = person("10", "free@acme.test", PersonRole::Agent, true);
    let made = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        Ok(people::fallback(c, &free, &[&free], &opts, placeholder))
    })
    .expect("fallback");
    assert!(!made.withheld());
    assert_eq!(account_of(&pool, "free@acme.test"), made.uuid);
}

/// Rows the run `job` left: its job row and id map entries, and any
/// placeholder author, which only a run that writes creates.
fn traces(pool: &TestPool, job: Uuid) -> (i64, i64, i64) {
    let mut conn = pool.get().expect("conn");
    let mut n = |sql: &str| {
        diesel::sql_query(sql)
            .bind::<diesel::sql_types::Uuid, _>(job)
            .get_result::<Count>(&mut conn)
            .expect("count")
            .n
    };
    (
        n("SELECT count(*) AS n FROM import_jobs WHERE id = $1"),
        n("SELECT count(*) AS n FROM import_source_ids WHERE job_id = $1"),
        n("SELECT count(*) AS n FROM users WHERE name = 'Unknown (Zendesk)' AND $1 IS NOT NULL"),
    )
}

async fn refused(pool: &TestPool, bundle: &Bundle, opts: &LoadOptions) -> String {
    match loader::load(pool, storage(), bundle, opts, &|_| {}).await {
        Err(loader::LoadFailure {
            error: loader::LoadError::Refused(why),
            ..
        }) => why,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[actix_web::test]
async fn a_tickets_only_export_refuses_and_writes_nothing() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_from(dir.path(), &[], &tickets(), Some(PeopleSource::Export));
    let job = Uuid::now_v7();
    let opts = options(&ws, job);

    // The plan still shows, with who is missing and what they touch.
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    let missing = plan.missing_people.clone().expect("missing");
    assert_eq!(
        (missing.people, missing.tickets, missing.replies),
        (4, 2, 5)
    );
    assert_eq!(missing.label, "not in the export");
    let why = plan.refused.clone().expect("a run would refuse");
    assert!(why.contains("--allow-missing-people"), "{why}");
    assert!(why.contains("users export"), "{why}");
    let text = backend::services::imports::helpdesk::report::render_plan(&plan);
    assert!(text.contains("not in the export (placeholder)"), "{text}");

    let why = refused(&pool, &bundle, &opts).await;
    assert!(
        why.contains(
            "4 people the bundle doesn't hold (not in the export), on 2 tickets and 5 replies"
        ),
        "{why}"
    );
    assert!(imported_tickets(&pool, &ws).is_empty());
    assert_eq!(
        traces(&pool, job),
        (0, 0, 0),
        "no job, id map or placeholder"
    );
    // Nor were the kept numbers (up to 102) reserved: the workspace's next
    // ticket is its first.
    assert_eq!(ordinary_ticket(&pool, &ws, "After the refusal").number, 1);
}

#[actix_web::test]
async fn a_missing_watcher_or_reply_author_alone_still_refuses() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let opts = options(&ws, Uuid::now_v7());
    let cast = vec![person(
        "1",
        "one@customer.test",
        PersonRole::Requester,
        true,
    )];

    // Only a watcher is missing.
    let dir = tempfile::tempdir().expect("dir");
    let watched = vec![raised_by("401", "1", &["9"])];
    let bundle = write_bundle_from(dir.path(), &cast, &watched, Some(PeopleSource::Export));
    let why = refused(&pool, &bundle, &opts).await;
    assert!(
        why.contains(
            "1 person the bundle doesn't hold (not in the export), on 1 ticket and 0 replies"
        ),
        "{why}"
    );

    // Only a reply's author is missing.
    let dir = tempfile::tempdir().expect("dir");
    let mut answered = raised_by("402", "1", &[]);
    answered
        .comments
        .push(comment("c402b", Some("9"), true, "<p>Reply</p>", at(1, 11)));
    let bundle = write_bundle_from(dir.path(), &cast, &[answered], Some(PeopleSource::Export));
    let why = refused(&pool, &bundle, &opts).await;
    assert!(
        why.contains(
            "1 person the bundle doesn't hold (not in the export), on 1 ticket and 1 reply"
        ),
        "{why}"
    );
    assert!(imported_tickets(&pool, &ws).is_empty());
}

#[actix_web::test]
async fn a_run_stopped_while_the_bundle_is_read_writes_nothing() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());
    let job = Uuid::now_v7();
    let opts = options(&ws, job);
    opts.cancel.cancel();
    let failed = loader::load(&pool, storage(), &bundle, &opts, &|_| {})
        .await
        .expect_err("stopped");
    assert!(
        matches!(failed.error, loader::LoadError::Cancelled),
        "{failed:?}"
    );
    assert_eq!(traces(&pool, job), (0, 0, 0));
    assert_eq!(ordinary_ticket(&pool, &ws, "After the stop").number, 1);
}

#[actix_web::test]
async fn allowed_missing_people_land_on_the_placeholder_and_the_note_names_them() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_from(dir.path(), &[], &tickets(), Some(PeopleSource::Export));
    let mut opts = options(&ws, Uuid::now_v7());
    opts.allow_missing_people = true;

    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.refused, None);
    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 2);
    assert_eq!(report.comments_written, 5);
    let missing = report.missing_people.clone().expect("missing");
    assert_eq!(
        (missing.people, missing.label.as_str()),
        (4, "not in the export")
    );
    let text = backend::services::imports::helpdesk::report::render_report(&report);
    assert!(text.contains("not in the export (placeholder)"), "{text}");

    let rows = imported_tickets(&pool, &ws);
    assert_eq!(rows[0].assignee_uuid, None);
    let thread = comments_of(&pool, &ws, rows[0].id);
    let note = &thread[0];
    assert!(note.is_internal && note.content.contains("Imported from Zendesk #101"));
    assert!(
        note.content.contains(
            "Requested there by Zendesk user u-req (not in the export); credited to the \
             placeholder author."
        ),
        "{}",
        note.content
    );
    assert!(
        note.content.contains(
            "It was assigned to Zendesk user u-agent (not in the export), so it is unassigned."
        ),
        "{}",
        note.content
    );
    assert!(
        thread[1..].iter().all(|c| c.author == "Unknown (Zendesk)"),
        "{thread:?}"
    );
    assert!(watchers_of(&pool, &ws, 102).is_empty());
}

#[actix_web::test]
async fn people_gone_at_zendesk_need_no_flag() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let mut list = tickets();
    list[1].requester = Some("u-ghost".into());
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_from(dir.path(), &people(), &list, Some(PeopleSource::Api));
    let opts = options(&ws, Uuid::now_v7());

    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.refused, None);
    let missing = plan.missing_people.clone().expect("missing");
    assert_eq!(
        (missing.people, missing.tickets, missing.replies),
        (1, 1, 1)
    );
    assert_eq!(missing.label, "gone at Zendesk");

    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 2);
    assert_eq!(
        report.missing_people.map(|m| m.label),
        Some("gone at Zendesk".to_string())
    );
    let thread = comments_of(&pool, &ws, ticket_id(&pool, &ws, 102));
    assert!(
        thread[0]
            .content
            .contains("Requested there by Zendesk user u-ghost (gone at Zendesk)"),
        "{}",
        thread[0].content
    );
}

#[actix_web::test]
async fn an_unreadable_people_line_means_absent_people_are_not_confirmed_gone() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    // An API bundle whose people.jsonl has a line that isn't a person: it
    // may have been u-ghost, so u-ghost isn't confirmed gone at Zendesk.
    let bundle = write_bundle(dir.path(), &tickets());
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(bundle.root().join("people.jsonl"))
            .expect("people");
        writeln!(f, "{{\"source_id\": \"u-ghost\", \"name\": ").expect("line");
    }
    let job = Uuid::now_v7();
    let mut opts = options(&ws, job);

    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    let missing = plan.missing_people.clone().expect("missing");
    assert_eq!(
        (missing.people, missing.label.as_str()),
        (1, "unreadable in the bundle")
    );
    assert!(plan.refused.is_some());
    let why = refused(&pool, &bundle, &opts).await;
    assert!(why.contains("(unreadable in the bundle)"), "{why}");
    assert!(why.contains("--refetch"), "{why}");
    assert!(imported_tickets(&pool, &ws).is_empty());
    assert_eq!(traces(&pool, job), (0, 0, 0));

    // With the flag it lands, and the report says why they're missing.
    opts.allow_missing_people = true;
    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 2);
    assert_eq!(report.unreadable_lines, 1);
    assert_eq!(
        report.missing_people.map(|m| m.label),
        Some("unreadable in the bundle".to_string())
    );
}

#[actix_web::test]
async fn a_bundle_that_predates_the_people_source_refuses() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle(dir.path(), &tickets());
    // As #1176 wrote it: no people_source at all.
    std::fs::write(
        bundle.root().join("manifest.json"),
        format!(
            r#"{{"format_version": {FORMAT_VERSION}, "source": "zendesk", "source_scope": "acme",
                "extracted_at": "2026-11-26T00:00:00Z", "counts": {{"tickets": 2}}, "notes": []}}"#
        ),
    )
    .expect("manifest");
    let job = Uuid::now_v7();
    let opts = options(&ws, job);

    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    let missing = plan.missing_people.clone().expect("missing");
    assert_eq!(
        (missing.people, missing.label.as_str()),
        (1, "not in the bundle")
    );
    let why = refused(&pool, &bundle, &opts).await;
    assert!(why.contains("--refetch"), "{why}");
    assert!(why.contains("(not in the bundle)"), "{why}");
    assert!(imported_tickets(&pool, &ws).is_empty());
    assert_eq!(traces(&pool, job), (0, 0, 0));
}

// ---------------------------------------------------------------- Phase 1:
// people and organizations

/// A bundle as the connector writes it now, with its organizations and the
/// manifest as `tweak` leaves it.
fn write_bundle_with(
    dir: &std::path::Path,
    people: &[Person],
    tickets: &[Ticket],
    orgs: &[Organization],
    tweak: impl FnOnce(&mut Manifest),
) -> Bundle {
    let b = write_bundle_from(dir, people, tickets, Some(PeopleSource::Export));
    let mut w = b.organizations_writer().expect("orgs");
    for o in orgs {
        w.write(o).expect("org");
    }
    w.checkpoint().expect("flush");
    let mut m = b.read_manifest().expect("manifest");
    tweak(&mut m);
    b.write_manifest(&m).expect("manifest");
    b
}

fn org(id: &str, name: &str) -> Organization {
    Organization {
        source_id: id.into(),
        name: name.into(),
        ..Default::default()
    }
}

/// `person` with an organization and a phone at the source.
fn customer(id: &str, email: &str, organization: Option<&str>, phone: Option<&str>) -> Person {
    Person {
        organization: organization.map(str::to_string),
        organization_id: organization.map(|_| format!("org-{id}")),
        phone: phone.map(str::to_string),
        ..person(id, email, PersonRole::Requester, true)
    }
}

#[derive(QueryableByName, Debug, PartialEq)]
struct ProfileRow {
    #[diesel(sql_type = Nullable<Text>)]
    organization: Option<String>,
    #[diesel(sql_type = Bool)]
    directory_synced: bool,
}

/// The account's profile in `ws`, read pinned to it.
fn profile_of(pool: &TestPool, ws: i32, user: Uuid) -> Option<ProfileRow> {
    run_in_workspace(pool, REF, ws, |c| {
        diesel::sql_query(
            "SELECT organization, directory_synced FROM user_profiles \
             WHERE user_uuid = $1 AND workspace_id = $2",
        )
        .bind::<diesel::sql_types::Uuid, _>(user)
        .bind::<Integer, _>(ws)
        .get_result::<ProfileRow>(c)
        .optional()
    })
    .expect("profile")
}

#[derive(QueryableByName, Debug, PartialEq)]
struct PhoneRow {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    phone: String,
    #[diesel(sql_type = Text)]
    phone_type: String,
    #[diesel(sql_type = Bool)]
    is_primary: bool,
    #[diesel(sql_type = Nullable<Text>)]
    source: Option<String>,
}

fn phones_of(pool: &TestPool, ws: i32, user: Uuid) -> Vec<PhoneRow> {
    run_in_workspace(pool, REF, ws, |c| {
        diesel::sql_query(
            "SELECT id, phone, phone_type, is_primary, source FROM user_phone_numbers \
             WHERE user_uuid = $1 AND workspace_id = $2 ORDER BY id",
        )
        .bind::<diesel::sql_types::Uuid, _>(user)
        .bind::<Integer, _>(ws)
        .load::<PhoneRow>(c)
    })
    .expect("phones")
}

/// Id map rows of `entity` for source id `id`, in `ws`.
fn mapped(pool: &TestPool, ws: i32, entity: &str, id: &str) -> Vec<String> {
    #[derive(QueryableByName)]
    struct T {
        #[diesel(sql_type = Text)]
        target_id: String,
    }
    let mut conn = pool.get().expect("conn");
    diesel::sql_query(
        "SELECT target_id FROM import_source_ids \
         WHERE workspace_id = $1 AND entity = $2 AND source_id = $3",
    )
    .bind::<Integer, _>(ws)
    .bind::<Text, _>(entity)
    .bind::<Text, _>(id)
    .load::<T>(&mut conn)
    .expect("id map")
    .into_iter()
    .map(|t| t.target_id)
    .collect()
}

fn addresses(pool: &TestPool, email: &str) -> i64 {
    let mut conn = pool.get().expect("conn");
    diesel::sql_query("SELECT count(*) AS n FROM user_emails WHERE lower(email) = lower($1)")
        .bind::<Text, _>(email)
        .get_result::<Count>(&mut conn)
        .expect("count")
        .n
}

/// The import's metadata on each reply of `number`, oldest first.
fn reply_metadata(pool: &TestPool, ws: &WorkspaceSeed, number: i32) -> Vec<serde_json::Value> {
    #[derive(QueryableByName)]
    struct M {
        #[diesel(sql_type = diesel::sql_types::Jsonb)]
        meta: serde_json::Value,
    }
    run_in_workspace(pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "SELECT c.channel_metadata->'import' AS meta FROM comments c \
             JOIN tickets t ON t.id = c.ticket_id \
             WHERE t.number = $1 AND c.channel_metadata->'import'->>'source_id' NOT LIKE 'import-note:%' \
             ORDER BY c.created_at, c.id",
        )
        .bind::<Integer, _>(number)
        .load::<M>(c)
    })
    .expect("metadata")
    .into_iter()
    .map(|m| m.meta)
    .collect()
}

fn quiet(pool: &TestPool, job: Uuid) {
    for outbox in ["notification_outbox", "webhook_outbox", "rule_event_outbox"] {
        let n = count(
            pool,
            &format!(
                "SELECT count(*) AS n FROM {outbox} o JOIN sync_actions s ON s.sync_id = o.sync_id \
                 WHERE s.actor_ref = $1"
            ),
            job,
        );
        assert_eq!(n, 0, "{outbox} got {n} rows from the import");
    }
}

#[actix_web::test]
async fn deleted_person_keeps_name_not_address() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    // One deleted person's address is a member's here; the other's is free,
    // and a third's belongs to an account from elsewhere.
    let member = account(
        &pool,
        "Mo Member",
        None,
        "mo@customer.test",
        &[],
        Some((ws.workspace_id, "member")),
    );
    let elsewhere = account(&pool, "Else", None, "else@customer.test", &[], None);
    let deleted = |id: &str, email: &str, name: &str| Person {
        name: name.into(),
        deleted: true,
        active: false,
        phone: None,
        ..customer(id, email, Some("Acme"), None)
    };
    let cast = vec![
        deleted("d1", "dee@customer.test", "Dee Deleted"),
        deleted("d2", "mo@customer.test", "Mo At Zendesk"),
        deleted("d3", "else@customer.test", "Else At Zendesk"),
        person("a1", "agent@acme.test", PersonRole::Agent, true),
    ];
    let mut t501 = raised_by("501", "d1", &[]);
    t501.assignee = Some("d1".into());
    t501.comments.push(comment(
        "c501b",
        Some("a1"),
        true,
        "<p>On it</p>",
        at(1, 11),
    ));
    let tickets = vec![
        t501,
        raised_by("502", "d2", &[]),
        raised_by("503", "d3", &[]),
    ];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, &tickets, &[], |_| {});
    let job = Uuid::now_v7();
    let opts = options(&ws, job);

    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.refused, None);
    assert_eq!(plan.people_detail.deleted, 3);
    assert_eq!(
        plan.people.get("deleted at the source (placeholder)"),
        Some(&2),
        "{:?}",
        plan.people
    );
    let text = backend::services::imports::helpdesk::report::render_plan(&plan);
    assert!(
        text.contains("Deleted at Zendesk: 3, history kept under their name, no address stored"),
        "{text}"
    );

    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 3);
    quiet(&pool, job);
    // No account, address or profile for the deleted person.
    assert_eq!(addresses(&pool, "dee@customer.test"), 0);
    assert!(mapped(&pool, ws.workspace_id, "person", "d1").is_empty());
    assert!(mapped(&pool, ws.workspace_id, "profile", "d1").is_empty());
    // Their history: the placeholder, with their name, not their address.
    let reqs = requesters(&pool, &ws);
    assert_ne!(reqs[&501], member);
    let thread = comments_of(&pool, &ws, ticket_id(&pool, &ws, 501));
    assert_eq!(thread[1].author, "Unknown (Zendesk)");
    let note = &thread[0].content;
    assert!(
        note.contains(
            "Requested there by Dee Deleted (deleted at Zendesk); credited to the placeholder author."
        ),
        "{note}"
    );
    assert!(
        note.contains("It was assigned to Dee Deleted (deleted at Zendesk), so it is unassigned."),
        "{note}"
    );
    assert!(!note.contains("dee@customer.test"), "{note}");
    let meta = reply_metadata(&pool, &ws, 501);
    assert_eq!(meta[0]["author_name"], "Dee Deleted");
    assert!(meta[0]["author_email"].is_null(), "{}", meta[0]);
    // The member here keeps the deleted person's history; their profile is
    // left alone.
    assert_eq!(reqs[&502], member);
    assert_eq!(profile_of(&pool, ws.workspace_id, member), None);
    // An account from elsewhere is never reached on a deleted person's word.
    assert_ne!(reqs[&503], elsewhere);
    assert_eq!(seat_of(&pool, ws.workspace_id, elsewhere), None);
}

#[actix_web::test]
async fn shared_address_is_one_account_in_any_batch_size() {
    use chrono::Duration;
    for batch in [1usize, 500] {
        let db = TestDb::new();
        let pool = db.pool_with_size(4);
        crate::common::ensure_test_keyring();
        let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
        let at_day = |d: i64| Some(at(1, 0) + Duration::days(d));
        let sharer = |id: &str, role, created: i64, name: &str| Person {
            name: name.into(),
            created_at: at_day(created),
            ..person(id, "Same@Customer.test", role, true)
        };
        let late = sharer("s1", PersonRole::Requester, 2, "Late Name");
        let early_agent = sharer("s2", PersonRole::Agent, 1, "Early Name");
        let gone = Person {
            deleted: true,
            active: false,
            ..sharer("s3", PersonRole::Requester, 0, "Gone Name")
        };
        let cast = vec![
            late,
            person("f1", "f1@customer.test", PersonRole::Requester, true),
            early_agent,
            person("f2", "f2@customer.test", PersonRole::Requester, true),
            gone,
        ];
        let mut t = raised_by("601", "s1", &["s2", "f1"]);
        t.comments.push(comment(
            "c601b",
            Some("s3"),
            true,
            "<p>Me too</p>",
            at(1, 12),
        ));
        let tickets = vec![t, raised_by("602", "f2", &[])];
        let dir = tempfile::tempdir().expect("dir");
        let bundle = write_bundle_with(dir.path(), &cast, &tickets, &[], |_| {});
        let mut opts = options(&ws, Uuid::now_v7());
        opts.people_batch_size = batch;

        let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
        assert_eq!(
            (
                plan.people_detail.shared_addresses,
                plan.people_detail.shared_address_people
            ),
            (1, 3)
        );
        run(&pool, &bundle, &opts).await;
        assert_eq!(addresses(&pool, "same@customer.test"), 1, "batch {batch}");
        let account = account_of(&pool, "same@customer.test");
        // The least privilege: a requester, never created as an agent.
        assert_eq!(
            seat_of(&pool, ws.workspace_id, account),
            Some(("member".to_string(), false)),
            "batch {batch}"
        );
        // Named for the earliest-created active member.
        let name: String = {
            #[derive(QueryableByName)]
            struct N {
                #[diesel(sql_type = Text)]
                name: String,
            }
            diesel::sql_query("SELECT name FROM users WHERE uuid = $1")
                .bind::<diesel::sql_types::Uuid, _>(account)
                .get_result::<N>(&mut pool.get().expect("conn"))
                .expect("name")
                .name
        };
        assert_eq!(name, "Early Name", "batch {batch}");
        // Everyone at the address maps to it; each reply keeps its own name.
        for id in ["s1", "s2", "s3"] {
            assert_eq!(
                mapped(&pool, ws.workspace_id, "person", id),
                vec![account.to_string()],
                "{id}, batch {batch}"
            );
        }
        assert_eq!(requesters(&pool, &ws)[&601], account);
        let meta = reply_metadata(&pool, &ws, 601);
        assert_eq!(meta[1]["author_name"], "Gone Name");
    }
}

#[actix_web::test]
async fn organization_filled_once_and_cleared_stays_cleared() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let cast = vec![customer("o1", "olive@customer.test", Some("Acme"), None)];
    let tickets = vec![raised_by("701", "o1", &[])];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(
        dir.path(),
        &cast,
        &tickets,
        &[org("org-o1", "Acme")],
        |_| {},
    );
    let job = Uuid::now_v7();
    let opts = options(&ws, job);
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.people_detail.organizations, 1);
    assert_eq!(plan.people_detail.profile.organizations, 1);

    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.profile.organizations, 1);
    quiet(&pool, job);
    let olive = account_of(&pool, "olive@customer.test");
    assert_eq!(
        profile_of(&pool, ws.workspace_id, olive).and_then(|p| p.organization),
        Some("Acme".to_string())
    );
    // Only in this workspace.
    let other = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).b;
    assert_eq!(profile_of(&pool, other.workspace_id, olive), None);
    assert_eq!(
        mapped(&pool, ws.workspace_id, "profile", "o1"),
        vec![olive.to_string()]
    );

    // Cleared here: a rerun leaves it cleared.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("UPDATE user_profiles SET organization = NULL WHERE user_uuid = $1")
            .bind::<diesel::sql_types::Uuid, _>(olive)
            .execute(c)
    })
    .expect("clear");
    let plan = loader::plan(&pool, &bundle, &options(&ws, Uuid::now_v7()))
        .await
        .expect("plan");
    assert_eq!(plan.people_detail.profile.organizations, 0);
    let again = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!(again.profile, Default::default());
    assert_eq!(
        profile_of(&pool, ws.workspace_id, olive).and_then(|p| p.organization),
        None
    );
}

#[actix_web::test]
async fn organization_skips_directory_synced_profile() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let synced = account(
        &pool,
        "Sid Synced",
        None,
        "sid@customer.test",
        &[],
        Some((ws.workspace_id, "member")),
    );
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO user_profiles (user_uuid, directory_synced, custom_fields) \
             VALUES ($1, true, '{}'::jsonb)",
        )
        .bind::<diesel::sql_types::Uuid, _>(synced)
        .execute(c)
    })
    .expect("synced profile");
    let cast = vec![customer(
        "y1",
        "sid@customer.test",
        Some("Zendesk Org"),
        Some("+61299990000"),
    )];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(
        dir.path(),
        &cast,
        &[raised_by("801", "y1", &[])],
        &[org("org-y1", "Zendesk Org")],
        |_| {},
    );
    let opts = options(&ws, Uuid::now_v7());
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.people_detail.profile.directory_synced, 1);
    assert_eq!(plan.people_detail.profile.organizations, 0);
    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.profile.directory_synced, 1);
    assert_eq!(
        profile_of(&pool, ws.workspace_id, synced),
        Some(ProfileRow {
            organization: None,
            directory_synced: true
        })
    );
    assert!(phones_of(&pool, ws.workspace_id, synced).is_empty());
}

#[actix_web::test]
async fn people_from_1176_get_profile_on_rerun() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    // As #1176 brought them across: no organization or phone written.
    let then = vec![customer("p1", "pat@customer.test", None, None)];
    let tickets = vec![raised_by("901", "p1", &[])];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &then, &tickets, &[], |m| {
        m.person_detail = 0;
    });
    let mut first = options(&ws, Uuid::now_v7());
    first.create_agents = false;
    run(&pool, &bundle, &first).await;
    let pat = account_of(&pool, "pat@customer.test");
    assert!(mapped(&pool, ws.workspace_id, "profile", "p1").is_empty());

    // A fresh bundle carries them; the rerun fills the profile once.
    let now = vec![customer(
        "p1",
        "pat@customer.test",
        Some("Acme"),
        Some("+61 2 9999 0000"),
    )];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &now, &tickets, &[org("org-p1", "Acme")], |_| {});
    let plan = loader::plan(&pool, &bundle, &options(&ws, Uuid::now_v7()))
        .await
        .expect("plan");
    assert_eq!(plan.people.get("already imported"), Some(&1));
    assert_eq!(
        (
            plan.people_detail.profile.organizations,
            plan.people_detail.profile.phones
        ),
        (1, 1)
    );
    let report = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!(
        (report.profile.organizations, report.profile.phones),
        (1, 1)
    );
    assert_eq!(
        profile_of(&pool, ws.workspace_id, pat).and_then(|p| p.organization),
        Some("Acme".to_string())
    );
    assert_eq!(phones_of(&pool, ws.workspace_id, pat).len(), 1);
}

#[actix_web::test]
async fn phone_added_once_and_deleted_stays_deleted() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    // One with a phone here already, one without; a shared number isn't
    // carried at all.
    let has_one = account(
        &pool,
        "Hal",
        None,
        "hal@customer.test",
        &[],
        Some((ws.workspace_id, "member")),
    );
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query(
            "INSERT INTO user_phone_numbers (user_uuid, phone, phone_type, is_primary) \
             VALUES ($1, '+1 555 0100', 'work', true)",
        )
        .bind::<diesel::sql_types::Uuid, _>(has_one)
        .execute(c)
    })
    .expect("phone");
    let cast = vec![
        customer("n1", "nia@customer.test", None, Some("+61299990000")),
        customer("n2", "hal@customer.test", None, Some("+61288880000")),
        Person {
            shared_phone: true,
            ..customer("n3", "sam@customer.test", None, None)
        },
    ];
    let tickets = vec![
        raised_by("1001", "n1", &[]),
        raised_by("1002", "n2", &[]),
        raised_by("1003", "n3", &[]),
    ];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, &tickets, &[], |_| {});
    let opts = options(&ws, Uuid::now_v7());
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(
        (
            plan.people_detail.profile.phones,
            plan.people_detail.profile.phones_kept,
            plan.people_detail.shared_phones
        ),
        (1, 1, 1)
    );
    let report = run(&pool, &bundle, &opts).await;
    assert_eq!((report.profile.phones, report.profile.phones_kept), (1, 1));
    let nia = account_of(&pool, "nia@customer.test");
    let phones = phones_of(&pool, ws.workspace_id, nia);
    assert_eq!(phones.len(), 1);
    let p = &phones[0];
    assert_eq!(
        (
            p.phone.as_str(),
            p.phone_type.as_str(),
            p.is_primary,
            p.source.as_deref()
        ),
        ("+61299990000", "other", true, None),
        "a manual row, editable here"
    );
    assert_eq!(
        mapped(&pool, ws.workspace_id, "phone", "n1"),
        vec![p.id.to_string()]
    );
    assert_eq!(phones_of(&pool, ws.workspace_id, has_one).len(), 1);
    assert!(phones_of(
        &pool,
        ws.workspace_id,
        account_of(&pool, "sam@customer.test")
    )
    .is_empty());

    // Deleted here: not added again.
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("DELETE FROM user_phone_numbers WHERE id = $1")
            .bind::<Integer, _>(p.id)
            .execute(c)
    })
    .expect("delete");
    run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert!(phones_of(&pool, ws.workspace_id, nia).is_empty());
}

#[actix_web::test]
async fn purge_then_rerun_brings_the_person_back() {
    use backend::sync::actor::ActorContext;
    use backend::sync::session::{pin_workspace, with_actor_bypass_context};
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let cast = vec![
        customer(
            "q1",
            "quin@customer.test",
            Some("Acme"),
            Some("+61299990000"),
        ),
        customer(
            "q2",
            "ray@customer.test",
            Some("Beta"),
            Some("+61288880000"),
        ),
    ];
    let tickets = vec![raised_by("1101", "q1", &[]), raised_by("1102", "q2", &[])];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, &tickets, &[], |_| {});
    run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    let quin = account_of(&pool, "quin@customer.test");
    for entity in ["person", "profile", "phone"] {
        assert_eq!(
            mapped(&pool, ws.workspace_id, entity, "q1").len(),
            1,
            "{entity}"
        );
    }

    // The purge forgets the person, their profile and their phone.
    let mut conn = pool.get().expect("conn");
    with_actor_bypass_context(&mut conn, &ActorContext::system(REF), |c| {
        pin_workspace(c, ws.workspace_id)?;
        backend::repository::users::purge_user(&quin, c, None)
    })
    .expect("purge");
    drop(conn);
    for entity in ["person", "profile", "phone"] {
        assert!(
            mapped(&pool, ws.workspace_id, entity, "q1").is_empty(),
            "{entity}"
        );
    }
    // Ray's mapping points at an account that is no longer one of the
    // workspace's people: the stale forget clears all three too.
    {
        let mut conn = pool.get().expect("conn");
        diesel::sql_query(
            "UPDATE import_source_ids SET target_id = $2 \
             WHERE workspace_id = $1 AND source_id = 'q2' AND entity IN ('person', 'profile')",
        )
        .bind::<Integer, _>(ws.workspace_id)
        .bind::<Text, _>(Uuid::now_v7().to_string())
        .execute(&mut conn)
        .expect("stale");
        diesel::sql_query("DELETE FROM user_emails WHERE lower(email) = 'ray@customer.test'")
            .execute(&mut conn)
            .expect("free the address");
    }

    // A rerun brings them back, profile and phone included, without
    // tripping over a mapping left behind; the plan says so first.
    let plan = loader::plan(&pool, &bundle, &options(&ws, Uuid::now_v7()))
        .await
        .expect("plan");
    assert_eq!(
        (
            plan.people_detail.profile.organizations,
            plan.people_detail.profile.phones
        ),
        (2, 2)
    );
    let report = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert!(
        report.people_problems.is_empty(),
        "{:?}",
        report.people_problems
    );
    assert!(
        report.profile_problems.is_empty(),
        "{:?}",
        report.profile_problems
    );
    for (id, email, org) in [
        ("q1", "quin@customer.test", "Acme"),
        ("q2", "ray@customer.test", "Beta"),
    ] {
        let back = account_of(&pool, email);
        assert_ne!(back, quin);
        assert_eq!(
            mapped(&pool, ws.workspace_id, "person", id),
            vec![back.to_string()]
        );
        assert_eq!(
            profile_of(&pool, ws.workspace_id, back).and_then(|p| p.organization),
            Some(org.to_string())
        );
        assert_eq!(phones_of(&pool, ws.workspace_id, back).len(), 1, "{id}");
    }
}

#[actix_web::test]
async fn changed_at_zendesk_reported_not_applied() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let then = Person {
        name: "Old Name".into(),
        updated_at: Some(at(1, 9)),
        ..person("c1", "old@customer.test", PersonRole::Requester, true)
    };
    let tickets = vec![raised_by("1201", "c1", &[])];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(
        dir.path(),
        std::slice::from_ref(&then),
        &tickets,
        &[],
        |_| {},
    );
    run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    let account = account_of(&pool, "old@customer.test");

    // Renamed and readdressed at Zendesk since.
    let now = Person {
        name: "New Name".into(),
        email: Some("new@customer.test".into()),
        updated_at: Some(at(5, 9)),
        ..then
    };
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &[now], &tickets, &[], |_| {});
    let plan = loader::plan(&pool, &bundle, &options(&ws, Uuid::now_v7()))
        .await
        .expect("plan");
    assert_eq!(plan.people_detail.changed_at_source.len(), 1);
    let text = backend::services::imports::helpdesk::report::render_plan(&plan);
    assert!(
        text.contains("Changed at Zendesk since the last import: 1 person (not applied)"),
        "{text}"
    );
    let report = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert_eq!(
        report.changed_at_source,
        vec!["New Name (source id c1)".to_string()]
    );
    assert_eq!(addresses(&pool, "new@customer.test"), 0);
    assert_eq!(account_of(&pool, "old@customer.test"), account);
    let names = comments_of(&pool, &ws, ticket_id(&pool, &ws, 1201));
    assert_eq!(names[1].author, "Old Name");
}

#[actix_web::test]
async fn old_bundle_refuses_create_agents() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &people(), &tickets(), &[], |m| {
        m.people_source = Some(PeopleSource::Api);
        m.person_detail = 0;
    });
    let job = Uuid::now_v7();
    let mut opts = options(&ws, job);
    assert!(opts.create_agents);
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    let why = plan.refused.clone().expect("refused");
    assert!(
        why.contains("--create-agents") && why.contains("--refetch"),
        "{why}"
    );
    let why = refused(&pool, &bundle, &opts).await;
    assert!(why.contains("limited staff"), "{why}");
    assert_eq!(traces(&pool, job), (0, 0, 0));

    // Without the flag it goes ahead.
    opts.create_agents = false;
    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 2);
}

#[actix_web::test]
async fn requester_organization_rule_matches() {
    use backend::models::RuleTriggerKind;
    use backend::services::rules::conditions;
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let cast = vec![
        customer("r1", "rita@acme.test", Some("Acme Corp"), None),
        customer("r2", "ben@beta.test", Some("Beta"), None),
    ];
    let tickets = vec![raised_by("1301", "r1", &[]), raised_by("1302", "r2", &[])];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, &tickets, &[], |_| {});
    run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;

    let predicate = conditions::compile(
        &serde_json::json!({
            "kind": "leaf", "field": "ticket.requester.organization", "op": "eq", "value": "acme corp"
        }),
        RuleTriggerKind::TicketCreated,
    )
    .expect("conditions");
    let ids = [ticket_id(&pool, &ws, 1301), ticket_id(&pool, &ws, 1302)];
    let facts = run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        backend::repository::rules::load_ticket_facts(c, &ids)
    })
    .expect("facts");
    let matched: Vec<i32> = facts
        .iter()
        .filter(|f| predicate.matches(f, None))
        .map(|f| f.id)
        .collect();
    assert_eq!(matched, vec![ids[0]]);
}

#[actix_web::test]
async fn people_the_fill_found_gone_need_no_flag_but_the_rest_refuse() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let cast = vec![person(
        "1",
        "one@customer.test",
        PersonRole::Requester,
        true,
    )];
    let mut t = raised_by("1401", "1", &["ghost"]);
    t.comments.push(comment(
        "c1401b",
        Some("ghost"),
        true,
        "<p>Hi</p>",
        at(1, 11),
    ));
    let lost = raised_by("1402", "lost", &[]);
    let fill = |m: &mut Manifest| {
        m.fill = Some(Fill {
            people_asked: 2,
            gone: vec!["ghost".into()],
            ..Default::default()
        });
    };

    // Only the one the fill confirmed gone: no flag needed.
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, std::slice::from_ref(&t), &[], fill);
    let opts = options(&ws, Uuid::now_v7());
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.refused, None);
    assert_eq!(plan.missing_people, None);
    let gone = plan.gone_people.clone().expect("gone");
    assert_eq!((gone.people, gone.tickets, gone.replies), (1, 1, 1));
    let text = backend::services::imports::helpdesk::report::render_plan(&plan);
    assert!(
        text.contains("Gone at Zendesk (asked, not returned): 1, on 1 ticket and 1 reply;"),
        "{text}"
    );
    assert!(text.contains("Filled from the API: 0 people"), "{text}");

    // Someone else the export lacks and the fill never confirmed: refused.
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, &[t.clone(), lost], &[], fill);
    let job = Uuid::now_v7();
    let mut opts = options(&ws, job);
    let why = refused(&pool, &bundle, &opts).await;
    assert!(
        why.contains("1 person the bundle doesn't hold (not in the export), on 1 ticket"),
        "{why}"
    );
    assert_eq!(traces(&pool, job), (0, 0, 0));
    opts.allow_missing_people = true;
    let report = run(&pool, &bundle, &opts).await;
    assert_eq!(report.tickets_written, 2);
    let note = &comments_of(&pool, &ws, ticket_id(&pool, &ws, 1402))[0].content;
    assert!(
        note.contains("Requested there by Zendesk user lost (not in the export)"),
        "{note}"
    );
}

/// The plan an operator sees for a bundle shaped as the export reader and
/// the API fill write one: printed with `--nocapture` for the record.
#[actix_web::test]
async fn the_plan_reads_as_the_operator_sees_it() {
    use backend::services::imports::helpdesk::bundle::{InputFile, InputSet};
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let mut light = person("l1", "light@acme.test", PersonRole::Agent, true);
    light.limited_staff = true;
    let mut custom = person("k1", "custom@acme.test", PersonRole::Agent, true);
    custom.custom_role = true;
    let mut dee = customer("d1", "dee@customer.test", None, None);
    dee.deleted = true;
    dee.active = false;
    let mut shared = customer("s9", "front@customer.test", None, None);
    shared.shared_phone = true;
    let cast = vec![
        Person {
            organization_id: Some("o1".into()),
            ..customer(
                "c1",
                "cara@customer.test",
                Some("Customer Co"),
                Some("+61299990000"),
            )
        },
        Person {
            organization_id: Some("o1".into()),
            ..customer("c2", "Cara@Customer.test", Some("Customer Co"), None)
        },
        Person {
            organization_id: Some("o9".into()),
            ..customer("c3", "nemo@customer.test", None, None)
        },
        shared,
        dee,
        light,
        custom,
    ];
    let mut big = raised_by("1501", "c1", &["c2", "s9"]);
    big.organization_id = Some("o1".into());
    big.assignee = Some("l1".into());
    big.comments.push(comment(
        "c1501b",
        Some("k1"),
        true,
        "<p>Hello</p>",
        at(1, 10),
    ));
    big.comments.push(comment(
        "c1501c",
        Some("ghost"),
        true,
        "<p>Old</p>",
        at(1, 11),
    ));
    let tickets = vec![
        big,
        raised_by("1502", "c3", &[]),
        raised_by("1503", "d1", &[]),
    ];
    let orgs = vec![
        Organization {
            domain_names: vec!["customer.test".into()],
            ..org("o1", "Customer Co")
        },
        Organization {
            shared_tickets: true,
            ..org("o2", "Sharing Co")
        },
    ];
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, &tickets, &orgs, |m| {
        m.inputs = Some(InputSet {
            zips: ["tickets.zip", "users.zip", "organizations.zip"]
                .iter()
                .map(|n| InputFile {
                    path: format!("/exports/{n}"),
                    size: 1,
                    sha256: (*n).into(),
                })
                .collect(),
            credential: true,
            api_fill: true,
        });
        m.fill = Some(Fill {
            people_asked: 3,
            people_returned: 2,
            gone: vec!["ghost".into()],
            organizations: 1,
            conversations: 1,
            failed: None,
        });
    });
    let plan = loader::plan(&pool, &bundle, &options(&ws, Uuid::now_v7()))
        .await
        .expect("plan");
    let text = backend::services::imports::helpdesk::report::render_plan(&plan);
    println!("{text}");
    for line in [
        "Read from: tickets.zip, users.zip, organizations.zip (+ API fill)",
        "People the tickets mention: 8",
        "Gone at Zendesk (asked, not returned): 1, on 1 ticket and 1 reply;",
        "Deleted at Zendesk: 1, history kept under their name, no address stored",
        "Shared addresses: 1 address used by 2 Zendesk users, one account each",
        "Limited staff (light agents, chat only, contributors): 1, no seat unless mapped",
        "Custom-role agents: 1 (check with --map-agent): custom@acme.test",
        "Organizations with someone brought across: 1",
        "As the person's organization: 1 (already set here: 0; directory-synced: 0)",
        "Named but not in the bundle, so no organization: 1 person",
        "Not carried: domain names (1 organization), notes, details, fields, tags",
        "Portal sharing by domain is off. At 1 non-free-mail domain, 2 tickets from organizations that didn't share at Zendesk would be shared if it were turned on",
        "Phones: 1 (already one here: 0; shared numbers skipped: 1)",
        "Not read: other addresses, user fields, notes, tags, photos, external ids, locale",
        "Filled from the API: 2 people, 1 organization, 1 conversation over 1 MB",
    ] {
        assert!(text.contains(line), "missing {line:?} in\n{text}");
    }
    // Limited staff are never made agents, even with --create-agents.
    let light = plan
        .staff
        .iter()
        .find(|(who, _, _)| who == "light@acme.test")
        .expect("light");
    assert_eq!(light.2, "former staff, no seat");
}

#[actix_web::test]
async fn the_portal_shows_the_source_name_on_placeholder_replies_never_the_address() {
    use actix_web::dev::Service;
    use actix_web::{test as http_test, web, App, HttpMessage};
    use backend::extractors::WorkspaceContext;
    use backend::handlers::portal::{PortalContext, PortalScope};
    use backend::middleware::RequestContext;
    use backend::sync::actor::ActorContext;
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let seeded = crate::common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (ws, b) = (seeded.a, seeded.b);
    // An address held by staff of another workspace: beyond reach, so its
    // replies go to the placeholder with the address kept on them.
    account(
        &pool,
        "Else",
        None,
        "else@customer.test",
        &[],
        Some((b.workspace_id, "agent")),
    );
    let cast = vec![
        person("r1", "rae@customer.test", PersonRole::Requester, true),
        Person {
            name: "Dee Deleted".into(),
            deleted: true,
            active: false,
            ..person("d1", "dee@customer.test", PersonRole::Requester, false)
        },
        Person {
            name: "Else At Zendesk".into(),
            ..person("w1", "else@customer.test", PersonRole::Requester, true)
        },
    ];
    let mut t = raised_by("1601", "r1", &[]);
    t.comments.push(comment(
        "c1601b",
        Some("d1"),
        true,
        "<p>From Dee</p>",
        at(1, 11),
    ));
    t.comments.push(comment(
        "c1601c",
        Some("w1"),
        true,
        "<p>From Else</p>",
        at(1, 12),
    ));
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, &[t], &[], |_| {});
    run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    let meta = reply_metadata(&pool, &ws, 1601);
    assert_eq!(
        meta[2]["author_email"], "else@customer.test",
        "kept for agents"
    );

    let rae = account_of(&pool, "rae@customer.test");
    let id = ticket_id(&pool, &ws, 1601);
    let workspace = WorkspaceContext {
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        slug: ws.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let portal = PortalContext {
        user_uuid: rae,
        workspace_id: ws.workspace_id,
        workspace_uuid: ws.workspace_uuid,
        scope: PortalScope::Full,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(rae, Some(corr)).with_workspace(ws.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(pool.clone()))
            .wrap_fn(move |req, srv| {
                req.extensions_mut().insert(workspace.clone());
                req.extensions_mut().insert(portal.clone());
                req.extensions_mut()
                    .insert(RequestContext::new(corr, actor.clone()));
                srv.call(req)
            })
            .route(
                "/api/portal/tickets/{id}",
                web::get().to(backend::handlers::portal::get_my_ticket),
            ),
    )
    .await;
    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::get()
            .uri(&format!("/api/portal/tickets/{id}"))
            .to_request(),
    )
    .await;
    assert!(resp.status().is_success(), "{}", resp.status());
    let body: serde_json::Value = http_test::read_body_json(resp).await;
    let text = body.to_string();
    assert!(!text.contains("else@customer.test"), "{text}");
    assert!(!text.contains("dee@customer.test"), "{text}");
    let authors: Vec<String> = body["comments"]
        .as_array()
        .expect("comments")
        .iter()
        .map(|c| c["author"]["name"].as_str().unwrap_or_default().to_string())
        .collect();
    assert_eq!(
        authors,
        vec!["Source r1", "Dee Deleted", "Else At Zendesk"],
        "{body}"
    );
}

#[actix_web::test]
async fn a_mapped_agents_organization_and_phone_never_land_on_the_staff_member() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let staff = account(
        &pool,
        "Stella Staff",
        None,
        "stella@here.test",
        &[],
        Some((ws.workspace_id, "agent")),
    );
    let agent = Person {
        organization: Some("Zendesk Inc".into()),
        organization_id: Some("o-z".into()),
        phone: Some("+61211110000".into()),
        ..person("a1", "zd@acme.test", PersonRole::Agent, true)
    };
    let cast = vec![customer("r1", "rob@customer.test", None, None), agent];
    let mut t = raised_by("1701", "r1", &[]);
    t.assignee = Some("a1".into());
    t.comments.push(comment(
        "c1701b",
        Some("a1"),
        true,
        "<p>Done</p>",
        at(1, 11),
    ));
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(dir.path(), &cast, &[t], &[], |_| {});
    let mut opts = options(&ws, Uuid::now_v7());
    opts.agent_map = HashMap::from([("zd@acme.test".to_string(), "stella@here.test".to_string())]);
    let plan = loader::plan(&pool, &bundle, &opts).await.expect("plan");
    assert_eq!(plan.people.get("agent, mapped to a seat"), Some(&1));
    assert_eq!(plan.people_detail.profile, Default::default());
    for _ in 0..2 {
        let mut o = opts.clone();
        o.job_id = Uuid::now_v7();
        let report = run(&pool, &bundle, &o).await;
        assert!(
            report.profile_problems.is_empty(),
            "{:?}",
            report.profile_problems
        );
        assert_eq!(profile_of(&pool, ws.workspace_id, staff), None);
        assert!(phones_of(&pool, ws.workspace_id, staff).is_empty());
    }
    assert_eq!(assignee_of(&pool, &ws, 1701), Some(staff));
    assert_eq!(
        mapped(&pool, ws.workspace_id, "profile", "a1"),
        vec![staff.to_string()],
        "done once, with nothing written"
    );
}

#[actix_web::test]
async fn someone_who_joins_a_shared_address_later_doesnt_fill_it_again() {
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    crate::common::ensure_test_keyring();
    let ws = crate::common::seed_two_workspaces(&mut pool.get().expect("conn")).a;
    let first = customer(
        "j1",
        "desk@customer.test",
        Some("Acme"),
        Some("+61299990000"),
    );
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(
        dir.path(),
        std::slice::from_ref(&first),
        &[raised_by("1801", "j1", &[])],
        &[],
        |_| {},
    );
    run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    let desk = account_of(&pool, "desk@customer.test");
    run_in_workspace(&pool, REF, ws.workspace_id, |c| {
        diesel::sql_query("UPDATE user_profiles SET organization = NULL WHERE user_uuid = $1")
            .bind::<diesel::sql_types::Uuid, _>(desk)
            .execute(c)?;
        diesel::sql_query("DELETE FROM user_phone_numbers WHERE user_uuid = $1")
            .bind::<diesel::sql_types::Uuid, _>(desk)
            .execute(c)
    })
    .expect("cleared here");

    // Another Zendesk user at the address, created earlier, so now the one
    // the group is decided as.
    let joiner = Person {
        created_at: Some(at(1, 0)),
        ..customer(
            "j2",
            "desk@customer.test",
            Some("Beta"),
            Some("+61288880000"),
        )
    };
    let dir = tempfile::tempdir().expect("dir");
    let bundle = write_bundle_with(
        dir.path(),
        &[first, joiner],
        &[raised_by("1801", "j1", &[]), raised_by("1802", "j2", &[])],
        &[],
        |_| {},
    );
    let plan = loader::plan(&pool, &bundle, &options(&ws, Uuid::now_v7()))
        .await
        .expect("plan");
    assert_eq!(plan.people_detail.profile, Default::default());
    let report = run(&pool, &bundle, &options(&ws, Uuid::now_v7())).await;
    assert!(
        report.profile_problems.is_empty(),
        "{:?}",
        report.profile_problems
    );
    assert!(
        report.people_problems.is_empty(),
        "{:?}",
        report.people_problems
    );
    assert_eq!(
        profile_of(&pool, ws.workspace_id, desk).and_then(|p| p.organization),
        None
    );
    assert!(phones_of(&pool, ws.workspace_id, desk).is_empty());
    assert_eq!(
        mapped(&pool, ws.workspace_id, "profile", "j2"),
        vec![desk.to_string()]
    );
    assert_eq!(
        mapped(&pool, ws.workspace_id, "person", "j2"),
        vec![desk.to_string()]
    );
}
