//! A database as 1.0.12 left it, holding realistic data, for upgrade tests.
//!
//! `UpgradeDb::at_1_0_12` is a fresh database migrated through the last
//! migration 1.0.12 shipped (`UpgradeDb::from_dump` restores a real one
//! instead). `seed` fills it the way a used 1.0.12 install with two
//! workspaces looks, and `seed_single_workspace` the way a Community install
//! with only its bootstrap workspace looks; both call one function per kind
//! of data, so a test can add its own the same way. Rows go in with the table's user triggers off, as a restore loads
//! them: no audit or sync side effects, timestamps exactly as given, foreign
//! keys still checked. `migrate_to_head` then runs every later migration as
//! the connecting superuser, one at a time, so a failure names its migration.
//!
//! Every timestamp sits in August 2026: 1.0.12 created `sync_actions`
//! partitions through August, and a row in the default partition would stop
//! the booting app from attaching that month's partition.

use std::collections::BTreeMap;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Integer};
use diesel_migrations::MigrationHarness;
use uuid::Uuid;

use backend::db::MIGRATIONS;

/// The last migration 1.0.12 shipped.
pub const LAST_1_0_12_MIGRATION: &str = "2026-06-18-100000_asset_serial_blank_is_null";

/// Tables whose row count an upgrade must not change.
pub const KEPT_TABLES: &[&str] = &[
    "workspaces",
    "users",
    "user_emails",
    "user_preferences",
    "workspace_members",
    "workflow_states",
    "working_calendars",
    "sla_policies",
    "ticket_categories",
    "site_settings",
    "tickets",
    "comments",
    "attachments",
    "ticket_watchers",
    "documentation_pages",
    "documentation_revisions",
    "api_tokens",
    "active_sessions",
    "refresh_tokens",
    "outbound_emails",
    "sync_actions",
];

/// A throwaway database, dropped with this value (even on panic).
pub struct UpgradeDb {
    name: String,
    pub url: String,
}

impl UpgradeDb {
    /// An empty database.
    fn create() -> Self {
        let name = format!(
            "nosdesk_upgrade_{}",
            &Uuid::new_v4().simple().to_string()[..16]
        );
        let url = super::with_database(&super::base_url(), &name);
        let mut admin =
            PgConnection::establish(&super::admin_url()).expect("connect to admin DB (postgres)");
        run(&mut admin, &format!("CREATE DATABASE \"{name}\""));
        Self { name, url }
    }

    /// A fresh database restored from a `pg_dump` file, plain SQL or the
    /// custom format (`pg_dump -Fc`), such as one taken from a real 1.0.12
    /// install. Runs `psql` or `pg_restore` from `PG_BIN_DIR` when set, else
    /// from `PATH`; their major version should match the server's.
    pub fn from_dump(path: &std::path::Path) -> Self {
        let db = Self::create();
        let mut magic = [0u8; 5];
        let custom = std::fs::File::open(path)
            .and_then(|mut f| std::io::Read::read_exact(&mut f, &mut magic))
            .map(|()| &magic == b"PGDMP")
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut command = if custom {
            let mut c = std::process::Command::new(pg_tool("pg_restore"));
            c.args(["--exit-on-error", "--dbname", &db.url]);
            c
        } else {
            let mut c = std::process::Command::new(pg_tool("psql"));
            c.args([
                "--quiet",
                "--set",
                "ON_ERROR_STOP=1",
                "--dbname",
                &db.url,
                "--file",
            ]);
            c
        };
        let status = command
            .arg(path)
            .status()
            .unwrap_or_else(|e| panic!("run the restore for {}: {e}", path.display()));
        assert!(
            status.success(),
            "restoring {} failed: {status}",
            path.display()
        );
        db
    }

    /// A fresh database migrated to exactly what 1.0.12 shipped.
    pub fn at_1_0_12() -> Self {
        let db = Self::create();
        let mut conn = db.conn();
        run(
            &mut conn,
            "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (\
             version VARCHAR(50) PRIMARY KEY NOT NULL, \
             run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
        );
        let mut reached = false;
        for m in sorted_pending(&mut conn) {
            let name = m.name().to_string();
            conn.run_migration(&*m)
                .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
            if name == LAST_1_0_12_MIGRATION {
                reached = true;
                break;
            }
        }
        assert!(
            reached,
            "{LAST_1_0_12_MIGRATION} must be in the migration set"
        );
        db
    }

    /// A new connection as the test's (superuser) role.
    pub fn conn(&self) -> PgConnection {
        PgConnection::establish(&self.url).expect("connect to the upgrade DB")
    }

    /// Run every migration after 1.0.12 in version order; their names.
    pub fn migrate_to_head(&self, conn: &mut PgConnection) -> Vec<String> {
        sorted_pending(conn)
            .into_iter()
            .map(|m| {
                let name = m.name().to_string();
                conn.run_migration(&*m)
                    .unwrap_or_else(|e| panic!("migration {name} failed on 1.0.12 data: {e}"));
                name
            })
            .collect()
    }
}

impl Drop for UpgradeDb {
    fn drop(&mut self) {
        // FORCE ends the sessions still open on it, including a booted
        // app's pool and workers that would otherwise reconnect.
        if let Ok(mut admin) = PgConnection::establish(&super::admin_url()) {
            let _ = diesel::sql_query(format!(
                "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
                self.name
            ))
            .execute(&mut admin);
        }
    }
}

/// `tool` from `PG_BIN_DIR` when set, else from `PATH`.
pub fn pg_tool(tool: &str) -> std::path::PathBuf {
    match std::env::var_os("PG_BIN_DIR") {
        Some(dir) => std::path::Path::new(&dir).join(tool),
        None => tool.into(),
    }
}

fn sorted_pending(
    conn: &mut PgConnection,
) -> Vec<Box<dyn diesel::migration::Migration<diesel::pg::Pg>>> {
    let mut pending = conn
        .pending_migrations(MIGRATIONS)
        .expect("list pending migrations");
    pending.sort_by_key(|m| m.name().to_string());
    pending
}

fn run(conn: &mut PgConnection, sql: &str) {
    diesel::sql_query(sql)
        .execute(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"));
}

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

/// Load rows into `table` with its user triggers off, as a restore would.
pub fn load(conn: &mut PgConnection, table: &str, sql: &str) {
    run(
        conn,
        &format!("ALTER TABLE public.{table} DISABLE TRIGGER USER"),
    );
    run(conn, sql);
    run(
        conn,
        &format!("ALTER TABLE public.{table} ENABLE TRIGGER USER"),
    );
}

/// `load` for one row, returning its `id` (the statement ends `RETURNING id`).
pub fn load_returning_id(conn: &mut PgConnection, table: &str, sql: &str) -> i32 {
    run(
        conn,
        &format!("ALTER TABLE public.{table} DISABLE TRIGGER USER"),
    );
    let id = diesel::sql_query(sql)
        .get_result::<Id>(conn)
        .unwrap_or_else(|e| panic!("{sql}: {e}"))
        .id;
    run(
        conn,
        &format!("ALTER TABLE public.{table} ENABLE TRIGGER USER"),
    );
    id
}

/// Row counts for `tables`, keyed by table.
pub fn row_counts(conn: &mut PgConnection, tables: &[&str]) -> BTreeMap<String, i64> {
    tables
        .iter()
        .map(|table| {
            let n = diesel::sql_query(format!("SELECT count(*) AS n FROM public.{table}"))
                .get_result::<Count>(conn)
                .unwrap_or_else(|e| panic!("count {table}: {e}"))
                .n;
            (table.to_string(), n)
        })
        .collect()
}

/// The state ids of one workspace, by category.
#[derive(Debug, Clone)]
pub struct States {
    pub triage: i32,
    pub backlog: i32,
    pub active: i32,
    pub done: i32,
    pub cancelled: i32,
    pub merged: i32,
}

/// A workspace and its workflow states.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub id: i32,
    pub states: States,
}

#[derive(Debug, Clone)]
pub struct Workspaces {
    /// The bootstrap workspace every 1.0 install has (id 1, slug `default`).
    pub default: i32,
    pub default_states: States,
    /// A second workspace on a slug 1.1 reserves (`teams`). `None` in the
    /// single-workspace shape every Community install has.
    pub teams: Option<Workspace>,
}

#[derive(Debug, Clone)]
pub struct People {
    /// Admin of both workspaces, holder of the API token and the session.
    pub admin: Uuid,
    /// Agent in the default workspace.
    pub agent: Uuid,
    /// Member (requester) in the default workspace.
    pub requester: Uuid,
    /// Removed from the default workspace the 1.0.x way, by deleting the
    /// membership row; still the author of a document revision.
    pub former: Uuid,
}

#[derive(Debug, Clone)]
pub struct Tickets {
    /// Open, with a requester, an assignee, comments, an attachment and a
    /// watcher.
    pub open: i32,
    /// Done with no `closed_at`; its state history says the agent closed it.
    pub done: i32,
    /// When the history says the done ticket was closed.
    pub done_closed_at: &'static str,
    /// Cancelled with no `closed_at` and no history; `updated_at` stands in.
    pub cancelled: i32,
    pub cancelled_updated_at: &'static str,
    /// Closed, then reopened into In Progress; 1.0.x left its `closed_at`
    /// and `closed_by` set.
    pub reopened: i32,
    /// Merged into `merge_target` (1.0 kept the merge on the ticket row).
    pub merged: i32,
    pub merge_target: i32,
    /// In the teams workspace but on the default workspace's Backlog, which
    /// 1.0.x let happen. `None` without a teams workspace.
    pub foreign_state: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct Seeded {
    pub workspaces: Workspaces,
    pub people: People,
    pub tickets: Tickets,
    pub page: i32,
    /// Raw `full`-scope API token for `people.admin` in the default
    /// workspace.
    pub api_token: String,
    /// The address every workspace mailed and that bounced.
    pub suppressed_email: &'static str,
}

/// Seed an install with two workspaces, in dependency order.
pub fn seed(conn: &mut PgConnection) -> Seeded {
    let workspaces = seed_workspaces(conn);
    seed_into(conn, workspaces)
}

/// Seed a Community install: the bootstrap workspace only.
pub fn seed_single_workspace(conn: &mut PgConnection) -> Seeded {
    let workspaces = seed_bootstrap_workspace(conn);
    seed_into(conn, workspaces)
}

fn seed_into(conn: &mut PgConnection, workspaces: Workspaces) -> Seeded {
    let people = seed_people(conn, &workspaces);
    let tickets = seed_tickets(conn, &workspaces, &people);
    seed_state_history(conn, &workspaces, &people, &tickets);
    seed_conversation(conn, &workspaces, &people, &tickets);
    let page = seed_document(conn, &workspaces, &people);
    let api_token = seed_api_token(conn, &workspaces, &people);
    seed_session(conn, &people);
    let suppressed_email = seed_mail(conn, &workspaces, &tickets);
    Seeded {
        workspaces,
        people,
        tickets,
        page,
        api_token,
        suppressed_email,
    }
}

fn states_of(conn: &mut PgConnection, workspace: i32) -> States {
    let mut state = |category: &str| {
        diesel::sql_query(format!(
            "SELECT id FROM workflow_states WHERE workspace_id = {workspace} \
             AND category = '{category}' ORDER BY position, id LIMIT 1"
        ))
        .get_result::<Id>(conn)
        .unwrap_or_else(|e| panic!("{category} state of workspace {workspace}: {e}"))
        .id
    };
    States {
        triage: state("triage"),
        backlog: state("backlog"),
        active: state("active"),
        done: state("done"),
        cancelled: state("cancelled"),
        merged: state("merged"),
    }
}

/// The weekly schedule 1.0.12's default working calendar had.
const DEFAULT_SCHEDULE: &str = r#"{"mon": [["09:00", "17:00"]], "tue": [["09:00", "17:00"]], "wed": [["09:00", "17:00"]], "thu": [["09:00", "17:00"]], "fri": [["09:00", "17:00"]], "sat": [], "sun": []}"#;

/// What 1.0.12's `seed_workspace_defaults` gave a workspace, each only where
/// it had none yet: the seven workflow states, a 9 to 5 working calendar
/// with a default SLA policy, and three ticket categories. The bootstrap
/// workspace's states, calendar and policy come with the schema.
pub fn seed_workspace_defaults(conn: &mut PgConnection, workspace: i32) {
    load(
        conn,
        "workflow_states",
        &format!(
            "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
             SELECT {workspace}, name, category, color, position, is_default \
             FROM workflow_states WHERE workspace_id = 1 \
             AND NOT EXISTS (SELECT 1 FROM workflow_states WHERE workspace_id = {workspace}) \
             ORDER BY id"
        ),
    );
    load(
        conn,
        "working_calendars",
        &format!(
            "INSERT INTO working_calendars (workspace_id, name, timezone, schedule, is_default) \
             SELECT {workspace}, 'Default 9-5', 'UTC', '{}', true \
             WHERE NOT EXISTS (SELECT 1 FROM working_calendars WHERE workspace_id = {workspace})",
            DEFAULT_SCHEDULE
        ),
    );
    load(
        conn,
        "sla_policies",
        &format!(
            "INSERT INTO sla_policies \
             (workspace_id, name, target_response_minutes, target_resolution_minutes, working_calendar_id, is_default) \
             SELECT {workspace}, 'Default', 240, 1440, c.id, true FROM working_calendars c \
             WHERE c.workspace_id = {workspace} AND c.is_default \
             AND NOT EXISTS (SELECT 1 FROM sla_policies WHERE workspace_id = {workspace})"
        ),
    );
    load(
        conn,
        "ticket_categories",
        &format!(
            "INSERT INTO ticket_categories (workspace_id, name, description, color, icon, display_order, is_active) \
             SELECT {workspace}, v.* FROM (VALUES \
               ('Support', 'General help requests', '#3b82f6', 'question', 0, true), \
               ('Bug', 'Defect reports', '#ef4444', 'bug', 1, true), \
               ('Feature request', 'Enhancement ideas', '#8b5cf6', 'lightbulb', 2, true) \
             ) AS v(name, description, color, icon, display_order, is_active) \
             WHERE NOT EXISTS (SELECT 1 FROM ticket_categories WHERE workspace_id = {workspace})"
        ),
    );
}

/// The Community shape: the bootstrap workspace with its defaults.
pub fn seed_bootstrap_workspace(conn: &mut PgConnection) -> Workspaces {
    seed_workspace_defaults(conn, 1);
    Workspaces {
        default: 1,
        default_states: states_of(conn, 1),
        teams: None,
    }
}

/// The bootstrap workspace plus a second one slugged `teams`, each with
/// its settings row and the defaults 1.0.12 seeded.
pub fn seed_workspaces(conn: &mut PgConnection) -> Workspaces {
    let teams = load_returning_id(
        conn,
        "workspaces",
        "INSERT INTO workspaces (slug, name, created_at) \
         VALUES ('teams', 'Teams', '2026-08-01T09:00:00Z') RETURNING id",
    );
    load(
        conn,
        "site_settings",
        &format!("INSERT INTO site_settings (workspace_id, app_name) VALUES ({teams}, 'Teams')"),
    );
    seed_workspace_defaults(conn, teams);
    let mut workspaces = seed_bootstrap_workspace(conn);
    workspaces.teams = Some(Workspace {
        id: teams,
        states: states_of(conn, teams),
    });
    workspaces
}

fn person(
    conn: &mut PgConnection,
    name: &str,
    email: &str,
    platform_role: &str,
    memberships: &[(i32, &str)],
) -> Uuid {
    let uuid = Uuid::new_v4();
    load(
        conn,
        "users",
        &format!(
            "INSERT INTO users (uuid, name, platform_role, created_at, updated_at) \
             VALUES ('{uuid}', '{name}', '{platform_role}', '2026-08-01T09:00:00Z', '2026-08-01T09:00:00Z')"
        ),
    );
    load(
        conn,
        "user_emails",
        &format!(
            "INSERT INTO user_emails (user_uuid, email, is_primary, is_verified) \
             VALUES ('{uuid}', '{email}', true, true)"
        ),
    );
    load(
        conn,
        "user_preferences",
        &format!("INSERT INTO user_preferences (user_uuid) VALUES ('{uuid}')"),
    );
    for (workspace, role) in memberships {
        load(
            conn,
            "workspace_members",
            &format!(
                "INSERT INTO workspace_members (workspace_id, user_uuid, role, invited_at, accepted_at) \
                 VALUES ({workspace}, '{uuid}', '{role}', '2026-08-01T09:00:00Z', '2026-08-01T09:00:00Z')"
            ),
        );
    }
    uuid
}

/// An admin of every workspace, an agent, a requester, and a former member
/// with no membership row left.
pub fn seed_people(conn: &mut PgConnection, ws: &Workspaces) -> People {
    let mut admin_of = vec![(ws.default, "admin")];
    if let Some(teams) = &ws.teams {
        admin_of.push((teams.id, "admin"));
    }
    People {
        admin: person(
            conn,
            "Ada Admin",
            "ada@example.com",
            "platform_admin",
            &admin_of,
        ),
        agent: person(
            conn,
            "Abe Agent",
            "abe@example.com",
            "user",
            &[(ws.default, "agent")],
        ),
        requester: person(
            conn,
            "Rae Requester",
            "rae@example.com",
            "user",
            &[(ws.default, "member")],
        ),
        former: person(conn, "Fern Former", "fern@example.com", "user", &[]),
    }
}

fn ticket(
    conn: &mut PgConnection,
    workspace: i32,
    state: i32,
    title: &str,
    created_at: &str,
    updated_at: &str,
    extra: &[(&str, String)],
) -> i32 {
    let columns: String = extra.iter().map(|(c, _)| format!(", {c}")).collect();
    let values: String = extra.iter().map(|(_, v)| format!(", {v}")).collect();
    load_returning_id(
        conn,
        "tickets",
        &format!(
            "INSERT INTO tickets (workspace_id, workflow_state_id, title, created_at, updated_at{columns}) \
             VALUES ({workspace}, {state}, '{title}', '{created_at}', '{updated_at}'{values}) RETURNING id"
        ),
    )
}

/// A quoted SQL literal.
fn q(value: impl std::fmt::Display) -> String {
    format!("'{value}'")
}

/// Open, done and cancelled tickets with no `closed_at`, a reopened one that
/// still has it, a merged pair, and (with a teams workspace) a teams ticket
/// on the default workspace's Backlog.
pub fn seed_tickets(conn: &mut PgConnection, ws: &Workspaces, people: &People) -> Tickets {
    let d = &ws.default_states;
    let open = ticket(
        conn,
        ws.default,
        d.backlog,
        "Printer on level 3 is jammed",
        "2026-08-03T09:00:00Z",
        "2026-08-03T09:30:00Z",
        &[
            ("requester_uuid", q(people.requester)),
            ("assignee_uuid", q(people.agent)),
            ("created_by", q(people.requester)),
        ],
    );
    let done_closed_at = "2026-08-19T10:00:00Z";
    let done = ticket(
        conn,
        ws.default,
        d.done,
        "Laptop will not boot",
        "2026-08-04T09:00:00Z",
        "2026-08-20T08:00:00Z",
        &[
            ("requester_uuid", q(people.requester)),
            ("assignee_uuid", q(people.agent)),
        ],
    );
    let cancelled_updated_at = "2026-08-21T08:00:00Z";
    let cancelled = ticket(
        conn,
        ws.default,
        d.cancelled,
        "Duplicate VPN request",
        "2026-08-05T09:00:00Z",
        cancelled_updated_at,
        &[("requester_uuid", q(people.requester))],
    );
    let reopened = ticket(
        conn,
        ws.default,
        d.active,
        "Monitor flickers again",
        "2026-08-09T09:00:00Z",
        "2026-08-12T09:00:00Z",
        &[
            ("requester_uuid", q(people.requester)),
            ("closed_at", q("2026-08-11T09:00:00Z")),
            ("closed_by", q(people.agent)),
        ],
    );
    let merge_target = ticket(
        conn,
        ws.default,
        d.active,
        "Email bouncing for sales team",
        "2026-08-06T09:00:00Z",
        "2026-08-10T09:00:00Z",
        &[],
    );
    let merged = ticket(
        conn,
        ws.default,
        d.merged,
        "Sales cannot receive mail",
        "2026-08-07T09:00:00Z",
        "2026-08-10T09:00:00Z",
        &[
            ("merged_into_ticket_id", merge_target.to_string()),
            ("merged_at", q("2026-08-10T09:00:00Z")),
            ("merged_by_user_uuid", q(people.agent)),
            ("merge_reason", q("Same outage")),
        ],
    );
    let foreign_state = ws.teams.as_ref().map(|teams| {
        ticket(
            conn,
            teams.id,
            d.backlog,
            "New starter needs a desk",
            "2026-08-08T09:00:00Z",
            "2026-08-08T09:00:00Z",
            &[("requester_uuid", q(people.admin))],
        )
    });
    Tickets {
        open,
        done,
        done_closed_at,
        cancelled,
        cancelled_updated_at,
        reopened,
        merged,
        merge_target,
        foreign_state,
    }
}

/// The done ticket's workflow history in `sync_actions`: moved to In
/// Progress, then to Done, both by the agent.
pub fn seed_state_history(
    conn: &mut PgConnection,
    ws: &Workspaces,
    people: &People,
    tickets: &Tickets,
) {
    for (state, at) in [
        (ws.default_states.active, "2026-08-15T10:00:00Z"),
        (ws.default_states.done, tickets.done_closed_at),
    ] {
        load(
            conn,
            "sync_actions",
            &format!(
                "INSERT INTO sync_actions \
                 (aggregate, aggregate_id, op, event_type, data, groups, actor_uuid, occurred_at, workspace_id) \
                 VALUES ('ticket', '{ticket}', 'U', 'ticket.workflow_state_changed', \
                         '{{\"workflow_state_id\": {state}}}', ARRAY['workspace'], '{actor}', '{at}', {workspace})",
                ticket = tickets.done,
                actor = people.agent,
                workspace = ws.default,
            ),
        );
    }
}

/// A public reply with an attachment and an internal note on the open
/// ticket, which the agent watches.
pub fn seed_conversation(
    conn: &mut PgConnection,
    ws: &Workspaces,
    people: &People,
    tickets: &Tickets,
) {
    let reply = load_returning_id(
        conn,
        "comments",
        &format!(
            "INSERT INTO comments (workspace_id, ticket_id, user_uuid, content, created_at, updated_at) \
             VALUES ({}, {}, '{}', 'Photo of the error attached', '2026-08-03T09:10:00Z', '2026-08-03T09:10:00Z') \
             RETURNING id",
            ws.default, tickets.open, people.requester
        ),
    );
    load(
        conn,
        "comments",
        &format!(
            "INSERT INTO comments (workspace_id, ticket_id, user_uuid, content, is_internal, created_at, updated_at) \
             VALUES ({}, {}, '{}', 'Ordered a new fuser', true, '2026-08-03T09:20:00Z', '2026-08-03T09:20:00Z')",
            ws.default, tickets.open, people.agent
        ),
    );
    load(
        conn,
        "attachments",
        &format!(
            "INSERT INTO attachments (workspace_id, comment_id, url, name, file_size, mime_type, uploaded_by, created_at) \
             VALUES ({}, {reply}, '/uploads/tickets/printer-error.png', 'printer-error.png', 20480, 'image/png', '{}', \
                     '2026-08-03T09:10:00Z')",
            ws.default, people.requester
        ),
    );
    load(
        conn,
        "ticket_watchers",
        &format!(
            "INSERT INTO ticket_watchers (workspace_id, ticket_id, user_uuid, created_at) \
             VALUES ({}, {}, '{}', '2026-08-03T09:20:00Z')",
            ws.default, tickets.open, people.agent
        ),
    );
}

/// A published page with two revisions, the first by the former member.
pub fn seed_document(conn: &mut PgConnection, ws: &Workspaces, people: &People) -> i32 {
    let page = load_returning_id(
        conn,
        "documentation_pages",
        &format!(
            "INSERT INTO documentation_pages \
             (workspace_id, title, slug, status, created_by, last_edited_by, yjs_document, yjs_state_vector, \
              created_at, updated_at) \
             VALUES ({}, 'Printer setup', 'printer-setup', 'published', '{}', '{}', '\\x0000', '\\x00', \
                     '2026-08-02T09:00:00Z', '2026-08-12T09:00:00Z') RETURNING id",
            ws.default, people.former, people.admin
        ),
    );
    for (number, author, at) in [
        (1, people.former, "2026-08-02T09:00:00Z"),
        (2, people.admin, "2026-08-12T09:00:00Z"),
    ] {
        load(
            conn,
            "documentation_revisions",
            &format!(
                "INSERT INTO documentation_revisions \
                 (workspace_id, page_id, revision_number, title, yjs_document_snapshot, yjs_state_vector, created_by, created_at) \
                 VALUES ({}, {page}, {number}, 'Printer setup', '\\x0000', '\\x00', '{author}', '{at}')",
                ws.default
            ),
        );
    }
    page
}

/// A `full`-scope API token for the admin in the default workspace; the raw
/// token. The hash and prefix functions are unchanged since 1.0.12.
pub fn seed_api_token(conn: &mut PgConnection, ws: &Workspaces, people: &People) -> String {
    use backend::repository::api_tokens::{get_token_prefix, hash_token};
    let raw = format!("nsk_{}", Uuid::new_v4().simple());
    load(
        conn,
        "api_tokens",
        &format!(
            "INSERT INTO api_tokens \
             (workspace_id, token_hash, token_prefix, user_uuid, name, scopes, created_by, created_at) \
             VALUES ({}, '{}', '{}', '{admin}', 'Inventory sync', ARRAY['full'], '{admin}', '2026-08-02T09:00:00Z')",
            ws.default,
            hash_token(&raw),
            get_token_prefix(&raw),
            admin = people.admin,
        ),
    );
    raw
}

/// The admin's signed-in session with a live refresh token.
pub fn seed_session(conn: &mut PgConnection, people: &People) {
    let session = Uuid::new_v4();
    load(
        conn,
        "active_sessions",
        &format!(
            "INSERT INTO active_sessions (user_uuid, session_id, device_name, created_at, last_active, expires_at) \
             VALUES ('{}', '{session}', 'Firefox on macOS', '2026-08-20T09:00:00Z', '2026-08-20T09:00:00Z', \
                     now() + interval '30 days')",
            people.admin
        ),
    );
    load(
        conn,
        "refresh_tokens",
        &format!(
            "INSERT INTO refresh_tokens (user_uuid, token_hash, session_id, family_id, created_at, expires_at) \
             VALUES ('{}', '{}', '{session}', '{}', '2026-08-20T09:00:00Z', now() + interval '30 days')",
            people.admin,
            Uuid::new_v4().simple(),
            Uuid::new_v4()
        ),
    );
}

/// Every workspace mailed one address that hard-bounced, so it is
/// suppressed; a second suppression has no outbound history left. The
/// suppressed address.
pub fn seed_mail(conn: &mut PgConnection, ws: &Workspaces, tickets: &Tickets) -> &'static str {
    let address = "bounced@example.org";
    let mut mailed = vec![(ws.default, tickets.open)];
    if let (Some(teams), Some(ticket)) = (&ws.teams, tickets.foreign_state) {
        mailed.push((teams.id, ticket));
    }
    for (workspace, ticket) in mailed {
        load(
            conn,
            "outbound_emails",
            &format!(
                "INSERT INTO outbound_emails \
                 (workspace_id, ticket_id, recipient, subject, body_text, message_id, status, created_at, sent_at, bounced_at) \
                 VALUES ({workspace}, {ticket}, '{address}', 'Your request', 'We are on it.', \
                         '<{}@mail.example.com>', 'sent', '2026-08-09T09:00:00Z', '2026-08-09T09:00:05Z', \
                         '2026-08-09T09:01:00Z')",
                Uuid::new_v4().simple()
            ),
        );
    }
    load(
        conn,
        "email_suppressions",
        &format!(
            "INSERT INTO email_suppressions (email, reason, bounce_count, created_at, last_seen_at) VALUES \
             ('{address}', 'hard_bounce', 1, '2026-08-09T09:01:00Z', '2026-08-09T09:01:00Z'), \
             ('stale@example.org', 'hard_bounce', 1, '2026-08-09T09:01:00Z', '2026-08-09T09:01:00Z')"
        ),
    );
    address
}
