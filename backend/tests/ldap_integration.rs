//! LDAP integration harness (P3). **Ignored by default** — it needs a real
//! directory (a Samba AD-DC or OpenLDAP container) reachable from the test host.
//! It exercises the full roundtrip the unit tests can't: connect → service-bind
//! → search → user-bind, and a sync that provisions users into the DB.
//!
//! ## Running it
//!
//! 1. Start a directory. A quick Samba AD-DC (provisions a domain + an admin):
//!
//!    ```sh
//!    docker run -d --name nosdesk-ad -p 389:389 -p 636:636 \
//!      -e "SAMBA_DOMAIN=ACME" -e "SAMBA_REALM=acme.test" \
//!      -e "SAMBA_ADMIN_PASSWORD=Passw0rd!" \
//!      nowsci/samba-domain
//!    # then create a test user:
//!    docker exec nosdesk-ad samba-tool user create alice 'Alice#2026' \
//!      --given-name=Alice --surname=Smith --mail-address=alice@acme.test
//!    ```
//!
//! 2. Point the test at it + run with `--ignored`. The defaults below match the
//!    Samba example; override any via env for a different directory:
//!
//!    ```sh
//!    cd backend && \
//!      LDAP_HOST=127.0.0.1 LDAP_PORT=636 \
//!      LDAP_BASE_DN='DC=acme,DC=test' \
//!      LDAP_BIND_DN='CN=Administrator,CN=Users,DC=acme,DC=test' \
//!      LDAP_BIND_PASSWORD='Passw0rd!' \
//!      LDAP_TEST_USER=alice LDAP_TEST_USER_PASSWORD='Alice#2026' \
//!      cargo test --test ldap_integration -- --ignored
//!    ```
//!
//!    Or against the seeded OpenLDAP rig (`make ldap-test`), which also exercises
//!    the non-AD full-scan fallback + group sync + role mapping:
//!
//!    ```sh
//!    cd backend && \
//!      LDAP_HOST=127.0.0.1 LDAP_PORT=636 \
//!      LDAP_BASE_DN='ou=People,dc=acme,dc=test' \
//!      LDAP_BIND_DN='cn=admin,dc=acme,dc=test' LDAP_BIND_PASSWORD='admin' \
//!      LDAP_TEST_USER=alice LDAP_TEST_USER_PASSWORD='Alice#2026' \
//!      LDAP_USERNAME_ATTR=uid LDAP_EXTERNAL_ID_ATTR=entryUUID \
//!      LDAP_USER_FILTER='(&(objectClass=inetOrgPerson)(uid={username}))' \
//!      LDAP_GROUP_BASE_DN='ou=Groups,dc=acme,dc=test' \
//!      LDAP_GROUP_OBJECT_CLASS=groupOfNames \
//!      cargo test --test ldap_integration -- --ignored --test-threads=1
//!    ```
//!
//! The DB-backed tests share workspace 1, so run them with `--test-threads=1`.
//! The connector's egress guard rejects the loopback/RFC1918 directory host by
//! default, exactly as it would a real on-prem DC; the fixture opts the host
//! into `NOSDESK_OUTBOUND_ALLOWED_HOSTS` just as a self-hoster would. The server
//! self-signs LDAPS, so the settings set `verify_certs=false`, which the
//! connector honours only because the run is non-production.

use std::sync::OnceLock;

use diesel::prelude::*;
use diesel::r2d2;
use diesel_migrations::MigrationHarness;
use serde_json::json;

use backend::db::{DbConnection, Pool, ResettingManager, MIGRATIONS};
use backend::models::{Group, WorkspaceLdapSettings};
use backend::repository::{groups as groups_repo, workspaces as ws_repo};
use backend::services::ldap::{auth as ldap_auth, sync as ldap_sync};

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn ldap_host() -> String {
    env_or("LDAP_HOST", "127.0.0.1")
}
fn ldap_port() -> u16 {
    env_or("LDAP_PORT", "636").parse().unwrap_or(636)
}
fn bind_password() -> String {
    env_or("LDAP_BIND_PASSWORD", "Passw0rd!")
}
fn test_user() -> String {
    env_or("LDAP_TEST_USER", "alice")
}
fn test_user_password() -> String {
    env_or("LDAP_TEST_USER_PASSWORD", "Alice#2026")
}

/// Opt the directory host into the outbound allowlist (it's loopback/RFC1918,
/// which egress rejects by default). Process-global, but every test uses the
/// same host, so setting it from each is safe.
fn allow_ldap_egress() {
    std::env::set_var("NOSDESK_OUTBOUND_ALLOWED_HOSTS", ldap_host());
}

/// Skip the file if the directory isn't reachable, so `cargo test` doesn't
/// red-light a machine that isn't running one.
fn ldap_reachable() -> bool {
    std::net::TcpStream::connect((ldap_host().as_str(), ldap_port())).is_ok()
}

/// Settings pointed at the test directory. AD defaults; the attribute_map +
/// filter match the `active_directory` preset.
fn test_settings() -> WorkspaceLdapSettings {
    let now = chrono::Utc::now();
    WorkspaceLdapSettings {
        workspace_id: 1,
        enabled: true,
        host: ldap_host(),
        port: ldap_port() as i32,
        tls_mode: env_or("LDAP_TLS_MODE", "ldaps"),
        // Samba self-signs LDAPS; honoured because the test run is non-production.
        verify_certs: false,
        ca_cert_pem: None,
        follow_referrals: false,
        connect_timeout_secs: 5,
        auth_mode: "simple_bind".into(),
        bind_dn: env_or("LDAP_BIND_DN", "CN=Administrator,CN=Users,DC=acme,DC=test"),
        encrypted_bind_password: None,
        encrypted_kek_id: None,
        user_base_dn: env_or("LDAP_BASE_DN", "DC=acme,DC=test"),
        username_attribute: env_or("LDAP_USERNAME_ATTR", "sAMAccountName"),
        user_filter: env_or(
            "LDAP_USER_FILTER",
            "(&(objectCategory=person)(objectClass=user)(sAMAccountName={username}))",
        ),
        page_size: 500,
        attribute_map: json!({
            "email": env_or("LDAP_EMAIL_ATTR", "mail"),
            "display_name": env_or("LDAP_DISPLAY_NAME_ATTR", "displayName"),
            // OpenLDAP keys identities on entryUUID, AD on objectGUID.
            "external_id": env_or("LDAP_EXTERNAL_ID_ATTR", "objectGUID")
        }),
        group_config: json!({}),
        provisioning: json!({}),
        created_at: now,
        updated_at: now,
    }
}

fn ldap_group_base_dn() -> String {
    env_or("LDAP_GROUP_BASE_DN", "")
}

/// `test_settings` plus group sync + a role mapping, for the groups-and-roles
/// test. The group object class is env-configurable so the AD-default harness can
/// also point at OpenLDAP (`groupOfNames`). The mapping matches the seed:
/// Helpdesk-Admins -> admin, Agents -> agent.
fn test_settings_with_groups() -> WorkspaceLdapSettings {
    let mut s = test_settings();
    s.group_config = json!({
        "group_base_dn": ldap_group_base_dn(),
        "object_class": env_or("LDAP_GROUP_OBJECT_CLASS", "group"),
        "member_attribute": "member",
        "name_attribute": "cn",
        "role_mappings": [
            { "group": "Helpdesk-Admins", "role": "admin" },
            { "group": "Agents", "role": "agent" },
        ],
    });
    s
}

fn build_pool() -> Pool {
    dotenvy::dotenv().ok();
    let url = std::env::var("TEST_DATABASE_URL")
        .expect("TEST_DATABASE_URL must be set for integration tests");
    ensure_migrated(&url);
    let manager = ResettingManager::new(url);
    r2d2::Pool::builder()
        .max_size(2)
        .connection_customizer(Box::new(WorkspaceGucCustomizer))
        // Keep the production per-checkout GUC scrub off this pool so the
        // ambient app.workspace_id=1 the customizer seeds survives.
        .test_on_check_out(false)
        .build(manager)
        .expect("build test pool")
}

/// Pin app.workspace_id=1 on every connection so the sync's tenant-table writes
/// (user_profiles, etc.) satisfy the workspace_id default + RLS.
#[derive(Debug)]
struct WorkspaceGucCustomizer;
impl r2d2::CustomizeConnection<diesel::PgConnection, r2d2::Error> for WorkspaceGucCustomizer {
    fn on_acquire(&self, conn: &mut diesel::PgConnection) -> Result<(), r2d2::Error> {
        diesel::sql_query("SELECT set_config('app.workspace_id', '1', false)")
            .execute(conn)
            .map_err(r2d2::Error::QueryError)?;
        Ok(())
    }
}

fn ensure_migrated(url: &str) {
    static INIT: OnceLock<()> = OnceLock::new();
    INIT.get_or_init(|| {
        let mut conn = diesel::PgConnection::establish(url)
            .expect("connect to TEST_DATABASE_URL for migration bootstrap");
        conn.run_pending_migrations(MIGRATIONS)
            .expect("apply migrations to test DB");
    });
}

#[tokio::test]
#[ignore = "requires a directory (Samba AD-DC) — see file header"]
async fn test_connection_service_binds() {
    if !ldap_reachable() {
        eprintln!("skipping: no directory reachable at {}", ldap_host());
        return;
    }
    allow_ldap_egress();
    ldap_auth::test_connection(&test_settings(), &bind_password())
        .await
        .expect("connect + service-bind should succeed");
}

#[tokio::test]
#[ignore = "requires a directory (Samba AD-DC) — see file header"]
async fn authenticate_a_known_user() {
    if !ldap_reachable() {
        eprintln!("skipping: no directory reachable at {}", ldap_host());
        return;
    }
    allow_ldap_egress();
    let result = ldap_auth::authenticate(
        &test_settings(),
        &bind_password(),
        &test_user(),
        &test_user_password(),
    )
    .await
    .expect("a valid user/password should authenticate");
    assert!(
        !result.external_id.is_empty(),
        "the resolved identity must carry an external_id (objectGUID)"
    );

    // A wrong password must be rejected, not authenticated.
    let bad =
        ldap_auth::authenticate(&test_settings(), &bind_password(), &test_user(), "wrong-pw").await;
    assert!(matches!(
        bad,
        Err(ldap_auth::LdapAuthError::InvalidCredentials)
    ));
}

#[tokio::test]
#[ignore = "requires a directory (Samba AD-DC) + TEST_DATABASE_URL — see file header"]
async fn sync_provisions_a_directory_user() {
    if !ldap_reachable() {
        eprintln!("skipping: no directory reachable at {}", ldap_host());
        return;
    }
    allow_ldap_egress();
    let pool = build_pool();
    let mut conn: DbConnection = pool.get().expect("pooled conn");

    let stats = ldap_sync::sync_users(&mut conn, &test_settings(), 1, &bind_password())
        .await
        .expect("sync should complete");
    assert!(
        stats.synced >= 1,
        "expected at least one provisioned user, got {stats:?}"
    );

    // The test user now has a workspace-scoped ldap identity row.
    use backend::schema::user_auth_identities::dsl as i;
    let ldap_identities: i64 = i::user_auth_identities
        .filter(i::provider_type.eq("ldap"))
        .filter(i::workspace_id.eq(1))
        .count()
        .get_result(&mut conn)
        .expect("count ldap identities");
    assert!(
        ldap_identities >= 1,
        "sync must create scoped ldap identities"
    );
}

/// Make workspace 1 unlimited-seat so the role mapping's staff promotions aren't
/// capped by the seat-limit trigger -- a deterministic precondition, not luck.
fn ensure_unlimited_seats(conn: &mut DbConnection) {
    diesel::sql_query("UPDATE workspaces SET seat_limit = NULL WHERE id = 1")
        .execute(conn)
        .expect("clear workspace 1 seat limit");
}

fn find_ldap_group(conn: &mut DbConnection, name: &str) -> Group {
    use backend::schema::groups::dsl as g;
    g::groups
        .filter(g::name.eq(name))
        .filter(g::external_source.eq("ldap"))
        .filter(g::workspace_id.eq(1))
        .first::<Group>(conn)
        .unwrap_or_else(|e| panic!("ldap group {name:?} should exist after sync: {e}"))
}

/// The full directory pipeline against a live server: `run_recorded_sync` is the
/// SAME entry point the admin trigger + the nightly reconcile use, so this
/// exercises user sync -> group sync (DN-resolved membership) -> group->role
/// mapping in one go, and asserts the result through the repository layer the app
/// reads from. Needs a directory with the seeded groups + `TEST_DATABASE_URL`.
#[tokio::test]
#[ignore = "requires a directory with groups + TEST_DATABASE_URL — see file header"]
async fn sync_provisions_groups_and_roles() {
    if !ldap_reachable() {
        eprintln!("skipping: no directory reachable at {}", ldap_host());
        return;
    }
    if ldap_group_base_dn().is_empty() {
        eprintln!("skipping: LDAP_GROUP_BASE_DN not set (no group base to sync)");
        return;
    }
    allow_ldap_egress();
    let pool = build_pool();
    let mut conn: DbConnection = pool.get().expect("pooled conn");
    ensure_unlimited_seats(&mut conn);

    let rec = ldap_sync::run_recorded_sync(
        &mut conn,
        &test_settings_with_groups(),
        1,
        &bind_password(),
        "ldap_users",
    )
    .await
    .expect("recorded sync should complete");
    assert!(
        rec.stats.synced >= 3,
        "expected the 3 seeded users provisioned, got {:?}",
        rec.stats
    );

    // Both directory groups landed as ldap-sourced groups in the workspace.
    let admins = find_ldap_group(&mut conn, "Helpdesk-Admins");
    let agents = find_ldap_group(&mut conn, "Agents");

    // Membership was resolved from the directory's member DNs via the persisted
    // DN map (the subtle path): alice in Helpdesk-Admins; bob + carol in Agents.
    let admin_members =
        groups_repo::get_member_uuids_for_group(&mut conn, admins.id).expect("admins members");
    let agent_members =
        groups_repo::get_member_uuids_for_group(&mut conn, agents.id).expect("agents members");
    assert_eq!(
        admin_members.len(),
        1,
        "Helpdesk-Admins should resolve to its 1 directory member, got {admin_members:?}"
    );
    assert_eq!(
        agent_members.len(),
        2,
        "Agents should resolve to its 2 directory members, got {agent_members:?}"
    );

    // The group->role mapping applied: the admin-group member is an admin, and
    // each agent-group member is an agent.
    assert_eq!(
        ws_repo::get_membership_role(&mut conn, 1, admin_members[0])
            .expect("admin role")
            .as_deref(),
        Some("admin"),
        "the Helpdesk-Admins member should be promoted to admin"
    );
    for member in agent_members {
        assert_eq!(
            ws_repo::get_membership_role(&mut conn, 1, member)
                .expect("agent role")
                .as_deref(),
            Some("agent"),
            "each Agents member should be promoted to agent"
        );
    }
}
