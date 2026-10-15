//! CSV user importer integration tests.

use diesel::prelude::*;
use uuid::Uuid;

use backend::services::imports::{self, csv_parser, ImportContext, ImportSummary, ImportType};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

use crate::common::{self, count_table, fixture_path, TestDb, TestPooledConn, WorkspaceSeed};

#[test]
fn happy_path_creates_users_and_primary_emails() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let ws1_admin = new_member(&mut conn, 1, "admin");
    let users_before = count_table(&mut conn, "users");

    let parsed = csv_parser::parse_file(&fixture_path("imports/users_valid.csv")).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Users, &parsed, &ctx_for(ws1_admin))
        .expect("dry-run");
    assert_eq!(summary.row_count, 3);
    assert_eq!(summary.would_create, 3);
    assert_eq!(summary.would_update, 0);
    assert!(summary.errors.is_empty(), "errors: {:?}", summary.errors);

    let records = imports::commit(&mut conn, ImportType::Users, &parsed, &ctx_for(ws1_admin))
        .expect("commit");
    assert_eq!(records.len(), 3);
    assert_eq!(count_table(&mut conn, "users"), users_before + 3);

    // Imports are first-class: each created user routes through the
    // canonical create path, so it emits a user.created sync action
    // (activity feed / webhooks / plugins), not a silent raw INSERT.
    #[derive(diesel::QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let created_events: i64 = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM sync_actions \
         WHERE event_type = 'user.created'",
    )
    .get_result::<CountRow>(&mut *conn)
    .map(|r| r.count)
    .expect("count user.created events");
    assert_eq!(
        created_events, 3,
        "each imported user should emit a user.created sync action"
    );

    // Sentinel: Alex's primary email row landed with the
    // right address and source='csv_import'.
    #[derive(diesel::QueryableByName)]
    struct EmailRow {
        #[diesel(sql_type = diesel::sql_types::Text)]
        email: String,
        #[diesel(sql_type = diesel::sql_types::Bool)]
        is_primary: bool,
        #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::Text>)]
        source: Option<String>,
    }
    let row: EmailRow = diesel::sql_query(
        "SELECT email, is_primary, source FROM user_emails \
         WHERE email = 'alex.kim@example.com'",
    )
    .get_result(&mut *conn)
    .expect("re-read Alex's email");
    assert!(row.is_primary);
    assert_eq!(row.source.as_deref(), Some("csv_import"));
    assert_eq!(row.email, "alex.kim@example.com");
}

#[test]
fn errors_csv_classifies_each_failure() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let ws1_admin = new_member(&mut conn, 1, "admin");

    let parsed = csv_parser::parse_file(&fixture_path("imports/users_errors.csv")).expect("parse");
    let summary = imports::dry_run(&mut conn, ImportType::Users, &parsed, &ctx_for(ws1_admin))
        .expect("dry-run");

    assert_eq!(summary.row_count, 9);
    // valid + the first occurrence of duplicate@example.com →
    // 2 would-creates. nopronouns@example.com fails because
    // name is required.
    assert_eq!(summary.would_create, 2);
    assert_eq!(summary.would_update, 0);

    let expected: &[(&str, &str)] = &[
        ("email", "required"),
        ("email", "not a valid email"),
        ("email", "appears more than once"),
        ("role", "not a valid role"),
        ("name", "required"),
    ];
    for (col, snippet) in expected {
        let hit = summary
            .errors
            .iter()
            .any(|e| e.column.as_deref() == Some(col) && e.message.contains(snippet));
        assert!(
            hit,
            "expected an error on column '{col}' containing '{snippet}'; got {:?}",
            summary.errors
        );
    }
}

#[test]
fn second_commit_upserts_by_email() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let ws1_admin = new_member(&mut conn, 1, "admin");

    let parsed = csv_parser::parse_file(&fixture_path("imports/users_valid.csv")).expect("parse");

    let first = imports::commit(&mut conn, ImportType::Users, &parsed, &ctx_for(ws1_admin))
        .expect("commit 1");
    assert_eq!(first.len(), 3);

    // Second pass: every primary email matches → 3 updates.
    let summary = imports::dry_run(&mut conn, ImportType::Users, &parsed, &ctx_for(ws1_admin))
        .expect("dry-run 2");
    assert_eq!(summary.would_create, 0);
    assert_eq!(summary.would_update, 3);
    assert!(summary.errors.is_empty());

    let second = imports::commit(&mut conn, ImportType::Users, &parsed, &ctx_for(ws1_admin))
        .expect("commit 2");
    assert_eq!(second.len(), 3);

    // No duplicate user_emails rows.
    #[derive(diesel::QueryableByName)]
    struct CountRow {
        #[diesel(sql_type = diesel::sql_types::BigInt)]
        count: i64,
    }
    let n: i64 = diesel::sql_query(
        "SELECT COUNT(*)::bigint AS count FROM user_emails \
         WHERE email = 'alex.kim@example.com'",
    )
    .get_result::<CountRow>(&mut *conn)
    .map(|r| r.count)
    .expect("count emails");
    assert_eq!(n, 1);
}

/// Errors name the line a row starts on, past a multi-line quoted field.
#[test]
fn row_errors_name_the_line_the_row_starts_on() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let ws1_admin = new_member(&mut conn, 1, "admin");
    let file = tempfile::NamedTempFile::new().expect("temp csv");
    std::fs::write(
        file.path(),
        "email,name,role,pronouns\n\
         lines@example.com,\"Two\nLines\",user,\n\
         \n\
         bad-email,Bad Email,user,\n",
    )
    .expect("write csv");
    let parsed = csv_parser::parse_file(file.path()).expect("parse");

    let summary = imports::dry_run(&mut conn, ImportType::Users, &parsed, &ctx_for(ws1_admin))
        .expect("dry-run");
    let errors: Vec<(usize, Option<&str>)> = summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref()))
        .collect();
    assert_eq!(errors, vec![(5, Some("email"))], "{:?}", summary.errors);
}

// ---- Scope: the import only touches the workspace's members --------------

fn parse(content: &str) -> csv_parser::ParsedCsv {
    let file = tempfile::NamedTempFile::new().expect("temp csv");
    std::fs::write(file.path(), content).expect("write csv");
    csv_parser::parse_file(file.path()).expect("parse")
}

fn add_primary_email(conn: &mut TestPooledConn, user: Uuid, email: &str) {
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, email_type, is_primary, is_verified) \
         VALUES ($1, $2, 'personal', true, true)",
    )
    .bind::<diesel::sql_types::Uuid, _>(user)
    .bind::<diesel::sql_types::Text, _>(email)
    .execute(&mut **conn)
    .expect("insert primary email");
}

fn set_platform_role(conn: &mut TestPooledConn, user: Uuid, role: &str) {
    use backend::schema::users;
    diesel::update(users::table.find(user))
        .set(users::platform_role.eq(role))
        .execute(&mut **conn)
        .expect("set platform role");
}

/// `(name, platform_role, pronouns)`.
fn profile(conn: &mut TestPooledConn, user: Uuid) -> (String, String, Option<String>) {
    use backend::schema::users;
    users::table
        .find(user)
        .select((users::name, users::platform_role, users::pronouns))
        .first(&mut **conn)
        .expect("read user")
}

fn member_role(conn: &mut TestPooledConn, ws: i32, user: Uuid) -> Option<String> {
    use backend::schema::workspace_members as wm;
    wm::table
        .filter(wm::workspace_id.eq(ws))
        .filter(wm::user_uuid.eq(user))
        .filter(wm::removed_at.is_null())
        .select(wm::role)
        .first(&mut **conn)
        .optional()
        .expect("read membership")
}

/// A new account holding `role` in workspace `ws`.
fn new_member(conn: &mut TestPooledConn, ws: i32, role: &str) -> Uuid {
    let user = common::insert_plain_user(conn, &format!("{role} of {ws}"));
    grant(conn, ws, user, role);
    user
}

fn grant(conn: &mut TestPooledConn, ws: i32, user: Uuid, role: &str) {
    use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
    let pin = ActorContext::user(user, None).with_workspace(ws);
    with_actor_context::<_, diesel::result::Error>(conn, &pin, |c| {
        add_membership(c, ws, user, role, SeatWriteAuthority::ControlPlane)
    })
    .expect("grant membership");
}

fn ctx_for(actor: Uuid) -> ImportContext {
    ImportContext {
        actor: Some(actor),
        job_id: Uuid::new_v4(),
        options: Default::default(),
    }
}

/// Dry-run then commit `parsed` into `ws`, pinned as the import handler pins
/// it, with `actor` as the importing admin. Returns the dry run and the
/// number of rows committed.
fn import_into(
    conn: &mut TestPooledConn,
    ws: &WorkspaceSeed,
    actor: Uuid,
    parsed: &csv_parser::ParsedCsv,
) -> (ImportSummary, usize) {
    let pin = ActorContext::user(actor, None).with_workspace(ws.workspace_id);
    let ctx = ctx_for(actor);
    let summary = with_actor_context::<_, diesel::result::Error>(conn, &pin, |c| {
        imports::dry_run(c, ImportType::Users, parsed, &ctx)
    })
    .expect("dry-run");
    let committed = with_actor_context::<_, imports::CommitError>(conn, &pin, |c| {
        imports::commit(c, ImportType::Users, parsed, &ctx)
    })
    .map_err(|e| format!("{e}"))
    .expect("commit")
    .len();
    (summary, committed)
}

fn errors_of(summary: &ImportSummary) -> Vec<(usize, Option<&str>, &str)> {
    summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref(), e.message.as_str()))
        .collect()
}

/// A row naming an account that belongs only to another workspace is refused
/// as the add-user API refuses it, and the account is left as it was.
#[actix_web::test]
async fn an_account_in_another_workspace_is_refused_like_add_user() {
    common::ensure_test_keyring();
    let db = TestDb::new();
    let pool = db.pool_with_size(4);
    let mut conn = pool.get().expect("conn");
    let seeded = common::seed_two_workspaces(&mut conn);
    let (a, b) = (&seeded.a, &seeded.b);
    add_primary_email(&mut conn, b.member_uuid, "bee@example.com");
    let before = profile(&mut conn, b.member_uuid);

    let parsed = parse("email,name,role,pronouns\nBee@example.com,Renamed,admin,xe/xem\n");
    let (summary, committed) = import_into(&mut conn, a, a.admin_uuid, &parsed);
    assert_eq!((summary.would_create, summary.would_update), (0, 0));
    assert_eq!(
        errors_of(&summary),
        vec![(2, Some("email"), "User with this email already exists")]
    );
    assert_eq!(committed, 0);
    assert_eq!(profile(&mut conn, b.member_uuid), before);
    assert_eq!(member_role(&mut conn, a.workspace_id, b.member_uuid), None);
    assert_eq!(
        member_role(&mut conn, b.workspace_id, b.member_uuid).as_deref(),
        Some("member")
    );

    // The add-user API gives the same answer for the same address.
    let (status, body) = crate::add_requester::post_user(
        &pool,
        a,
        serde_json::json!({
            "name": "Renamed",
            "email": "bee@example.com",
            "role": "user",
            "send_invitation": false,
        }),
    )
    .await;
    assert_eq!(status, actix_web::http::StatusCode::BAD_REQUEST, "{body}");
    assert!(
        body.to_string()
            .contains("User with this email already exists"),
        "{body}"
    );
}

/// A platform admin who isn't a member keeps their name and platform role.
#[test]
fn a_platform_admin_outside_the_workspace_is_left_alone() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let a = common::seed_two_workspaces(&mut conn).a;
    let operator = common::insert_user(&mut conn, "Operator");
    add_primary_email(&mut conn, operator.uuid, "operator@example.com");

    let parsed = parse("email,name,role,pronouns\noperator@example.com,Demoted,user,\n");
    let (summary, committed) = import_into(&mut conn, &a, a.admin_uuid, &parsed);
    assert_eq!(
        errors_of(&summary),
        vec![(2, Some("email"), "User with this email already exists")]
    );
    assert_eq!(committed, 0);
    let (name, platform_role, _) = profile(&mut conn, operator.uuid);
    assert_eq!(name, "Operator");
    assert_eq!(platform_role, "platform_admin");
    assert_eq!(member_role(&mut conn, a.workspace_id, operator.uuid), None);
}

/// `audit_reviewer` is never granted to an existing member. A new account
/// gets it as the add-user API gives it: self-hosted, by a workspace admin
/// (the hosted refusal is in `tests/imports_users_hosted.rs`).
#[test]
fn audit_reviewer_is_refused_on_update_and_created_as_add_user_does() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let a = common::seed_two_workspaces(&mut conn).a;
    add_primary_email(&mut conn, a.member_uuid, "member@example.com");

    let parsed = parse(
        "email,name,role,pronouns\n\
         member@example.com,a Member,audit_reviewer,\n\
         reviewer@example.com,New Reviewer,audit_reviewer,\n",
    );
    let (summary, committed) = import_into(&mut conn, &a, a.admin_uuid, &parsed);
    assert_eq!(
        errors_of(&summary),
        vec![(
            2,
            Some("role"),
            "audit_reviewer can't be set by import; set it from the user's page"
        )]
    );
    assert_eq!((summary.would_create, summary.would_update), (1, 0));
    assert_eq!(committed, 1);
    assert_eq!(profile(&mut conn, a.member_uuid).1, "user");

    use backend::schema::{user_emails, users};
    let created: (String, Uuid) = users::table
        .inner_join(user_emails::table.on(user_emails::user_uuid.eq(users::uuid)))
        .filter(user_emails::email.eq("reviewer@example.com"))
        .select((users::platform_role, users::uuid))
        .first(&mut *conn)
        .expect("created reviewer");
    assert_eq!(created.0, "audit_reviewer");
    assert_eq!(
        member_role(&mut conn, a.workspace_id, created.1).as_deref(),
        Some("member")
    );
}

/// A platform admin who owns the workspace renames a member and changes their
/// workspace role; the member's platform role stays whatever the CSV says.
#[test]
fn a_member_is_renamed_and_keeps_their_platform_role() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let a = common::seed_two_workspaces(&mut conn).a;
    let operator = common::insert_user(&mut conn, "Operator");
    grant(&mut conn, a.workspace_id, operator.uuid, "owner");
    add_primary_email(&mut conn, a.member_uuid, "member@example.com");
    set_platform_role(&mut conn, a.member_uuid, "audit_reviewer");

    let parsed =
        parse("email,name,role,pronouns\nmember@example.com,Renamed Member,technician,she/her\n");
    let (summary, committed) = import_into(&mut conn, &a, operator.uuid, &parsed);
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!((summary.would_create, summary.would_update), (0, 1));
    assert_eq!(committed, 1);
    assert_eq!(
        profile(&mut conn, a.member_uuid),
        (
            "Renamed Member".to_string(),
            "audit_reviewer".to_string(),
            Some("she/her".to_string())
        )
    );
    assert_eq!(
        member_role(&mut conn, a.workspace_id, a.member_uuid).as_deref(),
        Some("agent")
    );
}

/// A workspace admin can't rename another member, as on the user's page; the
/// row is refused whole. Their own row they may.
#[test]
fn a_workspace_admin_renames_only_themselves() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let a = common::seed_two_workspaces(&mut conn).a;
    add_primary_email(&mut conn, a.member_uuid, "member@example.com");
    add_primary_email(&mut conn, a.admin_uuid, "admin@example.com");

    let parsed = parse(
        "email,name,role,pronouns\n\
         member@example.com,Renamed Member,technician,he/him\n\
         admin@example.com,Renamed Admin,admin,\n",
    );
    let (summary, committed) = import_into(&mut conn, &a, a.admin_uuid, &parsed);
    assert_eq!(
        errors_of(&summary),
        vec![
            (
                2,
                Some("name"),
                "only the person or a platform admin can change their name"
            ),
            (
                2,
                Some("pronouns"),
                "only the person or a platform admin can change their pronouns"
            ),
        ]
    );
    assert_eq!((summary.would_create, summary.would_update), (0, 1));
    assert_eq!(committed, 1);
    assert_eq!(profile(&mut conn, a.member_uuid).0, "a Member");
    assert_eq!(
        member_role(&mut conn, a.workspace_id, a.member_uuid).as_deref(),
        Some("member")
    );
    assert_eq!(profile(&mut conn, a.admin_uuid).0, "Renamed Admin");
}

// ---- Roles: the member-role endpoint's gate ------------------------------

const FORBIDDEN_TIER: &str =
    "You can only manage members and agents; managing admins or owners requires the owner role";

/// A workspace admin can't give anyone admin, new or existing, as the
/// member-role endpoint refuses it; nothing is written.
#[test]
fn a_workspace_admin_cannot_assign_admin() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let a = common::seed_two_workspaces(&mut conn).a;
    add_primary_email(&mut conn, a.member_uuid, "member@example.com");

    let parsed = parse(
        "email,name,role,pronouns\n\
         new.admin@example.com,New Admin,admin,\n\
         member@example.com,a Member,admin,\n",
    );
    let (summary, committed) = import_into(&mut conn, &a, a.admin_uuid, &parsed);
    assert_eq!(
        errors_of(&summary),
        vec![
            (2, Some("role"), FORBIDDEN_TIER),
            (3, Some("role"), FORBIDDEN_TIER)
        ]
    );
    assert_eq!((summary.would_create, summary.would_update), (0, 0));
    assert_eq!(committed, 0);
    assert_eq!(
        member_role(&mut conn, a.workspace_id, a.member_uuid).as_deref(),
        Some("member")
    );
    use backend::schema::user_emails;
    let created: i64 = user_emails::table
        .filter(user_emails::email.eq("new.admin@example.com"))
        .count()
        .get_result(&mut *conn)
        .expect("count");
    assert_eq!(created, 0);
}

/// Roles the endpoint allows still import: an admin gives agent, an owner
/// gives admin, on new and existing members alike.
#[test]
fn roles_the_endpoint_allows_are_imported() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let a = common::seed_two_workspaces(&mut conn).a;
    let owner = new_member(&mut conn, a.workspace_id, "owner");
    add_primary_email(&mut conn, a.member_uuid, "member@example.com");

    let by_admin = parse(
        "email,name,role,pronouns\n\
         new.agent@example.com,New Agent,technician,\n\
         member@example.com,a Member,technician,\n",
    );
    let (summary, committed) = import_into(&mut conn, &a, a.admin_uuid, &by_admin);
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!((summary.would_create, summary.would_update), (1, 1));
    assert_eq!(committed, 2);
    assert_eq!(
        member_role(&mut conn, a.workspace_id, a.member_uuid).as_deref(),
        Some("agent")
    );

    let by_owner = parse(
        "email,name,role,pronouns\n\
         new.admin@example.com,New Admin,admin,\n\
         member@example.com,a Member,admin,\n",
    );
    let (summary, committed) = import_into(&mut conn, &a, owner, &by_owner);
    assert!(summary.errors.is_empty(), "{:?}", summary.errors);
    assert_eq!(committed, 2);
    assert_eq!(
        member_role(&mut conn, a.workspace_id, a.member_uuid).as_deref(),
        Some("admin")
    );
    use backend::schema::user_emails;
    let created: Uuid = user_emails::table
        .filter(user_emails::email.eq("new.admin@example.com"))
        .select(user_emails::user_uuid)
        .first(&mut *conn)
        .expect("new admin");
    assert_eq!(
        member_role(&mut conn, a.workspace_id, created).as_deref(),
        Some("admin")
    );
}

/// The only owner can't demote themselves, as the endpoint refuses it.
#[test]
fn the_only_owner_cannot_be_demoted() {
    let db = TestDb::new();
    let mut conn = db.conn();
    let a = common::seed_two_workspaces(&mut conn).a;
    let owner = new_member(&mut conn, a.workspace_id, "owner");
    add_primary_email(&mut conn, owner, "owner@example.com");

    let parsed = parse("email,name,role,pronouns\nowner@example.com,owner of x,admin,\n");
    let (summary, committed) = import_into(&mut conn, &a, owner, &parsed);
    assert_eq!(
        errors_of(&summary),
        vec![(
            2,
            Some("role"),
            "cannot demote the only owner; promote another member first"
        )]
    );
    assert_eq!(committed, 0);
    assert_eq!(
        member_role(&mut conn, a.workspace_id, owner).as_deref(),
        Some("owner")
    );
}
