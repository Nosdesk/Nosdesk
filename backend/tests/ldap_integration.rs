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
//! The connector's egress guard rejects the loopback/RFC1918 directory host by
//! default, exactly as it would a real on-prem DC; the fixture opts the host
//! into `NOSDESK_OUTBOUND_ALLOWED_HOSTS` just as a self-hoster would. Samba
//! self-signs LDAPS, so the settings set `verify_certs=false`, which the
//! connector honours only because the run is non-production.

use std::sync::OnceLock;

use diesel::prelude::*;
use diesel::r2d2;
use diesel_migrations::MigrationHarness;
use serde_json::json;

use backend::db::{DbConnection, Pool, ResettingManager, MIGRATIONS};
use backend::models::WorkspaceLdapSettings;
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
        username_attribute: "sAMAccountName".into(),
        user_filter: env_or(
            "LDAP_USER_FILTER",
            "(&(objectCategory=person)(objectClass=user)(sAMAccountName={username}))",
        ),
        page_size: 500,
        attribute_map: json!({
            "email": "mail",
            "display_name": "displayName",
            "external_id": "objectGUID"
        }),
        group_config: json!({}),
        provisioning: json!({}),
        created_at: now,
        updated_at: now,
    }
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
