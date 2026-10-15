//! On hosted, the user import leaves staff seats to the control plane and
//! can't create an audit reviewer, as the add-user API can't.
//!
//! A standalone binary rather than part of `tests/it`: deployment mode is
//! process-wide environment.

#![allow(clippy::expect_used)]

mod common;

use diesel::prelude::*;
use uuid::Uuid;

use backend::services::imports::{self, csv_parser, ImportContext, ImportType};
use backend::sync::actor::ActorContext;
use backend::sync::session::with_actor_context;

fn add_primary_email(conn: &mut common::TestPooledConn, user: Uuid, email: &str) {
    diesel::sql_query(
        "INSERT INTO user_emails (user_uuid, email, email_type, is_primary, is_verified) \
         VALUES ($1, $2, 'personal', true, true)",
    )
    .bind::<diesel::sql_types::Uuid, _>(user)
    .bind::<diesel::sql_types::Text, _>(email)
    .execute(conn)
    .expect("insert primary email");
}

#[test]
fn staff_seats_and_audit_reviewers_are_refused_on_hosted() {
    std::env::set_var("NOSDESK_DEPLOYMENT_MODE", "hosted");
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();
    let a = common::seed_two_workspaces(&mut conn).a;
    add_primary_email(&mut conn, a.admin_uuid, "admin@example.com");

    let file = tempfile::NamedTempFile::new().expect("temp csv");
    std::fs::write(
        file.path(),
        "email,name,role,pronouns\n\
         admin@example.com,Renamed Admin,user,\n\
         agent@example.com,New Agent,technician,\n\
         reviewer@example.com,New Reviewer,audit_reviewer,\n\
         requester@example.com,New Requester,user,\n",
    )
    .expect("write csv");
    let parsed = csv_parser::parse_file(file.path()).expect("parse");

    let pin = ActorContext::user(a.admin_uuid, None).with_workspace(a.workspace_id);
    let ctx = ImportContext {
        actor: Some(a.admin_uuid),
        job_id: Uuid::new_v4(),
    };
    let summary = with_actor_context::<_, diesel::result::Error>(&mut conn, &pin, |c| {
        imports::dry_run(c, ImportType::Users, &parsed, &ctx)
    })
    .expect("dry-run");
    let staff = "team members (admin, technician) are managed in the Nosdesk control plane; import requesters (user) only";
    let errors: Vec<(usize, Option<&str>, &str)> = summary
        .errors
        .iter()
        .map(|e| (e.row, e.column.as_deref(), e.message.as_str()))
        .collect();
    assert_eq!(
        errors,
        vec![
            (2, Some("role"), staff),
            (
                2,
                Some("name"),
                "this person's name is managed in their Nosdesk account"
            ),
            (3, Some("role"), staff),
            (
                4,
                Some("role"),
                "audit_reviewer can't be set by import; set it from the user's page"
            ),
        ]
    );
    assert_eq!((summary.would_create, summary.would_update), (1, 0));

    let committed = with_actor_context::<_, imports::CommitError>(&mut conn, &pin, |c| {
        imports::commit(c, ImportType::Users, &parsed, &ctx)
    })
    .map_err(|e| format!("{e}"))
    .expect("commit")
    .len();
    assert_eq!(committed, 1);

    use backend::schema::{user_emails, users, workspace_members as wm};
    let name: String = users::table
        .find(a.admin_uuid)
        .select(users::name)
        .first(&mut conn)
        .expect("admin");
    assert_eq!(name, "a Admin");
    let role: String = wm::table
        .filter(wm::workspace_id.eq(a.workspace_id))
        .filter(wm::user_uuid.eq(a.admin_uuid))
        .filter(wm::removed_at.is_null())
        .select(wm::role)
        .first(&mut conn)
        .expect("admin membership");
    assert_eq!(role, "admin");
    let created: Vec<String> = user_emails::table
        .filter(user_emails::email.eq_any([
            "agent@example.com",
            "reviewer@example.com",
            "requester@example.com",
        ]))
        .select(user_emails::email)
        .load(&mut conn)
        .expect("created emails");
    assert_eq!(created, vec!["requester@example.com".to_string()]);
}
