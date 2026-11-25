//! A workspace's directory sync speaks only for that workspace's people.
//!
//! Accounts are global and memberships are per workspace, so a directory one
//! workspace syncs must not reach an account only another workspace holds:
//! the Microsoft sync in B neither links, seats, renames, verifies nor strips
//! the addresses of an account seated only in A, and gives it no sign-in
//! identity; the LDAP provisioner in B doesn't link it either. For an account
//! both hold, B's directory names the person in B and leaves the account's
//! name alone. The address writers the Microsoft sync uses touch only their
//! own rows, a directory group in B takes in no one only A holds, the
//! portal's add-participant says nothing about the account an address
//! reached, and `user.*` webhooks and other members' sync rows carry no
//! platform role or dashboard layout.

use actix_web::dev::Service;
use actix_web::test as http_test;
use actix_web::{web, App, HttpMessage};
use diesel::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

use backend::db::DbConnection;
use backend::extractors::WorkspaceContext;
use backend::handlers::msgraph_integration::{
    apply_delta_group_membership, photo_sync_candidates, process_microsoft_user_no_photos,
    revoke_removed_directory_people, MicrosoftGraphGroupMember, MicrosoftGraphUser, UserSyncStats,
};
use backend::handlers::portal::PortalContext;
use backend::middleware::RequestContext;
use backend::models::{
    NewTicket, NewUserAuthIdentity, NewUserEmail, SyncAggregate, UserAuthIdentity,
};
use backend::repository::user_auth_identities as identity_repo;
use backend::repository::user_emails as emails_repo;
use backend::repository::workspaces::{add_membership, SeatWriteAuthority};
use backend::schema::{
    sync_actions, user_auth_identities, user_emails, user_profiles, users, webhook_deliveries,
    workspace_members,
};
use backend::services::oauth_provisioning::{find_or_create_projected_user, ProjectedUserInput};
use backend::services::webhooks::WebhookService;
use backend::sync::actor::ActorContext;
use backend::sync::session::{
    elevate_session_role, reset_session_role, run_in_workspace, with_actor_bypass_context,
    with_actor_context,
};
use backend::sync::visibility::{asset_keys_for, project_row, SyncViewer};

use crate::common::{self, TestPool, TwoWorkspaces};

const REF: &str = "test:directory_sync_account_scope";

struct Fixture {
    _db: common::TestDb,
    pool: TestPool,
    ws: TwoWorkspaces,
}

impl Fixture {
    fn new() -> Self {
        let db = common::TestDb::new();
        let pool = db.pool_with_size(4);
        let ws = common::seed_two_workspaces(&mut pool.get().expect("conn"));
        Self { _db: db, pool, ws }
    }

    fn a(&self) -> i32 {
        self.ws.a.workspace_id
    }

    fn b(&self) -> i32 {
        self.ws.b.workspace_id
    }

    /// Run `f` the way the Microsoft sync does: elevated for the session and
    /// pinned to `workspace_id`.
    fn as_graph_sync<T>(&self, workspace_id: i32, f: impl FnOnce(&mut DbConnection) -> T) -> T {
        let mut conn = self.pool.get().expect("conn");
        let actor = ActorContext::system(REF).with_workspace(workspace_id);
        elevate_session_role(&mut conn, &actor).expect("elevate");
        let out = f(&mut conn);
        reset_session_role(&mut conn);
        out
    }

    /// Read across every workspace.
    fn read<T>(&self, f: impl FnOnce(&mut DbConnection) -> QueryResult<T>) -> T {
        let mut conn = self.pool.get().expect("conn");
        with_actor_bypass_context(&mut conn, &ActorContext::system(REF), f).expect("read")
    }

    /// An account seated in each of `workspaces`, with a verified primary
    /// address and an unverified secondary one added by hand.
    fn account(&self, name: &str, workspaces: &[i32]) -> Account {
        let uuid = common::insert_plain_user(&mut self.pool.get().expect("conn"), name);
        let tag = Uuid::new_v4().simple().to_string();
        let primary = format!("primary-{tag}@victim.test");
        let secondary = format!("secondary-{tag}@victim.test");
        self.read(|c| {
            for (email, is_primary, is_verified, source) in [
                (&primary, true, true, None),
                (&secondary, false, false, Some("manual".to_string())),
            ] {
                diesel::insert_into(user_emails::table)
                    .values(&NewUserEmail {
                        user_uuid: uuid,
                        email: email.clone(),
                        email_type: "work".to_string(),
                        is_primary,
                        is_verified,
                        source,
                    })
                    .execute(c)?;
            }
            Ok(())
        });
        for &ws in workspaces {
            run_in_workspace(&self.pool, REF, ws, |c| {
                add_membership(c, ws, uuid, "member", SeatWriteAuthority::ControlPlane)
            })
            .expect("seat");
        }
        Account {
            uuid,
            name: name.to_string(),
            primary,
            secondary,
        }
    }

    fn remove(&self, workspace_id: i32, user: Uuid) {
        run_in_workspace(&self.pool, REF, workspace_id, |c| {
            backend::repository::workspaces::remove_membership(
                c,
                workspace_id,
                user,
                SeatWriteAuthority::ControlPlane,
            )
        })
        .expect("remove");
    }

    /// An LDAP identity for `user` in `workspace_id`, as a sync before the
    /// link rule could have left it.
    fn ldap_identity(&self, workspace_id: i32, user: Uuid, external_id: &str) {
        run_in_workspace(&self.pool, REF, workspace_id, |c| {
            identity_repo::create_identity(
                NewUserAuthIdentity {
                    user_uuid: user,
                    provider_type: "ldap".to_string(),
                    external_id: external_id.to_string(),
                    email: None,
                    metadata: None,
                    password_hash: None,
                    workspace_id: Some(workspace_id),
                },
                c,
            )
        })
        .expect("ldap identity");
    }

    fn session(&self, user: Uuid) {
        self.read(|c| {
            backend::repository::active_sessions::create_session(
                c,
                backend::models::NewActiveSession {
                    user_uuid: user,
                    device_name: None,
                    ip_address: None,
                    user_agent: None,
                    location: None,
                    expires_at: (chrono::Utc::now() + chrono::Duration::hours(1)).naive_utc(),
                    oidc_id_token: None,
                    confirmed_at: None,
                    provider_amr: None,
                },
            )
        });
    }

    fn sessions(&self, user: Uuid) -> i64 {
        self.read(|c| {
            backend::schema::active_sessions::table
                .filter(backend::schema::active_sessions::user_uuid.eq(user))
                .count()
                .get_result(c)
        })
    }

    fn name_of(&self, user: Uuid) -> String {
        self.read(|c| users::table.find(user).select(users::name).first(c))
    }

    /// (address, primary, verified, source) for each of the account's rows.
    fn addresses(&self, user: Uuid) -> Vec<(String, bool, bool, Option<String>)> {
        self.read(|c| {
            user_emails::table
                .filter(user_emails::user_uuid.eq(user))
                .order(user_emails::email)
                .select((
                    user_emails::email,
                    user_emails::is_primary,
                    user_emails::is_verified,
                    user_emails::source,
                ))
                .load(c)
        })
    }

    fn seats(&self, user: Uuid) -> Vec<i32> {
        self.read(|c| {
            workspace_members::table
                .filter(workspace_members::user_uuid.eq(user))
                .filter(workspace_members::removed_at.is_null())
                .order(workspace_members::workspace_id)
                .select(workspace_members::workspace_id)
                .load(c)
        })
    }

    fn identities(&self, user: Uuid) -> Vec<(String, Option<i32>)> {
        self.read(|c| {
            user_auth_identities::table
                .filter(user_auth_identities::user_uuid.eq(user))
                .select((
                    user_auth_identities::provider_type,
                    user_auth_identities::workspace_id,
                ))
                .load(c)
        })
    }

    fn local_name(&self, workspace_id: i32, user: Uuid) -> Option<String> {
        self.read(|c| {
            user_profiles::table
                .filter(user_profiles::workspace_id.eq(workspace_id))
                .filter(user_profiles::user_uuid.eq(user))
                .select(user_profiles::display_name)
                .first::<Option<String>>(c)
                .optional()
        })
        .flatten()
    }

    /// A Microsoft identity for `user`, as an earlier sync in A left it.
    fn microsoft_identity(&self, user: Uuid, person: &MicrosoftGraphUser) -> UserAuthIdentity {
        self.as_graph_sync(self.a(), |c| {
            identity_repo::create_identity(
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
        })
        .expect("identity")
    }

    fn graph_sync(
        &self,
        workspace_id: i32,
        person: &MicrosoftGraphUser,
        identity: Option<UserAuthIdentity>,
    ) -> UserSyncStats {
        self.as_graph_sync(workspace_id, |c| {
            let mut stats = UserSyncStats {
                new_users_created: 0,
                existing_users_updated: 0,
                identities_linked: 0,
                errors: Vec::new(),
                failures: Vec::new(),
            };
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

    /// Provision a directory entry the way the LDAP sync does: on a
    /// connection pinned to `workspace_id`, with an identity scoped to it.
    fn ldap_provision(
        &self,
        workspace_id: i32,
        external_id: &str,
        email: &str,
        name: &str,
    ) -> Result<Uuid, String> {
        let mut out = None;
        run_in_workspace(&self.pool, REF, workspace_id, |c| {
            out = Some(
                find_or_create_projected_user(
                    c,
                    ProjectedUserInput {
                        iss: "ldap".to_string(),
                        sub: external_id.to_string(),
                        identity_workspace_id: Some(workspace_id),
                        email: email.to_string(),
                        email_verified: true,
                        real_name: Some(name.to_string()),
                        name: Some(name.to_string()),
                        username: None,
                        avatar_url: None,
                        verified_email_set: None,
                        role: "member".to_string(),
                        workspace_id,
                        password_hash: None,
                        metadata: None,
                    },
                )
                .map(|o| o.into_user().uuid),
            );
            Ok(())
        })
        .expect("provision");
        out.expect("ran")
    }
}

struct Account {
    uuid: Uuid,
    name: String,
    primary: String,
    secondary: String,
}

/// A directory person listing `addresses`, the first as `mail`.
fn directory_person(name: &str, addresses: &[&str]) -> MicrosoftGraphUser {
    let proxies: Vec<String> = addresses.iter().map(|a| format!("smtp:{a}")).collect();
    serde_json::from_value(json!({
        "id": Uuid::new_v4().to_string(),
        "displayName": name,
        "mail": addresses[0],
        "userPrincipalName": addresses[0],
        "proxyAddresses": proxies,
    }))
    .expect("graph user")
}

#[test]
fn graph_sync_in_b_does_not_link_an_account_seated_only_in_a() {
    let fx = Fixture::new();
    let victim = fx.account("Victim In A", &[fx.a()]);
    let before = fx.addresses(victim.uuid);

    let person = directory_person("Renamed By B", &[&victim.primary, &victim.secondary]);
    let stats = fx.graph_sync(fx.b(), &person, None);

    assert_eq!(stats.identities_linked + stats.new_users_created, 0);
    assert!(
        stats.errors.iter().any(|e| e.starts_with("Skipped")),
        "the skip is listed for B's admin: {:?}",
        stats.errors
    );
    assert_eq!(fx.name_of(victim.uuid), victim.name, "not renamed");
    assert_eq!(fx.seats(victim.uuid), vec![fx.a()], "not seated in B");
    assert!(fx.identities(victim.uuid).is_empty(), "no sign-in identity");
    assert_eq!(fx.addresses(victim.uuid), before, "addresses untouched");
}

#[test]
fn graph_sync_in_b_leaves_an_account_reached_by_identity_alone_when_only_a_holds_it() {
    let fx = Fixture::new();
    let victim = fx.account("Victim In A", &[fx.a()]);
    let person = directory_person("Renamed By B", &[&victim.primary]);
    let identity = fx.microsoft_identity(victim.uuid, &person);
    let before = fx.addresses(victim.uuid);

    let stats = fx.graph_sync(fx.b(), &person, Some(identity));

    assert_eq!(stats.existing_users_updated, 0);
    assert!(
        stats.errors.iter().any(|e| e.starts_with("Skipped")),
        "{:?}",
        stats.errors
    );
    assert_eq!(fx.name_of(victim.uuid), victim.name, "not renamed");
    assert_eq!(fx.seats(victim.uuid), vec![fx.a()], "not seated in B");
    assert_eq!(fx.addresses(victim.uuid), before, "addresses untouched");
}

#[test]
fn graph_sync_names_an_account_another_workspace_holds_only_locally() {
    let fx = Fixture::new();
    let shared = fx.account("Shared Person", &[fx.a(), fx.b()]);
    let person = directory_person("Name From B", &[&shared.primary]);
    let identity = fx.microsoft_identity(shared.uuid, &person);
    let before = fx.addresses(shared.uuid);

    let stats = fx.graph_sync(fx.b(), &person, Some(identity));

    assert_eq!(stats.existing_users_updated, 1, "{:?}", stats.errors);
    assert_eq!(
        fx.name_of(shared.uuid),
        shared.name,
        "the account keeps its name"
    );
    assert_eq!(
        fx.local_name(fx.b(), shared.uuid).as_deref(),
        Some("Name From B")
    );
    assert_eq!(
        fx.local_name(fx.a(), shared.uuid),
        None,
        "A's profile untouched"
    );
    assert_eq!(
        fx.addresses(shared.uuid),
        before,
        "the secondary address is neither verified nor removed"
    );
}

#[test]
fn graph_sync_still_renames_an_account_only_its_workspace_holds() {
    let fx = Fixture::new();
    let own = fx.account("Own Person", &[fx.b()]);
    let person = directory_person("Directory Name", &[&own.primary]);
    let identity = fx.microsoft_identity(own.uuid, &person);

    let stats = fx.graph_sync(fx.b(), &person, Some(identity));

    assert_eq!(stats.existing_users_updated, 1, "{:?}", stats.errors);
    assert_eq!(fx.name_of(own.uuid), "Directory Name");
}

#[test]
fn a_directory_group_in_b_takes_in_no_one_only_a_holds() {
    let fx = Fixture::new();
    let victim = fx.account("Victim In A", &[fx.a()]);
    let colleague = fx.account("Colleague In B", &[fx.b()]);
    let victim_person = directory_person("Victim", &[&victim.primary]);
    let colleague_person = directory_person("Colleague", &[&colleague.primary]);
    fx.microsoft_identity(victim.uuid, &victim_person);
    fx.microsoft_identity(colleague.uuid, &colleague_person);
    let member = |p: &MicrosoftGraphUser| -> MicrosoftGraphGroupMember {
        serde_json::from_value(json!({ "@odata.type": "#microsoft.graph.user", "id": p.id }))
            .expect("group member")
    };

    let members = fx.as_graph_sync(fx.b(), |c| {
        let (group, _) = backend::repository::groups::upsert_external_group(
            c,
            "directory-group",
            "microsoft",
            "Staff",
            None,
            None,
            false,
            true,
        )
        .expect("group");
        futures::executor::block_on(apply_delta_group_membership(
            c,
            group.id,
            &[member(&victim_person), member(&colleague_person)],
            &[],
        ))
        .expect("apply membership");
        backend::repository::groups::get_member_uuids_for_group(c, group.id).expect("members")
    });

    assert_eq!(members, vec![colleague.uuid], "only B's own member joins");
    assert_eq!(fx.seats(victim.uuid), vec![fx.a()]);
}

#[test]
fn ldap_provisioning_in_b_does_not_link_an_account_seated_only_in_a() {
    let fx = Fixture::new();
    let victim = fx.account("Victim In A", &[fx.a()]);

    let outcome = fx.ldap_provision(fx.b(), "dir-entry-1", &victim.primary, "Renamed By B");

    assert!(outcome.is_err(), "the entry isn't provisioned: {outcome:?}");
    assert_eq!(fx.name_of(victim.uuid), victim.name, "not renamed");
    assert_eq!(fx.seats(victim.uuid), vec![fx.a()], "not seated in B");
    assert!(fx.identities(victim.uuid).is_empty(), "no ldap identity");
}

#[test]
fn ldap_links_no_account_another_workspace_also_holds_and_names_one_already_linked_locally() {
    let fx = Fixture::new();
    let shared = fx.account("Shared Person", &[fx.a(), fx.b()]);

    // A new entry with the shared person's address isn't linked: the
    // identity would sign in as an account A holds too.
    let outcome = fx.ldap_provision(fx.b(), "dir-entry-2", &shared.primary, "Name From B");
    assert!(outcome.is_err(), "{outcome:?}");
    assert!(fx.identities(shared.uuid).is_empty(), "no ldap identity");

    // An entry linked before this rule re-projects into B's profile only.
    fx.ldap_identity(fx.b(), shared.uuid, "dir-entry-2");
    let user = fx
        .ldap_provision(fx.b(), "dir-entry-2", &shared.primary, "Name From B")
        .expect("re-projected");
    assert_eq!(user, shared.uuid);
    assert_eq!(
        fx.name_of(shared.uuid),
        shared.name,
        "the account keeps its name"
    );
    assert_eq!(
        fx.local_name(fx.b(), shared.uuid).as_deref(),
        Some("Name From B")
    );
}

#[test]
fn ldap_signs_in_only_an_account_its_workspace_holds_alone() {
    let fx = Fixture::new();
    let own = fx.account("Own Person", &[fx.b()]);
    let shared = fx.account("Shared Person", &[fx.a(), fx.b()]);
    // Removed from A, still in B: A held them, so B's directory can't vouch.
    let formerly_a = fx.account("Formerly In A", &[fx.a(), fx.b()]);
    fx.remove(fx.a(), formerly_a.uuid);

    let permitted = |user: Uuid| {
        let mut conn = fx.pool.get().expect("conn");
        let actor = ActorContext::system(REF).with_workspace(fx.b());
        with_actor_context::<_, diesel::result::Error>(&mut conn, &actor, |c| {
            Ok(backend::handlers::auth::ldap_sign_in_permitted(
                c,
                fx.b(),
                user,
            ))
        })
        .expect("decide")
    };
    assert!(permitted(own.uuid));
    assert!(!permitted(shared.uuid));
    assert!(!permitted(formerly_a.uuid));
}

#[test]
fn graph_sync_links_no_account_another_workspace_also_holds() {
    let fx = Fixture::new();
    let shared = fx.account("Shared Person", &[fx.a(), fx.b()]);
    let person = directory_person("Name From B", &[&shared.primary]);

    let stats = fx.graph_sync(fx.b(), &person, None);

    assert_eq!(stats.identities_linked, 0);
    assert!(
        stats.errors.iter().any(|e| e.starts_with("Skipped")),
        "{:?}",
        stats.errors
    );
    assert!(
        fx.identities(shared.uuid).is_empty(),
        "no microsoft identity"
    );
}

#[test]
fn graph_sync_links_only_by_a_verified_address() {
    let fx = Fixture::new();
    let own = fx.account("Own Person", &[fx.b()]);
    // Only the unverified secondary address matches.
    let person = directory_person("Own Person", &[&own.secondary.to_uppercase()]);

    let stats = fx.graph_sync(fx.b(), &person, None);

    assert_eq!(stats.identities_linked + stats.new_users_created, 0);
    assert!(fx.identities(own.uuid).is_empty());
}

#[test]
fn graph_sync_treats_a_seat_removed_elsewhere_as_another_workspaces_account() {
    let fx = Fixture::new();
    let formerly_a = fx.account("Formerly In A", &[fx.a(), fx.b()]);
    fx.remove(fx.a(), formerly_a.uuid);
    let person = directory_person("Name From B", &[&formerly_a.primary]);
    let identity = fx.microsoft_identity(formerly_a.uuid, &person);

    fx.graph_sync(fx.b(), &person, Some(identity));

    assert_eq!(fx.name_of(formerly_a.uuid), formerly_a.name, "not renamed");
    assert_eq!(
        fx.local_name(fx.b(), formerly_a.uuid).as_deref(),
        Some("Name From B")
    );
}

#[test]
fn graph_sync_doesnt_seat_an_account_no_workspace_seats() {
    let fx = Fixture::new();
    let orphan = fx.account("Orphan", &[]);
    let person = directory_person("Name From B", &[&orphan.primary]);
    let identity = fx.microsoft_identity(orphan.uuid, &person);

    let stats = fx.graph_sync(fx.b(), &person, Some(identity));

    assert!(fx.seats(orphan.uuid).is_empty(), "not seated");
    assert_eq!(fx.name_of(orphan.uuid), orphan.name, "not renamed");
    assert!(
        stats.errors.iter().any(|e| e.starts_with("Skipped")),
        "{:?}",
        stats.errors
    );
}

#[test]
fn the_photo_pass_takes_only_the_workspaces_own_people() {
    let fx = Fixture::new();
    let own = fx.account("Own Person", &[fx.b()]);
    let shared = fx.account("Shared Person", &[fx.a(), fx.b()]);
    let in_a = fx.account("In A", &[fx.a()]);
    for account in [&own, &shared, &in_a] {
        fx.microsoft_identity(account.uuid, &directory_person("P", &[&account.primary]));
    }

    // The photo pass's own pooled connection.
    let candidates =
        photo_sync_candidates(&mut fx.pool.get().expect("conn"), fx.b()).expect("candidates");

    let people: Vec<Uuid> = candidates.into_iter().map(|(u, _)| u).collect();
    assert_eq!(people, vec![own.uuid]);
}

#[test]
fn an_entra_removal_revokes_only_the_workspaces_own_people() {
    let fx = Fixture::new();
    let own = fx.account("Own Person", &[fx.b()]);
    let shared = fx.account("Shared Person", &[fx.a(), fx.b()]);
    let own_person = directory_person("Own", &[&own.primary]);
    let shared_person = directory_person("Shared", &[&shared.primary]);
    fx.microsoft_identity(own.uuid, &own_person);
    fx.microsoft_identity(shared.uuid, &shared_person);
    for user in [own.uuid, shared.uuid] {
        fx.session(user);
    }

    fx.as_graph_sync(fx.b(), |c| {
        revoke_removed_directory_people(
            c,
            0,
            fx.b(),
            &[own_person.id.clone(), shared_person.id.clone()],
        )
    });

    assert_eq!(
        fx.sessions(own.uuid),
        0,
        "the workspace's own person is signed out"
    );
    assert_eq!(
        fx.sessions(shared.uuid),
        1,
        "A's person keeps their session"
    );
}

#[test]
fn requester_sso_links_no_account_another_workspace_seats() {
    use backend::repository::requester_identities::{resolve, Assertion, IdentityKey, Refused};
    let fx = Fixture::new();
    let shared = fx.account("Shared Person", &[fx.a(), fx.b()]);
    let in_a = fx.account("In A", &[fx.a()]);
    // Unverified primaries, so a verification flip would show.
    fx.read(|c| {
        diesel::update(
            user_emails::table.filter(
                user_emails::email.eq_any(vec![shared.primary.clone(), in_a.primary.clone()]),
            ),
        )
        .set(user_emails::is_verified.eq(false))
        .execute(c)
    });
    let domains = vec!["victim.test".to_string()];

    for (account, key) in [(&shared, "sub-shared"), (&in_a, "sub-a")] {
        let keys = [IdentityKey {
            provider_type: "oidc:https://idp.example".to_string(),
            external_id: key.to_string(),
        }];
        let outcome = run_in_workspace(&fx.pool, REF, fx.b(), |c| {
            resolve(
                c,
                fx.b(),
                &domains,
                &Assertion {
                    keys: &keys,
                    email: Some(&account.primary),
                    email_verified: Some(true),
                    name: Some("Someone"),
                    via: "test",
                },
            )
        })
        .expect("resolve");
        assert_eq!(outcome, Err(Refused::AccountElsewhere), "{}", account.name);
        assert!(fx.identities(account.uuid).is_empty(), "{}", account.name);
        assert!(
            fx.addresses(account.uuid)
                .iter()
                .all(|(_, primary, verified, _)| !primary || !verified),
            "{}: the primary address stays unverified",
            account.name
        );
    }
    assert_eq!(fx.seats(in_a.uuid), vec![fx.a()], "not seated in B");
}

#[test]
fn ldap_still_renames_an_account_only_its_workspace_holds() {
    let fx = Fixture::new();
    let own = fx.account("Own Person", &[fx.b()]);
    for name in ["Directory Name", "Directory Name"] {
        fx.ldap_provision(fx.b(), "dir-entry-3", &own.primary, name)
            .expect("provisioned");
    }
    assert_eq!(fx.name_of(own.uuid), "Directory Name");
}

#[test]
fn adding_addresses_leaves_another_accounts_row_alone() {
    let fx = Fixture::new();
    let other = fx.account("Other", &[fx.a()]);
    let mine = fx.account("Mine", &[fx.b()]);
    let before = fx.addresses(other.uuid);

    let stored = fx.read(|c| {
        emails_repo::add_multiple_emails(
            c,
            &mine.uuid,
            vec![(
                other.secondary.clone(),
                "work".to_string(),
                true,
                "microsoft".to_string(),
            )],
        )
    });

    assert!(
        stored.is_empty(),
        "nothing returned for a row it doesn't own"
    );
    assert_eq!(
        fx.addresses(other.uuid),
        before,
        "the other account's address stays unverified and theirs"
    );
}

#[test]
fn adding_addresses_matches_them_case_insensitively() {
    let fx = Fixture::new();
    let other = fx.account("Other", &[fx.a()]);
    let mine = fx.account("Mine", &[fx.b()]);
    let other_before = fx.addresses(other.uuid);
    let mine_before = fx.addresses(mine.uuid);

    let stored = fx.read(|c| {
        emails_repo::add_multiple_emails(
            c,
            &mine.uuid,
            vec![
                (
                    other.primary.to_uppercase(),
                    "work".to_string(),
                    true,
                    "microsoft".to_string(),
                ),
                (
                    mine.secondary.to_uppercase(),
                    "work".to_string(),
                    true,
                    "microsoft".to_string(),
                ),
            ],
        )
    });

    assert_eq!(stored.len(), 1, "only their own address is refreshed");
    assert_eq!(
        fx.addresses(other.uuid),
        other_before,
        "no case variant added"
    );
    let mine_after = fx.addresses(mine.uuid);
    assert_eq!(
        mine_after.len(),
        mine_before.len(),
        "no duplicate in another case"
    );
    assert!(mine_after
        .iter()
        .any(|(email, _, verified, _)| *email == mine.secondary && *verified));

    // Cleanup compares case-insensitively too.
    let synced = format!("Synced-{}@Victim.test", Uuid::new_v4().simple());
    fx.read(|c| {
        emails_repo::add_multiple_emails(
            c,
            &mine.uuid,
            vec![(
                synced.clone(),
                "work".to_string(),
                true,
                "microsoft".to_string(),
            )],
        )
    });
    let removed = fx.read(|c| {
        emails_repo::cleanup_obsolete_emails(c, &mine.uuid, &[synced.to_uppercase()], "microsoft")
    });
    assert_eq!(removed, 0, "a listed address in another case stays");
}

#[test]
fn cleanup_removes_only_the_addresses_its_source_added() {
    let fx = Fixture::new();
    let person = fx.account("Person", &[fx.b()]);
    let synced = format!("synced-{}@victim.test", Uuid::new_v4().simple());
    fx.read(|c| {
        diesel::insert_into(user_emails::table)
            .values(&NewUserEmail {
                user_uuid: person.uuid,
                email: synced.clone(),
                email_type: "work".to_string(),
                is_primary: false,
                is_verified: true,
                source: Some("microsoft".to_string()),
            })
            .execute(c)
    });

    let removed =
        fx.read(|c| emails_repo::cleanup_obsolete_emails(c, &person.uuid, &[], "microsoft"));

    assert_eq!(removed, 1);
    let left: Vec<String> = fx
        .addresses(person.uuid)
        .into_iter()
        .map(|(email, ..)| email)
        .collect();
    let mut expected = vec![person.primary.clone(), person.secondary.clone()];
    expected.sort();
    assert_eq!(left, expected, "the hand-added address stays");
}

#[actix_web::test]
async fn add_participant_says_nothing_about_the_account_an_address_reached() {
    common::ensure_test_keyring();
    let fx = Fixture::new();
    let a = fx.ws.a.clone();
    // An account only B knows; the requester in A knows its secondary address.
    let stranger = fx.account("Stranger In B", &[fx.b()]);
    fx.read(|c| {
        diesel::update(user_emails::table.filter(user_emails::email.eq(&stranger.secondary)))
            .set(user_emails::is_verified.eq(true))
            .execute(c)
    });
    let requester = a.member_uuid;
    let ticket = run_in_workspace(&fx.pool, REF, a.workspace_id, |c| {
        let state = backend::repository::workflow_states::default_state(c)?;
        diesel::insert_into(backend::schema::tickets::table)
            .values(&NewTicket {
                title: "Printer".to_string(),
                workflow_state_id: state.id,
                requester_uuid: Some(requester),
                ..Default::default()
            })
            .get_result::<backend::models::Ticket>(c)
    })
    .expect("ticket");

    let workspace = WorkspaceContext {
        workspace_id: a.workspace_id,
        workspace_uuid: a.workspace_uuid,
        slug: a.slug.clone(),
        name: "A".to_string(),
        organisation_id: None,
        custom_domain: None,
    };
    let portal = PortalContext {
        user_uuid: requester,
        workspace_id: a.workspace_id,
        workspace_uuid: a.workspace_uuid,
        scope: backend::handlers::portal::PortalScope::Full,
    };
    let corr = Uuid::now_v7();
    let actor = ActorContext::user(requester, Some(corr)).with_workspace(a.workspace_id);
    let app = http_test::init_service(
        App::new()
            .app_data(web::Data::new(fx.pool.clone()))
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
            )
            .route(
                "/api/portal/tickets/{id}/participants",
                web::post().to(backend::handlers::portal::add_participant),
            ),
    )
    .await;

    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::post()
            .uri(&format!("/api/portal/tickets/{}/participants", ticket.id))
            .set_json(json!({ "email": stranger.secondary }))
            .to_request(),
    )
    .await;
    assert!(resp.status().is_success(), "add: {}", resp.status());
    let added: Value = http_test::read_body_json(resp).await;
    assert_eq!(added, json!({ "email": stranger.secondary, "added": true }));

    let resp = http_test::call_service(
        &app,
        http_test::TestRequest::get()
            .uri(&format!("/api/portal/tickets/{}", ticket.id))
            .to_request(),
    )
    .await;
    assert!(resp.status().is_success(), "detail: {}", resp.status());
    let detail: Value = http_test::read_body_json(resp).await;
    let text = detail.to_string();
    assert!(
        !text.contains(&stranger.primary),
        "the account's primary address isn't shown: {text}"
    );
    assert!(!text.contains(&stranger.name), "nor its name: {text}");
    let added_person = detail["participants"]
        .as_array()
        .expect("participants")
        .iter()
        .find(|p| p["is_requester"] == false)
        .expect("the added person is on the request")
        .clone();
    assert_eq!(added_person["email"], Value::Null);
}

#[test]
fn user_webhooks_and_other_members_sync_rows_carry_no_platform_role_or_dashboard_layout() {
    let fx = Fixture::new();
    let a = fx.a();
    let person = fx.account("Webhook Person", &[a]);
    let hook = run_in_workspace(&fx.pool, REF, a, |c| {
        backend::repository::webhooks::create_webhook(
            c,
            "Users".to_string(),
            "https://example.invalid/users".to_string(),
            "whsec_test".to_string(),
            vec!["user.updated".to_string()],
            None,
            None,
        )
    })
    .expect("webhook")
    .id;

    run_in_workspace(&fx.pool, REF, a, |c| {
        backend::repository::users::update_user(
            &person.uuid,
            backend::models::UserUpdate {
                name: Some("Renamed Person".to_string()),
                pronouns: None,
                avatar_url: None,
                banner_url: None,
                avatar_thumb: None,
                microsoft_uuid: None,
                updated_at: None,
            },
            c,
            None,
        )
    })
    .expect("rename");

    // The event log keeps the person's whole row, for their own sessions.
    let recorded: Value = fx.read(|c| {
        sync_actions::table
            .filter(sync_actions::aggregate_id.eq(person.uuid.to_string()))
            .filter(sync_actions::event_type.eq("user.updated"))
            .filter(sync_actions::workspace_id.eq(a))
            .order(sync_actions::sync_id.desc())
            .select(sync_actions::data)
            .first(c)
    });
    assert!(recorded.get("platform_role").is_some(), "{recorded}");
    assert!(recorded.get("dashboard_layout").is_some(), "{recorded}");

    // Another member's sessions get the row without them (the sync read
    // paths project it), and the person's own keep it.
    let as_viewer = |viewer: Uuid| -> Value {
        let mut conn = fx.pool.get().expect("conn");
        with_actor_context::<_, diesel::result::Error>(
            &mut conn,
            &ActorContext::user(viewer, None).with_workspace(a),
            |c| {
                let me = backend::repository::users::get_user_by_uuid(&viewer, c)?;
                let viewer = SyncViewer::resolve(c, &me);
                let keys = asset_keys_for(c, &viewer, std::iter::once(Some(SyncAggregate::User)));
                let mut data = recorded.clone();
                project_row(&viewer, Some(SyncAggregate::User), &mut data, &keys);
                Ok(data)
            },
        )
        .expect("project")
    };
    for other in [fx.ws.a.admin_uuid, fx.ws.a.member_uuid] {
        let seen = as_viewer(other);
        assert!(seen.get("platform_role").is_none(), "{seen}");
        assert!(seen.get("dashboard_layout").is_none(), "{seen}");
    }
    assert!(as_viewer(person.uuid).get("dashboard_layout").is_some());

    loop {
        let mut conn = fx.pool.get().expect("conn");
        let (_, more) = with_actor_bypass_context(
            &mut conn,
            &ActorContext::system(REF),
            WebhookService::drain_batch_txn,
        )
        .expect("drain the outbox");
        if !more {
            break;
        }
    }
    let payloads: Vec<Value> = run_in_workspace(&fx.pool, REF, a, |c| {
        webhook_deliveries::table
            .filter(webhook_deliveries::webhook_id.eq(hook))
            .select(webhook_deliveries::payload)
            .load(c)
    })
    .expect("deliveries");
    let data = payloads
        .iter()
        .map(|p| &p["data"])
        .find(|d| d["uuid"] == json!(person.uuid) && d["name"] == "Renamed Person")
        .expect("the rename reached the webhook");
    let keys: Vec<&str> = {
        let mut k: Vec<&str> = data
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        k.sort_unstable();
        k
    };
    assert_eq!(
        keys,
        [
            "avatar_thumb",
            "avatar_url",
            "deleted_at",
            "email",
            "name",
            "pronouns",
            "uuid",
            "workspace_role"
        ],
        "every field but the platform role and dashboard layout"
    );
    assert_eq!(data["email"], json!(person.primary));
}
