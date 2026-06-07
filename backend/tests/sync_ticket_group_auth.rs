//! DB-backed coverage for `ticket:<id>` sync-group admission.
//!
//! `sync::groups::admit_ticket_groups` is the read-side gate that lets
//! the pool-native ticket detail view subscribe to a ticket's group on
//! bootstrap + delta. It must admit only tickets the caller can read
//! (staff: any ticket; end-user: requester or watcher), reusing the
//! same `can_view_ticket` predicate the REST detail endpoint uses, so
//! a subscription can never stream a ticket the caller can't see.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use uuid::Uuid;

use backend::models::{NewTicket, NewUser, PlatformRole, Ticket, User, WorkspaceRole};
use backend::repository::ticket_visibility::VisibilityContext;
use backend::sync::groups::admit_ticket_groups;

mod common;

const WS: i32 = 1;

fn default_state_id(conn: &mut PgConnection) -> i32 {
    use backend::schema::workflow_states::dsl as s;
    s::workflow_states
        .filter(s::workspace_id.eq(WS))
        .filter(s::is_default.eq(true))
        .select(s::id)
        .first(conn)
        .expect("default workflow state seeded")
}

fn user(conn: &mut PgConnection, name: &str) -> User {
    use backend::schema::users;
    diesel::insert_into(users::table)
        .values(&NewUser {
            uuid: Uuid::new_v4(),
            name: name.to_string(),
            pronouns: None,
            avatar_url: None,
            banner_url: None,
            avatar_thumb: None,
            microsoft_uuid: None,
            mfa_secret: None,
            mfa_secret_kek_id: None,
            mfa_enabled: false,
            platform_role: None,
        })
        .get_result(conn)
        .expect("insert user")
}

fn ticket(conn: &mut PgConnection, title: &str, requester: Uuid) -> Ticket {
    use backend::schema::tickets;
    let state_id = default_state_id(conn);
    diesel::insert_into(tickets::table)
        .values(&NewTicket {
            title: title.to_string(),
            workflow_state_id: state_id,
            requester_uuid: Some(requester),
            ..Default::default()
        })
        .get_result(conn)
        .expect("insert ticket")
}

/// Staff (workspace agent or above) see every ticket, so their
/// `ticket:<id>` subscription is admitted.
#[test]
fn staff_admits_any_ticket_group() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();

    let requester = user(&mut conn, "Requester");
    let t = ticket(&mut conn, "Printer down", requester.uuid);

    let staff = user(&mut conn, "Agent");
    let vis = VisibilityContext::new(
        staff.uuid,
        PlatformRole::from_db("user"),
        Some(WorkspaceRole::Agent),
    );

    let mut granted = vec!["workspace:1".to_string()];
    admit_ticket_groups(&mut conn, &format!("ticket:{}", t.id), &vis, &mut granted);
    assert!(
        granted.contains(&format!("ticket:{}", t.id)),
        "staff should be admitted to any ticket group, got {granted:?}"
    );
}

/// An end-user is admitted to their own (requested) ticket but not to
/// a stranger's ticket — the gate collapses to `can_view_ticket`.
#[test]
fn end_user_admitted_only_to_own_ticket() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();

    let requester = user(&mut conn, "Owner");
    let stranger = user(&mut conn, "Stranger");
    let mine = ticket(&mut conn, "My laptop", requester.uuid);
    let theirs = ticket(&mut conn, "Their laptop", stranger.uuid);

    let vis = VisibilityContext::new(requester.uuid, PlatformRole::from_db("user"), None);

    let mut granted: Vec<String> = Vec::new();
    admit_ticket_groups(
        &mut conn,
        &format!("ticket:{},ticket:{}", mine.id, theirs.id),
        &vis,
        &mut granted,
    );
    assert!(
        granted.contains(&format!("ticket:{}", mine.id)),
        "requester should be admitted to their own ticket, got {granted:?}"
    );
    assert!(
        !granted.contains(&format!("ticket:{}", theirs.id)),
        "requester must NOT be admitted to a stranger's ticket, got {granted:?}"
    );
}

/// Malformed entries are skipped, non-ticket groups are ignored, and an
/// already-present ticket group isn't duplicated.
#[test]
fn admission_skips_malformed_and_dedupes() {
    common::ensure_test_keyring();
    let db = common::TestDb::new();
    let mut conn = db.conn();

    let requester = user(&mut conn, "Requester");
    let t = ticket(&mut conn, "Onboarding", requester.uuid);

    let staff = user(&mut conn, "Admin");
    let vis = VisibilityContext::new(
        staff.uuid,
        PlatformRole::from_db("user"),
        Some(WorkspaceRole::Admin),
    );

    // Pre-seed the ticket group; admission must not duplicate it.
    let mut granted = vec![format!("ticket:{}", t.id), "project:7".to_string()];
    admit_ticket_groups(
        &mut conn,
        &format!("ticket:{},ticket:notanumber,project:7,workspace:1", t.id),
        &vis,
        &mut granted,
    );

    let occurrences = granted
        .iter()
        .filter(|g| *g == &format!("ticket:{}", t.id))
        .count();
    assert_eq!(
        occurrences, 1,
        "ticket group must not be duplicated: {granted:?}"
    );
    assert!(
        !granted.iter().any(|g| g == "ticket:notanumber"),
        "malformed ticket group must be skipped: {granted:?}"
    );
}
