//! LDAP group-to-role mapping reads only the groups of the workspace it maps.
//! The nightly reconcile runs it on an elevated connection, where row-level
//! security doesn't scope reads, so the query itself must.

use diesel::prelude::*;
use serde_json::json;

use backend::models::WorkspaceLdapSettings;
use backend::sync::actor::ActorContext;
use backend::sync::session::{elevate_session_role, reset_session_role, run_in_workspace};

use crate::common;

const REF: &str = "test:ldap_groups_in_workspace";

fn settings(workspace_id: i32) -> WorkspaceLdapSettings {
    let now = chrono::Utc::now();
    WorkspaceLdapSettings {
        workspace_id,
        enabled: true,
        host: "ldap.invalid".into(),
        port: 636,
        tls_mode: "ldaps".into(),
        verify_certs: true,
        ca_cert_pem: None,
        follow_referrals: false,
        connect_timeout_secs: 5,
        auth_mode: "simple_bind".into(),
        bind_dn: "cn=admin".into(),
        encrypted_bind_password: None,
        encrypted_kek_id: None,
        user_base_dn: "dc=acme".into(),
        username_attribute: "uid".into(),
        user_filter: "(uid={username})".into(),
        page_size: 500,
        attribute_map: json!({}),
        group_config: json!({
            "role_mappings": [{ "group": "Admins", "role": "admin" }],
            "default_role": "member",
        }),
        provisioning: json!({}),
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn another_workspaces_group_doesnt_raise_a_role() {
    let db = common::TestDb::new();
    let seeded = common::seed_two_workspaces(&mut db.pool_with_size(2).get().expect("conn"));
    let pool = db.runtime_pool(2);
    let (a, b) = (seeded.a.workspace_id, seeded.b.workspace_id);
    let user = seeded.a.member_uuid;
    {
        let mut c = db.conn();
        diesel::sql_query("UPDATE workspaces SET seat_limit = NULL WHERE id IN ($1, $2)")
            .bind::<diesel::sql_types::Integer, _>(a)
            .bind::<diesel::sql_types::Integer, _>(b)
            .execute(&mut c)
            .expect("unlimited seats");
    }

    // A directory user of workspace A...
    run_in_workspace(&pool, REF, a, |c| {
        diesel::sql_query(
            "INSERT INTO user_auth_identities (user_uuid, provider_type, external_id, workspace_id) \
             VALUES ($1, 'ldap', 'dir-user', $2)",
        )
        .bind::<diesel::sql_types::Uuid, _>(user)
        .bind::<diesel::sql_types::Integer, _>(a)
        .execute(c)
    })
    .expect("ldap identity in A");
    // ...in an LDAP group named "Admins" that exists only in workspace B.
    run_in_workspace(&pool, REF, b, |c| {
        diesel::sql_query(
            "WITH g AS (INSERT INTO groups (name, external_source, sync_enabled, workspace_id) \
                        VALUES ('Admins', 'ldap', true, $2) RETURNING id) \
             INSERT INTO user_groups (user_uuid, group_id, workspace_id) SELECT $1, g.id, $2 FROM g",
        )
        .bind::<diesel::sql_types::Uuid, _>(user)
        .bind::<diesel::sql_types::Integer, _>(b)
        .execute(c)
    })
    .expect("Admins group in B");

    // Workspace A maps "Admins" to admin; reconcile A as the nightly job does.
    let mut conn = pool.get().expect("conn");
    elevate_session_role(&mut conn, &ActorContext::system(REF).with_workspace(a)).expect("elevate");
    let stats =
        backend::services::ldap::role_mapping::apply_role_mappings(&mut conn, &settings(a), a)
            .expect("role mapping");
    let role =
        backend::repository::workspaces::get_membership_role(&mut conn, a, user).expect("role");
    reset_session_role(&mut conn);

    assert_eq!(stats.evaluated, 1);
    assert_eq!(role.as_deref(), Some("member"));
}
