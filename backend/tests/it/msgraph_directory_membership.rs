//! The Microsoft directory sync makes the people it brings in members of the
//! workspace it runs for. A new account gets its membership and `user.created`
//! there and nowhere else; a person an earlier run created without one is
//! repaired the next time the sync visits them. The sync runs elevated past
//! row security and pinned to its workspace, as `sync_users` does.

use diesel::prelude::*;
use uuid::Uuid;

use backend::db::DbConnection;
use backend::handlers::msgraph_integration::{
    process_microsoft_user_no_photos, MicrosoftGraphUser, UserSyncStats,
};
use backend::models::{NewUserAuthIdentity, UserAuthIdentity};
use backend::repository::user_auth_identities as identity_repo;
use backend::schema::{sync_actions, user_auth_identities, workspace_members};
use backend::sync::actor::ActorContext;
use backend::sync::session::{elevate_session_role, reset_session_role};

use crate::common::{self, TestPool};

const REF: &str = "test:msgraph_directory_membership";

/// Run `f` the way the Microsoft sync does: elevated for the session and
/// pinned to `workspace_id`.
fn as_sync<T>(pool: &TestPool, workspace_id: i32, f: impl FnOnce(&mut DbConnection) -> T) -> T {
    let mut conn = pool.get().expect("conn");
    let actor = ActorContext::system(REF).with_workspace(workspace_id);
    elevate_session_role(&mut conn, &actor).expect("elevate");
    let out = f(&mut conn);
    reset_session_role(&mut conn);
    out
}

fn directory_person(name: &str) -> MicrosoftGraphUser {
    let id = Uuid::new_v4();
    let address = format!("{}@directory.test", id.simple());
    serde_json::from_value(serde_json::json!({
        "id": id.to_string(),
        "displayName": name,
        "mail": address,
        "userPrincipalName": address,
    }))
    .expect("graph user")
}

fn empty_stats() -> UserSyncStats {
    UserSyncStats {
        new_users_created: 0,
        existing_users_updated: 0,
        identities_linked: 0,
        errors: Vec::new(),
        failures: Vec::new(),
    }
}

/// Run one person through the users pass in `workspace_id`.
fn sync_person(
    pool: &TestPool,
    workspace_id: i32,
    person: &MicrosoftGraphUser,
    identity: Option<UserAuthIdentity>,
) -> UserSyncStats {
    as_sync(pool, workspace_id, |c| {
        let mut stats = empty_stats();
        futures::executor::block_on(process_microsoft_user_no_photos(
            c,
            0,
            workspace_id,
            person,
            identity,
            &mut stats,
        ))
        .expect("process directory person");
        stats
    })
}

/// The person's active memberships, as (workspace, role).
fn memberships(pool: &TestPool, any_workspace: i32, user: Uuid) -> Vec<(i32, String)> {
    as_sync(pool, any_workspace, |c| {
        workspace_members::table
            .filter(workspace_members::user_uuid.eq(user))
            .filter(workspace_members::removed_at.is_null())
            .select((workspace_members::workspace_id, workspace_members::role))
            .order(workspace_members::workspace_id)
            .load(c)
            .expect("memberships")
    })
}

/// The workspaces each of the person's user events went to, as (event, workspace).
fn user_events(pool: &TestPool, any_workspace: i32, user: Uuid) -> Vec<(String, i32)> {
    as_sync(pool, any_workspace, |c| {
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(user.to_string()))
            .filter(sync_actions::event_type.like("user.%"))
            .select((sync_actions::event_type, sync_actions::workspace_id))
            .order(sync_actions::sync_id)
            .load(c)
            .expect("user events")
    })
}

fn user_of(pool: &TestPool, any_workspace: i32, person: &MicrosoftGraphUser) -> Uuid {
    as_sync(pool, any_workspace, |c| {
        user_auth_identities::table
            .filter(user_auth_identities::provider_type.eq("microsoft"))
            .filter(user_auth_identities::external_id.eq(&person.id))
            .select(user_auth_identities::user_uuid)
            .first(c)
            .expect("the sync linked the person")
    })
}

/// An account with a Microsoft identity and no membership anywhere, the way
/// the sync created people before it granted one.
fn bare_linked_account(
    pool: &TestPool,
    workspace_id: i32,
    person: &MicrosoftGraphUser,
) -> (Uuid, UserAuthIdentity) {
    as_sync(pool, workspace_id, |c| {
        let user = common::insert_plain_user(c, "Bare Directory Person");
        let identity = identity_repo::create_identity(
            NewUserAuthIdentity {
                user_uuid: user,
                provider_type: "microsoft".to_string(),
                external_id: person.id.clone(),
                email: person.mail.clone(),
                metadata: None,
                password_hash: None,
                workspace_id: None,
            },
            c,
        )
        .expect("identity");
        (user, identity)
    })
}

#[test]
fn a_person_the_sync_creates_joins_only_the_workspace_it_runs_for() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (ws.a.workspace_id, ws.b.workspace_id);

    let person = directory_person("New Directory Person");
    let stats = sync_person(&pool, a, &person, None);
    assert_eq!(stats.new_users_created, 1);
    assert!(stats.failures.is_empty(), "{:?}", stats.errors);

    let user = user_of(&pool, a, &person);
    assert_eq!(memberships(&pool, a, user), vec![(a, "member".to_string())]);
    assert_eq!(
        user_events(&pool, a, user),
        vec![("user.created".to_string(), a)],
        "one user.created, in A only (B is {b})"
    );
}

#[test]
fn a_linked_person_without_a_membership_is_repaired_by_the_next_run() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (ws.a.workspace_id, ws.b.workspace_id);

    let person = directory_person("Earlier Directory Person");
    let (user, identity) = bare_linked_account(&pool, a, &person);
    assert!(memberships(&pool, a, user).is_empty());

    let stats = sync_person(&pool, a, &person, Some(identity.clone()));
    assert_eq!(stats.existing_users_updated, 1);
    assert_eq!(memberships(&pool, a, user), vec![(a, "member".to_string())]);
    let joined = user_events(&pool, a, user);
    assert!(
        joined.contains(&("user.updated".to_string(), a)),
        "the repair tells A's clients: {joined:?}"
    );
    assert!(
        joined.iter().all(|(_, w)| *w == a),
        "nothing reaches B ({b}): {joined:?}"
    );

    // A second run finds the membership in place and sends nothing for it:
    // only the profile refresh every run makes (`update_user`) is new.
    sync_person(&pool, a, &person, Some(identity));
    let again = user_events(&pool, a, user);
    assert_eq!(again[..joined.len()], joined[..]);
    assert_eq!(again[joined.len()..], [("user.updated".to_string(), a)]);
}

#[test]
fn the_sync_keeps_an_existing_role_and_a_removal() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let ws = common::seed_two_workspaces(&mut pool.get().expect("conn"));
    let (a, b) = (ws.a.workspace_id, ws.b.workspace_id);

    // An agent in A keeps their role.
    let agent = directory_person("Directory Agent");
    let (agent_uuid, agent_identity) = bare_linked_account(&pool, a, &agent);
    as_sync(&pool, a, |c| {
        backend::repository::workspaces::ensure_membership(c, a, agent_uuid, "agent")
    })
    .expect("agent membership");
    sync_person(&pool, a, &agent, Some(agent_identity));
    assert_eq!(
        memberships(&pool, a, agent_uuid),
        vec![(a, "agent".to_string())]
    );

    // Someone an admin removed from B stays removed when B's sync visits them.
    let removed = directory_person("Removed Directory Person");
    let (removed_uuid, removed_identity) = bare_linked_account(&pool, b, &removed);
    as_sync(&pool, b, |c| {
        backend::repository::workspaces::ensure_membership(c, b, removed_uuid, "member")?;
        backend::repository::workspaces::remove_membership(
            c,
            b,
            removed_uuid,
            backend::repository::workspaces::SeatWriteAuthority::ControlPlane,
        )
    })
    .expect("remove from B");
    sync_person(&pool, b, &removed, Some(removed_identity));
    assert!(memberships(&pool, b, removed_uuid).is_empty());
}
