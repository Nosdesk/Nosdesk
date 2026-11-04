//! A webhook's signing secret and custom header values never reach the audit
//! log, which audit readers (including the platform audit reviewer and
//! `audit:read` tokens) can export. The log records only that they changed.

#![allow(clippy::expect_used)]

use diesel::prelude::*;
use serde_json::{json, Value};

use backend::models::WebhookUpdate;
use backend::repository::webhooks::{create_webhook, delete_webhook_by_uuid, update_webhook};
use backend::schema::audit_log;
use backend::sync::session::run_in_workspace;

use crate::common;

#[test]
fn the_audit_log_records_no_webhook_secret_or_header_value() {
    let db = common::TestDb::new();
    let pool = db.pool_with_size(2);
    let rows: Vec<(Option<Value>, Option<Value>)> =
        run_in_workspace(&pool, "test:webhook_audit", 1, |c| {
            let hook = create_webhook(
                c,
                "Sink".into(),
                "https://sink.invalid/hook".into(),
                "whsec_original".into(),
                vec!["ticket.created".into()],
                Some(json!({ "Authorization": "Bearer header-secret" })),
                None,
            )?;
            update_webhook(
                c,
                hook.id,
                WebhookUpdate {
                    name: None,
                    url: None,
                    secret: Some("whsec_rotated".into()),
                    events: None,
                    enabled: None,
                    headers: Some(json!({ "Authorization": "Bearer rotated-header" })),
                    last_triggered_at: None,
                    failure_count: None,
                    disabled_reason: None,
                },
            )?;
            delete_webhook_by_uuid(c, hook.uuid)?;
            audit_log::table
                .filter(audit_log::table_name.eq("webhooks"))
                .select((audit_log::before_jsonb, audit_log::after_jsonb))
                .load(c)
        })
        .expect("webhook writes");

    assert_eq!(rows.len(), 3, "insert, update and delete are each audited");
    let all = serde_json::to_string(&rows).expect("json");
    for secret in ["whsec_", "header-secret", "rotated-header"] {
        assert!(!all.contains(secret), "{secret} in the audit log: {all}");
    }
    let (_, created) = &rows[0];
    assert_eq!(
        created.as_ref().expect("after")["secret_changed"],
        json!(true)
    );
}
