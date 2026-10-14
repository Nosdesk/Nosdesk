//! Upgrading to staff-only drafts hides nothing someone outside staff reads
//! today: the migration publishes every draft a requester or a guest can open,
//! leaves the drafts open to staff only as drafts, and resolves the knowledge
//! gaps drafting on what it publishes, as publishing a page does. Runs the
//! migration set against a database seeded just before it.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Integer, Nullable, Text};
use diesel_migrations::MigrationHarness;

use backend::db::MIGRATIONS;
use backend::repository::PageAudience;

use crate::migration_backfill_existing_workspace::FreshDb;

#[derive(QueryableByName)]
struct Id {
    #[diesel(sql_type = Integer)]
    id: i32,
}

/// Run `sql` with the table's user triggers off, as if the rows already
/// existed. Returns the id it hands back, if any.
fn existing(conn: &mut PgConnection, table: &str, sql: &str) -> Option<i32> {
    diesel::sql_query(format!("ALTER TABLE {table} DISABLE TRIGGER USER"))
        .execute(conn)
        .expect("disable triggers");
    let id = if sql.contains("RETURNING id") {
        Some(
            diesel::sql_query(sql)
                .get_result::<Id>(conn)
                .unwrap_or_else(|e| panic!("seed {table}: {e}"))
                .id,
        )
    } else {
        diesel::sql_query(sql)
            .execute(conn)
            .unwrap_or_else(|e| panic!("seed {table}: {e}"));
        None
    };
    diesel::sql_query(format!("ALTER TABLE {table} ENABLE TRIGGER USER"))
        .execute(conn)
        .expect("enable triggers");
    id
}

/// What the migration should do, and what to check it against.
struct Seeded {
    ws: i32,
    requester: uuid::Uuid,
    /// Published by the migration, and open to the requester by the rules.
    published: Vec<(&'static str, i32)>,
    /// Published as public, whoever the rules open it to.
    published_public: Vec<(&'static str, i32)>,
    /// Left as they were.
    kept: Vec<(&'static str, i32)>,
    /// A gap drafting on a page the migration publishes.
    resolved_gap: i64,
    resolved_gap_page: i32,
    /// The tickets its live signals cite, and one a dismissed signal cites.
    cited: Vec<i32>,
    not_cited: i32,
    /// A gap drafting on a draft that stays a draft.
    drafting_gap: i64,
    drafting_gap_page: i32,
}

/// Seed one workspace, returning its id.
fn workspace(conn: &mut PgConnection, slug: &str) -> i32 {
    existing(
        conn,
        "workspaces",
        &format!("INSERT INTO workspaces (slug, name) VALUES ('{slug}', '{slug}') RETURNING id"),
    )
    .expect("workspace")
}

fn seed(conn: &mut PgConnection) -> Seeded {
    let ws = workspace(conn, "drafts");
    let (admin, agent, requester) = (
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
    );
    existing(
        conn,
        "users",
        &format!(
            "INSERT INTO users (uuid, name) VALUES ('{admin}', 'Admin'), \
             ('{agent}', 'Agent'), ('{requester}', 'Requester')"
        ),
    );
    existing(
        conn,
        "workspace_members",
        &format!(
            "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at) \
             VALUES ({ws}, '{admin}', 'admin', now()), ({ws}, '{agent}', 'agent', now()), \
                    ({ws}, '{requester}', 'member', now())"
        ),
    );
    let group = |conn: &mut PgConnection, ws: i32, name: &str| {
        existing(
            conn,
            "groups",
            &format!(
                "INSERT INTO groups (workspace_id, name) VALUES ({ws}, '{name}') RETURNING id"
            ),
        )
        .expect("group")
    };
    let staff_group = group(conn, ws, "IT");
    let requester_group = group(conn, ws, "Finance");
    let including_group = group(conn, ws, "Everyone");
    let grandparent_group = group(conn, ws, "Company");
    existing(
        conn,
        "user_groups",
        &format!(
            "INSERT INTO user_groups (workspace_id, user_uuid, group_id) \
             VALUES ({ws}, '{agent}', {staff_group}), ({ws}, '{requester}', {requester_group})"
        ),
    );
    // Company includes Everyone, which includes Finance.
    existing(
        conn,
        "group_includes",
        &format!(
            "INSERT INTO group_includes (workspace_id, parent_group_id, child_group_id) \
             VALUES ({ws}, {including_group}, {requester_group}), \
                    ({ws}, {grandparent_group}, {including_group})"
        ),
    );
    let collection = |conn: &mut PgConnection, ws: i32, slug: &str, restricted: bool| {
        existing(
            conn,
            "documentation_collections",
            &format!(
                "INSERT INTO documentation_collections (workspace_id, name, slug, restricted) \
                 VALUES ({ws}, '{slug}', '{slug}', {restricted}) RETURNING id"
            ),
        )
        .expect("collection")
    };
    let open_collection = collection(conn, ws, "handbook", false);
    let staff_collection = collection(conn, ws, "runbooks", true);
    let requester_collection = collection(conn, ws, "finance", true);
    existing(
        conn,
        "documentation_collection_visibility",
        &format!(
            "INSERT INTO documentation_collection_visibility (workspace_id, collection_id, group_id) \
             VALUES ({ws}, {staff_collection}, {staff_group}), \
                    ({ws}, {requester_collection}, {including_group})"
        ),
    );
    let page = |conn: &mut PgConnection,
                ws: i32,
                slug: &str,
                status: &str,
                public: bool,
                restricted: bool| {
        existing(
            conn,
            "documentation_pages",
            &format!(
                "INSERT INTO documentation_pages \
                 (workspace_id, title, slug, status, is_public, restricted, created_by, last_edited_by) \
                 VALUES ({ws}, '{slug}', '{slug}', '{status}', {public}, {restricted}, \
                         '{admin}', '{admin}') RETURNING id"
            ),
        )
        .expect("page")
    };
    let in_collection = |conn: &mut PgConnection, ws: i32, page: i32, collection: i32| {
        existing(
            conn,
            "documentation_collection_pages",
            &format!(
                "INSERT INTO documentation_collection_pages (workspace_id, collection_id, page_id) \
                 VALUES ({ws}, {collection}, {page})"
            ),
        );
    };
    let page_grant = |conn: &mut PgConnection, page: i32, column: &str, value: String| {
        existing(
            conn,
            "documentation_page_visibility",
            &format!(
                "INSERT INTO documentation_page_visibility (workspace_id, page_id, {column}) \
                 VALUES ({ws}, {page}, {value})"
            ),
        );
    };

    let public_staff_only = page(conn, ws, "public-runbook", "draft", true, true);
    let open = page(conn, ws, "open", "draft", false, false);
    let in_open = page(conn, ws, "in-open", "draft", false, false);
    in_collection(conn, ws, in_open, open_collection);
    let in_requester_collection = page(conn, ws, "in-finance", "draft", false, false);
    in_collection(conn, ws, in_requester_collection, requester_collection);
    let granted_requester = page(conn, ws, "to-requester", "draft", false, true);
    page_grant(
        conn,
        granted_requester,
        "user_uuid",
        format!("'{requester}'"),
    );
    let granted_group = page(conn, ws, "to-finance", "draft", false, true);
    page_grant(conn, granted_group, "group_id", requester_group.to_string());

    let in_staff_collection = page(conn, ws, "in-runbooks", "draft", false, false);
    in_collection(conn, ws, in_staff_collection, staff_collection);
    let granted_agent = page(conn, ws, "to-agent", "draft", false, true);
    page_grant(conn, granted_agent, "user_uuid", format!("'{agent}'"));
    let granted_staff_group = page(conn, ws, "to-it", "draft", false, true);
    page_grant(
        conn,
        granted_staff_group,
        "group_id",
        staff_group.to_string(),
    );
    let granted_grandparent = page(conn, ws, "to-company", "draft", false, true);
    page_grant(
        conn,
        granted_grandparent,
        "group_id",
        grandparent_group.to_string(),
    );
    let admins_only = page(conn, ws, "admins-only", "draft", false, true);
    let archived = page(conn, ws, "archived", "archived", false, false);
    let public_archived = page(conn, ws, "public-archived", "archived", true, false);
    let trashed = page(conn, ws, "trashed", "deleted", true, false);

    // A workspace with staff only: no one outside staff reads its drafts.
    let staff_ws = workspace(conn, "staff-only");
    existing(
        conn,
        "workspace_members",
        &format!(
            "INSERT INTO workspace_members (workspace_id, user_uuid, role, accepted_at) \
             VALUES ({staff_ws}, '{admin}', 'admin', now()), ({staff_ws}, '{agent}', 'agent', now())"
        ),
    );
    let staff_ws_open = page(conn, staff_ws, "staff-open", "draft", false, false);
    let staff_ws_collection = collection(conn, staff_ws, "staff-handbook", false);
    let staff_ws_in_open = page(conn, staff_ws, "staff-in-open", "draft", false, false);
    in_collection(conn, staff_ws, staff_ws_in_open, staff_ws_collection);
    let staff_ws_public = page(conn, staff_ws, "staff-public", "draft", true, false);

    // Knowledge gaps drafting on a page that's published and on one that isn't.
    let state = existing(
        conn,
        "workflow_states",
        &format!(
            "INSERT INTO workflow_states (workspace_id, name, category, color, position, is_default) \
             VALUES ({ws}, 'Open', 'backlog', 'gray', 0, true) RETURNING id"
        ),
    )
    .expect("state");
    let ticket = |conn: &mut PgConnection, n: i32| {
        existing(
            conn,
            "tickets",
            &format!(
                "INSERT INTO tickets (workspace_id, title, workflow_state_id, number) \
                 VALUES ({ws}, 'Ticket {n}', {state}, {n}) RETURNING id"
            ),
        )
        .expect("ticket")
    };
    let (flagged, clustered, already_linked, dismissed) = (
        ticket(conn, 1),
        ticket(conn, 2),
        ticket(conn, 3),
        ticket(conn, 4),
    );
    let gap = |conn: &mut PgConnection, title: &str, page: i32| -> i64 {
        existing(
            conn,
            "knowledge_gaps",
            &format!(
                "INSERT INTO knowledge_gaps (workspace_id, title, status, draft_page_id) \
                 VALUES ({ws}, '{title}', 'drafting', {page}) RETURNING id::integer AS id"
            ),
        )
        .expect("gap")
        .into()
    };
    let resolved_gap = gap(conn, "VPN drops", open);
    let drafting_gap = gap(conn, "Runbook missing", in_staff_collection);
    existing(
        conn,
        "knowledge_gap_signals",
        &format!(
            "INSERT INTO knowledge_gap_signals \
             (workspace_id, gap_id, signal_type, source_kind, source_ref, payload, dismissed_at) VALUES \
             ({ws}, {resolved_gap}, 'manual_flag', 'ticket', '{flagged}', '{{}}', NULL), \
             ({ws}, {resolved_gap}, 'ticket_cluster', 'cluster_key', 'abc', \
              '{{\"ticket_ids\": [{clustered}, {already_linked}, 999999, 2.5]}}', NULL), \
             ({ws}, {resolved_gap}, 'manual_flag', 'ticket', '{dismissed}', '{{}}', now()), \
             ({ws}, {drafting_gap}, 'manual_flag', 'ticket', '{flagged}', '{{}}', NULL)"
        ),
    );
    existing(
        conn,
        "documentation_page_tickets",
        &format!(
            "INSERT INTO documentation_page_tickets (workspace_id, page_id, ticket_id, link_type) \
             VALUES ({ws}, {open}, {already_linked}, 'references')"
        ),
    );

    Seeded {
        ws,
        requester,
        published: vec![
            ("an open draft in no collection", open),
            ("an open draft in an open collection", in_open),
            (
                "a draft in a collection a requester's group reaches",
                in_requester_collection,
            ),
            ("a draft shared with a requester", granted_requester),
            ("a draft shared with a requester's group", granted_group),
        ],
        published_public: vec![
            (
                "a public draft, even one only staff can open",
                public_staff_only,
            ),
            (
                "a public draft where there are no requesters",
                staff_ws_public,
            ),
        ],
        kept: vec![
            ("a draft in a staff-only collection", in_staff_collection),
            ("a draft shared with an agent", granted_agent),
            ("a draft shared with a staff group", granted_staff_group),
            (
                "a draft shared with a group two includes above a requester's",
                granted_grandparent,
            ),
            ("a restricted draft shared with no one", admins_only),
            ("an archived page", archived),
            ("a public archived page", public_archived),
            ("a page in the trash", trashed),
            ("an open draft where there are no requesters", staff_ws_open),
            (
                "a draft in an open collection where there are no requesters",
                staff_ws_in_open,
            ),
        ],
        resolved_gap,
        resolved_gap_page: open,
        cited: vec![flagged, clustered, already_linked],
        not_cited: dismissed,
        drafting_gap,
        drafting_gap_page: in_staff_collection,
    }
}

#[derive(QueryableByName)]
struct Status {
    #[diesel(sql_type = Integer)]
    id: i32,
    #[diesel(sql_type = Text)]
    status: String,
}

#[test]
fn the_upgrade_publishes_every_draft_someone_outside_staff_reads() {
    let db = FreshDb::new();
    let mut conn = PgConnection::establish(&db.url).expect("connect fresh db");
    diesel::sql_query(
        "CREATE TABLE IF NOT EXISTS __diesel_schema_migrations (\
         version VARCHAR(50) PRIMARY KEY NOT NULL, \
         run_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP)",
    )
    .execute(&mut conn)
    .expect("create migrations table");

    let mut pending = conn
        .pending_migrations(MIGRATIONS)
        .expect("list pending migrations");
    pending.sort_by_key(|m| m.name().to_string());
    let mut seeded = None;
    let mut before = Vec::new();
    for m in &pending {
        let name = m.name().to_string();
        if name.contains("_publish_reachable_drafts") && seeded.is_none() {
            let s = seed(&mut conn);
            before = statuses(&mut conn);
            seeded = Some(s);
        }
        conn.run_migration(&**m)
            .unwrap_or_else(|e| panic!("migration {name} failed: {e}"));
    }
    let s = seeded.expect("the migration ran");
    let after = statuses(&mut conn);
    let status = |rows: &[Status], id: i32| {
        rows.iter()
            .find(|r| r.id == id)
            .map(|r| r.status.clone())
            .expect("page")
    };

    for (what, id) in s.published.iter().chain(&s.published_public) {
        assert_eq!(status(&after, *id), "published", "{what}");
    }
    for (what, id) in &s.kept {
        assert_eq!(status(&after, *id), status(&before, *id), "{what}");
    }

    // The gap drafting on a published page is resolved by it, and the tickets
    // its live signals cite are linked as resolved by the page.
    let gaps: Vec<Gap> = diesel::sql_query(
        "SELECT id, status, draft_page_id, resolved_page_id, resolved_at IS NOT NULL AS resolved \
         FROM knowledge_gaps",
    )
    .load(&mut conn)
    .expect("gaps");
    let gap = |id: i64| gaps.iter().find(|g| g.id == id).expect("gap");
    let resolved = gap(s.resolved_gap);
    assert_eq!(
        (
            resolved.status.as_str(),
            resolved.draft_page_id,
            resolved.resolved_page_id,
            resolved.resolved
        ),
        ("resolved", None, Some(s.resolved_gap_page), true)
    );
    let drafting = gap(s.drafting_gap);
    assert_eq!(
        (
            drafting.status.as_str(),
            drafting.draft_page_id,
            drafting.resolved_page_id
        ),
        ("drafting", Some(s.drafting_gap_page), None)
    );
    let mut links: Vec<Link> = diesel::sql_query(format!(
        "SELECT ticket_id, link_type FROM documentation_page_tickets WHERE page_id = {}",
        s.resolved_gap_page
    ))
    .load(&mut conn)
    .expect("links");
    links.sort_by_key(|l| l.ticket_id);
    let mut expected = s.cited.clone();
    expected.sort();
    assert_eq!(
        links.iter().map(|l| l.ticket_id).collect::<Vec<_>>(),
        expected,
        "the cited tickets, and not the one a dismissed signal cites ({})",
        s.not_cited
    );
    assert!(links.iter().all(|l| l.link_type == "resolves"));
    drop(conn);

    // The migration agrees with the app's rules: read as the app does, every
    // page it published for the requester is one the rules open to them, and
    // every draft it kept in their workspace is one they don't.
    let pool = r2d2::Pool::builder()
        .max_size(1)
        .test_on_check_out(false)
        .build(backend::db::ResettingManager::new(&db.url))
        .expect("pool");
    let mut conn = pool.get().expect("conn");
    diesel::sql_query(format!(
        "SELECT set_config('app.workspace_id', '{}', false)",
        s.ws
    ))
    .execute(&mut conn)
    .expect("pin workspace");
    // Staff read drafts, so only the rules decide.
    let by_rules = PageAudience::User {
        user_uuid: s.requester,
        is_admin: false,
        is_staff: true,
    };
    let kept_drafts: Vec<(&str, i32)> = s
        .kept
        .iter()
        .copied()
        .filter(|(_, id)| status(&before, *id) == "draft")
        .collect();
    let ids: Vec<i32> = s
        .published
        .iter()
        .chain(&kept_drafts)
        .map(|(_, id)| *id)
        .collect();
    let hidden = by_rules.hidden_pages(&mut conn, &ids).expect("hidden");
    for (what, id) in &s.published {
        assert!(
            !hidden.contains(id),
            "the rules open {what} to the requester"
        );
    }
    for (what, id) in &kept_drafts {
        if what.contains("no requesters") {
            continue;
        }
        assert!(
            hidden.contains(id),
            "the rules close {what} to the requester"
        );
    }
}

#[derive(QueryableByName)]
struct Gap {
    #[diesel(sql_type = BigInt)]
    id: i64,
    #[diesel(sql_type = Text)]
    status: String,
    #[diesel(sql_type = Nullable<Integer>)]
    draft_page_id: Option<i32>,
    #[diesel(sql_type = Nullable<Integer>)]
    resolved_page_id: Option<i32>,
    #[diesel(sql_type = Bool)]
    resolved: bool,
}

#[derive(QueryableByName)]
struct Link {
    #[diesel(sql_type = Integer)]
    ticket_id: i32,
    #[diesel(sql_type = Text)]
    link_type: String,
}

fn statuses(conn: &mut PgConnection) -> Vec<Status> {
    diesel::sql_query("SELECT id, status::text AS status FROM documentation_pages")
        .load(conn)
        .expect("statuses")
}
