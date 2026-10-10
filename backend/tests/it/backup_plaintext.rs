//! A backup made without a password leaves credentials out
//! (`SENSITIVE_FIELDS`) and still restores: tokens and recovery codes stay
//! behind, webhooks come back off with a new secret, and a user whose MFA
//! secret was left out comes back without MFA.

use diesel::prelude::*;
use diesel::sql_types::{BigInt, Bool, Text, Uuid as SqlUuid};
use uuid::Uuid;

use backend::services::backup as backup_service;

use crate::common::{seed_backup_job, seed_two_workspaces, with_upload_dir, TestDb};

#[derive(QueryableByName)]
struct Count {
    #[diesel(sql_type = BigInt)]
    n: i64,
}

#[derive(QueryableByName)]
struct Webhook {
    #[diesel(sql_type = Text)]
    secret: String,
    #[diesel(sql_type = Bool)]
    enabled: bool,
}

#[derive(QueryableByName)]
struct Mfa {
    #[diesel(sql_type = Bool)]
    mfa_enabled: bool,
    #[diesel(sql_type = diesel::sql_types::Nullable<diesel::sql_types::SmallInt>)]
    mfa_secret_kek_id: Option<i16>,
}

fn count(conn: &mut PgConnection, table: &str) -> i64 {
    diesel::sql_query(format!("SELECT count(*) AS n FROM {table}"))
        .get_result::<Count>(conn)
        .expect("count")
        .n
}

fn webhook(conn: &mut PgConnection, id: i32) -> Webhook {
    diesel::sql_query("SELECT secret, enabled FROM webhooks WHERE id = $1")
        .bind::<diesel::sql_types::Integer, _>(id)
        .get_result(conn)
        .expect("webhook")
}

#[test]
fn a_backup_without_a_password_restores_without_its_credentials() {
    let db = TestDb::new();
    let mut conn = db.conn();
    with_upload_dir();
    let seeded = seed_two_workspaces(&mut conn);
    let admin = seeded.a.admin_uuid;

    // The admin is signed in, has an API token and recovery codes, and MFA.
    for statement in [
        "INSERT INTO refresh_tokens (token_hash, user_uuid, expires_at) \
         VALUES ('refresh-hash', $1, now() + interval '1 day')",
        "INSERT INTO user_recovery_codes (user_uuid, code_hash) VALUES ($1, 'code-hash')",
        "UPDATE users SET mfa_enabled = true, mfa_secret = '\\x01'::bytea, mfa_secret_kek_id = 1 \
         WHERE uuid = $1",
    ] {
        diesel::sql_query(statement)
            .bind::<SqlUuid, _>(admin)
            .execute(&mut conn)
            .expect(statement);
    }
    diesel::sql_query(
        "INSERT INTO api_tokens (uuid, token_hash, token_prefix, user_uuid, name, created_by, workspace_id) \
         VALUES ($1, 'api-hash', 'nsk_', $2, 'CI', $2, $3)",
    )
    .bind::<SqlUuid, _>(Uuid::new_v4())
    .bind::<SqlUuid, _>(admin)
    .bind::<diesel::sql_types::Integer, _>(seeded.a.workspace_id)
    .execute(&mut conn)
    .expect("api token");
    let before = webhook(&mut conn, seeded.a.webhook_id);
    assert!(before.enabled);

    let job = seed_backup_job(&mut conn);
    let archive = backup_service::create_backup(&mut conn, job, None).expect("create_backup");
    backup_service::restore_database(
        &mut conn,
        &archive,
        None,
        backup_service::RestoreOptions {
            force_non_empty: true,
            ..Default::default()
        },
    )
    .expect("restore_database");

    for table in ["refresh_tokens", "api_tokens", "user_recovery_codes"] {
        assert_eq!(count(&mut conn, table), 0, "{table}");
    }
    let after = webhook(&mut conn, seeded.a.webhook_id);
    assert!(!after.enabled, "off until the receiver has the new secret");
    assert!(after.secret.starts_with("whsec_") && after.secret.len() == 70);
    assert_ne!(after.secret, before.secret);
    let mfa: Mfa =
        diesel::sql_query("SELECT mfa_enabled, mfa_secret_kek_id FROM users WHERE uuid = $1")
            .bind::<SqlUuid, _>(admin)
            .get_result(&mut conn)
            .expect("admin");
    assert!(!mfa.mfa_enabled);
    assert_eq!(mfa.mfa_secret_kek_id, None);
}
