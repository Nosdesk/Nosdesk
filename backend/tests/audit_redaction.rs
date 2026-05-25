//! W4 guard: the audit_log_trigger() column-exclusion (D2 PII + D3
//! credentials) must keep redacted values out of the audit diff while
//! still recording *that* the column changed.
//!
//! Exercised against `users`, whose trigger is attached with
//! `audit_log_trigger('uuid', 'name', 'mfa_secret', 'mfa_backup_codes')`
//! by migration 2026-05-25-140000. If someone re-attaches that trigger
//! without the exclusion list, or regresses the function, these
//! assertions fail.

mod common;

use common::{insert_user, TestDb};
use diesel::prelude::*;
use diesel::sql_types::{Array, Nullable, Text};

use backend::schema::users;

#[derive(QueryableByName)]
struct AuditRow {
    #[diesel(sql_type = Nullable<diesel::sql_types::Jsonb>)]
    before_jsonb: Option<serde_json::Value>,
    #[diesel(sql_type = Nullable<diesel::sql_types::Jsonb>)]
    after_jsonb: Option<serde_json::Value>,
    #[diesel(sql_type = Nullable<Array<Nullable<Text>>>)]
    changed_cols: Option<Vec<Option<String>>>,
}

fn latest_audit_row(conn: &mut PgConnection, pk: &str, op: &str) -> AuditRow {
    diesel::sql_query(
        "SELECT before_jsonb, after_jsonb, changed_cols FROM audit_log \
         WHERE table_name = 'users' AND pk_text = $1 AND op = $2 \
         ORDER BY occurred_at DESC LIMIT 1",
    )
    .bind::<Text, _>(pk)
    .bind::<Text, _>(op)
    .get_result(conn)
    .expect("audit row exists")
}

fn changed(cols: &Option<Vec<Option<String>>>, name: &str) -> bool {
    cols.as_ref()
        .map(|c| c.iter().flatten().any(|k| k == name))
        .unwrap_or(false)
}

#[test]
fn audit_trigger_redacts_pii_and_credentials() {
    let db = TestDb::new();
    let mut conn = db.conn();

    let user = insert_user(&mut conn, "Redaction Probe");
    let pk = user.uuid.to_string();

    // --- INSERT row ---
    let ins = latest_audit_row(&mut conn, &pk, "I");
    let after = ins.after_jsonb.expect("insert after_jsonb present");

    assert!(
        after.get("name").is_none(),
        "PII column `name` value must not land in the audit diff"
    );
    assert!(
        after.get("mfa_secret").is_none(),
        "credential column `mfa_secret` value must not land in the audit diff"
    );
    assert_eq!(
        after.get("name_changed").and_then(|v| v.as_bool()),
        Some(true),
        "name_changed boolean should be true (value was set on insert)"
    );
    assert_eq!(
        after.get("mfa_secret_changed").and_then(|v| v.as_bool()),
        Some(false),
        "mfa_secret_changed should be false (left null on insert)"
    );
    assert!(
        after.get("role").is_some(),
        "non-redacted columns must still be captured by value"
    );

    // --- UPDATE: change the PII name and set a credential ---
    diesel::update(users::table.find(user.uuid))
        .set((
            users::name.eq("Probe Renamed"),
            users::mfa_secret.eq(Some("ENC::deadbeefcafe")),
        ))
        .execute(&mut conn)
        .expect("update user");

    let upd = latest_audit_row(&mut conn, &pk, "U");
    let after = upd.after_jsonb.expect("update after_jsonb present");
    let before = upd.before_jsonb.expect("update before_jsonb present");

    // changed_cols still NAMES the redacted columns (D2: the diff
    // records that the PII/credential column changed).
    assert!(
        changed(&upd.changed_cols, "name"),
        "changed_cols must still name the redacted `name` column"
    );
    assert!(
        changed(&upd.changed_cols, "mfa_secret"),
        "changed_cols must still name the redacted `mfa_secret` column"
    );

    // ... but neither value appears on either side of the diff.
    for side in [&before, &after] {
        assert!(side.get("name").is_none(), "name value leaked into diff");
        assert!(
            side.get("mfa_secret").is_none(),
            "mfa_secret value leaked into diff"
        );
    }

    assert_eq!(
        after.get("name_changed").and_then(|v| v.as_bool()),
        Some(true),
        "name_changed should report the rename"
    );
    assert_eq!(
        after.get("mfa_secret_changed").and_then(|v| v.as_bool()),
        Some(true),
        "mfa_secret_changed should report the credential being set"
    );
}
